"""Prepare a BEIR collection, or the first N documents of one, with vectors
embedded here (fastembed), for when no precomputed embeddings fit: the files
of prepare_beir.py, so every loader reads it as it reads the others.

Usage: python3 scripts/prepare_beir_local.py OUT_DIR NAME [--docs N] [--model M] [--threads T]
       (needs pyarrow, numpy, fastembed; curl on PATH)

Downloads BeIR/<NAME> (corpus and queries, Parquet) and BeIR/<NAME>-qrels
(test.tsv) from Hugging Face. With --docs N it keeps every document a test
query judges relevant, then the corpus's first documents, to N in all, in
the corpus's order: the judged documents are all there, so the scores
measure ranking over N documents (higher than over the whole corpus, the
same for every engine). Documents and queries are embedded with fastembed's
M (default BAAI/bge-small-en-v1.5, 384 dimensions, unit length), queries with
the model's query prefix; a document's text is its title and text, its
first 1,000 characters (the model reads at most 512 tokens).

OUT_DIR/<NAME>-<N/1000>k/ (OUT_DIR/<NAME>/ without --docs) gets docs.jsonl,
doc_vecs.f32, queries.jsonl, query_vecs.f32, qrels.tsv and meta.json, with
the synthetic year, category, rating and topic of prepare_beir.py.
"""
import argparse
import json
import os
import subprocess
import sys
import time

import numpy as np
import pyarrow.parquet as pq

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from prepare_beir import kmeans  # noqa: E402

HF = "https://huggingface.co/datasets/BeIR"


def fetch(url, path):
    if not os.path.exists(path) or os.path.getsize(path) == 0:
        subprocess.run(["curl", "-sSfL", "-o", path + ".part", url], check=True)
        os.replace(path + ".part", path)


def embed(model, texts, query, what):
    t0 = time.perf_counter()
    rows = []
    it = model.query_embed(texts) if query else model.embed(texts, batch_size=128)
    for i, v in enumerate(it):
        rows.append(np.asarray(v, dtype=np.float32))
        if i % 20000 == 0 and i:
            el = time.perf_counter() - t0
            print(f"   {what}: {i} of {len(texts)} in {el:.0f} s ({i / el:.0f}/s)", flush=True)
    out = np.stack(rows)
    out /= np.maximum(np.linalg.norm(out, axis=1, keepdims=True), 1e-12)
    print(f"   {what}: {len(texts)} embedded in {time.perf_counter() - t0:.0f} s", flush=True)
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("name")
    ap.add_argument("--docs", type=int)
    ap.add_argument("--model", default="BAAI/bge-small-en-v1.5")
    ap.add_argument("--threads", type=int, default=os.cpu_count())
    a = ap.parse_args()

    raw = os.path.join(a.out, "raw", a.name)
    os.makedirs(raw, exist_ok=True)
    corpus = os.path.join(raw, "corpus.parquet")
    queries = os.path.join(raw, "queries.parquet")
    qrels_path = os.path.join(raw, "test.tsv")
    fetch(f"{HF}/{a.name}/resolve/main/corpus/corpus-00000-of-00001.parquet", corpus)
    fetch(f"{HF}/{a.name}/resolve/main/queries/queries-00000-of-00001.parquet", queries)
    fetch(f"{HF}/{a.name}-qrels/resolve/main/test.tsv", qrels_path)

    qrels = []
    with open(qrels_path) as f:
        next(f)
        for line in f:
            qid, did, score = line.rstrip("\n").split("\t")
            qrels.append((qid, did, int(score)))
    judged_docs = {d for _, d, s in qrels if s > 0}
    judged_queries = {q for q, _, s in qrels if s > 0}

    t = pq.read_table(corpus, columns=["_id", "title", "text"])
    ids = t.column("_id").to_pylist()
    n_all = len(ids)
    if a.docs and a.docs < n_all:
        keep = np.zeros(n_all, dtype=bool)
        for i, d in enumerate(ids):
            if d in judged_docs:
                keep[i] = True
        room = a.docs - int(keep.sum())
        if room < 0:
            raise SystemExit(f"{int(keep.sum())} judged documents, more than --docs {a.docs}")
        for i in range(n_all):
            if room == 0:
                break
            if not keep[i]:
                keep[i] = True
                room -= 1
        idx = np.nonzero(keep)[0]
        dname = f"{a.name}-{a.docs // 1000}k"
    else:
        idx = np.arange(n_all)
        dname = a.name
    titles = t.column("title").to_pylist()
    texts = t.column("text").to_pylist()
    ids = [ids[i] for i in idx]
    titles = [titles[i] or "" for i in idx]
    texts = [texts[i] or "" for i in idx]
    del t
    n = len(ids)
    print(f"== {dname}: {n} of {n_all} documents ({len(judged_docs)} judged)", flush=True)

    d = os.path.join(a.out, dname)
    os.makedirs(d, exist_ok=True)
    from fastembed import TextEmbedding

    model = TextEmbedding(a.model, threads=a.threads)
    vec_path = os.path.join(d, "doc_vecs.f32")
    x = embed(model, [((ti + "\n" + te) if ti else te)[:1000] for ti, te in zip(titles, texts)], False, "documents")
    x.astype("<f4").tofile(vec_path)
    dim = x.shape[1]

    rng = np.random.default_rng(20261012)
    years = rng.integers(1990, 2025, size=n)
    zipf = 1.0 / np.arange(1, 65)
    cats = rng.choice(64, size=n, p=zipf / zipf.sum())
    ratings = np.round(rng.random(n) * 5.0, 2)
    topics = kmeans(x, 32)
    with open(os.path.join(d, "docs.jsonl"), "w") as f:
        for i in range(n):
            f.write(json.dumps({
                "id": ids[i], "title": titles[i], "text": texts[i],
                "year": int(years[i]), "category": f"c{int(cats[i]):02d}",
                "rating": float(ratings[i]), "topic": f"t{int(topics[i]):02d}",
            }) + "\n")

    q = pq.read_table(queries, columns=["_id", "text"]).to_pylist()
    q = [r for r in q if r["_id"] in judged_queries]
    qv = embed(model, [r["text"] for r in q], True, "queries")
    qv.astype("<f4").tofile(os.path.join(d, "query_vecs.f32"))
    with open(os.path.join(d, "queries.jsonl"), "w") as f:
        for r in q:
            f.write(json.dumps({"id": r["_id"], "text": r["text"]}) + "\n")
    kept = set(ids)
    with open(os.path.join(d, "qrels.tsv"), "w") as f:
        for qid, did, score in qrels:
            if did in kept:
                f.write(f"{qid}\t{did}\t{score}\n")
    json.dump({"docs": n, "queries": len(q), "dim": dim, "model": a.model, "source": f"BeIR/{a.name}", "of": n_all},
              open(os.path.join(d, "meta.json"), "w"))
    print(f"   {len(q)} judged test queries; wrote {d}", flush=True)


if __name__ == "__main__":
    main()
