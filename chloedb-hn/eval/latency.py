#!/usr/bin/env python3
"""Search latency of Hacker News on ChloeDB and of hn.algolia.com, on the
same queries, one at a time, from the same machine.

The queries are those compare.py asked (its compare.json), or a file of one
query a line. Each is asked of both, in turns, as Algolia's own page asks:
stories, 20 a page; ChloeDB's search of stories without facets (`only=story`,
`facets=0`). For each it keeps:

  server  ChloeDB's took_ms (the app's whole search: its understanding of the
          words, the candidates by words, text and meaning, the ranking, the
          count and the highlights, each a query of the cluster) and
          Algolia's processingTimeMS
  e2e     the request's whole time from this machine, connection reused

and the network's floor to each (a request that does no search: the app's
/healthz, Algolia's search of a word that matches nothing is not one, so its
floor is the smallest e2e time less its processingTimeMS).

A query is asked once cold (first), and again warm (second), so that a cache
on either side shows. Run it where the users would be: from this machine,
or from a pod beside the cluster (--app http://hn-app:7850).

    CHLOEDB_HN_TOKEN=... python3 eval/latency.py --app https://cmd.chloetech.ai/v1/apps/hn --queries eval/results/compare.json --out eval/results
"""
import argparse, http.client, json, math, os, statistics, sys, time, urllib.parse

ALGOLIA = "https://hn.algolia.com/api/v1/search"
TOKEN = os.environ.get("CHLOEDB_HN_TOKEN", "")


class Conn:
    """One kept-alive connection to a host (as a browser reuses one)."""

    def __init__(self, base, token=""):
        u = urllib.parse.urlparse(base)
        self.https = u.scheme == "https"
        self.host, self.port, self.prefix = u.hostname, u.port, u.path.rstrip("/")
        self.token = token
        self.c = None

    def get(self, path, timeout=60):
        hdr = {"user-agent": "chloedb-hn-latency", "accept-encoding": "identity"}
        if self.token:
            hdr["authorization"] = f"Bearer {self.token}"
        for attempt in range(3):
            if self.c is None:
                cls = http.client.HTTPSConnection if self.https else http.client.HTTPConnection
                self.c = cls(self.host, self.port, timeout=timeout)
            try:
                t0 = time.perf_counter()
                self.c.request("GET", self.prefix + path, headers=hdr)
                r = self.c.getresponse()
                body = r.read()
                ms = (time.perf_counter() - t0) * 1000
                if r.status >= 500 and attempt < 2:
                    time.sleep(2)
                    continue
                return r.status, (json.loads(body) if body[:1] in (b"{", b"[") else body.decode(errors="replace")), ms
            except (OSError, http.client.HTTPException):
                self.c = None
                if attempt == 2:
                    raise
                time.sleep(1)


def pct(xs, p):
    xs = sorted(x for x in xs if x is not None)
    if not xs:
        return None
    k = (len(xs) - 1) * p
    f, c = math.floor(k), math.ceil(k)
    return round(xs[f] + (xs[c] - xs[f]) * (k - f), 1)


def stats(xs):
    xs = [x for x in xs if x is not None]
    return {"n": len(xs), "p50": pct(xs, 0.5), "p90": pct(xs, 0.9), "p95": pct(xs, 0.95), "p99": pct(xs, 0.99), "mean": round(statistics.mean(xs), 1) if xs else None}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--app", default="http://127.0.0.1:7850")
    ap.add_argument("--queries", required=True, help="compare.json, or a file of one query a line")
    ap.add_argument("--out", default="eval/results")
    ap.add_argument("--name", default="latency", help="the results' file name, without .json")
    ap.add_argument("--where", default="", help="where this ran, for the results (a machine, a region)")
    ap.add_argument("--pause", type=float, default=0.2, help="seconds between requests")
    ap.add_argument("--limit", type=int, default=0)
    a = ap.parse_args()
    if a.queries.endswith(".json"):
        qs = [q["q"] for q in json.load(open(a.queries))["queries"]]
        qs += [p["q"] for p in json.load(open(a.queries))["sets"].get("topic", {}).get("per_query", [])]
    else:
        qs = [l.strip() for l in open(a.queries) if l.strip()]
    seen, uniq = set(), []
    for q in qs:
        if q.lower() not in seen:
            seen.add(q.lower())
            uniq.append(q)
    qs = uniq[: a.limit] if a.limit else uniq
    ours, theirs = Conn(a.app, TOKEN), Conn(ALGOLIA.rsplit("/api", 1)[0])
    # The network's floor to each, before any search.
    floor_o = [ours.get("/healthz")[2] for _ in range(10)]
    rows = []
    for i, q in enumerate(qs):
        e = urllib.parse.quote(q)
        rec = {"q": q}
        for k in ("cold", "warm"):
            st, j, ms = ours.get(f"/v1/search?q={e}&only=story&facets=0")
            rec[f"ours_{k}_server"] = j.get("took_ms") if isinstance(j, dict) else None
            rec[f"ours_{k}_e2e"] = ms
            rec[f"ours_{k}_status"] = st
            time.sleep(a.pause)
            st, j, ms = theirs.get(f"/api/v1/search?query={e}&tags=story&hitsPerPage=20")
            rec[f"algolia_{k}_server"] = j.get("processingTimeMS") if isinstance(j, dict) else None
            rec[f"algolia_{k}_e2e"] = ms
            rec[f"algolia_{k}_status"] = st
            time.sleep(a.pause)
        rows.append(rec)
        if i % 20 == 0:
            print(i, rec, file=sys.stderr)
    floor_a = min(r["algolia_cold_e2e"] - (r["algolia_cold_server"] or 0) for r in rows) if rows else None
    out = {
        "when": time.strftime("%Y-%m-%d %H:%M:%S %Z"), "where": a.where, "app": a.app, "queries": len(rows),
        "network_floor_ms": {"ours": stats(floor_o), "algolia_min": round(floor_a, 1) if floor_a is not None else None},
        "ours": {k: stats([r[f"ours_{k}"] for r in rows]) for k in ("cold_server", "cold_e2e", "warm_server", "warm_e2e")},
        "algolia": {k: stats([r[f"algolia_{k}"] for r in rows]) for k in ("cold_server", "cold_e2e", "warm_server", "warm_e2e")},
        "errors": {"ours": sum(1 for r in rows if r["ours_cold_status"] != 200), "algolia": sum(1 for r in rows if r["algolia_cold_status"] != 200)},
        "rows": rows,
    }
    os.makedirs(a.out, exist_ok=True)
    json.dump(out, open(os.path.join(a.out, f"{a.name}.json"), "w"), indent=1)
    print(json.dumps({k: v for k, v in out.items() if k != "rows"}, indent=1))


if __name__ == "__main__":
    main()
