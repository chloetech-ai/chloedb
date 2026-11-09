"""The data of the recommender serving benchmark (engine-compare's
`recsys-load` and `recsys-run`), made from Yambda-50M and the sequence
model that yambda.py trained on it.

It writes, under --out:

- `items.jsonl`: the `--items` most played tracks of the training listens,
  each with its learned vector (`emb`, 64 numbers, unit length), its first
  artist, the regions it may be played in, its DecayPop score (`pop`) and
  twelve features a ranker reads (`f00`..`f11`, below).
- `users.jsonl`: every training user with their vector at the start of the
  test day (the sequence model run upstream, as a two-tower system's user
  tower is), their region, the last 500 distinct tracks they played (`seen`)
  and their last 5 (`recent`), both only tracks of `items.jsonl`.
- `ranker/gbdt.json`: a LightGBM model (`--trees` trees) over the twelve
  features and the user-track dot product (`f12`), trained to tell which of a
  user's 200 nearest tracks they played on the validation day (as of its
  start); `ranker/features.json` names its inputs in order.
- `about.json`: what was made, and from what.

Regions are not in Yambda: each track is playable in each of 5 regions
with probability 0.8 and each user is in one, both from a hash of the id,
standing for the licensing a music service filters by.

    python serving_data.py --work ~/data/yambda/work --data ~/data/yambda --out ~/data/recsys-100k --items 100000
"""
import argparse
import hashlib
import json
import os
import sys
import time

import numpy as np
import polars as pl

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import yambda as Y  # noqa: E402

REGIONS = ["r0", "r1", "r2", "r3", "r4"]
FEATURES = [
    ("f00", "plays in the last day"),
    ("f01", "plays in the last 7 days"),
    ("f02", "plays in the last 30 days"),
    ("f03", "plays in all"),
    ("f04", "listeners in all"),
    ("f05", "mean share of the track played, all listens"),
    ("f06", "track length, seconds"),
    ("f07", "share of plays not from a recommendation"),
    ("f08", "likes"),
    ("f09", "dislikes"),
    ("f10", "days since its first play"),
    ("f11", "days since its last play"),
]


def h(s, salt):
    return int.from_bytes(hashlib.blake2b(f"{salt}:{s}".encode(), digest_size=8).digest(), "little")


def regions_of(item):
    rs = [r for i, r in enumerate(REGIONS) if h(item, f"region{i}") % 10 < 8]
    return rs or [REGIONS[h(item, "fallback") % len(REGIONS)]]


def region_of_user(u):
    return REGIONS[h(u, "user") % len(REGIONS)]


def log(*a):
    print(time.strftime("%H:%M:%S"), *a, flush=True)


def features(data, end):
    """The twelve features of every track, from what happened before `end`."""
    raw = pl.scan_parquet(os.path.join(data, "flat", "50m", "listens.parquet")).filter(pl.col("timestamp") < end)
    lp = raw.filter(pl.col("played_ratio_pct") >= Y.LISTEN_PLUS)
    day = Y.DAY
    a = lp.group_by("item_id").agg(
        (pl.col("timestamp") >= end - day).sum().alias("f00"),
        (pl.col("timestamp") >= end - 7 * day).sum().alias("f01"),
        (pl.col("timestamp") >= end - 30 * day).sum().alias("f02"),
        pl.len().alias("f03"),
        pl.col("uid").n_unique().alias("f04"),
        pl.col("is_organic").cast(pl.Float64).mean().alias("f07"),
        ((end - pl.col("timestamp").min()) / day).alias("f10"),
        ((end - pl.col("timestamp").max()) / day).alias("f11"),
    )
    b = raw.group_by("item_id").agg((pl.col("played_ratio_pct").cast(pl.Float64).clip(0, 100) / 100).mean().alias("f05"), pl.col("track_length_seconds").max().cast(pl.Float64).alias("f06"))
    likes = pl.scan_parquet(os.path.join(data, "flat", "50m", "likes.parquet")).filter(pl.col("timestamp") < end).group_by("item_id").agg(pl.len().alias("f08"))
    dis = pl.scan_parquet(os.path.join(data, "flat", "50m", "dislikes.parquet")).filter(pl.col("timestamp") < end).group_by("item_id").agg(pl.len().alias("f09"))
    f = a.join(b, on="item_id", how="left").join(likes, on="item_id", how="left").join(dis, on="item_id", how="left").collect()
    return f.with_columns([pl.col(c).cast(pl.Float64).fill_null(0.0) for c, _ in FEATURES]).rename({"item_id": "item"})


def user_vectors(model_dir, train, now, users):
    """Each user's vector at `now`, from their last 200 listens before it."""
    import torch

    from strata import _sequence as S

    model, cfg = S.load(model_dir)
    vec = S.read_items(model_dir)
    keys = list(vec)
    index = {k: i for i, k in enumerate(keys)}
    E = torch.tensor(np.asarray([vec[k] for k in keys], dtype=np.float32))
    del vec
    t = train.filter(pl.col("ts") < now).group_by("uid", maintain_order=True).agg(pl.col("item").tail(200), pl.col("ts").tail(200))
    hist = {u: [(str(i), None, float(s)) for i, s in zip(its, tss)] for u, its, tss in t.iter_rows()}
    out = {}
    us = [u for u in users if u in hist]
    torch.set_num_threads(os.cpu_count())
    for at in range(0, len(us), 256):
        part = us[at : at + 256]
        v = S.encode_batch(model, [hist[u] for u in part], index, {a: i for i, a in enumerate(cfg["actions"])}, cfg, E, [float(now)] * len(part))
        for u, x in zip(part, v.numpy()):
            out[u] = x.astype(np.float32)
    return out, index, E.numpy()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--work", default=os.path.expanduser("~/data/yambda/work"))
    ap.add_argument("--data", default=os.path.expanduser("~/data/yambda"))
    ap.add_argument("--out", required=True)
    ap.add_argument("--items", type=int, default=100_000)
    ap.add_argument("--trees", type=int, default=300)
    ap.add_argument("--rank-users", type=int, default=2000)
    a = ap.parse_args()
    os.makedirs(os.path.join(a.out, "ranker"), exist_ok=True)
    train, _ = Y.read_split(a.work)
    chosen = json.load(open(os.path.join(a.work, "baselines.json")))["chosen"]
    model_dir = os.path.join(a.work, "models", Y.MODEL)
    end = Y.TEST - Y.GAP
    counts = train.group_by("item").agg(pl.len().alias("n")).sort(["n", "item"], descending=[True, False])
    keep = counts["item"].to_list()[: a.items]
    keep_set = set(keep)
    pop = Y.decay_pop(train, chosen["decay_pop"])
    pop_of = dict(zip(pop["item"].to_list(), pop["w"].to_list()))
    artist = pl.read_parquet(os.path.join(a.data, "artist_item_mapping.parquet")).group_by("item_id").agg(pl.col("artist_id").min())
    artist_of = dict(zip(artist["item_id"].to_list(), artist["artist_id"].to_list()))
    log("features")
    feats = features(a.data, end)
    feat_of = {r[0]: r[1:] for r in feats.select(["item"] + [c for c, _ in FEATURES]).iter_rows()}
    log("user vectors at the test day")
    users = sorted(train["uid"].unique().to_list())
    uvec, index, E = user_vectors(model_dir, train, Y.TEST, users)
    rows = np.array([index[str(i)] for i in keep])
    item_E = E[rows]
    with open(os.path.join(a.out, "items.jsonl"), "w") as f:
        for i, it in enumerate(keep):
            fv = feat_of.get(it, (0.0,) * len(FEATURES))
            doc = {"id": str(it), "emb": [round(float(x), 6) for x in item_E[i]], "artist": f"a{artist_of.get(it, 0)}", "regions": regions_of(it), "pop": round(float(pop_of.get(it, 0.0)), 6)}
            doc.update({c: round(float(v), 6) for (c, _), v in zip(FEATURES, fv)})
            f.write(json.dumps(doc) + "\n")
    log(f"{len(keep)} items")
    # Users: their vector, region, and the tracks they played last.
    last = train.group_by("uid", maintain_order=True).agg(pl.col("item"))
    n_users = 0
    with open(os.path.join(a.out, "users.jsonl"), "w") as fu:
        for u, items in last.iter_rows():
            if u not in uvec:
                continue
            seen, recent, have = [], [], set()
            for it in reversed(items):
                if it in keep_set and it not in have:
                    have.add(it)
                    seen.append(str(it))
                    if len(recent) < 5:
                        recent.append(str(it))
                    if len(seen) >= 500:
                        break
            fu.write(json.dumps({"id": str(u), "vec": [round(float(x), 6) for x in uvec[u]], "region": region_of_user(u), "seen": seen, "recent": recent}) + "\n")
            n_users += 1
    log(f"{n_users} users")
    ranker(a, train, model_dir, keep, item_E, feat_of)
    json.dump({"items": len(keep), "users": n_users, "features": FEATURES, "regions": REGIONS, "decay_pop_hours": chosen["decay_pop"], "trees": a.trees}, open(os.path.join(a.out, "about.json"), "w"), indent=1)


def ranker(a, train, model_dir, keep, item_E, feat_of):
    """A LightGBM model of the features and the dot product, trained on
    whether a user played each of their 200 nearest tracks on the
    validation day."""
    import lightgbm as lgb

    lo, hi = Y.TEST - Y.DAY - Y.GAP, Y.TEST - Y.GAP
    lf = Y.listens(a.data)
    played = lf.filter((pl.col("ts") >= lo) & (pl.col("ts") < hi)).collect().group_by("uid").agg(pl.col("item").unique())
    played_of = {u: set(x) for u, x in played.iter_rows()}
    users = sorted(played_of)[: a.rank_users]
    log(f"ranker: user vectors at the validation day for {len(users)} users")
    uvec, _, _ = user_vectors(model_dir, train, lo, users)
    X, y, groups = [], [], []
    for u in users:
        if u not in uvec:
            continue
        s = item_E @ uvec[u]
        top = np.argpartition(-s, 200)[:200]
        for i in top:
            it = keep[i]
            X.append(list(feat_of.get(it, (0.0,) * len(FEATURES))) + [float(s[i])])
            y.append(1 if it in played_of[u] else 0)
        groups.append(len(top))
    X, y = np.asarray(X), np.asarray(y)
    log(f"ranker: {len(y)} rows, {y.mean():.3f} positive")
    m = lgb.train({"objective": "binary", "num_leaves": 31, "learning_rate": 0.05, "min_data_in_leaf": 50, "verbose": -1, "num_threads": os.cpu_count(), "seed": 0}, lgb.Dataset(X, y, feature_name=[c for c, _ in FEATURES] + ["f12"]), num_boost_round=a.trees)
    json.dump(m.dump_model(), open(os.path.join(a.out, "ranker", "gbdt.json"), "w"))
    m.save_model(os.path.join(a.out, "ranker", "model.txt"))
    json.dump({"features": [c for c, _ in FEATURES] + ["f12 (the dot product of the user's and the track's vectors)"], "trees": a.trees, "rows": len(y), "positive": float(y.mean())}, open(os.path.join(a.out, "ranker", "features.json"), "w"), indent=1)


if __name__ == "__main__":
    sys.exit(main())
