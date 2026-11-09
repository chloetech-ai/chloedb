"""On-device slices, vector side: how often a device that holds a few of a
catalogue's vector clusters can prove that its top k is the whole
catalogue's, so that it need not ask the cloud. Yambda-50M's 629K tracks,
on the test day of `../recsys/yambda.py`'s protocol, with the vectors and
tastes of `recs.py` (64-dimension SVD vectors, unit length; a user's taste
the decayed sum of their history's vectors, with recs.py's chosen decay).

A vector slice (`strata-slice --vector emb`, strata::slice) clusters the
table by k-means. Its manifest holds each cluster's centre, radius, angular
radius and longest and shortest vector, from which a device bounds the
best score any row of a cluster it does not hold could reach
(`Vectors::bound`: the lesser of a ball and a cone bound, plus a float32
rounding margin). Its `codes.bin.gz` holds every row's vector as one byte
a dimension (`Codes`), which bounds each row within half a step a
dimension. A device holds the clusters with the highest bound for its
user's taste, up to a budget of tracks; a search is proved when its k-th
score is at least every bound of a cluster not held. When it is not, the
clusters whose bound beats the k-th (`Answer::need`) are what the device
would fetch to prove it.

Two searches a user, each with k = 10 and k = 100:

- `taste`: the user's taste, "recommend for me";
- `last`: the vector of the last track the user played, "more like this".

`eval` replays every user with numpy (the bounds recomputed from the
manifest and the codes, as the device computes them) and checks that every
proved answer, before and after fetching what the codes name, is the exact
top k over all tracks; `verify` runs Strata's own device
(strata.SliceDevice, on the files the slice wrote) for a sample of users
and compares every answer it proves with the cloud's exact search
(`Database.query` on the SQL the device would send).

    python vectors.py prep     # split, vectors, tastes, tracks.strata
    python vectors.py slices   # strata-slice, with codes
    python vectors.py eval     # proved rates, every proved answer checked
    python vectors.py verify   # Strata's device against Strata's cloud
"""
import argparse
import gzip
import json
import os
import shutil
import subprocess
import sys
import time

import numpy as np
import polars as pl

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "recsys"))
sys.path.insert(0, HERE)
from yambda import listens, log, split, svd_vectors  # noqa: E402
from recs import SEED, WIDTH, tastes  # noqa: E402

CLUSTERS = [0, 4096]  # 0: strata-slice's default, about the square root of the rows
BUDGETS = [5_000, 20_000, 50_000]  # tracks a device holds
KS = [10, 100]
SLICE_BIN = os.path.join(HERE, "..", "..", "target", "ci", "strata-slice")


def work(a):
    w = os.path.join(a.work, "vectors")
    os.makedirs(w, exist_ok=True)
    return w


def cmd_prep(a):
    import strata

    w = work(a)
    hours = json.load(open(os.path.join(HERE, "recs", "recs_eval.json")))["taste_hours"]
    lf = listens(a.data)
    train, targets, now = split(lf, val=False)
    items = np.sort(train["item"].unique().to_numpy())
    v = svd_vectors(train, items, WIDTH, SEED)
    users = sorted(targets)
    taste = tastes(train, users, items, v, hours)
    last = (
        train.filter(pl.col("uid").is_in(pl.Series(users).implode()))
        .sort(["uid", "ts"])
        .group_by("uid", maintain_order=True)
        .agg(pl.col("item").last())
    )
    li = dict(zip(last["uid"].to_list(), last["item"].to_list()))
    lv = v[np.searchsorted(items, np.array([li[u] for u in users]))]
    np.savez(os.path.join(w, "queries.npz"), items=items, v=v, users=np.array(users), taste=taste, last=lv)
    log(f"{len(items)} tracks, {len(users)} users, taste hours {hours}")

    path = os.path.join(w, "tracks.strata")
    for p in (path, path + "-wal"):
        if os.path.exists(p):
            os.remove(p)
    schema = {"table": "tracks", "key": "id", "fields": [
        {"name": "id", "type": "int"},
        {"name": "emb", "type": "vector", "dim": WIDTH, "index": "flat"}]}
    db = strata.open(path, schema)
    t = time.time()
    r = db.import_arrow(pl.DataFrame({"id": np.arange(len(items), dtype=np.int64), "emb": v}))
    db.optimize()
    db.close()
    log(f"tracks.strata: {r['rows']} rows in {time.time() - t:.0f} s, {os.path.getsize(path) / 1e6:.0f} MB")
    json.dump({"tracks": int(len(items)), "users": len(users), "taste_hours": hours}, open(os.path.join(w, "vectors_prep.json"), "w"), indent=1)


def cmd_slices(a):
    w = work(a)
    prep = json.load(open(os.path.join(w, "vectors_prep.json")))
    builds = {}
    for c in CLUSTERS:
        out = os.path.join(w, f"slice-{c}")
        shutil.rmtree(out, ignore_errors=True)
        t = time.time()
        p = subprocess.run([SLICE_BIN, os.path.join(w, "tracks.strata"), "--vector", "emb", "--clusters", str(c), "--seed", str(SEED), "--out", out],
                           capture_output=True, text=True, check=True)
        m = json.load(open(os.path.join(out, "manifest.json")))
        builds[str(c)] = {
            "clusters": len(m["tiles"]),
            "seconds": round(time.time() - t, 1),
            "manifest_bytes": os.path.getsize(os.path.join(out, "manifest.json")),
            "tile_bytes": sum(e["bytes"] for e in m["tiles"]),
            "codes_bytes": m["codes"]["bytes"],
            "rows": sum(e["rows"] for e in m["tiles"]),
            "cluster_rows_median": int(np.median([e["rows"] for e in m["tiles"]])),
            "cluster_rows_max": max(e["rows"] for e in m["tiles"]),
            "stdout": p.stdout.strip(),
        }
        log(f"slice --clusters {c}: {p.stdout.strip()}")
    prep["builds"] = builds
    json.dump(prep, open(os.path.join(w, "vectors_prep.json"), "w"), indent=1)


class Bounds:
    """Vectors::bound in numpy: for each query and each cluster with rows,
    the best score a row of the cluster could reach."""

    def __init__(self, vk, tiles):
        self.c = np.array(vk["centroids"], np.float64)[tiles]
        self.r = np.array(vk["radii"])[tiles]
        self.a = np.array(vk["angles"])[tiles]
        self.hi = np.array(vk["max_norms"])[tiles]
        self.lo = np.array(vk["min_norms"])[tiles]
        self.cn = np.linalg.norm(self.c, axis=1)
        self.dim = vk["dim"]

    def __call__(self, q):
        q = q.astype(np.float64)
        qn = np.linalg.norm(q, axis=1, keepdims=True)
        dot = q @ self.c.T
        ball = dot + qn * self.r
        theta = np.arccos(np.clip(dot / np.maximum(qn * self.cn, 1e-300), -1.0, 1.0))
        cos = np.cos(np.maximum(theta - self.a - 1e-6, 0.0))
        cone = qn * np.where(cos >= 0, self.hi, self.lo) * cos
        cone = np.where((self.cn > 0) & (qn > 0), cone, np.inf)
        margin = self.dim * 2.0 ** -23 * qn * np.maximum(self.hi, self.cn + self.r)
        return np.minimum(ball, cone) + margin


class Slice:
    """A slice's manifest, each tile's rows (by cluster) and every row's
    codes, as the device reads them."""

    def __init__(self, w, c, n_items):
        out = self.dir = os.path.join(w, f"slice-{c}")
        self.manifest_text = open(os.path.join(out, "manifest.json")).read()
        m = self.m = json.loads(self.manifest_text)
        assert m["kind"] == "vector"
        self.tiles = np.array([e["x"] for e in m["tiles"]])
        self.sizes = np.array([e["rows"] for e in m["tiles"]])
        self.bytes = np.array([e["bytes"] for e in m["tiles"]])
        # Each row's id in the order of the codes: tile after tile, each
        # tile's rows in the order of its file.
        ids = []
        for x in self.tiles:
            with gzip.open(os.path.join(out, "tiles", f"{x}_0.jsonl.gz"), "rt") as f:
                ids += [json.loads(line)[0] for line in f if line.strip()]
        self.order = np.array(ids, np.int64)
        assert len(self.order) == n_items and len(np.unique(self.order)) == n_items, "every track in one cluster"
        self.starts = np.concatenate([[0], np.cumsum(self.sizes)])
        self.member = np.repeat(np.arange(len(self.tiles)), self.sizes)  # by position in `order`
        with gzip.open(os.path.join(out, "codes.bin.gz"), "rb") as f:
            self.codes = np.frombuffer(f.read(), np.uint8).reshape(n_items, -1).astype(np.float32)
        self.cmin = np.array(m["codes"]["min"], np.float64)
        self.cstep = np.array(m["codes"]["step"], np.float64)
        self.bounds = Bounds(m, self.tiles)

    def rows(self, ps):
        """The track ids of the tiles at positions `ps`."""
        return np.concatenate([self.order[self.starts[p]:self.starts[p + 1]] for p in ps]) if len(ps) else np.zeros(0, np.int64)

    def code_bounds(self, Q):
        """For each query, each cluster's most its rows could score by their
        codes (Codes::query and CodeQuery::best, plus Vectors::margin)."""
        Q = Q.astype(np.float64)
        dim = Q.shape[1]
        w = Q * self.cstep
        base = Q @ self.cmin
        half = np.abs(Q) @ self.cstep * (0.5 + 1e-6)
        fsum = (np.abs(w) * 255.0).sum(1) * 2.0 * dim * 2.0 ** -24
        s = self.codes @ w.T.astype(np.float32)  # rows x queries
        best = np.maximum.reduceat(s, self.starts[:-1], axis=0).T  # queries x clusters
        qn = np.linalg.norm(Q, axis=1, keepdims=True)
        margin = dim * 2.0 ** -23 * qn * self.bounds.hi
        return best + (base + half + fsum)[:, None] + margin


def plan(bt, sizes, budget):
    """Device::plan_vector: the clusters a device holds for a taste whose
    bounds are `bt`, best bound first, as many as fit in `budget` tracks
    (at least one)."""
    order = np.lexsort((np.arange(len(bt)), -bt))
    n = max(int(np.searchsorted(np.cumsum(sizes[order]), budget, side="right")), 1)
    return order[:n]


def topk(s, rows, k):
    """The best k of `rows` by the scores `s`, best first."""
    if len(rows) > k:
        rows = rows[np.argpartition(-s[rows], k)[:k]]
    return rows[np.argsort(-s[rows], kind="stable")]


def cmd_eval(a):
    w = work(a)
    z = np.load(os.path.join(w, "queries.npz"))
    v, taste, last = z["v"], z["taste"], z["last"]
    n_items, n_users = len(v), len(taste)
    prep = json.load(open(os.path.join(w, "vectors_prep.json")))
    out = {"tracks": n_items, "users": n_users, "taste_hours": prep["taste_hours"], "builds": prep["builds"], "runs": {}}
    queries = {"taste": taste, "last": last}
    kmax = max(KS)
    for c in CLUSTERS:
        sl = Slice(w, c, n_items)
        nC = len(sl.tiles)
        keys = [(qk, b, k) for qk in queries for b in BUDGETS for k in KS]
        tally = {key: {"searches": 0, "proved_centres": 0, "proved_codes": 0, "exact": 0, "unsound": 0, "need_clusters": [], "need_bytes": [],
                       "proved_after_fetch": 0, "exact_after_fetch": 0, "clusters_held": 0, "tracks_held": 0} for key in keys}
        t = time.time()
        for lo in range(0, n_users, 64):
            held = {b: [plan(bt, sl.sizes, b) for bt in sl.bounds(taste[lo:lo + 64])] for b in BUDGETS}
            for qk, Q in queries.items():
                q = Q[lo:lo + 64]
                S = q @ v.T  # float32, as Strata scores
                geo = sl.bounds(q)
                code = np.minimum(geo, sl.code_bounds(q))
                exact = np.argpartition(-S, kmax, axis=1)[:, :kmax]
                for j in range(len(q)):
                    ex = exact[j][np.argsort(-S[j, exact[j]], kind="stable")]
                    for b in BUDGETS:
                        h = held[b][j]
                        dk = topk(S[j], sl.rows(h), kmax)
                        notheld = np.ones(nC, bool)
                        notheld[h] = False
                        for k in KS:
                            r = tally[(qk, b, k)]
                            r["searches"] += 1
                            r["clusters_held"] += len(h)
                            r["tracks_held"] += int(sl.sizes[h].sum())
                            kth = S[j, dk[k - 1]]
                            same = bool(np.array_equal(S[j, dk[:k]], S[j, ex[:k]]))
                            pc = bool(kth >= geo[j][notheld].max(initial=-np.inf))
                            pk = bool(kth >= code[j][notheld].max(initial=-np.inf))
                            r["proved_centres"] += pc
                            r["proved_codes"] += pk
                            r["exact"] += same
                            r["unsound"] += (pc or pk) and not same
                            # What the codes name: the clusters not held that
                            # could beat the k-th. Holding them too proves it.
                            need = np.flatnonzero(notheld & (code[j] > kth))
                            r["need_clusters"].append(len(need))
                            r["need_bytes"].append(int(sl.bytes[need].sum()))
                            if len(need):
                                dk2 = topk(S[j], sl.rows(np.concatenate([h, need])), k)
                                nh2 = notheld.copy()
                                nh2[need] = False
                                p2 = bool(S[j, dk2[k - 1]] >= code[j][nh2].max(initial=-np.inf))
                                same2 = bool(np.array_equal(S[j, dk2[:k]], S[j, ex[:k]]))
                            else:
                                p2, same2 = pk, same
                            r["proved_after_fetch"] += p2
                            r["exact_after_fetch"] += same2
                            r["unsound"] += p2 and not same2
            if lo % 1024 == 0:
                log(f"clusters {nC}: {lo + len(q)} of {n_users} users, {time.time() - t:.0f} s")
        run = {"clusters": int(nC), "results": []}
        for (qk, b, k), r in tally.items():
            n = r["searches"]
            nb = np.array(r.pop("need_bytes"))
            nc = np.array(r.pop("need_clusters"))
            res = {"query": qk, "budget": b, "k": k, "searches": n,
                   "proved_centres": r["proved_centres"] / n, "proved_codes": r["proved_codes"] / n, "exact": r["exact"] / n,
                   "proved_after_fetch": r["proved_after_fetch"] / n, "exact_after_fetch": r["exact_after_fetch"] / n, "unsound": r["unsound"],
                   "fetch_share": float((nc > 0).mean()), "fetch_clusters_mean": float(nc.mean()), "fetch_bytes_mean": float(nb.mean()),
                   "fetch_bytes_p50_when_fetching": float(np.median(nb[nc > 0])) if (nc > 0).any() else 0.0,
                   "fetch_bytes_p90_when_fetching": float(np.percentile(nb[nc > 0], 90)) if (nc > 0).any() else 0.0,
                   "clusters_held_mean": r["clusters_held"] / n, "tracks_held_mean": r["tracks_held"] / n}
            run["results"].append(res)
            log(f"clusters {nC} {qk} budget {b} k {k}: proved by centres {res['proved_centres']:.3f}, by codes {res['proved_codes']:.3f}, "
                f"after fetching {res['proved_after_fetch']:.3f} ({res['fetch_clusters_mean']:.1f} clusters, {res['fetch_bytes_mean'] / 1e3:.0f} KB); unsound {r['unsound']}")
        out["runs"][str(c)] = run
    json.dump(out, open(os.path.join(HERE, "vectors", "vectors_eval.json"), "w"), indent=1)


def same_answer(got, gs, want, ws, tol=1e-6):
    """The same k scores, and the same rows above the k-th score (rows
    tied with it may differ)."""
    gs, ws = np.array(gs, np.float64), np.array(ws, np.float64)
    if len(gs) != len(ws) or not np.allclose(gs, ws, rtol=0, atol=tol):
        return False
    kth = ws[-1]
    return {x for x, s in zip(got, gs) if s > kth + tol} == {x for x, s in zip(want, ws) if s > kth + tol}


def cmd_verify(a):
    """Strata's device (SliceDevice, with the slice's codes) for a sample
    of users at one budget: each answer it proves, before and after it
    fetches the clusters it names, against the cloud's exact search of
    the same SQL; and its proofs against the replay's."""
    import strata
    from strata._strata import SliceDevice

    w = work(a)
    z = np.load(os.path.join(w, "queries.npz"))
    v, taste, last = z["v"], z["taste"], z["last"]
    rng = np.random.default_rng(SEED)
    sample = np.sort(rng.choice(len(taste), a.users, replace=False))
    cloud = strata.Database.open(os.path.join(w, "tracks.strata"))
    out = {"users": int(a.users), "budget": a.budget, "runs": []}
    for c in CLUSTERS:
        sl = Slice(w, c, len(v))
        codes_gz = open(os.path.join(sl.dir, "codes.bin.gz"), "rb").read()
        b = a.budget
        r = {"clusters": int(len(sl.tiles)), "searches": 0, "proved": 0, "proved_as_replay": 0, "fetched": 0, "proved_after_fetch": 0,
             "proved_and_same_as_cloud": 0, "proved_not_same": 0, "plan_as_replay": 0, "device_ms": [], "fetch_bytes": [], "cloud_ms": []}
        pos = {int(x): i for i, x in enumerate(sl.tiles)}
        for u in sample:
            n = len(plan(sl.bounds(taste[u:u + 1])[0], sl.sizes, b))
            dev = SliceDevice.create(os.path.join(w, "device"), sl.manifest_text)
            dev.add_codes(codes_gz)
            fetch, drop = dev.plan_vector(taste[u].tolist(), b)
            assert not drop
            r["plan_as_replay"] += int(len(fetch) == n)
            for x, y, path, _ in fetch:
                dev.add_tile(x, y, open(os.path.join(sl.dir, path), "rb").read())
            notheld = np.ones(len(sl.tiles), bool)
            notheld[[pos[x] for x, _, _, _ in fetch]] = False
            for q in (taste[u], last[u]):
                code = np.minimum(sl.bounds(q[None, :])[0], sl.code_bounds(q[None, :])[0])
                for k in KS:
                    t = time.perf_counter()
                    ans = json.loads(dev.search_vector(q.tolist(), k))
                    r["device_ms"].append((time.perf_counter() - t) * 1e3)
                    ask = json.loads(ans["ask"])
                    t = time.perf_counter()
                    rs = cloud.query(ask["sql"], {"v": q.tolist()})
                    r["cloud_ms"].append((time.perf_counter() - t) * 1e3)
                    r["searches"] += 1
                    replay = ans["kth"] >= code[notheld].max(initial=-np.inf)
                    r["proved_as_replay"] += int(ans["proved"] == bool(replay))
                    added = []
                    if ans["proved"]:
                        r["proved"] += 1
                    elif ans["why"] == "bound":
                        # Fetch what it named, and search again.
                        r["fetched"] += 1
                        r["fetch_bytes"].append(sum(nb for _, _, nb in ans["need"]))
                        for x, y, _ in ans["need"]:
                            dev.add_tile(x, y, open(os.path.join(sl.dir, f"tiles/{x}_{y}.jsonl.gz"), "rb").read())
                            added.append((x, y))
                        ans = json.loads(dev.search_vector(q.tolist(), k))
                        r["proved_after_fetch"] += ans["proved"]
                    if ans["proved"]:
                        same = same_answer(ans["keys"], ans["scores"], rs.keys, rs.scores)
                        r["proved_and_same_as_cloud"] += same
                        if not same:
                            r["proved_not_same"] += 1
                            log(f"user {u} k {k}: proved but not the cloud's: {ans['keys'][:5]} vs {rs.keys[:5]}")
                    for x, y in added:
                        dev.drop_tile(x, y)
            dev.close()
        for key in ("device_ms", "cloud_ms", "fetch_bytes"):
            x = np.array(r.pop(key) or [0.0])
            r[key + "_p50"] = float(np.median(x))
            r[key + "_p99"] = float(np.percentile(x, 99))
        log(f"clusters {len(sl.tiles)}: " + json.dumps(r))
        out["runs"].append(r)
    cloud.close()
    shutil.rmtree(os.path.join(w, "device"), ignore_errors=True)
    json.dump(out, open(os.path.join(HERE, "vectors", "vectors_verify.json"), "w"), indent=1)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--work", default="/home/user/data/slices/work")
    ap.add_argument("--data", default="/home/user/data/yambda")
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("prep")
    sub.add_parser("slices")
    sub.add_parser("eval")
    v = sub.add_parser("verify")
    v.add_argument("--users", type=int, default=200)
    v.add_argument("--budget", type=int, default=20_000)
    a = ap.parse_args()
    os.makedirs(os.path.join(HERE, "vectors"), exist_ok=True)
    {"prep": cmd_prep, "slices": cmd_slices, "eval": cmd_eval, "verify": cmd_verify}[a.cmd](a)


if __name__ == "__main__":
    main()
