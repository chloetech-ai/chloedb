#!/bin/bash
# chloedb-route-bench with each copy of a shard in a container of its own,
# on cores of its own (--cpuset-cpus), so that a copy asked more (or a
# hedge) takes capacity the other copies do not share, as on separate
# machines. The coordinator, the clients and the delay lines run in one
# more container on two cores of their own, on the same Docker network.
# One shard of three copies (eight cores hold no more whole ones), one
# scenario at a time, on fresh copies.
#
# Cores, not a CPU quota (--cpus): a server's threads spend a quota early
# in each 100 ms period and wait out the rest, which in a first run at
# 0.6 CPU each put p99 at 70-90 ms on its own. The servers
# serve open (--no-token): the network is Docker's own, no port published.
#
#   run.sh                 # equal stall busy ryw
#   SCENARIOS="stall" run.sh
#
# Needs: a Docker daemon (colima here: `colima start --cpu 8 --memory 16
# --vm-type vz --mount-type virtiofs --mount $W:w`), the image
# (`chloedb-route:dev`, chloedb-server and chloedb-route-bench built from
# this tree), and chloedb-route-bench built on this machine, which builds
# the databases (`--build-only`).
set -euo pipefail
IMG=${IMG:-chloedb-route:dev}
W=${W:-$HOME/chloedb-route-containers}
# Cores of each copy, and of the bench.
COPY_SETS=(${COPY_SETS:-2-3 4-5 6-7})
BENCH_SET=${BENCH_SET:-0-1}
ROWS=${ROWS:-1000000}
ROUNDS=${ROUNDS:-2}
SECS=${SECS:-10}
# Fresh copies start cold (their files are read through the VM's mount).
WARMUP=${WARMUP:-6}
CONCURRENCY=${CONCURRENCY:-1,16,64}
SCENARIOS=${SCENARIOS:-equal stall busy ryw}
OUT=${OUT:-$W/out}
HOST_BENCH=${HOST_BENCH:-$(dirname "$0")/../../target/release/chloedb-route-bench}
SETUP=${SETUP:-"Docker (colima, vz, 8 vCPUs, 16 GB) on an Apple M1 Pro, 10 cores, 32 GB, macOS 27.0.1: one shard of $ROWS rows, each of its 3 copies a container on 2 vCPUs of its own (cpusets ${COPY_SETS[*]}), the coordinator, clients and delay lines a container on vCPUs $BENCH_SET, one Docker network"}

DATA=$W/data-1shard-$ROWS
[ -f "$DATA/shard0/c0/db.strata" ] || "$HOST_BENCH" --rows "$ROWS" --shards 1 --build-only "$DATA"
docker network inspect routenet >/dev/null 2>&1 || docker network create routenet >/dev/null
mkdir -p "$OUT"

started=()
stop_all() { [ ${#started[@]} -eq 0 ] || docker rm -f "${started[@]}" >/dev/null 2>&1 || true; started=(); }
trap stop_all EXIT

# serve NAME CORES SLOTS [server args...]: a fresh copy of the shard.
serve() {
    local name=$1 cores=$2 slots=$3; shift 3
    local dir="$W/run/$name"
    rm -rf "$dir" && mkdir -p "$dir" && cp -R "$DATA/shard0/c0/." "$dir/"
    docker rm -f "$name" >/dev/null 2>&1 || true
    docker run -d --name "$name" --network routenet --cpuset-cpus "$cores" -v "$dir:/data" "$IMG" \
        chloedb-server --db /data/db.strata --listen 0.0.0.0:7710 --grpc 0.0.0.0:7711 \
        --max-concurrent "$slots" --timeout-ms 30000 --no-token "$@" >/dev/null
    started+=("$name")
}

# Waits for each server to answer; one that exited fails the run.
up() {
    for n in "$@"; do
        until docker exec "$n" curl -sf http://localhost:7710/v1/health >/dev/null 2>&1; do
            if [ "$(docker inspect -f '{{.State.Running}}' "$n")" != true ]; then
                docker logs "$n" 2>&1 | tail -5 >&2
                echo "$n exited" >&2
                exit 1
            fi
            sleep 0.5
        done
    done
}

bench() {
    local sc=$1; shift
    docker run --rm --name bench --network routenet --cpuset-cpus "$BENCH_SET" -e CHLOEDB_BENCH_SETUP="$SETUP" -v "$OUT:/out" "$IMG" \
        chloedb-route-bench --rows "$ROWS" --shards 1 --rounds "$ROUNDS" --seconds "$SECS" --warmup "$WARMUP" --concurrency "$CONCURRENCY" \
        --scenarios "$sc" --out "/out/$sc" "$@" 2>&1 | grep -v '^|\|^$\|^#\|^Machine' | tee "$OUT/$sc.log"
}

for sc in $SCENARIOS; do
    if [ "$sc" = ryw ]; then
        # Shard 0's copies: two replicas and a primary shipping to them,
        # replica 1 through the bench's late link.
        serve rep0 "${COPY_SETS[0]}" 64 --seq-no --replica
        serve rep1 "${COPY_SETS[1]}" 64 --seq-no --replica
        serve pri "${COPY_SETS[2]}" 64 --seq-no --replicas http://rep0:7711,http://bench:7799 --write-acks primary
        up rep0 rep1 pri
        bench ryw --external "http://rep0:7711|http://rep1:7711|http://pri:7711" --late-link 0.0.0.0:7799=http://rep1:7711
    else
        copies=()
        for k in 0 1 2; do
            slots=64
            [ "$sc" = busy ] && [ $k = 0 ] && slots=1
            serve "copy$k" "${COPY_SETS[$k]}" "$slots"
            copies+=("http://copy$k:7711")
        done
        up "${started[@]}"
        bench "$sc" --external "$(IFS='|'; echo "${copies[*]}")"
    fi
    stop_all
done
