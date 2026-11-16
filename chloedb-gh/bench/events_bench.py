"""How fast the explorer's cluster answers over every public GitHub event.

Statements of four kinds, each asked of the coordinator as the app asks
them (HTTP /v1/query):

- the whole table: counts and sums over every event since 2011;
- a window of time: one month or one day of every repo;
- one repo and one person: their events among all of them;
- the app's pages, end to end (several statements each).

Each is run once (the first run, on whatever the machines' page caches
hold), then --runs more times; the report gives the server's own time
(`took_ms`, measured where the request arrived, so the network between
this machine and the cluster is not in it) of the first run and the
median and the slowest of the others. Run it with the servers' answer
cache off (`--answer-cache-mb 0` on the shards and the coordinator),
or every run after the first is a lookup; a run the answer says came
from the cache is marked.

    CHLOEDB_KEY=... python3 bench/events_bench.py run --url http://127.0.0.1:7700 \\
        [--app http://127.0.0.1:7880] [--runs 5] --machine "4 x DO s-4vcpu-8gb-amd"
    python3 bench/events_bench.py report          # results/events.md from results/events.json

CHLOEDB_GH_TOKEN is the app's token, for --app. Standard library only.
"""

import argparse
import json
import os
import statistics
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
RESULTS = os.path.join(HERE, "results")
YEAR, MONTH, DAY = 31_556_952, 2_629_746, 86_400
WORK = "('push', 'pr_opened', 'pr_merged', 'pr_closed', 'issue_opened', 'issue_closed', 'comment', 'review', 'review_comment', 'commit_comment', 'release', 'create_branch', 'create_tag')"
MAR_2020, APR_2020 = 1_583_020_800, 1_585_699_200
JUN_18_2024 = 1_718_668_800
# 1 January of 2011 to 2027, in seconds.
_JAN1 = [1293840000, 1325376000, 1356998400, 1388534400, 1420070400, 1451606400, 1483228800, 1514764800, 1546300800, 1577836800, 1609459200, 1640995200, 1672531200, 1704067200, 1735689600, 1767225600, 1798761600]
YEARS = list(zip(_JAN1, _JAN1[1:]))

REPOS = ["torvalds/linux", "kubernetes/kubernetes", "facebook/react", "microsoft/vscode"]
PEOPLE = ["torvalds", "sindresorhus", "dependabot[bot]"]


def statements(ids):
    """(group, name, sql, params) of every statement asked."""
    r, p = ids["repos"], ids["people"]
    out = [
        ("every event", "count every event", "SELECT COUNT(*) AS n FROM gh_events", {}),
        ("every event", "first and last event", "SELECT MIN(t) AS first, MAX(t) AS last FROM gh_events", {}),
        ("every event", "events by kind", "SELECT kind, COUNT(*) AS n FROM gh_events GROUP BY kind ORDER BY n DESC", {}),
        ("every event", "stars ever", "SELECT COUNT(*) AS n FROM gh_events WHERE kind = 'star'", {}),
        ("every event", "commits pushed ever", "SELECT SUM(n) AS commits, COUNT(*) AS pushes FROM gh_events WHERE kind = 'push'", {}),
        ("every event", "events by year (a year computed from each row's INT time)", f"SELECT t - t % {YEAR} AS y, COUNT(*) AS n FROM gh_events GROUP BY y ORDER BY y", {}),
        ("every event", "events by year (one range count a year, 16 statements)", [f"SELECT COUNT(*) AS n FROM gh_events WHERE t >= {y0} AND t < {y1}" for y0, y1 in YEARS], {}),
        ("every event", "kinds of events in 2024 (a range of times)", "SELECT kind, COUNT(*) AS n FROM gh_events WHERE t >= :a AND t < :b GROUP BY kind ORDER BY n DESC", {"a": YEARS[13][0], "b": YEARS[13][1]}),
        ("monthly sums", "pushes, commits and stars of every month (gh_repo_months)", "SELECT month, SUM(pushes) AS pushes, SUM(commits) AS commits, SUM(stars) AS stars, COUNT(*) AS repos FROM gh_repo_months GROUP BY month ORDER BY month", {}),
        ("a window", "busiest repos of March 2020", "SELECT repo, COUNT(*) AS n FROM gh_events WHERE t >= :a AND t < :b GROUP BY repo ORDER BY n DESC LIMIT 20", {"a": MAR_2020, "b": APR_2020}),
        ("a window", "events of each hour of 18 June 2024", "SELECT t - t % 3600 AS h, COUNT(*) AS n FROM gh_events WHERE t >= :a AND t < :b GROUP BY h ORDER BY h", {"a": JUN_18_2024, "b": JUN_18_2024 + DAY}),
        ("a window", "most starred repos of 18 June 2024", "SELECT repo, COUNT(*) AS n FROM gh_events WHERE kind = 'star' AND t >= :a AND t < :b GROUP BY repo ORDER BY n DESC LIMIT 20", {"a": JUN_18_2024, "b": JUN_18_2024 + DAY}),
    ]
    for name, id in r.items():
        out += [
            ("one repo", f"{name}: work ever (count, first, last)", f"SELECT MAX(t) AS last, MIN(t) AS first, COUNT(*) AS n FROM gh_events WHERE repo = :id AND kind IN {WORK}", {"id": id}),
            ("one repo", f"{name}: who did the work ever", f"SELECT actor, COUNT(*) AS n, MIN(t) AS first, MAX(t) AS last FROM gh_events WHERE repo = :id AND kind IN {WORK} GROUP BY actor ORDER BY n DESC LIMIT 30", {"id": id}),
            ("one repo", f"{name}: stars month by month", f"SELECT t - t % {MONTH} AS m, COUNT(*) AS n FROM gh_events WHERE repo = :id AND kind = 'star' GROUP BY m ORDER BY m", {"id": id}),
        ]
    for login, id in p.items():
        out += [
            ("one person", f"{login}: what they did", "SELECT kind, COUNT(*) AS n FROM gh_events WHERE actor = :a GROUP BY kind ORDER BY n DESC LIMIT 30", {"a": id}),
            ("one person", f"{login}: year by year", f"SELECT t - t % {YEAR} AS y, COUNT(*) AS n FROM gh_events WHERE actor = :a AND kind IN {WORK} GROUP BY y ORDER BY y LIMIT 40", {"a": id}),
        ]
        if not login.endswith("[bot]"):
            out.append(("one person", f"{login}: where they work", f"SELECT repo, COUNT(*) AS n, MIN(t) AS first, MAX(t) AS last FROM gh_events WHERE actor = :a AND kind IN {WORK} GROUP BY repo ORDER BY n DESC LIMIT 24", {"a": id}))
    return out


PAGES = [
    ("a repo: torvalds/linux", "/v1/repo/torvalds/linux"),
    ("a repo: facebook/react", "/v1/repo/facebook/react"),
    ("a person: torvalds", "/v1/person/torvalds"),
    ("search: terminal file manager", "/v1/search?q=terminal+file+manager"),
    ("search: self-hosted photo backup", "/v1/search?q=self-hosted+photo+backup"),
    ("rising", "/v1/rising"),
    ("trends", "/v1/trends"),
    ("weekly", "/v1/weekly"),
    ("ask: who works on rust-lang/rust", "/v1/ask?q=" + urllib.parse.quote("who works on rust-lang/rust")),
    ("ask: languages by stars in 2015", "/v1/ask?q=" + urllib.parse.quote("languages by stars in 2015")),
    ("for you: sindresorhus", "/v1/foryou?login=sindresorhus"),
]


def call(url, token, body=None, timeout=900):
    req = urllib.request.Request(url, data=None if body is None else json.dumps(body).encode(), method="GET" if body is None else "POST")
    req.add_header("content-type", "application/json")
    req.add_header("x-chloedb-client", "chloedb-gh-bench")
    if token:
        req.add_header("authorization", "Bearer " + token)
    t0 = time.monotonic()
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            j = json.loads(r.read())
    except urllib.error.HTTPError as e:
        return {"error": f"{e.code}: {e.read()[:300].decode(errors='replace')}"}, (time.monotonic() - t0) * 1e3
    return j, (time.monotonic() - t0) * 1e3


def sql(url, key, q, params=None):
    """One statement, or a list of them run one after the other (their
    rows together, their times added)."""
    if isinstance(q, list):
        out = [sql(url, key, one, params) for one in q]
        return [r for o in out for r in o[0]], sum(o[1] for o in out), sum(o[2] for o in out), any(o[3] for o in out)
    j, wall = call(url + "/v1/query", key, {"sql": q, "params": params or {}, "objects": True, "steps": True, "timeout_ms": 1_800_000}, timeout=1900)
    if "error" in j:
        raise RuntimeError(f"{q[:80]}: {j['error']}")
    cached = "answer cache" in json.dumps(j.get("stats", {}))
    return j.get("rows", []), j.get("took_ms", wall), wall, cached


def resolve(url, key):
    ids = {"repos": {}, "people": {}}
    for n in REPOS:
        rows, *_ = sql(url, key, "SELECT repo FROM gh_repo_names WHERE lname = :n LIMIT 1", {"n": n.lower()})
        if rows:
            ids["repos"][n] = rows[0]["repo"]
    for l in PEOPLE:
        rows, *_ = sql(url, key, "SELECT actor FROM gh_actors WHERE llogin = :l LIMIT 1", {"l": l.lower()})
        if rows:
            ids["people"][l] = rows[0]["actor"]
    return ids


def summary(runs):
    ms = [r["ms"] for r in runs[1:]] or [runs[0]["ms"]]
    return {"first_ms": runs[0]["ms"], "median_ms": statistics.median(ms), "max_ms": max(ms), "cached": sum(r.get("cached", False) for r in runs)}


def run(a):
    key, app_token = os.environ.get("CHLOEDB_KEY"), os.environ.get("CHLOEDB_GH_TOKEN")
    out = {"machine": a.machine, "started": time.strftime("%Y-%m-%d %H:%M:%SZ", time.gmtime()), "runs": a.runs}
    ids = resolve(a.url, key)
    out["ids"] = ids
    print("ids:", ids, file=sys.stderr)
    rows, *_ = sql(a.url, key, "SELECT COUNT(*) AS n FROM gh_events")
    out["events"] = rows[0]["n"]
    print(f"events: {out['events']:,}", file=sys.stderr)
    out["statements"] = []
    for group, name, q, params in statements(ids):
        runs = []
        for i in range(a.runs + 1):
            if i == 2 and runs[0]["ms"] > 30_000:
                break  # a statement of minutes runs twice
            try:
                rows, took, wall, cached = sql(a.url, key, q, params)
                runs.append({"ms": took, "wall_ms": wall, "rows": len(rows), "cached": cached})
            except Exception as e:
                runs.append({"ms": float("nan"), "error": str(e)[:300]})
                break
        s = {"group": group, "name": name, "sql": q, "params": params, "runs": runs, "answer": rows[:3] if runs and "error" not in runs[-1] else None}
        if all("error" not in r for r in runs):
            s.update(summary(runs))
        out["statements"].append(s)
        print(f"{group:12} {name:52} " + (f"first {s['first_ms']:9.1f}  median {s['median_ms']:9.1f} ms" if "median_ms" in s else runs[-1].get("error", "")), file=sys.stderr)
    out["pages"] = []
    if a.app:
        for name, path in PAGES:
            runs = []
            for i in range(a.runs + 1):
                j, wall = call(a.app + path, app_token)
                if "error" in j:
                    runs.append({"ms": float("nan"), "error": j["error"]})
                    break
                runs.append({"ms": j.get("took_ms", wall), "wall_ms": wall, "statements": len(j.get("queries", [])), "refused": sum(1 for q in j.get("queries", []) if q.get("note"))})
            p = {"name": name, "path": path, "runs": runs}
            if all("error" not in r for r in runs):
                p.update(summary(runs))
            out["pages"].append(p)
            print(f"{'page':12} {name:52} " + (f"first {p['first_ms']:9.1f}  median {p['median_ms']:9.1f} ms" if "median_ms" in p else runs[-1].get("error", "")), file=sys.stderr)
    out["finished"] = time.strftime("%Y-%m-%d %H:%M:%SZ", time.gmtime())
    os.makedirs(RESULTS, exist_ok=True)
    with open(os.path.join(RESULTS, "events.json"), "w") as f:
        json.dump(out, f, indent=1)
    report(a)


def fmt(ms):
    if ms != ms:
        return "failed"
    return f"{ms / 1000:.2f} s" if ms >= 1000 else f"{ms:.0f} ms" if ms >= 10 else f"{ms:.1f} ms"


def report(a):
    j = json.load(open(os.path.join(RESULTS, "events.json")))
    lines = [
        "# Every public GitHub event: how fast the cluster answers",
        "",
        f"{j['events']:,} events in `gh_events`, on {j['machine']}; measured {j['started']} to {j['finished']} by `bench/events_bench.py`.",
        f"Each statement ran once, then {j['runs']} more times (one more when the first took over 30 s); times are the server's own (`took_ms`), with the answer cache off.",
        "",
        "| kind | statement | first run | median | slowest | rows |",
        "|---|---|---:|---:|---:|---:|",
    ]
    for s in j["statements"]:
        rows = s["runs"][-1].get("rows", "")
        c = " (cache)" if s.get("cached") else ""
        if "median_ms" in s:
            lines.append(f"| {s['group']} | {s['name']} | {fmt(s['first_ms'])} | {fmt(s['median_ms'])}{c} | {fmt(s['max_ms'])} | {rows} |")
        else:
            lines.append(f"| {s['group']} | {s['name']} | failed: {s['runs'][-1].get('error', '')[:120]} | | | |")
    if j.get("pages"):
        lines += ["", "The app's pages, end to end (all their statements):", "", "| page | statements | first run | median | slowest |", "|---|---:|---:|---:|---:|"]
        for p in j["pages"]:
            if "median_ms" in p:
                lines.append(f"| {p['name']} | {p['runs'][-1].get('statements', '')} | {fmt(p['first_ms'])} | {fmt(p['median_ms'])} | {fmt(p['max_ms'])} |")
            else:
                lines.append(f"| {p['name']} | | failed: {p['runs'][-1].get('error', '')[:120]} | | |")
    lines += ["", "The statements, with their parameters, are in `results/events.json`."]
    with open(os.path.join(RESULTS, "events.md"), "w") as f:
        f.write("\n".join(lines) + "\n")
    print("\n".join(lines))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run")
    r.add_argument("--url", required=True)
    r.add_argument("--app")
    r.add_argument("--runs", type=int, default=5)
    r.add_argument("--machine", required=True)
    sub.add_parser("report")
    a = ap.parse_args()
    run(a) if a.cmd == "run" else report(a)


if __name__ == "__main__":
    main()
