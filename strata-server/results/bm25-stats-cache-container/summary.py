"""Median (lowest to highest) of each build's passes, per kind and clients,
from engine-compare's run-<build>-p<N>.json reports (or .json.gz).
Usage: summary.py DIR [BEFORE AFTER]   (builds; default base cache)"""
import glob
import gzip
import json
import os
import statistics
import sys

d = sys.argv[1]
before, after = sys.argv[2:4] if len(sys.argv) > 3 else ("base", "cache")
rows = {}
for p in sorted(glob.glob(os.path.join(d, "run-*.json*"))):
    r = json.load(gzip.open(p) if p.endswith(".gz") else open(p))
    build = r["label"].rsplit("-p", 1)[0]
    for h in r["http"]:
        rows.setdefault((h["kind"], h["concurrency"], build), []).append(h)


def fmt(vals, f):
    m = statistics.median(vals)
    return f"{m:{f}} ({min(vals):{f}} to {max(vals):{f}})"


kinds = sorted({(k, c) for k, c, _ in rows}, key=lambda x: (x[0], x[1]))
builds = sorted({b for _, _, b in rows})
print("| kind | clients | build | passes | p50 ms | p99 ms | q/s |")
print("|---|---|---|---|---|---|---|")
for k, c in kinds:
    for b in builds:
        hs = rows.get((k, c, b))
        if not hs:
            continue
        print(f"| {k} | {c} | {b} | {len(hs)} | {fmt([h['p50_ms'] for h in hs], '.2f')} | {fmt([h['p99_ms'] for h in hs], '.2f')} | {fmt([h['qps'] for h in hs], '.0f')} |")
print()
print("| kind | clients | q/s before | q/s after | change | p50 before | p50 after |")
print("|---|---|---|---|---|---|---|")
for k, c in kinds:
    a, b = rows.get((k, c, before)), rows.get((k, c, after))
    if not a or not b:
        continue
    qa, qb = statistics.median([h["qps"] for h in a]), statistics.median([h["qps"] for h in b])
    pa, pb = statistics.median([h["p50_ms"] for h in a]), statistics.median([h["p50_ms"] for h in b])
    print(f"| {k} | {c} | {qa:.0f} | {qb:.0f} | {100 * (qb / qa - 1):+.0f}% | {pa:.2f} | {pb:.2f} |")
