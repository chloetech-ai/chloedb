"""G1: knowledge as rows, learned the way the model learns (a GPU run).

Teach a frozen model new text. Is a ChloeDB table of rows plugged into one
layer as good a learner as fine-tuning? Does it leave the rest of the model
alone? Can what it learned be deleted, one document at a time?

Learn set: the 16 eval docs used everywhere before, minus each doc's first
1,280 tokens. Tokens 0..640 are the held-out window, where perplexity is
measured as before (8 strides of 64 tokens after 128 of context), and tokens
704..1,216 are a validation window, used for early stopping and for picking
the learning rate. The rest of each doc is trained on.

Measured for each arm after training:
  held_out  ppl on the 16 docs' eval windows: the same docs, text never trained
  seen      ppl on a trained window of each doc (how much it memorised)
  domain    ppl on 8 other ChloeDB docs, not trained (does it carry over)
  general   ppl on WikiText-2's test text (does it forget what it knew)

Arms (same steps and batch, the best validation step kept, two learning rates):
  base       the model as it is
  lora       rank 16 on every linear layer of every block (about the FFN's size)
  ffn        layer L's feed-forward block fine-tuned (9.4 million weights on 0.6B)
  rows_small 4,096 rows at layer L (about the FFN's size)
  rows       NROWS rows at layer L (65,536 by default; 32 used per token)
A row is a key and a value, each the layer's width. The rows' output, the
softmax-weighted sum of the values of the 32 keys nearest the layer's input,
is added to the FFN's output. Keys start at the layer's own inputs on the
learn set (each row remembers which doc its key came from), values at zero,
so before training the model is exactly the base model. Only the rows train.

Then, for the bigger rows arm: every row deleted (the base model, exactly);
one doc's rows deleted, for each doc, to see that doc go back while the others
keep what they learned; and the rows served from ChloeDB (HNSW) inside the
forward pass, checked against torch, before and after a SQL DELETE."""
import json, os, sys, time, math, random, glob
import numpy as np, torch, torch.nn.functional as F, chloedb
from gpu import load, DEVICE

S = os.environ["S"]; OUT = sys.argv[1]; SMOKE = os.environ.get("SMOKE") == "1"
torch.manual_seed(0); torch.set_num_threads(int(os.environ.get("THREADS", 4)))
tok, model = load(S)
for p in model.parameters(): p.requires_grad_(False)
NL, D = model.config.num_hidden_layers, model.config.hidden_size
L = int(os.environ.get("LAYER", round(NL * 16 / 28)))
NROWS = int(os.environ.get("NROWS", 65536)); NSMALL = int(os.environ.get("NSMALL", 4096)); TOPK = 32
STEPS = int(os.environ.get("STEPS", 2000)); BATCH = int(os.environ.get("BATCH", 8)); WIN = int(os.environ.get("WIN", 256))
EVERY = int(os.environ.get("EVERY", 200)); AMP = DEVICE == "cuda"
ARMS = os.environ.get("ARMS", "lora,ffn,rows_small,rows").split(",")
LRS = {"lora": [1e-4, 3e-4], "ffn": [1e-5, 5e-5], "rows_small": [1e-3, 3e-3], "rows": [1e-3, 3e-3]}
if os.environ.get("LRFROM"):  # reuse the learning rates another run picked (e.g. 0.6B's for 4B)
    LRS = {}
    for fn in os.environ["LRFROM"].split(","):          # later files win
        prev = json.load(open(fn))["arms"]
        LRS.update({a: [prev[a]["lr"]] for a in prev if isinstance(prev[a], dict) and "lr" in prev[a]})
LRS["rows_multi"] = LRS.get("rows_multi", LRS.get("rows", [3e-3]))
for a in list(LRS) + ["rows_multi"]:                 # LR_rows=0.01,0.03 overrides one arm's learning rates
    if os.environ.get(f"LR_{a}"): LRS[a] = [float(x) for x in os.environ[f"LR_{a}"].split(",")]
ROW_LAYERS = [int(x) for x in os.environ.get("ROW_LAYERS", ",".join(str(round(NL * f)) for f in (0.25, 0.45, 0.6, 0.8))).split(",")]
DELETE_ARM = os.environ.get("DELETE_ARM", "rows")
DOCS = os.environ.get("DOCS", "/home/user/chloedb/docs")
EVAL = ["python", "netops", "javascript", "connectors", "security", "earth", "opensearch", "graphs",
        "arxiv", "hn", "apps", "postgres", "troubleshooting", "crawler", "sqlite", "frameworks"]
if SMOKE: EVAL = EVAL[-2:]
CTX, STRIDE, NSTR = 128, 64, 8
HELD, VAL, SEEN, TRAIN0 = 0, 704 - CTX, 1280, 1280

def toks(path): return tok(open(path).read(), add_special_tokens=False)["input_ids"]
DOC = {e: toks(f"{DOCS}/chloedb/{e}.md") for e in EVAL}
def segs(docs, off, nstr=NSTR):
    out = []
    for name, ids in docs.items():
        for k in range(nstr):
            s = off + CTX + k * STRIDE
            if s + STRIDE <= len(ids): out.append((name, ids[s - CTX:s], ids[s:s + STRIDE]))
    return out
SETS = {"held_out": segs(DOC, HELD), "seen": segs(DOC, SEEN)}
VALS = segs(DOC, VAL)
others = [f for f in sorted(glob.glob(f"{DOCS}/chloedb/*.md")) if os.path.basename(f)[:-3] not in EVAL + ["index", "README"]]
SETS["domain"] = segs({os.path.basename(f)[:-3]: toks(f) for f in others[:2 if SMOKE else 8]}, 0)
wt = f"{S}/wikitext2_test.txt"
if os.path.exists(wt):
    w = tok(open(wt).read()[:400000], add_special_tokens=False)["input_ids"]
    SETS["general"] = segs({f"wiki{i}": w[i * 4000:i * 4000 + 1000] for i in range(2 if SMOKE else 16)}, 0)
TRAIN = {e: ids[TRAIN0:] for e, ids in DOC.items()}
WEIGHTS = [len(TRAIN[e]) for e in EVAL]
res = {"model": os.environ.get("MODEL", "qwen3-0.6b"), "device": DEVICE, "layer": L, "layers": NL, "hidden": D,
       "steps": STEPS, "batch": BATCH, "window": WIN, "train_tokens": sum(WEIGHTS),
       "sets": {k: len(v) for k, v in SETS.items()}, "arms": {}}
if DEVICE == "cuda": res["gpu"] = torch.cuda.get_device_name()
print(res, flush=True)

@torch.no_grad()
def eval_set(sg, bs=16):
    per = {}
    for i in range(0, len(sg), bs):
        b = sg[i:i + bs]; x = torch.tensor([c + t for _, c, t in b], device=DEVICE)
        lp = torch.log_softmax(model(x).logits[:, CTX - 1:-1].float(), -1)
        nll = -lp.gather(-1, x[:, CTX:].unsqueeze(-1)).squeeze(-1).mean(-1)
        for (n, _, _), v in zip(b, nll.tolist()): per.setdefault(n, []).append(v)
    allv = [v for vs in per.values() for v in vs]
    return math.exp(float(np.mean(allv))), {n: math.exp(float(np.mean(v))) for n, v in per.items()}
def eval_all():
    out = {}
    for k, sg in SETS.items(): out[k], out[k + "_per_doc"] = eval_set(sg)
    return out

# ---- the plug-ins -------------------------------------------------------------------------
class LoRA:
    def __init__(self, r=16, alpha=32):
        self.params, self.hooks, self.on = [], [], True
        for blk in model.model.layers:
            for m in [blk.self_attn.q_proj, blk.self_attn.k_proj, blk.self_attn.v_proj, blk.self_attn.o_proj,
                      blk.mlp.gate_proj, blk.mlp.up_proj, blk.mlp.down_proj]:
                A = (torch.randn(r, m.in_features, device=DEVICE) / math.sqrt(m.in_features)).requires_grad_()
                B = torch.zeros(m.out_features, r, device=DEVICE, requires_grad=True)
                self.params += [A, B]
                self.hooks.append(m.register_forward_hook(
                    lambda mod, a, o, A=A, B=B: o + (a[0] @ A.t().to(a[0].dtype)) @ B.t().to(a[0].dtype) * (alpha / r) if self.on else o))
    def remove(self):
        for h in self.hooks: h.remove()

class Rows(torch.nn.Module):
    """y = FFN(x) + sum over the 32 nearest rows of softmax(scale * cos(x, key)) * value."""
    def __init__(self, K, owner):
        super().__init__()
        self.K = torch.nn.Parameter(K.clone().float()); self.V = torch.nn.Parameter(torch.zeros_like(self.K))
        self.logscale = torch.nn.Parameter(torch.tensor(math.log(20.0)))
        self.owner = owner; self.mask = None; self.db = None; self.on = True
    def forward(self, x):
        shp = x.shape; q = x.reshape(-1, shp[-1]).float(); qn = q / q.norm(dim=-1, keepdim=True)
        V = self.V
        if self.db is not None:                           # served by ChloeDB: one vector query per position
            idx, sims = [], []
            for row in qn.detach().cpu().numpy().astype(np.float32):
                t = time.perf_counter()
                r = self.db.query(f"SELECT id, score() FROM rows ORDER BY similarity(key, :v) DESC LIMIT {TOPK}", v=row).rows
                self.lookup_ms.append((time.perf_counter() - t) * 1000)
                idx.append([a for a, _ in r] + [0] * (TOPK - len(r))); sims.append([b for _, b in r] + [-1e9] * (TOPK - len(r)))
            idx = torch.tensor(idx, device=x.device); s = torch.tensor(sims, dtype=torch.float32, device=x.device)
        else:
            Kn = self.K / self.K.norm(dim=-1, keepdim=True); sc = qn @ Kn.t()
            if self.mask is not None: sc = sc.masked_fill(self.mask, -1e9)
            s, idx = sc.topk(TOPK, dim=-1)
        w = torch.softmax(self.logscale.exp() * s, -1)
        return (w.unsqueeze(-1) * V[idx]).sum(1).reshape(shp).to(x.dtype)

def mlp_inputs(n, seed, layer=None):
    """n of a layer's (L by default) FFN inputs on the learn set, with the doc each came from."""
    cap = []; h = model.model.layers[L if layer is None else layer].mlp.register_forward_hook(lambda m, a, o: cap.append(a[0].detach().float()))
    xs, own = [], []
    with torch.no_grad():
        for e in EVAL:
            r = TRAIN[e]
            for w0 in range(0, max(1, len(r) - 32), 512):
                cap.clear(); model(torch.tensor([r[w0:w0 + 512]], device=DEVICE)); xs.append(cap[0][0]); own += [e] * cap[0].shape[1]
    h.remove(); X = torch.cat(xs); g = torch.Generator(device="cpu").manual_seed(seed)
    pick = torch.randperm(len(X), generator=g, device="cpu")[:n]
    return X[pick.to(X.device)], [own[i] for i in pick.tolist()]

# ---- training -----------------------------------------------------------------------------
def train(params, lr, steps=None):
    STEPS = steps or globals()["STEPS"]
    opt = torch.optim.AdamW(params, lr=lr, weight_decay=0.0)
    sched = torch.optim.lr_scheduler.LambdaLR(opt, lambda s: min(1.0, (s + 1) / 50) * 0.5 * (1 + math.cos(math.pi * min(s, STEPS) / STEPS)))
    rng = random.Random(1); best = (float("inf"), None, 0); hist = []; t0 = time.time(); ls = []
    for step in range(1, STEPS + 1):
        xs = []
        for e in rng.choices(EVAL, weights=WEIGHTS, k=BATCH):
            r = TRAIN[e]; i = rng.randrange(0, max(1, len(r) - WIN)); xs.append(r[i:i + WIN])
        x = torch.tensor(xs, device=DEVICE)
        with torch.autocast("cuda", dtype=torch.bfloat16, enabled=AMP):
            lg = model(x).logits
        loss = F.cross_entropy(lg[:, :-1].float().reshape(-1, lg.shape[-1]), x[:, 1:].reshape(-1))
        opt.zero_grad(set_to_none=True); loss.backward(); torch.nn.utils.clip_grad_norm_(params, 1.0); opt.step(); sched.step()
        ls.append(loss.item()); del lg, loss
        if step % EVERY == 0 or step == STEPS:
            v, _ = eval_set(VALS); hist.append({"step": step, "train_loss": float(np.mean(ls[-EVERY:])), "val_ppl": v, "secs": round(time.time() - t0)})
            print(" ", hist[-1], flush=True)
            if v < best[0]: best = (v, [p.detach().clone() for p in params], step)
    with torch.no_grad():
        for p, b in zip(params, best[1]): p.copy_(b)
    return {"lr": lr, "best_step": best[2], "val_ppl": best[0], "secs": round(time.time() - t0), "history": hist}

base = eval_all(); base_val, _ = eval_set(VALS)
res["arms"]["base"] = {**base, "val_ppl": base_val}
print("base", {k: v for k, v in base.items() if not k.endswith("per_doc")}, flush=True)
json.dump(res, open(OUT, "w"), indent=1)

ffn = model.model.layers[L].mlp
ffn_orig = {k: v.detach().clone() for k, v in ffn.state_dict().items()}
rows_keep = None
for arm in ARMS:
    if arm not in LRS: continue
    best = None
    for lr in LRS[arm]:
        print(arm, "lr", lr, flush=True)
        if arm == "lora":
            lo = LoRA(); params = lo.params; teardown = lo.remove
        elif arm == "ffn":
            params = list(ffn.parameters())
            for p in params: p.requires_grad_(True)
            def teardown():
                for p in ffn.parameters(): p.requires_grad_(False)
                ffn.load_state_dict(ffn_orig)
        else:
            lays = ROW_LAYERS if arm == "rows_multi" else [L]
            n = {"rows": NROWS, "rows_small": NSMALL, "rows_multi": NROWS // len(lays)}[arm]
            mods, hooks = [], []
            for l in lays:
                K, own = mlp_inputs(n, seed=2 + l, layer=l); m = Rows(K, own); m.layer = l; mods.append(m)
                hooks.append(model.model.layers[l].mlp.register_forward_hook(lambda mm, a, o, m=m: o + m(a[0]) if m.on else o))
            params = [p for m in mods for p in m.parameters()]
            def teardown(hooks=hooks):
                for h in hooks: h.remove()
        r = train(params, lr, int(os.environ.get(f"STEPS_{arm}", STEPS))); r.update(eval_all()); r["trainable"] = sum(p.numel() for p in params)
        if arm.startswith("rows"): r["active_per_token"] = TOPK * 2 * D * len(mods); r["row_layers"] = [m.layer for m in mods]
        print(arm, lr, {k: v for k, v in r.items() if k in ("held_out", "seen", "domain", "general", "best_step", "val_ppl")}, flush=True)
        res["arms"].setdefault(arm + "_tried", []).append({k: v for k, v in r.items() if not k.endswith("per_doc") and k != "history"})
        if best is None or r["val_ppl"] < best["val_ppl"]:
            best = r
            if arm == DELETE_ARM: rows_keep = [{"K": m.K.detach().clone(), "V": m.V.detach().clone(), "ls": m.logscale.detach().clone(), "owner": m.owner, "layer": m.layer} for m in mods]
        teardown(); del params
        if DEVICE == "cuda": torch.cuda.empty_cache()
    res["arms"][arm] = best
    json.dump(res, open(OUT, "w"), indent=1)
assert all(torch.equal(v, ffn.state_dict()[k]) for k, v in ffn_orig.items())

# ---- deleting rows ------------------------------------------------------------------------
if rows_keep is not None:
    class Group:                                          # the kept rows of every layer, switched and masked together
        def __init__(self, ms): self.ms = ms
        def __setattr__(self, k, v):
            if k == "ms": object.__setattr__(self, k, v)
            elif k == "mask":
                for m in self.ms: m.mask = None if v is None else v[m.layer]
            else:
                for m in self.ms: setattr(m, k, v)
    mods = []
    for kp in rows_keep:
        m = Rows(kp["K"], kp["owner"]); m.layer = kp["layer"]
        with torch.no_grad(): m.V.copy_(kp["V"]); m.logscale.copy_(kp["ls"])
        for p in m.parameters(): p.requires_grad_(False)
        model.model.layers[m.layer].mlp.register_forward_hook(lambda mm, a, o, m=m: o + m(a[0]) if m.on else o); mods.append(m)
    rows = mods[0]; grp = Group(mods)
    res["delete"] = {"arm": DELETE_ARM, "layers": [m.layer for m in mods]}
    full = eval_all(); res["delete"]["rows_on"] = {k: full[k] for k in SETS}
    grp.on = False; off = eval_all(); grp.on = True
    res["delete"]["all_rows_deleted"] = {k: off[k] for k in SETS}
    res["delete"]["all_rows_deleted_equals_base"] = all(abs(off[k] - base[k]) < 1e-6 for k in SETS)
    owners = {m.layer: np.array(m.owner) for m in mods}; owner = owners[rows.layer]; per = {}
    for d in EVAL:
        grp.mask = {l: torch.tensor(o == d, device=DEVICE) for l, o in owners.items()}
        ho, ho_doc = eval_set(SETS["held_out"]); se, se_doc = eval_set(SETS["seen"])
        others_ho = float(np.mean([ho_doc[e] for e in EVAL if e != d])) if len(EVAL) > 1 else None
        others_se = float(np.mean([se_doc[e] for e in EVAL if e != d])) if len(EVAL) > 1 else None
        per[d] = {"rows": int(sum((o == d).sum() for o in owners.values())),
                  "seen_doc": {"base": base["seen_per_doc"][d], "rows": full["seen_per_doc"][d], "deleted": se_doc[d]},
                  "held_doc": {"base": base["held_out_per_doc"][d], "rows": full["held_out_per_doc"][d], "deleted": ho_doc[d]},
                  "others_seen_mean": {"rows": float(np.mean([full["seen_per_doc"][e] for e in EVAL if e != d])) if len(EVAL) > 1 else None, "deleted": others_se},
                  "others_held_mean": {"rows": float(np.mean([full["held_out_per_doc"][e] for e in EVAL if e != d])) if len(EVAL) > 1 else None, "deleted": others_ho}}
        print("delete", d, per[d], flush=True)
    grp.mask = None; res["delete"]["per_doc"] = per
    json.dump(res, open(OUT, "w"), indent=1)

    # ---- served by ChloeDB ------------------------------------------------------------------
    path = f"{S}/g1rows.chloedb"; os.system(f"rm -rf {path}*"); t0 = time.time()
    db = chloedb.open(path, schema={"table": "rows", "key": "id", "fields": [
        {"name": "id", "type": "int"}, {"name": "doc", "type": "keyword"}, {"name": "key", "type": "vector", "dim": D}]}, sync=False)
    Kn = (rows.K / rows.K.norm(dim=-1, keepdim=True)).detach().cpu().numpy().astype(np.float32)
    for a in range(0, len(Kn), 5000):
        db.insert([{"id": i, "doc": rows.owner[i], "key": Kn[i]} for i in range(a, min(len(Kn), a + 5000))])
    db.flush(); build = time.time() - t0
    one = [s for i, s in enumerate(SETS["held_out"]) if i % NSTR == 0]   # one stride per doc
    d0 = EVAL[1] if len(EVAL) > 1 else EVAL[0]
    dsegs = [s for s in SETS["seen"] if s[0] == d0][:2]
    _, t_one = eval_set(one, bs=1); _, t_d = eval_set(dsegs, bs=1)
    rows.db = db; rows.lookup_ms = []
    _, c_one = eval_set(one, bs=1); _, c_d = eval_set(dsegs, bs=1)
    db.sql(f"DELETE FROM rows WHERE doc = '{d0}'"); left = db.query("SELECT COUNT(*) FROM rows").rows[0][0]
    _, c_d_del = eval_set(dsegs, bs=1); rows.db = None
    rows.mask = torch.tensor(owner == d0, device=DEVICE); _, t_d_del = eval_set(dsegs, bs=1); rows.mask = None
    res["served_by_chloedb"] = {"served_layer": rows.layer, "rows": len(Kn), "build_secs": round(build, 1), "index": "hnsw (default)",
        "held_one_stride_per_doc": {"torch": t_one, "chloedb": c_one},
        "deleted_doc": d0, "rows_after_delete": left,
        "deleted_doc_seen": {"torch": t_d, "chloedb": c_d, "torch_masked": t_d_del, "chloedb_after_sql_delete": c_d_del},
        "ms_per_lookup_p50": float(np.median(rows.lookup_ms)), "ms_per_lookup_p99": float(np.percentile(rows.lookup_ms, 99))}
    print(res["served_by_chloedb"], flush=True)
json.dump(res, open(OUT, "w"), indent=1); print("DONE", flush=True)
