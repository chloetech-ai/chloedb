#!/bin/sh
# Bytes a line takes in VictoriaLogs: ingests /data/NAME.jsonl into an
# empty store (the body as _msg, host and service as the stream, the
# rest as fields), flushes, merges, and prints the store's size on disk
# per line as one JSON line. Starts its own container:
#   victorialogs.sh NAME JSONL_DIR WORK_DIR   (STREAM=service: the stream fields, default host,service)
set -e
n=$1; jsonl=$2; work=$3
rm -rf "$work/vl-$n"; mkdir -p "$work/vl-$n"
docker rm -f vl-probe >/dev/null 2>&1 || true
docker run -d --name vl-probe -p 9429:9428 -v "$work/vl-$n:/vl" victoriametrics/victoria-logs:v1.36.1 \
  -storageDataPath=/vl -retentionPeriod=100y -futureRetention=100y >/dev/null
until curl -sf localhost:9429/health >/dev/null; do sleep 1; done
curl -sSf -X POST -H 'Content-Type: application/stream+json' -T "$jsonl/$n.jsonl" \
  "http://localhost:9429/insert/jsonline?_time_field=ts&_msg_field=body&_stream_fields=${STREAM:-host,service}"
curl -sSf localhost:9429/internal/force_flush
curl -sSf "localhost:9429/internal/force_merge"
# force_merge returns at once: wait until no merge has run for 10 s.
quiet=0
while [ $quiet -lt 10 ]; do
  active=$(curl -sf localhost:9429/metrics | awk '/^vl_active_merges/ {s += $2} END {print s + 0}')
  if [ "$active" = 0 ]; then quiet=$((quiet + 1)); else quiet=0; fi
  sleep 1
done
rows=$(curl -sf localhost:9429/select/logsql/query -d 'query=_time:100y | stats count() n' | sed 's/.*"n":"\([0-9]*\)".*/\1/')
docker stop vl-probe >/dev/null
bytes=$(du -sb "$work/vl-$n" | cut -f1)
docker rm vl-probe >/dev/null
echo "{\"input\":\"$n\",\"lines\":$rows,\"victorialogs_bytes_per_line\":$(echo "scale=1; $bytes * 10 / $rows / 10" | bc -l)}"
