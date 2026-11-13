"""How far `count_tokens` (chloedb::query::pack), PACK's estimate of a
text's tokens, is from real tokenizers' counts: OpenAI's cl100k_base and
o200k_base (tiktoken), on the sample `chloedb-pack-bench --token-sample`
writes (English abstracts of SciFact and NFCorpus, Wikipedia passages of
MIRACL in other scripts, ChloeDB's Rust source).

    pip install tiktoken==0.12.0
    python3 results/pack/token_error.py SAMPLE.jsonl results/pack/token_error.json

Per source and tokenizer: the estimate over the true count, summed over
the texts and per text (median, 10th and 90th percentile), and the share
of texts the estimate counts more than 10% short (the unsafe side: a pack
of those texts spends more of the model's budget than it says).
"""
import json
import statistics
import sys
from collections import defaultdict

import tiktoken


def pct(xs, p):
    xs = sorted(xs)
    return xs[min(len(xs) - 1, int(p / 100 * len(xs)))]


def main(sample, out):
    rows = [json.loads(l) for l in open(sample, encoding="utf-8")]
    by_source = defaultdict(list)
    for r in rows:
        by_source[r["source"]].append(r)
    report = {"tiktoken": tiktoken.__version__, "sources": {}}
    for name in ("cl100k_base", "o200k_base"):
        enc = tiktoken.get_encoding(name)
        for source, rs in sorted(by_source.items()):
            true = [len(enc.encode(r["text"], disallowed_special=())) for r in rs]
            est = [r["estimate"] for r in rs]
            ratios = [e / t for e, t in zip(est, true) if t > 0]
            doc = {
                "texts": len(rs),
                "true_tokens": sum(true),
                "estimated_tokens": sum(est),
                "ratio_of_sums": sum(est) / max(1, sum(true)),
                "ratio_p10": pct(ratios, 10),
                "ratio_median": statistics.median(ratios),
                "ratio_p90": pct(ratios, 90),
                "share_under_by_10pct": sum(1 for x in ratios if x < 0.9) / len(ratios),
            }
            report["sources"].setdefault(source, {})[name] = doc
            print(f"{name:12} {source:12} {len(rs):5} texts  estimate/true: sum {doc['ratio_of_sums']:.2f}, "
                  f"median {doc['ratio_median']:.2f} (p10 {doc['ratio_p10']:.2f}, p90 {doc['ratio_p90']:.2f}), "
                  f"{100 * doc['share_under_by_10pct']:.0f}% of texts under by >10%")
    with open(out, "w", encoding="utf-8") as f:
        json.dump(report, f, indent=2, ensure_ascii=False)
        f.write("\n")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
