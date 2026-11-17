#!/usr/bin/env python3
"""Leaderboards that build themselves: the benchmark results papers state.

`extract` reads papers (jsonl with id, title, abstract, created) and asks
the model for every result each abstract states as a number on a named
benchmark: dataset, metric, value, the method it belongs to, whether it is
the paper's own, the setting (zero-shot, test split, ...) and the exact
words it comes from. A result is kept only when those words are in the
abstract and hold the value, so a number the model made up is dropped.
It writes one row per result (the table ax_results) and counts the tokens
it used, so a sample prices the whole run.

    python3 tools/leaderboards.py extract sample.jsonl results.jsonl --workers 16
    python3 tools/leaderboards.py price results.jsonl.usage.json --papers 400000

The key is read from ANTHROPIC_API_KEY (or STRATA_ANTHROPIC_KEY). Standard
library only.
"""

import argparse
import concurrent.futures as cf
import json
import os
import re
import sys
import time
import urllib.error
import urllib.request

MODEL = "claude-haiku-5-5"
# Dollars per million tokens, input and output (Claude Haiku 5.5, prompts of
# 100K tokens or fewer); the Message Batches API halves both.
PRICE = (0.10, 0.50)

SYSTEM = """You read the abstract of a research paper and list the benchmark results it states as numbers.

A result is a number the abstract gives for a method on a named dataset or benchmark with a metric: "85.3% top-1 accuracy on ImageNet", "a BLEU of 29.1 on WMT14 En-De", "FID 2.1 on CIFAR-10", "pass@1 of 71% on HumanEval".

Leave out:
- improvements, gains or ratios ("improves accuracy by 3 points", "2x faster", "reduces error by 30%"), since they are not a score on the benchmark;
- numbers without a named dataset or benchmark ("90% accuracy on our data", "a new dataset of 10k images");
- sizes, counts, costs and speeds (parameters, tokens, GPUs, hours, frames per second), unless the benchmark's metric is a speed;
- anything the abstract does not say outright.

For each result:
- task: the task in a few words, lower case ("image classification", "machine translation", "code generation").
- dataset: the dataset or benchmark as written, with its split or subset when given ("ImageNet-1k", "MMLU", "COCO test-dev", "WMT14 En-De").
- metric: the metric as written or clearly meant ("top-1 accuracy", "BLEU", "mAP", "pass@1", "FID", "WER", "accuracy").
- value: the number as a number (85.3 for 85.3%, 0.853 for 0.853).
- unit: "%" when the value is a percentage, otherwise "".
- higher_is_better: false for errors, losses, distances, perplexity, FID, WER and the like, otherwise true.
- method: the name of the model or method the number is for, as written (the paper's own method's name if it has one, "" if it has none).
- own: true when the number is for the method or model this paper proposes, false for a baseline or another paper's model.
- setting: a few words on how it was measured when the abstract says ("zero-shot", "5-shot", "fine-tuned", "test set", "without extra data"), otherwise "".
- quote: the shortest exact words of the abstract that contain the number, copied character for character.
- kind: "score" when the number is the method's score on the benchmark itself; "change" when it is a difference, gain, drop, improvement, ratio or a share of another score ("+1.2 mIoU", "improves by 13.9%", "a 1.8% drop", "retains 99% of", "0.4% worse than"). Only scores make a leaderboard, so mark every change as a change.
- public: true when the dataset or benchmark is a published one others evaluate on by that name (ImageNet, MMLU, LibriSpeech, a shared task), false when it is the paper's own, private or described rather than named ("our dataset", "a multi-center cohort", "94 products").

Answer with {"results": []} when the abstract states no such result."""

SCHEMA = {
    "type": "object",
    "properties": {
        "results": {
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "task": {"type": "string"},
                    "dataset": {"type": "string"},
                    "metric": {"type": "string"},
                    "value": {"type": "number"},
                    "unit": {"type": "string"},
                    "higher_is_better": {"type": "boolean"},
                    "method": {"type": "string"},
                    "own": {"type": "boolean"},
                    "setting": {"type": "string"},
                    "quote": {"type": "string"},
                    "kind": {"type": "string", "enum": ["score", "change"]},
                    "public": {"type": "boolean"},
                },
                "required": ["task", "dataset", "metric", "value", "unit", "higher_is_better", "method", "own", "setting", "quote", "kind", "public"],
                "additionalProperties": False,
            },
        }
    },
    "required": ["results"],
    "additionalProperties": False,
}


def request(paper):
    text = f"Title: {paper['title']}\n\nAbstract: {paper['abstract']}"
    return {
        "model": MODEL,
        "max_tokens": 2048,
        # A list to copy out, not a problem to think through.
        "thinking": {"type": "disabled"},
        "system": SYSTEM,
        "messages": [{"role": "user", "content": text}],
        "output_config": {"format": {"type": "json_schema", "schema": SCHEMA}},
    }


def call(body, key):
    base = os.environ.get("ANTHROPIC_API_URL", "https://api.anthropic.com")
    data = json.dumps(body).encode()
    err = None
    for attempt in range(6):
        try:
            req = urllib.request.Request(base + "/v1/messages", data=data, headers={"x-api-key": key, "anthropic-version": "2023-06-01", "content-type": "application/json"})
            with urllib.request.urlopen(req, timeout=120) as r:
                return json.loads(r.read())
        except urllib.error.HTTPError as e:
            err = f"HTTP {e.code}: {e.read()[:300]!r}"
            if e.code < 500 and e.code != 429:
                break
        except Exception as e:  # noqa: BLE001 - retried, then given up
            err = str(e)
        time.sleep(3 * (attempt + 1))
    raise RuntimeError(err)


def norm_space(s):
    return re.sub(r"\s+", " ", s or "").strip()


def number_in(quote, value):
    """Whether the quote holds the value as written (85.3, 85.30, 0.853, 1,024)."""
    nums = [float(x.replace(",", "")) for x in re.findall(r"\d[\d,]*\.?\d*|\.\d+", quote)]
    return any(abs(n - value) <= 1e-9 + 1e-6 * abs(value) for n in nums)


# What around a number makes it a change rather than a score, should the
# model call it a score: "+1.2", "by 3.4", "3.4% higher", "2x".
BEFORE = re.compile(r"([+±]\s?|\bby\s+(up to\s+|over\s+|about\s+|nearly\s+|roughly\s+|an average of\s+|as much as\s+)?[+-]?\$?)$", re.I)
AFTER = re.compile(r"^\s?(\\?%|pp|points?|percentage points?|p\.p\.)?\s*(higher|lower|improvement|gain|drop|increase|decrease|reduction|better|worse|relative|absolute|fewer|more|less|faster|slower)\b|^\s?(×|x|times)\b", re.I)


def is_change(quote, value):
    """Whether the value, where the quote gives it, reads as a change."""
    for m in re.finditer(r"\d[\d,]*\.?\d*|\.\d+", quote):
        try:
            n = float(m.group(0).replace(",", ""))
        except ValueError:
            continue
        if abs(n - value) <= 1e-9 + 1e-6 * abs(value):
            if BEFORE.search(quote[max(0, m.start() - 24):m.start()]) or AFTER.search(quote[m.end():m.end() + 32]):
                return True
    return False


def why_dropped(paper_text, r):
    q = norm_space(r.get("quote"))
    if not q or q not in paper_text:
        return "words not in the abstract"
    if not number_in(q, r.get("value", float("nan"))):
        return "value not in its words"
    if not r.get("dataset", "").strip() or not r.get("metric", "").strip():
        return "no dataset or metric"
    if r.get("kind") != "score" or is_change(q, r["value"]):
        return "a change, not a score"
    if not r.get("public"):
        return "not a public benchmark"
    return ""


def check(paper, results):
    """The scores on public benchmarks whose words are in the abstract and
    hold their value; the others with why each was dropped."""
    text = norm_space(paper["title"] + " " + paper["abstract"])
    kept, dropped = [], []
    for r in results:
        why = why_dropped(text, r)
        if why:
            dropped.append({**r, "dropped": why})
        else:
            kept.append(r)
    return kept, dropped


def key_of(s):
    """A dataset's or metric's key: lower case, letters and digits only."""
    return re.sub(r"[^a-z0-9]+", "", (s or "").lower())


def rows(paper, kept):
    out = []
    for i, r in enumerate(kept):
        out.append({
            "id": f"{paper['id']}#{i}", "paper": paper["id"], "created": paper.get("created"), "year": paper.get("year"), "pcat": paper.get("pcat"),
            "task": r["task"].strip().lower(), "dataset": r["dataset"].strip(), "dataset_key": key_of(r["dataset"]),
            "metric": r["metric"].strip(), "metric_key": key_of(r["metric"]), "value": r["value"], "unit": r["unit"],
            "higher": bool(r["higher_is_better"]), "method": r["method"].strip(), "own": bool(r["own"]),
            "setting": r["setting"].strip(), "quote": norm_space(r["quote"]),
        })
    return out


def extract(args):
    key = os.environ.get("ANTHROPIC_API_KEY") or os.environ.get("STRATA_ANTHROPIC_KEY")
    if not key:
        sys.exit("set ANTHROPIC_API_KEY")
    papers = [json.loads(l) for l in open(args.papers)]
    if args.limit:
        papers = papers[: args.limit]
    usage = {"papers": 0, "input_tokens": 0, "output_tokens": 0, "failed": 0, "results": 0, "dropped": 0, "with_results": 0, "seconds": 0.0}
    t0 = time.time()
    dropped_out = open(args.out + ".dropped.jsonl", "w")

    def one(p):
        j = call(request(p), key)
        text = "".join(c.get("text", "") for c in j.get("content", []) if c.get("type") == "text")
        res = json.loads(text).get("results", []) if text else []
        return p, j.get("usage", {}), res, j.get("stop_reason")

    with open(args.out, "w") as out, cf.ThreadPoolExecutor(args.workers) as ex:
        for k, fut in enumerate(cf.as_completed([ex.submit(one, p) for p in papers])):
            try:
                p, u, res, stop = fut.result()
            except Exception as e:  # noqa: BLE001 - counted
                usage["failed"] += 1
                print("failed:", e, file=sys.stderr)
                continue
            usage["papers"] += 1
            usage["input_tokens"] += u.get("input_tokens", 0)
            usage["output_tokens"] += u.get("output_tokens", 0)
            kept, dropped = check(p, res)
            usage["results"] += len(kept)
            usage["dropped"] += len(dropped)
            usage["with_results"] += bool(kept)
            for r in rows(p, kept):
                out.write(json.dumps(r) + "\n")
            for r in dropped:
                dropped_out.write(json.dumps({"paper": p["id"], **r}) + "\n")
            if (k + 1) % 100 == 0:
                print(f"  {k + 1} papers, {usage['results']} results, {time.time() - t0:.0f} s", file=sys.stderr)
    usage["seconds"] = round(time.time() - t0, 1)
    json.dump(usage, open(args.out + ".usage.json", "w"), indent=1)
    print(json.dumps(usage))


def price(args):
    u = json.load(open(args.usage))
    per = (u["input_tokens"] * PRICE[0] + u["output_tokens"] * PRICE[1]) / 1e6 / max(1, u["papers"])
    full = per * args.papers
    print(json.dumps({
        "papers_in_sample": u["papers"], "dollars_per_1000_papers": round(per * 1000, 4),
        "papers": args.papers, "dollars": round(full, 2), "dollars_with_batches": round(full / 2, 2),
        "results_per_paper": round(u["results"] / max(1, u["papers"]), 2), "papers_with_results": round(u["with_results"] / max(1, u["papers"]), 3),
        "dropped_share": round(u["dropped"] / max(1, u["results"] + u["dropped"]), 3),
    }, indent=1))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    e = sub.add_parser("extract")
    e.add_argument("papers")
    e.add_argument("out")
    e.add_argument("--workers", type=int, default=16)
    e.add_argument("--limit", type=int, default=0, help="only the first N papers")
    p = sub.add_parser("price")
    p.add_argument("usage")
    p.add_argument("--papers", type=int, required=True)
    args = ap.parse_args()
    {"extract": extract, "price": price}[args.cmd](args)


if __name__ == "__main__":
    main()
