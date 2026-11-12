"""How the sequence model's negatives are drawn, on Amazon Beauty or
MovieLens-1M: `chloedb.train.sequence`'s trainer (chloedb._sequence), items
only, item vectors learned from a random start, with one of

- `--negatives all`: every item (what eval.py and the gate train with);
- `--negatives in_batch`: the other targets of the batch;
- `--logq`: in-batch, each negative's score less the log of the chance
  the batch holds it;
- `--uniform N`: N items drawn evenly, added to each batch's negatives,

for `--minutes`, stopping early when validation NDCG@10 has not risen for
`--patience` epochs. Each user's last item is the test and the one before
it validates (leave-one-out, ranked against every item but the user's
own, every user), as eval.py and results/generative score. Writes
`seq-<data>-<tag>.json` under `--out` (the runs of the README are in
negatives/).

    python negatives.py --dir ~/data/beauty --out negatives --tag inbatch --minutes 15
    python negatives.py --dir ~/data/beauty --out negatives --tag inbatch-logq --logq --minutes 15
    python negatives.py --dir ~/data/beauty --out negatives --tag inbatch-logq-u2048 --logq --uniform 2048 --minutes 15
    python negatives.py --dir ~/data/beauty --out negatives --tag all --negatives all --minutes 15
    python negatives.py --dir ~/data/beauty --out negatives --tag logq-u2048-long --logq --uniform 2048 --minutes 45
    python negatives.py --data ml-1m --dir ~/data/ml-1m --out negatives --tag inbatch --max-len 200 --minutes 20
    python negatives.py --data ml-1m --dir ~/data/ml-1m --out negatives --tag inbatch-logq --logq --max-len 200 --minutes 20
"""
import argparse
import json
import os
import sys
import time

import torch

from chloedb import _sequence as S

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "generative"))
import tiger  # noqa: E402  (Beauty's reviews, read as results/generative reads them)


def ml1m(d):
    by = {}
    for n, line in enumerate(open(os.path.join(d, "ratings.dat"))):
        u, i, r, t = line.strip().split("::")
        by.setdefault(u, []).append((int(t), n, i))
    return {u: [i for _, _, i in sorted(v)] for u, v in sorted(by.items())}

ap = argparse.ArgumentParser()
ap.add_argument("--data", default="beauty"); ap.add_argument("--dir", required=True); ap.add_argument("--out", required=True)
ap.add_argument("--negatives", default="in_batch"); ap.add_argument("--logq", action="store_true"); ap.add_argument("--uniform", type=int, default=0)
ap.add_argument("--minutes", type=float, default=12); ap.add_argument("--max-len", type=int, default=50); ap.add_argument("--threads", type=int, default=2)
ap.add_argument("--batch", type=int, default=128); ap.add_argument("--tag", required=True); ap.add_argument("--seed", type=int, default=0)
ap.add_argument("--temperature", type=float, default=0.05); ap.add_argument("--patience", type=int, default=10)
a = ap.parse_args()
torch.set_num_threads(a.threads)
seqs = ml1m(a.dir) if a.data == "ml-1m" else tiger.sequences(os.path.join(a.dir, tiger.FILES[a.data][0]))
seqs = {u: [(i, None, None) for i in s] for u, s in seqs.items()}
items = sorted({e[0] for s in seqs.values() for e in s})
g = torch.Generator().manual_seed(a.seed)
start = torch.nn.functional.normalize(torch.randn(len(items), 64, generator=g), dim=-1)
vectors = {k: start[i].tolist() for i, k in enumerate(items)}
cfg = {"kind": "sequence", "table": "items", "field": "emb", "dim": None, "width": 64, "layers": 2, "heads": 1, "ffn": 256, "max_len": a.max_len,
       "gap_buckets": 32, "actions": [""], "eps": S.EPS}
t0 = time.time()
model, (keys, E), _, hist = S.train({u: s[:-1] for u, s in seqs.items()}, None, vectors, cfg, learn_items=True, negatives=a.negatives, temperature=a.temperature,
    epochs=1000, batch=a.batch, lr=1e-3, dropout=0.2, holdout=True, patience=a.patience, seed=a.seed, log=lambda r: print(json.dumps(r), flush=True),
    max_seconds=a.minutes * 60, logq=a.logq, uniform_negatives=a.uniform)
index = {k: i for i, k in enumerate(keys)}
test = S.evaluate(model, [(s[:-1], s[-1]) for s in seqs.values() if len(s) >= 3], index, {"": 0}, cfg, E)
best = max(hist, key=lambda r: r["ndcg@10"])
res = {"args": vars(a), "test": test, "best_valid": best, "epochs": len(hist), "train_seconds": round(time.time() - t0), "history": hist}
print("TEST", json.dumps(test), "best epoch", best["epoch"], "of", len(hist), flush=True)
os.makedirs(a.out, exist_ok=True); json.dump(res, open(os.path.join(a.out, f"seq-{a.data}-{a.tag}.json"), "w"), indent=1)
