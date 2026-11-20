"""Ideas 3 and 1, data collection. A small model reads ChloeDB's own docs
(written after its training, so it cannot know them) and we measure how well
it predicts each next 64 tokens with:
  none   the previous 128 tokens only
  once   + the best BM25 chunk for the segment's first 64 tokens, kept
  every  + the best BM25 chunk for the last 32 tokens, re-retrieved every 64
  cand k + each of the top 6 BM25 candidates (for idea 1's labels)
Chunks of the document being read are excluded, as In-Context RALM does.
Each stride's features (as a rank log would log them) and NLLs go to JSONL."""
import json, sys, os, time, re, requests, numpy as np
from llama_cpp import Llama

URL = "http://127.0.0.1:7700"
DOCS = ["python", "netops", "javascript", "connectors", "security", "earth", "opensearch", "graphs",
        "arxiv", "hn", "apps", "postgres", "troubleshooting", "crawler", "sqlite", "frameworks"]
CTX, STRIDE, START, END, K, PASS_TOK = 128, 64, 128, 128 + 8 * 64, 6, 160

def bm25(q, doc, k):
    t = time.perf_counter()
    r = requests.post(URL + "/v1/sql", json={"sql": "SELECT id, doc, pos, body, score() FROM doc_chunks WHERE doc != :d ORDER BY bm25(body, :q) DESC LIMIT %d" % k,
                                           "params": {"d": doc, "q": q}}).json()
    return r["rows"], (time.perf_counter() - t) * 1000

def words(s): return set(re.findall(r"[a-z0-9_]+", s.lower()))

def main(model_file, out):
    llm = Llama(model_file, n_ctx=1024, n_threads=4, logits_all=True, verbose=False)
    def nll(prefix, ctx, tgt):
        toks = prefix + ctx + tgt
        llm.reset(); llm.eval(toks)
        lg = np.asarray(llm.scores[len(toks) - len(tgt) - 1: len(toks) - 1], dtype=np.float64)
        lg -= lg.max(1, keepdims=True); lp = lg - np.log(np.exp(lg).sum(1, keepdims=True))
        return float(-lp[np.arange(len(tgt)), tgt].mean())
    def ptoks(body): return llm.tokenize((body + "\n\n").encode(), add_bos=False)[:PASS_TOK]
    f = open(out, "w")
    for name in DOCS:
        doc = f"chloedb/{name}.md"
        toks = llm.tokenize(open(f"/home/user/chloedb/docs/{doc}").read().encode(), add_bos=False)[:END]
        if len(toks) < END: continue
        once_q = llm.detokenize(toks[:64]).decode(errors="ignore")
        once_rows, _ = bm25(once_q, doc, 1)
        once_p = ptoks(once_rows[0][3]) if once_rows else []
        for s in range(START, END, STRIDE):
            ctx, tgt = toks[s - CTX:s], toks[s:s + STRIDE]
            q = llm.detokenize(toks[s - 32:s]).decode(errors="ignore")
            cands, ms = bm25(q, doc, K)
            rec = {"doc": doc, "pos": s, "query": q, "retrieval_ms": round(ms, 2),
                   "none": nll([], ctx, tgt), "once": nll(once_p, ctx, tgt) if once_p else None, "cands": []}
            ctx_words, q_words = words(llm.detokenize(ctx).decode(errors="ignore")), words(q)
            top = cands[0][4] if cands else 1.0
            for rank, (cid, cdoc, cpos, body, sc) in enumerate(cands):
                bw = words(body)
                rec["cands"].append({"id": cid, "nll": nll(ptoks(body), ctx, tgt), "features": {
                    "bm25": sc, "rank": rank, "bm25_rel": sc / top, "len_words": len(body.split()),
                    "q_overlap": len(bw & q_words) / max(1, len(q_words)),
                    "ctx_overlap": len(bw & ctx_words) / max(1, len(ctx_words)),
                    "ctx_jaccard": len(bw & ctx_words) / max(1, len(bw | ctx_words)),
                    "chunk_pos": cpos, "same_dir": int(cdoc.split("/")[0] == doc.split("/")[0]),
                    "ref_doc": int(cdoc in ("chloedb/server.md", "chloedb/query-language.md"))}})
            f.write(json.dumps(rec) + "\n"); f.flush()
            print(doc, s, round(rec["none"], 3), rec["once"] and round(rec["once"], 3),
                  [round(c["nll"], 3) for c in rec["cands"]], flush=True)

if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
