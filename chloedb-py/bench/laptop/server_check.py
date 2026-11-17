"""chloedb-server, two or more builds in turns on one machine: typo search
and one-row durable writes over HTTP, and whether the builds answer alike.

    python server_check.py --bin 0.3.5=/path/chloedb-server --bin fixes=/path/chloedb-server \\
        --data /work/data/beir --collections scifact,nfcorpus,fiqa --work /tmp/sc --out RESULTS_DIR

For each round, each build in turn, each collection:

  typo    the collection's queries with the second and third letters of their
          longest word swapped (common.typo_of, as A2's `typo` kind), sent
          as `ORDER BY bm25(body, :x, syntax => 'plain', fuzzy => 'auto')`
          to a server of the collection's documents (title and text), one
          client: a pass to warm up, then --passes timed passes; p50 and p99
          of the client's time and the server's `took_ms`, and the answers
  writes  on the first collection: --writes inserts of one document a
          request (`POST /v1/write`, each committed and synced before the
          answer), at 1 client and at 16, then a SIGKILL and a restart: every
          document answered as written must be there

Each build's answers to the typo queries must be the first build's, key for
key; the report says how many are.
RESULTS_DIR/server_check.json, and the tables on stdout.
"""

from __future__ import annotations

import argparse
import http.client
import json
import os
import shutil
import signal
import socket
import statistics
import subprocess
import sys
import threading
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from common import typo_of  # noqa: E402

SCHEMA = {"table": "docs", "key": "id", "fields": [{"name": "id", "type": "keyword"}, {"name": "body", "type": "text"}]}
TYPO = "SELECT id FROM docs ORDER BY bm25(body, :x, syntax => 'plain', fuzzy => 'auto') DESC LIMIT 10"


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class Server:
    def __init__(self, binary: str, db: Path, log: Path) -> None:
        self.binary, self.db, self.log = binary, db, log
        self.port = free_port()
        self.proc: subprocess.Popen | None = None

    def start(self) -> None:
        out = open(self.log, "a")
        self.proc = subprocess.Popen([self.binary, "--db", str(self.db), "--listen", f"127.0.0.1:{self.port}", "--answer-cache-mb", "0"], stdout=out, stderr=out)
        for _ in range(600):
            try:
                c = http.client.HTTPConnection("127.0.0.1", self.port, timeout=2)
                c.request("GET", "/v1/health")
                if c.getresponse().status == 200:
                    return
            except OSError:
                pass
            if self.proc.poll() is not None:
                raise SystemExit(f"{self.binary} exited: see {self.log}")
            time.sleep(0.1)
        raise SystemExit(f"{self.binary} did not answer: see {self.log}")

    def stop(self, kill: bool = False) -> None:
        if self.proc and self.proc.poll() is None:
            self.proc.send_signal(signal.SIGKILL if kill else signal.SIGTERM)
            self.proc.wait(60)
        self.proc = None


def post(conn: http.client.HTTPConnection, path: str, body: dict) -> tuple[float, dict]:
    data = json.dumps(body).encode()
    t = time.perf_counter()
    conn.request("POST", path, data, {"content-type": "application/json"})
    r = conn.getresponse()
    raw = r.read()
    ms = (time.perf_counter() - t) * 1000
    if r.status != 200:
        raise RuntimeError(f"{path}: {r.status} {raw[:300]!r}")
    return ms, json.loads(raw)


def pct(v: list[float], p: float) -> float:
    v = sorted(v)
    return v[min(len(v) - 1, int(round(p / 100 * (len(v) - 1))))]


def build_db(binary: str, coll: Path, work: Path) -> Path:
    work.mkdir(parents=True, exist_ok=True)
    rows = work / "rows.jsonl"
    with open(rows, "w") as f:
        for line in open(coll / "docs.jsonl"):
            d = json.loads(line)
            f.write(json.dumps({"id": d["id"], "body": f"{d.get('title', '')} {d.get('text', '')}".strip()}) + "\n")
    schema = work / "schema.json"
    schema.write_text(json.dumps(SCHEMA))
    db = work / "docs.chloedb"
    subprocess.run([binary, "import", "--db", str(db), "--schema", str(schema), str(rows)], check=True, stdout=subprocess.DEVNULL)
    return db


def typo_pass(port: int, qs: list[str]) -> tuple[list[float], list[float], list[list[str]]]:
    conn = http.client.HTTPConnection("127.0.0.1", port, timeout=60)
    client, server, answers = [], [], []
    for q in qs:
        ms, r = post(conn, "/v1/query", {"sql": TYPO, "params": {"x": q}})
        client.append(ms)
        server.append(r["took_ms"])
        answers.append([row[0] for row in r["rows"]])
    return client, server, answers


def writes(port: int, n: int, clients: int, prefix: str) -> dict:
    per = n // clients
    lat: list[list[float]] = [[] for _ in range(clients)]
    errors = [0] * clients

    def go(c: int) -> None:
        conn = http.client.HTTPConnection("127.0.0.1", port, timeout=60)
        for i in range(per):
            doc = {"id": f"{prefix}-{c}-{i}", "body": f"a new document {c} {i} about wireless headphones"}
            try:
                ms, _ = post(conn, "/v1/write", {"ops": [{"op": "insert", "doc": doc}]})
                lat[c].append(ms)
            except (RuntimeError, OSError):
                errors[c] += 1
                conn = http.client.HTTPConnection("127.0.0.1", port, timeout=60)

    t = time.perf_counter()
    ts = [threading.Thread(target=go, args=(c,)) for c in range(clients)]
    for x in ts:
        x.start()
    for x in ts:
        x.join()
    s = time.perf_counter() - t
    all_ms = [m for l in lat for m in l]
    return {"clients": clients, "rows": len(all_ms), "errors": sum(errors), "seconds": s, "rows_per_s": len(all_ms) / s, "p50_ms": pct(all_ms, 50), "p99_ms": pct(all_ms, 99)}


def count(port: int) -> int:
    conn = http.client.HTTPConnection("127.0.0.1", port, timeout=60)
    _, r = post(conn, "/v1/query", {"sql": "SELECT COUNT(*) FROM docs"})
    return int(r["rows"][0][0])


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--bin", action="append", required=True, help="LABEL=PATH, two or more; the first is the one answers are compared with")
    ap.add_argument("--data", required=True)
    ap.add_argument("--collections", default="scifact,nfcorpus,fiqa")
    ap.add_argument("--work", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--rounds", type=int, default=3)
    ap.add_argument("--passes", type=int, default=3)
    ap.add_argument("--writes", type=int, default=2000)
    a = ap.parse_args()
    bins = [tuple(b.split("=", 1)) for b in a.bin]
    colls = a.collections.split(",")
    work = Path(a.work)
    if work.exists():
        shutil.rmtree(work)
    queries = {}
    for c in colls:
        qs = [typo_of(json.loads(l)["text"]) for l in open(Path(a.data) / c / "queries.jsonl")]
        queries[c] = [q for q in qs if q]
    res: dict = {"machine": {"cpu": next((l.split(":", 1)[1].strip() for l in open("/proc/cpuinfo") if l.startswith("model name")), "?"), "cpus": os.cpu_count()}, "builds": {}, "rounds": a.rounds, "passes": a.passes}
    for label, binary in bins:
        v = subprocess.run([binary, "--version"], capture_output=True, text=True).stdout.strip()
        res["builds"][label] = {"binary": binary, "version": v, "typo": {c: [] for c in colls}, "writes": [], "durable": []}
    answers: dict = {}
    for rnd in range(a.rounds):
        for label, binary in bins:
            b = res["builds"][label]
            for ci, c in enumerate(colls):
                d = work / f"{label}-{c}-r{rnd}"
                db = build_db(binary, Path(a.data) / c, d)
                srv = Server(binary, db, d / "server.log")
                srv.start()
                typo_pass(srv.port, queries[c])  # warm
                cl, sv = [], []
                for _ in range(a.passes):
                    x, y, ans = typo_pass(srv.port, queries[c])
                    cl += x
                    sv += y
                answers.setdefault((label, c), ans)
                b["typo"][c].append({"queries": len(queries[c]), "p50_ms": pct(cl, 50), "p99_ms": pct(cl, 99), "server_p50_ms": pct(sv, 50)})
                print(f"r{rnd} {label:8s} {c:9s} typo p50 {pct(cl, 50):.3f} ms (server {pct(sv, 50):.3f})", flush=True)
                if ci == 0:
                    before = count(srv.port)
                    for clients in (1, 16):
                        w = writes(srv.port, a.writes, clients, f"r{rnd}-c{clients}")
                        b["writes"].append(w)
                        print(f"r{rnd} {label:8s} writes, {clients:2d} clients: {w['rows_per_s']:,.0f} rows/s, p50 {w['p50_ms']:.3f} ms, errors {w['errors']}", flush=True)
                    answered = sum(w["rows"] for w in b["writes"][-2:])
                    srv.stop(kill=True)
                    srv.start()
                    after = count(srv.port)
                    b["durable"].append({"before": before, "answered": answered, "after_kill": after, "all_there": after == before + answered})
                    print(f"r{rnd} {label:8s} after SIGKILL: {after} rows, {before} + {answered} answered: {'all there' if after == before + answered else 'MISSING'}", flush=True)
                srv.stop()
                shutil.rmtree(d)
    first = bins[0][0]
    res["same_answers"] = {label: {c: sum(x == y for x, y in zip(answers[(first, c)], answers[(label, c)])) for c in colls} for label, _ in bins}
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    (out / "server_check.json").write_text(json.dumps(res, indent=1))
    med = statistics.median
    print("\n| build | " + " | ".join(f"{c} typo p50" for c in colls) + " | writes 1 client | writes 16 clients | after SIGKILL |")
    print("|---|" + "---|" * (len(colls) + 3))
    for label, _ in bins:
        b = res["builds"][label]
        w1 = med([w["rows_per_s"] for w in b["writes"] if w["clients"] == 1])
        w16 = med([w["rows_per_s"] for w in b["writes"] if w["clients"] == 16])
        ok = all(x["all_there"] for x in b["durable"])
        print(f"| {label} | " + " | ".join(f"{med([t['p50_ms'] for t in b['typo'][c]]):.3f} ms" for c in colls) + f" | {w1:,.0f}/s | {w16:,.0f}/s | {'every row' if ok else 'ROWS MISSING'} |")
    print("\nanswers equal to " + first + "'s: " + json.dumps(res["same_answers"]))


if __name__ == "__main__":
    main()
