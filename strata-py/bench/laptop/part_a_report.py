"""Part A's reports as Markdown tables: python3 part_a_report.py DIR [--compare NAME=DIR ...]

DIR holds the JSON reports a1_app.py, a2_search.py, a_writes.py and
a3_recsys.py wrote (a1_<system>.json, a2_<system>_<collection>.json,
writes_<system>.json, a3_<system>.json). Every latency is shown as p50 and
p99 of one thread; throughput at 4 threads where the report has it. Each
--compare adds Strata's numbers from another directory (an earlier build)
beside the ones in DIR.
"""

from __future__ import annotations

import argparse
import glob
import json
import os
from typing import Any

A1_ORDER = ["strata", "sqlite-full", "sqlite-normal", "duckdb", "tantivy"]
A2_ORDER = ["strata", "sqlite", "tantivy", "duckdb", "lancedb"]
A2_COLLECTIONS = ["scifact", "nfcorpus", "fiqa", "trec-covid", "quora"]
W_ORDER = ["strata", "sqlite-full", "sqlite-normal", "duckdb", "lancedb", "tantivy"]
A3_ORDER = ["strata", "lancedb", "sqlite-vec", "faiss-sqlite"]


def load(path: str) -> dict | None:
    try:
        with open(path) as f:
            return json.load(f)
    except FileNotFoundError:
        return None


def ms(x: Any) -> str:
    if x is None:
        return "-"
    if x < 1:
        return f"{x * 1000:.0f} µs"
    if x < 100:
        return f"{x:.2f} ms" if x < 10 else f"{x:.1f} ms"
    return f"{x:,.0f} ms"


def lat(s: dict | None) -> str:
    """p50 / p99 of a timing entry."""
    if not s:
        return "-"
    return f"{ms(s.get('p50_ms'))} / {ms(s.get('p99_ms'))}"


def rate(x: Any) -> str:
    return "-" if x is None else f"{x:,.0f}/s"


def mb(b: Any) -> str:
    return "-" if b is None else f"{b / 1e6:,.0f} MB"


def num(x: Any, digits: int = 3) -> str:
    return "-" if x is None else f"{x:.{digits}f}"


def table(head: list[str], rows: list[list[str]]) -> str:
    out = ["| " + " | ".join(head) + " |", "|" + "|".join("---" for _ in head) + "|"]
    out += ["| " + " | ".join(r) + " |" for r in rows]
    return "\n".join(out)


def g(d: dict | None, *path: str) -> Any:
    for p in path:
        if not isinstance(d, dict):
            return None
        d = d.get(p)
    return d


def version(d: dict) -> str:
    return d.get("version") or ""


def a1(dirs: list[tuple[str, str]]) -> str:
    rows_r, rows_q, rows_w = [], [], []
    for label, d in dirs:
        for s in A1_ORDER if label == "" else ["strata"]:
            r = load(os.path.join(d, f"a1_{s}.json"))
            if r is None:
                continue
            name = s + (f" ({label})" if label else "") + (f", {version(r)}" if version(r) and not s.startswith("strata") else "")
            get = r["get"]
            rows_r.append([name, lat(get.get("product")), lat(get.get("product_by_key")), rate(g(get, "product_4threads", "qps")), lat(get.get("products_100")), lat(get.get("products_100_by_key")), lat(get.get("user_reviews")), lat(g(r, "page", "0")), lat(g(r, "page", "10000"))])
            rows_q.append([name, rate(g(r, "load", "reviews", "rows_per_s")), mb(g(r, "load", "bytes")), lat(g(r, "aggregate", "brand")), lat(g(r, "aggregate", "month")), lat(g(r, "join", "top")), lat(g(r, "join", "user"))])
            m = r["mixed"]
            rows_w.append([name, rate(g(r, "insert", "one_per_txn", "rows_per_s")), rate(g(r, "insert", "1000_per_txn", "rows_per_s")), rate(g(r, "update", "price_stock", "rows_per_s")), rate(g(r, "update", "counter", "rows_per_s")), rate(g(r, "delete", "by_id", "rows_per_s")), f"{rate(g(m, 'reads', 'qps'))}, p99 {ms(g(m, 'reads', 'p99_ms'))}", f"{rate(g(m, 'writes', 'qps'))}, p99 {ms(g(m, 'writes', 'p99_ms'))}", str(m.get("errors", "-"))])
    out = ["## A1: the app (Shop, 137,260 products and 4,624,176 reviews)", "", "Reads (p50 / p99, one thread):", ""]
    out.append(table(["system", "get by id (SQL)", "get by key (no SQL)", "get, 4 threads", "100 by id (SQL)", "100 by key", "a user's reviews", "page 1", "page 10,000"], rows_r))
    out += ["", "Loads, aggregates and joins:", ""]
    out.append(table(["system", "load reviews", "file", "reviews by brand", "reviews by month", "join: top products", "join: a user's"], rows_q))
    out += ["", "Writes (one row per transaction unless noted) and a mixed run of 2 readers and 1 writer:", ""]
    out.append(table(["system", "insert", "insert, 1,000 per txn", "update price", "counter + 1", "delete by id", "mixed reads", "mixed writes", "errors"], rows_w))
    return "\n".join(out)


def a2(dirs: list[tuple[str, str]]) -> str:
    out = ["## A2: search (BEIR, bge-small 384-d vectors)", "", "Each cell is p50 / p99 on one thread, then nDCG@10 (vector: and recall@10 against exact search). Hybrid is RRF of BM25 and vectors at depth 100.", ""]
    for c in A2_COLLECTIONS:
        rows = []
        n_docs = None
        for label, d in dirs:
            for s in A2_ORDER if label == "" else ["strata"]:
                r = load(os.path.join(d, f"a2_{s}_{c}.json"))
                if r is None:
                    continue
                n_docs = r.get("n_docs")
                k = r["kinds"]

                def cell(kind: str, recall: bool = False) -> str:
                    e = k.get(kind) or {}
                    if e.get("support") == "not supported" or not e.get("warm1"):
                        return "-"
                    t = lat(e.get("warm1"))
                    if e.get("ndcg10") is not None:
                        t += f" · {e['ndcg10']:.3f}"
                    if recall and e.get("recall10") is not None:
                        t += f" · r {e['recall10']:.3f}"
                    return t

                name = s + (f" ({label})" if label else "")
                rows.append([name, f"{r['build']['seconds']:.1f} s", mb(r["build"].get("bytes")), cell("bm25"), cell("vector", True), cell("hybrid"), cell("filtered_hybrid"), rate(g(k, "hybrid", "warm4", "qps"))])
        if rows:
            out += [f"### {c} ({n_docs:,} documents)" if n_docs else f"### {c}", ""]
            out.append(table(["system", "build", "size", "BM25", "vector", "hybrid", "filtered hybrid", "hybrid, 4 threads"], rows))
            out.append("")
    return "\n".join(out)


def writes(dirs: list[tuple[str, str]]) -> str:
    rows, srows = [], []
    for label, d in dirs:
        for s in W_ORDER if label == "" else ["strata"]:
            r = load(os.path.join(d, f"writes_{s}.json"))
            if r is None:
                continue
            name = s + (f" ({label})" if label else "")
            ins, up = r["insert"], r["update"]
            rows.append([name, f"{r['load']['seconds']:.1f} s", rate(g(ins, "1", "rows_per_s")), rate(g(ins, "100", "rows_per_s")), rate(g(ins, "1000", "rows_per_s")), rate(g(ins, "10000", "rows_per_s")), rate(g(up, "price_stock", "rows_per_s")), rate(g(up, "title", "rows_per_s")), rate(g(up, "vector", "rows_per_s")), rate(g(up, "upsert", "rows_per_s")), rate(g(up, "delete", "rows_per_s"))])
            for p in r["stream"]["phases"]:
                srows.append([name, f"{p['rate']:,}/s" if isinstance(p["rate"], int) else str(p["rate"]), rate(p.get("events_per_s")), ms(g(p, "commit", "p50_ms")), ms(g(p, "lag", "p99_ms")), f"{rate(g(p, 'reads', 'qps'))}, p99 {ms(g(p, 'reads', 'p99_ms'))}"])
    out = ["## Writes, updates and streams (137,260 products with a text index and a vector index)", "", "Rows per second, one writer:", ""]
    out.append(table(["system", "load", "insert 1/txn", "100/txn", "1,000/txn", "10,000/txn", "update price", "update title", "update vector", "upsert row", "delete"], rows))
    out += ["", "A stream of new rows at a target rate, with a reader running:", ""]
    out.append(table(["system", "target", "reached", "commit p50", "lag p99", "reads"], srows))
    return "\n".join(out)


def a3(dirs: list[tuple[str, str]]) -> str:
    rows = []
    for label, d in dirs:
        for s in A3_ORDER if label == "" else ["strata"]:
            r = load(os.path.join(d, f"a3_{s}.json"))
            if r is None:
                continue
            name = s + (f" ({label})" if label else "")
            sim, feed = r.get("similar") or {}, r.get("feed") or {}
            rows.append([name, f"{r['build']['seconds']:.1f} s", mb(r["build"].get("bytes")), lat(sim.get("warm1")), num(sim.get("recall10")), rate(g(sim, "warm4", "qps")), lat(feed.get("warm1")) if feed else "-"])
    out = ["## A3: recommendations (similar products under filters; a feed fusing vectors with a walk of the co-purchase graph)", ""]
    out.append(table(["system", "build", "size", "similar", "similar recall@10", "similar, 4 threads", "feed"], rows))
    return "\n".join(out)


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("dir")
    ap.add_argument("--compare", action="append", default=[], help="NAME=DIR: Strata's reports of another build")
    a = ap.parse_args()
    dirs = [("", a.dir)] + [tuple(c.split("=", 1)) for c in a.compare]  # type: ignore[misc]
    first = next(iter(sorted(glob.glob(os.path.join(a.dir, "*.json")))), None)
    m = load(first)["machine"] if first else {}
    print(f"Machine: {m.get('node_size', m.get('host', ''))} {m.get('node_region', '')}, {m.get('cpu', '')}, {m.get('cpus', '')} vCPU, {m.get('memory_bytes', 0) / 2**30:.0f} GiB, {m.get('os', '')}, Python {m.get('python', '')}.\n")
    for part in (a1, a2, writes, a3):
        print(part(dirs))
        print()


if __name__ == "__main__":
    main()
