"""Throughput of each encoder on the same texts, nothing else running:
documents per second (512 SciFact abstracts, about 300 tokens each) and
the latency of one query (median over 200 SciFact claims, batch of one),
at 1 and 4 threads."""
import os, sys, time, json
import numpy as np, torch
from common import load, hf

_, texts, _, qs, _ = load("scifact")
docs, queries = texts[:512], qs[:200]


def bench(name, make, threads):
    torch.set_num_threads(threads)
    os.environ["OMP_NUM_THREADS"] = str(threads)
    enc = make(threads)
    enc(docs[:32])
    t = time.perf_counter(); enc(docs); dps = len(docs) / (time.perf_counter() - t)
    lat = []
    for q in queries:
        t = time.perf_counter(); enc([q]); lat.append(time.perf_counter() - t)
    print(json.dumps({"encoder": name, "threads": threads, "docs_per_s": round(dps, 1), "query_ms_p50": round(1e3 * float(np.median(lat)), 3)}), flush=True)


def st(repo, dtype, maxlen=512):
    def make(threads):
        from sentence_transformers import SentenceTransformer
        m = SentenceTransformer(hf(repo), device="cpu", model_kwargs={"torch_dtype": dtype}, trust_remote_code=True); m.max_seq_length = maxlen
        return lambda xs: m.encode(xs, batch_size=32)
    return make


def onnx(d, f, maxlen=512):
    def make(threads):
        from onnx_encode import OnnxEncoder
        e = OnnxEncoder(os.path.expanduser(d), f, threads=threads, maxlen=maxlen)
        return e.encode
    return make


def m2v(path):
    def make(threads):
        from model2vec import StaticModel
        m = StaticModel.from_pretrained(path)
        return lambda xs: m.encode(xs, use_multiprocessing=False)
    return make


def sparse_doc(maxlen=512):
    def make(threads):
        from transformers import AutoModelForMaskedLM, AutoTokenizer
        p = hf("opensearch-project/opensearch-neural-sparse-encoding-doc-v3-distill")
        tok = AutoTokenizer.from_pretrained(p); m = AutoModelForMaskedLM.from_pretrained(p, torch_dtype=torch.bfloat16).eval()
        def enc(xs):
            with torch.no_grad():
                for i in range(0, len(xs), 16):
                    e = tok(xs[i:i + 16], padding=True, truncation=True, max_length=maxlen, return_tensors="pt")
                    torch.log1p(torch.relu(m(**e).logits.float())).max(1)
        return enc
    return make


ALL = {
    "potion-retrieval-32M (Model2Vec, Python)": m2v(hf("minishlab/potion-retrieval-32M")),
    "granite-small-r2 torch fp32": st("ibm-granite/granite-embedding-small-english-r2", torch.float32),
    "granite-small-r2 torch bf16": st("ibm-granite/granite-embedding-small-english-r2", torch.bfloat16),
    "granite-small-r2 onnx fp32": onnx("~/onnx/granite-small", "model.onnx"),
    "granite-small-r2 onnx int8": onnx("~/onnx/granite-small", "model-int8.onnx"),
    "granite-small-r2 onnx int8 maxlen 256": onnx("~/onnx/granite-small", "model-int8.onnx", 256),
    "granite-small-r2 onnx int8 maxlen 128": onnx("~/onnx/granite-small", "model-int8.onnx", 128),
    "bge-small-en-v1.5 torch fp32": st("BAAI/bge-small-en-v1.5", torch.float32),
    "bge-small-en-v1.5 torch bf16": st("BAAI/bge-small-en-v1.5", torch.bfloat16),
    "gte-modernbert-base torch bf16": st("Alibaba-NLP/gte-modernbert-base", torch.bfloat16),
    "sparse doc-v3-distill doc encoder bf16": sparse_doc(),
}
if __name__ == "__main__":
    pick = sys.argv[1:] or list(ALL)
    for name in pick:
        for th in (1, 4):
            bench(name, ALL[name], th)
