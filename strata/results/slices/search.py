"""On-device slices, search side: how much of a query log a slice of the
corpus answers on the device with a proof that its top k is the cloud's.

The design is the "Strata on the device" report (Claude Doc,
https://claude.ai/artifact/14s2MaZu2wua3Q3NNvbL2E). A slice is a set of
documents shipped to a device, scored there with the cloud's BM25
statistics, so a document scores the same on both sides. With it ships,
for each term of the slice, its residual bound: the most that term adds to
the score of any document left in the cloud. When the k-th local score
reaches the sum of the query's bounds, no cloud-only document can enter
the top k, and the device answers alone; otherwise it asks the cloud.

Data:

- Queries: ORCAS (Craswell et al., 2020; 10.4M Bing queries, non-commercial
  research licence): https://microsoft.github.io/msmarco/ORCAS. ORCAS has
  no query counts, so every query weighs 1, and after normalising
  (ASCII-only queries, words = runs of ASCII letters and digits, as
  Strata's analyser splits them, joined by spaces) and removing duplicate
  strings, train and test share no query string: a result cache of the
  training queries answers none of the test queries by construction.
- Corpus: MS MARCO passages, BEIR's copy (8.8M):
  https://public.ukp.informatik.tu-darmstadt.de/thakur/BEIR/datasets/msmarco.zip

Everything runs in one Strata database (`p`), which plays the cloud: a
device's local search is the same query with `WHERE tier_<method> <= t`,
which scores with the whole database's statistics as a slice would with the
cloud's. A term's residual bound is the top score of the one-word query
over the rows outside the slice, which is exactly the largest
`idf * tf-part` among them. Words are normalised so that each is one
analysed term or a query stopword, and a word's multiplicity in the query
is its weight, as `bm25` counts it. A word that is not a stopword and has
no posting in the slice counts as unknown to the device: the device has no
bound for it and asks the cloud.

Slices are nested, chosen from the training queries' cloud top 10:

- `cov`: query coverage. Greedily complete the training query with the
  fewest documents missing from the slice (each query weighs 1), until the
  largest tier is full.
- `pop`: popular documents. Documents by how many training queries' top
  10 hold them.

Stages, each writing to --work:

    python search.py prep --data ~/data/slices        # queries, corpus JSONL
    python search.py load                              # the database, tiers unset
    python search.py cloud train                       # training queries' top 10
    python search.py select                            # tiers of both methods
    python search.py load --tiers                      # the database again, with tiers
    python search.py cloud test                        # test queries' top 10
    python search.py eval test                         # bounds, local top 10, proofs: new queries
    python search.py eval seen                         # the same for training queries
    python search.py eval2 test                        # by query length, pair bounds, confidence rule
    python search.py eval2 seen
    python search.py bytes                             # slice files' sizes
    python search.py residuals                         # word bounds against their maxima
"""
import argparse
import gzip
import json
import math
import os
import random
import re
import shutil
import sys
import threading
import time
import zipfile
from concurrent.futures import ThreadPoolExecutor

import numpy as np

try:
    import strata
except ImportError:  # prep runs without it
    strata = None

WORD = re.compile(r"[A-Za-z0-9]+")
# Ranked queries drop these (strata/src/text/analyzer.rs QUERY_STOPWORDS);
# the single letters among them only after an apostrophe, and normalised
# queries have none, so they stay.
QUERY_STOPWORDS = set(
    "a about above after again against ain all am an and any are aren as at be because been before being below "
    "between both but by can couldn did didn do does doesn doing don down during each few for from further had "
    "hadn has hasn have haven having he her here hers herself him himself his how i if in into is isn it its "
    "itself just ll ma me mightn more most mustn my myself needn no nor not now of off on once only or other our "
    "ours ourselves out over own re same shan she should shouldn so some such than that the their theirs them "
    "themselves then there these they this those through to too under until up ve very was wasn we were weren "
    "what when where which while who whom why will with won wouldn you your yours yourself yourselves".split()
)
TIERS = [10_000, 30_000, 100_000, 200_000, 400_000]  # documents a slice holds
NOT = len(TIERS)  # the tier of a document in no slice
METHODS = ["cov", "pop"]
K = 10
KS = (1, 3, 5, 10)
SEED = 7


def log(*a):
    print(time.strftime("%H:%M:%S"), *a, flush=True)


def normalise(q):
    if not q.isascii():
        return None
    words = WORD.findall(q.lower())
    if not any(w not in QUERY_STOPWORDS for w in words):
        return None
    return " ".join(words)


def cmd_prep(a):
    os.makedirs(a.work, exist_ok=True)
    seen = {}
    raw = skipped = 0
    with gzip.open(os.path.join(a.data, "orcas-queries.tsv.gz"), "rt", encoding="utf-8", errors="replace") as f:
        for line in f:
            raw += 1
            parts = line.rstrip("\n").split("\t", 1)
            q = normalise(parts[1]) if len(parts) == 2 else None
            if q is None:
                skipped += 1
                continue
            seen[q] = seen.get(q, 0) + 1
    queries = sorted(seen)
    random.Random(SEED).shuffle(queries)
    test, train = queries[: a.test], queries[a.test : a.test + a.train]
    for name, qs in (("test", test), ("train", train)):
        with open(os.path.join(a.work, f"{name}.txt"), "w") as f:
            f.write("\n".join(qs) + "\n")
    stats = {"orcas_queries": raw, "skipped_non_ascii_or_only_stopwords": skipped, "distinct_normalised": len(queries),
             "raw_variants_per_string_max": max(seen.values()), "test": len(test), "train": len(train)}
    log("queries", stats)
    n = 0
    with zipfile.ZipFile(os.path.join(a.data, "msmarco.zip")) as z, z.open("msmarco/corpus.jsonl") as src, \
            open(os.path.join(a.work, "corpus.jsonl"), "w") as out:
        for line in src:
            d = json.loads(line)
            # Row i is passage i: tiers and cloud answers index by the id.
            assert int(d["_id"]) == n, (d["_id"], n)
            body = (d.get("title", "") + " " + d["text"]).strip()
            out.write(json.dumps({"id": d["_id"], "body": body}) + "\n")
            n += 1
            if a.docs and n >= a.docs:
                break
    stats["docs"] = n
    json.dump(stats, open(os.path.join(a.work, "prep.json"), "w"), indent=1)
    log("corpus", n)


SCHEMA = {"table": "p", "key": "id", "fields": [
    {"name": "id", "type": "keyword"},
    {"name": "body", "type": "text"},
    {"name": "tier_cov", "type": "int"},
    {"name": "tier_pop", "type": "int"},
]}


def db_path(a):
    return os.path.join(a.work, "cloud.strata")


def cmd_load(a):
    path = db_path(a)
    for p in (path, path + "-wal"):
        if os.path.exists(p):
            os.remove(p) if os.path.isfile(p) else shutil.rmtree(p)
    tiers = None
    if a.tiers:
        t = np.load(os.path.join(a.work, "tiers.npz"))
        tiers = {m: t[m] for m in METHODS}
    src = os.path.join(a.work, "corpus.jsonl")
    rows = os.path.join(a.work, "rows.jsonl")
    with open(src) as f, open(rows, "w") as out:
        for i, line in enumerate(f):
            d = json.loads(line)
            d["tier_cov"] = int(tiers["cov"][i]) if tiers else NOT
            d["tier_pop"] = int(tiers["pop"][i]) if tiers else NOT
            out.write(json.dumps(d) + "\n")
    t0 = time.time()
    db = strata.open(path, SCHEMA)
    r = db.import_file(rows)
    log("imported", r, f"{time.time() - t0:.0f}s")
    db.optimize()
    log("optimized", f"{time.time() - t0:.0f}s", "rows", len(db))
    db.close()
    os.remove(rows)


def queries(a, which):
    return [q for q in open(os.path.join(a.work, f"{which}.txt")).read().split("\n") if q]


SQL_TOP = "SELECT id, score() FROM p {where} ORDER BY bm25(body, :q, syntax => 'plain') DESC LIMIT {k}"


def run_all(items, fn, threads, label):
    """fn over items on a pool, in order, logging progress."""
    out = [None] * len(items)
    done = [0]
    lock = threading.Lock()
    t0 = time.time()

    def one(i):
        out[i] = fn(items[i])
        with lock:
            done[0] += 1
            if done[0] % 20000 == 0:
                el = time.time() - t0
                log(f"{label}: {done[0]}/{len(items)} {done[0] / el:.0f}/s")

    with ThreadPoolExecutor(threads) as ex:
        list(ex.map(one, range(len(items))))
    log(f"{label}: {len(items)} in {time.time() - t0:.0f}s")
    return out


def top(db, q, where="", k=K, params=None):
    p = {"q": q}
    if params:
        p.update(params)
    r = db.query(SQL_TOP.format(where=where, k=k), p)
    return [(int(row[0]), float(row[1])) for row in r.rows if row[1] > 0]


def cmd_cloud(a):
    db = strata.Database.open(db_path(a), threads=1)
    qs = queries(a, a.which)
    res = run_all(qs, lambda q: top(db, q), a.threads, f"cloud {a.which}")
    ids = np.full((len(qs), K), -1, np.int32)
    sc = np.zeros((len(qs), K), np.float32)
    for i, r in enumerate(res):
        for j, (d, s) in enumerate(r):
            ids[i, j], sc[i, j] = d, s
    np.savez(os.path.join(a.work, f"cloud_{a.which}.npz"), ids=ids, scores=sc)
    log("full top 10:", int((ids[:, -1] >= 0).sum()), "of", len(qs))


def cmd_select(a):
    c = np.load(os.path.join(a.work, "cloud_train.npz"))
    ids = c["ids"][c["ids"][:, -1] >= 0]  # training queries with a full top 10
    n_docs = json.load(open(os.path.join(a.work, "prep.json")))["docs"]
    cap = TIERS[-1]
    tiers = {}
    # pop: documents by the number of training top 10s holding them.
    counts = np.bincount(ids.ravel(), minlength=n_docs)
    order = np.lexsort((np.arange(n_docs), -counts))[:cap]
    tiers["pop"] = tier_of(order, n_docs)
    # cov: greedy completion of the queries with the fewest missing documents.
    nq = len(ids)
    flat = ids.ravel()
    by_doc = np.argsort(flat, kind="stable")
    starts = np.searchsorted(flat[by_doc], np.arange(n_docs + 1))
    missing = np.full(nq, K, np.int32)
    buckets = [[] for _ in range(K + 1)]
    buckets[K] = list(range(nq - 1, -1, -1))
    chosen = np.zeros(n_docs, bool)
    order = []
    covered_at = []
    b = 1
    while len(order) < cap:
        while b <= K and not buckets[b]:
            b += 1
        if b > K:
            break
        q = buckets[b].pop()
        if missing[q] != b:
            continue  # stale entry
        for d in ids[q]:
            if chosen[d]:
                continue
            chosen[d] = True
            order.append(int(d))
            for e in by_doc[starts[d]:starts[d + 1]]:
                q2 = e // K
                missing[q2] -= 1
                m = missing[q2]
                if m > 0:
                    buckets[m].append(q2)
                    b = min(b, m)
        covered_at.append(len(order))
    order = np.array(order[:cap], np.int64)
    tiers["cov"] = tier_of(order, n_docs)
    np.savez(os.path.join(a.work, "tiers.npz"), **tiers)
    # Training coverage: queries whose whole top 10 lies in each tier.
    out = {}
    for m in METHODS:
        t = tiers[m][ids]
        out[m] = {str(n): float((t.max(axis=1) <= i).mean()) for i, n in enumerate(TIERS)}
    json.dump({"train_queries_full_top10": int(nq), "train_coverage": out}, open(os.path.join(a.work, "select.json"), "w"), indent=1)
    log("train coverage", out)


def tier_of(order, n_docs):
    t = np.full(n_docs, NOT, np.int8)
    lo = 0
    for i, n in enumerate(TIERS):
        sel = order[lo:n]
        t[sel] = np.minimum(t[sel], i)
        lo = n
    return t


def cmd_eval(a):
    """`test`: queries never seen in training (new strings). `seen`: a
    sample of the training queries, the case of a query that repeats one
    the slice was chosen from (its cloud answer is from the database before
    the tiers were set: the same rows and statistics, so the same scores)."""
    db = strata.Database.open(db_path(a), threads=1)
    which = "test" if a.set == "test" else "train"
    qs = queries(a, which)
    c = np.load(os.path.join(a.work, f"cloud_{which}.npz"))
    cid, csc = c["ids"], c["scores"]
    keep = np.nonzero(cid[:, -1] >= 0)[0]
    if a.set == "seen":
        keep = np.sort(np.random.default_rng(SEED).choice(keep, 50_000, replace=False))
    tiers = np.load(os.path.join(a.work, "tiers.npz"))
    words = sorted({w for i in keep for w in qs[i].split() if w not in QUERY_STOPWORDS})
    log(f"{a.set} queries with a full top 10:", len(keep), "of", len(qs), "words", len(words))
    bands = list(range(NOT + 1))
    # Each word's top score within each tier band, per method: the residual
    # bound of tier t is the largest over bands > t, and the word is known to
    # a slice of tier t when some band <= t holds it.
    best = {}
    for m in METHODS:
        def word_bands(w, m=m):
            return [max([s for _, s in top(db, w, f"WHERE tier_{m} = :b", 1, {"b": b})], default=0.0) for b in bands]
        best[m] = np.array(run_all(words, word_bands, a.threads, f"bands {m}"), np.float64)
    widx = {w: i for i, w in enumerate(words)}
    np.savez(os.path.join(a.work, f"bands_{a.set}.npz"), words=np.array(words), **best)
    result = {"queries": len(qs), "full_top10": int(len(keep)), "words": len(words), "methods": {}}
    for m in METHODS:
        tm = tiers[m]
        cloud_tier = tm[cid[keep]].max(axis=1)  # the tier holding a query's whole top 10
        rmax = best[m].max()
        step = rmax / 255.0
        per = {}
        for ti, n in enumerate(TIERS):
            resid = best[m][:, ti + 1:].max(axis=1)
            known = best[m][:, : ti + 1].max(axis=1) > 0
            resid8 = np.ceil(resid / step) * step

            def local(i, ti=ti):
                return top(db, qs[i], f"WHERE tier_{m} <= :t", K, {"t": ti})

            loc = run_all(list(keep), local, a.threads, f"local {m} tier {n}")
            st = {k: {"proved": 0, "proved_8bit": 0, "covered": 0, "same": 0, "same_unproved": 0, "unproved": 0,
                      "overlap_unproved": 0.0, "unknown_word": 0, "violations": 0} for k in KS}
            for j, i in enumerate(keep):
                ws = [w for w in qs[i].split() if w not in QUERY_STOPWORDS]
                unknown = any(not known[widx[w]] for w in ws)
                bound = sum(resid[widx[w]] for w in ws)
                bound8 = sum(resid8[widx[w]] for w in ws)
                lids = [d for d, _ in loc[j]]
                lsc = [s for _, s in loc[j]]
                for k in KS:
                    s = st[k]
                    cloud_k = set(cid[i, :k].tolist())
                    covered = tm[cid[i, :k]].max() <= ti
                    same = len(lids) >= k and set(lids[:k]) == cloud_k
                    ok = len(lids) >= k and not unknown and lsc[k - 1] >= bound
                    ok8 = len(lids) >= k and not unknown and lsc[k - 1] >= bound8
                    s["covered"] += covered
                    s["same"] += same
                    s["unknown_word"] += unknown
                    if ok:
                        s["proved"] += 1
                        # Sound only if the proved answer is the cloud's (ties
                        # at the k-th score may swap equal-scored documents).
                        if not same and abs(lsc[k - 1] - float(csc[i, k - 1])) > 1e-4:
                            s["violations"] += 1
                    else:
                        s["unproved"] += 1
                        s["same_unproved"] += same
                        s["overlap_unproved"] += len(set(lids[:k]) & cloud_k) / k
                    s["proved_8bit"] += ok8
            nk = len(keep)
            per[str(n)] = {str(k): {
                "proved": v["proved"] / nk, "proved_8bit": v["proved_8bit"] / nk, "covered": v["covered"] / nk,
                "local_equals_cloud": v["same"] / nk, "unknown_word": v["unknown_word"] / nk,
                "equal_among_unproved": v["same_unproved"] / max(v["unproved"], 1),
                "overlap_among_unproved": v["overlap_unproved"] / max(v["unproved"], 1),
                "violations": v["violations"]} for k, v in st.items()}
            log(m, n, json.dumps(per[str(n)]["10"]))
        result["methods"][m] = per
    json.dump(result, open(os.path.join(a.work, f"eval_{a.set}.json"), "w"), indent=1)


ALPHAS = [1.0, 0.9, 0.8, 0.7, 0.6, 0.5, 0.4, 0.3]


def cmd_eval2(a):
    """Why proofs fail, and two ways past it, for the `cov` slices.

    - By query length: the share of queries whose top k the slice holds
      (`covered`, which an exact bound would prove) against the share the
      per-term bounds prove.
    - Pair bounds: for two words a and b that appear together in some
      training query, the slice can ship G(a, b), the most any cloud-only
      row scores on the two together: the top score over the rows holding
      both, or one of them alone. A query's bound is then the sum over a
      greedy pairing of its words, each group's G or a lone word's R, which
      is never more than the per-term sum and still a valid bound.
    - A confidence rule instead of a proof: answer locally when the k-th
      local score reaches a share alpha of the bound, and count how often
      that local answer is the cloud's."""
    db = strata.Database.open(db_path(a), threads=1)
    m = "cov"
    which = "test" if a.set == "test" else "train"
    qs = queries(a, which)
    c = np.load(os.path.join(a.work, f"cloud_{which}.npz"))
    cid, csc = c["ids"], c["scores"]
    keep = np.nonzero(cid[:, -1] >= 0)[0]
    if a.set == "seen":
        keep = np.sort(np.random.default_rng(SEED).choice(keep, 50_000, replace=False))
    tiers = np.load(os.path.join(a.work, "tiers.npz"))[m]
    b = np.load(os.path.join(a.work, f"bands_{a.set}.npz"))
    words = list(b["words"])
    widx = {w: i for i, w in enumerate(words)}
    best = b[m]
    # Pairs a slice could ship: those of the training queries.
    train_pairs = set()
    for q in queries(a, "train"):
        ws = sorted({w for w in q.split() if w not in QUERY_STOPWORDS})
        for i in range(len(ws)):
            for j in range(i + 1, len(ws)):
                train_pairs.add((ws[i], ws[j]))
    want = set()
    for i in keep:
        ws = sorted({w for w in qs[i].split() if w not in QUERY_STOPWORDS})
        for x in range(len(ws)):
            for y in range(x + 1, len(ws)):
                if (ws[x], ws[y]) in train_pairs:
                    want.add((ws[x], ws[y]))
    pairs = sorted(want)
    log(f"{a.set}: {len(train_pairs)} training pairs, {len(pairs)} of them in the evaluated queries")
    del train_pairs
    bands = list(range(NOT + 1))
    sql = ("SELECT score() FROM p WHERE MATCH(body, :m) AND tier_cov = :b "
           "ORDER BY bm25(body, :q, syntax => 'plain') DESC LIMIT 1")

    def pair_bands(pr):
        q = f"{pr[0]} {pr[1]}"
        mq = f"{pr[0]} AND {pr[1]}"
        return [max([r[0] for r in db.query(sql, {"m": mq, "q": q, "b": x}).rows], default=0.0) for x in bands]

    pbest = np.array(run_all(pairs, pair_bands, a.threads, "pair bands"), np.float64) if pairs else np.zeros((0, len(bands)))
    pidx = {pr: i for i, pr in enumerate(pairs)}
    out = {"set": a.set, "queries": int(len(keep)), "pairs": len(pairs), "tiers": {}}
    for ti, n in enumerate(TIERS):
        resid = best[:, ti + 1:].max(axis=1)
        known = best[:, : ti + 1].max(axis=1) > 0
        presid = pbest[:, ti + 1:].max(axis=1) if len(pairs) else np.zeros(0)
        loc = run_all(list(keep), lambda i: top(db, qs[i], "WHERE tier_cov <= :t", K, {"t": ti}), a.threads, f"local tier {n}")
        rows = []
        for j, i in enumerate(keep):
            ws = [w for w in qs[i].split() if w not in QUERY_STOPWORDS]
            unk = any(not known[widx[w]] for w in ws)
            single = sum(resid[widx[w]] for w in ws)
            # Greedy pairing of distinct words (a repeated word stays alone).
            cnt = {}
            for w in ws:
                cnt[w] = cnt.get(w, 0) + 1
            uniq = sorted(w for w, k in cnt.items() if k == 1)
            gains = []
            for x in range(len(uniq)):
                for y in range(x + 1, len(uniq)):
                    pi = pidx.get((uniq[x], uniq[y]))
                    if pi is None:
                        continue
                    ra, rb = resid[widx[uniq[x]]], resid[widx[uniq[y]]]
                    g = max(presid[pi], ra, rb)
                    gains.append((ra + rb - g, uniq[x], uniq[y]))
            gains.sort(reverse=True)
            used = set()
            paired = single
            for gain, x, y in gains:
                if gain > 0 and x not in used and y not in used:
                    used.update((x, y))
                    paired -= gain
            lsc = [s_ for _, s_ in loc[j]]
            lids = [d for d, _ in loc[j]]
            rows.append((len(set(ws)), unk, single, paired, lsc, lids, i))
        per = {}
        for k in KS:
            by_len = {}
            curve = {str(al): [0, 0] for al in ALPHAS}
            tot = {"covered": 0, "proved_single": 0, "proved_pairs": 0, "violations_pairs": 0}
            for nw, unk, single, paired, lsc, lids, i in rows:
                L = str(min(nw, 4))
                d = by_len.setdefault(L, {"queries": 0, "covered": 0, "proved_single": 0, "proved_pairs": 0})
                cloud_k = set(cid[i, :k].tolist())
                full = len(lsc) >= k
                same = full and set(lids[:k]) == cloud_k
                cov = bool(tiers[cid[i, :k]].max() <= ti)
                ps = full and not unk and lsc[k - 1] >= single
                pp = full and not unk and lsc[k - 1] >= paired
                d["queries"] += 1
                d["covered"] += cov
                d["proved_single"] += ps
                d["proved_pairs"] += pp
                tot["covered"] += cov
                tot["proved_single"] += ps
                tot["proved_pairs"] += pp
                if pp and not same and abs(lsc[k - 1] - float(csc[i, k - 1])) > 1e-4:
                    tot["violations_pairs"] += 1
                for al in ALPHAS:
                    if full and not unk and lsc[k - 1] >= al * paired:
                        curve[str(al)][0] += 1
                        curve[str(al)][1] += same
            nq = len(rows)
            per[str(k)] = {
                "covered": tot["covered"] / nq, "proved_single": tot["proved_single"] / nq,
                "proved_pairs": tot["proved_pairs"] / nq, "violations_pairs": tot["violations_pairs"],
                "by_words": {L: {"share": v["queries"] / nq, "covered": v["covered"] / v["queries"],
                                 "proved_single": v["proved_single"] / v["queries"], "proved_pairs": v["proved_pairs"] / v["queries"]}
                             for L, v in sorted(by_len.items())},
                "confidence": {al: {"answered_locally": v[0] / nq, "equal_to_cloud": v[1] / max(v[0], 1)} for al, v in curve.items()},
            }
        out["tiers"][str(n)] = per
        log(a.set, n, json.dumps({k: {x: per[k][x] for x in ("covered", "proved_single", "proved_pairs")} for k in ("1", "10")}))
    json.dump(out, open(os.path.join(a.work, f"eval2_{a.set}.json"), "w"), indent=1)


def cmd_residuals(a):
    """How close a word's residual bound is to its top score anywhere: the
    share of the words a slice knows whose best passage stays in the cloud."""
    b = np.load(os.path.join(a.work, "bands_test.npz"))
    out = {}
    for m in METHODS:
        x = b[m]
        mx = x.max(axis=1)
        out[m] = {}
        for ti, n in enumerate(TIERS):
            known = x[:, : ti + 1].max(axis=1) > 0
            ratio = x[known, ti + 1:].max(axis=1) / mx[known]
            out[m][str(n)] = {"words_known": int(known.sum()), "median_bound_over_max": float(np.median(ratio)),
                              "bound_at_least_0.9_of_max": float((ratio >= 0.9).mean()), "bound_equals_max": float((ratio >= 1 - 1e-6).mean())}
        log(m, json.dumps(out[m]))
    json.dump(out, open(os.path.join(a.work, "residuals.json"), "w"), indent=1)


def cmd_bytes(a):
    tiers = np.load(os.path.join(a.work, "tiers.npz"))
    wanted = np.minimum(tiers["cov"], tiers["pop"]) < NOT
    docs = {}
    for i, line in enumerate(open(os.path.join(a.work, "corpus.jsonl"))):
        if wanted[i]:
            docs[i] = json.loads(line)
    schema = {"table": "p", "key": "id", "fields": [{"name": "id", "type": "keyword"}, {"name": "body", "type": "text"}]}
    out = {}
    tmp = os.path.join(a.work, "bytes")
    os.makedirs(tmp, exist_ok=True)
    for m in METHODS:
        tm = tiers[m]
        out[m] = {}
        for ti, n in enumerate(TIERS):
            path = os.path.join(tmp, f"{m}-{n}.strata")
            for p in (path, path + "-wal"):
                if os.path.exists(p):
                    os.remove(p)
            rows = os.path.join(tmp, "rows.jsonl")
            sel = np.nonzero(tm <= ti)[0]
            with open(rows, "w") as f:
                for i in sel:
                    f.write(json.dumps(docs[i]) + "\n")
            db = strata.open(path, schema)
            db.import_file(rows)
            db.optimize()
            db.vacuum()
            db.close()
            size = os.path.getsize(path) + (os.path.getsize(path + "-wal") if os.path.exists(path + "-wal") else 0)
            text = sum(len(docs[i]["body"].encode()) for i in sel)
            out[m][str(n)] = {"docs": int(len(sel)), "file_bytes": size, "bytes_per_doc": size / len(sel), "text_bytes": text}
            log(m, n, out[m][str(n)])
            os.remove(rows)
    json.dump(out, open(os.path.join(a.work, "bytes.json"), "w"), indent=1)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--work", default=os.path.expanduser("~/data/slices/work"))
    ap.add_argument("--threads", type=int, default=4)
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("prep")
    p.add_argument("--data", default=os.path.expanduser("~/data/slices"))
    p.add_argument("--docs", type=int, default=0, help="first N passages (0: all)")
    p.add_argument("--train", type=int, default=1_000_000)
    p.add_argument("--test", type=int, default=50_000)
    p = sub.add_parser("load")
    p.add_argument("--tiers", action="store_true")
    p = sub.add_parser("cloud")
    p.add_argument("which", choices=["train", "test"])
    sub.add_parser("select")
    p = sub.add_parser("eval")
    p.add_argument("set", choices=["test", "seen"])
    sub.add_parser("bytes")
    sub.add_parser("residuals")
    p = sub.add_parser("eval2")
    p.add_argument("set", choices=["test", "seen"])
    a = ap.parse_args()
    {"prep": cmd_prep, "load": cmd_load, "cloud": cmd_cloud, "select": cmd_select, "eval": cmd_eval, "eval2": cmd_eval2, "bytes": cmd_bytes, "residuals": cmd_residuals}[a.cmd](a)


if __name__ == "__main__":
    main()
