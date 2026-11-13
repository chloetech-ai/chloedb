"""Point-in-time join: pandas merge_asof (reference), Feast 0.68 (DuckDB offline store), ChloeDB ASOF JOIN over HTTP.
Usage: offline.py DATA_DIR CHLOEDB_URL [spine_rows]"""
import sys, time, json, io, urllib.request
import numpy as np, pandas as pd, pyarrow as pa
D, URL = sys.argv[1], sys.argv[2]
N = int(sys.argv[3]) if len(sys.argv) > 3 else None
UF = ["clicks_7d", "buys_30d", "avg_price", "sessions_7d", "dwell_s"]
IF = ["ctr_7d", "views_1d", "price", "stock", "rating"]
spine = pd.read_parquet(f"{D}/spine.parquet")
if N: spine = spine.iloc[:N]
users = pd.read_parquet(f"{D}/users.parquet"); items = pd.read_parquet(f"{D}/items.parquet")
res = {"spine_rows": len(spine), "user_rows": len(users), "item_rows": len(items)}

def timed(name, f, runs=3):
    ts = []
    for _ in range(runs):
        t = time.perf_counter(); out = f(); ts.append(time.perf_counter() - t)
    res[name + "_s"] = sorted(ts)[len(ts) // 2]; res[name + "_runs_s"] = [round(x, 3) for x in ts]
    print(name, [round(x, 2) for x in ts], flush=True)
    return out

def ref():
    s = spine.sort_values("event_timestamp")
    a = pd.merge_asof(s, users.drop(columns="id").rename(columns={"event_timestamp": "u_ts"}).sort_values("u_ts"), left_on="event_timestamp", right_on="u_ts", by="u_id", allow_exact_matches=True)
    a = pd.merge_asof(a.sort_values("event_timestamp"), items.drop(columns="id").rename(columns={"event_timestamp": "i_ts"}).sort_values("i_ts"), left_on="event_timestamp", right_on="i_ts", by="i_id", allow_exact_matches=True)
    return a.set_index("id").sort_index()
R = timed("pandas_merge_asof", ref)

def feast():
    from feast import FeatureStore
    fs = FeatureStore(repo_path="/tmp/claude-0/fs/feast_repo")
    feats = [f"user_stats:{c}" for c in UF] + [f"item_stats:{c}" for c in IF]
    return fs.get_historical_features(entity_df=spine, features=feats).to_df().set_index("id").sort_index()
F = timed("feast_duckdb", feast)

SQL = ("SELECT s.id AS id, s.label AS label, s.event_timestamp AS ts, u.event_timestamp AS u_ts, i.event_timestamp AS i_ts, "
       + ", ".join(f"u.{c} AS {c}" for c in UF) + ", " + ", ".join(f"i.{c} AS {c}" for c in IF)
       + " FROM spine s"
       + " ASOF JOIN users u MATCH_CONDITION (s.event_timestamp >= u.event_timestamp) ON s.u_id = u.u_id"
       + " ASOF JOIN items i MATCH_CONDITION (s.event_timestamp >= i.event_timestamp) ON s.i_id = i.i_id"
       + (f" WHERE s.id IN (SELECT id FROM spine ORDER BY id LIMIT {N})" if False else ""))
def chloedb(sql=SQL):
    body = json.dumps({"sql": sql, "format": "arrow", "timeout_ms": 600000}).encode()
    req = urllib.request.Request(URL + "/v1/query", body, {"content-type": "application/json"})
    with urllib.request.urlopen(req, timeout=900) as r:
        data = r.read()
    return pa.ipc.open_stream(io.BytesIO(data)).read_all().to_pandas().set_index("id").sort_index()
C = timed("chloedb_asof", chloedb)

def compare(name, X):
    X = X.reindex(R.index)
    bad = {}
    for c in UF + IF:
        a, b = R[c].to_numpy(float), X[c].to_numpy(float)
        same = np.isclose(a, b, rtol=0, atol=1e-6) | (np.isnan(a) & np.isnan(b))
        bad[c] = int((~same).sum())
    res[name + "_mismatches"] = bad
    print(name, "mismatched values per column:", bad)
compare("feast", F); compare("chloedb", C)
# leakage: no feature row timestamped after its spine row
ts = pd.to_datetime(C["ts"], utc=True)
res["chloedb_future_user_rows"] = int((pd.to_datetime(C["u_ts"], utc=True) > ts).sum())
res["chloedb_future_item_rows"] = int((pd.to_datetime(C["i_ts"], utc=True) > ts).sum())
res["null_user_features"] = int(R["clicks_7d"].isna().sum())
print(json.dumps(res, indent=1))
json.dump(res, open(f"{D}/offline_result.json", "w"), indent=1)
