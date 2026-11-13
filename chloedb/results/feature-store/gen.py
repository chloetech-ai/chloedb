"""Synthetic feature-store data: daily user and item snapshots, a labelled spine.
Usage: gen.py OUTDIR USERS ITEMS DAYS SPINE"""
import sys, numpy as np, pandas as pd
out, U, I, D, S = sys.argv[1], *map(int, sys.argv[2:])
rng = np.random.default_rng(7)
t0 = pd.Timestamp("2026-10-01", tz="UTC")
def snapshots(n, prefix, cols):
    # each entity has a snapshot most days, at a random second of the day
    ent = np.repeat(np.arange(n), D); day = np.tile(np.arange(D), n)
    keep = rng.random(len(ent)) < 0.8
    ent, day = ent[keep], day[keep]
    ts = t0 + pd.to_timedelta(day * 86400 + rng.integers(0, 86400, len(ent)), unit="s")
    df = pd.DataFrame({"id": [f"{prefix}{e}_{d}" for e, d in zip(ent, day)], prefix + "_id": [f"{prefix}{e}" for e in ent], "event_timestamp": ts})
    for c in cols:
        df[c] = rng.gamma(2.0, 10.0, len(df)).round(3)
    return df
users = snapshots(U, "u", ["clicks_7d", "buys_30d", "avg_price", "sessions_7d", "dwell_s"])
items = snapshots(I, "i", ["ctr_7d", "views_1d", "price", "stock", "rating"])
spine = pd.DataFrame({
    "id": [f"s{k}" for k in range(S)],
    "u_id": [f"u{x}" for x in rng.integers(0, U, S)],
    "i_id": [f"i{x}" for x in rng.integers(0, I, S)],
    "event_timestamp": t0 + pd.to_timedelta(rng.integers(86400, D * 86400, S), unit="s"),
    "label": rng.integers(0, 2, S),
})
for name, df in [("users", users), ("items", items), ("spine", spine)]:
    df.to_parquet(f"{out}/{name}.parquet", index=False)
    print(name, len(df))
