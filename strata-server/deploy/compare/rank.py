#!/usr/bin/env python3
"""Rank the engines of a fanout.sh run by the sum of their places.

    python3 rank.py REPORTS_DIR > rank.md

The tests are each query kind at three measures (p50 and p99 at 1 client,
queries a second at 4 clients), recall@10 for the kinds with an exact answer,
and load time and index size. Each measure is the median of the passes
(run-<engine>.json, -pass2, -pass3, or .gz), so one cold pass does not decide
an engine's place. An engine that cannot run a kind takes last place on it.
Elasticsearch's kinds that need its paid licence come from its trial run
(run-es-trial.json). Recall, load time and size come from summary.json.
"""
import gzip
import json
import statistics
import sys
from pathlib import Path

KINDS = ["bm25", "bm25 + filter", "phrase", "prefix", "typo", "boosted (bm25 × decay)", "facet", "vector",
         "vector + filter", "hybrid (RRF)", "hybrid (weighted)", "similar items", "key lookup", "sql aggregate"]
RECALL = ["vector", "vector + filter", "similar items"]


def load(p):
    if p.exists():
        return json.loads(p.read_text())
    gz = p.with_name(p.name + ".gz")
    return json.loads(gzip.decompress(gz.read_bytes())) if gz.exists() else None


def medians(d, e):
    rows = {}
    for r in (load(d / f"run-{e}{s}.json") for s in ["", "-pass2", "-pass3"]):
        for h in (r or {}).get("http", []):
            if h["requests"] > 0:
                rows.setdefault((h["kind"], h["concurrency"]), []).append(h)
    return {k: {m: statistics.median(h[m] for h in hs) for m in ("p50_ms", "p99_ms", "qps")} | {"passes": len(hs)}
            for k, hs in rows.items()}


def load_seconds(l):
    # Shards built side by side: the load took as long as the slowest. Reports
    # from before analyze.py measured that (start to end) added them up.
    shards = l.get("shards") or []
    if shards and not any("started" in s or "start" in s for s in shards):
        return max(s["total_s"] for s in shards)
    return l["total_s"]


def main():
    d = Path(sys.argv[1])
    s = json.loads((d / "summary.json").read_text())
    engines = [e for e in s["engines"] if e != "es-trial"]
    H = {e: medians(d, e) for e in engines + ["es-trial"]}

    def get(e, k, c):
        return H[e].get((k, c)) or (H["es-trial"].get((k, c)) if e == "es" else None)

    tests = []
    for k in KINDS:
        tests.append((k, "p50", {e: get(e, k, 1)["p50_ms"] for e in engines if get(e, k, 1)}, False))
        tests.append((k, "p99", {e: get(e, k, 1)["p99_ms"] for e in engines if get(e, k, 1)}, False))
        tests.append((k, "q/s", {e: get(e, k, 4)["qps"] for e in engines if get(e, k, 4)}, True))
    Q, L = s["quality"], s["load"]
    for k in RECALL:
        tests.append((k, "recall", {e: Q[e][k]["recall_at_10"] for e in engines
                                    if (Q.get(e, {}).get(k) or {}).get("recall_at_10") is not None}, True))
    tests.append(("load", "time", {e: load_seconds(L[e]) for e in engines if e in L}, False))
    tests.append(("load", "size", {e: L[e].get("index_bytes") or L[e].get("memory_bytes") for e in engines
                                   if e in L and (L[e].get("index_bytes") or L[e].get("memory_bytes"))}, False))
    n = len(engines)
    score = dict.fromkeys(engines, 0)
    ran = dict.fromkeys(engines, 0)
    own = dict.fromkeys(engines, 0)
    firsts = {e: [] for e in engines}
    for k, dim, vals, high in tests:
        for e in engines:
            if e in vals:
                place = 1 + sum(1 for v in vals.values() if (v > vals[e] if high else v < vals[e]))
                ran[e] += 1
                own[e] += place
                if place == 1:
                    firsts[e].append(f"{k} {dim}")
            else:
                place = n
            score[e] += place
    print(f"## Places summed over {len(tests)} tests, on the median of the passes\n")
    print("| place | engine | points | average place | tests run | average place on those | first on |")
    print("|---|---|---|---|---|---|---|")
    for i, e in enumerate(sorted(engines, key=lambda e: score[e])):
        print(f"| {i + 1} | {e} | {score[e]} | {score[e] / len(tests):.2f} | {ran[e]} | {own[e] / max(ran[e], 1):.2f} | "
              f"{', '.join(firsts[e]) or '–'} |")


if __name__ == "__main__":
    main()
