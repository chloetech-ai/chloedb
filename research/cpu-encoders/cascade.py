"""Static student and transformer teacher together (idea 3), from vectors
already computed: the student is trained into the teacher's space
(student.py), so their vectors compare.

* progressive: a fraction of the documents has been re-embedded by the
  teacher in the background, the rest still has its student vector from
  ingest; queries by the teacher (or by the student);
* cascade: the student's top 100, re-ranked by the teacher's vectors of
  those documents only, embedded on demand: the share of the corpus that
  ever needs a teacher vector is reported."""
import sys, os, json
import numpy as np
from safetensors.numpy import load_file
from tokenizers import Tokenizer
from common import load, evaluate, ndcg10, unit, cached


def student(d):
    d = os.path.expanduser(d)
    tok = Tokenizer.from_file(os.path.join(d, "tokenizer.json"))
    t = load_file(os.path.join(d, "model.safetensors"))
    E, G = t["embeddings"].astype(np.float32), t.get("bigrams")
    def enc(texts):
        out = []
        for e in tok.encode_batch(texts, add_special_tokens=False):
            ids = e.ids[:512]
            v = E[ids].sum(0) if ids else np.zeros(E.shape[1], np.float32)
            if G is not None and len(ids) > 1:
                b = [((x * 1000003 + y) % G.shape[0]) for x, y in zip(ids, ids[1:])]
                v = v + G[b].sum(0)
            out.append(v)
        return unit(np.stack(out))
    return enc


sdir = sys.argv[1]
teacher = sys.argv[2] if len(sys.argv) > 2 else "granite-small-r2"
enc = student(sdir)
rng = np.random.default_rng(0)
for ds in ["scifact", "nfcorpus", "arguana"]:
    ids, texts, qids, qtexts, qrels = load(ds)
    Ds, Qs = enc(texts), enc(qtexts)
    Dt, _ = cached(f"{teacher}|512|{ds}|docs", None); Qt, _ = cached(f"{teacher}|512|{ds}|queries", None)
    perm = rng.permutation(len(ids))
    for frac in (0.0, 0.25, 0.5, 0.75, 1.0):
        D = Ds.copy(); up = perm[: int(frac * len(ids))]; D[up] = Dt[up]
        for qname, Q in (("teacher", Qt), ("student", Qs)):
            nd, _ = evaluate(ids, D, qids, Q, qrels)
            print(json.dumps({"dataset": ds, "mode": "progressive", "upgraded": frac, "queries": qname, "ndcg10": round(nd, 4)}), flush=True)
    for k in (20, 100):
        S = Qs @ Ds.T  # student-only first stage
        touched, nd = set(), []
        for i, qid in enumerate(qids):
            top = np.argpartition(-S[i], k)[:k]
            touched.update(top.tolist())
            sc = Dt[top] @ Qt[i]
            ranked = [ids[j] for j in top[np.argsort(-sc)] if ids[j] != qid]
            nd.append(ndcg10(ranked, qrels[qid]))
        print(json.dumps({"dataset": ds, "mode": f"cascade student top {k} -> teacher", "ndcg10": round(float(np.mean(nd)), 4), "corpus_share_teacher_embedded": round(len(touched) / len(ids), 3)}), flush=True)
