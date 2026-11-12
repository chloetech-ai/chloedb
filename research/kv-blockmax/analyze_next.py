"""Tables for the next steps: python analyze_next.py > results/next.md"""
import json, os, statistics as st

def mean(xs):
    xs = list(xs)
    return sum(xs) / len(xs) if xs else float("nan")

out = []
# (e) blending stitched caches
b = json.load(open("results/blend_1p5b.json"))["runs"]
old = {x["ex"]: x for x in json.load(open("results/rag_1p5b.json"))["runs"]}
full = [old[x["ex"]]["f1"]["full"] for x in b]
pre = st.median(old[x["ex"]]["t"]["prefill_full"] for x in b)
out.append(f"## Repairing stitched caches (Qwen2.5-1.5B, LongBench-E HotpotQA + 2WikiMQA, {len(b)} questions)\n")
out.append("The joint prefill and the plain stitch are run_rag.py's (`results/rag_1p5b.json`); the stitch is rerun here and scores the same. "
           f"Median joint prefill {pre:.1f} s; median seconds to recompute, on the same 4 cores.\n")
out.append("| condition | tokens recomputed | F1 | exact match | recompute (median) | better / worse than joint, per question |")
out.append("|---|---|---|---|---|---|")
out.append(f"| all passages prefilled together | all | {100 * mean(full):.1f} | {100 * mean(old[x['ex']]['em']['full'] for x in b):.1f} | {pre:.1f} s | |")
names = {"stitch": "stitched, nothing recomputed"}
for how, label in (("first", "the first tokens of every passage"), ("dev", "largest K/V deviation at layer 1 (CacheBlend)"), ("rand", "random")):
    for r in (5, 15, 30):
        names[f"blend_{how}_{r}"] = f"{label}, {r}%"
for c in ["stitch"] + [f"blend_{h}_{r}" for h in ("first", "dev", "rand") for r in (5, 15, 30)]:
    f = [x["f1"][c] for x in b]
    d = [a - j for a, j in zip(f, full)]
    t = st.median(x["t"].get(f"blend_{c}", 0) for x in b)
    extra = st.median(x["t"]["first_layers"] for x in b) if "_dev_" in c or "_rand_" in c else 0
    rec = f"{c.split('_')[-1]}%" if c != "stitch" else "0"
    out.append(f"| {names[c]} | {rec} | {100 * mean(f):.1f} | {100 * mean(x['em'][c] for x in b):.1f} | "
               f"{(t + extra):.1f} s | {sum(v > 0 for v in d)} / {sum(v < 0 for v in d)} |")
out.append("")
out.append("Deviation and random choices also replace layers 0 and 1 with a fresh joint prefill of those two layers "
           "(included in their time); the first-tokens choice does not.\n")

# (a) decode timing
out.append("## One decode step in Rust: dense attention against IVF over keys (decode-bench)\n")
for f in ("results/decode_real_32k.md", "results/decode_synth.md", "results/decode_synth_128k_sampled.md"):
    if os.path.exists(f):
        out.append(f"`{f}`:\n")
        out.extend(l.rstrip() for l in open(f) if l.startswith("|") or l.startswith("#"))
        out.append("")

# (d) write paths
if os.path.exists("results/write_paths.json"):
    w = json.load(open("results/write_paths.json"))["runs"]
    out.append("## Writing a stored cache to chloedb-server (28 rows, one per layer)\n")
    out.append("| tokens | bytes | base64 in /v1/write | POST /v1/media + /v1/write | read back from /v1/media | same bytes |")
    out.append("|---|---|---|---|---|---|")
    for r in w:
        out.append(f"| {r['tokens']} | {r['bytes'] / 2**20:.0f} MiB | {r['base64_s']:.2f} s | {r['binary_s']:.2f} s | "
                   f"{r['binary_read_s']:.2f} s | {'yes' if r['base64_ok'] and r['binary_ok'] else 'NO'} |")
    out.append("")

# (c) cluster reuse
if os.path.exists("results/reuse_qwen3_32k.json"):
    r = json.load(open("results/reuse_qwen3_32k.json"))
    m = r["mean"]
    wins = [int(k[5:]) for k in m if k.startswith("hit_w")]
    out.append(f"## Cluster reuse across decode steps (Qwen3-0.6B, {r['args']['L']} tokens of {r['book']}, "
               f"{r['args']['T'] - 1} steps, budget {r['budget']} tokens a KV head)\n")
    out.append(f"NLL of the decoded tokens: sparse {r['nll_sparse']:.3f}, dense {r['nll_dense']:.3f}. "
               f"Mean over layers 2 to {max(map(int, r['layers']))} and KV heads; read per step {100 * m['read_frac']:.1f}% of the prefix.\n")
    out.append("| RAM cache holds the clusters of the last | share of a step's tokens found in it | its size, share of the prefix |")
    out.append("|---|---|---|")
    for w_ in sorted(wins):
        out.append(f"| {w_} step{'s' if w_ > 1 else ''} | {100 * m[f'hit_w{w_}']:.1f}% | {100 * m[f'ram_w{w_}']:.1f}% |")
    out.append("")
    worst = sorted(r["layers"].items(), key=lambda kv: kv[1]["hit_w16"])[:3]
    out.append("Lowest reuse (16 steps): " + ", ".join(f"layer {l} {100 * v['hit_w16']:.0f}%" for l, v in worst) + ".")
print("\n".join(out))
