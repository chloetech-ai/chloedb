"""Prints costs.md's tables of the 40 queries from what before_after.sh
wrote: before_after.py DIR BEFORE AFTER (the two NAMEs)."""
import json
import os
import statistics
import sys

DIR, BEFORE, AFTER = sys.argv[1:4]
# fm_time.py's first asking of each statement: the filter cache has not
# kept the pattern's rows yet (it keeps them the second time).
STMTS = [("fm_1", "first 20"), ("fm_count_1", "count")]


def load(name, run):
    return json.load(open(os.path.join(DIR, f"{name}-{run}.json")))


def pct(xs, p):
    xs = sorted(xs)
    k = (len(xs) - 1) * p
    f, c = int(k), min(int(k) + 1, len(xs) - 1)
    return xs[f] + (xs[c] - xs[f]) * (k - f)


def summary():
    print("| page cache | statement | p50, ms | p90, ms | p99, ms | max, ms | sum, ms |")
    print("|---|---|---|---|---|---|---|")
    for run in ("warm", "cold"):
        b, a = load(BEFORE, run), load(AFTER, run)
        for key, label in STMTS:
            xb = [r[key] for r in b["rows"]]
            xa = [r[key] for r in a["rows"]]
            cells = [f"{pct(xb, p):.1f} → {pct(xa, p):.1f}" for p in (0.5, 0.9, 0.99)]
            print(f"| {run} | {label} | " + " | ".join(cells) + f" | {max(xb):.0f} → {max(xa):.0f} | {sum(xb):.0f} → {sum(xa):.0f} |")


def same():
    out = []
    for run in ("warm", "cold"):
        b, a = load(BEFORE, run), load(AFTER, run)
        for x, y in zip(b["rows"], a["rows"]):
            assert x["q"] == y["q"]
            out.append(x.get("ids") == y.get("ids") and x.get("count") == y.get("count"))
    return all(out), len(out)


def slowest():
    b, a = load(BEFORE, "cold"), load(AFTER, "cold")
    bw, aw = load(BEFORE, "warm"), load(AFTER, "warm")
    rows = sorted(range(len(b["rows"])), key=lambda i: -max(b["rows"][i]["fm_1"], b["rows"][i]["fm_count_1"], bw["rows"][i]["fm_1"], bw["rows"][i]["fm_count_1"]))
    print("| query | titles | first 20 warm, ms | count warm, ms | first 20 cold, ms | count cold, ms |")
    print("|---|---|---|---|---|---|")
    for i in rows[:10]:
        q = b["rows"][i]
        cells = [f"{bw['rows'][i][k]:.1f} → {aw['rows'][i][k]:.1f}" for k, _ in STMTS] + [f"{b['rows'][i][k]:.1f} → {a['rows'][i][k]:.1f}" for k, _ in STMTS]
        print(f"| `{q['q']}` | {q['count']:,} | " + " | ".join(cells) + " |")


def late():
    def read(name):
        d = {}
        for line in open(os.path.join(DIR, f"{name}-late.txt")):
            p, port, ms, n = line.rstrip("\n").split("\t")
            d.setdefault(p, []).append(float(ms))
        return d

    b, a = read(BEFORE), read(AFTER)
    print("| pattern | median of the 4 shards, ms | slowest shard, ms |")
    print("|---|---|---|")
    for p in b:
        print(f"| `{p}` | {statistics.median(b[p]):.1f} → {statistics.median(a[p]):.1f} | {max(b[p]):.1f} → {max(a[p]):.1f} |")


ok, n = same()
print(f"Same rows and counts in all {n} pairs: {ok}")
print()
summary()
print()
slowest()
print()
late()
