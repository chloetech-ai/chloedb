"""Grace's Monday numbers: built by clicking in ChloeDB's command center,
checked against SQL.

Grace, a support operations lead in the persona study, builds her Monday
report from a Zendesk export in a spreadsheet: tickets each week by
priority, the median time to solve, the top tags, satisfaction per agent.
Her adoption answer: she adopts when those numbers come by clicking, are
saved as a team view that opens on a phone, and the counts are true.

This writes 20,000 made-up tickets of an export's shape (tickets.py), loads
them on the Import page and builds each number in the Query builder, in
Chromium driven by Playwright (click.mjs: every click, pick and typed value
counted), then computes the same numbers two other ways: SQL written by hand
on the same ChloeDB, and DuckDB over the CSV file, the reference. Each
number the clicks read is compared with both.

    python3 run.py

Needs node with Playwright and Chromium, `pip install duckdb`, and a
chloedb-server build. Raw numbers go to raw/results.json, screenshots to raw/.
"""

import json
import os
import shutil
import statistics
import subprocess
import sys
import time
import urllib.request

import duckdb

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import common  # noqa: E402
import tickets  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
WORK = os.environ.get("H2H_WORK", "/tmp/h2h-grace")
RAW = os.path.join(HERE, "raw")
REPS = 7


def call(url, path, body=None):
    req = urllib.request.Request(url + path, data=None if body is None else json.dumps(body).encode(),
                                 method="POST" if body is not None else "GET")
    req.add_header("content-type", "application/json")
    with urllib.request.urlopen(req, timeout=120) as r:
        return json.loads(r.read())


def num(s):
    return float(s.replace(",", ""))


# The numbers, as SQL a colleague writes by hand (Sam, in Grace's story), on ChloeDB and on DuckDB.
CHLOEDB_SQL = {
    "per_week": "SELECT date_trunc('week', created_at) AS week, priority, COUNT(*) AS n FROM tickets GROUP BY 1, 2 ORDER BY 1, 2",
    "median_hours": "SELECT date_trunc('week', created_at) AS week, MEDIAN(date_diff('second', created_at, solved_at) / 3600.0) AS h "
                    "FROM tickets GROUP BY 1 ORDER BY 1",
    "top_tags": "SELECT tags, COUNT(*) AS n FROM tickets GROUP BY tags ORDER BY n DESC, tags LIMIT 10",
    "satisfaction": "SELECT assignee, 100.0 * SUM(CASE WHEN satisfaction = 'good' THEN 1 ELSE 0 END) / COUNT(*) AS pct "
                    "FROM tickets WHERE satisfaction IN ('good', 'bad') GROUP BY assignee ORDER BY assignee",
}
DUCKDB_SQL = {
    "per_week": "SELECT strftime(date_trunc('week', created_at), '%Y-%m-%dT%H:%M:%SZ') AS week, priority, COUNT(*) AS n "
                "FROM tickets GROUP BY 1, 2 ORDER BY 1, 2",
    "median_hours": "SELECT strftime(date_trunc('week', created_at), '%Y-%m-%dT%H:%M:%SZ') AS week, "
                    "MEDIAN(epoch(solved_at - created_at) / 3600.0) AS h FROM tickets GROUP BY 1 ORDER BY 1",
    "top_tags": "SELECT tag, COUNT(*) AS n FROM (SELECT unnest(string_split(tags, ' ')) AS tag FROM tickets) "
                "GROUP BY tag ORDER BY n DESC, tag LIMIT 10",
    "satisfaction": "SELECT assignee, 100.0 * SUM(CASE WHEN satisfaction = 'good' THEN 1 ELSE 0 END) / COUNT(*) AS pct "
                    "FROM tickets WHERE satisfaction IN ('good', 'bad') GROUP BY assignee ORDER BY assignee",
}


def chloedb_rows(url, sql):
    r = call(url, "/v1/query", {"sql": sql})
    return [list(x) for x in r["rows"]]


def timed(fn):
    ms = []
    for _ in range(REPS):
        t = time.perf_counter()
        fn()
        ms.append((time.perf_counter() - t) * 1000)
    return round(statistics.median(ms), 2)


def keyed(name, rows):
    """Each number by what it is of: {key: value}."""
    if name == "per_week":
        return {(str(r[0])[:10], r[1]): float(r[2]) for r in rows}
    if name == "median_hours":
        return {str(r[0])[:10]: (None if r[1] is None else float(r[1])) for r in rows}
    if name == "top_tags":
        return {r[0]: float(r[1]) for r in rows}
    return {r[0]: float(r[1]) for r in rows}


def clicked_keyed(name, ans):
    h, rows = ans["head"], ans["rows"]
    if name == "per_week":
        return {(r[0][:10], r[1]): num(r[2]) for r in rows}
    if name == "median_hours":
        return {r[0][:10]: num(r[1]) for r in rows}
    if name == "top_tags":
        return {r[0]: num(r[1]) for r in rows}
    # the share of each assignee's ratings that are good: the builder shows one row per assignee and rating
    i_s, i_p = h.index("satisfaction"), len(h) - 1
    return {r[0]: num(r[i_p]) for r in rows if r[i_s] == "good"}


def compare(name, got, ref, tol):
    keys = sorted(set(got) | set(ref), key=str)
    worst = 0.0
    missing = [str(k) for k in keys if k not in got]
    extra = [str(k) for k in keys if k not in ref]
    for k in keys:
        if k in got and k in ref and got[k] is not None and ref[k] is not None:
            worst = max(worst, abs(got[k] - ref[k]))
    same_order = list(got) == list(ref) if name == "top_tags" else None
    return {"numbers": len(ref), "missing": missing, "extra": extra, "largest_difference": round(worst, 6),
            "within": tol, "agree": not missing and not extra and worst <= tol, **({"same_order": same_order} if same_order is not None else {})}


# How close each kind of number must be: counts exactly; the median within a minute (the builder measures whole
# minutes, as Grace's sheet does); a share within its shown rounding (one decimal).
TOLERANCE = {"per_week": 0, "median_hours": 1 / 60, "top_tags": 0, "satisfaction": 0.05}


def main():
    if os.path.exists(WORK):
        shutil.rmtree(WORK)
    os.makedirs(WORK)
    os.makedirs(RAW, exist_ok=True)
    csv_path = os.path.join(WORK, "tickets.csv")
    rows = tickets.rows()
    tickets.write_csv(csv_path, rows)
    res = {"at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "machine": common.machine(), "tickets": len(rows),
           "csv_bytes": os.path.getsize(csv_path), "duckdb": duckdb.__version__}

    # ChloeDB: an open server, as Sam starts it for her, its answer cache off
    port = common.free_port()
    url = f"http://127.0.0.1:{port}"
    log = open(os.path.join(WORK, "server.log"), "w")
    srv = subprocess.Popen([common.server_bin(), "--db", os.path.join(WORK, "db.chloedb"), "--listen", f"127.0.0.1:{port}",
                            "--grpc", "off", "--start-apps", "off", "--answer-cache-mb", "0"], stdout=log, stderr=subprocess.STDOUT)
    try:
        for _ in range(300):
            try:
                call(url, "/v1/health")
                break
            except OSError:
                time.sleep(0.1)
        res["chloedb_version"] = subprocess.run([common.server_bin(), "--version"], capture_output=True, text=True).stdout.strip()
        t = time.perf_counter()
        subprocess.run(["node", os.path.join(HERE, "click.mjs"), url, csv_path, RAW], check=True)
        res["browser_s"] = round(time.perf_counter() - t, 1)
        clicks = json.load(open(os.path.join(RAW, "clicks.json")))
        res["clicks"] = {k: v for k, v in clicks.items() if k != "numbers"}
        res["builder_sql"] = {k: v["sql"] for k, v in clicks["numbers"].items()}
        hand = {k: chloedb_rows(url, s) for k, s in CHLOEDB_SQL.items()}
        res["chloedb_sql_ms"] = {k: timed(lambda s=s: call(url, "/v1/query", {"sql": s})) for k, s in CHLOEDB_SQL.items()}
    finally:
        srv.terminate()
        srv.wait(30)
        log.close()

    # DuckDB over the same file: the reference
    con = duckdb.connect()
    con.execute(f"CREATE TABLE tickets AS SELECT * FROM read_csv('{csv_path}', header = true, "
                "types = {'id': 'VARCHAR', 'created_at': 'TIMESTAMP', 'solved_at': 'TIMESTAMP'})")
    ref = {k: [list(r) for r in con.execute(s).fetchall()] for k, s in DUCKDB_SQL.items()}
    res["duckdb_ms"] = {k: timed(lambda s=s: con.execute(s).fetchall()) for k, s in DUCKDB_SQL.items()}

    res["numbers"] = {}
    for name in CHLOEDB_SQL:
        r = keyed(name, ref[name])
        got_click = clicked_keyed(name, clicks["numbers"][name])
        got_hand = keyed(name, hand[name])
        res["numbers"][name] = {
            "clicked_vs_duckdb": compare(name, got_click, r, TOLERANCE[name]),
            "hand_sql_vs_duckdb": compare(name, got_hand, r, 1e-9),
            "clicked": clicks["numbers"][name]["rows"],
            "duckdb": [[str(x) if not isinstance(x, (int, float)) else x for x in row] for row in ref[name]],
        }
        print(name, json.dumps(res["numbers"][name]["clicked_vs_duckdb"]), "hand:", res["numbers"][name]["hand_sql_vs_duckdb"]["agree"])
    common.write_json(os.path.join(RAW, "results.json"), res)
    print("wrote raw/results.json")


if __name__ == "__main__":
    main()
