"""Vector search: ChloeDB against Pinecone Local, on GloVe-100 from ann-benchmarks.

Asked directly (not by a persona of the study): "did we ever measure anything
against Pinecone? Can we try it locally, especially the vector search?"
Pinecone runs only as a hosted service; the one thing of it that runs on a
machine is Pinecone Local (`ghcr.io/pinecone-io/pinecone-local`), Pinecone's
in-memory emulator of its API for development and tests. This puts the two
side by side on the standard ANN collection both can take:

  1. GloVe-100 (ann-benchmarks' glove-100-angular: 1,183,514 word vectors of
     100 dimensions, 10,000 test queries), the vectors scaled to unit length
     so that cosine and the dot product agree; both systems get the same
     vectors. Two sizes: the first 100,000 vectors (Pinecone Local's
     documented limit per index) and all 1,183,514.
  2. Pinecone Local (Docker, an index of `metric: cosine`) gets them by
     `POST /vectors/upsert` 500 at a time (1,000 as JSON pass its 2 MB a request);
     ChloeDB (chloedb-server) by `POST /v1/write` 500 at a time, into
     `CREATE TABLE docs (id KEYWORD PRIMARY KEY, emb VECTOR(100))`, then
     `POST /v1/admin/optimize` to write the memtable and merge segments.
  3. The first 1,000 test queries are asked of each for the top 10:
     Pinecone `POST /query {vector, topK: 10}`, ChloeDB
     `SELECT id FROM docs ORDER BY similarity(emb, :v) DESC LIMIT 10`, at its
     default ef (200) and at ef 32, 64, 128 and 400.
  4. Recall@10 against the exact top 10 (numpy, over the same unit vectors),
     latency at 1 client (p50, p95, p99, after 20 warm-ups) and at 4 clients
     (questions a second, p99), load time, memory and disk.

    python3 run.py [100k full]

Needs Docker, `pip install -r ../requirements.txt` plus h5py, and a
chloedb-server build. It downloads glove-100-angular.hdf5 (463 MB, checked
by SHA-256) into $H2H_DATA (default /tmp/h2h-data). Raw numbers go to
raw/results-<size>.json.
"""

import concurrent.futures as cf
import hashlib
import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.request

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import common  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
WORK = os.environ.get("H2H_WORK", "/tmp/h2h-pinecone")
DATA = os.environ.get("H2H_DATA", "/tmp/h2h-data")
PC_IMAGE = os.environ.get(
    "H2H_PINECONE_IMAGE",
    "ghcr.io/pinecone-io/pinecone-local@sha256:0f944585830a30c37260ac6534c7c206280490947e1ddbb716474166b517b6c7")
GLOVE_URL = "http://ann-benchmarks.com/glove-100-angular.hdf5"
NQ = int(os.environ.get("H2H_QUERIES", "1000"))
WARM = 20
BATCH = 500  # Pinecone takes at most 2 MB a request: 1,000 of these vectors as JSON are more
EFS = (32, 64, 128, 400)
SIZES = {"100k": 100_000, "full": None}


# ---------------------------------------------------------------- data

def glove():
    os.makedirs(DATA, exist_ok=True)
    path = os.path.join(DATA, "glove-100-angular.hdf5")
    if not os.path.exists(path):
        print(f"downloading {GLOVE_URL}", flush=True)
        subprocess.run(["curl", "-fsSL", "-o", path + ".part", GLOVE_URL], check=True)
        os.rename(path + ".part", path)
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    digest = h.hexdigest()
    with open(os.path.join(HERE, "glove.sha256")) as f:
        want = f.read().split()[0]
    if digest != want:
        raise SystemExit(f"{path}: SHA-256 {digest}, expected {want}")
    import h5py
    with h5py.File(path, "r") as f:
        train = np.asarray(f["train"], dtype=np.float32)
        test = np.asarray(f["test"], dtype=np.float32)
    return train, test, digest


def unit(x):
    n = np.linalg.norm(x, axis=1, keepdims=True)
    n[n == 0] = 1
    return (x / n).astype(np.float32)


def exact_top(base, queries, k=10):
    """The exact top k of each query by the dot product, in blocks."""
    out = np.empty((len(queries), k), dtype=np.int64)
    for i in range(0, len(queries), 100):
        s = queries[i:i + 100] @ base.T
        part = np.argpartition(-s, k, axis=1)[:, :k]
        order = np.argsort(-np.take_along_axis(s, part, axis=1), axis=1)
        out[i:i + 100] = np.take_along_axis(part, order, axis=1)
    return out


def recall(found, truth):
    """found: lists of ids (strings); truth: rows of indices."""
    return round(float(np.mean([len(set(map(int, f[:10])) & set(t.tolist())) / 10 for f, t in zip(found, truth)])), 4)


# ---------------------------------------------------------------- timing

def latency(fn, items, clients):
    for it in items[:WARM]:
        fn(it)
    t0 = time.perf_counter()
    if clients == 1:
        ms = [_one(fn, it) for it in items]
    else:
        with cf.ThreadPoolExecutor(clients) as ex:
            ms = list(ex.map(lambda it: _one(fn, it), items))
    wall = time.perf_counter() - t0
    q = sorted(ms)
    return {"clients": clients, "n": len(ms), "qps": round(len(ms) / wall, 1),
            "p50_ms": round(q[len(q) // 2], 3), "p95_ms": round(q[int(len(q) * 0.95)], 3),
            "p99_ms": round(q[min(len(q) - 1, int(len(q) * 0.99))], 3)}


def _one(fn, it):
    t = time.perf_counter()
    fn(it)
    return (time.perf_counter() - t) * 1000


def docker_mem_kb(name):
    s = subprocess.run(["docker", "stats", "--no-stream", "--format", "{{.MemUsage}}", name],
                       capture_output=True, text=True).stdout
    v = s.split("/")[0].strip()
    for u, k in {"KiB": 1, "MiB": 1024, "GiB": 1024 * 1024}.items():
        if v.endswith(u):
            return int(float(v[:-len(u)]) * k)
    return 0


# ---------------------------------------------------------------- Pinecone Local

class Pinecone:
    """Pinecone Local's REST API (control plane on one port, each index on
    the next), as the SDKs send it."""
    HEADERS = {"Api-Key": "pclocal", "X-Pinecone-API-Version": "2025-01", "content-type": "application/json"}

    def __init__(self, port):
        self.port = port
        subprocess.run(["docker", "rm", "-f", "h2h-pinecone"], capture_output=True)
        subprocess.run(["docker", "run", "-d", "--name", "h2h-pinecone", "-e", f"PORT={port}",
                        "-e", "PINECONE_HOST=localhost", "-p", f"127.0.0.1:{port}-{port + 2}:{port}-{port + 2}",
                        PC_IMAGE], check=True, capture_output=True)
        self.control = f"http://127.0.0.1:{port}"
        for _ in range(100):
            try:
                if "indexes" in self.req("GET", self.control + "/indexes"):
                    break
            except (urllib.error.URLError, ConnectionError, OSError):
                pass
            time.sleep(0.2)
        else:
            raise SystemExit("pinecone local did not start")
        img = subprocess.run(["docker", "image", "inspect", PC_IMAGE, "--format",
                              "{{index .Config.Labels \"org.opencontainers.image.version\"}} {{.Created}}"],
                             capture_output=True, text=True).stdout.split()
        self.version = f"Pinecone Local {img[0]} (image of {img[1][:10]}, {PC_IMAGE.split('@')[0]})"

    def req(self, method, url, body=None):
        data = None if body is None else json.dumps(body).encode()
        r = urllib.request.Request(url, data=data, method=method, headers=self.HEADERS)
        try:
            with urllib.request.urlopen(r, timeout=600) as resp:
                t = resp.read()
                return json.loads(t) if t else {}
        except urllib.error.HTTPError as e:
            return {"_error": e.code, "text": e.read().decode(errors="replace")[:500]}

    def create(self, name, dim):
        r = self.req("POST", self.control + "/indexes", {"name": name, "dimension": dim, "metric": "cosine",
                                                          "spec": {"serverless": {"cloud": "aws", "region": "us-east-1"}}})
        if "_error" in r:
            raise SystemExit(f"create index: {r}")
        self.data = "http://" + r["host"].replace("localhost", "127.0.0.1")

    def stop(self):
        subprocess.run(["docker", "rm", "-f", "h2h-pinecone"], capture_output=True)


def run_pinecone(base, queries):
    pc = Pinecone(common.free_port())
    out = {"version": pc.version, "memory_start_kb": docker_mem_kb("h2h-pinecone")}
    pc.create("glove", base.shape[1])
    t = time.perf_counter()
    for i in range(0, len(base), BATCH):
        r = pc.req("POST", pc.data + "/vectors/upsert",
                   {"vectors": [{"id": str(j), "values": base[j].tolist()} for j in range(i, min(i + BATCH, len(base)))]})
        if "_error" in r:
            raise SystemExit(f"upsert failed at {i}: {r}")
    out["load_s"] = round(time.perf_counter() - t, 2)
    while True:
        st = pc.req("POST", pc.data + "/describe_index_stats", {})
        if st.get("totalVectorCount") == len(base):
            break
        time.sleep(0.5)
    out["ready_s"] = round(time.perf_counter() - t, 2)
    out["rows"] = st["totalVectorCount"]
    vecs = [q.tolist() for q in queries]

    def ask(j):
        r = pc.req("POST", pc.data + "/query", {"vector": vecs[j], "topK": 10})
        if "matches" not in r:
            raise SystemExit(f"pinecone refused: {json.dumps(r)[:500]}")
        return [m["id"] for m in r["matches"]]

    js = list(range(len(queries)))
    found = [ask(j) for j in js]
    out["latency"] = [latency(ask, js, 1), latency(ask, js, 4)]
    out["memory_after_kb"] = docker_mem_kb("h2h-pinecone")
    out["disk_bytes"] = 0  # in memory only: nothing is written to disk
    pc.stop()
    return out, {"pinecone": found}


# ---------------------------------------------------------------- ChloeDB

SQL = "SELECT id FROM docs ORDER BY similarity(emb, :v) DESC LIMIT 10"
SQL_EF = "SELECT id FROM docs ORDER BY similarity(emb, :v, ef => {ef}) DESC LIMIT 10"


def run_chloedb(base, queries):
    folder = os.path.join(WORK, "chloedb")
    subprocess.run(["rm", "-rf", folder])
    ch = common.Chloedb(folder)
    out = {"version": ch.version, "memory_start_kb": ch.memory_kb()}
    r = ch.sql(f"CREATE TABLE docs (id KEYWORD PRIMARY KEY, emb VECTOR({base.shape[1]}))")
    assert "error" not in r, r
    t = time.perf_counter()
    for i in range(0, len(base), BATCH):
        ops = [{"op": "upsert", "table": "docs", "doc": {"id": str(j), "emb": base[j].tolist()}}
               for j in range(i, min(i + BATCH, len(base)))]
        for attempt in range(100):
            r = ch.call("/v1/write", {"ops": ops})
            if r.get("status") != 429:
                break
            time.sleep(1)
        if "error" in r:
            raise SystemExit(f"write failed at {i}: {r}")
    out["load_s"] = round(time.perf_counter() - t, 2)
    # optimize merges what the background merges are not already merging, so
    # ask again until no merge runs and optimize has nothing left to merge
    rounds = []
    while True:
        st = ch.call("/v1/stats")
        if st["merges"]["running"]:
            time.sleep(0.5)
            continue
        opt = ch.call("/v1/admin/optimize", {})
        rounds.append(opt)
        if not opt.get("merges") and not ch.call("/v1/stats")["merges"]["running"]:
            break
    out["ready_s"] = round(time.perf_counter() - t, 2)
    out["optimize"] = rounds
    out["segments"] = [s["n_docs"] for s in ch.call("/v1/stats")["storage"]["segments"] if s.get("table") == "docs"]
    out["rows"] = ch.sql("SELECT COUNT(*) FROM docs")["rows"][0][0]
    vecs = [q.tolist() for q in queries]
    took = []

    def ask(j, sql=SQL, keep=False):
        r = ch.sql(sql, {"v": vecs[j]})
        if "rows" not in r:
            raise SystemExit(f"chloedb refused: {json.dumps(r)[:500]}")
        if keep:
            took.append(r.get("took_ms") or 0)
        return [row[0] for row in r["rows"]]

    js = list(range(len(queries)))
    runs = {"chloedb": [ask(j) for j in js]}
    out["latency"] = [latency(lambda j: ask(j, keep=True), js, 1), latency(ask, js, 4)]
    out["latency"][0]["server"] = common.percentiles(took[WARM:])
    out["ef"] = {}
    for ef in EFS:
        sql = SQL_EF.format(ef=ef)
        runs[f"chloedb_ef{ef}"] = [ask(j, sql) for j in js]
        out["ef"][ef] = latency(lambda j, s=sql: ask(j, s), js, 1)
    plan = ch.call("/v1/explain", {"sql": SQL, "params": {"v": vecs[0]}})
    out["plan"] = plan.get("plan")
    out["took_explain_ms"] = plan.get("took_ms")
    out["memory_after_kb"] = ch.memory_kb()
    ch.call("/v1/admin/vacuum", {})
    out["disk_bytes"] = ch.disk_bytes()
    ch.stop()
    return out, runs


# ---------------------------------------------------------------- main

def main():
    sizes = sys.argv[1:] or list(SIZES)
    os.makedirs(WORK, exist_ok=True)
    train, test, digest = glove()
    queries = unit(test[:NQ])
    for size in sizes:
        n = SIZES[size] or len(train)
        base = unit(train[:n])
        print(f"{size}: {n:,} vectors of {base.shape[1]}, {len(queries):,} queries", flush=True)
        truth = exact_top(base, queries)
        res = {"machine": common.machine(), "date": time.strftime("%Y-%m-%d"), "collection": "glove-100-angular",
               "sha256": digest, "vectors": n, "dim": int(base.shape[1]), "queries": len(queries)}
        po, pruns = run_pinecone(base, queries)
        print("pinecone", json.dumps(po)[:400], flush=True)
        co, cruns = run_chloedb(base, queries)
        print("chloedb", json.dumps({k: v for k, v in co.items() if k != "plan"})[:600], flush=True)
        runs = {**pruns, **cruns}
        res["recall_at_10"] = {k: recall(v, truth) for k, v in runs.items()}
        res["same_top10"] = round(float(np.mean([a == b for a, b in zip(runs["chloedb"], runs["pinecone"])])), 4)
        res["pinecone"], res["chloedb"] = po, co
        print("recall", res["recall_at_10"], flush=True)
        common.write_json(os.path.join(HERE, "raw", f"results-{size}.json"), res)


if __name__ == "__main__":
    main()
