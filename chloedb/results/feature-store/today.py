"""ChloeDB without ASOF JOIN: LEFT JOIN with the time in ON, then DISTINCT ON, one table at a time in WITH."""
import sys, json, io, time, urllib.request, numpy as np, pandas as pd, pyarrow as pa
D, URL = sys.argv[1], sys.argv[2]
UF = ["clicks_7d", "buys_30d", "avg_price", "sessions_7d", "dwell_s"]; IF = ["ctr_7d", "views_1d", "price", "stock", "rating"]
SQL = ("WITH a AS (SELECT DISTINCT ON (s.id) s.id AS id, s.label AS label, s.i_id AS i_id, s.event_timestamp AS ts, " + ", ".join(f"u.{c} AS {c}" for c in UF)
 + " FROM spine s LEFT JOIN users u ON s.u_id = u.u_id AND u.event_timestamp <= s.event_timestamp ORDER BY s.id, u.event_timestamp DESC NULLS LAST) "
 + "SELECT DISTINCT ON (a.id) a.id AS id, a.label AS label, " + ", ".join(f"a.{c} AS {c}" for c in UF) + ", " + ", ".join(f"i.{c} AS {c}" for c in IF)
 + " FROM a LEFT JOIN items i ON a.i_id = i.i_id AND i.event_timestamp <= a.ts ORDER BY a.id, i.event_timestamp DESC NULLS LAST")
def run():
    body = json.dumps({"sql": SQL, "format": "arrow", "timeout_ms": 600000, "profile": True}).encode()
    req = urllib.request.Request(URL + "/v1/query", body, {"content-type": "application/json"})
    with urllib.request.urlopen(req, timeout=900) as r:
        data = r.read(); h = r.headers
    return pa.ipc.open_stream(io.BytesIO(data)).read_all().to_pandas().set_index("id").sort_index(), h
ts = []
for _ in range(3):
    t = time.perf_counter()
    try: C, h = run()
    except urllib.error.HTTPError as e: print("error", e.read()[:400]); sys.exit()
    ts.append(round(time.perf_counter() - t, 2))
print("chloedb_today", ts, len(C))
print(str(h.get("x-chloedb-stats"))[:300])
R = pd.read_parquet(f"{D}/spine.parquet").set_index("id").sort_index()
# reference from the offline run's pandas: recompute quickly
users = pd.read_parquet(f"{D}/users.parquet"); items = pd.read_parquet(f"{D}/items.parquet")
s = R.reset_index().sort_values("event_timestamp")
a = pd.merge_asof(s, users.drop(columns="id").rename(columns={"event_timestamp": "u_ts"}).sort_values("u_ts"), left_on="event_timestamp", right_on="u_ts", by="u_id")
a = pd.merge_asof(a.sort_values("event_timestamp"), items.drop(columns="id").rename(columns={"event_timestamp": "i_ts"}).sort_values("i_ts"), left_on="event_timestamp", right_on="i_ts", by="i_id").set_index("id").sort_index()
C = C.reindex(a.index)
print({c: int((~(np.isclose(a[c].to_numpy(float), C[c].to_numpy(float)) | (a[c].isna().to_numpy() & C[c].isna().to_numpy()))).sum()) for c in UF + IF})
json.dump({"chloedb_today_runs_s": ts}, open(f"{D}/today_result.json", "w"))
