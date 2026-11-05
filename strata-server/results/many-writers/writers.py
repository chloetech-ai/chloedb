"""Many clients writing to one strata-server at once.

Usage: writers.py SERVER_BIN OUT_JSON [SECONDS] [PROCESSES]

Starts the server on a fresh database (durable commits, the server's own
options) and, for each workload and each client count, has that many
clients post small writes back to back for SECONDS, from up to PROCESSES
client processes (a Python process of threads is held to about 3,000
requests a second by its lock). Reports requests per second, client
latency, the server's cores busy, and the commits the writer made in the
window (strata_write_batches_total), so requests per commit shows the
grouping. At the end, the sum of the counters every answered add or
UPDATE added to must equal the number answered.
"""
import http.client
import json
import os
import random
import subprocess
import sys
import tempfile
import threading
import time

BIN, OUT = sys.argv[1:3]
SECS = float(sys.argv[3]) if len(sys.argv) > 3 else 8.0
PROCS = int(sys.argv[4]) if len(sys.argv) > 4 else 4
PORT = 7791
CLIENTS = [1, 4, 16, 64]
KEYS = 1000
WORDS = ("search engine index segment merge vector query ranking token document field table writer "
         "lock model embedding latency memory shard replica snapshot commit flush text keyword filter").split()


def conn():
    return http.client.HTTPConnection("127.0.0.1", PORT, timeout=120)


def post(c, path, body):
    c.request("POST", path, body=json.dumps(body), headers={"content-type": "application/json"})
    r = c.getresponse()
    data = r.read()
    return r.status, data


def metric(name):
    c = conn()
    c.request("GET", "/metrics")
    text = c.getresponse().read().decode()
    for line in text.splitlines():
        if line.startswith(name + " ") or line.startswith(name + "{"):
            return float(line.rsplit(" ", 1)[1])
    return 0.0


def pct(xs, p):
    xs = sorted(xs)
    return xs[min(len(xs) - 1, int(p * len(xs)))] if xs else None


def make_body(kind, rng, tag):
    words = " ".join(rng.choice(WORDS) for _ in range(20))
    if kind == "write_insert":
        return "/v1/write", {"ops": [{"op": "insert", "doc": {"id": tag, "title": words[:40], "body": words, "stock": 0}}]}
    if kind == "sql_insert":
        return "/v1/sql", {"sql": "INSERT INTO docs (id, title, body, stock) VALUES (:k, :t, :b, 0)", "params": {"k": tag, "t": words[:40], "b": words}}
    k = f"k{rng.randrange(KEYS):05d}"
    if kind == "write_add":
        return "/v1/write", {"ops": [{"op": "add", "key": k, "field": "stock", "value": 1}]}
    if kind == "sql_update":
        return "/v1/sql", {"sql": "UPDATE docs SET stock = stock + 1 WHERE id = :k", "params": {"k": k}}
    if kind == "sql_update_one_row":
        return "/v1/sql", {"sql": "UPDATE docs SET stock = stock + 1 WHERE id = 'k00000'"}
    raise ValueError(kind)


def proc_main(kind, clients, ids, stop, q):
    lat, errs, lock = [], {}, threading.Lock()

    def worker(i):
        rng, c, mine, bad, n = random.Random(i * 7919 + clients), conn(), [], {}, 0
        while not stop.is_set():
            path, body = make_body(kind, rng, f"{kind}-{clients}-{i}-{n}")
            n += 1
            t = time.perf_counter()
            try:
                st, data = post(c, path, body)
            except Exception as e:  # noqa: BLE001
                st, c = f"exc {type(e).__name__}", conn()
            ms = (time.perf_counter() - t) * 1e3
            if st == 200:
                mine.append(ms)
            else:
                bad[str(st)] = bad.get(str(st), 0) + 1
        with lock:
            lat.extend(mine)
            for k, v in bad.items():
                errs[k] = errs.get(k, 0) + v

    ts = [threading.Thread(target=worker, args=(i,)) for i in ids]
    [t.start() for t in ts]
    [t.join() for t in ts]
    q.put((lat, errs))


SRV_PID = None


def cpu_s(pid):
    f = open(f'/proc/{pid}/stat').read().rsplit(')', 1)[1].split()
    return (int(f[11]) + int(f[12])) / os.sysconf('SC_CLK_TCK')


def run(kind, clients):
    import multiprocessing as mp
    procs_n = min(clients, PROCS)
    stop, q = mp.Event(), mp.Queue()
    groups = [list(range(p, clients, procs_n)) for p in range(procs_n)]
    b0 = metric("strata_write_batches_total")
    c0 = cpu_s(SRV_PID)
    ps = [mp.Process(target=proc_main, args=(kind, clients, g, stop, q)) for g in groups]
    t0 = time.perf_counter()
    [p.start() for p in ps]
    time.sleep(SECS)
    stop.set()
    lat, errs = [], {}
    for _ in ps:
        l, e = q.get()
        lat.extend(l)
        for k, v in e.items():
            errs[k] = errs.get(k, 0) + v
    [p.join() for p in ps]
    took = time.perf_counter() - t0
    b1 = metric("strata_write_batches_total")
    server_cores = (cpu_s(SRV_PID) - c0) / took
    commits = b1 - b0
    return {"kind": kind, "clients": clients, "server_cores": server_cores, "client_processes": procs_n, "ok": len(lat), "secs": took, "per_sec": len(lat) / took,
            "p50_ms": pct(lat, 0.5), "p99_ms": pct(lat, 0.99), "errors": errs,
            "commits": commits, "commits_per_sec": commits / took, "requests_per_commit": (len(lat) / commits) if commits else None}


def main():
    work = tempfile.mkdtemp(prefix="writers-")
    schema = os.path.join(work, "schema.json")
    with open(schema, "w") as f:
        json.dump({"table": "docs", "key": "id", "fields": [
            {"name": "id", "type": "keyword"},
            {"name": "title", "type": "text"},
            {"name": "body", "type": "text"},
            {"name": "stock", "type": "int"},
        ]}, f)
    log = open(os.path.join(work, "server.log"), "w")
    srv = subprocess.Popen([BIN, "--db", os.path.join(work, "w.strata"), "--schema", schema,
                            "--listen", f"127.0.0.1:{PORT}", "--grpc", "off"], stdout=log, stderr=subprocess.STDOUT)
    global SRV_PID
    SRV_PID = srv.pid
    try:
        for _ in range(600):
            try:
                c = conn()
                c.request("GET", "/v1/health")
                if c.getresponse().status == 200:
                    break
            except OSError:
                pass
            time.sleep(0.1)
        else:
            raise RuntimeError("the server did not start")
        # The rows the counters add to.
        c = conn()
        for s in range(0, KEYS, 500):
            st, data = post(c, "/v1/write", {"ops": [{"op": "insert", "doc": {"id": f"k{k:05d}", "title": "t", "body": "b", "stock": 0}} for k in range(s, s + 500)]})
            assert st == 200, data
        results = []
        for kind in ["write_insert", "sql_insert", "write_add", "sql_update", "sql_update_one_row"]:
            for n in CLIENTS:
                r = run(kind, n)
                results.append(r)
                print(json.dumps(r), flush=True)
        # Every counter add that was answered 200 was applied exactly once.
        c = conn()
        st, data = post(c, "/v1/query", {"sql": "SELECT sum(stock) FROM docs"})
        total = json.loads(data)["rows"][0][0]
        adds = sum(r["ok"] for r in results if r["kind"] in ("write_add", "sql_update", "sql_update_one_row"))
        machine = {"cores": os.cpu_count(), "uname": " ".join(os.uname()), "cpu": next((l.split(":", 1)[1].strip() for l in open("/proc/cpuinfo") if l.startswith("model name")), "?")}
        out = {"machine": machine, "seconds": SECS, "client_processes_max": PROCS, "results": results, "sum_stock": total, "answered_adds": adds, "server": BIN}
        with open(OUT, "w") as f:
            json.dump(out, f, indent=1)
        print("sum(stock)", total, "answered adds", adds)
        assert total == adds, "an answered add was lost or applied twice"
    finally:
        srv.terminate()
        srv.wait()


if __name__ == '__main__':
    main()
