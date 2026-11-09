#!/usr/bin/env bash
# A local ChloeDB cluster for Parley: shards and a coordinator on this machine.
#
#   scripts/cluster.sh up DIR [N]        N shards (default 2) and a coordinator on 127.0.0.1:7700
#   scripts/cluster.sh adopt DIR URL     a coordinator over a server that already runs (its gRPC URL):
#                                        the day-one server becomes shard 0, nothing is copied
#   scripts/cluster.sh grow DIR          one more shard, and move slots onto it while serving
#   scripts/cluster.sh status DIR        the shards, their rows, and slots on the move
#   scripts/cluster.sh down DIR          stop everything it started
#
# Shard i listens on HTTP 7710+10i and gRPC 7711+10i. Pictures are kept by each
# shard in DIR/media-i (a coordinator does not serve them; Parley asks shards).
set -euo pipefail

BIN="${CHLOEDB_SERVER:-$(cd "$(dirname "$0")/../../.." && pwd)/chloedb-server/target/release/chloedb-server}"
COORD="${PARLEY_COORD_LISTEN:-127.0.0.1:7700}"
cmd="${1:-}"; dir="${2:-}"
[ -n "$cmd" ] && [ -n "$dir" ] || { sed -n '2,13p' "$0"; exit 2; }
[ -x "$BIN" ] || { echo "no chloedb-server at $BIN (cd chloedb-server && cargo build --release)" >&2; exit 1; }
mkdir -p "$dir"

wait_up() {
  for _ in $(seq 1 300); do curl -sf "$1/v1/health" >/dev/null && return 0; sleep 0.2; done
  echo "$1 did not come up; see $dir/*.log" >&2; return 1
}

start_shard() {
  local i="$1" http=$((7710 + 10 * $1)) grpc=$((7711 + 10 * $1))
  nohup "$BIN" --db "$dir/shard-$i.strata" --seq-no --listen "127.0.0.1:$http" --grpc "127.0.0.1:$grpc" \
    --media-store "$dir/media-$i" --no-token >"$dir/shard-$i.log" 2>&1 &
  echo $! >"$dir/shard-$i.pid"
  wait_up "http://127.0.0.1:$http"
  echo "http://127.0.0.1:$grpc" >>"$dir/shards.grpc"
  echo "http://127.0.0.1:$http" >>"$dir/shards.http"
  echo "shard $i: http://127.0.0.1:$http (gRPC $grpc)"
}

start_coord() {
  local shards; shards=$(paste -sd, "$dir/shards.grpc")
  nohup "$BIN" --shards "$shards" --listen "$COORD" --grpc off --no-token >"$dir/coord.log" 2>&1 &
  echo $! >"$dir/coord.pid"
  wait_up "http://$COORD"
  echo "coordinator: http://$COORD over $shards"
}

case "$cmd" in
  up)
    n="${3:-2}"; rm -f "$dir/shards.grpc" "$dir/shards.http"
    for i in $(seq 0 $((n - 1))); do start_shard "$i"; done
    start_coord
    echo "parley: python -m parley serve --server http://$COORD --shards $(paste -sd, "$dir/shards.http")"
    ;;
  adopt)
    url="${3:?give the gRPC URL of the running server, e.g. http://127.0.0.1:7701}"
    echo "$url" >"$dir/shards.grpc"; echo "${4:-}" >"$dir/shards.http"
    start_coord
    ;;
  grow)
    i=$(wc -l <"$dir/shards.grpc" | tr -d ' ')
    start_shard "$i"
    grpc=$(tail -n1 "$dir/shards.grpc")
    curl -sf "http://$COORD/v1/admin/shards" -H 'content-type: application/json' -d "{\"servers\": [\"$grpc\"]}"; echo
    curl -sf -X POST "http://$COORD/v1/admin/rebalance" -H 'content-type: application/json' -d '{}'; echo
    ;;
  status)
    curl -sf "http://$COORD/v1/cluster" | python3 -c '
import json, sys
d = json.load(sys.stdin)
for s in d.get("shards", []):
    reps = s.get("replicas") or [s]
    for r in reps:
        h = ((r.get("detail") or {}).get("health") or {})
        tables = h.get("tables") or {}
        print(s.get("id", "?"), r.get("url") or r.get("addr"), "slots", s.get("slots_held"), {t: (v.get("rows") if isinstance(v, dict) else v) for t, v in tables.items()})
' || curl -s "http://$COORD/v1/cluster"
    curl -s "http://$COORD/v1/admin/moves"; echo
    ;;
  down)
    for f in "$dir"/*.pid; do [ -f "$f" ] && kill "$(cat "$f")" 2>/dev/null || true; rm -f "$f"; done
    ;;
  *) sed -n '2,13p' "$0"; exit 2 ;;
esac
