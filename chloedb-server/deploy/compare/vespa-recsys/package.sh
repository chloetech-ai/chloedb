#!/usr/bin/env bash
# Writes Vespa's application package for engine-compare's recommender kinds
# (recsys.rs) as a zip, with the serving data's ranker as models/ranker.json:
#   [DIM=64] package.sh OUT.zip DATA_DIR
# Deploy it to the config server, then run recsys-load:
#   curl -sf -H Content-Type:application/zip --data-binary @OUT.zip \
#     http://localhost:19071/application/v2/tenant/default/prepareandactivate
set -euo pipefail
out=$(cd "$(dirname "$1")" && pwd)/$(basename "$1") data=$2
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cp -r "$here/schemas" "$here/search" "$here/services.xml" "$work/"
sed "s/x\[64\]/x[${DIM:-64}]/g" "$here/schemas/item.sd" >"$work/schemas/item.sd"
mkdir -p "$work/models"
cp "$data/ranker/gbdt.json" "$work/models/ranker.json"
rm -f "$out"
(cd "$work" && python3 -m zipfile -c "$out" services.xml schemas search models)
