"""On-device slices, search side, on ideal Zipf traffic: the study of
`aol.py` with the AOL log's counts replaced by Zipf's law, the k-th most
searched query searched 1/k^s as often as the first (s = 1: the first twice
the second, three times the third).

The query strings and their order are AOL's: its 1,000,000 most searched
training strings (`aol.py prep`), ranked by their AOL counts, ties in a
seeded random order. Rank r then gets probability p(r) = r^-s / H(N, s) over
those N = 1,000,000 strings, so the top 10,000 carry H(10^4)/H(10^6) = 68%
of searches at s = 1 (AOL's own top 10,000 carry 22% of its training
searches, and its counts fall as about r^-0.86 between ranks 10 and
100,000). Everything else is as in `aol.py`, which this reuses:

- Slices and the result cache are chosen from the exact weights, as a
  builder would from a long enough log (`train_counts.npy` holds the
  expected searches of each string in a period as long as AOL's training,
  17,626,504 searches).
- The test is 200,000 searches drawn from p, so each distinct string weighs
  its draws. Its cloud answers are the training strings' (`aol.py cloud
  train`), so no query runs twice.

The README reports s = 1 (`zipf/`) and s = 0.86 (`zipf086/`, `prep --s
0.86`), AOL's own slope over the same strings.

Stages, each writing to --work (`--aol` is `aol.py`'s work directory):

    python zipf.py prep          # weights, test draws, cloud answers
    python zipf.py select && python zipf.py load --tiers
    python zipf.py eval && python zipf.py bytes
"""
import argparse
import json
import os
import sys
from collections import Counter

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import aol  # noqa: E402
import search  # noqa: E402
from search import SEED, log  # noqa: E402

S = 1.0
TRAIN_SEARCHES = 17_626_504  # AOL's training searches, for the expected counts
TEST_SAMPLE = 200_000


def cmd_prep(a):
    os.makedirs(a.work, exist_ok=True)
    dst = os.path.join(a.work, "corpus.jsonl")
    if not os.path.lexists(dst):
        os.symlink(os.path.realpath(os.path.join(a.aol, "corpus.jsonl")), dst)
    qs = search.queries(argparse.Namespace(work=a.aol), "train")
    counts = np.load(os.path.join(a.aol, "train_counts.npy"))
    c = np.load(os.path.join(a.aol, "cloud_train.npz"))
    rng = np.random.default_rng(SEED)
    order = np.lexsort((rng.random(len(qs)), -counts))  # by AOL count, ties at random
    qs = [qs[i] for i in order]
    ids, scores = c["ids"][order], c["scores"][order]
    n = len(qs)
    r = np.arange(1, n + 1, dtype=np.float64)
    p = r ** -a.s
    p /= p.sum()
    with open(os.path.join(a.work, "train.txt"), "w") as f:
        f.write("\n".join(qs) + "\n")
    np.save(os.path.join(a.work, "train_counts.npy"), TRAIN_SEARCHES * p)
    np.savez(os.path.join(a.work, "cloud_train.npz"), ids=ids, scores=scores)
    draws = Counter(rng.choice(n, TEST_SAMPLE, p=p).tolist())
    tr = sorted(draws, key=lambda i: (-draws[i], i))
    with open(os.path.join(a.work, "test.txt"), "w") as f:
        f.write("\n".join(qs[i] for i in tr) + "\n")
    np.save(os.path.join(a.work, "test_counts.npy"), np.array([draws[i] for i in tr], np.int64))
    np.save(os.path.join(a.work, "test_train_counts.npy"), np.array([round(TRAIN_SEARCHES * p[i]) for i in tr], np.int64))
    np.savez(os.path.join(a.work, "cloud_test.npz"), ids=ids[tr], scores=scores[tr])
    cum = np.cumsum(p)
    stats = {"zipf_s": a.s, "strings": n, "train_searches": TRAIN_SEARCHES, "top_share": {str(k): float(cum[k - 1]) for k in (10, 100, 1000, 10_000, 100_000)},
             "aol_top_share_of_training": {str(k): float(counts[:k].sum() / TRAIN_SEARCHES) for k in (10, 100, 1000, 10_000, 100_000, 1_000_000)},
             "test_sample": TEST_SAMPLE, "test_sample_distinct": len(tr),
             "docs": json.load(open(os.path.join(a.aol, "prep.json")))["docs"]}
    json.dump(stats, open(os.path.join(a.work, "prep.json"), "w"), indent=1)
    log("prep", stats)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--work", default="/home/user/data/slices/zipf")
    ap.add_argument("--aol", default="/home/user/data/slices/aol", help="aol.py's work directory")
    ap.add_argument("--threads", type=int, default=4)
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("prep")
    p.add_argument("--s", type=float, default=S)
    sub.add_parser("select")
    p = sub.add_parser("load")
    p.add_argument("--tiers", action="store_true")
    sub.add_parser("eval")
    sub.add_parser("bytes")
    a = ap.parse_args()
    if a.cmd != "prep":
        import strata as st
        aol.strata = search.strata = st
    {"prep": cmd_prep, "select": aol.cmd_select, "load": search.cmd_load, "eval": aol.cmd_eval, "bytes": search.cmd_bytes}[a.cmd](a)


if __name__ == "__main__":
    main()
