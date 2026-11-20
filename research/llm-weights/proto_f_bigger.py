"""F: perplexity of bigger models with no retrieval on the same 8,192 tokens
as B and D (16 ChloeDB docs, 8 strides of 64 after 128 tokens of context),
to ask whether a small model plus ChloeDB rows replaces a bigger model's
knowledge. llama.cpp, Q4_K_M weights."""
import json, sys, numpy as np
from llama_cpp import Llama
EVAL = ["python", "netops", "javascript", "connectors", "security", "earth", "opensearch", "graphs",
        "arxiv", "hn", "apps", "postgres", "troubleshooting", "crawler", "sqlite", "frameworks"]
out = {}
for mf in sys.argv[2:]:
    llm = Llama(mf, n_ctx=512, n_threads=4, logits_all=True, verbose=False); nll = []
    for e in EVAL:
        toks = llm.tokenize(open(f"/home/user/chloedb/docs/chloedb/{e}.md").read().encode(), add_bos=False)
        for s in range(128, 128 + 8 * 64, 64):
            seg = toks[s - 128:s + 64]; llm.reset(); llm.eval(seg)
            lg = np.asarray(llm.scores[127:191], dtype=np.float64); lg -= lg.max(1, keepdims=True)
            lp = lg - np.log(np.exp(lg).sum(1, keepdims=True)); nll += list(-lp[np.arange(64), seg[128:192]])
    out[mf.split("/")[-1]] = {"ppl_all": float(np.exp(np.mean(nll))), "ppl_test": float(np.exp(np.mean(nll[4 * 512:])))}
    print(out, flush=True)
json.dump(out, open(sys.argv[1], "w"), indent=1)
