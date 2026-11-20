"""Kenji's head-to-head: a multi-tenant ticket search, ChloeDB's OpenSearch
API against Elasticsearch.

Kenji (persona study, round 4) runs a support-ticket service on Elasticsearch:
one index, a `tenant_id` on each ticket, and a search per request that is a
`bool` (a `multi_match` on title^3 and body; filters on the tenant, the
status and a date range), `terms` aggregations on `tags` (an array) and
`status`, highlights, a sort, and "load more" by `search_after`. His CTO wants
"memory and latency at 2 M docs on our machine sizes against our
Elasticsearch, measured the same way". This builds made-up tickets of his shape
(200 tenants, Zipf 1.07; titles and bodies written from a support vocabulary,
up to 8 tags), sends the same requests to both, and measures:

- whether they agree: the hit total, the first page's ids, the buckets;
- p50/p95/p99 of his search at 1 and 4 clients, and of its parts alone (the
  text query, the `tags` aggregation, the date sort with an `id` tie-breaker);
- paging a tenant to its end by `search_after`;
- loading by `_bulk`, memory and disk.

    python3 run.py [DOCS]          # default 300000 (round 4's size); raw/results-<DOCS>.json

Elasticsearch runs in Docker (`elasticsearch:9.5.5`, the image the repo's
compare kit uses), single node, security off, a heap of H2H_ES_HEAP (4g).
"""

import concurrent.futures
import json
import os
import random
import subprocess
import sys
import time
import urllib.error
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import common  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
ES_IMAGE = os.environ.get("H2H_ES_IMAGE", "mirror.gcr.io/library/elasticsearch:9.5.5")
ES_HEAP = os.environ.get("H2H_ES_HEAP", "4g")
SEARCHES = int(os.environ.get("H2H_SEARCHES", "1000"))

TOPICS = {
    "billing": (["payment", "invoice", "charge", "refund", "card", "declined", "subscription", "renewal", "billing", "receipt", "tax", "plan"],
                ["billing", "payment", "refund", "invoice"]),
    "login": (["password", "reset", "login", "account", "locked", "verification", "email", "code", "sso", "session", "signin", "mfa"],
              ["login", "sso", "account", "security"]),
    "performance": (["slow", "timeout", "latency", "dashboard", "loading", "spinner", "lag", "freeze", "crash", "memory", "cpu", "page"],
                    ["performance", "bug", "outage"]),
    "data": (["export", "import", "csv", "sync", "missing", "duplicate", "report", "records", "backup", "restore", "spreadsheet", "fields"],
             ["data", "export", "data-loss", "sync"]),
    "integration": (["api", "webhook", "token", "key", "rate", "limit", "endpoint", "oauth", "callback", "payload", "integration", "sdk"],
                    ["api", "integration", "webhook"]),
    "mobile": (["app", "iphone", "android", "notification", "push", "update", "install", "tablet", "offline", "camera", "battery", "screen"],
               ["mobile", "ios", "android", "notifications"]),
    "shipping": (["order", "delivery", "shipment", "tracking", "package", "address", "courier", "delayed", "damaged", "return", "warehouse", "label"],
                 ["shipping", "order", "returns"]),
}
FILLER = ["the", "our", "customer", "team", "since", "yesterday", "again", "after", "today", "please", "help", "urgent", "still",
          "cannot", "error", "issue", "problem", "when", "trying", "users", "report", "seems", "every", "time", "morning", "week"]
GENERIC_TAGS = ["urgent-customer", "feature-request", "bug", "question", "vip", "follow-up", "escalated", "web", "enterprise", "trial"]
STATUSES = ["open", "pending", "solved", "closed"]
PRIORITIES = ["low", "medium", "high", "urgent"]
TYPES = ["Incident", "Request", "Problem", "Change"]

MAPPING = {
    "settings": {"number_of_shards": 1, "number_of_replicas": 0},
    "mappings": {"properties": {
        "id": {"type": "keyword"}, "tenant_id": {"type": "keyword"},
        "title": {"type": "text", "analyzer": "english"}, "body": {"type": "text", "analyzer": "english"},
        "status": {"type": "keyword"}, "priority": {"type": "keyword"}, "queue": {"type": "keyword"}, "type": {"type": "keyword"},
        "tags": {"type": "keyword"}, "created_at": {"type": "date"},
    }},
}


def tenants(n_docs, n=200, s=1.07):
    w = [1 / (k ** s) for k in range(1, n + 1)]
    tot = sum(w)
    counts = [max(200, int(n_docs * x / tot)) for x in w]
    counts[0] += n_docs - sum(counts)
    return [(f"t{k:03d}", c) for k, c in enumerate(counts, 1)]


def tickets(n_docs, seed=42):
    rnd = random.Random(seed)
    topics = list(TOPICS)
    t0 = 1700000000  # 2023-11-14, so the tickets span the two years to 2025-11
    for tenant, count in tenants(n_docs):
        for i in range(count):
            topic = rnd.choices(topics, weights=[5, 4, 3, 3, 2, 2, 2])[0]
            words, tagset = TOPICS[topic]
            title = " ".join(rnd.sample(words, rnd.randint(2, 4)) + rnd.sample(FILLER, rnd.randint(1, 3)))
            body = " ".join(rnd.choice(words) if rnd.random() < 0.35 else rnd.choice(FILLER) for _ in range(rnd.randint(25, 70)))
            tags = sorted(set(rnd.sample(tagset, rnd.randint(1, len(tagset))) + rnd.sample(GENERIC_TAGS, rnd.randint(0, 4))))[:8]
            yield {"id": f"{tenant}-{i:06d}", "tenant_id": tenant, "title": title.capitalize(), "body": body.capitalize() + ".",
                   "status": rnd.choices(STATUSES, weights=[3, 2, 4, 6])[0], "priority": rnd.choices(PRIORITIES, weights=[4, 5, 3, 1])[0],
                   "queue": topic, "type": rnd.choice(TYPES), "tags": tags,
                   "created_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(t0 + rnd.randint(0, 2 * 365 * 86400)))}


def searches(n_docs, n, seed=7):
    """The service's search, for tenants drawn as its traffic is (the big
    ones most), each with two words of a topic, sorted by score or by date."""
    rnd = random.Random(seed)
    ts = tenants(n_docs)
    out = []
    for _ in range(n):
        tenant = rnd.choices([t for t, _ in ts], weights=[c for _, c in ts])[0]
        words, _ = TOPICS[rnd.choice(list(TOPICS))]
        q = " ".join(rnd.sample(words, 2))
        by_date = rnd.random() < 0.4
        body = {
            "size": 20, "track_total_hits": True,
            "query": {"bool": {
                "must": [{"multi_match": {"query": q, "fields": ["title^3", "body"]}}],
                "filter": [{"term": {"tenant_id": tenant}}, {"terms": {"status": ["open", "pending", "solved"]}},
                           {"range": {"created_at": {"gte": "2024-11-14T00:00:00Z"}}}],
            }},
            "aggs": {"tags": {"terms": {"field": "tags", "size": 10}}, "status": {"terms": {"field": "status"}}},
            "highlight": {"fields": {"title": {}, "body": {}}},
        }
        if by_date:
            body["sort"] = [{"created_at": "desc"}, {"id": "asc"}]
        out.append(body)
    return out


def parts(tenant):
    """The search taken apart, on the biggest tenant."""
    f = [{"term": {"tenant_id": tenant}}, {"terms": {"status": ["open", "pending", "solved"]}}, {"range": {"created_at": {"gte": "2024-11-14T00:00:00Z"}}}]
    text = {"bool": {"must": [{"multi_match": {"query": "payment declined", "fields": ["title^3", "body"]}}], "filter": f}}
    return {
        "text query": {"size": 20, "query": text},
        "+ highlight": {"size": 20, "query": text, "highlight": {"fields": {"title": {}, "body": {}}}},
        "terms on status (filters only)": {"size": 0, "query": {"bool": {"filter": f}}, "aggs": {"s": {"terms": {"field": "status"}}}},
        "terms on tags, an array (filters only)": {"size": 0, "query": {"bool": {"filter": f}}, "aggs": {"t": {"terms": {"field": "tags", "size": 10}}}},
        "sort by date": {"size": 20, "query": {"bool": {"filter": f}}, "sort": [{"created_at": "desc"}]},
        "sort by date, id tie-breaker": {"size": 20, "query": {"bool": {"filter": f}}, "sort": [{"created_at": "desc"}, {"id": "asc"}]},
        "terms on tags, whole index": {"size": 0, "aggs": {"t": {"terms": {"field": "tags", "size": 10}}}},
    }


class Http:
    def __init__(self, url, auth=None):
        self.url, self.auth = url, auth

    def req(self, method, path, body=None, ndjson=False):
        data = None
        if body is not None:
            data = body.encode() if isinstance(body, str) else json.dumps(body).encode()
        r = urllib.request.Request(self.url + path, data=data, method=method)
        r.add_header("content-type", "application/x-ndjson" if ndjson else "application/json")
        if self.auth:
            r.add_header("authorization", f"Bearer {self.auth}")
        try:
            with urllib.request.urlopen(r, timeout=600) as resp:
                return json.loads(resp.read() or b"null")
        except urllib.error.HTTPError as e:
            txt = e.read().decode(errors="replace")
            try:
                return {"_error": e.code, **json.loads(txt)}
            except ValueError:
                return {"_error": e.code, "text": txt[:500]}


def wait(http, path="/"):
    for _ in range(600):
        try:
            r = http.req("GET", path)
            if r and "_error" not in r:
                return r
        except (urllib.error.URLError, ConnectionError, OSError):
            pass
        time.sleep(0.5)
    raise SystemExit(f"{http.url} did not answer")


def bulk_load(http, n_docs, batch=2000):
    t = time.perf_counter()
    buf, sent = [], 0
    for d in tickets(n_docs):
        buf.append(json.dumps({"index": {"_index": "tickets", "_id": d["id"]}}))
        buf.append(json.dumps(d))
        if len(buf) >= 2 * batch:
            r = http.req("POST", "/_bulk", "\n".join(buf) + "\n", ndjson=True)
            if r.get("errors") or "_error" in r:
                raise SystemExit(f"bulk failed: {json.dumps(r)[:500]}")
            sent += len(buf) // 2
            buf = []
    if buf:
        r = http.req("POST", "/_bulk", "\n".join(buf) + "\n", ndjson=True)
        if r.get("errors") or "_error" in r:
            raise SystemExit(f"bulk failed: {json.dumps(r)[:500]}")
    http.req("POST", "/tickets/_refresh")
    return round(time.perf_counter() - t, 2)


def run_mix(http, bodies, clients):
    def one(b):
        t = time.perf_counter()
        r = http.req("POST", "/tickets/_search", b)
        return (time.perf_counter() - t) * 1000, "_error" in r
    for b in bodies[:50]:
        one(b)
    t = time.perf_counter()
    with concurrent.futures.ThreadPoolExecutor(clients) as ex:
        res = list(ex.map(one, bodies))
    wall = time.perf_counter() - t
    ms = [x for x, _ in res]
    import statistics
    q = statistics.quantiles(ms, n=100, method="inclusive")
    return {"clients": clients, "searches": len(bodies), "per_s": round(len(bodies) / wall, 1), "p50_ms": round(q[49], 2),
            "p95_ms": round(q[94], 2), "p99_ms": round(q[98], 2), "errors": sum(1 for _, e in res if e)}


def page_tenant(http, tenant):
    """Page one tenant to its end by search_after on (created_at desc, id asc)."""
    body = {"size": 100, "query": {"term": {"tenant_id": tenant}}, "sort": [{"created_at": "desc"}, {"id": "asc"}]}
    seen, pages, ms, ordered, last = set(), 0, [], True, None
    while True:
        t = time.perf_counter()
        r = http.req("POST", "/tickets/_search", body)
        ms.append((time.perf_counter() - t) * 1000)
        if "_error" in r:
            return {"error": json.dumps(r)[:300]}
        hits = r["hits"]["hits"]
        if not hits:
            break
        for h in hits:
            key = (h["_source"]["created_at"], h["_source"]["id"])
            if last is not None and not (key[0] < last[0] or (key[0] == last[0] and key[1] > last[1])):
                ordered = False
            last = key
            seen.add(h["_id"])
        pages += 1
        body["search_after"] = hits[-1]["sort"]
    import statistics
    return {"tenant": tenant, "pages": pages, "unique": len(seen), "in_order": ordered, "page_p50_ms": round(statistics.median(ms), 2)}


def compare(a, b):
    """Two answers to the same search: same total, first-page ids, buckets."""
    if "_error" in a or "_error" in b:
        return {"error": True}
    ia = [h["_id"] for h in a["hits"]["hits"]]
    ib = [h["_id"] for h in b["hits"]["hits"]]
    ba = {k: [(x["key"], x["doc_count"]) for x in v["buckets"]] for k, v in (a.get("aggregations") or {}).items()}
    bb = {k: [(x["key"], x["doc_count"]) for x in v["buckets"]] for k, v in (b.get("aggregations") or {}).items()}
    return {"total": a["hits"]["total"]["value"] == b["hits"]["total"]["value"], "ids_in_order": ia == ib,
            "ids_as_set": set(ia) == set(ib), "top10_overlap": len(set(ia[:10]) & set(ib[:10])), "buckets": ba == bb,
            "highlighted": all("highlight" in h for h in a["hits"]["hits"]) == all("highlight" in h for h in b["hits"]["hits"])}


def measure(name, http, n_docs, mem_fn, disk_fn, bodies):
    out = {"name": name}
    r = http.req("PUT", "/tickets", MAPPING)
    if "_error" in r:
        raise SystemExit(f"{name}: create index: {r}")
    out["bulk_s"] = bulk_load(http, n_docs)
    out["count"] = http.req("GET", "/tickets/_count")["count"]
    out["memory_after_load_kb"] = mem_fn()
    out["answers"] = [http.req("POST", "/tickets/_search", b) for b in bodies[:200]]
    out["mix"] = [run_mix(http, bodies, 1), run_mix(http, bodies, 4)]
    big = tenants(n_docs)[0][0]
    out["parts"] = {}
    for label, b in parts(big).items():
        r = http.req("POST", "/tickets/_search", b)
        if "_error" in r:
            out["parts"][label] = {"error": json.dumps(r)[:300]}
            continue
        ms = common.timed(lambda: http.req("POST", "/tickets/_search", b), 15)
        out["parts"][label] = common.percentiles(ms)
    out["paging"] = page_tenant(http, tenants(n_docs)[6][0])
    out["memory_after_kb"] = mem_fn()
    out["disk_bytes"] = disk_fn()
    return out


def run_elasticsearch(n_docs, bodies, work):
    name = "h2h-es"
    subprocess.run(["docker", "rm", "-f", name], capture_output=True)
    port = common.free_port()
    subprocess.run(["docker", "run", "-d", "--name", name, "-p", f"127.0.0.1:{port}:9200", "-e", "discovery.type=single-node",
                    "-e", "xpack.security.enabled=false", "-e", f"ES_JAVA_OPTS=-Xms{ES_HEAP} -Xmx{ES_HEAP}",
                    ES_IMAGE], check=True, capture_output=True)
    http = Http(f"http://127.0.0.1:{port}")
    info = wait(http)
    wait(http, "/_cluster/health?wait_for_status=yellow&timeout=60s")

    def mem():
        s = subprocess.run(["docker", "stats", "--no-stream", "--format", "{{.MemUsage}}", name], capture_output=True, text=True).stdout
        v = s.split("/")[0].strip()
        mult = {"KiB": 1, "MiB": 1024, "GiB": 1024 * 1024}
        for u, k in mult.items():
            if v.endswith(u):
                return int(float(v[:-len(u)]) * k)
        return 0

    def disk():
        http.req("POST", "/tickets/_forcemerge?max_num_segments=1")
        st = http.req("GET", "/tickets/_stats/store")
        return st["indices"]["tickets"]["total"]["store"]["size_in_bytes"]

    out = measure("elasticsearch", http, n_docs, mem, disk, bodies)
    out["version"] = f"Elasticsearch {info['version']['number']} (Docker {ES_IMAGE}, heap {ES_HEAP})"
    subprocess.run(["docker", "rm", "-f", name], capture_output=True)
    return out


def run_chloedb(n_docs, bodies, work):
    folder = os.path.join(work, "chloedb")
    if os.path.exists(folder):
        subprocess.run(["rm", "-rf", folder])
    port = common.free_port()
    ch = common.Chloedb(folder, extra=["--opensearch-listen", f"127.0.0.1:{port}"])
    http = Http(f"http://127.0.0.1:{port}", auth=ch.key)
    wait(http)

    def mem():
        with open(f"/proc/{ch.proc.pid}/status") as f:
            for line in f:
                if line.startswith("VmRSS:"):
                    return int(line.split()[1])
        return 0

    def disk():
        ch.call("/v1/admin/vacuum", {})
        return ch.disk_bytes()

    out = measure("chloedb", http, n_docs, mem, disk, bodies)
    out["version"] = ch.version + " (its OpenSearch API)"
    # The same tags facet natively, on a KEYWORD[] column, for the record (KENJI-401)
    ch.stop()
    return out


def main():
    n_docs = int(sys.argv[1]) if len(sys.argv) > 1 else 300000
    work = os.environ.get("H2H_WORK", f"/tmp/h2h-kenji/{n_docs}")
    os.makedirs(work, exist_ok=True)
    bodies = searches(n_docs, SEARCHES)
    result = {"machine": common.machine(), "docs": n_docs, "searches": SEARCHES, "at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
              "tenants": len(tenants(n_docs)), "biggest_tenant": tenants(n_docs)[0]}
    es = run_elasticsearch(n_docs, bodies, work)
    ch = run_chloedb(n_docs, bodies, work)
    agree = [compare(c, e) for c, e in zip(ch.pop("answers"), es.pop("answers"))]
    result["agreement"] = {k: sum(1 for a in agree if a.get(k)) for k in ("total", "ids_in_order", "ids_as_set", "buckets", "highlighted")}
    result["agreement"]["compared"] = len(agree)
    result["agreement"]["errors"] = sum(1 for a in agree if a.get("error"))
    result["agreement"]["top10_overlap_mean"] = round(sum(a.get("top10_overlap", 0) for a in agree) / len(agree), 2)
    result["elasticsearch"], result["chloedb"] = es, ch
    common.write_json(os.path.join(HERE, "raw", f"results-{n_docs}.json"), result)
    print(json.dumps({k: result[k] for k in ("agreement",)}, indent=1))


if __name__ == "__main__":
    main()
