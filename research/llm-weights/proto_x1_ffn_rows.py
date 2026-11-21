"""X1, the taken-out piece: one feed-forward block of Qwen3-0.6B (layer L) is
removed and replaced by rows of the same dimension, served by ChloeDB.

The block maps x (1024 numbers: the layer's normalised hidden state) to y
(1024 numbers added back to the residual stream). Its replacement is a
key-value memory: each row is a key (1024, unit length) and a value (1024);
the block's output is the softmax-weighted sum of the values of the top-k
keys nearest x. So the piece "speaks the model's dimension": it takes the
same input and returns the same kind of output, and every part of it is a
row that can be inserted, updated, searched and deleted.

Arms (perplexity on the same 8,192 tokens as before):
  original      the real block
  removed       the block's output set to zero
  copy N        N rows sampled from the block's own (x, y) pairs, no training
  distilled N   N rows trained to imitate the block (keys and values), served
                by ChloeDB's vector search inside the forward pass
  + inserted    new rows for the eval docs' subject, trained on the rest of
                those docs with the model's own loss (base rows frozen), then
                DELETE of the new rows"""
import json, os, sys, time, random, glob
import numpy as np, torch, chloedb
from common_hf import *

OUT = sys.argv[1]; L = int(os.environ.get("LAYER", 14)); TOPK = 32; D = model.config.hidden_size
NB = int(os.environ.get("NBASE", 16384)); PAIRS = int(os.environ.get("PAIRS", 120000))
layer = model.model.layers[L]; orig_mlp = layer.mlp
res = {"layer": L, "topk": TOPK, "rows_base": NB}

# 1. The block's own (x, y) pairs, from ChloeDB docs that are not eval docs.
cap = []
hk = orig_mlp.register_forward_hook(lambda m, a, o: cap.append((a[0][0].detach().half(), o[0].detach().half())))
files = [f for f in sorted(glob.glob(f"{DOCS}/chloedb/*.md") + glob.glob(f"{DOCS}/chloectx/*.md") + glob.glob(f"{DOCS}/guide/*.md"))
         if not any(f.endswith(f"chloedb/{e}.md") for e in EVAL)]
t0 = time.time(); n = 0
with torch.no_grad():
    for f in files:
        ids = toks(f)
        for w in range(0, len(ids) - 32, 256):
            model(torch.tensor([ids[w:w + 256]])); n += len(ids[w:w + 256])
            if n >= PAIRS: break
        if n >= PAIRS: break
hk.remove()
X = torch.cat([a for a, _ in cap]).float(); Y = torch.cat([b for _, b in cap]).float(); cap.clear()
res["pairs"] = {"n": len(X), "secs": round(time.time() - t0)}
print("pairs", res["pairs"], flush=True)

class RowMLP(torch.nn.Module):
    """y = sum over the top-k rows of softmax(scale * cos(x, key)) * value."""
    def __init__(self, K, V, scale=20.0):
        super().__init__()
        self.K = torch.nn.Parameter(K.clone()); self.V = torch.nn.Parameter(V.clone())
        self.logscale = torch.nn.Parameter(torch.tensor(float(np.log(scale))))
        self.extra_K = None; self.extra_V = None; self.db = None
    def keys(self):
        K = self.K if self.extra_K is None else torch.cat([self.K, self.extra_K]); return K / K.norm(dim=-1, keepdim=True)
    def values(self): return self.V if self.extra_V is None else torch.cat([self.V, self.extra_V])
    def forward(self, x):
        shp = x.shape; q = x.reshape(-1, shp[-1]); qn = q / q.norm(dim=-1, keepdim=True)
        if self.db is not None:                          # served by ChloeDB: one vector query per position
            idx, sims = [], []
            for row in qn.detach().numpy().astype(np.float32):
                r = self.db.query(f"SELECT id, score() FROM rows ORDER BY similarity(key, :v) DESC LIMIT {TOPK}", v=row).rows
                idx.append([a for a, _ in r]); sims.append([b for _, b in r])
            idx = torch.tensor(idx); s = torch.tensor(sims, dtype=torch.float32)
            V = self.db_values
        else:
            s, idx = (qn @ self.keys().t()).topk(TOPK, dim=-1); V = self.values()
        w = torch.softmax(self.logscale.exp() * s, -1)
        return (w.unsqueeze(-1) * V[idx]).sum(1).reshape(shp)

class Zero(torch.nn.Module):
    def forward(self, x): return torch.zeros_like(x)

def eval_ppl():
    xs = []
    with torch.no_grad():
        for e, s, ctx, tgt in segments(): xs.append(float(nll(None, ctx, tgt)))
    return ppl(xs), xs

ppls = {}
ppls["original"], base_x = eval_ppl()
layer.mlp = Zero(); ppls["removed"], _ = eval_ppl()
print(ppls, flush=True)

g = torch.Generator().manual_seed(0)
for N in (NB, 4 * NB):
    pick = torch.randperm(len(X), generator=g)[:N]
    layer.mlp = RowMLP(X[pick], Y[pick]); ppls[f"copy_{N}"], _ = eval_ppl(); print(ppls, flush=True)

# 2. Distil: train NB rows (keys, values, scale) to imitate the block on held-out pairs.
pick = torch.randperm(len(X), generator=g)
mem = RowMLP(X[pick[:NB]], Y[pick[:NB]]); tr = pick[NB:]
opt = torch.optim.Adam(mem.parameters(), lr=1e-3); t0 = time.time(); ls = []
for step in range(int(os.environ.get("DSTEPS", 1500))):
    b = tr[torch.randint(len(tr), (512,), generator=g)]
    yh = mem(X[b]); loss = ((yh - Y[b]) ** 2).sum(-1).mean() / (Y[b] ** 2).sum(-1).mean()
    opt.zero_grad(); loss.backward(); opt.step(); ls.append(loss.item())
    if step % 250 == 0: print("distil", step, round(float(np.mean(ls[-50:])), 4), round(time.time() - t0), flush=True)
res["distil"] = {"steps": len(ls), "secs": round(time.time() - t0), "rel_mse_last": float(np.mean(ls[-100:]))}
for p in mem.parameters(): p.requires_grad_(False)
layer.mlp = mem; ppls[f"distilled_{NB}"], dist_x = eval_ppl(); print(ppls, flush=True)

# 3. The same rows in ChloeDB, searched inside the forward pass.
path = f"{S}/ffnrows.chloedb"; os.system(f"rm -rf {path}*")
db = chloedb.open(path, schema={"table": "rows", "key": "id", "fields": [
    {"name": "id", "type": "int"}, {"name": "source", "type": "keyword"}, {"name": "key", "type": "vector", "dim": D}]}, sync=False)
Kn = mem.keys().numpy().astype(np.float32)
db.insert([{"id": i, "source": "distilled", "key": Kn[i]} for i in range(NB)])
mem.db = db; mem.db_values = mem.V.detach()
t0 = time.time(); xs = []
with torch.no_grad():
    for i, (e, s, ctx, tgt) in enumerate(segments()):
        if i % 8: continue                                # every 8th stride: 16 strides x 192 lookups
        xs.append(float(nll(None, ctx, tgt)))
secs = time.time() - t0
res["served_by_chloedb"] = {"strides": len(xs), "ppl": ppl(xs), "same_strides_torch_ppl": ppl(dist_x[::8]),
                            "ms_per_lookup": secs * 1000 / (len(xs) * 192)}
mem.db = None; print(res["served_by_chloedb"], flush=True)

# 4. Insert knowledge: new rows keyed on the eval docs' other text, trained with the LM loss; then delete them.
M = int(os.environ.get("NNEW", 2048)); rng = random.Random(4); rest = {}
for e in EVAL: rest[e] = toks(f"{DOCS}/chloedb/{e}.md")[END:END + 2048]
newx = []
hk = layer.register_forward_pre_hook(lambda m, a: None)
hk.remove()
cap2 = []
h2 = mem.register_forward_hook(lambda m, a, o: cap2.append(a[0][0].detach()))
with torch.no_grad():
    for e in EVAL:
        r = rest[e]
        for w in range(0, max(1, len(r) - 32), 256): model(torch.tensor([r[w:w + 256]]))
h2.remove()
allx = torch.cat(cap2); sel = torch.randperm(len(allx), generator=g)[:M]
with torch.no_grad(): v0 = mem(allx[sel])
mem.extra_K = allx[sel].clone(); mem.extra_V = torch.nn.Parameter(v0.clone())
opt = torch.optim.Adam([mem.extra_V], lr=float(os.environ.get("ILR", 3e-3))); t0 = time.time(); ls = []
for step in range(int(os.environ.get("ISTEPS", 150))):
    e = rng.choice(EVAL); r = rest[e]
    if len(r) < 200: continue
    i = rng.randrange(0, len(r) - 192); w = r[i:i + 192]
    loss = nll(None, w[:32], w[32:], grad=True); opt.zero_grad(); loss.backward(); opt.step(); ls.append(loss.item())
    if step % 25 == 0: print("insert", step, round(float(np.mean(ls[-25:])), 3), round(time.time() - t0), flush=True)
mem.extra_V.requires_grad_(False)
ppls[f"distilled_plus_{M}_inserted"], _ = eval_ppl()
res["insert"] = {"rows": M, "steps": len(ls), "secs": round(time.time() - t0), "loss_first": float(np.mean(ls[:10])), "loss_last": float(np.mean(ls[-10:]))}
db.insert([{"id": NB + i, "source": "inserted", "key": (mem.extra_K[i] / mem.extra_K[i].norm()).numpy().astype(np.float32)} for i in range(M)])
before = db.query("SELECT COUNT(*) FROM rows").rows[0][0]
db.sql("DELETE FROM rows WHERE source = 'inserted'"); after = db.query("SELECT COUNT(*) FROM rows").rows[0][0]
mem.extra_K = None; mem.extra_V = None
ppls["after_delete"], _ = eval_ppl()
res["delete"] = {"rows_before": before, "rows_after": after}
res["ppl"] = ppls
res["row_bytes"] = {"key_fp32": D * 4, "value_fp32": D * 4, "base_rows_mb": round(NB * D * 8 / 1e6, 1),
                    "original_block_params": sum(p.numel() for p in orig_mlp.parameters())}
json.dump(res, open(OUT, "w"), indent=1); print(json.dumps(res, indent=1))
