"""X2: a better router for adapter rows (W5 picked the right adapter 20% of the time).
The 16 adapter rows from W5/W6 are reused as they are ($S/adapters.chloedb). Each
stride's context picks one adapter, by:
  vector        the model's mean hidden state, as in W5 (raw dot product)
  centered      the same vectors minus the mean of all keys, unit length (cosine)
  bm25          BM25 of the context's words over each adapter's source text
  rrf           rrf(bm25, centered): both, in one ChloeDB query
  learned       a linear projection (1024 -> 256) trained contrastively on OTHER docs
                (windows of a doc against that doc's key), applied to these rows
                as they are: a router trained once, rows added later
Reported: share of strides routed to their own doc's adapter, and perplexity with
the picked adapter, against none and the oracle (own adapter)."""
import json, sys, time, random, os, glob
import numpy as np, torch, chloedb
from common_hf import *

OUT = sys.argv[1]; D = model.config.hidden_size; P = 256
add_lora(8)
db = chloedb.open(f"{S}/adapters.chloedb", sync=False)
rows = db.query("SELECT id, key FROM adapters LIMIT 100").rows
ids = [r[0] for r in rows]; K = torch.tensor(np.array([r[1] for r in rows], dtype=np.float32)); mu = K.mean(0)
def unit(v): return v / v.norm(dim=-1, keepdim=True)

# 1. A learned key projection, trained on docs that are not eval docs.
others = [f for f in sorted(glob.glob(f"{DOCS}/chloedb/*.md") + glob.glob(f"{DOCS}/chloectx/*.md") + glob.glob(f"{DOCS}/guide/*.md"))
          if not any(f.endswith(f"chloedb/{e}.md") for e in EVAL)]
t0 = time.time(); okeys, oq = [], []
for f in others:
    s = toks(f)
    if len(s) < 1200: continue
    okeys.append(mean_hidden(s[640:1152]))                              # the doc's key, as W5 makes keys
    oq.append([mean_hidden(s[w - 128:w]) for w in range(128, 640, 64)])  # contexts from another part of it
OK = torch.stack(okeys); OQ = [torch.stack(q) for q in oq]; omu = OK.mean(0)
proj = torch.nn.Linear(D, P, bias=False); opt = torch.optim.Adam(proj.parameters(), lr=1e-3); g = torch.Generator().manual_seed(0)
for step in range(1500):
    di = torch.randint(len(OQ), (64,), generator=g); qi = torch.randint(8, (64,), generator=g)
    q = torch.stack([OQ[d][i] for d, i in zip(di.tolist(), qi.tolist())])
    zq = unit(proj(q - omu)); zk = unit(proj(OK - omu))
    loss = torch.nn.functional.cross_entropy(zq @ zk.t() / 0.05, di)
    opt.zero_grad(); loss.backward(); opt.step()
train = {"docs": len(OK), "secs": round(time.time() - t0), "loss_last": float(loss)}
print("router trained", train, flush=True)

# 2. The same 16 rows, with the extra keys, in a routing table of their own (one ChloeDB query routes).
path = f"{S}/router.chloedb"; os.system(f"rm -rf {path}*")
rt = chloedb.open(path, schema={"table": "routes", "key": "id", "fields": [
    {"name": "id", "type": "keyword"}, {"name": "body", "type": "text"},
    {"name": "kc", "type": "vector", "dim": D}, {"name": "kl", "type": "vector", "dim": P}]}, sync=False)
with torch.no_grad(): KL = unit(proj(K - mu))
for i, e in enumerate(ids):
    rest = toks(f"{DOCS}/chloedb/{e}.md")[END:END + 2048]
    rt.insert([{"id": e, "body": tok.decode(rest), "kc": unit(K[i] - mu).numpy(), "kl": KL[i].numpy()}])

def pick(kind, ctx, v):
    words = tok.decode(ctx)
    if kind == "vector": sql, p = "SELECT id FROM adapters ORDER BY similarity(key, :v) DESC LIMIT 1", {"v": v.numpy()}; return db.query(sql, **p).rows[0][0]
    if kind == "centered": sql, p = "SELECT id FROM routes ORDER BY similarity(kc, :v) DESC LIMIT 1", {"v": unit(v - mu).numpy()}
    elif kind == "bm25": sql, p = "SELECT id FROM routes ORDER BY bm25(body, :q) DESC LIMIT 1", {"q": words}
    elif kind == "rrf": sql, p = "SELECT id FROM routes ORDER BY rrf(bm25(body, :q), similarity(kc, :v)) DESC LIMIT 1", {"q": words, "v": unit(v - mu).numpy()}
    elif kind == "learned":
        with torch.no_grad(): z = unit(proj(v - mu))
        sql, p = "SELECT id FROM routes ORDER BY similarity(kl, :v) DESC LIMIT 1", {"v": z.numpy()}
    r = rt.query(sql, **p).rows
    return r[0][0] if r else None

loaded = set()
def load(name):
    if name not in loaded:
        unpack_adapter(name, db.query("SELECT weights FROM adapters WHERE id = :i", i=name).rows[0][0]); loaded.add(name)
    return name
KINDS = ["vector", "centered", "bm25", "rrf", "learned"]
nl = {k: [] for k in ["none", "oracle"] + KINDS}; right = {k: 0 for k in KINDS}; ms = {k: [] for k in KINDS}; n = 0
cache = {}
for e, s, ctx, tgt in segments():
    v = mean_hidden(ctx); n += 1
    def run(name):
        if name not in cache.setdefault((e, s), {}):
            set_active([] if name is None else [(load(name), 1.0)]); cache[(e, s)][name] = float(nll(None, ctx, tgt)); set_active([])
        return cache[(e, s)][name]
    nl["none"].append(run(None)); nl["oracle"].append(run(e))
    for k in KINDS:
        t = time.perf_counter(); p = pick(k, ctx, v); ms[k].append((time.perf_counter() - t) * 1000)
        right[k] += p == e; nl[k].append(run(p))
out = {"strides": n, "router_train": train, "proj_dim": P,
       "routed_right": {k: right[k] / n for k in KINDS}, "ppl": {k: ppl(v) for k, v in nl.items()},
       "route_ms_p50": {k: float(np.median(v)) for k, v in ms.items()}}
json.dump(out, open(OUT, "w"), indent=1); print(json.dumps(out, indent=1))
