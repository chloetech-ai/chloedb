#!/usr/bin/env python3
"""Prints README.md's tables from the JSON `examples/substring_bench.rs`
wrote: `tables.py ssh.json android.json bgl.json`."""
import json
import sys


def mb(x):
    return f"{x / 1e6:.1f}"


def ms(x):
    return f"{x:.2f}" if x < 10 else f"{x:.0f}"


runs = [(p.rsplit("/", 1)[-1].split(".")[0], json.load(open(p))) for p in sys.argv[1:]]

print("| log | lines | bodies, MB | file without, MB | file with, MB | index, MB | of which transform | samples | index / bodies | parts | bytes a run |")
print("|---|---|---|---|---|---|---|---|---|---|---|")
for name, r in runs:
    s = r["substring"]["sub_body"]
    print(
        f"| {name} | {r['lines']:,} | {mb(r['body_bytes'])} | {mb(r['plain']['file_bytes'])} | {mb(r['substring']['file_bytes'])} "
        f"| {mb(s['bytes'])} | {mb(s['bwt_bytes'])} | {mb(s['sa_bytes'])} | {s['bytes'] / r['body_bytes']:.2f} | {s['parts']} | {s['text_bytes'] / s['runs']:.1f} |"
    )
print()
print("| log | load without, s | load with, s | merge without, s | merge with, s |")
print("|---|---|---|---|---|")
for name, r in runs:
    p, s = r["plain"], r["substring"]
    print(f"| {name} | {p['load_seconds']} | {s['load_seconds']} | {p['merge_seconds']} | {s['merge_seconds']} |")
for name, r in runs:
    print()
    print(f"{name}:")
    print()
    print("| condition | rows | count without, ms | count with, ms | first 10 without, ms | first 10 with, ms |")
    print("|---|---|---|---|---|---|")
    for q in r["queries"]:
        cond = q["pattern"] if q["op"] == "WHERE" else f"body {q['op']} '{q['pattern']}'"
        c, f = q["count_ms"], q["first_10_ms"]
        print(f"| `{cond}` | {q['rows']:,} | {ms(c['scan'])} | {ms(c['index'])} | {ms(f['scan'])} | {ms(f['index'])} |")
