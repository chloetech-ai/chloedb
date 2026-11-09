#!/usr/bin/env python3
"""The spread of fanout.sh's three passes of `run`, per engine and kind.

    python3 passes.py REPORTS_DIR > passes.md

Reads run-<engine>.json, run-<engine>-pass2.json and run-<engine>-pass3.json
(or .json.gz) from REPORTS_DIR and prints, for every engine, kind and
concurrency, the median of the three passes' p50 and p99 latencies and qps,
with the lowest and highest beside each. analyze.py's tables use the first
pass; this says how far the other two land from it.
"""
import gzip
import json
import statistics
import sys
from pathlib import Path


def load(p):
    if p.exists():
        return json.loads(p.read_text())
    gz = p.with_name(p.name + ".gz")
    return json.loads(gzip.decompress(gz.read_bytes())) if gz.exists() else None


def cell(xs, digits):
    if not xs:
        return "n/a"
    m = statistics.median(xs)
    return f"{m:.{digits}f} ({min(xs):.{digits}f} to {max(xs):.{digits}f})" if len(xs) > 1 else f"{m:.{digits}f}"


def main():
    d = Path(sys.argv[1])
    engines = sorted(p.name[4:].split(".")[0] for p in d.glob("run-*.json*") if "-pass" not in p.name and "trial" not in p.name)
    print("## Three passes of every timed query: median (lowest to highest)\n")
    print("| engine | kind | clients | p50 ms | p99 ms | qps | errors |")
    print("|---|---|---|---|---|---|---|")
    for e in engines:
        runs = [r for r in (load(d / f"run-{e}{s}.json") for s in ["", "-pass2", "-pass3"]) if r]
        rows = {}
        for r in runs:
            for h in r.get("http", []):
                rows.setdefault((h["kind"], h["concurrency"]), []).append(h)
        for (kind, c), hs in rows.items():
            ok = [h for h in hs if h["requests"] > 0]
            print(f"| {e} | {kind} | {c} | {cell([h['p50_ms'] for h in ok], 2)} | {cell([h['p99_ms'] for h in ok], 2)} | "
                  f"{cell([h['qps'] for h in ok], 0)} | {sum(h['errors'] for h in hs)} |")
        print(f"| {e} | passes | | {len(runs)} | | | |")


if __name__ == "__main__":
    main()
