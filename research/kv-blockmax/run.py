"""Runs the measurements. See README.md.

  python run.py ppl     --L 16384 --books 4 --out results/ppl_16k.json
  python run.py passkey --L 16384 --trials 6 --out results/passkey_16k.json
"""
import argparse, json, os, random, time, math
import torch
import pandas as pd
from transformers import AutoModelForCausalLM, AutoTokenizer
import kvsel
from kvsel import ctl

ap = argparse.ArgumentParser()
ap.add_argument("mode", choices=["ppl", "passkey"])
ap.add_argument("--model", default="Qwen/Qwen2.5-0.5B-Instruct")
ap.add_argument("--data", default=os.environ.get("PG19", "data/data/test-00000-of-00001-29a571947c0b5ccc.parquet"))
ap.add_argument("--L", type=int, default=8192)
ap.add_argument("--T", type=int, default=256)
ap.add_argument("--books", type=int, default=2)
ap.add_argument("--trials", type=int, default=4)
ap.add_argument("--budgets", default="64,128,256,512,1024,2048")
ap.add_argument("--methods", default=",".join(kvsel.ALL_METHODS))
ap.add_argument("--stat-methods", default=None)
ap.add_argument("--dense-layers", default="0,1")
ap.add_argument("--no-stats", action="store_true")
ap.add_argument("--out", required=True)
args = ap.parse_args()

torch.manual_seed(0)
torch.set_num_threads(os.cpu_count())
tok = AutoTokenizer.from_pretrained(args.model)
model = AutoModelForCausalLM.from_pretrained(args.model, dtype=torch.bfloat16, attn_implementation="kvsel")
model.eval()
budgets = [int(b) for b in args.budgets.split(",")]
methods = args.methods.split(",")
ctl.stat_budgets = budgets
ctl.stat_methods = (args.stat_methods or args.methods).split(",")
ctl.dense_layers = {int(x) for x in args.dense_layers.split(",") if x != ""}
books = pd.read_parquet(args.data)
nlayers = model.config.num_hidden_layers


def prefill(ids):
    ctl.mode, ctl.L = "dense", 0
    t = time.time()
    with torch.no_grad():
        out = model(input_ids=ids.unsqueeze(0), use_cache=True, logits_to_keep=1)
    print(f"  prefill {ids.numel()} tokens in {time.time()-t:.0f}s", flush=True)
    ctl.reset_prefix(ids.numel())
    return out.past_key_values


def run_chunk(cache, chunk, mode, method=None, budget=0):
    ctl.mode, ctl.method, ctl.budget = mode, method, budget
    L = ctl.L
    with torch.no_grad():
        out = model(input_ids=chunk.unsqueeze(0), past_key_values=cache, use_cache=True,
                    position_ids=torch.arange(L, L + chunk.numel()).unsqueeze(0))
    cache.crop(-chunk.numel())
    return out.logits[0].float()


def stats_summary():
    """Turn the accumulated stats into per-layer means over queries and heads."""
    res = {}
    for k, v in ctl.stats.items():
        if k[0] == "count":
            continue
        layer = k[-1]
        cnt = ctl.stats[("count", layer)]
        res["|".join(map(str, k))] = (v / cnt).mean().item()
        if k[0] in ("recall", "oerr", "prefix_mass"):
            res["|".join(map(str, k)) + "|perhead"] = (v / cnt).flatten().tolist()
    return res


def book_ids(i, n):
    text = books.text.iloc[i]
    ids = tok(text[: n * 6], return_tensors="pt").input_ids[0]
    return ids


results = {"args": vars(args), "runs": []}
if args.mode == "ppl":
    order = [i for i in range(len(books))]
    random.Random(0).shuffle(order)
    done = 0
    for bi in order:
        ids = book_ids(bi, args.L + args.T + 2000)
        if ids.numel() < args.L + args.T + 1000:
            continue
        # skip the Gutenberg header: start 1000 tokens in
        ids = ids[1000: 1000 + args.L + args.T]
        title = books.short_book_title.iloc[bi]
        print(f"book {bi} {title!r}", flush=True)
        cache = prefill(ids[: args.L])
        chunk = ids[args.L:]
        tgt = chunk[1:]
        run = {"book": title, "nll": {}}
        lg = run_chunk(cache, chunk, "dense")
        run["nll"]["dense"] = torch.nn.functional.cross_entropy(lg[:-1], tgt).item()
        if not args.no_stats:
            ctl.stats = {}
            t = time.time()
            lg2 = run_chunk(cache, chunk, "stats")
            print(f"  stats in {time.time()-t:.0f}s", flush=True)
            run["nll"]["dense_check"] = torch.nn.functional.cross_entropy(lg2[:-1], tgt).item()
            run["stats"] = stats_summary()
        for m in methods:
            for B in budgets:
                lg = run_chunk(cache, chunk, "sparse", m, B)
                run["nll"][f"{m}|{B}"] = torch.nn.functional.cross_entropy(lg[:-1], tgt).item()
            print(f"  {m}: " + " ".join(f"{B}:{run['nll'][f'{m}|{B}']:.3f}" for B in budgets)
                  + f"  (dense {run['nll']['dense']:.3f})", flush=True)
        results["runs"].append(run)
        done += 1
        del cache
        os.makedirs(os.path.dirname(args.out) or ".", exist_ok=True)
        json.dump(results, open(args.out, "w"))
        if done >= args.books:
            break

else:  # passkey
    rng = random.Random(1)
    hay = book_ids(3, args.L * 2)
    hay = hay[1000:]
    # the instruct model's chat format: the haystack is the user's message, the
    # question ends it, and the answer is scored under teacher forcing
    head = tok("<|im_start|>system\nYou are Qwen, created by Alibaba Cloud. You are a helpful assistant."
               "<|im_end|>\n<|im_start|>user\n", return_tensors="pt").input_ids[0]
    q_ids = tok("\n\nWhat is the pass key mentioned in the text above? Answer with the number only."
                "<|im_end|>\n<|im_start|>assistant\n", return_tensors="pt").input_ids[0]
    for trial in range(args.trials):
        key = str(rng.randint(10000, 99999))
        depth = [0.1, 0.5, 0.9, 0.3, 0.7, 0.2, 0.8, 0.6][trial % 8]
        needle = tok(f" The pass key is {key}. Remember it. {key} is the pass key. ", return_tensors="pt").input_ids[0]
        n = args.L - head.numel() - needle.numel()
        cut = int(n * depth)
        prefix = torch.cat([head, hay[:cut], needle, hay[cut:n]])
        assert prefix.numel() == args.L
        ans = tok(key, return_tensors="pt").input_ids[0]
        chunk = torch.cat([q_ids, ans])
        na = ans.numel()
        print(f"trial {trial} depth {depth} key {key}", flush=True)
        cache = prefill(prefix)
        run = {"depth": depth, "key": key, "ok": {}, "nll": {}}

        def score(lg):
            pred = lg[-na - 1:-1].argmax(-1)
            ok = bool((pred == chunk[-na:]).all())
            nll = torch.nn.functional.cross_entropy(lg[-na - 1:-1], chunk[-na:]).item()
            return ok, nll
        run["ok"]["dense"], run["nll"]["dense"] = score(run_chunk(cache, chunk, "dense"))
        for m in methods:
            for B in budgets:
                run["ok"][f"{m}|{B}"], run["nll"][f"{m}|{B}"] = score(run_chunk(cache, chunk, "sparse", m, B))
            print(f"  {m}: " + " ".join(f"{B}:{int(run['ok'][f'{m}|{B}'])}" for B in budgets)
                  + f"  (dense {int(run['ok']['dense'])})", flush=True)
        results["runs"].append(run)
        del cache
        os.makedirs(os.path.dirname(args.out) or ".", exist_ok=True)
        json.dump(results, open(args.out, "w"))
print("done", flush=True)
