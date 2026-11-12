"""Use 1: multi-document QA from stored per-document KV caches (LongBench).

  python run_rag.py --n 30 --out results/rag_1p5b.json [--server http://127.0.0.1:7700]
"""
import argparse, json, os, re, time, random
import torch
import requests
from transformers import AutoModelForCausalLM, AutoTokenizer
import kvsel
from kvsel import ctl
from kvrag import Runner, score

ap = argparse.ArgumentParser()
ap.add_argument("--model", default="Qwen/Qwen2.5-1.5B-Instruct")
ap.add_argument("--data", default="longbench/data")
ap.add_argument("--sets", default="hotpotqa_e,2wikimqa_e")
ap.add_argument("--n", type=int, default=30, help="examples per set")
ap.add_argument("--min-tokens", type=int, default=3000)
ap.add_argument("--max-tokens", type=int, default=9000)
ap.add_argument("--frac", type=float, default=0.0625)
ap.add_argument("--topk", type=int, default=3)
ap.add_argument("--server", default=None, help="chloedb-server for BM25 (conditions bm25_*)")
ap.add_argument("--cache-dir", default="/tmp/kvcache")
ap.add_argument("--out", required=True)
args = ap.parse_args()

torch.manual_seed(0)
torch.set_num_threads(os.cpu_count())
tok = AutoTokenizer.from_pretrained(args.model)
model = AutoModelForCausalLM.from_pretrained(args.model, dtype=torch.bfloat16, attn_implementation="kvsel").eval()
R = Runner(model, tok)
os.makedirs(args.cache_dir, exist_ok=True)

INSTR = "Answer the question based on the given passages. Only give me the answer and do not output any other words."
HEADER = ("<|im_start|>system\nYou are a helpful assistant.<|im_end|>\n<|im_start|>user\n"
          f"{INSTR}\n\nThe following are given passages.\n")
header_ids = R.ids(HEADER)


def sql(stmt, **params):
    r = requests.post(f"{args.server}/v1/sql" if not stmt.lstrip().upper().startswith("SELECT") else f"{args.server}/v1/query",
                      json={"sql": stmt, "params": params, "objects": True}, timeout=60)
    r.raise_for_status()
    return r.json()


if args.server:
    try:
        sql("CREATE TABLE passages (id KEYWORD PRIMARY KEY, ex KEYWORD, body TEXT)")
    except requests.HTTPError:
        pass  # exists


def bm25_top(ex_id, docs, question, k):
    ops = [{"op": "upsert", "table": "passages", "doc": {"id": f"{ex_id}/{i}", "ex": ex_id, "body": d}} for i, d in enumerate(docs)]
    requests.post(f"{args.server}/v1/write", json={"ops": ops}, timeout=60).raise_for_status()
    q = " ".join(re.findall(r"[A-Za-z0-9]+", question)).lower()
    t = time.time()
    res = sql("SELECT id FROM passages WHERE ex = :ex ORDER BY bm25(body, :q) DESC LIMIT %d" % k, ex=ex_id, q=q)
    took = time.time() - t
    return sorted(int(r["id"].split("/")[1]) for r in res["rows"]), took


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


results = {"args": vars(args), "runs": []}
hl = R.layers(R.prefill(header_ids))
h = header_ids.numel()
for name, i, r in examples():
    ex_id = f"{name}-{i}"
    bodies = [p.strip() for p in re.split(r"Passage \d+:\n", r["context"]) if p.strip()]
    docs = [f"Passage {j + 1}:\n{b}\n" for j, b in enumerate(bodies)]
    doc_ids = [R.ids(d) for d in docs]
    tail_ids = R.ids(f"\n{INSTR}\n\nQuestion: {r['input']}<|im_end|>\n<|im_start|>assistant\n")
    P = h + sum(d.numel() for d in doc_ids)
    run = {"ex": ex_id, "answers": r["answers"], "tokens": P, "docs": len(docs), "pred": {}, "f1": {}, "em": {}, "t": {}}

    def record(cond, pred, t_ans):
        run["pred"][cond] = pred
        run["f1"][cond], run["em"][cond] = score(pred, r["answers"])
        run["t"][f"answer_{cond}"] = t_ans

    def timed(f, *a, **k):
        t = time.time(); x = f(*a, **k); return x, time.time() - t

    # closed book
    c = R.prefill(header_ids)
    pred, ta = timed(R.answer, c, h, tail_ids)
    record("closed", pred, ta)
    # full: everything prefilled together, as re-reading the text does
    c, tp = timed(R.prefill, torch.cat([header_ids] + doc_ids))
    run["t"]["prefill_full"] = tp
    pred, ta = timed(R.answer, c, P, tail_ids); record("full", pred, ta)
    mt = ctl.meta_time
    pred, ta = timed(R.answer, c, P, tail_ids, "sparse", "cluster_cent", args.frac); record("full_sparse", pred, ta)
    run["t"]["index_full"] = ctl.meta_time - mt
    del c
    # stored per-document caches: built once (timed), saved, loaded, stitched
    t = time.time()
    dcs = [R.doc_cache(header_ids, d) for d in doc_ids]
    run["t"]["build_docs"] = time.time() - t
    paths = [os.path.join(args.cache_dir, f"{ex_id}-{j}.pt") for j in range(len(dcs))]
    t = time.time()
    for p, dc in zip(paths, dcs):
        torch.save(dc, p)
    run["t"]["save_docs"] = time.time() - t
    run["bytes_docs"] = sum(os.path.getsize(p) for p in paths)
    t = time.time()
    dcs = [torch.load(p) for p in paths]
    run["t"]["load_docs"] = time.time() - t
    (c, P2), ts = timed(R.stitch, hl, dcs)
    assert P2 == P
    run["t"]["stitch"] = ts
    pred, ta = timed(R.answer, c, P, tail_ids); record("stitch", pred, ta)
    mt = ctl.meta_time
    pred, ta = timed(R.answer, c, P, tail_ids, "sparse", "cluster_cent", args.frac); record("stitch_sparse", pred, ta)
    run["t"]["index_stitch"] = ctl.meta_time - mt
    del c
    # retrieval first: ChloeDB BM25 picks the top documents
    if args.server:
        top, tq = bm25_top(ex_id, bodies, r["input"], args.topk)
        run["bm25_top"] = top
        run["t"]["bm25"] = tq
        sel = [doc_ids[j] for j in top]
        Pk = h + sum(d.numel() for d in sel)
        c, tp = timed(R.prefill, torch.cat([header_ids] + sel))
        run["t"]["prefill_bm25"] = tp
        pred, ta = timed(R.answer, c, Pk, tail_ids); record("bm25_text", pred, ta)
        del c
        (c, _), ts = timed(R.stitch, hl, [dcs[j] for j in top])
        run["t"]["stitch_bm25"] = ts
        pred, ta = timed(R.answer, c, Pk, tail_ids); record("bm25_stitch", pred, ta)
        del c
    for p in paths:
        os.remove(p)
    results["runs"].append(run)
    print(ex_id, P, "f1", {k: round(v, 2) for k, v in run["f1"].items()},
          "t", {k: round(v, 1) for k, v in run["t"].items()}, flush=True)
    json.dump(results, open(args.out, "w"))
print("done", flush=True)
