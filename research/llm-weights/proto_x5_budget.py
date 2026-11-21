"""X5: the lookup budget. How fast and how exact is ChloeDB's vector search on the model's
own hidden states, by index and codes, and what does an approximate neighbour cost the
model? Rows: D's 59,999 kNN-LM rows (Qwen3-0.6B last-layer states, raw, 1,024 numbers,
the dot product as similarity, as in D). Queries: the 8,192 eval positions' states.
For each configuration: build time, recall@32 against exact numpy search, latency per
lookup (p50, p99, one thread of queries), and kNN-LM perplexity with D's lambda and
temperature using the neighbours it returned (exact gives D's 32.8)."""
import json, math, os, sys, time
import numpy as np, torch, chloedb
from transformers import AutoTokenizer, AutoModelForCausalLM

S = os.environ["S"]; OUT = sys.argv[1]; K = 32
torch.set_num_threads(4); torch.set_grad_enabled(False)
tok = AutoTokenizer.from_pretrained(f"{S}/models/qwen3-0.6b-hf")
model = AutoModelForCausalLM.from_pretrained(f"{S}/models/qwen3-0.6b-hf", dtype=torch.float32).eval()
EVAL = ["python", "netops", "javascript", "connectors", "security", "earth", "opensearch", "graphs",
        "arxiv", "hn", "apps", "postgres", "troubleshooting", "crawler", "sqlite", "frameworks"]
CTX, STRIDE, START, END = 128, 64, 128, 128 + 8 * 64
d = json.load(open(f"{os.path.dirname(os.path.abspath(__file__))}/results/d_knnlm.json")); LAM, TEMP = d["lambda"], d["temp"]

src = chloedb.open(f"{S}/knn.chloedb", sync=False)
ids, nxt, keys = [], [], []
for a in range(0, 60000, 5000):
    for r in src.query("SELECT id, next, key FROM knn WHERE id >= :a AND id < :b LIMIT 5000", a=a, b=a + 5000).rows:
        ids.append(r[0]); nxt.append(r[1]); keys.append(np.array(r[2], dtype=np.float32))
src.close(); X = np.stack(keys); NXT = dict(zip(ids, nxt)); IDS = np.array(ids)
print("rows", X.shape, flush=True)

Q, LPY, Y = [], [], []
for e in EVAL:
    s_ids = tok(open(f"/home/user/chloedb/docs/chloedb/{e}.md").read(), add_special_tokens=False)["input_ids"]
    for s in range(START, END, STRIDE):
        seg = s_ids[s - CTX:s + STRIDE]; o = model(torch.tensor([seg]), output_hidden_states=True)
        h = o.hidden_states[-1][0].numpy(); lp = torch.log_softmax(o.logits[0].float(), -1)
        for p in range(CTX - 1, CTX + STRIDE - 1):
            Q.append(h[p].astype(np.float32)); LPY.append(float(lp[p][seg[p + 1]])); Y.append(seg[p + 1])
Q = np.stack(Q); print("queries", Q.shape, flush=True)
TRUE = np.argsort(-(Q @ X.T), axis=1)[:, :K]; TRUE_IDS = [set(IDS[t].tolist()) for t in TRUE]

def mixed_ppl(neigh):
    tot = []
    for (lpy, y), r in zip(zip(LPY, Y), neigh):
        p_lm = math.exp(lpy)
        if r:
            sims = np.array([s for _, s in r]); w = np.exp((sims - sims.max()) / TEMP); w /= w.sum()
            p_knn = float(sum(wi for (i, _), wi in zip(r, w) if NXT[i] == y))
        else: p_knn = 0.0
        tot.append(-math.log(max(1e-12, (1 - LAM) * p_lm + LAM * p_knn)))
    return math.exp(float(np.mean(tot)))
exact = [[(int(IDS[j]), float(Q[i] @ X[j])) for j in TRUE[i]] for i in range(len(Q))]
res = {"rows": len(X), "queries": len(Q), "k": K, "lambda": LAM, "temp": TEMP, "ppl_none": math.exp(-float(np.mean(LPY))),
       "ppl_exact_numpy": mixed_ppl(exact), "configs": {}}
print(res, flush=True)

CONFIGS = [  # name, field options, similarity options
 ("flat_sq8", {"index": "flat"}, ""),
 ("hnsw_sq8_ef200", {}, ""),
 ("hnsw_sq8_ef64", {}, ", ef => 64"),
 ("hnsw_sq8_ef32", {}, ", ef => 32"),
 ("hnsw_binary", {"codes": "binary"}, ""),
 ("hnsw_rabitq4", {"codes": "rabitq4"}, ""),
 ("ivf256_nprobe8", {"index": {"ivf": {"nlist": 256}}}, ", nprobe => 8"),
 ("ivf256_nprobe32", {"index": {"ivf": {"nlist": 256}}}, ", nprobe => 32"),
 ("hnsw_strategy_flat", {}, ", strategy => 'flat'"),
]
built = {}
for name, opts, sim in CONFIGS:
    key = json.dumps(opts, sort_keys=True)
    if key not in built:
        path = f"{S}/x5_{len(built)}.chloedb"; os.system(f"rm -rf {path}*"); t0 = time.time()
        db = chloedb.open(path, schema={"table": "knn", "key": "id", "fields": [
            {"name": "id", "type": "int"}, {"name": "key", "type": "vector", "dim": 1024, **opts}]}, sync=False)
        for a in range(0, len(X), 5000):
            db.insert([{"id": int(IDS[i]), "key": X[i]} for i in range(a, min(len(X), a + 5000))])
        db.flush(); db.optimize() if hasattr(db, "optimize") else None
        secs = time.time() - t0; size = sum(os.path.getsize(os.path.join(dp, f)) for dp, _, fs in os.walk(os.path.dirname(path)) for f in fs if f.startswith(os.path.basename(path)))
        built[key] = (db, round(secs, 1), round(size / 1e6, 1))
    db, bsecs, mb = built[key]
    sql = f"SELECT id, score() FROM knn ORDER BY similarity(key, :v{sim}) DESC LIMIT {K}"
    try:
        neigh, ms = [], []
        for q in Q:
            t = time.perf_counter(); r = db.query(sql, v=q).rows; ms.append((time.perf_counter() - t) * 1000); neigh.append([(a, b) for a, b in r])
    except Exception as ex:
        res["configs"][name] = {"error": str(ex)[:300]}; print(name, "error", ex, flush=True); continue
    rec = float(np.mean([len(TRUE_IDS[i] & {a for a, _ in neigh[i]}) / K for i in range(len(Q))]))
    res["configs"][name] = {"field": opts, "query_options": sim.strip(", "), "build_secs": bsecs, "file_mb": mb,
                            "recall_at_32": rec, "ms_p50": float(np.median(ms)), "ms_p99": float(np.percentile(ms, 99)),
                            "ppl_knn": mixed_ppl(neigh)}
    print(name, res["configs"][name], flush=True)
    json.dump(res, open(OUT, "w"), indent=1)
json.dump(res, open(OUT, "w"), indent=1); print(json.dumps(res, indent=1))
