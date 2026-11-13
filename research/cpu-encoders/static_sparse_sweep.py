"""The static sparse expansion, pruned: each token's expansion cut to its
top k terms, and each document's weights to its top n, max-pooled as
SPLADE pools; queries as in sparse.py (IDF of their tokens)."""
import json, numpy as np, scipy.sparse as sp
from transformers import AutoTokenizer
from common import load, ndcg10, hf, CACHE

path = hf("opensearch-project/opensearch-neural-sparse-encoding-doc-v3-distill")
tok = AutoTokenizer.from_pretrained(path); V = tok.vocab_size
special = set(tok.all_special_ids)
idf = np.ones(V, np.float32)
for t, w in json.load(open(f"{path}/idf.json")).items():
    i = tok.convert_tokens_to_ids(t)
    if i is not None and i < V: idf[i] = w
T64 = sp.load_npz(CACHE / "static-sparse-64.npz").tocsr()

def prune_rows(M, k):
    rows = []
    for i in range(M.shape[0]):
        s, e = M.indptr[i], M.indptr[i + 1]
        idx, d = M.indices[s:e], M.data[s:e]
        o = np.argsort(-d)[:k]; rows.append((idx[o], d[o]))
    ip = np.cumsum([0] + [len(a) for a, _ in rows])
    return sp.csr_matrix((np.concatenate([b for _, b in rows]), np.concatenate([a for a, _ in rows]), ip), shape=M.shape)

def docs(T, texts, n):
    out = []
    for e in tok(texts, add_special_tokens=False, truncation=True, max_length=512)["input_ids"]:
        ids = sorted(set(e) - special)
        coo = T[ids].tocoo()
        if coo.nnz == 0: out.append((np.zeros(0, np.int64), np.zeros(0, np.float32))); continue
        o = np.lexsort((-coo.data, coo.col)); c, d = coo.col[o], coo.data[o]
        f = np.r_[True, c[1:] != c[:-1]]; c, d = c[f], d[f]
        o = np.argsort(-d)[:n]; out.append((c[o], d[o]))
    ip = np.cumsum([0] + [len(a) for a, _ in out])
    return sp.csr_matrix((np.concatenate([b for _, b in out]).astype(np.float32), np.concatenate([a for a, _ in out]).astype(np.int64), ip), shape=(len(out), V))

def queries(texts):
    out = []
    for t in texts:
        ids = sorted(set(tok(t, add_special_tokens=False)["input_ids"]) - special); out.append((np.array(ids, np.int64), idf[ids]))
    ip = np.cumsum([0] + [len(a) for a, _ in out])
    return sp.csr_matrix((np.concatenate([b for _, b in out]), np.concatenate([a for a, _ in out]), ip), shape=(len(out), V))

for k in (4, 16):
    T = prune_rows(T64, k)
    for ds in ("scifact", "nfcorpus", "arguana"):
        ids, texts, qids, qt, qrels = load(ds); Q = queries(qt)
        for n in (128, 400):
            D = docs(T, texts, n); S = (Q @ D.T).toarray(); nd = []
            for i, qid in enumerate(qids):
                top = np.argsort(-S[i])[:101]; nd.append(ndcg10([ids[j] for j in top if ids[j] != qid], qrels[qid]))
            print(json.dumps({"token_topk": k, "doc_terms": n, "dataset": ds, "ndcg10": round(float(np.mean(nd)), 4)}), flush=True)
