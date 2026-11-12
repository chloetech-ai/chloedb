"""Use 3: a project's memory as one stored KV cache, on a local model, against
retrieval through ChloeDB (what ChloeCtx does today: search, then put the text
in the prompt).

The corpus is ChloeCtx's own documentation (about 15k tokens); the questions,
in ctx/questions.json, each have a short answer stated once in it.

  closed        no context
  cache         the whole corpus, prefilled once and kept; each question runs
                only its own tokens and the answer over it (dense attention)
  cache_sparse  the same cache, read with sparse retrieval attention (~6%)
  bm25_text     ChloeDB BM25 picks the top chunks of the corpus; their text is
                prefilled with the question (today's RAG)
  bm25_stitch   the same chunks from stored per-chunk caches

  python run_ctx.py --server http://127.0.0.1:7700 --out results/ctx_1p5b.json
"""
import argparse, json, os, re, time
import torch, requests
from transformers import AutoModelForCausalLM, AutoTokenizer
import kvsel
from kvsel import ctl
from kvrag import Runner, normalize

ap = argparse.ArgumentParser()
ap.add_argument("--model", default="Qwen/Qwen2.5-1.5B-Instruct")
ap.add_argument("--repo", default="../..")
ap.add_argument("--files", default="docs/chloectx/index.md,docs/chloectx/setup.md,docs/chloectx/inspector.md,AGENTS.md")
ap.add_argument("--chunk-words", type=int, default=250)
ap.add_argument("--topk", type=int, default=4)
ap.add_argument("--frac", type=float, default=0.0625)
ap.add_argument("--server", required=True)
ap.add_argument("--out", required=True)
args = ap.parse_args()

torch.set_num_threads(os.cpu_count())
tok = AutoTokenizer.from_pretrained(args.model)
model = AutoModelForCausalLM.from_pretrained(args.model, dtype=torch.bfloat16, attn_implementation="kvsel").eval()
R = Runner(model, tok)
qs = json.load(open(os.path.join(os.path.dirname(__file__) or ".", "ctx/questions.json")))

INSTR = "Answer the question from the project documentation above. Answer with a few words only."
HEADER = ("<|im_start|>system\nYou are a helpful assistant.<|im_end|>\n<|im_start|>user\n"
          "Here is the documentation of a software project.\n\n")
header_ids = R.ids(HEADER)
h = header_ids.numel()

# chunks: paragraphs grouped up to about chunk-words words, never across files
chunks = []
for f in args.files.split(","):
    text = open(os.path.join(args.repo, f)).read()
    cur, n = [], 0
    for para in re.split(r"\n\s*\n", text):
        w = len(para.split())
        if cur and n + w > args.chunk_words:
            chunks.append("\n\n".join(cur) + "\n\n"); cur, n = [], 0
        cur.append(para); n += w
    if cur:
        chunks.append("\n\n".join(cur) + "\n\n")
chunk_ids = [R.ids(c) for c in chunks]
P = h + sum(c.numel() for c in chunk_ids)
print(f"{len(chunks)} chunks, {P} tokens", flush=True)


def sql(stmt, **params):
    url = f"{args.server}/v1/query" if stmt.lstrip().upper().startswith("SELECT") else f"{args.server}/v1/sql"
    r = requests.post(url, json={"sql": stmt, "params": params, "objects": True}, timeout=60)
    r.raise_for_status()
    return r.json()


try:
    sql("CREATE TABLE docs (id INT PRIMARY KEY, body TEXT)")
except requests.HTTPError:
    pass
requests.post(f"{args.server}/v1/write", json={"ops": [
    {"op": "upsert", "table": "docs", "doc": {"id": i, "body": c}} for i, c in enumerate(chunks)]}, timeout=60).raise_for_status()


def correct(pred, keys):
    p = normalize(pred)
    return any(normalize(k) in p for k in keys)


res = {"args": vars(args), "tokens": P, "chunks": len(chunks), "t": {}, "runs": []}
t = time.time()
full = R.prefill(torch.cat([header_ids] + chunk_ids))
res["t"]["prefill_corpus"] = time.time() - t
t = time.time()
hl = R.layers(R.prefill(header_ids))
ccs = [R.doc_cache(header_ids, c) for c in chunk_ids]
res["t"]["build_chunk_caches"] = time.time() - t
res["cache_bytes"] = sum(k.numel() * 2 * 2 for k, _ in R.layers(full))
print("prefill", round(res["t"]["prefill_corpus"]), "s; chunk caches", round(res["t"]["build_chunk_caches"]), "s", flush=True)

for qi, item in enumerate(qs):
    tail = R.ids(f"{INSTR}\n\nQuestion: {item['q']}<|im_end|>\n<|im_start|>assistant\n")
    run = {"q": item["q"], "a": item["a"], "pred": {}, "ok": {}, "t": {}}

    def rec(cond, f, *a):
        t = time.time(); pred = f(*a); run["t"][cond] = time.time() - t
        run["pred"][cond] = pred; run["ok"][cond] = correct(pred, item["a"])

    c = R.prefill(header_ids)
    rec("closed", R.answer, c, h, tail)
    rec("cache", R.answer, full, P, tail)
    mt = ctl.meta_time
    rec("cache_sparse", R.answer, full, P, tail, "sparse", "cluster_cent", args.frac, 32, "corpus")
    run["t"]["index_build"] = ctl.meta_time - mt
    q = " ".join(re.findall(r"[A-Za-z0-9_]+", item["q"])).lower()
    t = time.time()
    top = sorted(r["id"] for r in sql(f"SELECT id FROM docs ORDER BY bm25(body, :q) DESC LIMIT {args.topk}", q=q)["rows"])
    run["t"]["bm25"] = time.time() - t
    run["top"] = top
    sel = [chunk_ids[i] for i in top]
    Pk = h + sum(s.numel() for s in sel)

    def text_rag():
        c = R.prefill(torch.cat([header_ids] + sel))
        return R.answer(c, Pk, tail)

    def stitch_rag():
        c, _ = R.stitch(hl, [ccs[i] for i in top])
        return R.answer(c, Pk, tail)
    rec("bm25_text", text_rag)
    rec("bm25_stitch", stitch_rag)
    res["runs"].append(run)
    print(qi, {k: int(v) for k, v in run["ok"].items()}, {k: round(v, 1) for k, v in run["t"].items()},
          {k: v[:40] for k, v in run["pred"].items()}, flush=True)
    json.dump(res, open(args.out, "w"))
print("done", flush=True)
