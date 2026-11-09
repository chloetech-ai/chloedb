"""On-device slices, search side, weighted by query volume: the study of
`search.py` on a query log with counts.

ORCAS has no counts, so `search.py` counts every distinct query string once.
Here the queries are the AOL search log (March to May 2006: 36 million rows, about 21 million
searches by 650,000 users), each search counted once, and
the corpus is again MS MARCO's 8.8 million passages (the database of
`search.py`), so the cloud's answer is BM25 over a modern web corpus rather
than over the pages AOL's users saw. The AOL log is the one released in 2006
and withdrawn for its privacy failings; it is used here, as in much IR
research (ir_datasets' `aol-ia`), only for counts and words: nothing written
by this script or its README quotes a query.

- Time split: training is every search before 17 May 2006, test the two
  weeks after it. Queries are normalised as in `search.py` (ASCII, words =
  runs of ASCII letters and digits, at least one word not a stopword).
- Training queries: the 1,000,000 most searched training strings, with their
  counts. Test queries: 200,000 test searches sampled uniformly, so each
  distinct string weighs its sample count and every share below is a share
  of test search volume.
- Slices are chosen as in `search.py` but weighted by training counts:
  `cov` greedily completes the training query with the most searches per
  missing passage, `pop` takes passages by the searches whose top 10 hold
  them.
- Besides the proof (word and pair bounds), a device can hold a result
  cache: the cloud's top 10 ids for the most searched training strings.
  A cached answer is complete on the device only when the slice also holds
  its passages.

Stages, each writing to --work (`--orcas` is `search.py`'s work directory,
for its corpus and database):

    python aol.py prep --data ~/data/slices        # aol-data.tar.gz -> train/test, counts
    python aol.py cloud train && python aol.py cloud test
    python aol.py select && python aol.py load --tiers
    python aol.py eval
    python aol.py bytes
"""
import argparse
import gzip
import heapq
import io
import json
import os
import sys
import tarfile
from collections import Counter

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import search  # noqa: E402
from search import K, KS, METHODS, NOT, QUERY_STOPWORDS, SEED, TIERS, db_path, log, normalise, queries, run_all, tier_of, top  # noqa: E402

TEST_FROM = "2006-05-17"
TRAIN = 1_000_000
TEST_SAMPLE = 200_000
CACHE = [10_000, 30_000, 100_000, 300_000, 1_000_000]  # answers a result cache holds
ANSWER_BYTES = 4 * K  # ten 4-byte ids, plus the query string
BUDGET_MB = 50
BANDS = [(0, 0), (1, 9), (10, 99), (100, 999), (1000, None)]  # training searches of a test string


def searches(path):
    """(query, time) of each search. Consecutive rows of one user and one
    query are one search: its clicks, and its later rows are requests for
    the next page of results (the log's README), not new searches."""
    with tarfile.open(path) as tar:
        for m in sorted(tar.getmembers(), key=lambda m: m.name):
            if not m.name.endswith(".txt.gz"):
                continue
            f = io.TextIOWrapper(gzip.GzipFile(fileobj=tar.extractfile(m)), encoding="utf-8", errors="replace")
            next(f)  # header
            last = None
            for line in f:
                p = line.rstrip("\n").split("\t")
                if len(p) < 3:
                    continue
                key = (p[0], p[1])
                if key == last:
                    continue
                last = key
                yield p[1], p[2]


def cmd_prep(a):
    os.makedirs(a.work, exist_ok=True)
    for name in ("corpus.jsonl", "cloud.strata"):
        dst = os.path.join(a.work, name)
        if not os.path.lexists(dst):
            os.symlink(os.path.join(a.orcas, name), dst)
    train, test = Counter(), []
    raw = skipped = 0
    for q, t in searches(os.path.join(a.data, "aol-data.tar.gz")):
        raw += 1
        n = normalise(q) if q != "-" else None
        if n is None:
            skipped += 1
            continue
        if t >= TEST_FROM:
            test.append(n)
        else:
            train[n] += 1
    top_train = sorted(train.items(), key=lambda kv: (-kv[1], kv[0]))[:TRAIN]
    with open(os.path.join(a.work, "train.txt"), "w") as f:
        f.write("\n".join(q for q, _ in top_train) + "\n")
    np.save(os.path.join(a.work, "train_counts.npy"), np.array([c for _, c in top_train], np.int64))
    rng = np.random.default_rng(SEED)
    pick = rng.choice(len(test), TEST_SAMPLE, replace=False)
    sample = Counter(test[i] for i in pick)
    tq = sorted(sample, key=lambda q: (-sample[q], q))
    with open(os.path.join(a.work, "test.txt"), "w") as f:
        f.write("\n".join(tq) + "\n")
    np.save(os.path.join(a.work, "test_counts.npy"), np.array([sample[q] for q in tq], np.int64))
    np.save(os.path.join(a.work, "test_train_counts.npy"), np.array([train.get(q, 0) for q in tq], np.int64))
    in_top = set(q for q, _ in top_train)
    stats = {
        "searches": raw, "skipped_dash_non_ascii_or_only_stopwords": skipped,
        "train_searches": int(sum(train.values())), "train_distinct": len(train),
        "train_kept": len(top_train), "train_kept_share_of_searches": sum(c for _, c in top_train) / sum(train.values()),
        "test_searches": len(test), "test_distinct": len(set(test)), "test_sample": TEST_SAMPLE, "test_sample_distinct": len(tq),
        "test_volume_seen_in_training": sum(1 for q in test if q in train) / len(test),
        "test_volume_in_kept_training": sum(1 for q in test if q in in_top) / len(test),
        "docs": json.load(open(os.path.join(a.orcas, "prep.json")))["docs"],
    }
    json.dump(stats, open(os.path.join(a.work, "prep.json"), "w"), indent=1)
    log("prep", stats)


def cmd_select(a):
    """As `search.cmd_select`, weighted by each training query's searches."""
    c = np.load(os.path.join(a.work, "cloud_train.npz"))
    full = c["ids"][:, -1] >= 0
    ids = c["ids"][full]
    w = np.load(os.path.join(a.work, "train_counts.npy"))[full].astype(np.float64)
    n_docs = json.load(open(os.path.join(a.work, "prep.json")))["docs"]
    cap = TIERS[-1]
    tiers = {}
    weight = np.bincount(ids.ravel(), weights=np.repeat(w, K), minlength=n_docs)
    tiers["pop"] = tier_of(np.lexsort((np.arange(n_docs), -weight))[:cap], n_docs)
    nq = len(ids)
    flat = ids.ravel()
    by_doc = np.argsort(flat, kind="stable")
    starts = np.searchsorted(flat[by_doc], np.arange(n_docs + 1))
    missing = np.full(nq, K, np.int32)
    heap = [(-w[q] / K, q, K) for q in range(nq)]
    heapq.heapify(heap)
    chosen = np.zeros(n_docs, bool)
    order = []
    while heap and len(order) < cap:
        _, q, m = heapq.heappop(heap)
        if missing[q] != m or m == 0:
            continue  # stale, or already complete
        for d in ids[q]:
            if chosen[d]:
                continue
            chosen[d] = True
            order.append(int(d))
            for e in by_doc[starts[d]:starts[d + 1]]:
                q2 = e // K
                missing[q2] -= 1
                if missing[q2] > 0:
                    heapq.heappush(heap, (-w[q2] / missing[q2], q2, int(missing[q2])))
    tiers["cov"] = tier_of(np.array(order[:cap], np.int64), n_docs)
    np.savez(os.path.join(a.work, "tiers.npz"), **tiers)
    out = {}
    for m in METHODS:
        held = tiers[m][ids].max(axis=1)
        out[m] = {str(n): float(w[held <= i].sum() / w.sum()) for i, n in enumerate(TIERS)}
    json.dump({"train_queries_full_top10": int(nq), "train_volume_held": out}, open(os.path.join(a.work, "select.json"), "w"), indent=1)
    log("train volume held", out)


def band_of(n):
    for i, (lo, hi) in enumerate(BANDS):
        if n >= lo and (hi is None or n <= hi):
            return i


def cmd_eval(a):
    db = strata.Database.open(db_path(a), threads=1)
    qs = queries(a, "test")
    wt = np.load(os.path.join(a.work, "test_counts.npy")).astype(np.float64)
    trc = np.load(os.path.join(a.work, "test_train_counts.npy"))
    train = queries(a, "train")
    rank = {q: i for i, q in enumerate(train)}  # 0 = most searched
    c = np.load(os.path.join(a.work, "cloud_test.npz"))
    cid, csc = c["ids"], c["scores"]
    keep = np.nonzero(cid[:, -1] >= 0)[0]
    total = wt.sum()
    tiers = np.load(os.path.join(a.work, "tiers.npz"))
    words = sorted({x for i in keep for x in qs[i].split() if x not in QUERY_STOPWORDS})
    log(f"test: {len(qs)} strings, {len(keep)} with a full top 10 ({wt[keep].sum() / total:.3f} of volume), {len(words)} words")
    bands = list(range(NOT + 1))
    best = {}
    for m in METHODS:
        def word_bands(x, m=m):
            return [max([s for _, s in top(db, x, f"WHERE tier_{m} = :b", 1, {"b": b})], default=0.0) for b in bands]
        best[m] = np.array(run_all(words, word_bands, a.threads, f"bands {m}"), np.float64)
    widx = {x: i for i, x in enumerate(words)}
    # Pairs a slice could ship: those of the training queries.
    train_pairs = set()
    for q in train:
        ws = sorted({x for x in q.split() if x not in QUERY_STOPWORDS})
        train_pairs.update((ws[i], ws[j]) for i in range(len(ws)) for j in range(i + 1, len(ws)))
    want = set()
    for i in keep:
        ws = sorted({x for x in qs[i].split() if x not in QUERY_STOPWORDS})
        want.update(p for p in ((ws[x], ws[y]) for x in range(len(ws)) for y in range(x + 1, len(ws))) if p in train_pairs)
    pairs = sorted(want)
    n_train_pairs = len(train_pairs)
    del train_pairs
    sql = ("SELECT score() FROM p WHERE MATCH(body, :m) AND tier_{m} = :b "
           "ORDER BY bm25(body, :q, syntax => 'plain') DESC LIMIT 1")
    pbest = {}
    for m in METHODS:
        def pair_bands(pr, m=m):
            return [max([r[0] for r in db.query(sql.format(m=m), {"m": f"{pr[0]} AND {pr[1]}", "q": f"{pr[0]} {pr[1]}", "b": x}).rows], default=0.0)
                    for x in bands]
        pbest[m] = np.array(run_all(pairs, pair_bands, a.threads, f"pair bands {m}"), np.float64) if pairs else np.zeros((0, len(bands)))
    pidx = {p: i for i, p in enumerate(pairs)}
    log(f"{n_train_pairs} training pairs, {len(pairs)} in the test queries")
    result = {"test_strings": len(qs), "full_top10": int(len(keep)), "volume_full_top10": float(wt[keep].sum() / total),
              "words": len(words), "train_pairs": n_train_pairs, "pairs": len(pairs), "methods": {}}
    cache_rank = np.array([rank.get(qs[i], 1 << 40) for i in keep])
    wk = wt[keep]
    band = np.array([band_of(int(trc[i])) for i in keep])
    for m in METHODS:
        tm = tiers[m]
        per = {}
        for ti, n in enumerate(TIERS):
            resid = best[m][:, ti + 1:].max(axis=1)
            known = best[m][:, : ti + 1].max(axis=1) > 0
            presid = pbest[m][:, ti + 1:].max(axis=1) if len(pairs) else np.zeros(0)
            loc = run_all(list(keep), lambda i: top(db, qs[i], f"WHERE tier_{m} <= :t", K, {"t": ti}), a.threads, f"local {m} tier {n}")
            st = {k: {x: np.zeros(len(keep), bool) for x in ("held", "same", "proved", "proved_pairs", "unknown")} for k in KS}
            viol = 0
            for j, i in enumerate(keep):
                ws = [x for x in qs[i].split() if x not in QUERY_STOPWORDS]
                unk = any(not known[widx[x]] for x in ws)
                single = sum(resid[widx[x]] for x in ws)
                cnt = Counter(ws)
                uniq = sorted(x for x, k in cnt.items() if k == 1)
                gains = []
                for x in range(len(uniq)):
                    for y in range(x + 1, len(uniq)):
                        pi = pidx.get((uniq[x], uniq[y]))
                        if pi is not None:
                            ra, rb = resid[widx[uniq[x]]], resid[widx[uniq[y]]]
                            gains.append((ra + rb - max(presid[pi], ra, rb), uniq[x], uniq[y]))
                gains.sort(reverse=True)
                used, paired = set(), single
                for g, x, y in gains:
                    if g > 0 and x not in used and y not in used:
                        used.update((x, y))
                        paired -= g
                lids = [d for d, _ in loc[j]]
                lsc = [s for _, s in loc[j]]
                for k in KS:
                    s = st[k]
                    full = len(lids) >= k
                    s["held"][j] = tm[cid[i, :k]].max() <= ti
                    s["same"][j] = full and set(lids[:k]) == set(cid[i, :k].tolist())
                    s["unknown"][j] = unk
                    s["proved"][j] = full and not unk and lsc[k - 1] >= single
                    s["proved_pairs"][j] = full and not unk and lsc[k - 1] >= paired
                    if s["proved_pairs"][j] and not s["same"][j] and abs(lsc[k - 1] - float(csc[i, k - 1])) > 1e-4:
                        viol += 1
            out = {"violations": viol}
            for k in KS:
                s = st[k]
                v = lambda mask: float(wk[mask].sum() / wk.sum())  # noqa: E731
                d = {x: v(s[x]) for x in ("held", "same", "proved", "proved_pairs", "unknown")}
                # A result cache of the N most searched training strings: an
                # answer is whole on the device when the slice holds its passages.
                d["cache_ids"] = {str(c_): v(cache_rank < c_) for c_ in CACHE}
                d["cache_held"] = {str(c_): v((cache_rank < c_) & s["held"]) for c_ in CACHE}
                d["proved_or_cache_held"] = {str(c_): v(s["proved_pairs"] | ((cache_rank < c_) & s["held"])) for c_ in CACHE}
                d["by_training_searches"] = {
                    f"{lo}-{hi if hi is not None else ''}": {"volume": v(band == b), "held": float(wk[(band == b) & s["held"]].sum() / max(wk[band == b].sum(), 1)),
                                                             "proved_pairs": float(wk[(band == b) & s["proved_pairs"]].sum() / max(wk[band == b].sum(), 1))}
                    for b, (lo, hi) in enumerate(BANDS)}
                out[str(k)] = d
            per[str(n)] = out
            log(m, n, json.dumps({x: out["10"][x] for x in ("held", "proved", "proved_pairs")}), "top1", json.dumps({x: out["1"][x] for x in ("held", "proved", "proved_pairs")}), "violations", viol)
        result["methods"][m] = per
    json.dump(result, open(os.path.join(a.work, "eval.json"), "w"), indent=1)


strata = None


def main():
    global strata
    ap = argparse.ArgumentParser()
    ap.add_argument("--work", default="/home/user/data/slices/aol")
    ap.add_argument("--orcas", default="/home/user/data/slices/work", help="search.py's work directory")
    ap.add_argument("--threads", type=int, default=4)
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("prep")
    p.add_argument("--data", default="/home/user/data/slices")
    p = sub.add_parser("cloud")
    p.add_argument("which", choices=["train", "test"])
    sub.add_parser("select")
    p = sub.add_parser("load")
    p.add_argument("--tiers", action="store_true")
    sub.add_parser("eval")
    sub.add_parser("bytes")
    a = ap.parse_args()
    if a.cmd != "prep":
        import strata as s
        strata = search.strata = s
    {"prep": cmd_prep, "cloud": search.cmd_cloud, "select": cmd_select, "load": search.cmd_load, "eval": cmd_eval,
     "bytes": search.cmd_bytes}[a.cmd](a)


if __name__ == "__main__":
    main()
