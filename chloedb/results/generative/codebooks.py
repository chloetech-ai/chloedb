"""Plain against balanced semantic-ID codebooks on a collection's item
vectors: `chloedb.train.codebook` with and without `balanced=True` (3
levels of 256, seed 0), each scored by `chloedb.train.codebook_report`.

The vectors are the items' texts as tiger.py reads them (title, brand,
categories and price), embedded by bge-small-en-v1.5 through
sentence-transformers and kept in `<work>/emb.npy` (tiger.py's database
embeds the same texts by the same model inside ChloeDB).

    pip install sentence-transformers
    python codebooks.py --data beauty --dir ~/data/beauty --work ~/work/beauty --out codebooks-beauty.json
"""
import argparse
import json
import os
import tempfile
import time

import numpy as np

import chloedb.train
import tiger


def vectors(a):
    path = os.path.join(a.work, "emb.npy")
    if os.path.exists(path):
        return np.load(path).astype(np.float32)
    from sentence_transformers import SentenceTransformer

    seqs = tiger.sequences(os.path.join(a.dir, tiger.FILES[a.data][0]))
    items = sorted({i for s in seqs.values() for i in s})
    text = tiger.texts(os.path.join(a.dir, tiger.FILES[a.data][1]), set(items))
    model = SentenceTransformer("BAAI/bge-small-en-v1.5", device="cpu")
    emb = model.encode([text.get(i, f"Item {i}") for i in items], batch_size=64, normalize_embeddings=True, show_progress_bar=False).astype(np.float32)
    os.makedirs(a.work, exist_ok=True)
    np.save(path, emb)
    json.dump(items, open(os.path.join(a.work, "items.json"), "w"))
    return emb


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--data", choices=sorted(tiger.FILES), default="beauty")
    ap.add_argument("--dir", required=True)
    ap.add_argument("--work", required=True)
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    emb = vectors(a)
    out = {"data": a.data, "items": len(emb), "dim": int(emb.shape[1]), "machine": tiger.machine(), "codebooks": {}}
    with tempfile.TemporaryDirectory() as models:
        for name, balanced in (("plain", False), ("balanced", True)):
            t = time.time()
            chloedb.train.codebook(emb, name, models=models, levels=3, k=256, balanced=balanced)
            seconds = round(time.time() - t, 1)
            out["codebooks"][name] = {**chloedb.train.codebook_report(emb, name, models=models), "train_seconds": seconds}
            print(name, json.dumps(out["codebooks"][name]), flush=True)
    with open(a.out, "w") as f:
        json.dump(out, f, indent=1)
        f.write("\n")


if __name__ == "__main__":
    main()
