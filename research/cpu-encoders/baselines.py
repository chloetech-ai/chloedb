"""Quality and CPU throughput of the encoders as published: Model2Vec
(potion), BGE-small (BERT), and ModernBERT embedders (granite small r2,
gte-modernbert-base), on BEIR test splits."""
import sys, os, time, json
import numpy as np, torch
from common import load, evaluate, unit, cached, cached_seconds, hf

torch.set_num_threads(int(os.environ.get("THREADS", "4")))
MODELS = {
    "potion-retrieval-32M": ("minishlab/potion-retrieval-32M", "m2v", ""),
    "potion-base-8M": ("minishlab/potion-base-8M", "m2v", ""),
    "bge-small-en-v1.5": ("BAAI/bge-small-en-v1.5", "st", "Represent this sentence for searching relevant passages: "),
    "granite-small-r2": ("ibm-granite/granite-embedding-small-english-r2", "st", ""),
    "gte-modernbert-base": ("Alibaba-NLP/gte-modernbert-base", "st", ""),
}
MAXLEN = int(os.environ.get("MAXLEN", "512"))


def encoder(name):
    if name.startswith("onnx:"):  # onnx:<dir>:<file>[:<query prefix>]
        from onnx_encode import OnnxEncoder
        _, d, f, *qp = name.split(":")
        e = OnnxEncoder(os.path.expanduser(d), f, threads=int(os.environ.get("THREADS", "4")), maxlen=MAXLEN)
        return e.encode, e.encode, (qp[0] if qp else "")
    if name not in MODELS:  # a Model2Vec directory
        MODELS[name] = (os.path.expanduser(name), "m2v-dir", "")
    repo, kind, qp = MODELS[name]
    if kind == "m2v-dir":
        from model2vec import StaticModel
        m = StaticModel.from_pretrained(repo)
        return (lambda xs: m.encode(xs, batch_size=1024)), (lambda xs: m.encode(xs)), qp
    if kind == "m2v":
        from model2vec import StaticModel
        m = StaticModel.from_pretrained(hf(repo))
        return (lambda xs: m.encode(xs, batch_size=1024)), (lambda xs: m.encode(xs)), qp
    from sentence_transformers import SentenceTransformer
    m = SentenceTransformer(hf(repo), device="cpu", trust_remote_code=True, model_kwargs={"torch_dtype": getattr(torch, os.environ.get("DTYPE", "bfloat16"))})
    m.max_seq_length = MAXLEN
    f = lambda xs: m.encode(xs, batch_size=32, normalize_embeddings=True, convert_to_numpy=True)
    return f, f, qp


if __name__ == "__main__":
    name = sys.argv[1]
    datasets = sys.argv[2:] or ["scifact", "nfcorpus", "arguana"]
    enc = None
    for ds in datasets:
        ids, texts, qids, qtexts, qrels = load(ds)
        def lazy():
            global enc
            if enc is None:
                enc = encoder(name)
            return enc
        if name not in MODELS and not name.startswith("onnx:"):
            MODELS[name] = (os.path.expanduser(name), "m2v-dir", "")
        qprefix = name.split(":")[3] if name.startswith("onnx:") and len(name.split(":")) > 3 else MODELS.get(name, ("", "", ""))[2]
        D, dt = cached(f"{name}|{MAXLEN}|{ds}|docs", lambda: unit(lazy()[0](texts)))
        Q, _ = cached(f"{name}|{MAXLEN}|{ds}|queries", lambda: unit(lazy()[1]([qprefix + q for q in qtexts])))
        dt = dt or cached_seconds(f"{name}|{MAXLEN}|{ds}|docs")
        nd, rc = evaluate(ids, D, qids, Q, qrels)
        print(json.dumps({"model": name, "maxlen": MAXLEN, "dataset": ds, "docs": len(ids), "ndcg10": round(nd, 4), "recall100": round(rc, 4), "doc_seconds": round(dt, 1), "docs_per_s": round(len(ids) / dt, 1)}), flush=True)
