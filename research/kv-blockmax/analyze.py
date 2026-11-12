"""Summarises results/*.json into Markdown tables: python analyze.py results/ppl_8k.json ..."""
import json, sys, statistics as st
from collections import defaultdict

SPARSE_LAYERS = range(2, 24)
NL = 24


def mean(xs):
    return sum(xs) / len(xs) if xs else float("nan")


def ppl_tables(path):
    r = json.load(open(path))
    a = r["args"]
    budgets = [int(b) for b in a["budgets"].split(",")]
    methods = a["methods"].split(",")
    runs = r["runs"]
    L = a["L"]
    global SPARSE_LAYERS, NL
    NL = 1 + max(int(k.split("|")[-1]) for k in runs[0]["stats"] if k.startswith("prefix_mass|") and not k.endswith("perhead"))
    SPARSE_LAYERS = range(2, NL)
    out = [f"### {path}: L={L}, T={a['T']}, {len(runs)} books\n"]
    # attention recall (mass kept), averaged over sparse layers, heads, queries, books
    for key, title, fmt in [("recall", "Attention mass kept (recall)", "{:.3f}"),
                            ("oerr", "Relative error of the attention output", "{:.3f}"),
                            ("read", "Fraction of the prefix actually read", "{:.4f}")]:
        out.append(f"**{title}**, layers 2-{NL-1}, by tokens read per KV head (and % of the {L}-token prefix)\n")
        out.append("| method | " + " | ".join(f"{B} ({100*B/L:.1f}%)" for B in budgets) + " |")
        out.append("|---|" + "---|" * len(budgets))
        for m in methods:
            row = []
            for B in budgets:
                vals = [mean([run["stats"][f"{key}|{m}|{B}|{l}"] for l in SPARSE_LAYERS]) for run in runs if "stats" in run]
                row.append(fmt.format(mean(vals)))
            out.append(f"| {m} | " + " | ".join(row) + " |")
        out.append("")
    pm = mean([mean([run["stats"][f"prefix_mass|{l}"] for l in SPARSE_LAYERS]) for run in runs if "stats" in run])
    out.append(f"Mass on the prefix with dense attention (rest is on the new chunk): {pm:.3f}\n")
    # exact pruning
    ks = sorted({int(k.split("|")[1]) for k in runs[0]["stats"] if k.startswith("exact_pages_quest")})
    out.append("**Exact top-k with block-max pruning**: fraction of the prefix that must be read so that no skipped "
               f"page could hold one of the true top-k keys (per query head; layers 2-{NL-1})\n")
    out.append("| k | Quest page bounds | perfect page bounds (page max) | clustered boxes | RaBitQ 1-bit + error bound (tokens rescored) | top-k missed by RaBitQ bound |")
    out.append("|---|---|---|---|---|---|")
    for k in ks:
        q = mean([mean([run["stats"][f"exact_pages_quest|{k}|{l}"] for l in SPARSE_LAYERS]) for run in runs])
        i = mean([mean([run["stats"][f"exact_pages_ideal|{k}|{l}"] for l in SPARSE_LAYERS]) for run in runs])
        c = mean([mean([run["stats"][f"exact_tokens_cluster|{k}|{l}"] for l in SPARSE_LAYERS]) for run in runs])
        rq = mean([mean([run["stats"][f"exact_tokens_rabitq|{k}|{l}"] for l in SPARSE_LAYERS]) for run in runs])
        rm = mean([mean([run["stats"][f"exact_miss_rabitq|{k}|{l}"] for l in SPARSE_LAYERS]) for run in runs])
        out.append(f"| {k} | {q:.3f} | {i:.3f} | {c:.3f} | {rq:.3f} | {rm:.4f} |")
    out.append("")
    # per layer quest exact
    out.append("Per layer, Quest bound, k=16: " + ", ".join(
        f"{l}:{mean([run['stats'][f'exact_pages_quest|16|{l}'] for run in runs]):.2f}" for l in range(NL)) + "\n")
    # NLL
    out.append("**Perplexity of the next tokens** (dense = " +
               f"{st.mean(run['nll']['dense'] for run in runs):.3f} mean NLL); entries are NLL minus dense\n")
    out.append("| method | " + " | ".join(str(B) for B in budgets) + " |")
    out.append("|---|" + "---|" * len(budgets))
    for m in methods:
        row = [f"{mean([run['nll'][f'{m}|{B}'] - run['nll']['dense'] for run in runs]):+.3f}" for B in budgets]
        out.append(f"| {m} | " + " | ".join(row) + " |")
    out.append("")
    return "\n".join(out)


def passkey_tables(path):
    r = json.load(open(path))
    a = r["args"]
    budgets = [int(b) for b in a["budgets"].split(",")]
    methods = a["methods"].split(",")
    runs = r["runs"]
    out = [f"### {path}: passkey, L={a['L']}, {len(runs)} trials (depths "
           + ", ".join(str(x['depth']) for x in runs) + ")\n"]
    out.append(f"Dense: {sum(x['ok']['dense'] for x in runs)}/{len(runs)} correct\n")
    out.append("| method | " + " | ".join(str(B) for B in budgets) + " |")
    out.append("|---|" + "---|" * len(budgets))
    for m in methods:
        out.append(f"| {m} | " + " | ".join(
            f"{sum(x['ok'][f'{m}|{B}'] for x in runs)}/{len(runs)}" for B in budgets) + " |")
    out.append("")
    return "\n".join(out)


for p in sys.argv[1:]:
    r = json.load(open(p))
    print(ppl_tables(p) if r["args"]["mode"] == "ppl" else passkey_tables(p))
