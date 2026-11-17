"""The explorer's search against GitHub's own repository search.

Each query in queries.tsv is a description of a kind of project, with the
repos that answer it. Both engines get the query as it is written, and each
keeps its own order: GitHub's search API with its default "best match"
order, and the explorer's /v1/search. A query scores when an answer is in
the first ten, and its reciprocal rank is 1/position of the first answer
(MRR@10). Repos are compared by their GitHub id, so a repo renamed since is
still the same repo.

    python3 bench/search_vs_github.py resolve              # the answers' ids, from GitHub
    python3 bench/search_vs_github.py github               # GitHub's top ten for each query
    python3 bench/search_vs_github.py ours --url URL       # the explorer's (CHLOEDB_GH_TOKEN for its token)
    python3 bench/search_vs_github.py score                # the table, into results/report.md

GitHub's answers are kept in results/github.json and a query already there
is not asked again. GitHub allows 30 searches a minute with a token
(GITHUB_TOKEN, or --gh to ask through the gh command line) and 10 without:
the script waits between calls to stay under that. Standard library only.
"""

import argparse
import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
RESULTS = os.path.join(HERE, "results")


def load_queries(path):
    out = []
    for line in open(path, encoding="utf-8"):
        line = line.rstrip("\n")
        if not line.strip() or line.startswith("#"):
            continue
        parts = line.split("\t")
        out.append({"q": parts[0], "answers": [a for a in parts[1].split(",") if a], "tuned": len(parts) > 2 and parts[2] == "tuned"})
    return out


def read_json(name, default):
    p = os.path.join(RESULTS, name)
    return json.load(open(p)) if os.path.exists(p) else default


def write_json(name, value):
    os.makedirs(RESULTS, exist_ok=True)
    p = os.path.join(RESULTS, name)
    with open(p + ".tmp", "w") as f:
        json.dump(value, f, indent=1)
    os.replace(p + ".tmp", p)


class GitHub:
    def __init__(self, use_gh):
        self.use_gh = use_gh
        self.token = os.environ.get("GITHUB_TOKEN")
        authed = use_gh or bool(self.token)
        # Under the limit: 30 a minute with a token, 10 without.
        self.gap = 2.5 if authed else 6.5
        self.last = 0.0

    def search(self, q, per_page=10):
        wait = self.last + self.gap - time.time()
        if wait > 0:
            time.sleep(wait)
        self.last = time.time()
        for attempt in range(6):
            try:
                if self.use_gh:
                    r = subprocess.run(["gh", "api", "-X", "GET", "search/repositories", "-f", f"q={q}", "-f", f"per_page={per_page}"], capture_output=True, text=True)
                    if r.returncode != 0 and '"status":"422"' in r.stdout:
                        return {"items": [], "total_count": 0, "invalid": True}
                    r.check_returncode()
                    return json.loads(r.stdout)
                url = "https://api.github.com/search/repositories?" + urllib.parse.urlencode({"q": q, "per_page": per_page})
                headers = {"Accept": "application/vnd.github+json", "User-Agent": "chloedb-gh-bench"}
                if self.token:
                    headers["Authorization"] = f"Bearer {self.token}"
                try:
                    with urllib.request.urlopen(urllib.request.Request(url, headers=headers), timeout=60) as r:
                        return json.load(r)
                except urllib.error.HTTPError as e:
                    if e.code == 422:  # a repo: that does not exist
                        return {"items": [], "total_count": 0, "invalid": True}
                    raise
            except Exception as e:  # noqa: BLE001 - a limit or a network failure: wait and ask again
                pause = 30 * (attempt + 1)
                print(f"github: {q!r}: {str(e)[:200]}; again in {pause} s", file=sys.stderr, flush=True)
                time.sleep(pause)
        raise RuntimeError(f"GitHub did not answer {q!r}")


def resolve(a):
    queries = load_queries(a.queries)
    ids = read_json("answers.json", {})
    gh = GitHub(a.gh)
    names = sorted({n for q in queries for n in q["answers"]} - set(ids))
    print(f"resolve: {len(names)} names to look up", flush=True)
    for n in names:
        items = gh.search(f"repo:{n}", per_page=1).get("items", [])
        if items:
            ids[n] = {"id": items[0]["id"], "full_name": items[0]["full_name"], "stars": items[0]["stargazers_count"]}
        else:
            print(f"resolve: {n} not found (renamed or gone?)", flush=True)
        write_json("answers.json", ids)
    print(f"resolve: {len(ids)} names known", flush=True)


def github(a):
    queries = load_queries(a.queries)
    got = read_json("github.json", {})
    gh = GitHub(a.gh)
    todo = [q for q in queries if q["q"] not in got]
    print(f"github: {len(todo)} queries to ask", flush=True)
    for i, q in enumerate(todo):
        t0 = time.time()
        d = gh.search(q["q"])
        got[q["q"]] = {"total": d.get("total_count"), "ms": round((time.time() - t0) * 1000), "top": [{"id": it["id"], "name": it["full_name"], "stars": it["stargazers_count"]} for it in d.get("items", [])[:10]]}
        write_json("github.json", got)
        if (i + 1) % 10 == 0:
            print(f"github: {i + 1}/{len(todo)}", flush=True)


def ours(a):
    queries = load_queries(a.queries)
    token = os.environ.get("CHLOEDB_GH_TOKEN", "")
    headers = {"Authorization": f"Bearer {token}"} if token else {}

    def get(path):
        req = urllib.request.Request(a.url.rstrip("/") + path, headers=headers)
        with urllib.request.urlopen(req, timeout=120) as r:
            return json.load(r)

    # Answers GitHub's search no longer finds by the name written (renamed
    # since): their ids from the names each repo had in the archive.
    ids = read_json("answers.json", {})
    for n in sorted({n for q in queries for n in q["answers"]} - set(ids)):
        try:
            d = get("/v1/repo/" + urllib.parse.quote(n))
        except urllib.error.HTTPError:
            d = {}
        if d.get("found"):
            r = d["repo"]
            ids[n] = {"id": r["repo"], "full_name": r["name"], "stars": r.get("stars"), "via": "the archive's names"}
            print(f"ours: {n} is {r['name']} (id {r['repo']})", flush=True)
        else:
            print(f"ours: {n} not in the archive either", flush=True)
    write_json("answers.json", ids)
    got = {}
    for q in queries:
        t0 = time.time()
        d = get("/v1/search?" + urllib.parse.urlencode({"q": q["q"]}))
        got[q["q"]] = {"total": d.get("total"), "ms": round((time.time() - t0) * 1000), "took_ms": d.get("took_ms"), "top": [{"id": it["repo"], "name": it["name"], "stars": it.get("stars")} for it in d.get("repos", [])[:10]]}
    write_json(a.out, got)
    print(f"ours: {len(got)} queries into results/{a.out}", flush=True)


def rank_of(top, answer_ids):
    for i, it in enumerate(top):
        if it["id"] in answer_ids:
            return i + 1
    return None


def score(a):
    queries = load_queries(a.queries)
    ids = read_json("answers.json", {})
    engines = {"GitHub search": read_json("github.json", {}), "Explorer": read_json(a.ours, {})}
    rows = []
    for q in queries:
        want = {ids[n]["id"] for n in q["answers"] if n in ids}
        row = {"q": q["q"], "tuned": q["tuned"], "answers": q["answers"]}
        for e, got in engines.items():
            r = got.get(q["q"])
            row[e] = None if r is None else {"rank": rank_of(r["top"], want), "top": r["top"][:3], "total": r["total"], "ms": r["ms"]}
        rows.append(row)

    def summary(sel):
        out = {}
        for e in engines:
            rs = [r[e] for r in sel if r[e] is not None]
            if not rs:
                continue
            n = len(rs)
            out[e] = {
                "n": n,
                "at1": sum(1 for r in rs if r["rank"] == 1) / n,
                "at10": sum(1 for r in rs if r["rank"]) / n,
                "mrr": sum(1 / r["rank"] for r in rs if r["rank"]) / n,
                "empty": sum(1 for r in rs if not r["top"]),
                "ms": sorted(r["ms"] for r in rs)[n // 2],
            }
        return out

    every, untuned = summary(rows), summary([r for r in rows if not r["tuned"]])
    lines = ["# The explorer's search against GitHub's", ""]
    lines.append(f"{len(rows)} searches written as a person looking for a kind of project would type them, each with the repos that answer it (bench/queries.tsv). A search scores when one of its answers is in the first ten. The explorer's time is the whole request to its API inside the cluster; GitHub's is over the internet and paced under its limit, so it is not compared.")
    lines.append("")
    lines.append("| | searches | answer first | answer in top 10 | MRR@10 | no results | median time |")
    lines.append("|---|---|---|---|---|---|---|")
    for label, s in (("all", every), ("without the six tuned on", untuned)):
        for e, v in s.items():
            ms = f"{v['ms']} ms" if e == "Explorer" else "not comparable"
            lines.append(f"| {e}, {label} | {v['n']} | {v['at1']:.0%} | {v['at10']:.0%} | {v['mrr']:.3f} | {v['empty']} | {ms} |")
    lines.append("")
    lines.append("| search | GitHub search | Explorer |")
    lines.append("|---|---|---|")
    for r in rows:
        cells = []
        for e in engines:
            v = r[e]
            if v is None:
                cells.append("not run")
                continue
            first = ", ".join(t["name"] for t in v["top"]) or "nothing"
            cells.append(f"{'#' + str(v['rank']) if v['rank'] else 'missed'}: {first}")
        lines.append(f"| {r['q']}{' (tuned)' if r['tuned'] else ''} | {cells[0]} | {cells[1]} |")
    os.makedirs(RESULTS, exist_ok=True)
    open(os.path.join(RESULTS, "report.md"), "w").write("\n".join(lines) + "\n")
    write_json("scores.json", {"all": every, "untuned": untuned, "rows": rows})
    print("\n".join(lines[:9]))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("what", choices=["resolve", "github", "ours", "score"])
    ap.add_argument("--queries", default=os.path.join(HERE, "queries.tsv"))
    ap.add_argument("--gh", action="store_true", help="ask GitHub through the gh command line")
    ap.add_argument("--url", default="http://127.0.0.1:7880")
    ap.add_argument("--out", default="ours.json")
    ap.add_argument("--ours", default="ours.json", help="score: the explorer's answers to read")
    a = ap.parse_args()
    {"resolve": resolve, "github": github, "ours": ours, "score": score}[a.what](a)


if __name__ == "__main__":
    main()
