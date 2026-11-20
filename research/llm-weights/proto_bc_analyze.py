"""Ideas 3 and 1, analysis of proto_bc_collect.py's JSONL.
Idea 3: perplexity with no retrieval, retrieval once, retrieval every 64 tokens.
Idea 1: a LightGBM LambdaRank model trained on the model's own feedback (how
much each candidate lowered its loss), evaluated on documents it never saw
(4 folds by document): perplexity of the chunk it picks against BM25's top 1."""
import json, sys, math, random
import numpy as np, lightgbm as lgb

recs = [json.loads(l) for l in open(sys.argv[1])]
recs = [r for r in recs if r["cands"]]
FEATS = list(recs[0]["cands"][0]["features"])

def ppl(x): return math.exp(float(np.mean(x)))
def paired(a, b):
    d = np.array(a) - np.array(b); return d.mean(), d.std(ddof=1) / math.sqrt(len(d))

none = [r["none"] for r in recs]
once = [r["once"] if r["once"] is not None else r["none"] for r in recs]
every = [r["cands"][0]["nll"] for r in recs]
oracle = [min([r["none"]] + [c["nll"] for c in r["cands"]]) for r in recs]
out = {"strides": len(recs), "docs": len({r["doc"] for r in recs}), "tokens_scored": 64 * len(recs),
       "retrieval_ms_p50": float(np.median([r["retrieval_ms"] for r in recs]))}
out["idea3_ppl"] = {"none": ppl(none), "once": ppl(once), "every_64": ppl(every), "oracle_of_6_or_none": ppl(oracle)}
out["idea3_nll_delta_vs_none"] = {k: paired(v, none) for k, v in (("once", once), ("every_64", every))}
out["idea3_every_vs_once"] = paired(every, once)

def grade(r, c):
    g = r["none"] - c["nll"]
    return 3 if g > 0.3 else 2 if g > 0.1 else 1 if g > 0 else 0

docs = sorted({r["doc"] for r in recs}); random.Random(7).shuffle(docs)
folds = [docs[i::4] for i in range(4)]
picked, picked_overlap, chosen_rank = [], [], []
for test_docs in folds:
    tr = [r for r in recs if r["doc"] not in test_docs]; te = [r for r in recs if r["doc"] in test_docs]
    X = [[c["features"][f] for f in FEATS] for r in tr for c in r["cands"]]
    y = [grade(r, c) for r in tr for c in r["cands"]]
    m = lgb.LGBMRanker(objective="lambdarank", n_estimators=150, learning_rate=0.05, num_leaves=7,
                       min_child_samples=10, verbose=-1)
    m.fit(np.array(X), np.array(y), group=[len(r["cands"]) for r in tr])
    for r in te:
        s = m.predict(np.array([[c["features"][f] for f in FEATS] for c in r["cands"]]))
        i = int(np.argmax(s)); picked.append((r, r["cands"][i]["nll"])); chosen_rank.append(i)
        j = int(np.argmax([c["features"]["ctx_overlap"] for c in r["cands"]])); picked_overlap.append(r["cands"][j]["nll"])
order = [r for r, _ in picked]
bm = [r["cands"][0]["nll"] for r in order]; gb = [n for _, n in picked]
nn = [r["none"] for r in order]; orc = [min(c["nll"] for c in r["cands"]) for r in order]
out["idea1_ppl_held_out"] = {"none": ppl(nn), "bm25_top1": ppl(bm), "context_overlap_top1": ppl(picked_overlap),
                             "gbdt_from_model_feedback": ppl(gb), "oracle_of_6": ppl(orc)}
out["idea1_gbdt_vs_bm25_nll"] = paired(gb, bm)
out["idea1_gap_closed"] = (np.mean(bm) - np.mean(gb)) / (np.mean(bm) - np.mean(orc))
out["idea1_gbdt_picked_rank_share"] = {str(k): chosen_rank.count(k) / len(chosen_rank) for k in range(6)}
out["idea1_oracle_rank_share"] = {str(k): sum(1 for r in recs if int(np.argmin([c["nll"] for c in r["cands"]])) == k) / len(recs) for k in range(6)}
m = lgb.LGBMRanker(objective="lambdarank", n_estimators=150, learning_rate=0.05, num_leaves=7, min_child_samples=10, verbose=-1)
m.fit(np.array([[c["features"][f] for f in FEATS] for r in recs for c in r["cands"]]),
      np.array([grade(r, c) for r in recs for c in r["cands"]]), group=[len(r["cands"]) for r in recs])
out["idea1_feature_importance"] = dict(sorted(zip(FEATS, map(int, m.booster_.feature_importance("gain"))), key=lambda x: -x[1]))
print(json.dumps(out, indent=1))
