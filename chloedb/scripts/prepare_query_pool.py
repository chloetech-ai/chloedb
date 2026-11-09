"""A pool of queries for engine-compare's traffic simulator, beside a
prepared collection: ORCAS's queries and the MS MARCO v2 passage train
queries, without duplicates and without the collection's own (dev) queries,
in a seeded random order, embedded as prepare_msmarco_compare.py embeds its
queries (Snowflake Arctic Embed M v1.5, its query prefix, the CLS output,
unit length).

Usage: python3 scripts/prepare_query_pool.py COLLECTION_DIR [--n N] [--model DIR] [--raw DIR]
       (needs numpy, onnxruntime, tokenizers; curl on PATH)

COLLECTION_DIR (say $CHLOEDB_DATA/msmarco-1000k) gets pool_queries.jsonl
{"id", "text", "source"}, pool_vecs.f32 (N x dim float32, dim from its
meta.json) and pool_meta.json. A profile with `queries.source = "pool"`
draws from them: its head is a seeded draw of the pool, its tail the
queries after the head, each sent once (`engine-compare plan`).

The pool's order is the same for any N (one seeded shuffle of all the
queries, then the first N), so a pool of 200,000 is the start of one of
1,000,000. Embedding runs on the CPU, about 150 queries a second on 4 cores:
200,000 take about 25 minutes, 1,000,000 about 2 hours (--n sets the size).

Both sources are Microsoft's, for non-commercial research (the MS MARCO and
ORCAS terms); the pool stays with the benchmark data and is not published.
"""
import argparse
import gzip
import json
import os
import subprocess
import sys
import time

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

BASE = "https://msmarco.z22.web.core.windows.net/msmarcoranking"
SOURCES = [
    ("orcas", f"{BASE}/orcas-doctrain-queries.tsv.gz"),
    ("msmarco-train", f"{BASE}/passv2_train_queries.tsv"),
]
MODEL = "https://huggingface.co/Snowflake/snowflake-arctic-embed-m-v1.5/resolve/main"
SEED = 20261105


def fetch(url, path):
    if not os.path.exists(path):
        subprocess.run(["curl", "-sSfL", "--retry", "5", "--retry-all-errors", "-o", path + ".part", url], check=True)
        os.rename(path + ".part", path)


def norm(t):
    return " ".join(t.lower().split())


def read_tsv(path):
    op = gzip.open if path.endswith(".gz") else open
    with op(path, "rt", encoding="utf-8") as f:
        for line in f:
            parts = line.rstrip("\n").split("\t", 1)
            if len(parts) == 2 and parts[1].strip():
                yield parts[0], parts[1].strip()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("collection")
    ap.add_argument("--n", type=int, default=1_000_000)
    ap.add_argument("--model", help="a directory with model.onnx and tokenizer.json (downloaded into --raw if not given)")
    ap.add_argument("--raw", help="where downloads go (default: COLLECTION/../raw)")
    ap.add_argument("--batch", type=int, default=64)
    a = ap.parse_args()
    meta = json.load(open(os.path.join(a.collection, "meta.json")))
    dim = meta["dim"]
    raw = a.raw or os.path.join(os.path.dirname(os.path.abspath(a.collection)), "raw")
    os.makedirs(raw, exist_ok=True)
    t0 = time.time()

    # The collection's own queries stay out of the pool.
    own = set()
    qpath = os.path.join(a.collection, "queries.jsonl")
    if os.path.exists(qpath):
        for line in open(qpath, encoding="utf-8"):
            own.add(norm(json.loads(line).get("text", "")))
    seen, pool = set(own), []
    for name, url in SOURCES:
        path = os.path.join(raw, url.rsplit("/", 1)[1])
        fetch(url, path)
        n0 = len(pool)
        for qid, text in read_tsv(path):
            k = norm(text)
            if k and k not in seen:
                seen.add(k)
                pool.append((f"{name}:{qid}", text, name))
        print(f"   {name}: {len(pool) - n0} queries ({time.time() - t0:.0f} s)", flush=True)
    rng = np.random.default_rng(SEED)
    order = rng.permutation(len(pool))
    pool = [pool[i] for i in order[: a.n]]
    print(f"   pool: {len(pool)} of {len(order)} queries, in a seeded order", flush=True)

    from embed_queries import PREFIX, Model  # needs onnxruntime and tokenizers: only now

    model = a.model
    if not model:
        model = os.path.join(raw, "arctic-m-v1.5")
        os.makedirs(model, exist_ok=True)
        fetch(f"{MODEL}/onnx/model.onnx", os.path.join(model, "model.onnx"))
        fetch(f"{MODEL}/tokenizer.json", os.path.join(model, "tokenizer.json"))
    m = Model(model)
    tmp_q = os.path.join(a.collection, "pool_queries.jsonl.part")
    tmp_v = os.path.join(a.collection, "pool_vecs.f32.part")
    with open(tmp_q, "w", encoding="utf-8") as fq, open(tmp_v, "wb") as fv:
        step = a.batch * 64
        for s in range(0, len(pool), step):
            chunk = pool[s:s + step]
            x = m.embed([PREFIX + t for _, t, _ in chunk], dim, batch=a.batch)
            fv.write(x.astype("<f4").tobytes())
            for qid, text, src in chunk:
                fq.write(json.dumps({"id": qid, "text": text, "source": src}) + "\n")
            done = s + len(chunk)
            rate = done / max(time.time() - t0, 1e-9)
            print(f"   embedded {done} of {len(pool)} ({rate:.0f} a second overall)", flush=True)
    os.rename(tmp_q, os.path.join(a.collection, "pool_queries.jsonl"))
    os.rename(tmp_v, os.path.join(a.collection, "pool_vecs.f32"))
    json.dump(
        {"queries": len(pool), "dim": dim, "seed": SEED, "sources": [n for n, _ in SOURCES], "model": "snowflake-arctic-embed-m-v1.5", "excluded_own_queries": len(own)},
        open(os.path.join(a.collection, "pool_meta.json"), "w"),
    )
    print(f"   wrote the pool of {len(pool)} queries into {a.collection} ({time.time() - t0:.0f} s)", flush=True)


if __name__ == "__main__":
    main()
