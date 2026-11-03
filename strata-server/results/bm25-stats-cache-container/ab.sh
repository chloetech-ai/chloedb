#!/bin/bash
# Base and cache builds, alternating, PASSES times: each run on fresh
# copies of the two shard databases, the client on cpu3.
# Usage: [BUILDS="base cache"] ab.sh OUT_DIR PASSES KINDS
set -u
OUT=$1; PASSES=$2; KINDS=$3
SP=$(cd "$(dirname "$0")" && pwd)
D=${BM25_DATA:-$HOME/bm25-data}
mkdir -p "$OUT"
for pass in $(seq 1 "$PASSES"); do
  for build in ${BUILDS:-base cache}; do
    pkill -x strata-server; sleep 1
    cp $D/s0.orig $D/s0.strata; cp $D/s1.orig $D/s1.strata; rm -f $D/s0.strata-wal $D/s1.strata-wal
    "$SP/serve.sh" ${BM25_BIN:-$HOME/bm25-bin}/$build > /dev/null
    STRATA_DATA=$D taskset -c 3 ${BM25_BIN:-$HOME/bm25-bin}/$build/engine-compare --collection msmarco-1000k run --engine strata \
      --url http://127.0.0.1:7700 --kinds "$KINDS" --label "$build-p$pass" --out "$OUT" > "$OUT/$build-p$pass.log" 2>&1
    curl -s http://127.0.0.1:7700/metrics | grep -E "^strata_stats_cache" > "$OUT/$build-p$pass.metrics"
    for p in 7710 7720; do curl -s http://127.0.0.1:$p/metrics | grep -E "^strata_stale_stats_total" >> "$OUT/$build-p$pass.metrics"; done
    grep -E "c=" "$OUT/$build-p$pass.log"
  done
done
pkill -x strata-server
echo finished
