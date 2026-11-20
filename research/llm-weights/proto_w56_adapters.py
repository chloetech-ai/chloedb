"""W5 and W6: weights assembled per request from rows of ChloeDB.
W5, adapters as rows: each of the 16 eval docs is compiled into a LoRA
(rank 8 on every layer's o_proj and down_proj, ~1.6M parameters, fp16)
trained on the rest of that doc (never the scored tokens), and stored as a
row with a key vector (the model's mean hidden state of the text it learned)
and its tenant and date. Per 64-token stride, the context's vector picks
the adapter by similarity, which is loaded from its row and applied.
W6, a filtered mixture of experts: the same rows are experts; the router is
one ChloeDB query, top 2 by similarity WHERE tenant = :t (and valid_from <=
:as_of), mixed by softmax of their scores. Document-level routing per
stride, not PEER's per-token routing, which needs training at scale."""
import json, sys, time, random, os
import numpy as np, torch, chloedb
from common_hf import *

OUT = sys.argv[1]; STEPS = int(os.environ.get("STEPS", 24)); D = model.config.hidden_size
add_lora(8)
path = f"{S}/adapters.chloedb"; os.system(f"rm -rf {path}*")
db = chloedb.open(path, schema={"table": "adapters", "key": "id", "fields": [
    {"name": "id", "type": "keyword"}, {"name": "tenant", "type": "keyword"}, {"name": "valid_from", "type": "timestamp"},
    {"name": "key", "type": "vector", "dim": D}, {"name": "weights", "type": "text", "indexed": False}]}, sync=False)
TENANT = {e: f"t{i % 4}" for i, e in enumerate(EVAL)}
VALID = {e: ("2026-10-01T00:00:00Z" if i < 8 else "2026-11-15T00:00:00Z") for i, e in enumerate(EVAL)}

# 1. Compile each doc's rest into an adapter row.
rng = random.Random(5); t0 = time.time(); learned = {}
for e in EVAL:
    rest = toks(f"{DOCS}/chloedb/{e}.md")[END:END + 2048]
    if len(rest) < 256: rest = toks(f"{DOCS}/chloedb/{e}.md")[END:]
    params = [p for m in LORAS for p in m.new(e)]
    opt = torch.optim.Adam(params, lr=2e-3); set_active([(e, 1.0)]); ls = []
    for step in range(STEPS):
        i = rng.randrange(0, max(1, len(rest) - 192)); w = rest[i:i + 192]
        loss = nll(None, w[:32], w[32:], grad=True); opt.zero_grad(); loss.backward(); opt.step(); ls.append(loss.item())
    set_active([]); key = mean_hidden(rest[:512])
    db.insert([{"id": e, "tenant": TENANT[e], "valid_from": VALID[e], "key": key.numpy(), "weights": pack_adapter(e)}])
    learned[e] = {"rest_tokens": len(rest), "loss_first": round(float(np.mean(ls[:4])), 3), "loss_last": round(float(np.mean(ls[-4:])), 3)}
    print(e, learned[e], round(time.time() - t0), flush=True)
build_secs = round(time.time() - t0)
for m in LORAS: m.adapters.clear()                       # from here on, adapters come only from their rows

loaded = {}
def load(name):
    if name not in loaded:
        t = time.perf_counter(); blob = db.query("SELECT weights FROM adapters WHERE id = :i", i=name).rows[0][0]
        unpack_adapter(name, blob); loaded[name] = (time.perf_counter() - t) * 1000
    return name
def route(v, where="", k=1, temp=0.02, **params):
    t = time.perf_counter()
    r = db.query(f"SELECT id, score() FROM adapters {where} ORDER BY similarity(key, :v) DESC LIMIT {k}", v=v.numpy(), **params).rows
    ms = (time.perf_counter() - t) * 1000
    if not r: return [], ms
    s = np.array([x[1] for x in r]); w = np.exp((s - s.max()) / temp); w /= w.sum()
    return [(load(x[0]), float(wi)) for x, wi in zip(r, w)], ms

arms = {k: [] for k in ("none", "own_adapter", "w5_retrieved", "w6_tenant_top2", "w6_wrong_tenant_top2", "w6_as_of_nov1")}
picked_own, route_ms, docs = 0, [], []
for e, s, ctx, tgt in segments():
    docs.append(e); v = mean_hidden(ctx)
    def run(pairs): set_active(pairs); x = float(nll(None, ctx, tgt)); set_active([]); return x
    arms["none"].append(run([]))
    arms["own_adapter"].append(run([(load(e), 1.0)]))
    p, ms = route(v); route_ms.append(ms); picked_own += p[0][0] == e
    arms["w5_retrieved"].append(run(p))
    arms["w6_tenant_top2"].append(run(route(v, "WHERE tenant = :t", 2, t=TENANT[e])[0]))
    other = f"t{(int(TENANT[e][1]) + 1) % 4}"
    arms["w6_wrong_tenant_top2"].append(run(route(v, "WHERE tenant = :t", 2, t=other)[0]))
    arms["w6_as_of_nov1"].append(run(route(v, "WHERE tenant = :t AND valid_from <= TIMESTAMP '2026-11-01'", 2, t=TENANT[e])[0]))

docs = np.array(docs); late = np.isin(docs, EVAL[8:])
out = {"adapter": {"rank": 8, "layers": "o_proj and down_proj of all 28", "params": sum(m.r * (m.base.in_features + m.base.out_features) for m in LORAS),
                   "bytes_fp16": sum(m.r * (m.base.in_features + m.base.out_features) for m in LORAS) * 2,
                   "steps": STEPS, "build_secs": build_secs, "per_doc": learned},
       "ppl": {k: ppl(v) for k, v in arms.items()},
       "w5_picked_own_doc": picked_own / len(docs), "route_ms_p50": float(np.median(route_ms)),
       "row_load_ms_p50": float(np.median(list(loaded.values()))),
       "as_of_split": {"docs_valid_from_oct1": {k: ppl(np.array(arms[k])[~late]) for k in ("w6_tenant_top2", "w6_as_of_nov1")},
                       "docs_valid_from_nov15": {k: ppl(np.array(arms[k])[late]) for k in ("w6_tenant_top2", "w6_as_of_nov1", "none")}}}
json.dump(out, open(OUT, "w"), indent=1); print(json.dumps(out, indent=1))
