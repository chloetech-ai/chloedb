#!/bin/sh
# The nearby demo on one machine: a cloud (chloedb-server, with a city's
# places and their slice) and a phone (chloedb-nearby) that searches on the
# device. Run from the repository root after
#   (cd chloedb-server && cargo build --release --bin chloedb-server --bin chloedb-nearby)
# then open http://127.0.0.1:7740/.
#
#   CITY=nyc DIR=~/nearby chloedb-server/demo/nearby/run.sh
set -eu
CITY=${CITY:-nyc}
DIR=${DIR:-$HOME/nearby}
BIN=${BIN:-chloedb-server/target/release}
HERE=chloedb-server/demo/nearby
mkdir -p "$DIR"
case $CITY in
  nyc) LAT0=40.7 LON0=-73.98 ;;
  houston) LAT0=29.76 LON0=-95.37 ;;
  *) echo "CITY is nyc or houston" >&2; exit 1 ;;
esac
DB="$DIR/$CITY.strata"
if [ ! -f "$DB" ]; then
  [ -f "$DIR/$CITY.jsonl" ] || python3 "$HERE/places.py" --city "$CITY" --out "$DIR/$CITY.jsonl"
  "$BIN/chloedb-server" import --db "$DB" --schema "$HERE/schema.json" "$DIR/$CITY.jsonl"
fi
"$BIN/chloedb-server" --db "$DB" --slices "$DIR/slices-$CITY" --listen 127.0.0.1:7700 &
CLOUD=$!
trap 'kill $CLOUD 2>/dev/null' EXIT INT TERM
until curl -sf http://127.0.0.1:7700/readyz >/dev/null; do sleep 0.5; done
curl -sf -X POST http://127.0.0.1:7700/v1/admin/slices -H 'content-type: application/json' \
  -d "{\"table\": \"places\", \"text\": \"body\", \"lat\": \"lat\", \"lon\": \"lon\", \"lat0\": $LAT0, \"lon0\": $LON0}"
echo
"$BIN/chloedb-nearby" --cloud http://127.0.0.1:7700 --table places --listen 127.0.0.1:7740 --lat "$LAT0" --lon "$LON0"
