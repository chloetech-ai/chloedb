#!/usr/bin/env python3
"""One table of every memory system on every benchmark, from the results the
runs wrote: reader_eval.py (LoCoMo, LongMemEval-S), bench_eval.py (BEAM) and
memory_bench.py (the coding A/B), plus what building each system's memory
cost (the *.summary.json files the context builders write).

  python3 scripts/memory_baselines_table.py --spec results/baselines/table.json --out results/baselines/TABLE.md

The spec names, for each benchmark, the summary files (several = several
gradings of the same answers, reported as their mean and range) and which
config of them is which system; and, per system, the files that hold what its
memory cost to build:

  {"systems": ["ChloeCtx", "Zep (Graphiti)", ...],
   "benchmarks": {
     "locomo": {"title": "LoCoMo (J, cat. 1-4)", "summaries": ["results/baselines/locomo/g1/summary.json"],
                "configs": {"ChloeCtx": "chloectx", ...}, "metric": "accuracy",
                "build": {"ChloeCtx": "results/baselines/locomo/chloectx.build.json", ...}},
     "beam": {..., "metric": "mean_over_types"},
     "coding": {"title": "Coding A/B (solved)", "summaries": ["results/memory_bench/bakehouse/summary.json", ...],
                "configs": {"ChloeCtx": "engram@main", ...}, "metric": "solved_rate"}}}

A build file is any JSON with `cost_usd` (and optionally `histories`); the
cell shows the cost per history. Coding A/B summaries are pooled over their
scenarios by runs. Missing cells are shown as "not run".
"""

import argparse
import json
import os
import statistics


def load(path):
    with open(path) as f:
        return json.load(f)


def reader_cell(bench, system):
    name = bench["configs"].get(system)
    if not name:
        return None
    vals, n, chars, cost = [], 0, [], []
    for p in bench["summaries"]:
        if not os.path.exists(p):
            continue
        c = load(p)["configs"].get(name)
        if not c or not c.get("n"):
            continue
        vals.append(c[bench.get("metric", "accuracy")])
        n = c["n"]
        chars.append(c.get("context_chars", 0))
        cost.append(c.get("cost_usd", 0.0))
    if not vals:
        return None
    return {"mean": statistics.mean(vals), "lo": min(vals), "hi": max(vals), "gradings": len(vals), "n": n,
            "context_chars": statistics.mean(chars), "read_cost": statistics.mean(cost) / max(1, n)}


def coding_cell(bench, system):
    name = bench["configs"].get(system)
    if not name:
        return None
    runs = solved = 0
    cost = mem = 0.0
    hist = 0.0
    for p in bench["summaries"]:
        if not os.path.exists(p):
            continue
        lane = load(p)["lanes"].get(name)
        if not lane:
            continue
        runs += lane["runs"]
        solved += lane["solved"]
        cost += lane["cost_usd"] * lane["runs"]
        mem += lane.get("memory_cost_usd", 0.0) * lane["runs"]
        hist += (lane.get("history") or {}).get("memory_cost_usd", 0.0)
    if not runs:
        return None
    return {"mean": solved / runs, "lo": None, "hi": None, "n": runs, "run_cost": (cost + mem) / runs, "history_memory_cost": hist}


def build_cell(bench, system):
    p = (bench.get("build") or {}).get(system)
    if not p or not os.path.exists(p):
        return None
    d = load(p)
    h = d.get("histories") or 1
    return d["cost_usd"] / h


def fmt_score(c):
    if not c:
        return "not run"
    s = f"{100 * c['mean']:.1f}"
    if c.get("gradings", 1) > 1:
        s += f" ({100 * c['lo']:.1f}-{100 * c['hi']:.1f})"
    return s


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--spec", required=True)
    ap.add_argument("--out", required=True)
    args = ap.parse_args()
    spec = load(args.spec)
    benches = spec["benchmarks"]
    systems = spec["systems"]
    cells = {}
    for key, b in benches.items():
        for s in systems:
            cells[(key, s)] = coding_cell(b, s) if key == "coding" else reader_cell(b, s)

    lines = ["| system | " + " | ".join(b["title"] for b in benches.values()) + " |",
             "|---|" + "---:|" * len(benches)]
    for s in systems:
        lines.append(f"| {s} | " + " | ".join(fmt_score(cells[(k, s)]) for k in benches) + " |")
    lines += ["", "What it cost: building the memory (per history), reading (per question: the reader and judge, or the agent), and context size.", "",
              "| system | " + " | ".join(f"{b['title'].split(' (')[0]}" for k, b in benches.items()) + " |",
              "|---|" + "---|" * len(benches)]
    for s in systems:
        row = []
        for k, b in benches.items():
            c = cells[(k, s)]
            if not c:
                row.append("")
                continue
            if k == "coding":
                row.append(f"${c['run_cost']:.3f}/run, history memory ${c['history_memory_cost']:.2f}")
                continue
            built = build_cell(b, s)
            parts = [f"build ${built:.3f}" if built is not None else None, f"read ${c['read_cost']:.4f}", f"{c['context_chars']:,.0f} chars"]
            row.append(", ".join(p for p in parts if p))
        lines.append(f"| {s} | " + " | ".join(row) + " |")
    ns = {k: max((cells[(k, s)] or {}).get("n", 0) for s in systems) for k in benches}
    lines += ["", "Questions (or runs): " + ", ".join(f"{benches[k]['title'].split(' (')[0]} {n}" for k, n in ns.items()) + "."]
    os.makedirs(os.path.dirname(os.path.abspath(args.out)), exist_ok=True)
    with open(args.out, "w") as f:
        f.write("\n".join(lines) + "\n")
    with open(os.path.splitext(args.out)[0] + ".json", "w") as f:
        json.dump({f"{k}|{s}": v for (k, s), v in cells.items()}, f, indent=1)
    print("\n".join(lines))


if __name__ == "__main__":
    main()
