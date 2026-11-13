"""Shared pieces of the CPU-encoder study: BEIR collections as fetched by
chloedb/tests/fixtures/beir/fetch.sh, exact cosine search, nDCG@10 with
linear gains (as chloedb::metrics::ndcg_at_k), and a cache of embeddings."""
import json, os, time, hashlib
from pathlib import Path
import numpy as np

BEIR = Path(os.environ.get("CHLOEDB_BEIR_DIR", Path.home() / "beir"))
CACHE = Path(os.environ.get("ENC_CACHE", Path.home() / "enc-cache"))
CACHE.mkdir(parents=True, exist_ok=True)


def load(name, split="test"):
    d = BEIR / name
    docs = [json.loads(l) for l in open(d / "corpus.jsonl")]
    qs = {}
    for l in open(d / "queries.jsonl"):
        q = json.loads(l)
        qs[q["_id"]] = q["text"]
    qrels = {}
    with open(d / "qrels" / f"{split}.tsv") as f:
        next(f)
        for l in f:
            qid, did, g = l.rstrip("\n").split("\t")
            qrels.setdefault(qid, {})[did] = int(g)
    qids = [q for q in qrels if q in qs and any(g > 0 for g in qrels[q].values())]
    ids = [x["_id"] for x in docs]
    # title and text joined by a newline, as chloedb-embed-eval's emb_both
    texts = [(x.get("title") or "") + "\n" + x["text"] if x.get("title") else x["text"] for x in docs]
    return ids, texts, qids, [qs[q] for q in qids], qrels


def ndcg10(ranked, judged):
    dcg = sum(judged.get(d, 0) / np.log2(i + 2) for i, d in enumerate(ranked[:10]))
    ideal = sorted((g for g in judged.values() if g > 0), reverse=True)[:10]
    idcg = sum(g / np.log2(i + 2) for i, g in enumerate(ideal))
    return dcg / idcg if idcg else 0.0


def evaluate(ids, D, qids, Q, qrels, k=100):
    """nDCG@10 and recall@100 of exact dot-product search (rows unit length)."""
    S = Q @ D.T
    nd, rc = [], []
    for i, qid in enumerate(qids):
        top = np.argpartition(-S[i], k + 1)[: k + 1]
        top = top[np.argsort(-S[i][top])]
        ranked = [ids[j] for j in top if ids[j] != qid][:k]  # BEIR ignores a query's own id (ArguAna)
        j = qrels[qid]
        nd.append(ndcg10(ranked, j))
        rel = [d for d, g in j.items() if g > 0]
        rc.append(len(set(ranked) & set(rel)) / len(rel))
    return float(np.mean(nd)), float(np.mean(rc))


def unit(x):
    x = np.asarray(x, dtype=np.float32)
    n = np.linalg.norm(x, axis=1, keepdims=True)
    return x / np.maximum(n, 1e-12)


def cached(key, fn):
    p = CACHE / (hashlib.sha1(key.encode()).hexdigest()[:16] + ".npy")
    if p.exists():
        return np.load(p), None
    t = time.perf_counter()
    x = fn()
    dt = time.perf_counter() - t
    np.save(p, x)
    (CACHE / (p.stem + ".json")).write_text(json.dumps({"key": key, "seconds": dt}))
    return x, dt


def cached_seconds(key):
    p = CACHE / (hashlib.sha1(key.encode()).hexdigest()[:16] + ".json")
    return json.loads(p.read_text())["seconds"] if p.exists() else None


def hf(repo):
    """Local snapshot directory of a Hugging Face repository fetched into $HF_HOME."""
    root = Path(os.environ.get("HF_HOME", Path.home() / "hf")) / "hub" / ("models--" + repo.replace("/", "--")) / "snapshots"
    return str(next(root.iterdir()))
