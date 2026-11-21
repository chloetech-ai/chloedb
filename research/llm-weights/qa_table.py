"""One table of every technique on the 230 questions, closed book and with BM25 RAG, from the GPU runs' JSON files."""
import json, os, sys
A, B = sys.argv[1], sys.argv[2]
def J(d, n):
    p = f"{d}/{n}.json"; return json.load(open(p)) if os.path.exists(p) else None
def acc(x): return None if not x else (x["right"], x["of"])
rows = []   # (model, technique, closed, rag, note)
def add(m, t, c, r, note=""): rows.append((m, t, acc(c), acc(r), note))
for m, g1, lora, ffn, kr, q2, d in [("0.6B", "g1_06b_qa", "g1_06b_lora_qa", "g1_06b_ffn_qa", "g1_06b_knn_route_qa", "q2_06b_facts", A),
                                    ("4B", "g1_4b_gated", "g1_4b_lora_qa", "g1_4b_ffn_qa", "g1_4b_knn_route_qa", "q2_4b_facts", A),
                                    ("8B", "g1_8b", "g1_8b", None, "g1_8b_knn_route_qa", "q2_8b_facts", B)]:
    x = J(d, g1)
    if x:
        b = x["arms"]["base"]; add(m, "base model", b.get("qa"), b.get("qa_rag"), f"ppl {b['held_out']:.1f}, WikiText {b['general']:.1f}")
        for a in ("rows_multi_gated_iso",):
            if a in x["arms"]: r = x["arms"][a]; add(m, "ChloeDB rows (4 layers, gated, per doc)", r.get("qa"), r.get("qa_rag"), f"ppl {r['held_out']:.1f}, WikiText {r['general']:.1f}")
    for n, a, label in ((lora, "lora", "LoRA fine-tune"), (ffn, "ffn", "FFN fine-tune (one layer)")):
        y = J(d, n) if n else None
        if y and a in y["arms"]: r = y["arms"][a]; add(m, label, r.get("qa"), r.get("qa_rag"), f"ppl {r['held_out']:.1f}, WikiText {r['general']:.1f}")
    y = J(d, kr)
    if y:
        k = y["arms"].get("knn")
        if k: add(m, "kNN-LM served by ChloeDB", k["qa"], k["qa_rag"], f"lambda {k['lambda']}, temp {k['temp']}, {k['ms_per_lookup_p50']:.2f} ms/lookup")
        r = y["arms"].get("routed_lora")
        if r:
            add(m, "one LoRA per doc, BM25-routed", r["qa"], r["qa_rag"], f"router right {r['router_right']:.0%}")
            add(m, "  same, told the right doc", r["qa_oracle_route"], r["qa_rag_oracle_route"], "ceiling")
    z = J(d, q2)
    if z:
        f = z["facts_rows"]; add(m, "facts as ChloeDB rows", f["qa"], f["qa_rag"], f"matched {f['qa']['matched']}/{f['qa']['of']} ({f['qa']['matched_right_doc']} right doc), {z['write_secs_per_fact']} s/fact, delete {z['delete_one_doc']['before']:.2f}->{z['delete_one_doc']['after_sql_delete']:.2f} (base {z['delete_one_doc']['base']:.2f})")
def pct(v): return "" if v is None else f"{v[0] / v[1]:.0%} ({v[0]})"
print("| Model | Technique | Closed book | With RAG | Notes |\n|---|---|---|---|---|")
for m, t, c, r, n in rows: print(f"| {m} | {t} | {pct(c)} | {pct(r)} | {n} |")
json.dump([dict(zip(("model", "technique", "closed", "rag", "note"), r)) for r in rows], open(sys.argv[3], "w"), indent=1) if len(sys.argv) > 3 else None
