"""What every head-to-head shares: start a chloedb-server or a Postgres on
this machine, send them SQL, time a query, and read memory and disk.

Each comparison runs on one machine, both systems in turn, and writes its raw
numbers to `raw/` beside it; its `results.md` names the machine. Nothing here
reaches the network but `127.0.0.1`.
"""

import json
import os
import platform
import shutil
import socket
import statistics
import subprocess
import time
import urllib.error
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))


def server_bin():
    """chloedb-server: CHLOEDB_SERVER_BIN, else this checkout's build."""
    env = os.environ.get("CHLOEDB_SERVER_BIN")
    if env:
        return env
    for profile in ("release", "ci"):
        p = os.path.join(REPO, "chloedb-server", "target", profile, "chloedb-server")
        if os.path.exists(p):
            return p
    found = shutil.which("chloedb-server")
    if found:
        return found
    raise SystemExit("no chloedb-server: build it (cd chloedb-server && cargo build --profile ci) or set CHLOEDB_SERVER_BIN")


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def machine():
    """The machine a number came from, for results.md."""
    cpu = platform.processor() or platform.machine()
    try:
        with open("/proc/cpuinfo") as f:
            for line in f:
                if line.startswith("model name"):
                    cpu = line.split(":", 1)[1].strip()
                    break
    except OSError:
        pass
    mem = 0
    try:
        with open("/proc/meminfo") as f:
            mem = int(f.readline().split()[1]) // 1024
    except OSError:
        pass
    return {"cpu": cpu, "cores": os.cpu_count(), "memory_mb": mem, "os": f"{platform.system()} {platform.release()}"}


def pss_kb(pids):
    """Proportional set size of these processes, summed (shared pages split
    between them, so a Postgres's shared buffers count once)."""
    total = 0
    for pid in pids:
        try:
            with open(f"/proc/{pid}/smaps_rollup") as f:
                for line in f:
                    if line.startswith("Pss:"):
                        total += int(line.split()[1])
        except OSError:
            pass
    return total


def tree(pid):
    """pid and every process under it."""
    out = [pid]
    try:
        kids = subprocess.run(["pgrep", "-P", str(pid)], capture_output=True, text=True).stdout.split()
    except OSError:
        kids = []
    for k in kids:
        out += tree(int(k))
    return out


def du_bytes(path):
    total = 0
    for root, _, files in os.walk(path):
        for name in files:
            try:
                total += os.path.getsize(os.path.join(root, name))
            except OSError:
                pass
    return total


def percentiles(ms):
    ms = sorted(ms)
    q = statistics.quantiles(ms, n=100, method="inclusive") if len(ms) > 1 else [ms[0]] * 99
    return {"p50_ms": round(q[49], 3), "p95_ms": round(q[94], 3), "n": len(ms)}


class Chloedb:
    """A chloedb-server on a fresh folder, talked to over its HTTP JSON API.
    Its answer cache is off: each comparison repeats a query to time it, and
    the others it is compared with keep no answers."""

    def __init__(self, folder, extra=()):
        os.makedirs(folder, exist_ok=True)
        self.folder = folder
        self.port = free_port()
        self.url = f"http://127.0.0.1:{self.port}"
        args = [server_bin(), "--db", os.path.join(folder, "db.chloedb"), "--listen", f"127.0.0.1:{self.port}",
                "--grpc", "off", "--start-apps", "off", "--answer-cache-mb", "0", *extra]
        self.log = open(os.path.join(folder, "server.log"), "w")
        self.proc = subprocess.Popen(args, stdout=self.log, stderr=subprocess.STDOUT)
        for _ in range(300):
            try:
                urllib.request.urlopen(self.url + "/v1/health", timeout=1).read()
                break
            except (urllib.error.URLError, ConnectionError, OSError):
                time.sleep(0.1)
        else:
            raise SystemExit(f"chloedb-server did not start: see {folder}/server.log")
        self.key = self.call("/v1/admin/keys", {"name": "h2h", "scope": "admin"})["secret"]
        self.version = subprocess.run([server_bin(), "--version"], capture_output=True, text=True).stdout.strip()

    def call(self, path, body=None, method=None):
        data = None if body is None else json.dumps(body).encode()
        req = urllib.request.Request(self.url + path, data=data, method=method or ("POST" if data else "GET"))
        req.add_header("content-type", "application/json")
        if getattr(self, "key", None):
            req.add_header("authorization", f"Bearer {self.key}")
        try:
            with urllib.request.urlopen(req, timeout=600) as r:
                return json.loads(r.read() or b"null")
        except urllib.error.HTTPError as e:
            return {"error": e.read().decode(errors="replace"), "status": e.code}

    def sql(self, sql, params=None):
        body = {"sql": sql}
        if params:
            body["params"] = params
        return self.call("/v1/sql", body)

    def write(self, table, docs, batch=2000):
        for i in range(0, len(docs), batch):
            ops = [{"op": "upsert", "table": table, "doc": d} for d in docs[i:i + batch]]
            r = self.call("/v1/write", {"ops": ops})
            if "error" in r:
                raise SystemExit(f"write failed: {r}")

    def memory_kb(self):
        return pss_kb([self.proc.pid])

    def disk_bytes(self):
        return sum(du_bytes(os.path.join(self.folder, f)) if os.path.isdir(os.path.join(self.folder, f))
                   else os.path.getsize(os.path.join(self.folder, f))
                   for f in os.listdir(self.folder) if f.startswith("db.chloedb"))

    def stop(self):
        self.proc.terminate()
        self.proc.wait(30)
        self.log.close()


class Postgres:
    """A Postgres on a fresh folder (initdb), run as the `postgres` user when
    this script runs as root, talked to with psycopg."""

    def __init__(self, folder, bindir=None, conf=()):
        import psycopg
        self.bindir = bindir or os.environ.get("PG_BINDIR") or self._find_bindir()
        self.folder = folder
        self.port = free_port()
        self.as_user = ["runuser", "-u", "postgres", "--"] if os.geteuid() == 0 else []
        if os.path.exists(folder):
            shutil.rmtree(folder)
        os.makedirs(folder)
        if self.as_user:
            shutil.chown(folder, "postgres", "postgres")
        data = os.path.join(folder, "data")
        subprocess.run(self.as_user + [os.path.join(self.bindir, "initdb"), "-D", data, "-A", "trust", "-U", "postgres",
                                       "--no-locale", "-E", "UTF8"], check=True, capture_output=True)
        with open(os.path.join(data, "postgresql.conf"), "a") as f:
            f.write(f"\nport = {self.port}\nlisten_addresses = '127.0.0.1'\nunix_socket_directories = '{folder}'\n")
            for line in conf:
                f.write(line + "\n")
        self.proc = subprocess.Popen(self.as_user + [os.path.join(self.bindir, "postgres"), "-D", data],
                                     stdout=open(os.path.join(folder, "pg.log"), "w"), stderr=subprocess.STDOUT)
        for _ in range(300):
            try:
                self.conn = psycopg.connect(host="127.0.0.1", port=self.port, user="postgres", dbname="postgres", autocommit=True)
                break
            except psycopg.OperationalError:
                time.sleep(0.1)
        else:
            raise SystemExit(f"postgres did not start: see {folder}/pg.log")
        self.version = self.conn.execute("SELECT version()").fetchone()[0]
        self.data = data

    @staticmethod
    def _find_bindir():
        for v in ("18", "17", "16", "15"):
            p = f"/usr/lib/postgresql/{v}/bin"
            if os.path.exists(os.path.join(p, "postgres")):
                return p
        pg = shutil.which("postgres")
        if pg:
            return os.path.dirname(pg)
        raise SystemExit("no Postgres: install it or set PG_BINDIR")

    def memory_kb(self):
        # The postmaster's process tree runs as another user under runuser:
        # find the postmaster by its data folder.
        pids = subprocess.run(["pgrep", "-f", f"postgres -D {self.data}"], capture_output=True, text=True).stdout.split()
        out = []
        for p in pids:
            out += tree(int(p))
        return pss_kb(sorted(set(out)))

    def stop(self):
        self.conn.close()
        subprocess.run(self.as_user + [os.path.join(self.bindir, "pg_ctl"), "-D", self.data, "stop", "-m", "fast"],
                       capture_output=True)
        self.proc.wait(30)


def timed(fn, reps):
    """fn() run reps times; the wall-clock time of each, in milliseconds."""
    out = []
    for _ in range(reps):
        t = time.perf_counter()
        fn()
        out.append((time.perf_counter() - t) * 1000)
    return out


def write_json(path, obj):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        json.dump(obj, f, indent=1, ensure_ascii=False)
        f.write("\n")
