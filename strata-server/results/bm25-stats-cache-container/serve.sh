#!/bin/bash
# Two shards and a coordinator on this machine, each pinned to a core of
# its own (shard 0: cpu0, shard 1: cpu1, coordinator: cpu2; the client
# goes on cpu3). Usage: serve.sh BIN_DIR [extra coordinator args...]
# Stops any earlier instance first.
set -u
BIN=$1; shift
D=${BM25_DATA:-$HOME/bm25-data}
pkill -f "strata-server --db $D/s" 2>/dev/null
pkill -f "strata-server --listen 127.0.0.1:7700" 2>/dev/null
sleep 1
taskset -c 0 "$BIN/strata-server" --db $D/s0.strata --listen 127.0.0.1:7710 --grpc 127.0.0.1:7711 --timeout-ms 30000 > $D/s0.log 2>&1 &
taskset -c 1 "$BIN/strata-server" --db $D/s1.strata --listen 127.0.0.1:7720 --grpc 127.0.0.1:7721 --timeout-ms 30000 > $D/s1.log 2>&1 &
for p in 7710 7720; do until curl -sf http://127.0.0.1:$p/v1/health > /dev/null; do sleep 0.5; done; done
taskset -c 2 "$BIN/strata-server" --listen 127.0.0.1:7700 --grpc=off --shards=http://127.0.0.1:7711,http://127.0.0.1:7721 --timeout-ms=30000 "$@" > $D/coord.log 2>&1 &
until curl -sf http://127.0.0.1:7700/v1/health > /dev/null; do sleep 0.5; done
echo up
