"""Inverted index x transformers: inference-free learned sparse retrieval.

The document side is a transformer (OpenSearch's doc-v3-distill, a 6-layer
DistilBERT with its MLM head): each document becomes term weights
max_over_tokens log(1 + relu(logits)), once, at ingest. The query side is
inference-free: its tokens, each weighted by the model's IDF table, a
lookup as cheap as BM25's. Scoring is a dot product over shared terms,
which is what ChloeDB's SPARSE fields compute with the text index's top-k
code.

"Static" expansion (fastText-style): the document side made a lookup too.
Every vocabulary token is run through the encoder alone once; a document's
weights are the max (as SPLADE pools) of its tokens' expansions.

Writes the documents' and queries' term weights as JSONL for ChloeDB
(--export DIR)."""
import argparse, json, os, sys, time
import numpy as np, scipy.sparse as sp, torch
from transformers import AutoModelForMaskedLM, AutoTokenizer
from common import load, ndcg10, hf, CACHE

p = argparse.ArgumentParser()
p.add_argument("--repo", default="opensearch-project/opensearch-neural-sparse-encoding-doc-v3-distill")
p.add_argument("--datasets", default="scifact,nfcorpus,arguana")
p.add_argument("--maxlen", type=int, default=512)
p.add_argument("--static-topk", type=int, default=64)
p.add_argument("--export", default="")
a = p.parse_args()
torch.set_num_threads(int(os.environ.get("THREADS", "4")))
dtype = getattr(torch, os.environ.get("DTYPE", "bfloat16"))
path = hf(a.repo)
tok = AutoTokenizer.from_pretrained(path)
model = AutoModelForMaskedLM.from_pretrained(path, torch_dtype=dtype).eval()
V = tok.vocab_size
idf_map = json.load(open(os.path.join(path, "idf.json")))
idf = np.ones(V, np.float32)
for t, w in idf_map.items():
    i = tok.convert_tokens_to_ids(t)
    if i is not None and i < V:
        idf[i] = w
special = set(tok.all_special_ids)


def doc_encode(texts, bs=16):
    rows = []
    order = np.argsort([len(t) for t in texts])
    out = [None] * len(texts)
    with torch.no_grad():
        for i in range(0, len(texts), bs):
            idx = order[i:i + bs]
            enc = tok([texts[j] for j in idx], padding=True, truncation=True, max_length=a.maxlen, return_tensors="pt")
            logits = model(**enc).logits.float()
            w = torch.log1p(torch.relu(logits)) * enc["attention_mask"][..., None]
            w = w.max(1).values
            w[:, list(special)] = 0
            for j, r in zip(idx, w):
                nz = torch.nonzero(r).squeeze(1)
                out[j] = (nz.numpy(), r[nz].numpy())
    return to_csr(out)


def to_csr(vs):
    indptr = np.cumsum([0] + [len(i) for i, _ in vs])
    return sp.csr_matrix((np.concatenate([w for _, w in vs]).astype(np.float32), np.concatenate([i for i, _ in vs]).astype(np.int64), indptr), shape=(len(vs), V))


def query_encode(texts):
    vs = []
    for t in texts:
        ids = sorted(set(tok(t, add_special_tokens=False)["input_ids"]) - special)
        vs.append((np.array(ids, np.int64), idf[ids]))
    return to_csr(vs)


def static_table():
    f = CACHE / f"static-sparse-{a.static_topk}.npz"
    if f.exists():
        return sp.load_npz(f)
    vs = []
    with torch.no_grad():
        for s in range(0, V, 256):
            ids = torch.arange(s, min(V, s + 256))
            inp = torch.stack([torch.full_like(ids, tok.cls_token_id), ids, torch.full_like(ids, tok.sep_token_id)], 1)
            logits = model(input_ids=inp, attention_mask=torch.ones_like(inp)).logits.float()
            w = torch.log1p(torch.relu(logits)).max(1).values
            w[:, list(special)] = 0
            top = torch.topk(w, a.static_topk, dim=1)
            for i, wv in zip(top.indices, top.values):
                keep = wv > 0
                vs.append((i[keep].numpy(), wv[keep].numpy()))
    T = to_csr(vs)
    sp.save_npz(f, T)
    return T


def static_docs(T, texts):
    vs = []
    for e in tok(texts, add_special_tokens=False, truncation=True, max_length=a.maxlen)["input_ids"]:
        ids = sorted(set(e) - special)
        sub = T[ids]
        # max over the tokens' expansions, per term
        coo = sub.tocoo()
        if coo.nnz == 0:
            vs.append((np.zeros(0, np.int64), np.zeros(0, np.float32))); continue
        order = np.lexsort((-coo.data, coo.col))
        cols, data = coo.col[order], coo.data[order]
        first = np.r_[True, cols[1:] != cols[:-1]]
        vs.append((cols[first].astype(np.int64), data[first]))
    return to_csr(vs)


def score(ids, D, qids, Q, qrels):
    S = (Q @ D.T).toarray()
    nd = []
    for i, qid in enumerate(qids):
        top = np.argsort(-S[i])[:101]
        ranked = [ids[j] for j in top if ids[j] != qid][:100]
        nd.append(ndcg10(ranked, qrels[qid]))
    return float(np.mean(nd))


T = static_table()
print(f"static expansion table: {T.nnz} weights ({T.nnz / V:.0f} a token)", file=sys.stderr)
for ds in a.datasets.split(","):
    ids, texts, qids, qtexts, qrels = load(ds)
    f = CACHE / f"sparse-docs-{ds}-{a.maxlen}.npz"
    t0 = time.perf_counter()
    if f.exists():
        D = sp.load_npz(f); dt = json.load(open(str(f) + ".json"))["seconds"]
    else:
        D = doc_encode(texts); dt = time.perf_counter() - t0
        sp.save_npz(f, D); json.dump({"seconds": dt}, open(str(f) + ".json", "w"))
    t0 = time.perf_counter(); Q = query_encode(qtexts); qt = (time.perf_counter() - t0) / len(qtexts)
    t0 = time.perf_counter(); Ds = static_docs(T, texts); st = time.perf_counter() - t0
    for name, DD, sec in (("transformer docs", D, dt), ("static expansion docs", Ds, st)):
        print(json.dumps({"dataset": ds, "docs": name, "ndcg10": round(score(ids, DD, qids, Q, qrels), 4), "terms_per_doc": round(DD.nnz / DD.shape[0], 1), "doc_seconds": round(sec, 1), "docs_per_s": round(len(ids) / sec, 1), "query_us": round(qt * 1e6, 1)}), flush=True)
    if a.export:
        os.makedirs(a.export, exist_ok=True)
        inv = tok.convert_ids_to_tokens(list(range(V)))
        def obj(row):
            return {inv[i]: round(float(w), 4) for i, w in zip(row.indices, row.data)}
        with open(os.path.join(a.export, f"{ds}-docs.jsonl"), "w") as o:
            for i, d in enumerate(ids):
                o.write(json.dumps({"id": d, "text": texts[i], "sparse": obj(D[i]), "static": obj(Ds[i])}) + "\n")
        with open(os.path.join(a.export, f"{ds}-queries.jsonl"), "w") as o:
            for i, q in enumerate(qids):
                o.write(json.dumps({"id": q, "text": qtexts[i], "sparse": obj(Q[i])}) + "\n")
