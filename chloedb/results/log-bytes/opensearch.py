#!/usr/bin/env python3
"""Bytes a line takes in OpenSearch, in an index shaped as log_bytes.rs's
table: id (also the document's _id, as Strata's primary key), ts, host,
service, severity, pid and body (text, positions indexed, as Strata's
TEXT), one shard, no replicas, force-merged to one segment as Strata's
optimize merges. Once with the default codec (LZ4 stored fields) and once
with best_compression (DEFLATE). Talks to a single node at 127.0.0.1:9200
(no security plugin), for example opensearch-min 3.3.0 started with
`discovery.type: single-node` and a 4 GB heap.

opensearch.py NAME JSONL   (prints one JSON line; deletes its indexes after)
"""
import http.client
import json
import queue
import sys
import threading
import time

NAME, PATH = sys.argv[1], sys.argv[2]
HOST, PORT = "127.0.0.1", 9200
BATCH, THREADS = 5000, 4


def call(method, path, body=None, conn=None):
    c = conn or http.client.HTTPConnection(HOST, PORT, timeout=3600)
    data = body if isinstance(body, (bytes, type(None))) else json.dumps(body).encode()
    c.request(method, path, body=data, headers={"content-type": "application/x-ndjson" if path.endswith("_bulk") else "application/json"})
    r = c.getresponse()
    out = json.loads(r.read() or b"null")
    if r.status >= 300:
        raise SystemExit(f"{method} {path}: {r.status} {out}")
    return out


MAPPINGS = {
    "dynamic": "strict",
    "properties": {
        "id": {"type": "long"},
        "ts": {"type": "date", "format": "epoch_millis"},
        "host": {"type": "keyword"},
        "service": {"type": "keyword"},
        "severity": {"type": "keyword"},
        "pid": {"type": "long"},
        "body": {"type": "text"},
    },
}


def load(index):
    q = queue.Queue(maxsize=THREADS * 2)
    errors = []

    def worker():
        conn = http.client.HTTPConnection(HOST, PORT, timeout=3600)
        while (body := q.get()) is not None:
            r = call("POST", f"/{index}/_bulk", body, conn)
            if r.get("errors"):
                errors.append(next(i for i in r["items"] if "error" in i["index"]))
        conn.close()

    threads = [threading.Thread(target=worker) for _ in range(THREADS)]
    for t in threads:
        t.start()
    lines, raw, buf = 0, 0, []
    with open(PATH, "rb") as f:
        for line in f:
            raw += len(line)
            r = json.loads(line)
            doc = {"id": lines, "ts": r["ts"] // 1000, "host": r.get("host"), "service": r.get("service"), "severity": r.get("severity"), "pid": r.get("pid"), "body": r.get("body")}
            buf.append(json.dumps({"index": {"_id": str(lines)}}))
            buf.append(json.dumps({k: v for k, v in doc.items() if v is not None}))
            lines += 1
            if lines % BATCH == 0:
                q.put(("\n".join(buf) + "\n").encode())
                buf = []
    if buf:
        q.put(("\n".join(buf) + "\n").encode())
    for _ in threads:
        q.put(None)
    for t in threads:
        t.join()
    if errors:
        raise SystemExit(f"bulk errors, first: {errors[0]}")
    return lines, raw


out = {"input": NAME}
for codec in ["default", "best_compression"]:
    index = f"probe-{NAME}-{codec.replace('_', '-')}"
    call("DELETE", f"/{index}?ignore_unavailable=true")
    call("PUT", f"/{index}", {"settings": {"number_of_shards": 1, "number_of_replicas": 0, "refresh_interval": "-1", "codec": codec}, "mappings": MAPPINGS})
    t = time.time()
    lines, raw = load(index)
    load_s = time.time() - t
    call("POST", f"/{index}/_refresh")
    t = time.time()
    call("POST", f"/{index}/_forcemerge?max_num_segments=1")
    # The merged segment replaces the others (whose files then go) once a
    # refresh opens it.
    call("POST", f"/{index}/_refresh")
    call("POST", f"/{index}/_flush")
    merge_s = time.time() - t
    stats = call("GET", f"/{index}/_stats/store,docs,segments?include_segment_file_sizes=true")["indices"][index]["primaries"]
    assert stats["docs"]["count"] == lines, stats["docs"]
    assert stats["segments"]["count"] == 1, stats["segments"]
    per = lambda b: round(b / lines, 1)
    out.update({
        "lines": lines,
        "input_bytes_per_line": per(raw),
        f"opensearch_{codec}_bytes_per_line": per(stats["store"]["size_in_bytes"]),
        f"opensearch_{codec}_segments": stats["segments"]["count"],
        f"opensearch_{codec}_load_seconds": round(load_s, 1),
        f"opensearch_{codec}_merge_seconds": round(merge_s, 1),
        # By Lucene file: fdt stored fields (_source and _id), tim/tip/tmd
        # terms, doc postings, pos positions, dvd doc values, kdd points,
        # nvd norms.
        f"opensearch_{codec}_files_bytes_per_line": {k: per(v["size_in_bytes"]) for k, v in stats["segments"]["file_sizes"].items() if per(v["size_in_bytes"]) >= 0.1},
    })
    call("DELETE", f"/{index}")
print(json.dumps(out))
