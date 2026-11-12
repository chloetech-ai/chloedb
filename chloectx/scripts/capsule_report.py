#!/usr/bin/env python3
"""Pools the handoff runs of scripts/memory_bench.py over its scenarios: does
a capsule help the next agent, and what does it cost in tasks solved,
tokens, money and time?

  python3 scripts/capsule_report.py RESULTS_DIR [--scenarios stockroom slotwise bakehouse kestrel]

RESULTS_DIR holds one directory per scenario (memory_bench.py --results),
each with runs.jsonl, history_*.json and handoff_*.json. Writes
RESULTS_DIR/pooled.json and RESULTS_DIR/pooled.md. Standard library only.
"""

import argparse
import json
import os
import random
import statistics
import sys
from collections import defaultdict

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from ab_stats import fisher  # noqa: E402

# Lanes in the order the tables show them, with what each hands the next agent.
LANES = [
    ("engram+none", "nothing (ChloeCtx off)"),
    ("engram+summary", "a handoff note in the system prompt"),
    ("engram+capsule", "a capsule mounted as MCP tools"),
    ("engram+both", "the note and the capsule"),
    ("engram+short", "a short note (under 1,500 characters)"),
    ("engram+shortcap", "the short note and the capsule"),
    ("engram", "ChloeCtx itself: briefing, recall and memory tools"),
    ("full", "the whole history in context (a fork of it)"),
]
NO_HISTORY_KINDS = ("control", "repo")


def pct(xs, q):
    xs = sorted(xs)
    if not xs:
        return None
    k = (len(xs) - 1) * q
    lo, hi = int(k), min(int(k) + 1, len(xs) - 1)
    return xs[lo] + (xs[hi] - xs[lo]) * (k - lo)


def mean(xs):
    xs = list(xs)
    return sum(xs) / len(xs) if xs else None


def paired(rows, a, b, key, iters, rng):
    """Mean of `key` in lane a minus lane b, and the two-sided p of a
    permutation test shuffling the lane labels within each (scenario, task)."""
    by = defaultdict(lambda: {a: [], b: []})
    for r in rows:
        if r["lane"] in (a, b):
            by[(r["scenario"], r["task"])][r["lane"]].append(float(r[key]))
    cells = [(v[a] + v[b], len(v[a])) for v in by.values() if v[a] and v[b]]
    if not cells:
        return None, None
    na, nb = sum(k for _, k in cells), sum(len(p) - k for p, k in cells)
    total = sum(sum(p) for p, _ in cells)
    sa = sum(sum(p[:k]) for p, k in cells)
    obs = sa / na - (total - sa) / nb
    hits = 0
    for _ in range(iters):
        s = sum(sum(rng.sample(p, k)) for p, k in cells)
        hits += abs(s / na - (total - s) / nb) >= abs(obs) - 1e-12
    return obs, (hits + 1) / (iters + 1)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("results")
    ap.add_argument("--scenarios", nargs="+", default=["stockroom", "slotwise", "bakehouse", "kestrel"])
    ap.add_argument("--iters", type=int, default=20000)
    args = ap.parse_args()
    rng = random.Random(1)

    rows, hand, hist = [], {}, {}
    for sc in args.scenarios:
        d = os.path.join(args.results, sc)
        p = os.path.join(d, "runs.jsonl")
        if not os.path.exists(p):
            continue
        rows += [json.loads(l) for l in open(p)]
        hp = os.path.join(d, "handoff_engram.json")
        hand[sc] = json.load(open(hp)) if os.path.exists(hp) else {}
        for lane in ("engram", "full"):
            hp = os.path.join(d, f"history_{lane}.json")
            if os.path.exists(hp):
                h = json.load(open(hp))
                hist[(sc, lane)] = {"agent_usd": h.get("cost_usd"), "memory_usd": (h.get("memory") or {}).get("cost_usd"), "wall_s": h.get("wall_s")}
    rows = [r for r in rows if not r.get("error")]
    lanes = [(l, what) for l, what in LANES if any(r["lane"] == l for r in rows)]
    needs = lambda r: (r.get("kind") or "") not in NO_HISTORY_KINDS  # noqa: E731

    out = {"scenarios": args.scenarios, "lanes": {}, "handoff": hand, "history": {f"{k[0]}/{k[1]}": v for k, v in hist.items()}, "per_scenario": {}}
    for lane, what in lanes:
        rs = [r for r in rows if r["lane"] == lane]
        nr = [r for r in rs if needs(r)]
        caps = [r.get("capsule") or {} for r in rs]
        calls = [c for cap in caps for c in cap.get("calls", [])]
        out["lanes"][lane] = {
            "what": what, "runs": len(rs), "solved": sum(r["solved"] for r in rs), "needs_history_runs": len(nr), "needs_history_solved": sum(r["solved"] for r in nr),
            "score": mean(r["score"] for r in rs),
            "cost_usd": mean((r["cost_usd"] or 0) for r in rs), "memory_cost_usd": mean((r.get("memory_cost_usd") or 0) for r in rs),
            "input_tokens_p50": pct([r["input_tokens"] for r in rs], 0.5), "input_tokens_mean": mean(r["input_tokens"] for r in rs),
            "uncached_input_tokens_mean": mean(r["input_tokens"] - r.get("cache_read_tokens", 0) for r in rs),
            "output_tokens_mean": mean(r["output_tokens"] for r in rs), "steps_mean": mean((r["steps"] or 0) for r in rs),
            "wall_s_p50": pct([r["wall_s"] for r in rs], 0.5), "wall_s_p95": pct([r["wall_s"] for r in rs], 0.95), "wall_s_mean": mean(r["wall_s"] for r in rs),
            "memory_tool_calls_mean": mean(r["memory_tool_calls"] for r in rs), "runs_with_memory_tool_call": sum(r["memory_tool_calls"] > 0 for r in rs),
            "capsule": None if not any(caps) else {
                "calls": len(calls), "by_tool": {t: sum(c["tool"] == t for c in calls) for t in sorted({c["tool"] for c in calls})},
                "call_ms_p50": pct([c["ms"] for c in calls], 0.5), "call_ms_p95": pct([c["ms"] for c in calls], 0.95), "call_ms_max": max([c["ms"] for c in calls], default=None),
                "ask_ms_p50": pct([c["ms"] for c in calls if c["tool"] == "capsule_ask"], 0.5),
                "ask_chars_mean": mean(c["chars"] for c in calls if c["tool"] == "capsule_ask"),
                "chars_mean": mean(c["chars"] for c in calls),
                "mount_ready_ms_p50": pct([cap["ready_ms"] for cap in caps if cap.get("ready_ms") is not None], 0.5),
                "mount_ready_ms_p95": pct([cap["ready_ms"] for cap in caps if cap.get("ready_ms") is not None], 0.95),
                "open_ms_p50": pct([o["open_ms"] for cap in caps for o in cap.get("opens", [])], 0.5),
            },
        }
        for sc in args.scenarios:
            s = [r for r in rs if r["scenario"] == sc]
            if s:
                out["per_scenario"].setdefault(sc, {})[lane] = {"runs": len(s), "solved": sum(r["solved"] for r in s)}

    comparisons = [("engram+capsule", "engram+none"), ("engram+summary", "engram+none"), ("engram+capsule", "engram+summary"),
                   ("engram+both", "engram+summary"), ("engram+shortcap", "engram+short"), ("engram+short", "engram+summary"),
                   ("engram+both", "engram"), ("engram+both", "full"), ("engram", "full")]
    out["comparisons"] = []
    for a, b in comparisons:
        if a not in out["lanes"] or b not in out["lanes"]:
            continue
        la, lb = out["lanes"][a], out["lanes"][b]
        two, _ = fisher(la["solved"], la["runs"] - la["solved"], lb["solved"], lb["runs"] - lb["solved"])
        d, p = paired(rows, a, b, "solved", args.iters, rng)
        dt, pt = paired(rows, a, b, "input_tokens", args.iters // 4, rng)
        dc, pc = paired(rows, a, b, "wall_s", args.iters // 4, rng)
        out["comparisons"].append({"a": a, "b": b, "solved": [la["solved"], lb["solved"]], "runs": [la["runs"], lb["runs"]], "fisher_p": two,
                                   "solved_diff": d, "perm_p": p, "input_tokens_diff": dt, "input_tokens_p": pt, "wall_s_diff": dc, "wall_s_p": pc})
    json.dump(out, open(os.path.join(args.results, "pooled.json"), "w"), indent=1)

    f = lambda v, p=0: "–" if v is None else f"{v:,.{p}f}"  # noqa: E731
    md = ["# Capsules: what the next agent gets, and what it costs", "",
          f"{len(rows)} runs over {len(out['per_scenario'])} projects ({', '.join(s for s in args.scenarios if s in out['per_scenario'])}). "
          "Every lane but `full` runs on the same recorded history and repository (the `engram` lane's); `full` records its own. "
          "Solved = every check of the task's grader passes. Tokens are the agent's input per run, cached or not. Wall time includes the lanes running beside it.", "",
          "| lane | what the next agent gets | runs | solved | needs an earlier session: solved | $ per run (agent + memory) | input tokens p50 | output tokens | steps | wall s p50 / p95 | runs that called a memory tool |",
          "|---|---|---|---|---|---|---|---|---|---|---|"]
    for lane, d in out["lanes"].items():
        md.append(f"| {lane} | {d['what']} | {d['runs']} | {d['solved']}/{d['runs']} ({100 * d['solved'] / d['runs']:.0f}%) | {d['needs_history_solved']}/{d['needs_history_runs']} | "
                  f"{d['cost_usd']:.4f} + {d['memory_cost_usd']:.4f} | {f(d['input_tokens_p50'])} | {f(d['output_tokens_mean'])} | {f(d['steps_mean'], 1)} | "
                  f"{f(d['wall_s_p50'])} / {f(d['wall_s_p95'])} | {d['runs_with_memory_tool_call']}/{d['runs']} |")
    md += ["", "Paired comparisons (permutation test shuffling lane labels within each task; Fisher's exact test on pooled solved runs):", "",
           "| lane | against | solved | difference in solved rate | p (perm / Fisher) | input tokens, difference per run | wall s, difference per run |", "|---|---|---|---|---|---|---|"]
    for c in out["comparisons"]:
        md.append(f"| {c['a']} | {c['b']} | {c['solved'][0]}/{c['runs'][0]} vs {c['solved'][1]}/{c['runs'][1]} | {100 * c['solved_diff']:+.1f} points | "
                  f"{c['perm_p']:.4f} / {c['fisher_p']:.4f} | {c['input_tokens_diff']:+,.0f} (p {c['input_tokens_p']:.3f}) | {c['wall_s_diff']:+.1f} (p {c['wall_s_p']:.3f}) |")
    md += ["", "The capsule's mount, from its own log (every run starts a new Claude Code session, so a new mount):", "",
           "| lane | capsule calls | by tool | call ms p50 / p95 / max | capsule_ask ms p50 | characters returned per call (mean) | mount ready ms p50 / p95 |", "|---|---|---|---|---|---|---|"]
    for lane, d in out["lanes"].items():
        c = d["capsule"]
        if c:
            md.append(f"| {lane} | {c['calls']} | {', '.join(f'{t} {n}' for t, n in c['by_tool'].items())} | {f(c['call_ms_p50'], 1)} / {f(c['call_ms_p95'], 1)} / {f(c['call_ms_max'], 1)} | "
                      f"{f(c['ask_ms_p50'], 1)} | {f(c['chars_mean'])} | {f(c['mount_ready_ms_p50'])} / {f(c['mount_ready_ms_p95'])} |")
    md += ["", "Making the handoff, once per project:", "",
           "| project | capsule records | capsule bytes | pack ms (backup + cut) | note characters (short note) | note written from (transcript characters) | note $ (short note $) | ChloeCtx's own model calls while recording the history, $ |",
           "|---|---|---|---|---|---|---|---|"]
    for sc, h in hand.items():
        cap, note, short = h.get("capsule") or {}, h.get("note") or {}, h.get("note_short") or {}
        mem = (hist.get((sc, "engram")) or {}).get("memory_usd")
        md.append(f"| {sc} | {f(cap.get('records'))} | {f(cap.get('bytes'))} | {f(cap.get('total_ms'))} | {f(note.get('chars'))} ({f(short.get('chars'))}) | {f(note.get('transcript_chars'))} | "
                  f"{f(note.get('cost_usd'), 3)} ({f(short.get('cost_usd'), 3)}) | {f(mem, 3)} |")
    md += ["", "Solved per project:", "", "| project | " + " | ".join(l for l in out["lanes"]) + " |", "|---|" + "---|" * len(out["lanes"])]
    for sc, by in out["per_scenario"].items():
        md.append(f"| {sc} | " + " | ".join(f"{by[l]['solved']}/{by[l]['runs']}" if l in by else "–" for l in out["lanes"]) + " |")
    open(os.path.join(args.results, "pooled.md"), "w").write("\n".join(md) + "\n")
    print("\n".join(md))


if __name__ == "__main__":
    main()
