"""Online lookup over HTTP at 1, 4 and 16 clients: Feast 0.68 feature server (`feast serve -w 4`, Redis online store)
vs chloedb-server /v1/kv/get (two calls). 10 features for 1,000 (user, item) pairs a request.
Usage: serve.py OUT.json"""
import sys, json, time, threading, statistics, http.client
import numpy as np
UF = ["clicks_7d", "buys_30d", "avg_price", "sessions_7d", "dwell_s"]; IF = ["ctr_7d", "views_1d", "price", "stock", "rating"]
rng = np.random.default_rng(1)
batches = [[(f"u{u}", f"i{i}") for u, i in zip(rng.integers(0, 20000, 1000), rng.integers(0, 10000, 1000))] for _ in range(200)]
class Http:
    def __init__(self, port): self.c = http.client.HTTPConnection("127.0.0.1", port, timeout=60)
    def post(self, path, body):
        self.c.request("POST", path, json.dumps(body), {"content-type": "application/json"})
        r = self.c.getresponse(); d = r.read()
        assert r.status == 200, d[:200]
        return json.loads(d)
def feast(h, b):
    return h.post("/get-online-features", {"features": [f"user_stats:{c}" for c in UF] + [f"item_stats:{c}" for c in IF],
                                           "entities": {"u_id": [u for u, _ in b], "i_id": [i for _, i in b]}})
def chloedb(h, b):
    return (h.post("/v1/kv/get", {"table": "users_latest", "keys": [u for u, _ in b], "columns": UF}),
            h.post("/v1/kv/get", {"table": "items_latest", "keys": [i for _, i in b], "columns": IF}))
res = {}
for name, port, f in [("feast_server_redis", 6566, feast), ("chloedb_kv", 7790, chloedb)]:
    # same values for the first batch
    for clients in (1, 4, 16):
        lat, stop, n = [], time.time() + 8, [0]
        lock = threading.Lock()
        def worker(k):
            h = Http(port)
            for b in batches[:5]: f(h, b)  # warm-up
            i = k
            while time.time() < stop:
                t = time.perf_counter(); f(h, batches[i % len(batches)]); dt = (time.perf_counter() - t) * 1000
                with lock: lat.append(dt)
                i += clients
        ths = [threading.Thread(target=worker, args=(k,)) for k in range(clients)]
        t0 = time.time(); [t.start() for t in ths]; [t.join() for t in ths]
        lat.sort()
        r = {"requests": len(lat), "per_s": round(len(lat) / (time.time() - t0), 1), "p50_ms": round(statistics.median(lat), 2), "p99_ms": round(lat[max(0, int(len(lat) * 0.99) - 1)], 2)}
        res[f"{name}@{clients}"] = r; print(name, clients, r, flush=True)
F = feast(Http(6566), batches[0]); C = chloedb(Http(7790), batches[0])
names = F["metadata"]["feature_names"]; fv = {n: F["results"][k]["values"] for k, n in enumerate(names)}
res["same_values"] = all(np.allclose(np.array(fv[c], float), np.array([r[c] if r else np.nan for r in C[0]["rows"]], float), equal_nan=True) for c in UF) and \
    all(np.allclose(np.array(fv[c], float), np.array([r[c] if r else np.nan for r in C[1]["rows"]], float), equal_nan=True) for c in IF)
print("same values", res["same_values"]); json.dump(res, open(sys.argv[1], "w"), indent=1)
