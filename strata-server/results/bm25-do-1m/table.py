"""Before and after, from two passes.md tables (deploy/compare/passes.py).

    python3 table.py BEFORE.md ENGINE AFTER.md ENGINE [KIND,...]

prints each kind and concurrency's median q/s and p50 of both, and the
change in q/s; KINDS (comma separated) keeps only those kinds."""
import sys


def read(path, engine):
    rows = {}
    for line in open(path):
        c = [x.strip() for x in line.strip().strip("|").split("|")]
        if len(c) == 7 and c[0] == engine and c[2].isdigit():
            rows[(c[1], int(c[2]))] = (float(c[5].split()[0]), float(c[3].split()[0]))
    return rows


before, after = read(sys.argv[1], sys.argv[2]), read(sys.argv[3], sys.argv[4])
kinds = sys.argv[5].split(",") if len(sys.argv) > 5 else None
print("| kind | clients | q/s before | q/s after | change | p50 ms before | p50 ms after |")
print("|---|---|---|---|---|---|---|")
for (kind, c), (q0, p0) in before.items():
    if (kind, c) not in after or (kinds and kind not in kinds):
        continue
    q1, p1 = after[(kind, c)]
    print(f"| {kind} | {c} | {q0:,.0f} | {q1:,.0f} | {100 * (q1 / q0 - 1):+.0f}% | {p0:.2f} | {p1:.2f} |")
