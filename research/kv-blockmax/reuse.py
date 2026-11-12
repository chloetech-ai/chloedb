"""Next step (c): how much the clusters a decode step reads overlap with the
previous steps', which decides whether a cache kept on SSD (centroids in RAM,
clusters fetched on demand) can serve decode from a small RAM cache.

One PG19 book: an L-token prefix prefilled densely, then T tokens of the book
decoded one at a time (teacher forced) with sparse cluster_cent attention at
`frac` of the prefix. For every layer, KV head and step, the clusters read are
recorded; reported: the share of the step's tokens that the previous step read
too, and the share found in a RAM cache holding the clusters of the last W
steps (and how big that cache is, as a share of the prefix).

Also dumps one layer's K and V (bf16, H x L x d) for decode-bench (--dump-layer).

  python reuse.py --model Qwen/Qwen3-0.6B --L 32768 --out results/reuse_qwen3_32k.json --dump-dir DIR
"""
import argparse, json, os, random, time
import numpy as np
import torch
import pandas as pd
from transformers import AutoModelForCausalLM, AutoTokenizer
import kvsel
from kvsel import ctl

ap = argparse.ArgumentParser()
ap.add_argument("--model", default="Qwen/Qwen3-0.6B")
ap.add_argument("--data", default="data/data/test-00000-of-00001-29a571947c0b5ccc.parquet")
ap.add_argument("--L", type=int, default=32768)
ap.add_argument("--T", type=int, default=128)
ap.add_argument("--frac", type=float, default=0.0625)
ap.add_argument("--book", type=int, default=0, help="index into the shuffled books that are long enough")
ap.add_argument("--windows", default="1,4,16,64")
ap.add_argument("--dump-layer", type=int, default=14)
ap.add_argument("--dump-dir", default=None)
ap.add_argument("--out", required=True)
args = ap.parse_args()

torch.manual_seed(0)
torch.set_num_threads(os.cpu_count())
tok = AutoTokenizer.from_pretrained(args.model)
model = AutoModelForCausalLM.from_pretrained(args.model, dtype=torch.bfloat16, attn_implementation="kvsel").eval()
books = pd.read_parquet(args.data)
order = list(range(len(books)))
random.Random(0).shuffle(order)
got = -1
for bi in order:
    ids = tok(books.text.iloc[bi][: (args.L + args.T + 2000) * 6], return_tensors="pt").input_ids[0]
    if ids.numel() >= args.L + args.T + 1000:
        got += 1
        if got == args.book:
            break
ids = ids[1000: 1000 + args.L + args.T]
title = books.short_book_title.iloc[bi]
print("book", bi, title, flush=True)

ctl.mode, ctl.L = "dense", 0
t = time.time()
with torch.no_grad():
    out = model(input_ids=ids[: args.L][None], use_cache=True, logits_to_keep=1)
cache = out.past_key_values
print(f"prefill {args.L} in {time.time() - t:.0f}s", flush=True)
L = args.L
if args.dump_dir:
    os.makedirs(args.dump_dir, exist_ok=True)
    lay = cache.layers[args.dump_layer]
    for name, x in (("k", lay.keys), ("v", lay.values)):
        x[0].contiguous().view(torch.int16).numpy().tofile(os.path.join(args.dump_dir, f"{name}_l{args.dump_layer}_{L}.bf16"))
    print("dumped layer", args.dump_layer, tuple(lay.keys.shape), flush=True)

ctl.reset_prefix(L)
ctl.budget = max(64, int(round(args.frac * L / 16)) * 16)
ctl.record, ctl.record_size = {}, {}
nll = []
t = time.time()
with torch.no_grad():
    for s in range(args.T - 1):
        ctl.mode, ctl.method = "sparse", "cluster_cent"
        o = model(input_ids=ids[L + s: L + s + 1][None], position_ids=torch.tensor([[L + s]]),
                  past_key_values=cache, use_cache=True)
        nll.append(float(torch.nn.functional.cross_entropy(o.logits[0, -1].float()[None], ids[L + s + 1: L + s + 2])))
print(f"decoded {args.T - 1} steps in {time.time() - t:.0f}s (index build {ctl.meta_time:.0f}s)", flush=True)
cache.crop(-(args.T - 1))
ctl.mode, ctl.L = "dense", 0
with torch.no_grad():
    o = model(input_ids=ids[L:][None], position_ids=torch.arange(L, L + args.T)[None], past_key_values=cache, use_cache=True)
dense_nll = float(torch.nn.functional.cross_entropy(o.logits[0, :-1].float(), ids[L + 1:]))

wins = [int(w) for w in args.windows.split(",")]
res = {"args": vars(args), "book": title, "budget": ctl.budget, "nll_sparse": float(np.mean(nll)), "nll_dense": dense_nll,
       "layers": {}}
for layer, steps in sorted(ctl.record.items()):
    sel = torch.stack(steps)            # (S, H, C)
    size = ctl.record_size[layer].float()  # (H, C)
    read = (sel.float() * size).sum(-1)    # (S, H) tokens read per step
    r = {"read_frac": float(read.mean() / L)}
    prev = (sel[1:] & sel[:-1]).float()
    r["same_as_prev"] = float(((prev * size).sum(-1) / read[1:]).mean())
    for w in wins:
        hits, union = [], []
        for s in range(w, sel.shape[0]):
            ram = sel[s - w: s].any(0)
            hits.append(float((((sel[s] & ram).float() * size).sum(-1) / read[s]).mean()))
            union.append(float((ram.float() * size).sum(-1).mean() / L))
        r[f"hit_w{w}"] = float(np.mean(hits))
        r[f"ram_w{w}"] = float(np.mean(union))
    res["layers"][layer] = r
    print(layer, {k: round(v, 3) for k, v in r.items()}, flush=True)
keys = list(next(iter(res["layers"].values())).keys())
res["mean"] = {k: float(np.mean([r[k] for r in res["layers"].values()])) for k in keys}
print("mean", {k: round(v, 3) for k, v in res["mean"].items()}, "nll sparse", round(res["nll_sparse"], 3), "dense", round(dense_nll, 3))
json.dump(res, open(args.out, "w"), indent=1)
