"""ChloeDB's own time per query (`took_ms`, measured inside the server, no
HTTP or JSON) at every setting run.py times, to set beside Pinecone's own time
per query (its `x-pinecone-request-latency-ms` and
`x-pinecone-response-duration-ms` headers, kept in raw/results-<size>.json).

It loads the same unit vectors as run.py (through ../pinecone-local/run.py's
code), asks the same 1,000 queries once after 20 warm-ups at each setting, at
1 client and at 4, and keeps the server's took_ms of each. It also counts
recall again, as a check that the index is the one run.py measured.

    python3 server_time.py [100k full]

Raw numbers go to raw/server-time-<size>.json.
"""

import concurrent.futures as cf
import json
import os
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import run  # noqa: E402  (this folder's run.py, which loads ../pinecone-local/run.py)

local, common = run.local, run.common


def load(base):
    folder = os.path.join(local.WORK, "chloedb-took")
    os.system(f"rm -rf {folder}")
    ch = common.Chloedb(folder)
    r = ch.sql(f"CREATE TABLE docs (id KEYWORD PRIMARY KEY, emb VECTOR({base.shape[1]}))")
    assert "error" not in r, r
    for i in range(0, len(base), local.BATCH):
        ops = [{"op": "upsert", "table": "docs", "doc": {"id": str(j), "emb": base[j].tolist()}}
               for j in range(i, min(i + local.BATCH, len(base)))]
        for _ in range(100):
            r = ch.call("/v1/write", {"ops": ops})
            if r.get("status") != 429:
                break
            time.sleep(1)
        if "error" in r:
            raise SystemExit(f"write failed at {i}: {r}")
    while True:
        if ch.call("/v1/stats")["merges"]["running"]:
            time.sleep(0.5)
            continue
        opt = ch.call("/v1/admin/optimize", {})
        if not opt.get("merges") and not ch.call("/v1/stats")["merges"]["running"]:
            break
    segs = [s["n_docs"] for s in ch.call("/v1/stats")["storage"]["segments"] if s.get("table") == "docs"]
    return ch, segs


def measure(ch, sql, vecs, clients):
    def ask(j):
        t = time.perf_counter()
        r = ch.sql(sql, {"v": vecs[j]})
        client = (time.perf_counter() - t) * 1000
        if "rows" not in r:
            raise SystemExit(f"chloedb refused: {json.dumps(r)[:300]}")
        return [row[0] for row in r["rows"]], float(r.get("took_ms") or 0), client

    js = list(range(len(vecs)))
    for j in js[:local.WARM]:
        ask(j)
    t = time.perf_counter()
    if clients == 1:
        got = [ask(j) for j in js]
    else:
        with cf.ThreadPoolExecutor(clients) as ex:
            got = list(ex.map(ask, js))
    wall = time.perf_counter() - t
    return ([g[0] for g in got],
            {"clients": clients, "qps": round(len(js) / wall, 1),
             "took": run.pct([g[1] for g in got]), "client": run.pct([g[2] for g in got])})


def main():
    sizes = sys.argv[1:] or ["100k", "full"]
    train, test, digest = local.glove()
    queries = local.unit(test[:local.NQ])
    vecs = [q.tolist() for q in queries]
    for size in sizes:
        n = local.SIZES[size] or len(train)
        base = local.unit(train[:n])
        truth = local.exact_top(base, queries)
        print(f"{size}: loading {n:,}", flush=True)
        ch, segs = load(base)
        settings = [("default", local.SQL)] + [(f"ef{ef}", local.SQL_EF.format(ef=ef)) for ef in local.EFS] \
            + [("flat", local.SQL_FLAT)]
        out = {"machine": common.machine(), "date": time.strftime("%Y-%m-%d"), "version": ch.version,
               "vectors": n, "queries": len(vecs), "segments": segs, "settings": {}}
        for name, sql in settings:
            found, one = measure(ch, sql, vecs, 1)
            _, four = measure(ch, sql, vecs, 4)
            out["settings"][name] = {"sql": sql, "recall_at_10": local.recall(found, truth), "one": one, "four": four}
            print(size, name, out["settings"][name]["recall_at_10"], one["took"], four["took"], flush=True)
        ch.stop()
        common.write_json(os.path.join(HERE, "raw", f"server-time-{size}.json"), out)


if __name__ == "__main__":
    main()
