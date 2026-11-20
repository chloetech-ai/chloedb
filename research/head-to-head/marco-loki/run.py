"""Marco's logs: ChloeDB against Loki, on an hour of his kind of lines.

Marco, a platform engineer in the persona study, runs Loki for most logs and
an old Elasticsearch for substring search. In his adoption answer he said a
substring grep over 15 minutes takes Loki seconds where ChloeDB took
milliseconds, and that ChloeDB's disk "must come within about 2x of Loki's
cost per day for the same lines, counted from the file". This measures both.

lines.py makes an hour of lines at 1,000 a second (edge nginx, JSON services
with trace ids, network syslog). The same lines go to

- ChloeDB, through `POST /v1/logs` as OpenTelemetry's Collector sends them
  (OTLP JSON, 2,000 records a request, four senders), twice: into the logs
  guide's table (a substring index on `host` and `body`, `trace_id` kept,
  zstd), and into the table `/v1/logs` makes itself (no substring index);
- Loki 3.5, one process with its chunks on the local disk, through
  `/loki/api/v1/push` (the same requests' lines, labels job, service, host and
  level, trace_id as structured metadata), twice: with its default chunk
  encoding (snappy) and with zstd.

Each answers the same six on-call questions, timed 7 times; each answer is
checked against the lines themselves; then the disk each keeps for the hour.

    python3 run.py

Needs Docker (grafana/loki:3.5.5, through mirror.gcr.io) and a chloedb-server
build. Raw numbers go to raw/results.json.
"""

import datetime as dt
import http.client
import json
import os
import shutil
import statistics
import subprocess
import sys
import threading
import time
import urllib.parse
import zlib

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import common  # noqa: E402
import lines as L  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
WORK = os.environ.get("H2H_WORK", "/tmp/h2h-marco")
RAW = os.path.join(HERE, "raw")
LOKI_IMAGE = "mirror.gcr.io/grafana/loki:3.5.5"
LOKI_PORT = 3100
PER_REQUEST = 2000
SENDERS = 4
REPS = 7
TABLE = "logs_edge"

GUIDE_TABLE = f"""CREATE TABLE {TABLE} (
  ts TIMESTAMP NOT NULL,
  severity KEYWORD,
  severity_number INT,
  host KEYWORD OPTIONS(substring = true),
  service KEYWORD,
  body TEXT OPTIONS(substring = true),
  trace_id KEYWORD,
  attributes JSON
) OPTIONS(retain = '7 days', compression = 'zstd')"""


def iso(ns):
    return dt.datetime.fromtimestamp(ns / 1e9, dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%fZ")


def epoch_s(v):
    if isinstance(v, (int, float)):
        return int(v / 1e6) if v > 1e14 else int(v)
    return int(dt.datetime.fromisoformat(str(v).replace("Z", "+00:00")).timestamp())


# ------------------------------------------------------------------ requests


def build_requests(rows, folder):
    """The hour as requests of PER_REQUEST lines, in time order, for each system: OTLP JSON for ChloeDB,
    Loki's push JSON for Loki. Written once to disk, so sending times only the sending."""
    os.makedirs(os.path.join(folder, "otlp"), exist_ok=True)
    os.makedirs(os.path.join(folder, "loki"), exist_ok=True)
    sizes = {"otlp": 0, "loki": 0}
    n = 0
    for i in range(0, len(rows), PER_REQUEST):
        part = rows[i:i + PER_REQUEST]
        res, streams = {}, {}
        for x in part:
            rk = None if x["job"] == "syslog" else (x["service"], x["host"])
            rec = {"timeUnixNano": str(x["ts"]), "severityText": x["sev_text"], "body": {"stringValue": x["body"]}}
            if x["sev_num"] is not None:
                rec["severityNumber"] = x["sev_num"]
            if x["trace_id"]:
                rec["traceId"] = x["trace_id"]
            if x["attributes"]:
                rec["attributes"] = [{"key": k, "value": {"stringValue": v}} for k, v in x["attributes"].items()]
            res.setdefault(rk, []).append(rec)
            lk = (x["job"], x["service"], x["host"], x["level"])
            v = [str(x["ts"]), x["body"]]
            if x["trace_id"]:
                v.append({"trace_id": x["trace_id"]})
            streams.setdefault(lk, []).append(v)
        otlp = {"resourceLogs": [
            {"resource": {"attributes": [] if rk is None else [
                {"key": "service.name", "value": {"stringValue": rk[0]}},
                {"key": "host.name", "value": {"stringValue": rk[1]}}]},
             "scopeLogs": [{"logRecords": recs}]} for rk, recs in res.items()]}
        loki = {"streams": [{"stream": {"job": k[0], "service": k[1], "host": k[2], "level": k[3]}, "values": v}
                            for k, v in streams.items()]}
        for kind, body in (("otlp", otlp), ("loki", loki)):
            b = json.dumps(body, separators=(",", ":")).encode()
            sizes[kind] += len(b)
            with open(os.path.join(folder, kind, f"{n:06d}.json"), "wb") as f:
                f.write(b)
        n += 1
    return n, sizes


def send_all(folder, kind, port, path):
    """Send every request with SENDERS threads over keep-alive connections; a 429 or 503 is sent again after
    its Retry-After, as an exporter does. Answers the wall time and what happened."""
    files = sorted(os.listdir(os.path.join(folder, kind)))
    nxt = [0]
    lock = threading.Lock()
    stats = {"requests": len(files), "retries": 0, "failed": 0, "statuses": {}, "first_error": None}

    def worker():
        conn = http.client.HTTPConnection("127.0.0.1", port, timeout=300)
        while True:
            with lock:
                if nxt[0] >= len(files):
                    break
                i = nxt[0]
                nxt[0] += 1
            body = open(os.path.join(folder, kind, files[i]), "rb").read()
            for attempt in range(200):
                try:
                    conn.request("POST", path, body=body, headers={"content-type": "application/json"})
                    r = conn.getresponse()
                    text = r.read()
                    st = r.status
                    wait = float(r.getheader("retry-after") or 0.25)
                except (OSError, http.client.HTTPException) as e:
                    conn.close()
                    conn = http.client.HTTPConnection("127.0.0.1", port, timeout=300)
                    st, text, wait = 0, str(e).encode(), 0.25
                with lock:
                    stats["statuses"][str(st)] = stats["statuses"].get(str(st), 0) + 1
                if st in (200, 204):
                    break
                if st in (0, 429, 502, 503, 504):
                    with lock:
                        stats["retries"] += 1
                    time.sleep(min(wait, 5))
                    continue
                with lock:
                    stats["failed"] += 1
                    stats["first_error"] = stats["first_error"] or f"{st}: {text[:300]!r}"
                break
        conn.close()

    t = time.perf_counter()
    ths = [threading.Thread(target=worker) for _ in range(SENDERS)]
    for th in ths:
        th.start()
    for th in ths:
        th.join()
    stats["seconds"] = round(time.perf_counter() - t, 2)
    return stats


def get(port, path, params=None, timeout=300):
    q = "" if not params else "?" + urllib.parse.urlencode(params)
    conn = http.client.HTTPConnection("127.0.0.1", port, timeout=timeout)
    conn.request("GET", path + q)
    r = conn.getresponse()
    body = r.read()
    conn.close()
    return r.status, (json.loads(body) if body[:1] in (b"{", b"[") else body.decode(errors="replace"))


def post(port, path, obj, timeout=300):
    conn = http.client.HTTPConnection("127.0.0.1", port, timeout=timeout)
    conn.request("POST", path, body=json.dumps(obj), headers={"content-type": "application/json"})
    r = conn.getresponse()
    body = r.read()
    conn.close()
    return r.status, (json.loads(body) if body[:1] in (b"{", b"[") else body.decode(errors="replace"))


def timed(fn, budget_s=120):
    """REPS runs, or fewer once a slow query has used budget_s (at least 2)."""
    ms, out = [], None
    for _ in range(REPS):
        t = time.perf_counter()
        out = fn()
        ms.append((time.perf_counter() - t) * 1000)
        if len(ms) >= 2 and sum(ms) / 1000 > budget_s:
            break
    return out, {"p50_ms": round(statistics.median(ms), 2), "max_ms": round(max(ms), 2), "first_ms": round(ms[0], 2),
                 "runs": len(ms)}


# ------------------------------------------------------------------ the questions


def chloedb_sql(end_ns):
    q = L.questions(end_ns)
    w = lambda name: f"ts > '{iso(q[name][1])}' AND ts <= '{iso(end_ns)}'"  # noqa: E731
    return {
        "ip": f"SELECT body FROM {TABLE} WHERE body LIKE '%{L.NEEDLE_IP}%' AND {w('ip')} ORDER BY ts DESC LIMIT 5000",
        "trace": f"SELECT body FROM {TABLE} WHERE trace_id = '{L.TRACE}' AND {w('trace')} ORDER BY ts",
        "errors_per_minute": f"SELECT time_bucket('1 minute', ts) AS minute, service, count(*) AS errors FROM {TABLE} "
                             f"WHERE severity_number >= 17 AND {w('errors_per_minute')} GROUP BY minute, service",
        "status_503": f"SELECT host, count(*) AS n FROM {TABLE} WHERE service = 'edge-nginx' AND body LIKE '%\" 503 %' "
                      f"AND {w('status_503')} GROUP BY host",
        "timeout": f"SELECT count(*) FROM {TABLE} WHERE body ILIKE '%timeout%' AND {w('timeout')}",
        "tail": f"SELECT body FROM {TABLE} WHERE service = 'checkout' AND {w('tail')} ORDER BY ts DESC LIMIT 100",
    }


def chloedb_answer(name, rows):
    if name in ("ip", "trace", "tail"):
        return sorted(r[0] for r in rows)
    if name == "errors_per_minute":
        return {f"{epoch_s(r[0])}|{r[1]}": int(r[2]) for r in sorted(rows, key=lambda r: (epoch_s(r[0]), r[1]))}
    if name == "status_503":
        return {r[0]: int(r[1]) for r in sorted(rows)}
    return int(rows[0][0])


def logql(end_ns):
    q = L.questions(end_ns)
    s = lambda name: q[name][1]  # noqa: E731
    return {
        "ip": ("query_range", {"query": f'{{job=~".+"}} |= "{L.NEEDLE_IP}"', "start": s("ip"), "end": end_ns,
                               "limit": 5000, "direction": "backward"}),
        "trace": ("query_range", {"query": f'{{job="app"}} |= "{L.TRACE}"', "start": s("trace"), "end": end_ns,
                                  "limit": 5000, "direction": "forward"}),
        "errors_per_minute": ("query_range", {"query": 'sum by (service) (count_over_time({level="ERROR"}[1m]))',
                                              "start": s("errors_per_minute") + 60 * 10**9, "end": end_ns, "step": 60}),
        "status_503": ("query", {"query": 'sum by (host) (count_over_time({job="nginx"} |= "\\" 503 " [1h]))',
                                 "time": end_ns}),
        "timeout": ("query", {"query": 'sum(count_over_time({job=~".+"} |~ "(?i)timeout" [15m]))', "time": end_ns}),
        "tail": ("query_range", {"query": '{service="checkout"}', "start": s("tail"), "end": end_ns, "limit": 100,
                                 "direction": "backward"}),
    }


def loki_answer(name, data):
    res = data["data"]["result"]
    if name in ("ip", "trace", "tail"):
        return sorted(v[1] for s in res for v in s["values"])
    if name == "errors_per_minute":
        out = {}
        for s in res:
            for t, v in s["values"]:
                if int(v):
                    out[f"{int(float(t)) - 60}|{s['metric'].get('service')}"] = int(v)
        return dict(sorted(out.items()))
    if name == "status_503":
        return {s["metric"]["host"]: int(s["value"][1]) for s in sorted(res, key=lambda s: s["metric"]["host"])}
    return int(res[0]["value"][1]) if res else 0


def check(name, got, want):
    if isinstance(want, int):
        return {"agree": got == want, "got": got, "want": want}
    if isinstance(want, dict):
        diff = sorted(k for k in set(got) | set(want) if got.get(k) != want.get(k))
        return {"agree": not diff, "values": len(want), "differ": diff[:10]}
    return {"agree": got == want, "lines": len(got), "want_lines": len(want),
            "missing": len([x for x in want if x not in set(got)]), "extra": len([x for x in got if x not in set(want)])}


# ------------------------------------------------------------------ ChloeDB


def run_chloedb(config, reqs, n_lines, end_ns, want):
    work = os.path.join(WORK, f"chloedb-{config}")
    shutil.rmtree(work, ignore_errors=True)
    os.makedirs(work)
    port = common.free_port()
    log = open(os.path.join(work, "server.log"), "w")
    srv = subprocess.Popen([common.server_bin(), "--db", os.path.join(work, "logs.chloedb"), "--listen",
                            f"127.0.0.1:{port}", "--grpc", "off", "--start-apps", "off", "--answer-cache-mb", "0"],
                           stdout=log, stderr=subprocess.STDOUT)
    out = {"config": config}
    try:
        for _ in range(300):
            try:
                if get(port, "/v1/health")[0] == 200:
                    break
            except OSError:
                time.sleep(0.1)
        out["version"] = subprocess.run([common.server_bin(), "--version"], capture_output=True, text=True).stdout.strip()
        if config == "guide":
            st, r = post(port, "/v1/sql", {"sql": GUIDE_TABLE})
            if st != 200 or (isinstance(r, dict) and r.get("error")):
                raise SystemExit(f"create table: {st} {r}")
        out["load"] = send_all(reqs, "otlp", port, f"/v1/logs?table={TABLE}")
        out["load"]["lines_per_s"] = round(n_lines / out["load"]["seconds"])
        count = lambda: post(port, "/v1/query", {"sql": f"SELECT count(*) FROM {TABLE}"})[1]["rows"][0][0]  # noqa: E731
        t = time.perf_counter()
        while count() < n_lines and time.perf_counter() - t < 300:
            time.sleep(1)
        out["stored"] = count()
        out["sent"] = n_lines
        time.sleep(5)
        out["memory_kb"] = common.pss_kb(common.tree(srv.pid))
        out["questions"] = {}
        for name, sql in chloedb_sql(end_ns).items():
            # once as an on-call page sends it, with the server's default deadline (5 s)
            st, r = post(port, "/v1/query", {"sql": sql})
            default = {"status": st, "error_code": r.get("error_code") if isinstance(r, dict) else None}

            def q(sql=sql):
                st, r = post(port, "/v1/query", {"sql": sql, "timeout_ms": 600000}, timeout=900)
                if st != 200:
                    raise SystemExit(f"{name}: {st} {r}")
                return r["rows"]
            rows, tm = timed(q)
            out["questions"][name] = {"sql": sql, "default_deadline": default, **tm,
                                      **check(name, chloedb_answer(name, rows), want[name])}
            print(f"  chloedb-{config} {name}: {tm['p50_ms']} ms, agree {out['questions'][name]['agree']}", flush=True)
        st, tables = get(port, "/v1/index/tables")
        out["tables_totals"] = tables.get("totals") if isinstance(tables, dict) else None
        out["disk_live_bytes"] = common.du_bytes(work) - os.path.getsize(os.path.join(work, "server.log"))
        # the server gives free space at the end of the file back by itself every 5 minutes: wait for it, as a
        # disk holds the file a while after the lines came in
        out["give_back"] = []
        t = time.perf_counter()
        while time.perf_counter() - t < 9 * 60:
            time.sleep(20)
            tot = get(port, "/v1/index/tables")[1].get("totals", {})
            out["give_back"].append({"s": round(time.perf_counter() - t), "file_bytes": tot.get("file_bytes"),
                                     "free_bytes": tot.get("free_bytes")})
            if tot.get("file_bytes", 0) < out["tables_totals"]["file_bytes"] and out["give_back"][-1]["s"] > 330:
                break
        out["tables_totals_after"] = get(port, "/v1/index/tables")[1].get("totals")
    finally:
        srv.terminate()
        srv.wait(60)
        log.close()
    out["disk_stopped_bytes"] = common.du_bytes(work) - os.path.getsize(os.path.join(work, "server.log"))
    out["files_stopped"] = {f: os.path.getsize(os.path.join(work, f)) for f in sorted(os.listdir(work))
                            if os.path.isfile(os.path.join(work, f)) and f != "server.log"}
    return out


# ------------------------------------------------------------------ Loki


def loki_config(encoding):
    src = open(os.path.join(HERE, "loki.yaml")).read()
    return src + f"\ningester:\n  chunk_encoding: {encoding}\n"


def run_loki(encoding, reqs, n_lines, end_ns, want):
    work = os.path.join(WORK, f"loki-{encoding}")
    subprocess.run(["docker", "rm", "-f", "h2h-loki"], capture_output=True)
    shutil.rmtree(work, ignore_errors=True)
    os.makedirs(os.path.join(work, "data"))
    os.chmod(os.path.join(work, "data"), 0o777)
    with open(os.path.join(work, "loki.yaml"), "w") as f:
        f.write(loki_config(encoding))
    subprocess.run(["docker", "run", "-d", "--name", "h2h-loki", "--network", "host", "-v",
                    f"{work}/loki.yaml:/etc/loki/local.yaml:ro", "-v", f"{work}/data:/loki", LOKI_IMAGE,
                    "-config.file=/etc/loki/local.yaml"], check=True, capture_output=True)
    out = {"config": encoding, "version": LOKI_IMAGE.rsplit("/", 1)[1]}
    try:
        for _ in range(600):
            try:
                if get(LOKI_PORT, "/ready")[0] == 200:
                    break
            except OSError:
                pass
            time.sleep(0.5)
        out["load"] = send_all(reqs, "loki", LOKI_PORT, "/loki/api/v1/push")
        out["load"]["lines_per_s"] = round(n_lines / out["load"]["seconds"])
        out["stored"] = count_loki(end_ns)
        out["sent"] = n_lines
        pid = int(subprocess.run(["docker", "inspect", "-f", "{{.State.Pid}}", "h2h-loki"], capture_output=True,
                                 text=True).stdout.strip())
        time.sleep(5)
        out["memory_kb"] = common.pss_kb(common.tree(pid))
        out["questions"] = ask_loki(end_ns, want, "live")
        out["disk_before_flush"] = parts(os.path.join(work, "data"))
        st, _ = post(LOKI_PORT, "/flush", {})
        out["flush_status"] = st
        settle(os.path.join(work, "data", "chunks"))
        out["disk"] = parts(os.path.join(work, "data"))
        out["questions_after_flush"] = ask_loki(end_ns, want, "after flush")
    finally:
        out["log_tail"] = subprocess.run(["docker", "logs", "--tail", "20", "h2h-loki"], capture_output=True,
                                         text=True).stderr[-3000:]
        subprocess.run(["docker", "rm", "-f", "h2h-loki"], capture_output=True)
    return out


def count_loki(end_ns):
    st, r = get(LOKI_PORT, "/loki/api/v1/query", {"query": 'sum(count_over_time({job=~".+"}[2h]))', "time": end_ns + 60 * 10**9})
    return int(r["data"]["result"][0]["value"][1]) if st == 200 and r["data"]["result"] else None


def ask_loki(end_ns, want, label):
    res = {}
    for name, (kind, params) in logql(end_ns).items():
        def q(kind=kind, params=params):
            st, r = get(LOKI_PORT, f"/loki/api/v1/{kind}", params)
            if st != 200:
                raise SystemExit(f"loki {name}: {st} {r}")
            return r
        r, tm = timed(q)
        res[name] = {"logql": params["query"], **tm, **check(name, loki_answer(name, r), want[name])}
        print(f"  loki {label} {name}: {tm['p50_ms']} ms, agree {res[name]['agree']}", flush=True)
    return res


def parts(data):
    """Loki's disk by part: the chunks (what object storage holds), the index, and the write-ahead log."""
    out = {}
    for name in sorted(os.listdir(data)):
        p = os.path.join(data, name)
        out[name] = common.du_bytes(p) if os.path.isdir(p) else os.path.getsize(p)
    return out


def settle(path, quiet_s=15, limit_s=600):
    last, since, t = -1, time.time(), time.time()
    while time.time() - t < limit_s:
        now = common.du_bytes(path)
        if now != last:
            last, since = now, time.time()
        elif time.time() - since >= quiet_s:
            return
        time.sleep(1)


# ------------------------------------------------------------------ main


def main():
    os.makedirs(WORK, exist_ok=True)
    os.makedirs(RAW, exist_ok=True)
    path = os.path.join(RAW, "results.json")
    res = json.load(open(path)) if os.path.exists(path) and os.environ.get("H2H_ONLY") else {}
    end_ns = int(time.time() // 60 * 60) * 10**9
    rows = L.lines(end_ns)
    want = L.truth(rows, end_ns)
    reqs = os.path.join(WORK, "requests")
    shutil.rmtree(reqs, ignore_errors=True)
    n_req, sizes = build_requests(rows, reqs)
    text = "\n".join(x["body"] for x in rows).encode() + b"\n"
    res.update({"at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "machine": common.machine(),
                "lines": len(rows), "rate": L.RATE, "seconds": L.SECONDS, "end": iso(end_ns), "requests": n_req,
                "per_request": PER_REQUEST, "senders": SENDERS, "request_bytes": sizes, "raw_bytes": len(text),
                "gzip_bytes": len(zlib.compress(text, 6)),
                "truth": {k: (v if isinstance(v, int) else len(v)) for k, v in want.items()},
                "by_job": {j: sum(1 for x in rows if x["job"] == j) for j in ("nginx", "app", "syslog")}})
    del text
    only = os.environ.get("H2H_ONLY")
    for side, config in (("chloedb", "guide"), ("chloedb", "otlp"), ("loki", "snappy"), ("loki", "zstd")):
        key = f"{side}-{config}"
        if only and only not in (side, key):
            continue
        print(key, flush=True)
        res[key] = (run_chloedb if side == "chloedb" else run_loki)(config, reqs, len(rows), end_ns, want)
        common.write_json(path, res)
    shutil.rmtree(reqs, ignore_errors=True)
    print("wrote raw/results.json")


if __name__ == "__main__":
    main()
