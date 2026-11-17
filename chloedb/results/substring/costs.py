"""Prints costs.md's tables from costs-ssh.jsonl and costs-hn.jsonl, the
lines `examples/substring_costs.rs` printed: what a row walked, a value
tested and a byte scanned cost, and how the rule of `like::LOCATE_BYTES`,
`EXACT_ROWS` and `GENERAL_SCAN` chooses between listing and scanning for
each value of the first two.

    python3 costs.py
"""
import json
import os
import statistics

HERE = os.path.dirname(os.path.abspath(__file__))
SETS = [("ssh", "Loghub SSH, `body`", "costs-ssh.jsonl"), ("hn", "Hacker News, `title`", "costs-hn.jsonl")]


def load(name):
    return [json.loads(line) for line in open(os.path.join(HERE, name))]


def general(j):
    # What the row evaluator tests: a `_`, or more than one literal.
    p = j["pattern"]
    return j["plan"] == "pieces" and ("_" in p or "%" in p.strip("%"))


def choose(j, locate_bytes, exact_rows, general_scan=2):
    """What `like::too_many` picks for the whole index, as if it were one part."""
    lb = max(locate_bytes * j["bytes"] // j["stored"], exact_rows)
    walk = lb // exact_rows
    test = 0 if j["plan"] in ("exact", "spelled") else lb - walk
    byte = general_scan if general(j) else 1
    lst = j["list"]
    return "scan" if lst["located"] * walk + lst["read"] * test > j["bytes"] * byte else "list"


def ms(j, way):
    return j[way]["ms"]


def med(xs):
    return f"{statistics.median(xs):.2f} ({min(xs):.2f} to {max(xs):.2f})" if xs else "-"


def unit_costs(rows):
    print("| data | plan | patterns | scan of every value, ns a byte of the rows | patterns listed with 100 rows or more | list, µs a row walked | list, µs a value tested |")
    print("|---|---|---|---|---|---|---|")
    for key, label, _ in SETS:
        for plan in ("exact", "spelled", "pieces"):
            rs = [j for j in rows[key] if j["plan"] == plan]
            if not rs:
                continue
            scan = [ms(j, "scan") * 1e6 / j["stored"] for j in rs if j["scan"]["read"] == j["docs"]]
            walked = [ms(j, "list") * 1e3 / j["list"]["located"] for j in rs if j["list"]["read"] == 0 and j["list"]["located"] >= 100]
            tested = [ms(j, "list") * 1e3 / j["list"]["read"] for j in rs if j["list"]["read"] >= 100]
            many = sum(1 for j in rs if j["list"]["located"] >= 100)
            print(f"| {label} | {plan} | {len(rs)} | {med(scan)} | {many} | {med(walked)} | {med(tested)} |")


def sweep(rows):
    print("| `LOCATE_BYTES` | `EXACT_ROWS` | " + " | ".join(f"{label}: time chosen / fastest, patterns 15% and 0.5 ms slower" for _, label, _ in SETS) + " |")
    print("|---|---|" + "---|" * len(SETS))
    for lb in (1024, 2048, 3072, 4096, 6144, 8192):
        for er in (2, 3, 4):
            cells = []
            for key, _, _ in SETS:
                chosen = best = 0.0
                worse = 0
                for j in rows[key]:
                    t = ms(j, choose(j, lb, er))
                    b = min(ms(j, "list"), ms(j, "scan"))
                    chosen += t
                    best += b
                    worse += t > 1.15 * b and t - b > 0.5
                cells.append(f"{chosen / best:.3f}, {worse}")
            mark = " **(the rule)**" if (lb, er) == (4096, 2) else ""
            print(f"| {lb}{mark} | {er} | " + " | ".join(cells) + " |")


def patterns(rows, key, least):
    print("| op | pattern | plan | rows walked | values tested | matches | list, ms | scan, ms | the rule |")
    print("|---|---|---|---|---|---|---|---|---|")
    for j in rows[key]:
        if j["list"]["located"] < least:
            continue
        pick = choose(j, 4096, 2)
        faster = "list" if ms(j, "list") <= ms(j, "scan") else "scan"
        print(f"| {j['op']} | `{j['pattern']}` | {j['plan']} | {j['list']['located']:,} | {j['list']['read']:,} | {j['list']['matches']:,} | {ms(j, 'list'):,.1f} | {ms(j, 'scan'):,.1f} | {pick}{'' if pick == faster else ' (slower)'} |")


def main():
    rows = {key: load(f) for key, _, f in SETS}
    for key, label, _ in SETS:
        j = rows[key][0]
        print(f"- {label}: {j['docs']:,} values, {j['bytes']:,} bytes of them in the index, rows of {j['stored']:,} bytes, {len(rows[key])} patterns")
    print()
    unit_costs(rows)
    print()
    sweep(rows)
    print()
    patterns(rows, "ssh", 0)
    print()
    patterns(rows, "hn", 100)


main()
