#!/bin/bash
# before_after.sh CLUSTER_DIR QUERIES OUT_DIR NAME=BIN...: for each server binary, on the
# cluster cluster.sh runs in CLUSTER_DIR (its data loaded by load.py):
#   NAME-cold.json  the queries by fm_time.py, the page cache dropped and the cluster just started
#   NAME-late.txt   a LIMIT 20 of words that came into use late, on each shard (ms, rows)
#   NAME-warm.json  the queries again, the cluster started anew on a warm page cache
# Each start is followed by a query of a string in no title, which opens every segment's
# readers and index (some 50 ms on a shard, once a process) before anything is timed.
# Dropping the page cache needs root.
set -e
DIR=$1; QUERIES=$2; OUT=$3; shift 3
HERE=$(cd "$(dirname "$0")" && pwd)
LATE=('%bitcoin %' '%itcoin%' '%bitcoi%' '%ubernetes%' '%chatgpt%' '%covid%')
opened() {
  curl -sf 127.0.0.1:7700/v1/query -H 'content-type: application/json' \
    -d '{"sql":"SELECT COUNT(*) FROM hn_stories WHERE title ILIKE :p","params":{"p":"%qqzzx%"}}' > /dev/null
}
for nb in "$@"; do
  NAME=${nb%%=*}; BIN=${nb#*=}
  bash "$HERE/cluster.sh" x "$DIR" stop
  bash "$HERE/cluster.sh" "$BIN" "$DIR" start > /dev/null
  sleep 5
  opened
  sync; echo 3 > /proc/sys/vm/drop_caches
  python3 -I "$HERE/fm_time.py" http://127.0.0.1:7700 "$QUERIES" "$OUT/$NAME-cold.json" > /dev/null 2>&1
  : > "$OUT/$NAME-late.txt"
  for p in "${LATE[@]}"; do
    for port in 7800 7810 7820 7830; do
      curl -s 127.0.0.1:$port/v1/query -H 'content-type: application/json' \
        -d "{\"sql\":\"SELECT id FROM hn_stories WHERE title ILIKE :p LIMIT 20\",\"params\":{\"p\":\"$p\"}}" |
        python3 -I -c "import sys, json; j = json.load(sys.stdin); print('$p', $port, round(j['took_ms'], 1), len(j['rows']), sep='\t')" >> "$OUT/$NAME-late.txt"
    done
  done
  bash "$HERE/cluster.sh" x "$DIR" stop
  bash "$HERE/cluster.sh" "$BIN" "$DIR" start > /dev/null
  sleep 5
  opened
  python3 -I "$HERE/fm_time.py" http://127.0.0.1:7700 "$QUERIES" "$OUT/$NAME-warm.json" > /dev/null 2>&1
done
bash "$HERE/cluster.sh" x "$DIR" stop
