"""Next listens on Yambda-50M: Strata's recommendation sources against the
baselines of the dataset's paper, on the paper's global time split.

Yambda (Ploshkin et al., 2025, arXiv 2505.22238; data
https://huggingface.co/datasets/yandex/yambda, Apache 2.0, "published
exclusively for scientific and research purposes") is a year of Yandex
Music: Yambda-50M is 10,000 users, 934,057 tracks and 46.5M listens. The
protocol is the paper's (its section 4.1 and its code's constants):

- Listen+ events only: listens of at least 50% of the track.
- Train on everything before the test day, less a 30-minute gap; test on
  the last day (from 25,913,600 s), for the users seen in training. Every
  Listen+ of a test user that day is a target, repeats of tracks they
  already know included (68% of them are), so nothing is left out of the
  ranking.
- Hyperparameters are chosen on the day before, with training ending 30
  minutes before it, as the paper's code does.
- Recall@k is hits in the top k over min(targets, k); NDCG@k divides by the
  ideal ranking of min(distinct targets, k) hits; HR@k is the share of users
  with a hit; coverage@k is the share of the training tracks in some user's
  top k. `recall_published@k` divides by all the targets, as the paper's
  Table 6 did (its code's comment on Recall says so).

Baselines, written here from the paper's descriptions (not its code):
MostPop; DecayPop (listens weighted by 0.9 per `hours`, back from the end of
training); ItemKNN (a user is the cosine of their decayed listens' user
columns, and a track is its own user column); and `history`, a user's own
tracks by how often they played them in the last `hours`.

Strata's sources run as queries on a database of the training listens
(`load`):

- `encode`: `similarity(emb, encode('yambda50m', last 200 listens, now))`,
  a sequence model trained by `strata.train.sequence` (`train`); its learned
  item vectors are the `emb` of `items`, HNSW-indexed.
- `history`: the user's own tracks, `GROUP BY` over their events.
- `next_items`: counted transitions (`strata.next_items`, order 1).
- `ppr`: personalized PageRank over the edge table of what came next,
  seeded by the last 5 listens.
- `pop`: `ORDER BY pop`, DecayPop's score kept as a column.
- `fused`: one query, `rrf()` of `encode`, `ppr`, and the user's history
  given as `ranked(:keys)`.

Each source answers 100 tracks; one that answers fewer is filled with
DecayPop's. Stages, each resumable from `--work`:

    python yambda.py prep --data ~/data/yambda            # split and baselines
    python yambda.py train --minutes 90                   # the sequence model
    python yambda.py load                                 # the Strata database
    python yambda.py eval                                 # Strata's sources
"""
import argparse
import datetime
import json
import math
import os
import shutil
import sys
import time

import numpy as np
import polars as pl

HERE = os.path.dirname(os.path.abspath(__file__))
LAST = 26_000_000
DAY = 86_400
GAP = 1_800
TEST = LAST - DAY
LISTEN_PLUS = 50
K = 100
KS = (10, 100)
MODEL = "yambda50m"
# The paper's code's grids for Yambda-50M listens.
DECAY_HOURS = [0.5, 1, 2, 3, 6, 12, 24]
KNN_HOURS = [0, 0.001, 0.002, 0.004, 0.008, 0.016, 0.032, 0.064, 0.128, 0.256, 0.5, 1.0, 2]
HISTORY_HOURS = [1, 6, 24, 72, 168, 720, None]


def log(*a):
    print(time.strftime("%H:%M:%S"), *a, flush=True)


# -- data ----------------------------------------------------------------


def listens(data):
    return (
        pl.scan_parquet(os.path.join(data, "flat", "50m", "listens.parquet"))
        .filter(pl.col("played_ratio_pct") >= LISTEN_PLUS)
        .select(pl.col("uid"), pl.col("item_id").alias("item"), pl.col("timestamp").alias("ts"))
    )


def split(lf, val):
    """Training events (sorted by user, then time), each target user's
    targets, and the time the targets start (`now`)."""
    end = TEST - GAP - (DAY + GAP if val else 0)
    lo, hi = (TEST - DAY - GAP, TEST - GAP) if val else (TEST, None)
    train = lf.filter(pl.col("ts") < end).collect().sort(["uid", "ts"], maintain_order=True)
    t = lf.filter(pl.col("ts") >= lo)
    if hi is not None:
        t = t.filter(pl.col("ts") < hi)
    users = train["uid"].unique()
    t = t.filter(pl.col("uid").is_in(users.implode())).collect().sort(["uid", "ts"], maintain_order=True)
    targets = {u: np.asarray(items, dtype=np.int64) for u, items in t.group_by("uid", maintain_order=True).agg("item").iter_rows()}
    return train, targets, lo


# -- metrics -------------------------------------------------------------


class Tally:
    def __init__(self, n_items):
        self.n_items, self.n, self.sums, self.cover, self.ms = n_items, 0, {}, {k: set() for k in KS}, []

    def add(self, top, targets, ms=None):
        top = list(top)[:K]
        want = set(int(x) for x in targets)
        hits = [1.0 if int(x) in want else 0.0 for x in top] + [0.0] * (K - len(top))
        n, nu = len(targets), len(want)
        for k in KS:
            h = sum(hits[:k])
            dcg = sum(hits[i] / math.log2(i + 2) for i in range(k))
            idcg = sum(1 / math.log2(i + 2) for i in range(min(nu, k)))
            for name, v in ((f"recall@{k}", h / min(n, k)), (f"recall_published@{k}", h / n), (f"hr@{k}", float(h > 0)), (f"ndcg@{k}", dcg / idcg)):
                self.sums[name] = self.sums.get(name, 0.0) + v
            self.cover[k].update(int(x) for x in top[:k])
        self.n += 1
        if ms is not None:
            self.ms.append(ms)

    def result(self):
        out = {k: round(v / self.n, 4) for k, v in sorted(self.sums.items())}
        for k in KS:
            out[f"coverage@{k}"] = round(len(self.cover[k]) / self.n_items, 4)
        out["users"] = self.n
        if self.ms:
            ms = sorted(self.ms)
            out["ms_p50"] = round(ms[len(ms) // 2], 2)
            out["ms_p99"] = round(ms[min(len(ms) - 1, int(len(ms) * 0.99))], 2)
        return out


# -- baselines -----------------------------------------------------------


def tau_of(hours):
    """Weight per second back, so that `hours` back weighs 0.9."""
    return 0.9 ** (1 / (3600 * hours))


def decay_pop(train, hours):
    end = train["ts"].max()
    w = train.select(pl.col("item"), (pl.lit(tau_of(hours)) ** (end - pl.col("ts").cast(pl.Float64))).alias("w"))
    return w.group_by("item").agg(pl.col("w").sum()).sort(["w", "item"], descending=[True, False])


def most_pop(train):
    return train.group_by("item").agg(pl.len().alias("w")).sort(["w", "item"], descending=[True, False])


def history_top(train, now, hours, users):
    """Each user's own tracks by how often they played them since `hours`
    before `now`, then by when they last did (all their history for None)."""
    t = train.filter(pl.col("uid").is_in(pl.Series(list(users)).implode()))
    if hours is not None:
        t = t.filter(pl.col("ts") >= now - int(hours * 3600))
    g = t.group_by(["uid", "item"]).agg(pl.len().alias("n"), pl.col("ts").max().alias("last"))
    g = g.sort(["uid", "n", "last", "item"], descending=[False, True, True, False])
    return {u: items[:K] for u, items in g.group_by("uid", maintain_order=True).agg(pl.col("item")).iter_rows()}


def fill(top, pop):
    top = list(top)[:K]
    have = set(top)
    for x in pop:
        if len(top) >= K:
            break
        if x not in have:
            top.append(x)
            have.add(x)
    return top


def item_knn(train, users, hours_grid, targets, n_items):
    """ItemKNN as the paper describes it, scored for `users`, for each
    `hours`; returns {hours: Tally} (and the tops of the last)."""
    import scipy.sparse as sp

    items = np.sort(train["item"].unique().to_numpy())
    uids = np.sort(train["uid"].unique().to_numpy())
    ui = np.searchsorted(uids, train["uid"].to_numpy())
    ii = np.searchsorted(items, train["item"].to_numpy())
    U = sp.csr_matrix((np.ones(len(ui), dtype=np.float32), (ui, ii)), shape=(len(uids), len(items)))
    U.sum_duplicates()
    norms = np.sqrt(np.asarray(U.multiply(U).sum(axis=0)).ravel()) + 1e-12
    Uc = (U @ sp.diags(1.0 / norms).astype(np.float32)).tocsc().astype(np.float32)  # each track's user column, unit length
    last = train.group_by("uid").agg(pl.col("ts").max().alias("m"))
    end_of = dict(zip(last["uid"].to_list(), last["m"].to_list()))
    delta = np.array([end_of[u] for u in train["uid"].to_list()], dtype=np.float64) - train["ts"].to_numpy().astype(np.float64)
    rows = np.searchsorted(uids, np.array(sorted(users)))
    out = {}
    for h in hours_grid:
        tau = 0.0 if h == 0 else tau_of(h)
        w = np.power(tau, delta)
        keep = w > 1e-9
        W = sp.csr_matrix((w[keep].astype(np.float32), (ui[keep], ii[keep])), shape=U.shape)
        W.sum_duplicates()
        V = (W[rows] @ U.T).toarray()
        V /= np.linalg.norm(V, axis=1, keepdims=True) + 1e-12
        tally = Tally(n_items)
        for at in range(0, len(rows), 64):
            S = (Uc.T @ V[at : at + 64].T).T  # users x tracks
            idx = np.argpartition(-S, K, axis=1)[:, :K]
            for r in range(S.shape[0]):
                o = idx[r][np.argsort(-S[r, idx[r]], kind="stable")]
                u = int(uids[rows[at + r]])
                tally.add(items[o], targets[u])
        out[h] = tally
        log(f"item_knn hours {h}: {tally.result()}")
    return out


def baselines(train, targets, now, n_items, grids):
    users = sorted(targets)
    res, best = {}, {}
    t = Tally(n_items)
    mp = most_pop(train)["item"].to_list()[:K]
    for u in users:
        t.add(mp, targets[u])
    res["most_pop"] = t.result()
    pops = {}
    for h in grids["decay"]:
        pops[h] = decay_pop(train, h)["item"].to_list()[:K]
        t = Tally(n_items)
        for u in users:
            t.add(pops[h], targets[u])
        res[f"decay_pop@{h}"] = t.result()
    best["decay_pop"] = max(grids["decay"], key=lambda h: res[f"decay_pop@{h}"]["recall@100"])
    pop = pops[best["decay_pop"]]
    for h in grids["history"]:
        tops = history_top(train, now, h, users)
        t = Tally(n_items)
        for u in users:
            t.add(fill(tops.get(u, []), pop), targets[u])
        res[f"history@{h}"] = t.result()
    best["history"] = max(grids["history"], key=lambda h: res[f"history@{h}"]["recall@100"])
    knn = item_knn(train, users, grids["knn"], targets, n_items)
    for h, t in knn.items():
        res[f"item_knn@{h}"] = t.result()
    best["item_knn"] = max(grids["knn"], key=lambda h: res[f"item_knn@{h}"]["recall@100"])
    return res, best


def cmd_prep(a):
    lf = listens(a.data)
    os.makedirs(a.work, exist_ok=True)
    grids = {"decay": DECAY_HOURS, "history": HISTORY_HOURS, "knn": KNN_HOURS if not a.quick else [0, 0.004, 0.064, 1.0]}
    # Choose on the day before the test day.
    train, targets, now = split(lf, val=True)
    n_items = train["item"].n_unique()
    log(f"validation: {len(train)} training listens, {len(targets)} users, {n_items} tracks")
    val, best = baselines(train, targets, now, n_items, grids)
    log(f"chosen on validation: {best}")
    # Then the test day, with the chosen hours.
    train, targets, now = split(lf, val=False)
    n_items = train["item"].n_unique()
    log(f"test: {len(train)} training listens, {len(targets)} users, {n_items} tracks, {sum(len(v) for v in targets.values())} targets")
    test, _ = baselines(train, targets, now, n_items, {"decay": [best["decay_pop"]], "history": [best["history"]], "knn": [best["item_knn"]]})
    train.write_parquet(os.path.join(a.work, "train.parquet"))
    pl.DataFrame({"uid": list(targets), "items": [v.tolist() for v in targets.values()]}).write_parquet(os.path.join(a.work, "targets.parquet"))
    out = {
        "split": {"train_end": TEST - GAP, "test_start": TEST, "train_listens": len(train), "test_users": len(targets), "targets": int(sum(len(v) for v in targets.values())), "tracks": n_items},
        "chosen": best,
        "validation": val,
        "baselines": {k.split("@")[0]: v for k, v in test.items()},
    }
    json.dump(out, open(os.path.join(a.work, "baselines.json"), "w"), indent=1)
    for k, v in out["baselines"].items():
        log(f"{k:>10} {v}")


# -- Strata --------------------------------------------------------------


def read_split(work):
    train = pl.read_parquet(os.path.join(work, "train.parquet"))
    t = pl.read_parquet(os.path.join(work, "targets.parquet"))
    targets = {u: np.asarray(items, dtype=np.int64) for u, items in t.iter_rows()}
    return train, targets


def svd_vectors(train, items, width, seed):
    """Unit item vectors from a truncated SVD of the user-track matrix of
    log(1 + plays), the items' right singular vectors scaled by the square
    root of their singular values: where training starts, so that the
    model learns to map histories into a space where similar tracks are
    already close (random vectors, learned from scratch, did not train:
    one epoch left the holdout's recall@10 at 0)."""
    import scipy.sparse as sp
    from scipy.sparse.linalg import svds

    t = time.time()
    c = train.group_by(["uid", "item"]).agg(pl.len().alias("n"))
    users = np.sort(c["uid"].unique().to_numpy())
    rows = np.searchsorted(users, c["uid"].to_numpy())
    cols = np.searchsorted(items, c["item"].to_numpy())
    m = sp.csr_matrix((np.log1p(c["n"].to_numpy().astype(np.float32)), (rows, cols)), shape=(len(users), len(items)))
    _, sv, vt = svds(m, k=width, random_state=seed)
    v = (vt.T * np.sqrt(sv)).astype(np.float32)
    v /= np.maximum(np.linalg.norm(v, axis=1, keepdims=True), 1e-12)
    log(f"svd of {m.shape[0]} x {m.shape[1]} ({m.nnz} plays) in {time.time() - t:.0f} s")
    return v


def cmd_train(a):
    import torch

    import strata.train

    train, _ = read_split(a.work)
    torch.set_num_threads(os.cpu_count())
    items = np.sort(train["item"].unique().to_numpy())
    if a.init == "svd":
        start = svd_vectors(train, items, a.width, a.seed)
    else:
        g = torch.Generator().manual_seed(a.seed)
        start = torch.nn.functional.normalize(torch.randn(len(items), a.width, generator=g), dim=-1).numpy()
    vectors = {str(int(k)): start[i] for i, k in enumerate(items)}
    # Each user's newest `--max-events` listens, oldest first, with their times.
    t = train.group_by("uid", maintain_order=True).agg(pl.col("item").tail(a.max_events), pl.col("ts").tail(a.max_events))
    seqs = {u: [(str(i), None, float(s)) for i, s in zip(its, tss)] for u, its, tss in t.iter_rows()}
    log(f"training on {sum(len(s) for s in seqs.values())} listens of {len(seqs)} users, {len(items)} tracks")
    models = os.path.join(a.work, "models")
    r = strata.train.sequence(
        None, MODEL, vectors=vectors, sequences=seqs, models=models, width=a.width, layers=2, heads=1, max_len=200,
        learn_items=True, negatives="in_batch", temperature=0.05, epochs=a.epochs, batch=a.batch, lr=1e-3, dropout=0.2,
        holdout=True, patience=3, max_seconds=a.minutes * 60, seed=a.seed, log=lambda row: log(json.dumps(row)),
        logq=a.logq, uniform_negatives=a.uniform_negatives,
    )
    about = {"width": a.width, "batch": a.batch, "max_events": a.max_events, "minutes": a.minutes, "init": a.init, "logq": a.logq, "uniform_negatives": a.uniform_negatives}
    json.dump({"history": r["history"], **about}, open(os.path.join(a.work, "train.json"), "w"), indent=1)


EPOCH = datetime.datetime(1970, 1, 1, tzinfo=datetime.timezone.utc)


def cmd_load(a):
    import strata
    from strata import _sequence as S, ngrams

    train, _ = read_split(a.work)
    baseline = json.load(open(os.path.join(a.work, "baselines.json")))
    models = os.path.join(a.work, "models")
    vec = S.read_items(os.path.join(models, MODEL))
    keys = list(vec)
    dim = len(vec[keys[0]])
    pop = decay_pop(train, baseline["chosen"]["decay_pop"])
    pop_of = dict(zip(pop["item"].to_list(), pop["w"].to_list()))
    path = os.path.join(a.work, "db")
    shutil.rmtree(path, ignore_errors=True)
    t0 = time.time()
    db = strata.Database.create(path, {"table": "items", "key": "id", "fields": [{"name": "id", "type": "keyword"}, {"name": "emb", "type": "vector", "dim": dim}, {"name": "pop", "type": "float"}]}, models=models, directory=True)
    arr = np.asarray([vec[k] for k in keys], dtype=np.float32)
    del vec
    items = pl.DataFrame({"id": keys, "emb": pl.Series(arr, dtype=pl.Array(pl.Float32, dim)), "pop": [float(pop_of.get(int(k), 0.0)) for k in keys]})
    log(db.import_arrow(items, table="items"))
    log(f"items in {time.time() - t0:.0f} s")
    db.sql("CREATE TABLE events (id INT PRIMARY KEY, usr KEYWORD NOT NULL, item KEYWORD NOT NULL, ts TIMESTAMP NOT NULL) OPTIONS(cluster_by = 'usr')")
    ev = train.with_row_index("id").select(
        pl.col("id").cast(pl.Int64), pl.col("uid").cast(pl.Utf8).alias("usr"), pl.col("item").cast(pl.Utf8), (pl.col("ts").cast(pl.Int64) * 1_000_000).cast(pl.Datetime("us", "UTC")).alias("ts")
    )
    t0 = time.time()
    log(db.import_arrow(ev, table="events"))
    log(f"{len(ev)} events in {time.time() - t0:.0f} s")
    # Transitions: each listen after the one before it (order 1), counted.
    nxt = train.with_columns(pl.col("item").shift(1).over("uid").alias("prev")).drop_nulls("prev")
    counts = nxt.group_by(["prev", "item"]).agg(pl.len().alias("n"))
    db.sql(ngrams.create_table("next"))
    rows = counts.select(
        (pl.col("prev").cast(pl.Utf8) + ngrams.NEXT + pl.col("item").cast(pl.Utf8)).alias("id"), pl.col("prev").cast(pl.Utf8).alias("ctx"), pl.col("item").cast(pl.Utf8).alias("nxt"), pl.col("n").cast(pl.Float64)
    )
    t0 = time.time()
    log(db.import_arrow(rows, table="next"))
    log(f"{len(rows)} transitions in {time.time() - t0:.0f} s")
    db.sql("CREATE TABLE after (id KEYWORD PRIMARY KEY, src KEYWORD NOT NULL REFERENCES items, dst KEYWORD NOT NULL REFERENCES items, w FLOAT DEFAULT 0) OPTIONS(edges = 'src, dst, w')")
    edges = counts.filter(pl.col("prev") != pl.col("item")).select(
        (pl.col("prev").cast(pl.Utf8) + ">" + pl.col("item").cast(pl.Utf8)).alias("id"), pl.col("prev").cast(pl.Utf8).alias("src"), pl.col("item").cast(pl.Utf8).alias("dst"), pl.col("n").cast(pl.Float64).alias("w")
    )
    t0 = time.time()
    log(db.import_arrow(edges, table="after"))
    log(f"{len(edges)} edges in {time.time() - t0:.0f} s")
    t0 = time.time()
    db.optimize()
    log(f"optimized in {time.time() - t0:.0f} s")
    db.close()


def cmd_eval(a):
    import strata

    train, targets = read_split(a.work)
    baseline = json.load(open(os.path.join(a.work, "baselines.json")))
    chosen = baseline["chosen"]
    n_items = train["item"].n_unique()
    pop = [int(x) for x in decay_pop(train, chosen["decay_pop"])["item"].to_list()[: 2 * K]]
    db = strata.Database.open(os.path.join(a.work, "db"), models=os.path.join(a.work, "models"))
    users = sorted(targets)
    if a.users:
        users = users[: a.users]
    now_us = TEST * 1_000_000
    h = chosen["history"]
    # events.ts is a TIMESTAMP, which takes its parameters as text.
    since = (EPOCH + datetime.timedelta(seconds=TEST - int(h * 3600) if h is not None else 0)).strftime("%Y-%m-%dT%H:%M:%SZ")
    hist = f"SELECT item, COUNT(*) AS n, MAX(ts) AS last FROM events WHERE usr = :u AND ts >= :since GROUP BY item ORDER BY n DESC, last DESC, item LIMIT {K}"
    enc = f"encode('{MODEL}', (SELECT item, ts FROM events WHERE usr = :u ORDER BY ts DESC LIMIT 200), now => :t)"
    last5 = "(SELECT item FROM events WHERE usr = :u ORDER BY ts DESC LIMIT 5)"
    walk = f"ppr(after, {last5}, dir => 'out', eps => {a.eps})"
    sql = {
        "encode": f"SELECT id FROM items ORDER BY similarity(emb, {enc}) DESC LIMIT {K}",
        "ppr": f"SELECT id FROM items ORDER BY {walk} DESC LIMIT {K}",
        "pop": f"SELECT id FROM items ORDER BY pop DESC, id LIMIT {K}",
        "fused": f"SELECT id FROM items ORDER BY rrf(ranked(:hist), similarity(emb, {enc}), {walk}) DESC LIMIT {K}",
    }
    tally = {s: Tally(n_items) for s in ["history", "encode", "next_items", "ppr", "pop", "fused"]}
    for u in users[:20]:
        db.query(sql["encode"], {"u": str(u), "t": now_us})
    t_all = time.time()
    for n, u in enumerate(users):
        p = {"u": str(u)}
        t = time.perf_counter()
        hk = [r[0] for r in db.query(hist, {**p, "since": since}).rows]
        tally["history"].add(fill([int(x) for x in hk], pop), targets[u], (time.perf_counter() - t) * 1e3)
        for s in ("encode", "ppr", "pop"):
            t = time.perf_counter()
            top = [int(r[0]) for r in db.query(sql[s], {**p, "t": now_us}).rows]
            tally[s].add(fill(top, pop), targets[u], (time.perf_counter() - t) * 1e3)
        t = time.perf_counter()
        last = db.query("SELECT item FROM events WHERE usr = :u ORDER BY ts DESC LIMIT 1", p).rows
        nx = [int(x) for x, _ in strata.next_items(db, [last[0][0]], K, table="next", order=1)] if last else []
        tally["next_items"].add(fill(nx, pop), targets[u], (time.perf_counter() - t) * 1e3)
        t = time.perf_counter()
        top = [int(r[0]) for r in db.query(sql["fused"], {**p, "t": now_us, "hist": hk}).rows]
        tally["fused"].add(fill(top, pop), targets[u], (time.perf_counter() - t) * 1e3)
        if n % 500 == 0:
            log(f"{n} users, {time.time() - t_all:.0f} s")
    got = {s: t.result() for s, t in tally.items()}
    for s, r in got.items():
        log(f"{s:>10} {r}")
    json.dump({"eps": a.eps, "sources": got}, open(os.path.join(a.work, "strata.json"), "w"), indent=1)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("stage", choices=["prep", "train", "load", "eval"])
    ap.add_argument("--data", default=os.path.expanduser("~/data/yambda"))
    ap.add_argument("--work", default=os.path.expanduser("~/data/yambda/work"))
    ap.add_argument("--quick", action="store_true", help="prep: a short ItemKNN grid")
    ap.add_argument("--minutes", type=float, default=90)
    ap.add_argument("--epochs", type=int, default=50)
    ap.add_argument("--batch", type=int, default=64)
    ap.add_argument("--width", type=int, default=64)
    ap.add_argument("--max-events", type=int, default=1000)
    ap.add_argument("--init", choices=["svd", "random"], default="svd", help="train: where the item vectors start")
    ap.add_argument("--logq", action="store_true", help="train: take from each in-batch negative's score the log of the chance the batch holds it")
    ap.add_argument("--uniform-negatives", type=int, default=0, help="train: this many tracks drawn evenly, added to each batch's negatives")
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--eps", type=float, default=1e-6)
    ap.add_argument("--users", type=int, default=None)
    a = ap.parse_args()
    {"prep": cmd_prep, "train": cmd_train, "load": cmd_load, "eval": cmd_eval}[a.stage](a)


if __name__ == "__main__":
    sys.exit(main())
