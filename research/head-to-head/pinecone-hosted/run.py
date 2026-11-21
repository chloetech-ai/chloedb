"""Vector search: ChloeDB against hosted Pinecone (serverless), on GloVe-100.

The hosted half of ../pinecone-local: the same collection, the same unit
vectors, the same 1,000 test queries and the same recall and latency code
(imported from ../pinecone-local/run.py), against a real Pinecone serverless
index on the Starter (free) plan instead of the emulator.

  1. An index of `dimension: 100, metric: cosine` in aws us-east-1 (the one
     region the Starter plan offers) is created on api.pinecone.io, its host
     read from the create's answer, and loaded by `POST /vectors/upsert` 500
     vectors a request (1,000 as JSON pass 2 MB), with H2H_PC_WRITERS
     requests at once (default 1, as ChloeDB is loaded). Load time stops when
     the last upsert is acknowledged; ready time when
     `describe_index_stats` counts every vector, polled each second.
  2. The same 1,000 queries, `POST /query {vector, topK: 10}`, over kept-alive
     HTTPS connections (one per client thread, as Pinecone's SDK pools them),
     at 1 and 4 clients after 20 warm-ups. Each answer's
     `x-pinecone-response-duration-ms` header (Pinecone's own time for the
     request, when it sends one) and its read units are kept, so the page can
     split the client's time into Pinecone's and the network's.
  3. The network floor: the time of a request that does no search work
     (`POST /describe_index_stats`) on the same kept-alive connection, and of
     a TCP and TLS handshake on a new one. This container reaches the internet
     through an egress proxy that ends TLS and opens it again, so the floor
     includes that hop; the machine's own position is not known.
  4. ChloeDB runs on this machine as in ../pinecone-local (its run_chloedb),
     so both come from the same run; its numbers are on the machine, with no
     network between client and server.
  5. The index is deleted at the end, also when the run fails.

    pinecone=<API key> python3 run.py [100k full]

The key is read from the environment variable `pinecone` (or
PINECONE_API_KEY) and never written anywhere. Raw numbers go to
raw/results-<size>.json.
"""

import concurrent.futures as cf
import importlib.util
import json
import os
import socket
import ssl
import sys
import threading
import time
import urllib.parse

import numpy as np
import requests

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
import common  # noqa: E402

_spec = importlib.util.spec_from_file_location("pc_local", os.path.join(os.path.dirname(HERE), "pinecone-local", "run.py"))
local = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(local)

CONTROL = "https://api.pinecone.io"
API_VERSION = "2025-04"
INDEX = os.environ.get("H2H_PC_INDEX", "h2h-glove")
WRITERS = int(os.environ.get("H2H_PC_WRITERS", "1"))
BATCH = local.BATCH
WARM = local.WARM


def key():
    k = os.environ.get("pinecone") or os.environ.get("PINECONE_API_KEY")
    if not k:
        raise SystemExit("set the Pinecone API key in $pinecone or $PINECONE_API_KEY")
    return k


class Hosted:
    """Pinecone's REST API, as the SDKs send it, with one kept-alive session
    per thread."""

    def __init__(self):
        self.headers = {"Api-Key": key(), "X-Pinecone-API-Version": API_VERSION, "content-type": "application/json"}
        self.local = threading.local()
        self.data = None
        self.served = []  # (x-pinecone-response-duration-ms, read units) of each query
        self.lock = threading.Lock()

    def session(self):
        s = getattr(self.local, "s", None)
        if s is None:
            s = self.local.s = requests.Session()
            s.headers.update(self.headers)
        return s

    def req(self, method, url, body=None, tries=8):
        for attempt in range(tries):
            try:
                r = self.session().request(method, url, data=None if body is None else json.dumps(body), timeout=120)
            except requests.RequestException as e:
                if attempt == tries - 1:
                    raise
                time.sleep(2 ** attempt * 0.5)
                continue
            if r.status_code == 429 or r.status_code >= 500:
                if attempt == tries - 1:
                    return r
                time.sleep(2 ** attempt * 0.5)
                continue
            return r
        return r

    def json(self, method, url, body=None):
        r = self.req(method, url, body)
        if r.status_code >= 300:
            raise SystemExit(f"{method} {urllib.parse.urlparse(url).path}: {r.status_code} {r.text[:400]}")
        return r.json() if r.content else {}

    def delete(self, name):
        r = self.req("DELETE", f"{CONTROL}/indexes/{name}")
        return r.status_code

    def create(self, name, dim):
        if self.req("GET", f"{CONTROL}/indexes/{name}").status_code == 200:
            self.delete(name)
            self.wait_gone(name)
        t = time.perf_counter()
        self.json("POST", f"{CONTROL}/indexes", {
            "name": name, "dimension": dim, "metric": "cosine", "deletion_protection": "disabled",
            "spec": {"serverless": {"cloud": "aws", "region": "us-east-1"}}})
        while True:
            d = self.json("GET", f"{CONTROL}/indexes/{name}")
            if d.get("status", {}).get("ready"):
                break
            time.sleep(1)
        self.data = "https://" + d["host"]
        self.describe = {k: d.get(k) for k in ("dimension", "metric", "spec", "vector_type")}
        return round(time.perf_counter() - t, 2)

    def wait_gone(self, name):
        for _ in range(120):
            if self.req("GET", f"{CONTROL}/indexes/{name}").status_code == 404:
                return
            time.sleep(1)

    def stats(self):
        return self.json("POST", self.data + "/describe_index_stats", {})

    def query(self, vec):
        t = time.perf_counter()
        r = self.req("POST", self.data + "/query", {"vector": vec, "topK": 10})
        client = (time.perf_counter() - t) * 1000
        if r.status_code != 200:
            raise SystemExit(f"pinecone refused: {r.status_code} {r.text[:400]}")
        b = r.json()
        h = r.headers
        with self.lock:
            self.served.append({"client": client, "duration": _f(h.get("x-pinecone-response-duration-ms")),
                                "latency": _f(h.get("x-pinecone-request-latency-ms")),
                                "upstream": _f(h.get("x-envoy-upstream-service-time")),
                                "ru": (b.get("usage") or {}).get("readUnits")})
        return [m["id"] for m in b["matches"]]


def pct(ms):
    q = sorted(ms)
    return {"p50_ms": round(q[len(q) // 2], 3), "p95_ms": round(q[int(len(q) * 0.95)], 3),
            "p99_ms": round(q[min(len(q) - 1, int(len(q) * 0.99))], 3), "n": len(q)}


def _f(v):
    return float(v) if v is not None else None


def handshake_ms(host, n=10):
    """TCP connect to the proxy, CONNECT to the host and the TLS handshake, on
    a new connection each time (what a client without kept-alive connections
    pays on every request)."""
    proxy = os.environ.get("HTTPS_PROXY") or os.environ.get("https_proxy")
    ctx = ssl.create_default_context(cafile=os.environ.get("SSL_CERT_FILE") or None)
    out = []
    for _ in range(n):
        t = time.perf_counter()
        if proxy:
            p = urllib.parse.urlparse(proxy)
            s = socket.create_connection((p.hostname, p.port), timeout=30)
            s.sendall(f"CONNECT {host}:443 HTTP/1.1\r\nHost: {host}:443\r\n\r\n".encode())
            buf = b""
            while b"\r\n\r\n" not in buf:
                buf += s.recv(4096)
            if b" 200" not in buf.split(b"\r\n")[0]:
                raise SystemExit(f"proxy refused CONNECT: {buf[:100]!r}")
        else:
            s = socket.create_connection((host, 443), timeout=30)
        tls = ctx.wrap_socket(s, server_hostname=host)
        out.append((time.perf_counter() - t) * 1000)
        tls.close()
    return pct(out)


def floor_ms(pc, n=100):
    """A request that does no search work, on a kept-alive connection."""
    pc.stats()
    out = []
    for _ in range(n):
        t = time.perf_counter()
        pc.stats()
        out.append((time.perf_counter() - t) * 1000)
    return pct(out)


def served_summary(rows):
    """Pinecone's own times from its headers, and the rest of each request's
    time on the client (the network, the proxy and the client's own work)."""
    out = {"n": len(rows)}
    for k in ("duration", "latency", "upstream"):
        v = [r[k] for r in rows if r[k] is not None]
        out[f"{k}_ms"] = pct(v) if v else None
    rest = [r["client"] - r["duration"] for r in rows if r["duration"] is not None]
    out["client_minus_duration_ms"] = pct(rest) if rest else None
    rus = [r["ru"] for r in rows if r["ru"] is not None]
    out["read_units_each"] = sorted(set(rus))
    out["read_units_total"] = round(sum(rus), 2) if rus else None
    return out


def run_pinecone(base, queries):
    pc = Hosted()
    out = {"version": f"Pinecone serverless (hosted, Starter plan), aws us-east-1, API {API_VERSION}", "writers": WRITERS}
    try:
        out["create_s"] = pc.create(INDEX, base.shape[1])
        out["index"] = pc.describe
        out["handshake_ms"] = handshake_ms(pc.data[len("https://"):])
        batches = list(range(0, len(base), BATCH))

        def upsert(i):
            vecs = [{"id": str(j), "values": base[j].tolist()} for j in range(i, min(i + BATCH, len(base)))]
            r = pc.req("POST", pc.data + "/vectors/upsert", {"vectors": vecs}, tries=12)
            if r.status_code != 200:
                raise SystemExit(f"upsert failed at {i}: {r.status_code} {r.text[:400]}")
            return r.json().get("upsertedCount", 0)

        t = time.perf_counter()
        if WRITERS == 1:
            done = sum(upsert(i) for i in batches)
        else:
            with cf.ThreadPoolExecutor(WRITERS) as ex:
                done = sum(ex.map(upsert, batches))
        out["load_s"] = round(time.perf_counter() - t, 2)
        out["upserted"] = done
        print(f"pinecone: {done:,} upserted in {out['load_s']} s", flush=True)
        polls = []
        while True:
            st = pc.stats()
            polls.append((round(time.perf_counter() - t, 1), st.get("totalVectorCount")))
            if st.get("totalVectorCount") == len(base):
                break
            time.sleep(1)
        out["ready_s"] = round(time.perf_counter() - t, 2)
        out["count_polls"] = polls[::max(1, len(polls) // 30)] + polls[-1:]
        out["rows"] = st["totalVectorCount"]
        out["stats"] = {k: st.get(k) for k in ("dimension", "indexFullness", "totalVectorCount", "metric", "vectorType")}
        out["floor_ms"] = floor_ms(pc)
        vecs = [q.tolist() for q in queries]
        js = list(range(len(queries)))
        found = [pc.query(vecs[j]) for j in js]
        out["first_pass_served"] = served_summary(pc.served)
        pc.served.clear()
        out["latency"] = [local.latency(lambda j: pc.query(vecs[j]), js, 1)]
        out["latency"][0]["served"] = served_summary(pc.served[WARM:])
        pc.served.clear()
        out["latency"].append(local.latency(lambda j: pc.query(vecs[j]), js, 4))
        out["latency"][1]["served"] = served_summary(pc.served[WARM:])
        # the same queries again: right after the timed runs, and once the index
        # has stood H2H_PC_SETTLE seconds since the load (serverless indexes
        # build their index in the background, so the first answers after a
        # load may come from another path than the later ones)
        later = [pc.query(vecs[j]) for j in js]
        settle = float(os.environ.get("H2H_PC_SETTLE", "600"))
        wait = settle - (time.perf_counter() - t)
        if wait > 0:
            print(f"pinecone: waiting {wait:.0f} s for the index to settle", flush=True)
            time.sleep(wait)
        out["settled_after_s"] = round(time.perf_counter() - t, 1)
        pc.served.clear()
        settled = [pc.query(vecs[j]) for j in js]
        out["settled_served"] = served_summary(pc.served)
        pc.served.clear()
        out["latency_settled"] = [local.latency(lambda j: pc.query(vecs[j]), js, 1)]
        out["latency_settled"][0]["served"] = served_summary(pc.served[WARM:])
        out["floor_after_ms"] = floor_ms(pc, 50)
        out["same_top10_first_settled"] = round(float(np.mean([a == b for a, b in zip(found, settled)])), 4)
    finally:
        code = pc.delete(INDEX)
        pc.wait_gone(INDEX)
        out["deleted"] = code
        print(f"pinecone: index deleted ({code})", flush=True)
    return out, {"pinecone": settled, "pinecone_first": found, "pinecone_later": later}


def main():
    sizes = sys.argv[1:] or ["100k"]
    os.makedirs(local.WORK, exist_ok=True)
    train, test, digest = local.glove()
    queries = local.unit(test[:local.NQ])
    for size in sizes:
        n = local.SIZES[size] or len(train)
        base = local.unit(train[:n])
        print(f"{size}: {n:,} vectors of {base.shape[1]}, {len(queries):,} queries", flush=True)
        truth = local.exact_top(base, queries)
        res = {"machine": common.machine(), "date": time.strftime("%Y-%m-%d"), "collection": "glove-100-angular",
               "sha256": digest, "vectors": n, "dim": int(base.shape[1]), "queries": len(queries)}
        only = os.environ.get("H2H_ONLY")  # pinecone or chloedb: one half now, the other merged in later
        raw = os.path.join(HERE, "raw", f"results-{size}.json")
        ids = os.path.join(local.WORK, f"hosted-ids-{size}.json")
        if only == "chloedb":
            prev = json.load(open(raw))
            po, pruns = prev["pinecone"], json.load(open(ids))
            res["machine_pinecone_run"], res["date_pinecone_run"] = prev["machine"], prev["date"]
        else:
            po, pruns = run_pinecone(base, queries)
            common.write_json(ids, pruns)
            print("pinecone", json.dumps({k: v for k, v in po.items() if k != "count_polls"})[:900], flush=True)
        if only == "pinecone":
            co, cruns = None, {}
        else:
            co, cruns = local.run_chloedb(base, queries)
            print("chloedb", json.dumps({k: v for k, v in co.items() if k != "plan"})[:600], flush=True)
        runs = {**pruns, **cruns}
        res["recall_at_10"] = {k: local.recall(v, truth) for k, v in runs.items()}
        if cruns:
            res["same_top10"] = round(float(np.mean([a == b for a, b in zip(runs["chloedb"], runs["pinecone"])])), 4)
        res["pinecone"], res["chloedb"] = po, co
        print("recall", res["recall_at_10"], flush=True)
        common.write_json(raw, res)


if __name__ == "__main__":
    main()
