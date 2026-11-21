#!/usr/bin/env python3
"""Sums up the personal-memory benchmark: one memory, many readers.

  python3 summarize.py --runs DIR --deep DIR/deep/longmemeval_s_deep_persona.rows.jsonl \
      --profile DIR/profile_4000.jsonl > summary.md

DIR holds reader/<reader>/ and reader/<reader>_full/ (reader_eval.py outputs).
Per reader and context: accuracy, tokens the reader read, dollars per question
(reader, plus the memory's own model calls), and the reader's API seconds.
"""
import argparse, json, os, statistics
from collections import defaultdict

ap = argparse.ArgumentParser()
ap.add_argument("--runs", required=True)
ap.add_argument("--deep", required=True)
ap.add_argument("--profile", required=True)
ap.add_argument("--json", default="")
a = ap.parse_args()

def load(d):
    ans, ver = {}, {}
    p = os.path.join(d, "answers.jsonl")
    if not os.path.exists(p):
        return []
    for l in open(p):
        r = json.loads(l); ans[(r["config"], r["qid"])] = r
    for l in open(os.path.join(d, "verdicts_haiku.jsonl")):
        v = json.loads(l); ver[(v["config"], v["qid"])] = v
    out = []
    for k, r in ans.items():
        if k in ver:
            r = dict(r, correct=ver[k]["correct"], cost_judge=ver[k]["cost_judge"]); out.append(r)
    return out

deep = {r["qid"]: r for r in map(json.loads, open(a.deep))}
prof = {r["qid"]: r for r in map(json.loads, open(a.profile))}
LABEL = {"none": "No memory", "profile": "Plain memory file", "mem0": "mem0", "hindsight_small": "Hindsight, small context", "hindsight": "Hindsight", "search": "ChloeCtx, search", "engine": "ChloeCtx, query engine", "engine16k": "ChloeCtx, query engine, 16,000 characters", "engine48k": "ChloeCtx, query engine, 48,000 characters", "full": "Whole history in context"}
ORDER = ["none", "profile", "mem0", "hindsight_small", "search", "engine", "engine16k", "engine48k", "hindsight", "full"]

table = []
for reader in ["haiku", "sonnet", "opus"]:
    rows = load(os.path.join(a.runs, "reader", reader)) + load(os.path.join(a.runs, "reader", reader + "_full"))
    by = defaultdict(list)
    for r in rows:
        by[r["config"]].append(r)
    for c in ORDER:
        rs = by.get(c)
        if not rs:
            continue
        mem_cost = []
        for r in rs:
            if c.startswith("engine"):
                mem_cost.append(deep[r["qid"]]["cost_usd"])
            else:
                mem_cost.append(0.0)
        t = {
            "reader": reader, "context": c, "label": LABEL[c], "n": len(rs),
            "accuracy": sum(r["correct"] for r in rs) / len(rs),
            "tokens": statistics.mean(r["reader_tokens"] for r in rs),
            "chars": statistics.mean(r["context_chars"] for r in rs),
            "reader_usd": statistics.mean(r["cost_reader"] for r in rs),
            "memory_usd": statistics.mean(mem_cost),
            "seconds": statistics.median(r.get("seconds", 0) for r in rs),
            "models": sorted({m for r in rs for m in (r.get("model") or "").split(",") if m}),
        }
        # model calls to store a person's history (results/v3/competitors for mem0 and Hindsight)
        t["store_usd"] = {"profile": statistics.mean(prof[r["qid"]]["cost"] for r in rs) if c == "profile" else 0.0,
                          "mem0": 12.78 / 100, "hindsight": 100 / 100, "hindsight_small": 100 / 100}.get(c, 0.0)
        if c.startswith("engine"):
            t["memory_seconds"] = statistics.median(deep[r["qid"]]["ms"] / 1000 for r in rs)
        if c == "profile":
            t["memory_seconds"] = statistics.median(prof[r["qid"]]["seconds"] for r in rs)
        table.append(t)
    # the same 25 questions as the full-history subsample, for a like-for-like line
    full_q = {r["qid"] for r in by.get("full", [])}
    if full_q and len(full_q) < 100:
        for c in ORDER:
            rs = [r for r in by.get(c, []) if r["qid"] in full_q]
            if rs and c != "full":
                table.append({"reader": reader, "context": c, "label": LABEL[c] + " (same 25)", "n": len(rs), "accuracy": sum(r["correct"] for r in rs) / len(rs), "subset": True})

print("| reader | context | n | correct | tokens read | $ reader / q | $ memory / q | $ to store a person | reader s (p50) |")
print("|---|---|---|---|---|---|---|---|---|")
for t in table:
    if t.get("subset"):
        print(f"| {t['reader']} | {t['label']} | {t['n']} | {t['accuracy']:.0%} | | | | | |")
        continue
    print(f"| {t['reader']} | {t['label']} | {t['n']} | {t['accuracy']:.0%} | {t['tokens']:,.0f} | {t['reader_usd']:.5f} | {t['memory_usd']:.5f} | {t['store_usd']:.3f} | {t['seconds']:.1f} |")
print()
print("models:", sorted({m for t in table for m in t.get("models", [])}))
if prof:
    ps = list(prof.values())
    print(f"plain memory file: {len(ps)} histories, {statistics.mean(len(p['context']) for p in ps):,.0f} chars on average (max {max(len(p['context']) for p in ps):,}), {statistics.mean(p['calls'] for p in ps):.1f} calls, ${statistics.mean(p['cost'] for p in ps):.4f} and {statistics.mean(p['seconds'] for p in ps):.0f} s of model time per history of {statistics.mean(p['sessions'] for p in ps):.0f} sessions")
print(f"query engine: ${statistics.mean(r['cost_usd'] for r in deep.values()):.5f} per question, {statistics.median(r['ms'] for r in deep.values())/1000:.1f} s median (two model calls)")
if a.json:
    json.dump(table, open(a.json, "w"), indent=1)
