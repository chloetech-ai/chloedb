"""The MS MARCO 1M slice of the DigitalOcean comparison, text only.

Same passages, ids and synthetic year/category/rating as
strata/scripts/prepare_msmarco_compare.py (first 1M of segmented shard 00),
but with 8-dimension placeholder vectors instead of the 768-d Arctic ones:
the BM25 kinds never read the vector field, and an 8-d HNSW builds in
seconds. topic is a hash of the id (the real one is a k-means cluster).

Usage: python3 prep.py OUT_DIR   (needs numpy; curl on PATH)
"""
import gzip
import hashlib
import json
import os
import subprocess
import sys

import numpy as np

TAR = "https://msmarco.z22.web.core.windows.net/msmarcoranking/msmarco_v2.1_doc_segmented.tar"
QUERIES = "https://msmarco.z22.web.core.windows.net/msmarcoranking/passv2_dev_queries.tsv"
BLOCK = 65536
DOCS = 1_000_000
NQ = 1000
DIM = 8


def range_bytes(a, b):
    return subprocess.run(["curl", "-sSfL", "--retry", "5", "-r", f"{a}-{b}", TAR], check=True, capture_output=True).stdout


def member0():
    off = 0
    while True:
        h = range_bytes(off, off + 511)
        name = h[:100].rstrip(b"\0").decode()
        sf = h[124:136]
        size = int.from_bytes(sf[1:], "big") if sf[0] & 0x80 else int(sf.rstrip(b"\0 ").decode() or "0", 8)
        if name.endswith("_00.json.gz"):
            return off + 512, size
        off += 512 + (size + 511) // 512 * 512


def attrs(block, rows):
    rng = np.random.default_rng([20261012, block])
    years = rng.integers(1990, 2025, size=rows)
    zipf = 1.0 / np.arange(1, 65)
    cats = rng.choice(64, size=rows, p=zipf / zipf.sum())
    ratings = np.round(rng.random(rows) * 5.0, 2)
    return years, cats, ratings


def main():
    out = os.path.join(sys.argv[1], "msmarco-1000k")
    os.makedirs(out, exist_ok=True)
    off, size = member0()
    curl = subprocess.Popen(["curl", "-sSfL", "--retry", "5", "-r", f"{off}-{off + size - 1}", TAR], stdout=subprocess.PIPE)
    text = gzip.open(curl.stdout, "rt", encoding="utf-8")
    docs = open(os.path.join(out, "docs.jsonl"), "w")
    vecs = open(os.path.join(out, "doc_vecs.f32"), "wb")
    n = 0
    while n < DOCS:
        blk = n // BLOCK
        take = min(BLOCK, DOCS - n)
        y, c, r = attrs(blk, BLOCK)
        rows = []
        for j in range(take):
            p = json.loads(text.readline())
            pid = p["docid"]
            hsh = hashlib.sha256(pid.encode()).digest()
            rows.append(np.frombuffer(hsh[:DIM * 4], dtype="<u4").astype(np.float32) / 2**32 - 0.5)
            docs.write(json.dumps({
                "id": pid, "title": p.get("title") or "", "text": p.get("segment") or "",
                "year": int(y[j]), "category": f"c{int(c[j]):02d}", "rating": float(r[j]), "topic": f"t{hsh[0] % 32:02d}",
            }) + "\n")
        x = np.stack(rows)
        x /= np.linalg.norm(x, axis=1, keepdims=True) + 1e-12
        vecs.write(x.astype("<f4").tobytes())
        n += take
        print(f"{n} passages", flush=True)
    curl.kill()
    docs.close()
    vecs.close()
    qtsv = subprocess.run(["curl", "-sSfL", "--retry", "5", QUERIES], check=True, capture_output=True).stdout.decode("utf-8")
    qs = []
    for line in qtsv.splitlines():
        i, t = line.rstrip("\n").split("\t", 1)
        qs.append((i, t.strip()))
        if len(qs) == NQ:
            break
    with open(os.path.join(out, "queries.jsonl"), "w") as f:
        for i, t in qs:
            f.write(json.dumps({"id": i, "text": t}) + "\n")
    q = np.ones((len(qs), DIM), dtype="<f4") / np.sqrt(DIM)
    q.tofile(os.path.join(out, "query_vecs.f32"))
    with open(os.path.join(out, "qrels.tsv"), "w") as f:
        f.write("query-id\tcorpus-id\tscore\n")
    json.dump({"docs": n, "queries": len(qs), "dim": DIM, "source": "msmarco-v2.1 segmented shard 00, text only, 8-d placeholder vectors"},
              open(os.path.join(out, "meta.json"), "w"))
    print("done", flush=True)


if __name__ == "__main__":
    main()
