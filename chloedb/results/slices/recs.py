"""On-device slices, recommendation side: how close recommendations made on
the device, from the user's own history and a few shared "meaning packs" of
the catalogue, come to the same recommendations made in the cloud over the
whole catalogue. Yambda-50M, on the test day of `../recsys/yambda.py`'s
protocol (its split, metrics and baselines are imported from there).

The design is the "Strata on the device" report (Claude Doc,
https://claude.ai/artifact/14s2MaZu2wua3Q3NNvbL2E):

- Every track gets a vector: a truncated SVD of the user-track matrix
  (`yambda.svd_vectors`, 64 dimensions), and a two-level semantic ID:
  residual k-means, 256 centroids a level, so a track's pack is its
  `c0.c1` prefix (65,536 possible packs).
- A user's taste is the decayed sum of their history's vectors. The cloud
  ranks every track by its cosine with it (`vec`). The device ranks only
  the tracks it holds: the user's own history, a shared base pack of the
  most popular tracks, and the packs whose centroids are nearest the
  taste, up to a budget of tracks.
- `history` (a user's own tracks by plays in the last 720 hours, the
  baseline's best) needs nothing from the cloud; `rrf` fuses it with `vec`.

Stages, each writing to --work:

    python recs.py prep --data ~/data/yambda   # split, vectors, codes
    python recs.py eval                        # cloud against device
    python recs.py churn                       # daily changes to a base pack
    python recs.py bytes                       # a pack file's size per track
"""
import argparse
import json
import os
import sys
import time

import numpy as np
import polars as pl
import scipy.sparse as sp

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "recsys"))
import yambda  # noqa: E402

from yambda import DAY, K, Tally, fill, history_top, listens, log, split, svd_vectors, decay_pop, tau_of  # noqa: E402

WIDTH = 64
LEVEL_K = 256
BUDGETS = [1_000, 5_000, 20_000, 50_000]  # pack tracks a device holds
BASE = 1_000  # tracks in the shared base pack
TASTE_HOURS = [1, 24, 168, None]
HISTORY_HOURS = 720
SEED = 7


def kmeans(x, k, iters, seed):
    """Plain k-means (Euclidean), Lloyd's iterations from a random sample."""
    rng = np.random.default_rng(seed)
    c = x[rng.choice(len(x), k, replace=False)].copy()
    for _ in range(iters):
        a = assign(x, c)
        sums = np.zeros_like(c)
        np.add.at(sums, a, x)
        n = np.bincount(a, minlength=k)
        empty = n == 0
        c[~empty] = sums[~empty] / n[~empty, None]
        c[empty] = x[rng.choice(len(x), int(empty.sum()), replace=False)]
    return c, assign(x, c)


def assign(x, c):
    out = np.empty(len(x), np.int64)
    cc = (c * c).sum(1)
    for lo in range(0, len(x), 100_000):
        d = cc[None, :] - 2 * x[lo:lo + 100_000] @ c.T
        out[lo:lo + 100_000] = d.argmin(1)
    return out


def tastes(train, users, items, v, hours):
    """Each user's decayed sum of their history's vectors, unit length."""
    t = train.filter(pl.col("uid").is_in(pl.Series(users).implode()))
    end = t.group_by("uid").agg(pl.col("ts").max().alias("end"))
    t = t.join(end, on="uid")
    w = np.ones(len(t)) if hours is None else np.power(tau_of(hours), (t["end"] - t["ts"]).to_numpy().astype(np.float64))
    ui = np.searchsorted(np.array(users), t["uid"].to_numpy())
    ii = np.searchsorted(items, t["item"].to_numpy())
    # A sparse users x tracks matrix of weights times the vectors, not one
    # row per listen (29M x 64 floats would not fit in memory).
    m = sp.csr_matrix((w, (ui, ii)), shape=(len(users), len(items)))
    u = np.asarray(m @ v.astype(np.float64))
    u /= np.maximum(np.linalg.norm(u, axis=1, keepdims=True), 1e-12)
    return u.astype(np.float32)


def rrf(*lists, k=60):
    s = {}
    for lst in lists:
        for r, x in enumerate(lst):
            s[x] = s.get(x, 0.0) + 1.0 / (k + r + 1)
    return [x for x, _ in sorted(s.items(), key=lambda kv: -kv[1])][:K]


def run(train, targets, now, work, hours_grid, budgets, pick=None):
    """Cloud and device metrics; returns {name: result}."""
    items = np.sort(train["item"].unique().to_numpy())
    n_items = len(items)
    v = svd_vectors(train, items, WIDTH, SEED)
    c0, a0 = kmeans(v, LEVEL_K, 12, SEED)
    r = v - c0[a0]
    c1, a1 = kmeans(r, LEVEL_K, 12, SEED + 1)
    cell = a0 * LEVEL_K + a1
    cells, cell_of = np.unique(cell, return_inverse=True)
    sizes = np.bincount(cell_of)
    cent = c0[cells // LEVEL_K] + c1[cells % LEVEL_K]
    cent /= np.maximum(np.linalg.norm(cent, axis=1, keepdims=True), 1e-12)
    members = np.argsort(cell_of, kind="stable")
    starts = np.concatenate([[0], np.cumsum(sizes)])
    log(f"{n_items} tracks, {len(cells)} packs, median {int(np.median(sizes))} tracks, largest {sizes.max()}")
    users = sorted(targets)
    pop = decay_pop(train, 1)["item"].to_list()
    base = np.searchsorted(items, np.array(pop[:BASE]))
    hist = history_top(train, now, HISTORY_HOURS, users)
    # Each user's distinct tracks (on the device already).
    g = train.filter(pl.col("uid").is_in(pl.Series(users).implode())).group_by("uid").agg(pl.col("item").unique())
    held = {u: np.searchsorted(items, np.asarray(x)) for u, x in g.iter_rows()}
    out = {"tracks": n_items, "packs": int(len(cells)), "pack_median_tracks": int(np.median(sizes)), "pack_max_tracks": int(sizes.max())}
    t = Tally(n_items)
    for u in users:
        t.add(fill(hist.get(u, []), pop), targets[u])
    out["history"] = t.result()
    # Discovery (the test run only): targets the user never played, against
    # lists without the user's own tracks, so that re-listening, which
    # `history` already answers on the device, does not decide it.
    disc = hours_grid is None
    if disc:
        tp = Tally(n_items)
        for u in users:
            hs = set(items[held.get(u, np.zeros(0, np.int64))].tolist())
            new = [x for x in targets[u] if x not in hs]
            if new:
                tp.add([x for x in pop if x not in hs][:K], new)
        out["discovery_users"] = tp.n
        out["discovery_decay_pop"] = tp.result()
    best_h, best = None, -1.0
    for h in (hours_grid if hours_grid is not None else [pick]):
        U = tastes(train, users, items, v, h)
        cloud, crrf = Tally(n_items), Tally(n_items)
        dev = {b: (Tally(n_items), Tally(n_items)) for b in budgets}
        dcloud, ddev = Tally(n_items), {b: Tally(n_items) for b in budgets}
        for lo in range(0, len(users), 64):
            S = U[lo:lo + 64] @ v.T
            idx = np.argpartition(S, -K, axis=1)[:, -K:]
            for j in range(S.shape[0]):
                u = users[lo + j]
                o = idx[j][np.argsort(-S[j, idx[j]], kind="stable")]
                top = items[o].tolist()
                cloud.add(top, targets[u])
                crrf.add(rrf(hist.get(u, []), top), targets[u])
                hu = held.get(u, np.zeros(0, np.int64))
                new = None
                if disc:
                    hs = set(items[hu].tolist())
                    new = [x for x in targets[u] if x not in hs] or None
                if new:
                    sd = S[j].copy()
                    sd[hu] = -np.inf
                    od = np.argpartition(sd, -K)[-K:]
                    dcloud.add(items[od[np.argsort(-sd[od], kind="stable")]].tolist(), new)
                # The device: packs nearest the taste, up to each budget.
                order = np.argsort(-(cent @ U[lo + j]), kind="stable")
                cum = np.cumsum(sizes[order])
                for b in budgets:
                    n_packs = int(np.searchsorted(cum, b, side="right"))
                    chosen = order[:max(n_packs, 1)]
                    cand = np.concatenate([members[starts[p]:starts[p + 1]] for p in chosen] + [base, held.get(u, np.zeros(0, np.int64))])
                    cand = np.unique(cand)
                    s = S[j, cand]
                    kk = min(K, len(cand))
                    o2 = cand[np.argsort(-s, kind="stable")[:kk]]
                    dtop = items[o2].tolist()
                    dev[b][0].add(dtop, targets[u])
                    dev[b][1].add(rrf(hist.get(u, []), dtop), targets[u])
                    if new:
                        cd = np.setdiff1d(cand, hu)
                        sdv = S[j, cd]
                        ddev[b].add(items[cd[np.argsort(-sdv, kind="stable")[:K]]].tolist(), new)
        res = {"vec_cloud": cloud.result(), "rrf_cloud": crrf.result()}
        for b in budgets:
            res[f"vec_device@{b}"] = dev[b][0].result()
            res[f"rrf_device@{b}"] = dev[b][1].result()
        if disc:
            res["discovery_vec_cloud"] = dcloud.result()
            for b in budgets:
                res[f"discovery_vec_device@{b}"] = ddev[b].result()
        log(f"taste hours {h}: " + json.dumps({k: v_["ndcg@10"] for k, v_ in res.items()}))
        if res["vec_cloud"]["recall@100"] > best:
            best, best_h = res["vec_cloud"]["recall@100"], h
            out.update(res)
    out["taste_hours"] = best_h
    return out


def cmd_prep(a):
    os.makedirs(a.work, exist_ok=True)
    lf = listens(a.data)
    train, targets, now = split(lf, val=True)
    log(f"validation: {len(train)} listens, {len(targets)} users")
    val = run(train, targets, now, a.work, TASTE_HOURS, [BUDGETS[1]])
    json.dump(val, open(os.path.join(a.work, "recs_validation.json"), "w"), indent=1)
    log("chosen taste hours", val["taste_hours"])


def cmd_eval(a):
    val = json.load(open(os.path.join(a.work, "recs_validation.json")))
    lf = listens(a.data)
    train, targets, now = split(lf, val=False)
    log(f"test: {len(train)} listens, {len(targets)} users, {sum(len(x) for x in targets.values())} targets")
    res = run(train, targets, now, a.work, None, BUDGETS, pick=val["taste_hours"])
    res["cloud_baselines_from_recsys"] = json.load(open(os.path.join(HERE, "..", "recsys", "quality", "baselines.json")))["baselines"]
    json.dump(res, open(os.path.join(a.work, "recs_eval.json"), "w"), indent=1)


def cmd_churn(a):
    """How many tracks enter a base pack of the N most played each day, over
    the last 30 days before the test day, and how many tracks are new to the
    catalogue each day."""
    lf = listens(a.data)
    end = yambda.TEST - yambda.GAP
    ev = lf.filter(pl.col("ts") < end).select("item", "ts").collect()
    first = ev.group_by("item").agg(pl.col("ts").min().alias("first"))
    out = {"days": [], "base": {}}
    sets = {n: [] for n in (1_000, 10_000, 50_000)}
    for d in range(30, 0, -1):
        hi = end - (d - 1) * DAY
        day = ev.filter((pl.col("ts") >= hi - 7 * DAY) & (pl.col("ts") < hi))
        top = day.group_by("item").agg(pl.len().alias("n")).sort(["n", "item"], descending=[True, False])["item"].to_numpy()
        for n in sets:
            sets[n].append(set(top[:n].tolist()))
        new = int(((first["first"] >= hi - DAY) & (first["first"] < hi)).sum())
        out["days"].append({"day_end": int(hi), "new_tracks": new})
    for n, ss in sets.items():
        entered = [len(ss[i] - ss[i - 1]) for i in range(1, len(ss))]
        out["base"][str(n)] = {"entered_per_day_mean": float(np.mean(entered)), "entered_per_day_max": int(max(entered))}
    out["new_tracks_per_day_mean"] = float(np.mean([d["new_tracks"] for d in out["days"]]))
    out["catalogue_tracks"] = int(first.height)
    json.dump(out, open(os.path.join(a.work, "recs_churn.json"), "w"), indent=1)
    log(json.dumps(out["base"]), "new per day", out["new_tracks_per_day_mean"])


def cmd_bytes(a):
    """A pack file's bytes per track: a Strata file of tracks with an id, a
    semantic-ID keyword and a 64-dimension vector, float32 (what rescoring
    reads today) and SQ8 codes beside it, searched flat (a device scans a
    few thousand tracks; no graph)."""
    import strata

    rng = np.random.default_rng(SEED)
    out = {}
    for n in (5_000, 50_000):
        path = os.path.join(a.work, f"pack-{n}.strata")
        for p in (path, path + "-wal"):
            if os.path.exists(p):
                os.remove(p)
        schema = {"table": "tracks", "key": "id", "fields": [
            {"name": "id", "type": "keyword"}, {"name": "sid", "type": "keyword"},
            {"name": "emb", "type": "vector", "dim": WIDTH, "codes": "sq8", "index": "flat"}]}
        db = strata.open(path, schema)
        x = rng.standard_normal((n, WIDTH)).astype(np.float32)
        x /= np.linalg.norm(x, axis=1, keepdims=True)
        db.insert([{"id": str(i), "sid": f"{i % 256}.{(i // 256) % 256}", "emb": x[i].tolist()} for i in range(n)])
        db.optimize()
        db.vacuum()
        db.close()
        size = os.path.getsize(path)
        out[str(n)] = {"file_bytes": size, "bytes_per_track": size / n}
        log(n, out[str(n)])
    json.dump(out, open(os.path.join(a.work, "recs_bytes.json"), "w"), indent=1)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--work", default="/home/user/data/slices/work")
    ap.add_argument("--data", default="/home/user/data/yambda")
    sub = ap.add_subparsers(dest="cmd", required=True)
    for c in ("prep", "eval", "churn", "bytes"):
        sub.add_parser(c)
    a = ap.parse_args()
    {"prep": cmd_prep, "eval": cmd_eval, "churn": cmd_churn, "bytes": cmd_bytes}[a.cmd](a)


if __name__ == "__main__":
    main()
