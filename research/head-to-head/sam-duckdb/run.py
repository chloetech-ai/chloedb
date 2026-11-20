"""Sam's head-to-head: his analytic battery, ChloeDB against DuckDB (and
Postgres where the same SQL runs there).

Sam (persona study, round 4) wants "a benchmark of our own models against
DuckDB and Postgres (same results, run time)". This runs his round 4 battery
(`queries.py`: 45 queries over trips and zones, 18 over a music store, 10 the
docs say are refused, one past the server's row window) on made-up data of
his shape (`gen.py`), on each engine in turn on one machine, and records:

- whether ChloeDB's rows equal DuckDB's, in order (floats to 9 significant
  digits, timestamps as text, as Sam compared them), and Postgres's too;
- each query's time: ChloeDB's own `took_ms` and the client's wall time over
  HTTP, DuckDB's and Postgres's wall time in process, the median of 5 after a
  warm-up;
- loading, peak memory and disk of each.

    python3 run.py [TRIPS]        # default 205064, Sam's size; writes raw/results-<TRIPS>.json
"""

import datetime
import decimal
import json
import os
import resource
import sqlite3
import statistics
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import common  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from queries import Q  # noqa: E402

REPS = int(os.environ.get("H2H_REPS", "5"))
SQLITE_TYPES = {"INTEGER": "BIGINT", "REAL": "DOUBLE PRECISION", "TEXT": "TEXT", "DATETIME": "TIMESTAMP"}


def norm(v):
    """Sam's normalisation: floats to 9 significant digits, ints as floats,
    timestamps as 'YYYY-MM-DD HH:MM:SS' text."""
    if v is None or isinstance(v, bool):
        return v
    if isinstance(v, int):
        return float(v)
    if isinstance(v, decimal.Decimal):
        v = float(v)
    if isinstance(v, float):
        return 0.0 if v == 0 else float(f"{v:.9g}")
    if isinstance(v, datetime.datetime):
        if v.tzinfo:
            v = v.astimezone(datetime.timezone.utc).replace(tzinfo=None)
        return v.isoformat(sep=" ")
    if isinstance(v, datetime.date):
        return datetime.datetime(v.year, v.month, v.day).isoformat(sep=" ")
    if isinstance(v, str) and len(v) >= 20 and v[4] == "-" and v[10] == "T" and v.endswith("Z"):
        try:
            return datetime.datetime.fromisoformat(v[:-1]).isoformat(sep=" ")
        except ValueError:
            return v
    if isinstance(v, (list, tuple)):
        return [norm(x) for x in v]
    return v


def rows_of(rows):
    return [[norm(x) for x in r] for r in rows]


def median(xs):
    return round(statistics.median(xs), 3) if xs else None


# --- DuckDB, in a process of its own so its memory is its own -------------

def duck_worker(data, out):
    import duckdb
    db = os.path.join(os.path.dirname(out), "sam.duckdb")
    if os.path.exists(db):
        os.remove(db)
    con = duckdb.connect(db)
    t = time.perf_counter()
    con.execute(f"CREATE TABLE trips AS SELECT * FROM read_parquet('{data}/trips.parquet')")
    con.execute("CHECKPOINT")
    trips_s = time.perf_counter() - t
    con.execute(f"CREATE TABLE zones AS SELECT * FROM read_parquet('{data}/zones.parquet')")
    lite = sqlite3.connect(os.path.join(data, "music.sqlite"))
    for (name,) in lite.execute("SELECT name FROM sqlite_master WHERE type = 'table'").fetchall():
        cols = lite.execute(f"PRAGMA table_info({name})").fetchall()
        con.execute(f"CREATE TABLE {name} (" + ", ".join(f"{c[1]} {SQLITE_TYPES[c[2]].replace('DOUBLE PRECISION', 'DOUBLE')}" for c in cols) + ")")
        con.executemany(f"INSERT INTO {name} VALUES ({', '.join('?' * len(cols))})", lite.execute(f"SELECT * FROM {name}").fetchall())
    con.execute("CHECKPOINT")
    load_s = time.perf_counter() - t
    res = {}
    for q in Q:
        sql = q["duck"] or q["sql"]
        try:
            rows = con.execute(sql).fetchall()
            ms = common.timed(lambda: con.execute(sql).fetchall(), REPS)
            res[q["id"]] = {"rows": rows_of(rows), "ms": median(ms), "error": None}
        except Exception as e:  # noqa: BLE001 - an error is a result
            res[q["id"]] = {"rows": None, "ms": None, "error": str(e).split("\n")[0][:300]}
    con.close()
    json.dump({"version": duckdb.__version__, "load_s": round(load_s, 3), "trips_load_s": round(trips_s, 3), "disk_bytes": os.path.getsize(db),
               "peak_rss_kb": resource.getrusage(resource.RUSAGE_SELF).ru_maxrss, "queries": res},
              open(out, "w"), default=str)


def run_duckdb(data, work):
    out = os.path.join(work, "duck.json")
    subprocess.run([sys.executable, os.path.abspath(__file__), "duck-worker", data, out], check=True)
    return json.load(open(out))


# --- ChloeDB: imported with the CLI as Sam did, then served -----------------

def run_chloedb(data, work):
    folder = os.path.join(work, "chloedb")
    os.makedirs(folder, exist_ok=True)
    db = os.path.join(folder, "db.chloedb")
    for f in os.listdir(folder):
        os.remove(os.path.join(folder, f))
    b = common.server_bin()
    t = time.perf_counter()
    subprocess.run([b, "import", "--db", db, "--table", "trips", "--key", "trip_id", f"{data}/trips.parquet"], check=True, capture_output=True)
    trips_s = time.perf_counter() - t
    subprocess.run([b, "import", "--db", db, "--table", "zones", "--key", "LocationID", f"{data}/zones.parquet"], check=True, capture_output=True)
    subprocess.run([b, "import", "--db", db, f"{data}/music.sqlite"], check=True, capture_output=True)
    load_s = time.perf_counter() - t
    ch = common.Chloedb(folder)
    res = {}
    for q in Q:
        r = ch.sql(q["sql"])
        if "error" in r:
            res[q["id"]] = {"rows": None, "ms": None, "wall_ms": None, "error": r["error"][:1000]}
            continue
        took, wall = [], []
        for _ in range(REPS):
            t = time.perf_counter()
            took.append(ch.sql(q["sql"]).get("took_ms") or 0)
            wall.append((time.perf_counter() - t) * 1000)
        res[q["id"]] = {"rows": rows_of(r["rows"]), "ms": median(took), "wall_ms": median(wall), "error": None,
                        "truncated": r.get("truncated") or r.get("next") is not None}
    hwm = 0
    with open(f"/proc/{ch.proc.pid}/status") as f:
        for line in f:
            if line.startswith("VmHWM:"):
                hwm = int(line.split()[1])
    ch.stop()
    return {"version": ch.version, "load_s": round(load_s, 3), "trips_load_s": round(trips_s, 3), "disk_bytes": ch.disk_bytes(), "peak_rss_kb": hwm, "queries": res}


# --- Postgres: the same SQL (DuckDB's form, for its timestamp literals) -----

def run_postgres(data, work):
    import duckdb
    pg = common.Postgres(os.path.join(work, "pg"))
    c = pg.conn
    duck = duckdb.connect()
    t = time.perf_counter()
    for name, cols in [("trips", None), ("zones", None)]:
        rel = duck.execute(f"SELECT * FROM read_parquet('{data}/{name}.parquet')")
        desc = [(d[0], {"BIGINT": "BIGINT", "TIMESTAMP": "TIMESTAMP", "VARCHAR": "TEXT", "DOUBLE": "DOUBLE PRECISION"}[str(d[1])]) for d in rel.description]
        c.execute(f"CREATE TABLE {name} (" + ", ".join(f"{n} {ty}" for n, ty in desc) + ")")
        with c.cursor() as cur, cur.copy(f"COPY {name} FROM STDIN") as cp:
            for row in rel.fetchall():
                cp.write_row(row)
        if name == "trips":
            trips_s = time.perf_counter() - t
    lite = sqlite3.connect(os.path.join(data, "music.sqlite"))
    for (name,) in lite.execute("SELECT name FROM sqlite_master WHERE type = 'table'").fetchall():
        cols = lite.execute(f"PRAGMA table_info({name})").fetchall()
        c.execute(f"CREATE TABLE {name} (" + ", ".join(f"{col[1]} {SQLITE_TYPES[col[2]]}" for col in cols) + ")")
        with c.cursor() as cur, cur.copy(f"COPY {name} FROM STDIN") as cp:
            for row in lite.execute(f"SELECT * FROM {name}").fetchall():
                cp.write_row(row)
    c.execute("VACUUM ANALYZE")
    load_s = time.perf_counter() - t
    disk = c.execute("SELECT SUM(pg_total_relation_size(c.oid)) FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace "
                     "WHERE n.nspname = 'public' AND c.relkind = 'r'").fetchone()[0]
    res = {}
    for q in Q:
        sql = (q["duck"] or q["sql"]).replace("%", "%%")
        try:
            rows = c.execute(sql).fetchall()
            ms = common.timed(lambda: c.execute(sql).fetchall(), REPS)
            res[q["id"]] = {"rows": rows_of(rows), "ms": median(ms), "error": None}
        except Exception as e:  # noqa: BLE001
            res[q["id"]] = {"rows": None, "ms": None, "error": str(e).split("\n")[0][:300]}
    mem = pg.memory_kb()
    pg.stop()
    return {"version": pg.version.split(" on ")[0], "load_s": round(load_s, 3), "trips_load_s": round(trips_s, 3), "disk_bytes": int(disk), "pss_kb": mem, "queries": res}


def verdict(a, b, ordered):
    if a["error"] and b["error"]:
        return "both refuse"
    if a["error"]:
        return "refused"
    if b["error"]:
        return "other refuses"
    if a["rows"] == b["rows"]:
        return "same"
    ka = sorted(json.dumps(r, default=str) for r in a["rows"])
    kb = sorted(json.dumps(r, default=str) for r in b["rows"])
    if ka == kb:
        return "same" if not ordered else "same rows, other order"
    return "different"


def main():
    trips = int(sys.argv[1]) if len(sys.argv) > 1 else 205064
    work = os.environ.get("H2H_WORK", f"/tmp/h2h-sam/{trips}")
    data = os.path.join(work, "data")
    if not os.path.exists(os.path.join(data, "trips.parquet")):
        subprocess.run([sys.executable, os.path.join(HERE, "gen.py"), data, str(trips)], check=True)
    out = {"machine": common.machine(), "trips": trips, "reps": REPS, "at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
           "parquet_bytes": os.path.getsize(os.path.join(data, "trips.parquet"))}
    out["duckdb"] = run_duckdb(data, work)
    out["chloedb"] = run_chloedb(data, work)
    out["postgres"] = run_postgres(data, work)
    per = []
    for q in Q:
        d, c, p = out["duckdb"]["queries"][q["id"]], out["chloedb"]["queries"][q["id"]], out["postgres"]["queries"][q["id"]]
        row = {"id": q["id"], "ordered": q["ordered"], "note": q["note"],
               "chloedb_vs_duckdb": verdict(c, d, q["ordered"]), "postgres_vs_duckdb": verdict(p, d, q["ordered"]),
               "rows": len(d["rows"]) if d["rows"] is not None else None,
               "chloedb_ms": c["ms"], "chloedb_wall_ms": c.get("wall_ms"), "duckdb_ms": d["ms"], "postgres_ms": p["ms"],
               "chloedb_error": c["error"], "duckdb_error": d["error"], "postgres_error": p["error"]}
        if row["chloedb_vs_duckdb"] == "different":
            cs = {json.dumps(r, default=str) for r in c["rows"]}
            ds = {json.dumps(r, default=str) for r in d["rows"]}
            row["diff"] = {"chloedb_only": sorted(cs - ds)[:3], "duckdb_only": sorted(ds - cs)[:3], "chloedb_n": len(c["rows"]), "duckdb_n": len(d["rows"])}
        per.append(row)
        print(f"{q['id']:28s} {row['chloedb_vs_duckdb']:24s} pg:{row['postgres_vs_duckdb']:24s} ch={c['ms']} duck={d['ms']} pg={p['ms']}", flush=True)
    for side in ("duckdb", "chloedb", "postgres"):
        out[side].pop("queries")
    out["queries"] = per
    common.write_json(os.path.join(HERE, "raw", f"results-{trips}.json"), out)


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "duck-worker":
        duck_worker(sys.argv[2], sys.argv[3])
    else:
        main()
