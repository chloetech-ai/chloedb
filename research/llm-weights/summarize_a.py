"""Prototype A's table: per model, prompt and grammar, the share of queries
that ran, the share answered right (search questions: recall of the gold
top 5), output tokens and seconds per question."""
import json, glob, collections
rows = []
for f in sorted(glob.glob("research/llm-weights/results/a_*.json")):
    rows += json.load(open(f))
g = collections.defaultdict(list)
for r in rows: g[(r["model"], r["prompt"], r["grammar"])].append(r)
print("| model | prompt | grammar | ran | correct (answers) | recall@5 (search) | tokens | s/question |")
print("|---|---|---|---|---|---|---|---|")
for (m, p, gr), rs in sorted(g.items()):
    ans = [r for r in rs if r["kind"] == "answer"]; se = [r for r in rs if r["kind"] == "search"]
    print(f"| {m.split('-Q')[0]} | {p} | {'on' if gr else 'off'} | {sum(r['ran'] for r in rs)/len(rs):.0%} | "
          f"{sum(r['score'] for r in ans)/len(ans):.0%} | {sum(r['score'] for r in se)/len(se):.0%} | "
          f"{sum(r['tokens'] for r in rs)/len(rs):.0f} | {sum(r['secs'] for r in rs)/len(rs):.1f} |")
errs = collections.Counter()
for r in rows:
    if not r["ran"] and not r["grammar"]:
        e = r["error"] or ""
        errs["MATCH or bm25 written wrong" if "MATCH" in r["sql"].upper() and ("near" in e or "syntax" in e) else e[:70]] += 1
print("\nCommonest errors without the grammar:")
for e, n in errs.most_common(8): print(f"- {n} x {e}")
