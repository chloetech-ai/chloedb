#!/usr/bin/env python3
"""Sends a log file of parse.py's JSON lines to a Strata server as an
OpenTelemetry exporter would: OTLP/HTTP JSON, gzip-compressed, to
POST /v1/logs, BATCH records a request, each request's records grouped by
resource (service.name and host.name). The pid is the record's attribute
`pid`; the severity its text.

    otlp_send.py URL IN.jsonl [BATCH]

Prints one JSON line: lines sent, requests, seconds, lines a second, and
the records any request refused.
"""
import gzip
import json
import sys
import time
import urllib.request


def records(lines):
    """The request body of a batch of parsed lines."""
    by_resource = {}
    for r in lines:
        rec = {"timeUnixNano": str(r["ts"] * 1000), "body": {"stringValue": r["body"]}}
        if r.get("severity"):
            rec["severityText"] = r["severity"]
        if r.get("pid") is not None:
            rec["attributes"] = [{"key": "pid", "value": {"intValue": str(r["pid"])}}]
        by_resource.setdefault((r.get("service"), r.get("host")), []).append(rec)
    out = []
    for (service, host), recs in by_resource.items():
        attrs = []
        if service:
            attrs.append({"key": "service.name", "value": {"stringValue": service}})
        if host:
            attrs.append({"key": "host.name", "value": {"stringValue": host}})
        out.append({"resource": {"attributes": attrs}, "scopeLogs": [{"logRecords": recs}]})
    return {"resourceLogs": out}


def send(url, body):
    data = gzip.compress(json.dumps(body).encode(), compresslevel=1)
    req = urllib.request.Request(url + "/v1/logs", data=data, headers={"content-type": "application/json", "content-encoding": "gzip"})
    for attempt in range(60):
        try:
            with urllib.request.urlopen(req, timeout=120) as a:
                return json.loads(a.read() or b"{}")
        except urllib.error.HTTPError as e:
            if e.code in (429, 503):
                time.sleep(float(e.headers.get("retry-after", "1")))
                continue
            raise SystemExit(f"{e.code}: {e.read()[:500]!r}")
    raise SystemExit("the server stayed busy")


def main():
    url, path = sys.argv[1], sys.argv[2]
    batch = int(sys.argv[3]) if len(sys.argv) > 3 else 5000
    t0 = time.time()
    sent = requests = rejected = 0
    pending = []
    with open(path) as f:
        for line in f:
            pending.append(json.loads(line))
            if len(pending) == batch:
                a = send(url, records(pending))
                rejected += int(a.get("partialSuccess", {}).get("rejectedLogRecords", 0))
                sent, requests, pending = sent + len(pending), requests + 1, []
    if pending:
        a = send(url, records(pending))
        rejected += int(a.get("partialSuccess", {}).get("rejectedLogRecords", 0))
        sent, requests = sent + len(pending), requests + 1
    s = time.time() - t0
    print(json.dumps({"input": path.rsplit("/", 1)[-1], "lines": sent, "requests": requests, "batch": batch, "seconds": round(s, 1), "lines_per_second": round(sent / s), "rejected": rejected}))


if __name__ == "__main__":
    main()
