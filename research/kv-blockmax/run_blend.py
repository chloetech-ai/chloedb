"""Next step (e): CacheBlend-style repair of stitched per-passage caches.

The same 40 LongBench-E questions as run_rag.py (deterministic, so its `full`
and `stitch` scores carry over; `stitch` is rerun as a check). After stitching,
a share r of the passage tokens is recomputed through every layer, attending to
the stitched cache, and written back (Runner.blend). Which tokens:

  blend_dev_R    the r with the largest K/V deviation at layer 1 from a fresh
                 joint prefill of layers 0-1 (CacheBlend, Yao et al. 2024);
                 layers 0-1 are replaced by that fresh prefill
  blend_rand_R   r chosen at random (same layers 0-1 fix): is the choice doing work
  blend_first_R  the first tokens of every passage, r in all (EPIC/positional)

  python run_blend.py --out results/blend_1p5b.json [--check]
"""
import argparse, json, os, re, time
import torch
from transformers import AutoModelForCausalLM, AutoTokenizer
import kvsel
from kvsel import ctl
from kvrag import Runner, score

ap = argparse.ArgumentParser()
ap.add_argument("--model", default="Qwen/Qwen2.5-1.5B-Instruct")
ap.add_argument("--data", default="longbench/data")
ap.add_argument("--sets", default="hotpotqa_e,2wikimqa_e")
ap.add_argument("--n", type=int, default=20)
ap.add_argument("--min-tokens", type=int, default=3000)
ap.add_argument("--max-tokens", type=int, default=9000)
ap.add_argument("--rates", default="0.05,0.15,0.3")
ap.add_argument("--check", action="store_true", help="blend every passage token on one example: must match full")
ap.add_argument("--out", required=True)
args = ap.parse_args()

torch.manual_seed(0)
torch.set_num_threads(os.cpu_count())
tok = AutoTokenizer.from_pretrained(args.model)
model = AutoModelForCausalLM.from_pretrained(args.model, dtype=torch.bfloat16, attn_implementation="kvsel").eval()
R = Runner(model, tok)

INSTR = "Answer the question based on the given passages. Only give me the answer and do not output any other words."
HEADER = ("<|im_start|>system\nYou are a helpful assistant.<|im_end|>\n<|im_start|>user\n"
          f"{INSTR}\n\nThe following are given passages.\n")
header_ids = R.ids(HEADER)
h = header_ids.numel()
hl = R.layers(R.prefill(header_ids))
rates = [float(x) for x in args.rates.split(",")]


def examples():
    for name in args.sets.split(","):
        rows = [json.loads(l) for l in open(os.path.join(args.data, f"{name}.jsonl"))]
        got = 0
        for i, r in enumerate(rows):
            n = len(tok(r["context"]).input_ids)
            if not (args.min_tokens <= n <= args.max_tokens):
                continue
            yield name, i, r
            got += 1
            if got >= args.n:
                break


def timed(f, *a, **k):
    t = time.time(); x = f(*a, **k); return x, time.time() - t


results = {"args": vars(args), "runs": []}
for name, i, r in examples():
    ex_id = f"{name}-{i}"
    bodies = [p.strip() for p in re.split(r"Passage \d+:\n", r["context"]) if p.strip()]
    doc_ids = [R.ids(f"Passage {j + 1}:\n{b}\n") for j, b in enumerate(bodies)]
    ids = torch.cat([header_ids] + doc_ids)
    P = ids.numel()
    tail_ids = R.ids(f"\n{INSTR}\n\nQuestion: {r['input']}<|im_end|>\n<|im_start|>assistant\n")
    run = {"ex": ex_id, "answers": r["answers"], "tokens": P, "pred": {}, "f1": {}, "em": {}, "t": {}, "recomputed": {}}

    def record(cond, pred, t_ans):
        run["pred"][cond] = pred
        run["f1"][cond], run["em"][cond] = score(pred, r["answers"])
        run["t"][f"answer_{cond}"] = t_ans

    dcs = [R.doc_cache(header_ids, d) for d in doc_ids]
    starts, s = [], h
    for d in doc_ids:
        starts.append(s); s += d.numel()
    doc_pos = torch.arange(h, P)

    if args.check:
        full = R.prefill(ids)
        a_full = R.answer(full, P, tail_ids)
        c, _ = R.stitch(hl, dcs)
        fresh = R.first_layers(ids)
        R.blend(c, ids, doc_pos, fresh)
        err = max(float((a.keys.float() - b.keys.float()).abs().max()) for a, b in zip(c.layers, full.layers))
        a_bl = R.answer(c, P, tail_ids)
        print("check", ex_id, "max |dK|", err, "full:", a_full, "| blend all:", a_bl, flush=True)
        break

    (c, _), ts = timed(R.stitch, hl, dcs)
    run["t"]["stitch"] = ts
    pred, ta = timed(R.answer, c, P, tail_ids); record("stitch", pred, ta)
    del c
    fresh, tf = timed(R.first_layers, ids)
    run["t"]["first_layers"] = tf
    c, _ = R.stitch(hl, dcs)
    dev = R.deviation(c, fresh)[h:]   # passage tokens only (the header is exact)
    del c
    g = torch.Generator().manual_seed(i)
    rand_order = torch.randperm(P - h, generator=g)
    for rate in rates:
        m = max(1, int(round(rate * (P - h))))
        per_doc = max(1, m // len(doc_ids))
        choices = {
            "dev": h + dev.topk(m).indices,
            "rand": h + rand_order[:m],
            "first": torch.cat([torch.arange(st, min(st + per_doc, st + d.numel())) for st, d in zip(starts, doc_ids)]),
        }
        for how, pos in choices.items():
            cond = f"blend_{how}_{int(rate * 100)}"
            c, _ = R.stitch(hl, dcs)
            _, tb = timed(R.blend, c, ids, pos, None if how == "first" else fresh)
            run["t"][f"blend_{cond}"] = tb
            run["recomputed"][cond] = int(pos.numel())
            pred, ta = timed(R.answer, c, P, tail_ids); record(cond, pred, ta)
            del c
    results["runs"].append(run)
    print(ex_id, P, {k: round(v, 2) for k, v in run["f1"].items()},
          {k: round(v, 1) for k, v in run["t"].items() if k.startswith("blend") or k == "first_layers"}, flush=True)
    json.dump(results, open(args.out, "w"))
print("done", flush=True)
