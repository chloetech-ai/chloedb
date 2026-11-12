#!/bin/bash
# Waits for the main run of SCENARIO to end, then runs the short-note lanes on the same history.
sc=$1
while pgrep -f "memory_bench.py run --scenario $sc " >/dev/null; do sleep 20; done
cd /home/user/chloedb/chloectx
exec python3 scripts/memory_bench.py run --scenario "$sc" --lanes engram \
  --lane-bin engram=/home/user/bench/bin/chloectx-capsule \
  --variant engram+short=BENCH_HANDOFF=short \
  --variant engram+shortcap=BENCH_HANDOFF=short+capsule \
  --repeats 4 --work /home/user/bench/work --shared /home/user/bench/shared \
  --engram-models /home/user/data/models --results /home/user/bench/results/$sc
