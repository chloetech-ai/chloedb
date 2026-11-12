"""Tables for the three uses: python analyze_uses.py > results/uses.md"""
import json, statistics as st

def mean(xs):
    xs = list(xs)
    return sum(xs) / len(xs) if xs else float("nan")

out = []
# use 2: the shared tier
rows = [json.loads(l) for l in open("results/tier_1p5b.jsonl")]
by = {}
for r in rows:
    by.setdefault(r["tokens"], {})[r["mode"]] = r
out.append("## Use 2: chloedb-server as a shared KV-cache tier (Qwen2.5-1.5B, bf16, 4 CPU cores)\n")
out.append("| prefix tokens | cache size | prefill + first token (cold) | from chloedb-server: lookup + fetch + rebuild + first token | from a local file | write to chloedb-server | same answer |")
out.append("|---|---|---|---|---|---|---|")
for n in sorted(by):
    p, g, l = by[n]["put"], by[n]["get"], by[n]["local"]
    same = p["answer"] == g["answer"] == l["answer"] and p["first_token"] == g["first_token"] == l["first_token"]
    out.append(f"| {n} | {p['bytes'] / 2**20:.0f} MiB | {p['ttft_cold_s']:.1f} s | {g['ttft_warm_s']:.2f} s "
               f"({g['lookup_s']:.3f} + {g['fetch_s']:.2f} + {g['deserialize_s']:.2f} + {g['tail_s']:.2f}) | "
               f"{l['ttft_local_s']:.2f} s | {p['put_s']:.1f} s | {'yes' if same else 'NO'} ({p['answer']}) |")
out.append("")

# use 3: project memory
r = json.load(open("results/ctx_1p5b.json"))
conds = ["closed", "cache", "cache_sparse", "bm25_text", "bm25_stitch"]
names = {"closed": "no context", "cache": "whole corpus, stored cache, dense",
         "cache_sparse": "whole corpus, stored cache, sparse (6%)",
         "bm25_text": "ChloeDB BM25 top 4 chunks, text prefilled",
         "bm25_stitch": "ChloeDB BM25 top 4 chunks, stored chunk caches"}
out.append(f"## Use 3: project memory on a local model ({r['tokens']} tokens of ChloeCtx docs in {r['chunks']} chunks, {len(r['runs'])} questions)\n")
out.append(f"One-off costs: prefill of the corpus {r['t']['prefill_corpus']:.0f} s ({r['cache_bytes'] / 2**20:.0f} MiB of KV); "
           f"caches of every chunk {r['t']['build_chunk_caches']:.0f} s; the cluster index of the corpus "
           f"{sum(x['t'].get('index_build', 0) for x in r['runs']):.0f} s.\n")
out.append("| condition | right | mean seconds per answer (incl. decoding) |")
out.append("|---|---|---|")
for c in conds:
    ok = sum(x["ok"][c] for x in r["runs"])
    t = mean(x["t"][c] for x in r["runs"]) + (mean(x["t"]["bm25"] for x in r["runs"]) if c.startswith("bm25") else 0)
    out.append(f"| {names[c]} | {ok}/{len(r['runs'])} | {t:.1f} |")
out.append("")

# use 1: multi-document RAG
r = json.load(open("results/rag_1p5b.json"))
runs = r["runs"]
conds = ["closed", "full", "full_sparse", "stitch", "stitch_sparse", "bm25_text", "bm25_stitch"]
names = {"closed": "no passages", "full": "all passages prefilled together (re-read every question)",
         "full_sparse": "all passages together, sparse (6%)",
         "stitch": "stored per-passage caches stitched", "stitch_sparse": "stitched caches, sparse (6%)",
         "bm25_text": "ChloeDB BM25 top 3, text prefilled", "bm25_stitch": "ChloeDB BM25 top 3, stored caches"}
tok = mean(x["tokens"] for x in runs)
out.append(f"## Use 1: multi-document QA from stored caches (LongBench-E HotpotQA + 2WikiMQA, {len(runs)} questions, "
           f"mean {tok:.0f} tokens, 10 passages each)\n")
out.append("| condition | F1 | exact match | time to answer |")
out.append("|---|---|---|---|")
for c in conds:
    f = mean(x["f1"][c] for x in runs)
    e = mean(x["em"][c] for x in runs)
    t = mean(x["t"][f"answer_{c}"] for x in runs)
    if c in ("full", "full_sparse"):
        t += mean(x["t"]["prefill_full"] for x in runs)
    if c in ("stitch", "stitch_sparse"):
        t += mean(x["t"]["load_docs"] + x["t"]["stitch"] for x in runs)
    if c == "bm25_text":
        t += mean(x["t"]["prefill_bm25"] + x["t"]["bm25"] for x in runs)
    if c == "bm25_stitch":
        t += mean(x["t"]["stitch_bm25"] + x["t"]["bm25"] for x in runs)
    out.append(f"| {names[c]} | {100 * f:.1f} | {100 * e:.1f} | {t:.1f} s |")
out.append("")
for s in ["hotpotqa_e", "2wikimqa_e"]:
    sub = [x for x in runs if x["ex"].startswith(s)]
    if sub:
        out.append(f"{s} ({len(sub)}): " + ", ".join(f"{c} {100 * mean(x['f1'][c] for x in sub):.1f}" for c in conds))
out.append("")
out.append(f"Per question, building the passage caches took {mean(x['t']['build_docs'] for x in runs):.1f} s once "
           f"(vs. {mean(x['t']['prefill_full'] for x in runs):.1f} s for the joint prefill); loading them "
           f"{mean(x['t']['load_docs'] for x in runs):.2f} s and stitching {mean(x['t']['stitch'] for x in runs):.2f} s; "
           f"the cluster index of a stitched prefix {mean(x['t'].get('index_stitch', 0) for x in runs):.1f} s; "
           f"{mean(x['bytes_docs'] for x in runs) / 2**20:.0f} MiB of caches per question.")
# paired: stitched vs full
d = [x["f1"]["stitch"] - x["f1"]["full"] for x in runs]
out.append(f"\nStitched minus joint F1, per question: mean {100 * mean(d):+.1f}, "
           f"{sum(v > 0 for v in d)} better, {sum(v < 0 for v in d)} worse, {sum(v == 0 for v in d)} the same.")
print("\n".join(out))
