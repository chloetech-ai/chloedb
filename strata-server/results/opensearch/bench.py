#!/usr/bin/env python3
"""Strata's OpenSearch API next to OpenSearch, on the same machine and the
same requests: memory, disk, bulk indexing and the latency of ten kinds of
search, on BEIR's TREC-COVID corpus (171,332 abstracts) with a few made-up
fields to filter, sort and count by.

    bench.py --data DIR --engine NAME=URL[,pid=PID][,disk=PATH] ... --out results.json

DIR holds corpus.jsonl.gz and queries.jsonl.gz of BeIR/trec-covid (revision
d158b3b1564c0d022d67c482c3d5bbb10922dfe7 on Hugging Face). Each engine is
measured alone, one after the other, with the same requests in the same
order. A search's latency is the client's: from sending the request to
reading the whole answer, over one kept-alive connection."""
import argparse, gzip, http.client, json, os, random, re, statistics, subprocess, sys, threading, time, urllib.parse

INDEX = "covid"
SOURCES = ["pubmed", "medrxiv", "biorxiv", "who", "elsevier", "arxiv"]

def docs_of(path, limit):
    rnd = random.Random(42)
    with gzip.open(path, "rt") as f:
        for i, line in enumerate(f):
            if limit and i >= limit:
                break
            d = json.loads(line)
            year = rnd.randint(2000, 2022)
            yield d["_id"], {
                "title": d.get("title", ""),
                "text": d.get("text", ""),
                "source": rnd.choice(SOURCES),
                "year": year,
                "published": f"{year}-{rnd.randint(1, 12):02d}-{rnd.randint(1, 28):02d}",
                "cites": int(rnd.paretovariate(1.2)) - 1,
            }

MAPPING = {
    "settings": {"index": {"number_of_shards": 1, "number_of_replicas": 0, "requests.cache.enable": False}},
    "mappings": {"properties": {
        "title": {"type": "text"},
        "text": {"type": "text"},
        "source": {"type": "keyword"},
        "year": {"type": "long"},
        "published": {"type": "date"},
        "cites": {"type": "long"},
    }},
}

class Conn:
    def __init__(self, url):
        u = urllib.parse.urlparse(url)
        self.host, self.port = u.hostname, u.port
        self.c = http.client.HTTPConnection(self.host, self.port, timeout=600)

    def call(self, method, path, body=None, nd=False):
        data = None if body is None else (body if isinstance(body, (bytes, str)) else json.dumps(body))
        if isinstance(data, str):
            data = data.encode()
        headers = {"content-type": "application/x-ndjson" if nd else "application/json"}
        for attempt in range(2):
            try:
                self.c.request(method, path, body=data, headers=headers)
                r = self.c.getresponse()
                t = r.read()
                return r.status, (json.loads(t) if t else None)
            except (http.client.HTTPException, ConnectionError):
                self.c.close()
                self.c = http.client.HTTPConnection(self.host, self.port, timeout=600)
                if attempt:
                    raise

def rss_kb(pid):
    if not pid:
        return None
    out = {}
    with open(f"/proc/{pid}/status") as f:
        for line in f:
            k, _, v = line.partition(":")
            if k in ("VmRSS", "VmHWM"):
                out[k] = int(v.split()[0])
    return out

def disk_bytes(path):
    if not path:
        return None
    total = 0
    if os.path.isdir(path):
        for root, _, files in os.walk(path):
            for f in files:
                try:
                    total += os.path.getsize(os.path.join(root, f))
                except OSError:
                    pass
        return total
    d, base = os.path.split(path)
    for f in os.listdir(d or "."):
        # The database and its write-ahead log, not the server's own logs.
        if f == base or f == base + "-wal":
            total += os.path.getsize(os.path.join(d, f))
    return total

def queries(data, n, seed=7):
    """n requests of each kind, the same for every engine."""
    rnd = random.Random(seed)
    topics = []
    with gzip.open(os.path.join(data, "queries.jsonl.gz"), "rt") as f:
        for line in f:
            q = json.loads(line)
            topics.append(q["metadata"]["query"])
            topics.append(q["text"])
    words = sorted({w for t in topics for w in re.findall(r"[a-z0-9]+", t.lower()) if len(w) > 3})
    pick = lambda: rnd.choice(topics)
    kinds = {
        "match one word (title)": lambda: {"query": {"match": {"title": rnd.choice(words)}}},
        "match a topic (text)": lambda: {"query": {"match": {"text": pick()}}},
        "match + filter + range": lambda: {"query": {"bool": {"must": {"match": {"text": pick()}}, "filter": [{"term": {"source": rnd.choice(SOURCES)}}, {"range": {"year": {"gte": rnd.randint(2000, 2015)}}}]}}},
        "multi_match title^2, text": lambda: {"query": {"multi_match": {"query": pick(), "fields": ["title^2", "text"]}}},
        "match_phrase (text)": lambda: {"query": {"match_phrase": {"text": " ".join(rnd.sample(["respiratory syndrome", "public health", "viral load", "immune response", "clinical trial", "social distancing", "case fatality"], 1))}}},
        "term filter only": lambda: {"query": {"bool": {"filter": {"term": {"source": rnd.choice(SOURCES)}}}}},
        "range + sort by date": lambda: {"query": {"range": {"published": {"gte": f"{rnd.randint(2000, 2021)}-01-01"}}}, "sort": [{"published": "desc"}]},
        "terms agg on a match": lambda: {"size": 0, "query": {"match": {"text": pick()}}, "aggs": {"by_source": {"terms": {"field": "source"}}}},
        "date_histogram, all docs": lambda: {"size": 0, "aggs": {"per_year": {"date_histogram": {"field": "published", "calendar_interval": "year"}, "aggs": {"cites": {"avg": {"field": "cites"}}}}}},
        "match + highlight": lambda: {"query": {"match": {"text": pick()}}, "highlight": {"fields": {"text": {}}}},
    }
    return {k: [f() for _ in range(n)] for k, f in kinds.items()}

def pct(xs, p):
    xs = sorted(xs)
    return xs[min(len(xs) - 1, int(round(p / 100 * (len(xs) - 1))))]

def bench(name, url, pid, disk, data, limit, nq, conc_secs):
    out = {"engine": name, "url": url}
    c = Conn(url)
    s, info = c.call("GET", "/")
    out["version"] = info.get("version", {}).get("number") if isinstance(info, dict) else None
    c.call("DELETE", "/" + INDEX)
    s, r = c.call("PUT", "/" + INDEX, MAPPING)
    assert s == 200, r
    out["rss_kb_empty"] = rss_kb(pid)
    # Bulk: 1,000 documents a request, one client.
    batch, n, t0 = [], 0, time.time()
    errors = 0
    def send(batch):
        nonlocal errors
        s, r = c.call("POST", f"/{INDEX}/_bulk", "".join(batch), nd=True)
        assert s == 200, (s, str(r)[:300])
        errors += sum(1 for it in r["items"] if "error" in next(iter(it.values())))
    for id_, d in docs_of(os.path.join(data, "corpus.jsonl.gz"), limit):
        batch.append(json.dumps({"index": {"_id": id_}}) + "\n" + json.dumps(d) + "\n")
        n += 1
        if len(batch) == 1000:
            send(batch)
            batch = []
    if batch:
        send(batch)
    t_bulk = time.time() - t0
    c.call("POST", f"/{INDEX}/_refresh")
    t_all = time.time() - t0
    s, cnt = c.call("GET", f"/{INDEX}/_count")
    out["bulk"] = {"docs": n, "errors": errors, "seconds": round(t_all, 2), "docs_per_s": round(n / t_all), "seconds_before_refresh": round(t_bulk, 2), "count_after": cnt["count"]}
    print(name, "bulk", out["bulk"], flush=True)
    # Let background merges settle.
    time.sleep(30)
    out["rss_kb_loaded"] = rss_kb(pid)
    out["disk_bytes"] = disk_bytes(disk)
    s, cat = c.call("GET", f"/_cat/indices/{INDEX}?format=json&bytes=b")
    out["store_size_reported"] = cat[0].get("store.size") if isinstance(cat, list) and cat else None
    # Searches: warm up with a pass of each kind, then time them.
    qs = queries(data, nq)
    for kind, bodies in qs.items():
        for b in bodies[: max(5, nq // 10)]:
            c.call("POST", f"/{INDEX}/_search", b)
    lat = {}
    totals = {}
    for kind, bodies in qs.items():
        ms = []
        tot = []
        for b in bodies:
            t = time.perf_counter()
            s, r = c.call("POST", f"/{INDEX}/_search", b)
            ms.append((time.perf_counter() - t) * 1000)
            assert s == 200, (kind, s, str(r)[:400])
            tot.append(r["hits"]["total"]["value"] if r["hits"].get("total") else None)
        lat[kind] = {"p50_ms": round(pct(ms, 50), 2), "p90_ms": round(pct(ms, 90), 2), "p99_ms": round(pct(ms, 99), 2), "mean_ms": round(statistics.mean(ms), 2), "n": len(ms)}
        totals[kind] = tot
        print(name, kind, lat[kind], flush=True)
    out["latency"] = lat
    out["totals"] = totals
    # Throughput: 8 clients sending topic matches for conc_secs.
    stop = time.time() + conc_secs
    done = [0] * 8
    def worker(k):
        cc = Conn(url)
        rnd = random.Random(k)
        bodies = qs["match a topic (text)"]
        while time.time() < stop:
            s, r = cc.call("POST", f"/{INDEX}/_search", rnd.choice(bodies))
            assert s == 200
            done[k] += 1
    ts = [threading.Thread(target=worker, args=(k,)) for k in range(8)]
    for t in ts:
        t.start()
    for t in ts:
        t.join()
    out["throughput_8_clients"] = {"kind": "match a topic (text)", "seconds": conc_secs, "searches_per_s": round(sum(done) / conc_secs, 1)}
    print(name, "throughput", out["throughput_8_clients"], flush=True)
    out["rss_kb_after_searches"] = rss_kb(pid)
    # OpenSearch's request cache is off for the index; its counters say so.
    s, st = c.call("GET", f"/{INDEX}/_stats/request_cache")
    try:
        out["request_cache"] = st["_all"]["total"]["request_cache"]
    except (KeyError, TypeError):
        out["request_cache"] = None
    return out

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--data", required=True)
    ap.add_argument("--engine", action="append", required=True, help="NAME=URL[,pid=PID][,disk=PATH]")
    ap.add_argument("--docs", type=int, default=0, help="index only the first N documents (0: all)")
    ap.add_argument("--searches", type=int, default=300, help="searches timed of each kind")
    ap.add_argument("--concurrent-seconds", type=int, default=20)
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    machine = {"cpu": next((l.split(":", 1)[1].strip() for l in open("/proc/cpuinfo") if l.startswith("model name")), None), "cores": os.cpu_count(),
               "ram_gb": round(os.sysconf("SC_PAGE_SIZE") * os.sysconf("SC_PHYS_PAGES") / 2**30, 1), "kernel": os.uname().release}
    results = {"machine": machine, "when": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "engines": []}
    for spec in a.engine:
        name, _, rest = spec.partition("=")
        parts = rest.split(",")
        url = parts[0]
        opts = dict(p.split("=", 1) for p in parts[1:])
        results["engines"].append(bench(name, url, int(opts["pid"]) if "pid" in opts else None, opts.get("disk"), a.data, a.docs, a.searches, a.concurrent_seconds))
    # Do the engines find as many hits for each search?
    if len(results["engines"]) == 2:
        x, y = results["engines"]
        results["same_totals"] = {k: sum(1 for p, q in zip(x["totals"][k], y["totals"][k]) if p == q) for k in x["totals"]}
    for e in results["engines"]:
        e.pop("totals", None)
    with open(a.out, "w") as f:
        json.dump(results, f, indent=2)
    print(json.dumps(results.get("same_totals"), indent=1))

main()
