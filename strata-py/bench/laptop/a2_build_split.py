"""How much of Strata's A2 build time is the vector index: Strata built as
a2_search.py builds it (text, filters and a 384-d vector), Strata without the
vector field, and Tantivy (text and filters, as a2_search.py builds it, and
again storing the title and text the way Strata stores them), on the same
documents, one after the other on an otherwise idle machine.

    python3 a2_build_split.py --data DIR --collections scifact,fiqa --work DIR --out DIR [--repeat 2]

Writes build_split_<collection>.json: per variant, each repeat's seconds
(Strata's split into inserts and optimize()), and the file size.
"""

from __future__ import annotations

import argparse
import shutil
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from common import Collection, body, files_bytes, write_report  # noqa: E402

STRATA_FIELDS = [
    {"name": "id", "type": "keyword"},
    {"name": "title", "type": "text", "indexed": False, "positions": False},
    {"name": "text", "type": "text", "indexed": False, "positions": False},
    {"name": "body", "type": "text", "from": ["title", "text"]},
    {"name": "year", "type": "int"},
    {"name": "category", "type": "keyword"},
    {"name": "rating", "type": "float"},
    {"name": "topic", "type": "keyword"},
]
COLS = ("id", "title", "text", "year", "category", "rating", "topic")


def remove(path: Path) -> None:
    shutil.rmtree(path, ignore_errors=True)
    for p in (path, Path(str(path) + "-wal")):
        if p.is_file():
            p.unlink()


def strata_build(coll: Collection, path: Path, vectors: bool) -> dict:
    import strata

    fields = STRATA_FIELDS + ([{"name": "emb", "type": "vector", "dim": coll.dim}] if vectors else [])
    t0 = time.perf_counter()
    db = strata.open(str(path), schema={"table": "docs", "key": "id", "fields": fields})
    for start in range(0, len(coll.docs), 2000):
        rows = []
        for i in range(start, min(start + 2000, len(coll.docs))):
            d = coll.docs[i]
            row = {k: d[k] for k in COLS}
            if vectors:
                # A numpy row, as an app holds it (strata-py reads its bytes).
                row["emb"] = coll.vecs[i]
            rows.append(row)
        db.insert(rows)
    t1 = time.perf_counter()
    db.optimize()
    t2 = time.perf_counter()
    db.close()
    return {"seconds": t2 - t0, "insert_seconds": t1 - t0, "optimize_seconds": t2 - t1, "bytes": files_bytes([path, Path(str(path) + "-wal")])}


def tantivy_build(coll: Collection, path: Path, store_text: bool) -> dict:
    import tantivy as t

    sb = t.SchemaBuilder()
    sb.add_text_field("id", stored=True, tokenizer_name="raw")
    sb.add_text_field("body", stored=False, tokenizer_name="en_stem", index_option="position")
    if store_text:
        # Stored, not indexed, as Strata keeps them (tantivy-py's text
        # fields are always indexed, its bytes fields need not be).
        sb.add_bytes_field("title", stored=True, indexed=False)
        sb.add_bytes_field("text", stored=True, indexed=False)
    sb.add_integer_field("year", indexed=True, fast=True)
    sb.add_float_field("rating", indexed=True, fast=True)
    sb.add_text_field("category", stored=False, tokenizer_name="raw", fast=True)
    sb.add_text_field("topic", stored=False, tokenizer_name="raw", fast=True)
    schema = sb.build()
    path.mkdir(parents=True, exist_ok=True)
    t0 = time.perf_counter()
    index = t.Index(schema, path=str(path))
    w = index.writer(heap_size=512_000_000)
    for d in coll.docs:
        extra = {"title": d["title"].encode(), "text": d["text"].encode()} if store_text else {}
        w.add_document(t.Document(id=d["id"], body=body(d), year=int(d["year"]), rating=float(d["rating"]), category=d["category"], topic=d["topic"], **extra))
    w.commit()
    w.wait_merging_threads()
    index.reload()
    return {"seconds": time.perf_counter() - t0, "bytes": files_bytes([path])}


VARIANTS = {
    "strata": lambda c, p: strata_build(c, p, True),
    "strata_text_only": lambda c, p: strata_build(c, p, False),
    "tantivy": lambda c, p: tantivy_build(c, p, False),
    "tantivy_stored_text": lambda c, p: tantivy_build(c, p, True),
}


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--data", required=True)
    ap.add_argument("--collections", default="scifact")
    ap.add_argument("--variants", default=",".join(VARIANTS))
    ap.add_argument("--work", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--repeat", type=int, default=2)
    a = ap.parse_args()
    work = Path(a.work)
    work.mkdir(parents=True, exist_ok=True)
    for name in a.collections.split(","):
        coll = Collection.load(a.data, name)
        rep: dict = {"collection": name, "n_docs": len(coll.docs), "variants": {}}
        for v in a.variants.split(","):
            runs = []
            for r in range(a.repeat):
                path = work / f"{v}_{name}_{r}"
                remove(path)
                runs.append(VARIANTS[v](coll, path))
                remove(path)
                print(f"{name:10} {v:20} {runs[-1]['seconds']:8.1f} s  {runs[-1]['bytes'] / 1e6:8.0f} MB", flush=True)
            rep["variants"][v] = {"runs": runs, "best_seconds": min(x["seconds"] for x in runs)}
        write_report(Path(a.out) / f"build_split_{name}.json", rep)


if __name__ == "__main__":
    main()
