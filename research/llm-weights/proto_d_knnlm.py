"""Aggressive idea D (similarity here is the dot product of the raw hidden states, MIPS): knowledge as rows in the model's own representation space.
kNN-LM (Khandelwal et al., 2020) with the datastore in an embedded ChloeDB
table: each row is (the model's last hidden state at a token, the token that
came next). At inference, every position's hidden state is a vector query;
the neighbours' next tokens form a distribution mixed into the model's.
INSERT writes knowledge in, DELETE takes it out, with no training.

Same model and eval segments as prototype B: Qwen3-0.6B, 16 ChloeDB docs it
has never seen, 8 strides of 64 tokens after 128 tokens of context each."""
import json, os, sys, time, glob, math
import numpy as np, torch, chloedb
from transformers import AutoTokenizer, AutoModelForCausalLM

S = os.environ["S"]; OUT = sys.argv[1]
torch.set_num_threads(4); torch.set_grad_enabled(False)
tok = AutoTokenizer.from_pretrained(f"{S}/models/qwen3-0.6b-hf")
model = AutoModelForCausalLM.from_pretrained(f"{S}/models/qwen3-0.6b-hf", torch_dtype=torch.float32).eval()
EVAL = ["python", "netops", "javascript", "connectors", "security", "earth", "opensearch", "graphs",
        "arxiv", "hn", "apps", "postgres", "troubleshooting", "crawler", "sqlite", "frameworks"]
DEV = EVAL[:4]
CTX, STRIDE, START, END, K = 128, 64, 128, 128 + 8 * 64, 32
DS_CAP, REST_CAP = 60000, 4000

def toks_of(path): return tok(open(path).read(), add_special_tokens=False)["input_ids"]

def hidden_and_logits(ids):
    o = model(torch.tensor([ids]), output_hidden_states=True)
    return o.hidden_states[-1][0].numpy(), torch.log_softmax(o.logits[0].float(), -1).numpy()

def windows(ids, doc, first_id):
    """Keys with 128 to 255 tokens of context, as the eval positions have 128."""
    rows, nid = [], first_id
    for w in range(0, max(1, len(ids) - 1), 128):
        seg = ids[w:w + 256]
        if len(seg) < 2: break
        h, _ = hidden_and_logits(seg)
        lo = 0 if w == 0 else 128
        for p in range(lo, len(seg) - 1):
            rows.append({"id": nid, "doc": doc, "next": int(seg[p + 1]), "norm": float(np.linalg.norm(h[p])), "key": h[p].astype(np.float32)}); nid += 1
        if len(seg) < 256: break
    return rows

path = f"{S}/knn.chloedb"
REUSE = os.path.exists(path) and os.environ.get("REUSE") == "1"
if os.path.exists(path) and not REUSE: os.system(f"rm -rf {path}*")
db = chloedb.open(path, schema={"table": "knn", "key": "id", "fields": [
    {"name": "id", "type": "int"}, {"name": "doc", "type": "keyword"}, {"name": "next", "type": "int"},
    {"name": "norm", "type": "float"}, {"name": "key", "type": "vector", "dim": 1024}]}, sync=False)

# 1. Datastore: ChloeDB docs other than the 16 eval docs.
t0 = time.time(); nid, n = 0, 0
if REUSE:
    db.sql("DELETE FROM knn WHERE doc LIKE 'rest/%'")
    n = nid = db.query("SELECT COUNT(*) FROM knn").rows[0][0]; others = []
else: others = [f for f in sorted(glob.glob("/home/user/chloedb/docs/chloedb/*.md") + glob.glob("/home/user/chloedb/docs/chloectx/*.md"))
          if not any(f.endswith(f"chloedb/{e}.md") for e in EVAL)]
for f in others:
    if n >= DS_CAP: break
    ids = toks_of(f)[:min(8000, DS_CAP - n)]
    rows = windows(ids, os.path.relpath(f, "/home/user/chloedb/docs"), nid)
    db.insert(rows); nid += len(rows); n += len(rows)
build = {"reused": REUSE, "rows": n, "secs": round(time.time() - t0, 1)}
print("datastore", build, flush=True)

# 2. Eval positions: model log-probs and hidden states, once.
evalset = []
for e in EVAL:
    ids = toks_of(f"/home/user/chloedb/docs/chloedb/{e}.md")
    for s in range(START, END, STRIDE):
        seg = ids[s - CTX:s + STRIDE]; h, lp = hidden_and_logits(seg)
        for p in range(CTX - 1, CTX + STRIDE - 1):
            evalset.append({"doc": e, "h": h[p].copy(), "lpy": float(lp[p][seg[p + 1]]), "y": seg[p + 1]})
print("eval positions", len(evalset), flush=True)

def neighbours(docs_excluded=()):
    out, ms = [], []
    for x in evalset:
        t = time.perf_counter()
        r = db.query("SELECT next, score() FROM knn ORDER BY similarity(key, :v) DESC LIMIT %d" % K, v=x["h"].astype(np.float32)).rows
        ms.append((time.perf_counter() - t) * 1000); out.append(r)
    return out, float(np.median(ms))

def mixed_nll(nb, lam, temp, docs):
    tot = []
    for x, r in zip(evalset, nb):
        if x["doc"] not in docs: continue
        p_lm = math.exp(x["lpy"])
        if r:
            sims = np.array([s for _, s in r]); w = np.exp((sims - sims.max()) / temp); w /= w.sum()
            p_knn = float(sum(wi for (nx, _), wi in zip(r, w) if nx == x["y"]))
        else: p_knn = 0.0
        tot.append(-math.log(max(1e-12, (1 - lam) * p_lm + lam * p_knn)))
    return float(np.mean(tot))

def tune(nb):
    best = min(((mixed_nll(nb, l, t, DEV), l, t) for l in (0.05, 0.1, 0.2, 0.3, 0.5) for t in (0.3, 1, 3, 10, 30)))
    return best[1], best[2]

TEST = EVAL[4:]
res = {"model": "Qwen3-0.6B (HF, fp32, CPU)", "datastore": build, "k": K, "eval_positions": len(evalset)}
base = {"all": mixed_nll([[]] * len(evalset), 0.0, 1, EVAL), "test": mixed_nll([[]] * len(evalset), 0.0, 1, TEST)}
res["ppl_none"] = {k: math.exp(v) for k, v in base.items()}

nb, ms = neighbours(); lam, temp = tune(nb)
res["lookup_ms_p50"] = ms; res["lambda"], res["temp"] = lam, temp
res["ppl_knn_other_docs"] = {"all": math.exp(mixed_nll(nb, lam, temp, EVAL)), "test": math.exp(mixed_nll(nb, lam, temp, TEST))}

# 3. INSERT knowledge: the rest of each eval doc (after its first 640 tokens), never the scored tokens.
t0 = time.time(); added = 0
for e in EVAL:
    ids = toks_of(f"/home/user/chloedb/docs/chloedb/{e}.md")[END:END + REST_CAP]
    if len(ids) > 2:
        rows = windows(ids, f"rest/{e}", nid); db.insert(rows); nid += len(rows); added += len(rows)
nb2, _ = neighbours(); lam2, temp2 = tune(nb2)
res["insert_rest_of_docs"] = {"rows": added, "secs": round(time.time() - t0, 1), "lambda": lam2, "temp": temp2,
    "ppl": {"all": math.exp(mixed_nll(nb2, lam2, temp2, EVAL)), "test": math.exp(mixed_nll(nb2, lam2, temp2, TEST))}}

# 4. DELETE it again: knowledge out, with one statement.
t0 = time.time()
db.sql("DELETE FROM knn WHERE doc LIKE 'rest/%'")
nb3, _ = neighbours()
res["delete_rest_of_docs"] = {"secs": round(time.time() - t0, 1),
    "ppl": {"all": math.exp(mixed_nll(nb3, lam, temp, EVAL)), "test": math.exp(mixed_nll(nb3, lam, temp, TEST))}}
json.dump(res, open(OUT, "w"), indent=1); print(json.dumps(res, indent=1))
