"""Online lookup of 10 features for 1,000 (user, item) pairs: Feast 0.68 (SQLite online store, in process)
vs ChloeDB /v1/kv/get (two calls, users_latest and items_latest, columns only). Usage: online.py DATA URL"""
import sys, time, json, urllib.request, statistics
import numpy as np, pandas as pd
D, URL = sys.argv[1], sys.argv[2]
UF = ["clicks_7d", "buys_30d", "avg_price", "sessions_7d", "dwell_s"]; IF = ["ctr_7d", "views_1d", "price", "stock", "rating"]
rng = np.random.default_rng(1)
batches = [[(f"u{u}", f"i{i}") for u, i in zip(rng.integers(0, 20000, 1000), rng.integers(0, 10000, 1000))] for _ in range(60)]
from feast import FeatureStore
fs = FeatureStore(repo_path="/tmp/claude-0/fs/feast_repo")
feats = [f"user_stats:{c}" for c in UF] + [f"item_stats:{c}" for c in IF]
def feast(b):
    return fs.get_online_features(features=feats, entity_rows=[{"u_id": u, "i_id": i} for u, i in b]).to_dict()
def post(path, body):
    req = urllib.request.Request(URL + path, json.dumps(body).encode(), {"content-type": "application/json"})
    with urllib.request.urlopen(req) as r:
        return json.load(r)
def chloedb(b):
    us = post("/v1/kv/get", {"table": "users_latest", "keys": [u for u, _ in b], "columns": UF})
    it = post("/v1/kv/get", {"table": "items_latest", "keys": [i for _, i in b], "columns": IF})
    return us, it
res = {}
for name, f in [("feast_sqlite", feast), ("chloedb_kv", chloedb)]:
    for b in batches[:5]: f(b)
    ts = []
    for b in batches[5:]:
        t = time.perf_counter(); f(b); ts.append((time.perf_counter() - t) * 1000)
    ts.sort()
    res[name] = {"p50_ms": round(statistics.median(ts), 2), "p99_ms": round(ts[int(len(ts) * 0.99) - 1], 2), "n": len(ts)}
    print(name, res[name], flush=True)
# same answers?
b = batches[0]
F = feast(b); us, it = chloedb(b)
uk = {c: [x[c] if x else None for x in us["rows"]] for c in UF}
ok = all(np.allclose(np.array(F[c], float), np.array(uk[c], float), equal_nan=True) for c in UF)
res["same_user_values"] = bool(ok)
print(res); json.dump(res, open(f"{D}/online_result.json", "w"), indent=1)
