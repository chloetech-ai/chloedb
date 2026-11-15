#!/bin/bash
# start.sh DATASET SLOTS [server args...]: a fresh copy of DATASET served.
set -e
pkill -x chloedb-server 2>/dev/null && sleep 1 || true
rm -rf /work/run && mkdir -p /work/run && cp -R /work/data/$1/. /work/run/
slots=$2; shift 2
setsid nohup /work/bin/chloedb-server --db /work/run/db.strata --listen 0.0.0.0:7710 --grpc 0.0.0.0:7711 \
  --max-concurrent "$slots" --timeout-ms 30000 --no-token --answer-cache-mb 0 "$@" > /work/server.log 2>&1 < /dev/null &
echo $! > /work/server.pid
for i in $(seq 1 240); do
  curl -sf http://localhost:7710/v1/health >/dev/null 2>&1 && exit 0
  kill -0 $(cat /work/server.pid) 2>/dev/null || { tail -5 /work/server.log; exit 1; }
  sleep 0.5
done
echo "not healthy"; exit 1
