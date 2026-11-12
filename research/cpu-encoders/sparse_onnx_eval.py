"""Checks the ONNX export of the learned sparse document encoder against
the PyTorch one (sparse.py's cache): nDCG@10 with the same IDF queries,
terms per document and documents/s. Usage: sparse_onnx_eval.py DIR FILE
[datasets] (FILE: model.onnx or model-int8.onnx)."""
import json, os, sys, time
import numpy as np, scipy.sparse as sp, onnxruntime as ort
from tokenizers import Tokenizer
from common import load, ndcg10, CACHE

d = os.path.expanduser(sys.argv[1]); fname = sys.argv[2]
datasets = (sys.argv[3] if len(sys.argv) > 3 else "scifact,nfcorpus,arguana").split(",")
maxlen = int(os.environ.get("MAXLEN", "512"))
so = ort.SessionOptions(); so.intra_op_num_threads = int(os.environ.get("THREADS", "4"))
so.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL
s = ort.InferenceSession(os.path.join(d, fname), so, providers=["CPUExecutionProvider"])
tok = Tokenizer.from_file(os.path.join(d, "tokenizer.json"))
tok.enable_truncation(maxlen)
tok.no_padding()  # tokenizer.json may pad a batch to its longest: the mask would then cover the padding
V = tok.get_vocab_size()
vocab = tok.get_vocab()
idf = np.ones(V, np.float32)
for t, w in json.load(open(os.path.join(d, "idf.json"))).items():
    if t in vocab: idf[vocab[t]] = w
special = [vocab[t] for t in ("[CLS]", "[SEP]", "[PAD]", "[UNK]", "[MASK]")]


def csr(vs):
    indptr = np.cumsum([0] + [len(i) for i, _ in vs])
    return sp.csr_matrix((np.concatenate([w for _, w in vs]).astype(np.float32), np.concatenate([i for i, _ in vs]).astype(np.int64), indptr), shape=(len(vs), V))


def batches(order, lens, budget):
    """Runs of documents, sorted by length, of at most `budget` tokens with
    padding: the MLM head's logits are [batch, tokens, vocabulary], 2 GB of
    float32 for 32 documents of 512 tokens."""
    run = []
    for j in order:
        if run and (len(run) + 1) * lens[j] > budget:
            yield run; run = []
        run.append(j)
    if run:
        yield run


def docs(texts):
    encs = tok.encode_batch(texts)
    lens = [len(e.ids) for e in encs]
    order = np.argsort(lens); out = [None] * len(texts)
    for idx in batches(order, lens, int(os.environ.get("TOKENS", "4096"))):
        n = max(len(encs[j].ids) for j in idx)
        ids = np.zeros((len(idx), n), np.int64); m = np.zeros_like(ids)
        for r, j in enumerate(idx):
            e = encs[j].ids; ids[r, :len(e)] = e; m[r, :len(e)] = 1
        w = s.run(None, {"input_ids": ids, "attention_mask": m})[0]
        w[:, special] = 0
        for r, j in enumerate(idx):
            nz = np.nonzero(w[r])[0]; out[j] = (nz, w[r, nz])
    return csr(out)


def queries(texts):
    vs = []
    for t in texts:
        ids = sorted(set(tok.encode(t, add_special_tokens=False).ids) - set(special))
        vs.append((np.array(ids, np.int64), idf[ids]))
    return csr(vs)


def score(ids, D, qids, Q, qrels):
    S = (Q @ D.T).toarray(); nd = []
    for i, qid in enumerate(qids):
        top = np.argsort(-S[i])[:101]
        nd.append(ndcg10([ids[j] for j in top if ids[j] != qid][:100], qrels[qid]))
    return float(np.mean(nd))


for ds in datasets:
    ids, texts, qids, qtexts, qrels = load(ds)
    Q = queries(qtexts)
    t0 = time.perf_counter(); D = docs(texts); dt = time.perf_counter() - t0
    ref = sp.load_npz(CACHE / f"sparse-docs-{ds}-512.npz")
    print(json.dumps({"model": fname, "maxlen": maxlen, "dataset": ds, "ndcg10": round(score(ids, D, qids, Q, qrels), 4),
                      "torch_ndcg10": round(score(ids, ref, qids, Q, qrels), 4), "terms_per_doc": round(D.nnz / len(ids), 1),
                      "torch_terms_per_doc": round(ref.nnz / len(ids), 1), "docs_per_s": round(len(ids) / dt, 1)}), flush=True)
