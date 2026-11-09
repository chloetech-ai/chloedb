"""Markdown tables of engine-compare's recommender runs: one table per
kind, one row per engine, from the recsys-run-ENGINE.json reports in a
directory (`local.sh`'s OUT, or this folder's serving/):

    python serving_tables.py serving/
"""
import json
import os
import sys

ENGINES = ["strata", "es", "os", "vespa", "qdrant", "milvus", "redis", "postgres"]
NAMES = {"strata": "Strata", "es": "Elasticsearch", "os": "OpenSearch", "vespa": "Vespa", "qdrant": "Qdrant", "milvus": "Milvus", "redis": "Redis", "postgres": "Postgres"}
SCORES = {"r1": "recall@500", "r2": "recall@1000", "r3": "rows_found", "r4": "precision@50", "r5": "recall@20", "r6": "recall@500"}


def at(runs, c):
    return next((r for r in runs if r["concurrency"] == c), None)


def ms(x):
    return f"{x:.1f}" if x < 100 else f"{x:.0f}"


def main(d):
    reports = {}
    for e in ENGINES:
        p = os.path.join(d, f"recsys-run-{e}.json")
        if os.path.exists(p):
            reports[e] = json.load(open(p))
    if not reports:
        sys.exit(f"no recsys-run-*.json in {d}")
    titles = next(iter(reports.values()))["kinds"]
    for k in SCORES:
        if not any(k in r["kinds"] for r in reports.values()):
            continue
        print(f"\n**{k} {titles[k]['title']}** ({SCORES[k]} of the exact answer)\n")
        if k == "r6":
            print("| engine | score | r1 p50 / p99 at 1, ms | r1 p99 at 16, ms | write p99 at 16, ms | play visible at 16, p50 / p99, ms |")
            print("|---|---|---|---|---|---|")
        else:
            print("| engine | score | p50 / p99 at 1, ms | req/s at 4 | req/s at 16 | p99 at 16, ms |")
            print("|---|---|---|---|---|---|")
        for e, r in reports.items():
            x = r["kinds"].get(k)
            if x is None:
                continue
            score = x.get(SCORES[k])
            s = "errors" if not x.get("answered") else f"{score:.3f}"
            if k == "r6":
                one, top = at(x["runs"], 1), at(x["runs"], 16)
                print(f"| {NAMES[e]} | {s} | {ms(one['reads']['p50_ms'])} / {ms(one['reads']['p99_ms'])} | {ms(top['reads']['p99_ms'])} | {ms(top['writes']['p99_ms'])} | {ms(top['fresh']['p50_ms'])} / {ms(top['fresh']['p99_ms'])} |")
            else:
                one, four, top = (at(x["runs"], c)["latency"] for c in (1, 4, 16))
                print(f"| {NAMES[e]} | {s} | {ms(one['p50_ms'])} / {ms(one['p99_ms'])} | {four['qps']:.0f} | {top['qps']:.0f} | {ms(top['p99_ms'])} |")


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(os.path.abspath(__file__)), "serving"))
