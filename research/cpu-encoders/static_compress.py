"""fastText.zip for Model2Vec: how small can the static table get?
fp16, int8 with a scale per row, PCA, and product quantisation (PQ, as
fastText.zip compresses its tables), scored on BEIR by rebuilding the table
from its codes and embedding with it."""
import sys, json
import numpy as np, faiss
from model2vec import StaticModel
from common import load, evaluate, unit, hf

REPO = sys.argv[1] if len(sys.argv) > 1 else "minishlab/potion-retrieval-32M"
DATASETS = ["scifact", "nfcorpus", "arguana"]
m = StaticModel.from_pretrained(hf(REPO))
E = np.asarray(m.embedding, dtype=np.float32).copy()
V, d = E.shape
print(f"{REPO}: {V} tokens x {d} dims, {E.nbytes/1e6:.1f} MB fp32", file=sys.stderr)


def int8_rows(X):
    s = np.abs(X).max(1, keepdims=True) / 127 + 1e-12
    return np.round(X / s).astype(np.int8).astype(np.float32) * s, X.shape[0] * (X.shape[1] + 4)


def pca(X, k):
    mu = X.mean(0)
    U, S, Vt = np.linalg.svd(X - mu, full_matrices=False)
    return (X - mu) @ Vt[:k].T  # mean removal changes no ranking after unit length? it does: keep it as Model2Vec does (it PCAs centred)


def pq(X, M, nbits=8):
    q = faiss.ProductQuantizer(X.shape[1], M, nbits)
    q.train(X)
    codes = q.compute_codes(X)
    return q.decode(codes), codes.nbytes + faiss.vector_to_array(q.centroids).size * 4


variants = {"fp32": (E, E.nbytes)}
variants["fp16"] = (E.astype(np.float16).astype(np.float32), E.nbytes // 2)
variants["int8/row"] = int8_rows(E)
for k in (256, 128):
    P = pca(E, k)
    variants[f"pca{k}"] = (P, P.nbytes)
    variants[f"pca{k}+int8"] = int8_rows(P)
for M in (128, 64, 32):
    variants[f"pq{M}x8"] = pq(E, M)
P = pca(E, 256)
variants["pca256+pq32x8"] = pq(P, 32)

tok = m.tokenizer
for ds in DATASETS:
    ids, texts, qids, qtexts, qrels = load(ds)
    denc = tok.encode_batch_fast(texts, add_special_tokens=False) if hasattr(tok, "encode_batch_fast") else tok.encode_batch(texts, add_special_tokens=False)
    qenc = tok.encode_batch(qtexts, add_special_tokens=False)
    # mean of rows (Model2Vec, without its unknown token), at most 512 tokens
    unk = tok.token_to_id("[UNK]")
    def bag(encs):
        rows, cols = [], []
        for i, e in enumerate(encs):
            t = [x for x in e.ids if x != unk][:512]
            rows += [i] * len(t); cols += t
        import scipy.sparse as sp
        A = sp.csr_matrix((np.ones(len(rows), np.float32), (rows, cols)), shape=(len(encs), V))
        return A
    Ad, Aq = bag(denc), bag(qenc)
    for name, (T, nbytes) in variants.items():
        nd, rc = evaluate(ids, unit(Ad @ T), qids, unit(Aq @ T), qrels)
        print(json.dumps({"table": name, "dataset": ds, "mb": round(nbytes / 1e6, 1), "ndcg10": round(nd, 4), "recall100": round(rc, 4)}), flush=True)
