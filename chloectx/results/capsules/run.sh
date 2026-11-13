#!/bin/bash
# run.sh SCENARIO REPEATS RESULTS_DIR [extra args...]
sc=$1; rep=$2; res=$3; shift 3
cd /home/user/chloedb/chloectx
exec python3 scripts/memory_bench.py run --scenario "$sc" --lanes engram full \
  --lane-bin engram=/home/user/bench/bin/chloectx-capsule \
  --variant engram+none=BENCH_HANDOFF=none \
  --variant engram+summary=BENCH_HANDOFF=summary \
  --variant engram+capsule=BENCH_HANDOFF=capsule \
  --variant engram+both=BENCH_HANDOFF=summary+capsule \
  --repeats "$rep" --work /home/user/bench/work --shared /home/user/bench/shared \
  --engram-models /home/user/data/models --results "$res" "$@"
