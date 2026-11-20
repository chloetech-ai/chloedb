"""Priya's hybrid retrieval: Elasticsearch BM25 plus Qdrant, fused in Python,
against ChloeDB's `rrf(bm25(...), similarity(...))` in one statement.

Priya runs RAG retrieval as Elasticsearch for BM25 and Qdrant for vectors,
with bge-small-en-v1.5 vectors she computes herself and reciprocal rank fusion
in her service. Her tech lead asks for "a head-to-head on our judged set
against Elasticsearch plus Qdrant: same relevance or better, p95 no worse,
fewer machines". This runs that on two public judged sets, BEIR SciFact and
BEIR FiQA-2018 (57,638 passages, 648 test questions), on one machine:

  1. Every passage and question is embedded once with fastembed's
     bge-small-en-v1.5 (as Priya does); both systems get the same vectors.
  2. Elasticsearch (Docker, one node, `english` analyzer) gets the passages
     by `_bulk`; Qdrant (Docker, cosine, its default HNSW) the vectors by
     upsert; ChloeDB (chloedb-server) the passages and vectors by /v1/write.
  3. Every test question is asked of each: BM25 alone, the vectors alone,
     and the two fused by RRF (k = 60, top 100 of each), Elasticsearch and
     Qdrant fused in Python as her service does, ChloeDB in one SQL query.
  4. nDCG@10 and recall@100 against BEIR's judgements, a paired bootstrap of
     the difference, latency at 1 and 4 clients, memory, disk, load time.

    python3 run.py [scifact fiqa]

Needs `pip install -r ../requirements.txt`, Docker, and a
chloedb-server build. It downloads the collections (pinned by SHA-256; SciFact
through the repository's chloedb/tests/fixtures/beir/fetch.sh) and the model
(about 130 MB, by fastembed). Raw numbers go to raw/results-<set>.json.
"""

import concurrent.futures as cf
import hashlib
import json
import os
import re
import subprocess
import sys
import time
import urllib.error
import urllib.request

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import common  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
WORK = os.environ.get("H2H_WORK", "/tmp/h2h-priya")
ES_IMAGE = os.environ.get("H2H_ES_IMAGE", "mirror.gcr.io/library/elasticsearch:9.5.5")
ES_HEAP = os.environ.get("H2H_ES_HEAP", "1g")
QDRANT_IMAGE = os.environ.get("H2H_QDRANT_IMAGE", "mirror.gcr.io/qdrant/qdrant:v1.20.0")
MODEL = "BAAI/bge-small-en-v1.5"
DEPTH = 100  # each list's depth, and the fused list's
RRF_K = 60

HF = "https://huggingface.co/datasets/BeIR"
FIQA = [  # file, URL, SHA-256
    ("corpus.parquet", f"{HF}/fiqa/resolve/979c07a7cb5ccc6ca009792241fa1250b98055dd/corpus/corpus-00000-of-00001.parquet",
     "b36bb07dab3bfad488190818828e749a52faf5ba391c1d51dac8b06064576f99"),
    ("queries.parquet", f"{HF}/fiqa/resolve/979c07a7cb5ccc6ca009792241fa1250b98055dd/queries/queries-00000-of-00001.parquet",
     "e9b3863724ffd538d8efff19684ef8ae495bdbddd96b35e81bdc39e3e22860d6"),
    ("test.tsv", f"{HF}/fiqa-qrels/resolve/252958f2d646e22cab6d0c72dd3f0d5de6d0655a/test.tsv",
     "6adc2a640dcdd22bb8b3858f89107adef2a7c3db20a63550dfa7a0f71e379e44"),
]


# ---------------------------------------------------------------- the data

def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for b in iter(lambda: f.read(1 << 20), b""):
            h.update(b)
    return h.hexdigest()


def fetch(name):
    d = os.path.join(WORK, "data", name)
    if name == "scifact":
        subprocess.run([os.path.join(common.REPO, "chloedb", "tests", "fixtures", "beir", "fetch.sh"), os.path.join(WORK, "data")],
                       check=True)
        return d
    os.makedirs(d, exist_ok=True)
    for file, url, want in FIQA:
        p = os.path.join(d, file)
        if not (os.path.exists(p) and sha256(p) == want):
            urllib.request.urlretrieve(url, p + ".tmp")
            got = sha256(p + ".tmp")
            if got != want:
                raise SystemExit(f"{p}: SHA-256 {got}, expected {want}")
            os.replace(p + ".tmp", p)
    return d


def load(name):
    """passages [(id, title, text)], test questions {qid: text}, judgements {qid: {docid: grade}}"""
    d = fetch(name)
    if name == "scifact":
        docs = [(r["_id"], r.get("title") or "", r.get("text") or "") for r in map(json.loads, open(os.path.join(d, "corpus.jsonl")))]
        queries = {r["_id"]: r["text"] for r in map(json.loads, open(os.path.join(d, "queries.jsonl")))}
        qrels_path = os.path.join(d, "qrels", "test.tsv")
    else:
        import duckdb
        c = duckdb.connect()
        docs = [(str(i), t or "", x or "") for i, t, x in c.sql(f"SELECT _id, title, text FROM '{d}/corpus.parquet'").fetchall()]
        queries = {str(i): t for i, t in c.sql(f"SELECT _id, text FROM '{d}/queries.parquet'").fetchall()}
        qrels_path = os.path.join(d, "test.tsv")
    qrels = {}
    for line in open(qrels_path).read().splitlines()[1:]:
        q, doc, g = line.split("\t")
        if int(g) > 0:
            qrels.setdefault(q, {})[doc] = int(g)
    queries = {q: queries[q] for q in sorted(qrels)}
    return docs, queries, qrels


def embed(name, docs, queries):
    """bge-small-en-v1.5 by fastembed, once per collection: unit vectors of
    the title and the text, and of each question with bge's query prefix."""
    from fastembed import TextEmbedding
    dp, qp = os.path.join(WORK, f"emb-{name}-docs.npy"), os.path.join(WORK, f"emb-{name}-queries.npy")
    tp = os.path.join(WORK, f"emb-{name}.json")
    out = {}
    if os.path.exists(dp) and os.path.exists(qp):
        # the times of the run that embedded them
        out = {**(json.load(open(tp)) if os.path.exists(tp) else {}), "cached": True}
        return np.load(dp), np.load(qp), out
    model = TextEmbedding(MODEL)
    t = time.perf_counter()
    dv = np.array(list(model.embed([f"{title} {text}".strip() for _, title, text in docs], batch_size=64)), dtype=np.float32)
    out["docs_s"] = round(time.perf_counter() - t, 1)
    t = time.perf_counter()
    qv = np.array(list(model.query_embed(list(queries.values()))), dtype=np.float32)
    out["queries_s"] = round(time.perf_counter() - t, 1)
    np.save(dp, dv)
    np.save(qp, qv)
    json.dump(out, open(tp, "w"))
    return dv, qv, out


# ---------------------------------------------------------------- measures

def ndcg10(ranked, rel):
    dcg = sum(rel.get(d, 0) / np.log2(i + 2) for i, d in enumerate(ranked[:10]))
    ideal = sorted(rel.values(), reverse=True)[:10]
    idcg = sum(g / np.log2(i + 2) for i, g in enumerate(ideal))
    return dcg / idcg if idcg else 0.0


def recall100(ranked, rel):
    return len(set(ranked[:100]) & set(rel)) / len(rel)


def score(runs, qrels):
    per = {q: (ndcg10(r, qrels[q]), recall100(r, qrels[q])) for q, r in runs.items()}
    return {"ndcg@10": round(float(np.mean([v[0] for v in per.values()])), 4),
            "recall@100": round(float(np.mean([v[1] for v in per.values()])), 4),
            "queries": len(per)}, per


def paired(a, b, reps=10000, seed=1):
    """The mean of a minus b over the questions, with a bootstrap 95% interval."""
    qs = sorted(a)
    diffs = {}
    for i, m in enumerate(("ndcg@10", "recall@100")):
        d = np.array([a[q][i] - b[q][i] for q in qs])
        rng = np.random.default_rng(seed)
        means = d[rng.integers(0, len(d), (reps, len(d)))].mean(axis=1)
        diffs[m] = {"diff": round(float(d.mean()), 4), "lo": round(float(np.percentile(means, 2.5)), 4),
                    "hi": round(float(np.percentile(means, 97.5)), 4),
                    "better": int((d > 0).sum()), "worse": int((d < 0).sum())}
    return diffs


def rrf(*lists, k=RRF_K, depth=DEPTH):
    s = {}
    for lst in lists:
        for i, d in enumerate(lst[:depth]):
            s[d] = s.get(d, 0.0) + 1.0 / (k + i + 1)
    return [d for d, _ in sorted(s.items(), key=lambda x: (-x[1], x[0]))][:depth]


def exact(dv, qv, depth=DEPTH):
    top = np.argpartition(-(qv @ dv.T), depth, axis=1)[:, :depth]
    return [t[np.argsort(-(dv[t] @ q))] for t, q in zip(top, qv)]


def latency(fn, items, clients):
    """Every item once after 20 warm-ups; wall time of each, and the rate."""
    for it in items[:20]:
        fn(it)
    t0 = time.perf_counter()
    if clients == 1:
        ms = [_one(fn, it) for it in items]
    else:
        with cf.ThreadPoolExecutor(clients) as ex:
            ms = list(ex.map(lambda it: _one(fn, it), items))
    wall = time.perf_counter() - t0
    return {**common.percentiles(ms), "clients": clients, "per_s": round(len(items) / wall, 1)}


def _one(fn, it):
    t = time.perf_counter()
    fn(it)
    return (time.perf_counter() - t) * 1000


# ---------------------------------------------------------------- HTTP

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
            if r is not None and "_error" not in r:
                return r
        except (urllib.error.URLError, ConnectionError, OSError):
            pass
        time.sleep(0.5)
    raise SystemExit(f"{http.url}{path} did not answer")


def docker_mem_kb(name):
    s = subprocess.run(["docker", "stats", "--no-stream", "--format", "{{.MemUsage}}", name], capture_output=True, text=True).stdout
    v = s.split("/")[0].strip()
    for u, k in {"KiB": 1, "MiB": 1024, "GiB": 1024 * 1024}.items():
        if v.endswith(u):
            return int(float(v[:-len(u)]) * k)
    return 0


def docker_run(name, image, port, inner, env=()):
    subprocess.run(["docker", "rm", "-f", name], capture_output=True)
    args = ["docker", "run", "-d", "--name", name, "-p", f"127.0.0.1:{port}:{inner}"]
    for e in env:
        args += ["-e", e]
    subprocess.run(args + [image], check=True, capture_output=True)


# ---------------------------------------------------------------- Elasticsearch + Qdrant

ES_BODY = lambda q: {"size": DEPTH, "_source": False, "query": {"match": {"body": q}}}  # noqa: E731
# BEIR's own Elasticsearch baseline: both fields, best_fields, tie_breaker 0.5
ES_BEIR = lambda q: {"size": DEPTH, "_source": False,  # noqa: E731
                     "query": {"multi_match": {"query": q, "fields": ["title", "text"], "type": "best_fields", "tie_breaker": 0.5}}}


def run_es_qdrant(docs, dv, queries, qv):
    out = {}
    es_port, qd_port = common.free_port(), common.free_port()
    docker_run("h2h-es", ES_IMAGE, es_port, 9200, ["discovery.type=single-node", "xpack.security.enabled=false",
                                                   f"ES_JAVA_OPTS=-Xms{ES_HEAP} -Xmx{ES_HEAP}"])
    docker_run("h2h-qdrant", QDRANT_IMAGE, qd_port, 6333)
    es, qd = Http(f"http://127.0.0.1:{es_port}"), Http(f"http://127.0.0.1:{qd_port}")
    info = wait(es)
    wait(es, "/_cluster/health?wait_for_status=yellow&timeout=60s")
    qinfo = wait(qd)
    out["version"] = f"Elasticsearch {info['version']['number']} (Docker {ES_IMAGE}, heap {ES_HEAP}); Qdrant {qinfo.get('version')} (Docker {QDRANT_IMAGE})"
    out["memory_start_kb"] = {"elasticsearch": docker_mem_kb("h2h-es"), "qdrant": docker_mem_kb("h2h-qdrant")}

    # Elasticsearch: the passages
    eng = {"type": "text", "analyzer": "english"}
    r = es.req("PUT", "/docs", {"settings": {"number_of_shards": 1, "number_of_replicas": 0},
                                "mappings": {"properties": {"title": eng, "text": eng, "body": eng}}})
    assert "_error" not in r, r
    t = time.perf_counter()
    for i in range(0, len(docs), 2000):
        lines = []
        for did, title, text in docs[i:i + 2000]:
            lines.append(json.dumps({"index": {"_index": "docs", "_id": did}}))
            lines.append(json.dumps({"title": title, "text": text, "body": f"{title}\n{text}".strip()}))
        r = es.req("POST", "/_bulk", "\n".join(lines) + "\n", ndjson=True)
        if r.get("errors") or "_error" in r:
            raise SystemExit(f"bulk failed: {json.dumps(r)[:500]}")
    es.req("POST", "/docs/_refresh")
    out["es_load_s"] = round(time.perf_counter() - t, 2)

    # Qdrant: the vectors, by integer id (Qdrant takes integers or UUIDs), its default HNSW
    ids = [d[0] for d in docs]
    r = qd.req("PUT", "/collections/docs", {"vectors": {"size": dv.shape[1], "distance": "Cosine"}})
    assert "_error" not in r, r
    t = time.perf_counter()
    for i in range(0, len(docs), 1000):
        pts = [{"id": j, "vector": dv[j].tolist(), "payload": {"doc": ids[j]}} for j in range(i, min(i + 1000, len(docs)))]
        r = qd.req("PUT", "/collections/docs/points?wait=true", {"points": pts})
        if "_error" in r:
            raise SystemExit(f"qdrant upsert failed: {json.dumps(r)[:500]}")
    for _ in range(1200):
        st = qd.req("GET", "/collections/docs")["result"]
        if st["status"] == "green" and st.get("optimizer_status") == "ok":
            break
        time.sleep(0.5)
    out["qdrant_load_s"] = round(time.perf_counter() - t, 2)
    out["qdrant_collection"] = {k: st.get(k) for k in ("status", "points_count", "indexed_vectors_count", "segments_count")}

    qids = list(queries)
    qtext = [queries[q] for q in qids]

    def es_search(body):
        return [h["_id"] for h in es.req("POST", "/docs/_search", body)["hits"]["hits"]]

    def qd_search(j):
        r = qd.req("POST", "/collections/docs/points/query", {"query": qv[j].tolist(), "limit": DEPTH})
        return [ids[p["id"]] for p in r["result"]["points"]]

    pool = cf.ThreadPoolExecutor(2)

    def hybrid(j):  # her service: both asked at once, fused in Python
        a = pool.submit(es_search, ES_BEIR(qtext[j]))
        b = pool.submit(qd_search, j)
        return rrf(a.result(), b.result())

    def hybrid_seq(j):
        return rrf(es_search(ES_BEIR(qtext[j])), qd_search(j))

    runs = {
        "es_bm25_body": {qids[j]: es_search(ES_BODY(qtext[j])) for j in range(len(qids))},
        "es_bm25_beir": {qids[j]: es_search(ES_BEIR(qtext[j])) for j in range(len(qids))},
        "qdrant": {qids[j]: qd_search(j) for j in range(len(qids))},
    }
    runs["es_qdrant_rrf"] = {q: rrf(runs["es_bm25_beir"][q], runs["qdrant"][q]) for q in qids}
    js = list(range(len(qids)))
    out["latency"] = {
        "es_bm25_beir": [latency(lambda j: es_search(ES_BEIR(qtext[j])), js, c) for c in (1, 4)],
        "qdrant": [latency(qd_search, js, c) for c in (1, 4)],
        "es_qdrant_rrf": [latency(hybrid, js, 1), latency(hybrid_seq, js, 4)],
        "es_qdrant_rrf_sequential": [latency(hybrid_seq, js, 1)],
    }
    pool.shutdown()
    out["memory_after_kb"] = {"elasticsearch": docker_mem_kb("h2h-es"), "qdrant": docker_mem_kb("h2h-qdrant")}
    es.req("POST", "/docs/_forcemerge?max_num_segments=1")
    es_disk = es.req("GET", "/docs/_stats/store")["indices"]["docs"]["total"]["store"]["size_in_bytes"]
    qd_disk = int(subprocess.run(["docker", "exec", "h2h-qdrant", "du", "-sb", "/qdrant/storage"], capture_output=True, text=True).stdout.split()[0])
    out["disk_bytes"] = {"elasticsearch": es_disk, "qdrant": qd_disk}
    for n in ("h2h-es", "h2h-qdrant"):
        subprocess.run(["docker", "rm", "-f", n], capture_output=True)
    return out, runs


# ---------------------------------------------------------------- ChloeDB

SQL_BM25 = "SELECT id FROM docs ORDER BY bm25(body, :q, syntax => 'plain') DESC LIMIT 100"
SQL_VEC = "SELECT id FROM docs ORDER BY similarity(emb, :v) DESC LIMIT 100"
SQL_RRF = "SELECT id FROM docs ORDER BY rrf(bm25(body, :q, syntax => 'plain'), similarity(emb, :v)) DESC LIMIT 100"


def run_chloedb(docs, dv, queries, qv):
    folder = os.path.join(WORK, "chloedb")
    subprocess.run(["rm", "-rf", folder])
    ch = common.Chloedb(folder)
    out = {"version": ch.version, "memory_start_kb": ch.memory_kb()}
    r = ch.sql(f"CREATE TABLE docs (id KEYWORD PRIMARY KEY, body TEXT, emb VECTOR({dv.shape[1]}))")
    assert "error" not in r, r
    t = time.perf_counter()
    rows = [{"id": did, "body": f"{title}\n{text}".strip(), "emb": dv[j].tolist()} for j, (did, title, text) in enumerate(docs)]
    ch.write("docs", rows, batch=500)
    n = ch.sql("SELECT COUNT(*) FROM docs")["rows"][0][0]
    out["load_s"] = round(time.perf_counter() - t, 2)
    out["rows"] = n
    qids = list(queries)
    qtext = [queries[q] for q in qids]
    vecs = [v.tolist() for v in qv]
    took = {}

    def ask(sql, j, key=None):
        r = ch.sql(sql, {"q": qtext[j], "v": vecs[j]})
        if "rows" not in r:
            raise SystemExit(f"chloedb refused: {json.dumps(r)[:500]}")
        if key:
            took.setdefault(key, []).append(r.get("took_ms") or 0)
        return [row[0] for row in r["rows"]]

    runs = {name: {qids[j]: ask(sql, j) for j in range(len(qids))}
            for name, sql in (("chloedb_bm25", SQL_BM25), ("chloedb_vector", SQL_VEC), ("chloedb_rrf", SQL_RRF))}
    js = list(range(len(qids)))
    out["latency"] = {}
    for name, sql in (("chloedb_bm25", SQL_BM25), ("chloedb_vector", SQL_VEC), ("chloedb_rrf", SQL_RRF)):
        took.pop(name, None)
        out["latency"][name] = [latency(lambda j, s=sql, k=name: ask(s, j, k), js, 1), latency(lambda j, s=sql: ask(s, j), js, 4)]
        out["latency"][name][0]["server"] = common.percentiles(took[name][20:] or took[name])
    plan = ch.call("/v1/explain", {"sql": SQL_RRF, "params": {"q": qtext[0], "v": vecs[0]}})
    # the plan as the server gave it, the question's vector left out
    out["plan"] = re.sub(r'"vector": \[[^\]]*\]', '"vector": "(384 numbers)"', json.dumps(plan))[:4000]
    out["memory_after_kb"] = ch.memory_kb()
    ch.call("/v1/admin/vacuum", {})
    out["disk_bytes"] = ch.disk_bytes()
    ch.stop()
    return out, runs


# ---------------------------------------------------------------- main

def main():
    sets = sys.argv[1:] or ["scifact", "fiqa"]
    os.makedirs(WORK, exist_ok=True)
    for name in sets:
        docs, queries, qrels = load(name)
        print(f"{name}: {len(docs):,} passages, {len(queries):,} test questions", flush=True)
        dv, qv, emb = embed(name, docs, queries)
        print(f"embedded {emb}", flush=True)
        res = {"set": name, "at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "machine": common.machine(),
               "passages": len(docs), "questions": len(queries), "judgements": sum(len(v) for v in qrels.values()),
               "model": MODEL, "embedding": emb, "depth": DEPTH, "rrf_k": RRF_K}
        ids = [d[0] for d in docs]
        ex = exact(dv, qv)
        qids = list(queries)
        exact_runs = {qids[j]: [ids[i] for i in ex[j]] for j in range(len(qids))}

        ch, ch_runs = run_chloedb(docs, dv, queries, qv)
        print("chloedb", json.dumps({k: v for k, v in ch.items() if k != "plan"})[:600], flush=True)
        eq, eq_runs = run_es_qdrant(docs, dv, queries, qv)
        print("es+qdrant", json.dumps(eq)[:600], flush=True)
        runs = {**eq_runs, **ch_runs, "exact_vector": exact_runs}
        per = {}
        res["sources"] = {}
        for k, r in runs.items():
            res["sources"][k], per[k] = score(r, qrels)
        # how much of the exact vector top 100 each vector search found
        for k in ("qdrant", "chloedb_vector"):
            res["sources"][k]["vs_exact_top10"] = round(float(np.mean([len(set(runs[k][q][:10]) & set(exact_runs[q][:10])) / 10 for q in qids])), 4)
            res["sources"][k]["vs_exact_top100"] = round(float(np.mean([len(set(runs[k][q]) & set(exact_runs[q])) / 100 for q in qids])), 4)
        res["paired"] = {
            "chloedb_rrf - es_qdrant_rrf": paired(per["chloedb_rrf"], per["es_qdrant_rrf"]),
            "chloedb_bm25 - es_bm25_beir": paired(per["chloedb_bm25"], per["es_bm25_beir"]),
            "chloedb_bm25 - es_bm25_body": paired(per["chloedb_bm25"], per["es_bm25_body"]),
            "chloedb_vector - qdrant": paired(per["chloedb_vector"], per["qdrant"]),
        }
        res["same_top10"] = {
            "rrf": round(float(np.mean([runs["chloedb_rrf"][q][:10] == runs["es_qdrant_rrf"][q][:10] for q in qids])), 4),
            "vector": round(float(np.mean([runs["chloedb_vector"][q][:10] == runs["qdrant"][q][:10] for q in qids])), 4),
        }
        res["chloedb"], res["es_qdrant"] = ch, eq
        common.write_json(os.path.join(HERE, "raw", f"results-{name}.json"), res)
        print(f"wrote raw/results-{name}.json", json.dumps(res["sources"]), flush=True)


if __name__ == "__main__":
    main()
