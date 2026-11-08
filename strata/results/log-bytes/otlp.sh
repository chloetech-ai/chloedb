#!/usr/bin/env bash
# Bytes per log line through OTLP ingest: starts strata-server on a new
# database, sends a log file to POST /v1/logs as an OpenTelemetry exporter
# would (otlp_send.py: OTLP/HTTP JSON, gzip, 5,000 records a request), stops
# the server, and measures table otel_logs merged to one segment
# (examples/log_bytes.rs --table). Appends one JSON line per log to
# otlp.jsonl.
#
#   otlp.sh SERVER_BIN LOG_BYTES_BIN WORK_DIR IN.jsonl...
set -euo pipefail
server=$1 log_bytes=$2 work=$3
shift 3
here=$(cd "$(dirname "$0")" && pwd)
for input in "$@"; do
  name=$(basename "$input" .jsonl)
  db="$work/otlp-$name.strata"
  rm -f "$db" "$db"-wal "$db".keys.json
  "$server" --db "$db" --listen 127.0.0.1:7790 --grpc off --log-store off >"$work/otlp-$name.log" 2>&1 &
  pid=$!
  until curl -sf 127.0.0.1:7790/v1/health >/dev/null; do sleep 0.2; done
  sent=$(python3 "$here/otlp_send.py" http://127.0.0.1:7790 "$input")
  kill -INT "$pid"
  wait "$pid" || true
  measured=$("$log_bytes" "$db" --table otel_logs)
  python3 -c 'import json,sys; s=json.loads(sys.argv[1]); m=json.loads(sys.argv[2]); m.update(input=s["input"], send=s); print(json.dumps(m))' "$sent" "$measured" | tee -a "$here/otlp.jsonl"
  rm -f "$db" "$db"-wal "$db".keys.json
done
