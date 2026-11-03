"""Prepare the first N MS MARCO v2.1 passages, with their Snowflake Arctic
Embed M v1.5 vectors (768 dimensions), in the files of prepare_beir.py, for
the engine comparison (strata-server/deploy/compare) at a size BEIR has no
collection for.

Usage: python3 scripts/prepare_msmarco_compare.py OUT_DIR [--docs N] [--queries Q] [--model DIR]
       (needs pyarrow, numpy, onnxruntime, tokenizers; curl on PATH)

OUT_DIR/msmarco-<N/1000>k/ gets docs.jsonl, doc_vecs.f32, queries.jsonl,
query_vecs.f32, qrels.tsv and meta.json. The sources are those of
prepare_msmarco.py: per shard of the Hugging Face dataset
Snowflake/msmarco-v2.1-snowflake-arctic-embed-m-v1.5, its Parquet file (ids
and vectors) and the same shard of the segmented corpus (text, fetched alone
by HTTP range request), whose ids are checked row by row. Shards are read in
order until N passages: 1M is the start of shard 00, 10M shards 00 to 05.

A passage is its document's title and its segment, the text the dataset's
vectors embed. year, category and rating are synthetic, drawn as
prepare_beir.py draws them; topic is the k-means cluster (32, trained on the
first 50,000 vectors) of the vector. Each 65,536 rows draw from a generator
of their own, so the first N passages are the same whatever N: the 1M
collection is the start of the 10M one.

The queries are the first Q (default 1,000) MS MARCO v2 passage dev queries,
embedded here with the same model (embed_queries.py: its query prefix, the
CLS output, unit length). There are no relevance judgments for a slice of
the corpus: qrels.tsv holds its header only, and the comparison scores the
vector kinds against an exact search (analyze.py).
"""
import argparse
import gzip
import json
import os
import sys
import time

import numpy as np
import pyarrow.parquet as pq

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from prepare_msmarco import HF, TAR, curl, cut, kmeans, tar_index  # noqa: E402

DIM = 768
BLOCK = 65536
MODEL = "https://huggingface.co/Snowflake/snowflake-arctic-embed-m-v1.5/resolve/main"
QUERIES = "https://msmarco.z22.web.core.windows.net/msmarcoranking/passv2_dev_queries.tsv"


def fetch(url, path, *extra):
    if not os.path.exists(path):
        curl(list(extra) + ["-o", path + ".part", url])
        os.rename(path + ".part", path)


def attrs(block, rows):
    """year, category, rating of a block of rows, from the block's own generator."""
    rng = np.random.default_rng([20261012, block])
    years = rng.integers(1990, 2025, size=rows)
    zipf = 1.0 / np.arange(1, 65)
    cats = rng.choice(64, size=rows, p=zipf / zipf.sum())
    ratings = np.round(rng.random(rows) * 5.0, 2)
    return years, cats, ratings


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("--docs", type=int, default=1_000_000)
    ap.add_argument("--queries", type=int, default=1000)
    ap.add_argument("--model", help="a directory with model.onnx and tokenizer.json (downloaded into OUT_DIR/raw if not given)")
    a = ap.parse_args()
    d = os.path.join(a.out, f"msmarco-{a.docs // 1000}k")
    raw = os.path.join(a.out, "raw")
    os.makedirs(d, exist_ok=True)
    os.makedirs(raw, exist_ok=True)
    t0 = time.time()

    idx = tar_index(raw)
    docs = open(os.path.join(d, "docs.jsonl"), "w")
    vecs = open(os.path.join(d, "doc_vecs.f32"), "wb")
    n, shard, cents, shards = 0, 0, None, []
    while n < a.docs:
        pq_path = os.path.join(raw, f"{shard:02d}.parquet")
        text_path = os.path.join(raw, f"text_{shard:02d}.json.gz")
        off, size = idx[f"msmarco_v2.1_doc_segmented_{shard:02d}.json.gz"]
        fetch(TAR, text_path, "-r", f"{off}-{off + size - 1}")
        fetch(HF.format(shard), pq_path)
        pf = pq.ParquetFile(pq_path)
        if cents is None:
            first = next(pf.iter_batches(batch_size=50_000, columns=["embedding"]))
            cents = kmeans(cut(first.column(0), DIM), 32)
        text = gzip.open(text_path, "rt", encoding="utf-8")
        start = n
        # Batches of BLOCK rows, so that a block's attributes come from its generator.
        for b in pf.iter_batches(batch_size=BLOCK, columns=["doc_id", "embedding"]):
            take = min(len(b), a.docs - n)
            ids = b.column(0).to_pylist()[:take]
            x = cut(b.column(1), DIM)[:take]
            topics = np.argmax(x @ cents.T, axis=1)
            # Blocks are counted over the whole collection; a shard ends mid-block.
            years, cats, ratings = np.empty(take, np.int64), np.empty(take, np.int64), np.empty(take)
            i = 0
            while i < take:
                blk, at = divmod(n + i, BLOCK)
                m = min(take - i, BLOCK - at)
                y, c, r = attrs(blk, BLOCK)
                years[i:i + m], cats[i:i + m], ratings[i:i + m] = y[at:at + m], c[at:at + m], r[at:at + m]
                i += m
            for j, pid in enumerate(ids):
                p = json.loads(text.readline())
                if p["docid"] != pid:
                    raise SystemExit(f"shard {shard}: row {n + j - start}: vector id {pid} but text id {p['docid']}")
                docs.write(json.dumps({
                    "id": pid, "title": p.get("title") or "", "text": p.get("segment") or "",
                    "year": int(years[j]), "category": f"c{int(cats[j]):02d}",
                    "rating": float(ratings[j]), "topic": f"t{int(topics[j]):02d}",
                }) + "\n")
            vecs.write(x.astype("<f4").tobytes())
            n += take
            if n >= a.docs:
                break
        shards.append({"shard": shard, "rows": n - start})
        print(f"   shard {shard:02d}: {n - start} passages, {n} in all, {time.time() - t0:.0f} s", flush=True)
        # The raw shard is done with (the text stays small, and is the source of any re-run).
        os.remove(pq_path)
        shard += 1
    docs.close()
    vecs.close()

    from embed_queries import PREFIX, Model  # needs onnxruntime and tokenizers: only now

    model = a.model
    if not model:
        model = os.path.join(raw, "arctic-m-v1.5")
        os.makedirs(model, exist_ok=True)
        fetch(f"{MODEL}/onnx/model.onnx", os.path.join(model, "model.onnx"))
        fetch(f"{MODEL}/tokenizer.json", os.path.join(model, "tokenizer.json"))
    qpath = os.path.join(raw, "passv2_dev_queries.tsv")
    fetch(QUERIES, qpath)
    qids, qtexts = [], []
    for line in open(qpath, encoding="utf-8"):
        i, t = line.rstrip("\n").split("\t", 1)
        qids.append(i)
        qtexts.append(t.strip())
        if len(qids) == a.queries:
            break
    qx = Model(model).embed([PREFIX + t for t in qtexts], DIM)
    qx.astype("<f4").tofile(os.path.join(d, "query_vecs.f32"))
    with open(os.path.join(d, "queries.jsonl"), "w") as f:
        for i, t in zip(qids, qtexts):
            f.write(json.dumps({"id": i, "text": t}) + "\n")
    with open(os.path.join(d, "qrels.tsv"), "w") as f:
        f.write("query-id\tcorpus-id\tscore\n")
    json.dump({"docs": n, "queries": len(qids), "dim": DIM, "source": "msmarco-v2.1 segmented + snowflake-arctic-embed-m-v1.5",
               "shards": shards}, open(os.path.join(d, "meta.json"), "w"))
    print(f"   {n} passages, {len(qids)} queries, dim {DIM}, in {d} ({time.time() - t0:.0f} s)", flush=True)


if __name__ == "__main__":
    main()
