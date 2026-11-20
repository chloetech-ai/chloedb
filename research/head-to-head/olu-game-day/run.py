"""Olu's game day: the same three drills on a ChloeDB cluster and on an
Elasticsearch cluster, under the same steady writer and reader.

Olu, an SRE in the persona study, runs Elasticsearch 8 on Kubernetes and
Postgres with Patroni. His adoption answer: ChloeDB "already beats ES by
roughly an order of magnitude in my drills: seconds against the tens of
seconds to minutes I live with". This measures that claim on one machine:

  ChloeDB: two shards, three copies each (`--failover-after-ms 2000`, as the
  guide's "Operate a cluster" page runs them), a coordinator in front.
  Elasticsearch 9.5: three nodes in Docker, each master-eligible and data,
  an index of two primary shards with two replicas each (three copies of a
  shard, as on ChloeDB), the client knowing all three nodes. The nodes listen
  on the host's loopback, as ChloeDB's copies do.

A writer sends one row at a time, 50 a second, each waiting for its answer
(30 s at most) and sending a refused row again every 0.25 s for up to a
minute, as a client does; a reader sends a search ten times a second. Each drill
starts from a healthy cluster:

  kill    SIGKILL the server holding shard 0's primary; start it again 30 s later
  freeze  SIGSTOP it for 20 s (a gray failure: a long GC pause, a hung disk), then SIGCONT
  rolling every server restarted in turn, as an upgrade does, by each system's
          documented procedure (ChloeDB: replicas, a handover, the old primary;
          Elasticsearch: allocation to primaries only, a flush, the node, back to all)

For each drill: writes answered and refused, the longest a write waited, the
longest time with no write answered, the same for searches, how long until
the system said it was whole again, and at the end, whether every answered
write is there.

    python3 run.py

Needs Docker (mirror.gcr.io/library/elasticsearch:9.5.5) and a chloedb-server
build. Raw numbers go to raw/results.json.
"""

import json
import os
import random
import secrets
import shutil
import signal
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import common  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
WORK = os.environ.get("H2H_WORK", "/tmp/h2h-olu")
ES_IMAGE = os.environ.get("H2H_ES_IMAGE", "mirror.gcr.io/library/elasticsearch:9.5.5")
ES_HEAP = os.environ.get("H2H_ES_HEAP", "1g")
WRITES_PER_S = 50
READS_PER_S = 10
TIMEOUT_S = 30
RETRY_FOR_S = 60
STEADY_S = 10
KILL_DOWN_S = 30
FREEZE_S = 20
PRELOAD = 5000
WORDS = ("sofa table lamp chair desk shelf rug bed mirror stool cabinet bench clock vase "
         "oak pine steel glass wool linen velvet rattan walnut cherry red blue green grey").split()


def body(rng):
    return " ".join(rng.choice(WORDS) for _ in range(8))


def http(method, url, data=None, headers=None, timeout=TIMEOUT_S):
    """(status, json or text); status 0 for no answer."""
    raw = None if data is None else (data if isinstance(data, bytes) else json.dumps(data).encode())
    r = urllib.request.Request(url, data=raw, method=method)
    r.add_header("content-type", "application/json")
    for k, v in (headers or {}).items():
        r.add_header(k, v)
    try:
        with urllib.request.urlopen(r, timeout=timeout) as resp:
            txt = resp.read()
            return resp.status, (json.loads(txt) if txt else None)
    except urllib.error.HTTPError as e:
        txt = e.read().decode(errors="replace")
        try:
            return e.code, json.loads(txt)
        except ValueError:
            return e.code, txt[:300]
    except (urllib.error.URLError, ConnectionError, OSError, TimeoutError) as e:
        return 0, str(e)[:200]


def until(fn, limit=600, step=0.25):
    t = time.monotonic()
    while time.monotonic() - t < limit:
        try:
            if fn():
                return round(time.monotonic() - t, 2)
        except Exception:  # noqa: BLE001 - a check that fails is a check not yet true
            pass
        time.sleep(step)
    return None


# ---------------------------------------------------------------- ChloeDB

class ChloeCluster:
    name = "chloedb"

    def __init__(self, folder):
        if os.path.exists(folder):
            shutil.rmtree(folder)
        os.makedirs(folder)
        self.folder = folder
        self.bin = common.server_bin()
        self.version = subprocess.run([self.bin, "--version"], capture_output=True, text=True).stdout.strip()
        self.env = {**os.environ, "CHLOEDB_CLUSTER_TOKEN": secrets.token_hex(32)}
        self.admin = secrets.token_hex(32)
        self.copies = {}  # name -> {"http", "grpc", "proc"}
        for s in (0, 1):
            for c in "abc":
                self.copies[f"s{s}{c}"] = {"http": common.free_port(), "grpc": common.free_port(), "proc": None}
        g = lambda n: f"http://127.0.0.1:{self.copies[n]['grpc']}"  # noqa: E731
        for s in (0, 1):
            names = [f"s{s}{c}" for c in "abc"]
            self.start(names[0], ["--replicas", ",".join(g(n) for n in names[1:])])
            for n in names[1:]:
                self.start(n, ["--replica", "--replicas", ",".join(g(m) for m in names if m != n)])
        self.port = common.free_port()
        self.url = f"http://127.0.0.1:{self.port}"
        shards = ",".join("|".join(g(f"s{s}{c}") for c in "abc") for s in (0, 1))
        self.coord = subprocess.Popen([self.bin, "--listen", f"127.0.0.1:{self.port}", "--grpc", "off", "--node-name", "coord",
                                       "--start-apps", "off", "--shards", shards],
                                      env={**self.env, "CHLOEDB_TOKEN": self.admin},
                                      stdout=open(os.path.join(folder, "coord.log"), "a"), stderr=subprocess.STDOUT)
        self.auth = {"authorization": f"Bearer {self.admin}"}
        if until(self.whole, 120) is None:
            raise SystemExit(f"the cluster did not come up: see {folder}")

    def start(self, n, role=()):
        c = self.copies[n]
        d = os.path.join(self.folder, n)
        os.makedirs(os.path.join(d, "backups"), exist_ok=True)
        c["proc"] = subprocess.Popen([self.bin, "--db", os.path.join(d, "db.chloedb"), "--seq-no", "--failover-after-ms", "2000",
                                      "--listen", f"127.0.0.1:{c['http']}", "--grpc", f"127.0.0.1:{c['grpc']}", "--node-name", n,
                                      "--backup-dir", os.path.join(d, "backups"), "--start-apps", "off", *role],
                                     env=self.env, stdout=open(os.path.join(d, "server.log"), "a"), stderr=subprocess.STDOUT)

    def cluster(self):
        st, c = http("GET", self.url + "/v1/cluster", headers=self.auth, timeout=5)
        return c if st == 200 else None

    def whole(self):
        c = self.cluster()
        return bool(c) and all(s["state"] == "ok" for s in c["shards"]) and all(
            r["status"] == "ok" and r["serves_reads"] for s in c["shards"] for r in s["replicas"])

    def _name(self, r):
        return r.get("name") or r.get("short")

    def primary(self, shard=0):
        c = self.cluster()
        return [self._name(r) for r in c["shards"][shard]["replicas"] if r["role"] == "primary" and r["status"] == "ok"][0]

    def replicas(self, shard):
        c = self.cluster()
        return [self._name(r) for r in c["shards"][shard]["replicas"] if r["role"] == "replica"]

    def setup(self):
        st, r = http("POST", self.url + "/v1/sql", {"sql": "CREATE TABLE gd (id KEYWORD PRIMARY KEY, body TEXT, n INT)"}, self.auth)
        assert st == 200, r

    def write(self, docs):
        ops = [{"op": "upsert", "table": "gd", "doc": d} for d in docs]
        st, r = http("POST", self.url + "/v1/write", {"ops": ops, "idempotency_key": docs[0]["id"] + f"x{len(docs)}"}, self.auth)
        return st == 200 and isinstance(r, dict) and r.get("written") == len(docs), (st, r if st != 200 else None)

    def search(self, word):
        st, r = http("POST", self.url + "/v1/sql",
                     {"sql": "SELECT id FROM gd WHERE MATCH(body, :w) ORDER BY bm25(body, :w) DESC LIMIT 10", "params": {"w": word}},
                     self.auth)
        return st == 200 and "rows" in (r or {})

    def present(self, ids):
        got = set()
        for i in range(0, len(ids), 500):
            st, r = http("POST", self.url + "/v1/sql", {"sql": "SELECT id FROM gd WHERE id IN (" + ",".join(f"'{x}'" for x in ids[i:i + 500]) + ") LIMIT 500"},
                         self.auth, timeout=60)
            assert st == 200, r
            got |= {row[0] for row in r["rows"]}
        return got

    def count(self):
        st, r = http("POST", self.url + "/v1/sql", {"sql": "SELECT COUNT(*) FROM gd"}, self.auth)
        return r["rows"][0][0]

    # the drills
    def kill(self, n):
        self.copies[n]["proc"].send_signal(signal.SIGKILL)
        self.copies[n]["proc"].wait()

    def revive(self, n):
        self.start(n)

    def freeze(self, n):
        self.copies[n]["proc"].send_signal(signal.SIGSTOP)

    def thaw(self, n):
        self.copies[n]["proc"].send_signal(signal.SIGCONT)

    def restart(self, n):
        p = self.copies[n]["proc"]
        p.terminate()
        p.wait(60)
        self.start(n)
        c = self.copies[n]
        # a copy shows its replication only to a credential it takes: the cluster's token, not the coordinator's admin one
        copy_auth = {"authorization": f"Bearer {self.env['CHLOEDB_CLUSTER_TOKEN']}"}

        def back():
            r = (http("GET", f"http://127.0.0.1:{c['http']}/v1/health", headers=copy_auth, timeout=2)[1] or {}).get("replication") or {}
            return r.get("serves_reads") or r.get("role") == "primary"
        if until(back, 300) is None or until(self.whole, 300) is None:
            raise SystemExit(f"{n} did not come back within 300 s: see {self.folder}")

    def rolling(self, log):
        for shard in (0, 1):
            primary, reps = self.primary(shard), self.replicas(shard)
            for r in reps:
                t = time.monotonic()
                self.restart(r)
                log.append({"step": f"restart {r}", "s": round(time.monotonic() - t, 2)})
            t = time.monotonic()
            st, _ = http("POST", self.url + f"/v1/admin/shards/{shard}/promote", {"url": f"http://127.0.0.1:{self.copies[reps[0]]['grpc']}"}, self.auth)
            until(lambda: self.primary(shard) == reps[0], 60)
            log.append({"step": f"handover shard {shard} to {reps[0]}", "s": round(time.monotonic() - t, 2), "status": st})
            t = time.monotonic()
            self.restart(primary)
            log.append({"step": f"restart {primary}", "s": round(time.monotonic() - t, 2)})

    def memory_kb(self):
        return common.pss_kb([c["proc"].pid for c in self.copies.values()] + [self.coord.pid])

    def stop(self):
        for c in self.copies.values():
            try:
                c["proc"].send_signal(signal.SIGCONT)
                c["proc"].terminate()
                c["proc"].wait(30)
            except Exception:  # noqa: BLE001
                pass
        self.coord.terminate()
        self.coord.wait(30)


# ---------------------------------------------------------------- Elasticsearch

class EsCluster:
    name = "elasticsearch"
    NODES = ("es1", "es2", "es3")

    def __init__(self):
        subprocess.run(["docker", "rm", "-f", *self.NODES], capture_output=True)
        # The three nodes share the host's loopback, as ChloeDB's copies do: on a network address Elasticsearch enforces
        # its production checks, and this container's file limit (20,000) is below the 65,535 they ask for.
        self.ports = {n: common.free_port() for n in self.NODES}
        transport = {n: common.free_port() for n in self.NODES}
        seeds = ",".join(f"127.0.0.1:{transport[n]}" for n in self.NODES)
        for n in self.NODES:
            subprocess.run(["docker", "run", "-d", "--name", n, "--network", "host",
                            "-e", "network.host=127.0.0.1", "-e", f"http.port={self.ports[n]}", "-e", f"transport.port={transport[n]}",
                            "-e", f"node.name={n}", "-e", "cluster.name=h2h", "-e", f"discovery.seed_hosts={seeds}",
                            "-e", "cluster.initial_master_nodes=es1,es2,es3", "-e", "xpack.security.enabled=false",
                            "-e", f"ES_JAVA_OPTS=-Xms{ES_HEAP} -Xmx{ES_HEAP}", ES_IMAGE], check=True, capture_output=True)
        self.order = list(self.NODES)
        if until(lambda: self.health().get("number_of_nodes") == 3, 300) is None:
            raise SystemExit("Elasticsearch did not form a cluster")
        st, info = self.req("GET", "/")
        self.version = f"Elasticsearch {info['version']['number']} (Docker {ES_IMAGE}, three nodes, heap {ES_HEAP} each)"

    def req(self, method, path, data=None, timeout=TIMEOUT_S):
        """The client knows every node: a node that does not answer is skipped, as a client's node list does."""
        last = (0, "no node answered")
        for n in list(self.order):
            st, r = http(method, f"http://127.0.0.1:{self.ports[n]}{path}", data, timeout=timeout)
            if st != 0:
                return st, r
            last = (st, r)
            self.order.remove(n)
            self.order.append(n)
        return last

    def health(self):
        st, r = self.req("GET", "/_cluster/health", timeout=5)
        return r if st == 200 else {}

    def whole(self):
        h = self.health()
        return h.get("status") == "green" and h.get("number_of_nodes") == 3

    def setup(self):
        st, r = self.req("PUT", "/gd", {"settings": {"number_of_shards": 2, "number_of_replicas": 2},
                                        "mappings": {"properties": {"id": {"type": "keyword"}, "body": {"type": "text"}, "n": {"type": "integer"}}}})
        assert st == 200, r
        until(self.whole, 120)

    def primary(self, shard=0):
        st, r = self.req("GET", "/_cat/shards/gd?format=json&h=shard,prirep,node,state")
        return [x["node"] for x in r if x["shard"] == str(shard) and x["prirep"] == "p" and x["state"] == "STARTED"][0]

    def master(self):
        st, r = self.req("GET", "/_cat/master?format=json")
        return r[0]["node"]

    def write(self, docs):
        if len(docs) == 1:
            d = docs[0]
            st, r = self.req("PUT", f"/gd/_doc/{d['id']}?timeout={TIMEOUT_S}s", d)
            return st in (200, 201), (st, r if st not in (200, 201) else None)
        lines = []
        for d in docs:
            lines += [json.dumps({"index": {"_index": "gd", "_id": d["id"]}}), json.dumps(d)]
        st, r = self.req("POST", "/_bulk", ("\n".join(lines) + "\n").encode())
        return st == 200 and not r.get("errors"), (st, None)

    def search(self, word):
        st, r = self.req("POST", "/gd/_search", {"size": 10, "_source": False, "query": {"match": {"body": word}}})
        return st == 200 and "hits" in (r or {})

    def present(self, ids):
        self.req("POST", "/gd/_refresh")
        got = set()
        for i in range(0, len(ids), 1000):
            st, r = self.req("POST", "/gd/_search", {"size": 1000, "_source": False, "query": {"ids": {"values": ids[i:i + 1000]}}}, timeout=60)
            assert st == 200, r
            got |= {h["_id"] for h in r["hits"]["hits"]}
        return got

    def count(self):
        self.req("POST", "/gd/_refresh")
        return self.req("GET", "/gd/_count")[1]["count"]

    def kill(self, n):
        subprocess.run(["docker", "kill", "--signal", "KILL", n], capture_output=True)

    def revive(self, n):
        subprocess.run(["docker", "start", n], capture_output=True)

    def freeze(self, n):
        subprocess.run(["docker", "pause", n], capture_output=True)

    def thaw(self, n):
        subprocess.run(["docker", "unpause", n], capture_output=True)

    def rolling(self, log):
        # Elastic's rolling restart: primaries-only allocation, a flush, restart the node, allocation back, wait for green
        for n in self.NODES:
            t = time.monotonic()
            self.req("PUT", "/_cluster/settings", {"persistent": {"cluster.routing.allocation.enable": "primaries"}})
            self.req("POST", "/_flush")
            subprocess.run(["docker", "restart", "-t", "60", n], capture_output=True)
            until(lambda: self.health().get("number_of_nodes") == 3, 300)
            self.req("PUT", "/_cluster/settings", {"persistent": {"cluster.routing.allocation.enable": None}})
            until(self.whole, 300)
            log.append({"step": f"restart {n}", "s": round(time.monotonic() - t, 2)})

    def memory_kb(self):
        total = 0
        for n in self.NODES:
            s = subprocess.run(["docker", "stats", "--no-stream", "--format", "{{.MemUsage}}", n], capture_output=True, text=True).stdout
            v = s.split("/")[0].strip()
            for u, k in {"KiB": 1, "MiB": 1024, "GiB": 1024 * 1024}.items():
                if v.endswith(u):
                    total += int(float(v[:-len(u)]) * k)
        return total

    def stop(self):
        subprocess.run(["docker", "rm", "-f", *self.NODES], capture_output=True)
        subprocess.run(["docker", "network", "rm", "h2h-es-net"], capture_output=True)


# ---------------------------------------------------------------- the load

class Load:
    """A writer, 50 rows a second, one at a time, each waiting for its answer;
    a reader, ten searches a second. Every request is logged with its times."""

    def __init__(self, system, prefix, seed):
        self.system, self.prefix = system, prefix
        self.rng = random.Random(seed)
        self.writes, self.reads = [], []
        self.stop_ev = threading.Event()
        self.threads = [threading.Thread(target=self._write, daemon=True), threading.Thread(target=self._read, daemon=True)]

    def _write(self):
        i, nxt = 0, time.monotonic()
        while not self.stop_ev.is_set():
            d = {"id": f"{self.prefix}-{i:06d}", "body": body(self.rng), "n": i}
            t0 = time.monotonic()
            # as a client does: a refused write is sent again, the same row with the same id
            # (and on ChloeDB the same idempotency key), every 0.25 s, for up to a minute
            tries, errs = 0, []
            while True:
                tries += 1
                ok, err = self.system.write([d])
                if ok or time.monotonic() - t0 > RETRY_FOR_S or self.stop_ev.is_set():
                    break
                errs.append(str(err)[:400])
                time.sleep(0.25)
            t1 = time.monotonic()
            self.writes.append({"id": d["id"], "t0": t0, "t1": t1, "ok": ok, "tries": tries,
                                "err": errs[0] if errs else None, "last_err": errs[-1] if errs else None,
                                "final_err": None if ok else str(err)[:400]})
            i += 1
            nxt += 1 / WRITES_PER_S
            time.sleep(max(0.0, nxt - time.monotonic()))
            nxt = max(nxt, time.monotonic() - 1)

    def _read(self):
        nxt = time.monotonic()
        while not self.stop_ev.is_set():
            w = self.rng.choice(WORDS)
            t0 = time.monotonic()
            ok = self.system.search(w)
            self.reads.append({"t0": t0, "t1": time.monotonic(), "ok": ok})
            nxt += 1 / READS_PER_S
            time.sleep(max(0.0, nxt - time.monotonic()))
            nxt = max(nxt, time.monotonic() - 1)

    def __enter__(self):
        for t in self.threads:
            t.start()
        return self

    def __exit__(self, *exc):
        self.stop_ev.set()
        for t in self.threads:
            t.join(TIMEOUT_S + 5)


def summary(log, t_start, t_end):
    """What a client saw between t_start and t_end."""
    # every request open in the window, so that one sent just before it and held through it counts
    xs = [x for x in log if x["t1"] >= t_start and x["t0"] <= t_end]
    if not xs:
        return {}
    ok = [x for x in xs if x["ok"]]
    lat = [(x["t1"] - x["t0"]) * 1000 for x in xs]
    # the longest time with no request answered well: between two good answers, or from t_start
    ends = sorted([t_start] + [x["t1"] for x in ok] + [t_end])
    gap = max(b - a for a, b in zip(ends, ends[1:]))
    errs = {}
    for x in xs:
        if x.get("err"):
            k = x["err"][:100]
            errs[k] = errs.get(k, 0) + 1
    extra = {}
    if "tries" in xs[0]:
        extra = {"sent_again": sum(1 for x in xs if x["tries"] > 1), "tries": sum(x["tries"] for x in xs)}
    return {"sent": len(xs), "ok": len(ok), "refused": len(xs) - len(ok), **extra, "longest_wait_s": round(max(lat) / 1000, 2),
            "longest_gap_s": round(gap, 2), **{k: v for k, v in common.percentiles(lat).items() if k != "n"},
            "p99_ms": round(sorted(lat)[min(len(lat) - 1, int(len(lat) * 0.99))], 1),
            "errors": dict(sorted(errs.items(), key=lambda kv: -kv[1])[:4])}


def drill(system, kind, seed):
    """One drill under load; what the clients saw and how long the system took to be whole."""
    out = {"drill": kind}
    target = system.primary(0)
    out["target"] = target
    if system.name == "elasticsearch":
        out["target_was_master"] = system.master() == target
    with Load(system, f"{kind}", seed) as load:
        time.sleep(STEADY_S)
        t_ev = time.monotonic()
        steps = []
        if kind == "kill":
            system.kill(target)
            time.sleep(KILL_DOWN_S)
            system.revive(target)
        elif kind == "freeze":
            system.freeze(target)
            time.sleep(FREEZE_S)
            system.thaw(target)
        else:
            system.rolling(steps)
        t_back = time.monotonic()
        whole = until(system.whole, 600)
        out["event_s"] = round(t_back - t_ev, 2)
        out["whole_after_s"] = whole  # after the server was started again / thawed / the last restart
        if steps:
            out["steps"] = steps
        time.sleep(STEADY_S)
        t_end = time.monotonic()
    out["before"] = summary(load.writes, t_ev - STEADY_S, t_ev)
    out["writes"] = summary(load.writes, t_ev, t_end)
    out["reads"] = summary(load.reads, t_ev, t_end)
    out["steady_reads"] = summary(load.reads, t_ev - STEADY_S, t_ev)
    # the writes that were sent more than once, or held longest: what the client was told
    held = sorted((w for w in load.writes if w["t1"] >= t_ev and w["t0"] <= t_end), key=lambda w: w["t0"] - w["t1"])[:3]
    out["held_longest"] = [{"id": w["id"], "at_s": round(w["t0"] - t_ev, 2), "wait_s": round(w["t1"] - w["t0"], 2), "ok": w["ok"],
                            "tries": w["tries"], "first_err": w["err"], "last_err": w["last_err"]} for w in held]
    return out, load.writes


def run(system):
    res = {"version": system.version}
    system.setup()
    rng = random.Random(3)
    docs = [{"id": f"pre-{i:06d}", "body": body(rng), "n": i} for i in range(PRELOAD)]
    t = time.perf_counter()
    for i in range(0, PRELOAD, 500):
        ok, err = system.write(docs[i:i + 500])
        assert ok, err
    res["preload_s"] = round(time.perf_counter() - t, 2)
    res["memory_kb"] = system.memory_kb()
    res["drills"] = []
    all_writes = []
    for k, kind in enumerate(("kill", "freeze", "rolling")):
        until(system.whole, 600)
        d, writes = drill(system, kind, 10 + k)
        print(system.name, kind, json.dumps({x: d[x] for x in ("target", "event_s", "whole_after_s")}),
              json.dumps(d["writes"])[:300], flush=True)
        res["drills"].append(d)
        all_writes += writes
    until(system.whole, 600)
    acked = [w["id"] for w in all_writes if w["ok"]]
    refused = [w["id"] for w in all_writes if not w["ok"]]
    got = system.present(acked + refused)
    res["writes_answered"] = len(acked)
    res["writes_refused"] = len(refused)
    res["answered_but_missing"] = sorted(set(acked) - got)
    res["refused_but_there"] = len(set(refused) & got)
    res["rows"] = system.count()
    res["rows_expected"] = PRELOAD + len(acked) + res["refused_but_there"]
    return res


def main():
    os.makedirs(WORK, exist_ok=True)
    res = {"at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "machine": common.machine(),
           "writes_per_s": WRITES_PER_S, "reads_per_s": READS_PER_S, "timeout_s": TIMEOUT_S, "kill_down_s": KILL_DOWN_S,
           "freeze_s": FREEZE_S, "steady_s": STEADY_S, "preload": PRELOAD}
    out = os.path.join(HERE, "raw", "results.json")
    only = os.environ.get("H2H_ONLY")  # chloedb or elasticsearch: run that one, keep the other's from the last run
    if only and os.path.exists(out):
        res = {**json.load(open(out)), **{k: v for k, v in res.items()}}
    for name, make in (("chloedb", lambda: ChloeCluster(os.path.join(WORK, "chloedb"))), ("elasticsearch", EsCluster)):
        if only and only != name:
            continue
        system = make()
        try:
            res[name] = run(system)
        finally:
            system.stop()
        common.write_json(out, res)
        print(f"wrote raw/results.json ({name})", flush=True)


if __name__ == "__main__":
    main()
