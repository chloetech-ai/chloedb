#!/usr/bin/env bash
# The recommender kinds (engine-compare recsys-load and recsys-run, see
# src/bin/engine-compare/recsys.rs) on one machine's Docker, one engine at a
# time, each capped at CPUS and MEM:
#
#   DATA=~/data/recsys-100k OUT=~/data/recsys-out local.sh strata es os vespa qdrant milvus redis postgres
#
# DATA is serving_data.py's output (strata/results/recsys). Reports go to
# OUT as recsys-load-ENGINE.json and recsys-run-ENGINE[-LABEL].json.
# Images come through mirror.gcr.io where Docker Hub limits pulls. Strata
# runs from target/ci (build it first: cargo build --profile ci --bins in
# strata-server), on the first CPUS cores (CPUS whole), with no memory cap;
# its database is merged after the load (recsys-optimize), as the others'.
# Extra arguments for recsys-run go in RUN_ARGS.
# Elasticsearch's and OpenSearch's disk watermarks are off: a container's
# disk allowance can look 95% full to them while it has room. Elasticsearch
# runs on its trial license, which its rrf retriever needs (as compare.sh).
set -euo pipefail
DATA=${DATA:?serving data directory} OUT=${OUT:?report directory}
CPUS=${CPUS:-3} MEM=${MEM:-6g} RUN_ARGS=${RUN_ARGS:-}
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../../../.." && pwd)
BIN=${BIN:-$ROOT/strata-server/target/ci}
EC="$BIN/engine-compare"
mkdir -p "$OUT"

wait_for() { # URL [grep pattern]
    for _ in $(seq 1 300); do
        if out=$(curl -sf "$1" 2>/dev/null) && { [ -z "${2:-}" ] || grep -q "$2" <<<"$out"; }; then return 0; fi
        sleep 2
    done
    echo "timed out waiting for $1" >&2
    return 1
}

serve_strata() {
    # The containers' CPU cap, as the cores Strata may run on.
    taskset -c "0-$(( ${CPUS%.*} - 1 ))" "$BIN/strata-server" --db "$OUT/strata.db" --schema "$HERE/strata-items.json" --models "$DATA" --listen 127.0.0.1:7700 --grpc off >>"$OUT/strata-server.log" 2>&1 &
    echo $! >"$OUT/strata.pid"
    wait_for http://127.0.0.1:7700/v1/health
}

up() {
    local e=$1 lim=(--cpus "$CPUS" --memory "$MEM")
    case $e in
    strata)
        rm -rf "$OUT/strata.db"
        serve_strata ;;
    es)
        docker run -d --name rs-es "${lim[@]}" -p 9200:9200 -e discovery.type=single-node -e xpack.security.enabled=false \
            -e cluster.routing.allocation.disk.threshold_enabled=false -e indices.merge.disk.check_interval=0s -e xpack.license.self_generated.type=trial \
            -e ES_JAVA_OPTS="-Xms3g -Xmx3g" mirror.gcr.io/library/elasticsearch:9.5.5 >/dev/null
        wait_for http://localhost:9200/_cluster/health '"status"' ;;
    os)
        docker run -d --name rs-os "${lim[@]}" -p 9200:9200 -e discovery.type=single-node -e DISABLE_SECURITY_PLUGIN=true \
            -e cluster.routing.allocation.disk.threshold_enabled=false \
            -e DISABLE_INSTALL_DEMO_CONFIG=true -e OPENSEARCH_JAVA_OPTS="-Xms3g -Xmx3g" mirror.gcr.io/opensearchproject/opensearch:3.9.0 >/dev/null
        wait_for http://localhost:9200/_cluster/health '"status"' ;;
    vespa)
        docker run -d --name rs-vespa "${lim[@]}" -p 8080:8080 -p 19071:19071 mirror.gcr.io/vespaengine/vespa:8.778.45 >/dev/null
        wait_for http://localhost:19071/state/v1/health '"up"'
        "$ROOT/strata-server/deploy/compare/vespa-recsys/package.sh" "$OUT/vespa-recsys.zip" "$DATA"
        curl -sf -H Content-Type:application/zip --data-binary @"$OUT/vespa-recsys.zip" \
            http://localhost:19071/application/v2/tenant/default/prepareandactivate >/dev/null
        wait_for http://localhost:8080/state/v1/health '"up"' ;;
    qdrant)
        docker run -d --name rs-qdrant "${lim[@]}" -p 6333:6333 mirror.gcr.io/qdrant/qdrant:v1.20.0 >/dev/null
        wait_for http://localhost:6333/readyz ;;
    milvus)
        printf 'listen-client-urls: http://0.0.0.0:2379\nadvertise-client-urls: http://0.0.0.0:2379\nquota-backend-bytes: 4294967296\nauto-compaction-mode: revision\nauto-compaction-retention: "1000"\n' >"$OUT/embedEtcd.yaml"
        docker run -d --name rs-milvus "${lim[@]}" -p 19530:19530 -p 9091:9091 -e ETCD_USE_EMBED=true -e ETCD_DATA_DIR=/var/lib/milvus/etcd \
            -e ETCD_CONFIG_PATH=/milvus/configs/embedEtcd.yaml -e COMMON_STORAGETYPE=local -e DEPLOY_MODE=STANDALONE \
            -v "$OUT/embedEtcd.yaml:/milvus/configs/embedEtcd.yaml" milvusdb/milvus:v2.6.26 milvus run standalone >/dev/null
        wait_for http://localhost:9091/healthz ;;
    redis)
        docker run -d --name rs-redis "${lim[@]}" -p 6379:6379 redis:8.8.4 redis-server --save "" --appendonly no >/dev/null
        until docker exec rs-redis redis-cli ping 2>/dev/null | grep -q PONG; do sleep 1; done ;;
    postgres)
        docker run -d --name rs-postgres "${lim[@]}" --shm-size 4g -p 5432:5432 -e POSTGRES_HOST_AUTH_METHOD=trust \
            mirror.gcr.io/paradedb/paradedb:0.26.4-pg17 postgres -c shared_buffers=1536MB -c effective_cache_size=4GB -c maintenance_work_mem=2GB -c work_mem=64MB >/dev/null
        until docker exec rs-postgres pg_isready -U postgres >/dev/null 2>&1; do sleep 1; done
        sleep 3 ;;
    *) echo "unknown engine $e" >&2; return 1 ;;
    esac
}

down() {
    case $1 in
    strata)
        if [ -f "$OUT/strata.pid" ]; then
            pid=$(cat "$OUT/strata.pid")
            kill "$pid" 2>/dev/null || true
            while kill -0 "$pid" 2>/dev/null; do sleep 0.2; done
            rm -f "$OUT/strata.pid"
        fi ;;
    # With its volumes, which would otherwise fill the disk run by run.
    *) docker rm -f -v "rs-$1" >/dev/null 2>&1 || true ;;
    esac
}

url() {
    case $1 in
    strata) echo http://127.0.0.1:7700 ;;
    es | os) echo http://localhost:9200 ;;
    vespa) echo http://localhost:8080 ;;
    qdrant) echo http://localhost:6333 ;;
    milvus) echo http://localhost:19530 ;;
    redis) echo redis://localhost:6379 ;;
    postgres) echo postgres://localhost:5432 ;;
    esac
}

for e in "$@"; do
    echo "== $e"
    down "$e"
    up "$e"
    "$EC" recsys-load --engine "$e" --url "$(url "$e")" --data "$DATA" --out "$OUT"
    if [ "$e" = strata ]; then
        # Merged as the others are force-merged; the server has no optimize.
        down strata
        "$EC" recsys-optimize --db "$OUT/strata.db" --report "$OUT/recsys-load-strata.json"
        serve_strata
    fi
    # shellcheck disable=SC2086
    "$EC" recsys-run --engine "$e" --url "$(url "$e")" --data "$DATA" --out "$OUT" $RUN_ARGS
    down "$e"
done
