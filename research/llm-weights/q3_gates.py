"""Gates and cascades over proto_q3_combo's per-question records: accuracy and context tokens read per question.
Thresholds are picked on DEV (Haiku's questions) and scored on TEST (Sonnet's). Usage: q3_gates.py q3_*.json"""
import json, sys
import numpy as np

def single(recs, a): return np.mean([r[a]["ok"] for r in recs]), np.mean([r[a]["ctx_tokens"] for r in recs])
def cascade(recs, steps):
    """steps: [(arm, test)]; take the first arm whose test passes (the last always does); tokens = all arms tried."""
    ok, tk = [], []
    for r in recs:
        t = 0
        for i, (a, f) in enumerate(steps):
            t += r[a]["ctx_tokens"]
            if i == len(steps) - 1 or f(r[a]): ok.append(r[a]["ok"]); break
        tk.append(t)
    return float(np.mean(ok)), float(np.mean(tk))
GRID_S = [0.8, 0.85, 0.88, 0.9, 0.92, 0.94, 0.96, 0.98]; GRID_C = [-3, -2, -1.5, -1, -0.7, -0.5, -0.3, -0.2, -0.1]
def gates():
    g = {}
    g["rows_s if score>=s else rag3"] = [(lambda s: [("rows_s", lambda x, s=s: (x["score"] or 0) >= s), ("rag3", None)]), GRID_S]
    g["rows_s if conf>=c else rag3"] = [(lambda c: [("rows_s", lambda x, c=c: x["conf"] >= c), ("rag3", None)]), GRID_C]
    g["rows_s if conf>=c else rag1"] = [(lambda c: [("rows_s", lambda x, c=c: x["conf"] >= c), ("rag1", None)]), GRID_C]
    g["rows_si if conf>=c else rag3"] = [(lambda c: [("rows_si", lambda x, c=c: x["conf"] >= c), ("rag3", None)]), GRID_C]
    g["rows_s, text_s, rag1, rag3 by conf>=c"] = [(lambda c: [("rows_s", lambda x, c=c: x["conf"] >= c), ("text_s", lambda x, c=c: x["conf"] >= c),
                                                            ("rag1", lambda x, c=c: x["conf"] >= c), ("rag3", None)]), GRID_C]
    g["text_s if conf>=c else rag3"] = [(lambda c: [("text_s", lambda x, c=c: x["conf"] >= c), ("rag3", None)]), GRID_C]
    return g
for fn in sys.argv[1:]:
    res = json.load(open(fn))
    for tag in ("records", "records_scaled"):
        if tag not in res: continue
        dev, test = res[tag]["dev"], res[tag]["test"]
        print(f"\n## {res['model']} ({'with distractors' if tag.endswith('scaled') else 'eval docs only'}), dev {len(dev)}, test {len(test)}")
        print("| option | dev acc | test acc | test tokens/question |\n|---|---|---|---|")
        for a in ("base", "rows_g", "rows_s", "rows_si", "text_s", "rag1", "rag3"):
            d, _ = single(dev, a); t, k = single(test, a); print(f"| {a} | {d:.0%} | {t:.0%} | {k:.0f} |")
        for name, (mk, grid) in gates().items():
            best = max(grid, key=lambda v: cascade(dev, mk(v))[0] - 1e-5 * cascade(dev, mk(v))[1])   # best dev accuracy, then fewer tokens
            d, _ = cascade(dev, mk(best)); t, k = cascade(test, mk(best))
            print(f"| {name} (t={best}, picked on dev) | {d:.0%} | {t:.0%} | {k:.0f} |")
        both = np.mean([r["rows_s"]["ok"] or r["rag3"]["ok"] for r in test]); print(f"| bound: rows_s or rag3 right | | {both:.0%} | |")
    if "delete_scoped" in res:
        dd = res["delete_scoped"]; q = sum(v["questions"] for v in dd.values())
        print(f"\nDELETE (scoped): {sum(v['answers_same_as_base'] for v in dd.values())}/{q} answers back to the base model's, "
              f"{sum(v['right_after_delete'] for v in dd.values())}/{q} still right")
    print("lookup ms p50", round(res.get("lookup_ms_p50", 0), 2), "scaled", round(res.get("lookup_ms_p50_scaled", 0), 2), "rows", res.get("rows_after_scale"))
