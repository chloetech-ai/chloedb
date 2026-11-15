#!/bin/bash
# drive.sh [SCENARIO...]: the route bench's scenarios on the route-bench cluster, each on
# fresh copies; results fetched to $S/out/<name> after each. ROUNDS, SECS, WARMUP and CONC
# set the run's shape. cpuheavy1-K, ryw-fix and rywnet-fix need the experimental bench build
# (main plus ../experiment-cool-off-fix.patch) at /work/bin/chloedb-route-bench-fix on the bench pod.
source "$(dirname "$0")/common.sh"
set -o pipefail
ROUNDS=${ROUNDS:-5}; SECS=${SECS:-10}; WARMUP=${WARMUP:-6}; CONC=${CONC:-1,16,64}
declare -A IP
for p in "${COPIES[@]}" $BENCH; do IP[$p]=$($K get pod $p -o jsonpath='{.status.podIP}'); done
U() { echo "http://${IP[$1]}:7711"; }
URLS3="$(U c00)|$(U c01)|$(U c02),$(U c10)|$(U c11)|$(U c12),$(U c20)|$(U c21)|$(U c22)"
BASE="DigitalOcean nyc1, DOKS 1.37.2: 3 shards of 1M rows, each copy a pod on a c-2 node of its own (2 dedicated vCPUs), the coordinator, clients and delay lines one pod on a c-32 node (32 dedicated vCPUs), pod network; every server with --answer-cache-mb 0 (no answer cache)"
BASE1="DigitalOcean nyc1, DOKS 1.37.2: one shard of 1M rows, two replicas and a primary each a pod on a c-2 node of its own (2 dedicated vCPUs), the coordinator, clients and delay lines one pod on a c-32 node (32 dedicated vCPUs), pod network; every server with --answer-cache-mb 0 (no answer cache)"
BASE1C="DigitalOcean nyc1, DOKS 1.37.2: one shard of 1M rows, each of its 3 copies a pod on a c-2 node of its own (2 dedicated vCPUs), the coordinator, clients and delay lines one pod on a c-32 node (32 dedicated vCPUs), pod network; every server with --answer-cache-mb 0 (no answer cache)"
URLS1="$(U c00)|$(U c01)|$(U c02)"
# start1 SLOTS0: fresh copies of the one shard on c00 c01 c02, copy 0 with SLOTS0 query slots.
start1() { par "s=64; [ {} = c00 ] && s=$1; /work/start.sh d1 \$s" c00 c01 c02; }
ALLC="round-robin,round-robin adaptive,least-cost,least-cost adaptive,least-cost no-load"

stop_all() { par 'pkill -x chloedb-server; /work/calm.sh; sleep 1; true' "${COPIES[@]}"; }
# start3 SLOTS0: fresh copies of the 3 shards, copy 0 of shard 0 with SLOTS0 query slots.
start3() { par "s=64; [ {} = c00 ] && s=$1; /work/start.sh d3 \$s" "${COPIES[@]}"; }
# bench NAME SETUP ARGS...: one run of the bench, waited for, fetched.
bench() {
  local name=$1 setup=$2; shift 2
  local args=""; for a in "$@"; do args+=" '$a'"; done
  log "bench $name start:$args"
  ex $BENCH "mkdir -p /work/out; cd /work; CHLOEDB_BENCH_SETUP='$setup' setsid nohup ${BIN:-/work/bin/chloedb-route-bench} --rounds $ROUNDS --seconds $SECS --warmup $WARMUP --concurrency $CONC --out /work/out/$name $args > /work/out/$name.log 2>&1 < /dev/null & echo \$! > /work/out/$name.pid" || return 1
  sleep 5
  # Alive and not a zombie (the pod's PID 1, sleep, reaps nothing).
  while ex $BENCH "grep -qP '^State:\s*[RSD]' /proc/\$(cat /work/out/$name.pid)/status 2>/dev/null"; do
    sleep 30
    ex $BENCH "grep -c '^[a-z]* | ' /work/out/$name.log" > $S/progress-$name 2>/dev/null
  done
  mkdir -p $S/out
  kcp $BENCH:/work/out/$name $S/out/$name; kcp $BENCH:/work/out/$name.log $S/out/$name.log
  if [ -f $S/out/$name/route.json ]; then log "bench $name done ($(grep -c '^' $S/out/$name/route.md) lines)"; else log "bench $name FAILED: $(tail -5 $S/out/$name.log)"; return 1; fi
}

run() {
  local sc=$1
  stop_all
  case $sc in
    equal) start3 64 && bench equal "$BASE; copies alike" --rows 3000000 --shards 3 --scenarios equal --external "$URLS3" ;;
    slow) start3 64 && bench slow "$BASE; copy 0 of shard 0 behind the delay line of the bench, 30 ms per answer" --rows 3000000 --shards 3 --scenarios slow --external "$URLS3" ;;
    cpubusy) start3 64 && ex c00 'setsid nohup stress-ng --cpu 2 > /dev/null 2>&1 < /dev/null &' && \
      bench cpubusy "$BASE; the node of copy 0 of shard 0 also runs stress-ng --cpu 2 (two CPU-bound threads at the priority of the server)" --rows 3000000 --shards 3 --scenarios equal --external "$URLS3" --configs "$ALLC" ;;
    cpuhalf) start3 64 && ex c00 'setsid nohup stress-ng --cpu 1 > /dev/null 2>&1 < /dev/null &' && \
      bench cpuhalf "$BASE; the node of copy 0 of shard 0 also runs stress-ng --cpu 1 (one CPU-bound thread at the priority of the server)" --rows 3000000 --shards 3 --scenarios equal --external "$URLS3" --configs "round-robin,least-cost,least-cost no-load" ;;
    busy) start3 1 && bench busy "$BASE; copy 0 of shard 0 serves with --max-concurrent 1, the others 64" --rows 3000000 --shards 3 --scenarios busy --external "$URLS3" ;;
    stall) start3 64 && bench stall "$BASE; copy 0 of shard 0 behind the delay line of the bench, which each second holds what it answers in 50 ms for 60 ms" --rows 3000000 --shards 3 --scenarios stall --external "$URLS3" ;;
    sigstop) start3 64 && ex c00 'setsid nohup perl /work/stall-loop.pl $(cat /work/server.pid) 60 1000 > /dev/null 2>&1 < /dev/null &' && \
      bench sigstop "$BASE; the server process of copy 0 of shard 0 frozen (SIGSTOP) 60 ms of every second" --rows 3000000 --shards 3 --scenarios equal --external "$URLS3" ;;
    ryw) par "/work/start.sh d1 64 --seq-no --replica" c00 c01 && \
      ex c02 "/work/start.sh d1 64 --seq-no --replicas $(U c00),http://${IP[$BENCH]}:7799 --write-acks primary" && \
      bench ryw "$BASE1; replica 1 receives what the primary ships through the late link of the bench, 50 ms each way" --rows 1000000 --shards 1 --scenarios ryw --external "$(U c00)|$(U c01)|$(U c02)" --late-link "0.0.0.0:7799=$(U c01)" ;;
    rywnet) par "/work/start.sh d1 64 --seq-no --replica" c00 c01 && \
      ex c02 "/work/start.sh d1 64 --seq-no --replicas $(U c00),$(U c01) --write-acks primary && tc qdisc add dev eth0 root handle 1: prio && tc qdisc add dev eth0 parent 1:3 handle 30: netem delay 50ms && tc filter add dev eth0 protocol ip parent 1:0 prio 1 u32 match ip dst ${IP[c01]}/32 flowid 1:3 && tc -s qdisc show dev eth0 | head -4 && curl -s -o /dev/null -w 'to replica 1: connect %{time_connect} s\\n' http://${IP[c01]}:7710/v1/health && curl -s -o /dev/null -w 'to replica 0: connect %{time_connect} s\\n' http://${IP[c00]}:7710/v1/health" | tee -a $LOG && \
      bench rywnet "$BASE1; packets from the primary to replica 1 delayed 50 ms by the kernel (tc netem on the interface of the primary), the link direct" --rows 1000000 --shards 1 --scenarios ryw --external "$(U c00)|$(U c01)|$(U c02)" ;;
    # One shard of 1M rows, its 3 copies on c00 c01 c02 (as results/route-containers): each query one shard call, so the copies, not the coordinator, run out of CPU first.
    equal1) start1 64 && bench equal1 "$BASE1C; copies alike" --rows 1000000 --shards 1 --scenarios equal --external "$URLS1" ;;
    busy1) start1 1 && bench busy1 "$BASE1C; copy 0 serves with --max-concurrent 1, the others 64" --rows 1000000 --shards 1 --scenarios busy --external "$URLS1" ;;
    cpuhalf1) start1 64 && ex c00 'setsid nohup stress-ng --cpu 1 > /dev/null 2>&1 < /dev/null &' && \
      bench cpuhalf1 "$BASE1C; the node of copy 0 also runs stress-ng --cpu 1 (one CPU-bound thread at the priority of the server)" --rows 1000000 --shards 1 --scenarios equal --external "$URLS1" --configs "round-robin,least-cost,least-cost no-load" ;;
    cpubusy1) start1 64 && ex c00 'setsid nohup stress-ng --cpu 2 > /dev/null 2>&1 < /dev/null &' && \
      bench cpubusy1 "$BASE1C; the node of copy 0 also runs stress-ng --cpu 2 (two CPU-bound threads at the priority of the server)" --rows 1000000 --shards 1 --scenarios equal --external "$URLS1" --configs "$ALLC" ;;
    stall1) start1 64 && bench stall1 "$BASE1C; copy 0 behind the delay line of the bench, which each second holds what it answers in 50 ms for 60 ms" --rows 1000000 --shards 1 --scenarios stall --external "$URLS1" ;;
    sigstop1) start1 64 && ex c00 'setsid nohup perl /work/stall-loop.pl $(cat /work/server.pid) 60 1000 > /dev/null 2>&1 < /dev/null &' && \
      bench sigstop1 "$BASE1C; the server process of copy 0 frozen (SIGSTOP) 60 ms of every second" --rows 1000000 --shards 1 --scenarios equal --external "$URLS1" ;;
    ryw-fix) par "/work/start.sh d1 64 --seq-no --replica" c00 c01 && \
      ex c02 "/work/start.sh d1 64 --seq-no --replicas $(U c00),http://${IP[$BENCH]}:7799 --write-acks primary" && \
      BIN=/work/bin/chloedb-route-bench-fix bench ryw-fix "$BASE1; replica 1 receives what the primary ships through the late link of the bench, 50 ms each way; least-cost fixed is an experimental build that spares a copy outrun before --hedge-ms and chooses by cost when every copy is in its cool-off" --rows 1000000 --shards 1 --scenarios ryw --external "$(U c00)|$(U c01)|$(U c02)" --late-link "0.0.0.0:7799=$(U c01)" --configs "round-robin,least-cost,least-cost fixed,least-cost prefer" ;;
    equal1-mc*) n=${sc#equal1-mc}; par "/work/start.sh d1 $n" c00 c01 c02 && \
      bench equal1-mc$n "$BASE1C; copies alike, every copy with --max-concurrent $n (asked by the simulation thread)" --rows 1000000 --shards 1 --scenarios equal --external "$URLS1" --configs "round-robin,least-cost" ;;
    cpubusy1-mc*) n=${sc#cpubusy1-mc}; par "/work/start.sh d1 $n" c00 c01 c02 && ex c00 'setsid nohup stress-ng --cpu 2 > /dev/null 2>&1 < /dev/null &' && \
      bench cpubusy1-mc$n "$BASE1C; every copy with --max-concurrent $n, and the node of copy 0 also runs stress-ng --cpu 2 (two CPU-bound threads at the priority of the server) (asked by the simulation thread)" --rows 1000000 --shards 1 --scenarios equal --external "$URLS1" --configs "round-robin,least-cost" ;;
    rywnet-fix) par "/work/start.sh d1 64 --seq-no --replica" c00 c01 && \
      ex c02 "/work/start.sh d1 64 --seq-no --replicas $(U c00),$(U c01) --write-acks primary && tc qdisc add dev eth0 root handle 1: prio && tc qdisc add dev eth0 parent 1:3 handle 30: netem delay 50ms && tc filter add dev eth0 protocol ip parent 1:0 prio 1 u32 match ip dst ${IP[c01]}/32 flowid 1:3 && tc -s qdisc show dev eth0 | head -4 && curl -s -o /dev/null -w 'to replica 1: connect %{time_connect} s\\n' http://${IP[c01]}:7710/v1/health && curl -s -o /dev/null -w 'to replica 0: connect %{time_connect} s\\n' http://${IP[c00]}:7710/v1/health" | tee -a $LOG && \
      BIN=/work/bin/chloedb-route-bench-fix bench rywnet-fix "$BASE1; packets from the primary to replica 1 delayed 50 ms by the kernel (tc netem on the interface of the primary), the link direct; least-cost fixed is an experimental build that spares a copy outrun before --hedge-ms and chooses by cost when every copy is in its cool-off" --rows 1000000 --shards 1 --scenarios ryw --external "$(U c00)|$(U c01)|$(U c02)" --configs "round-robin,least-cost,least-cost fixed,least-cost prefer" ;;
    cpuheavy1-*) k=${sc#cpuheavy1-}; start1 64 && ex c00 "setsid nohup stress-ng --cpu $k > /dev/null 2>&1 < /dev/null &" && \
      BIN=/work/bin/chloedb-route-bench-fix bench cpuheavy1-$k "$BASE1C; the node of copy 0 also runs stress-ng --cpu $k ($k CPU-bound threads at the priority of the server); least-cost fixed is an experimental build that spares a copy outrun before --hedge-ms and chooses by cost when every copy is in its cool-off" --rows 1000000 --shards 1 --scenarios equal --external "$URLS1" --configs "round-robin,least-cost,least-cost fixed" ;;
    *) log "unknown scenario $sc"; return 1 ;;
  esac
  local rc=$?
  stop_all
  return $rc
}

log "drive start: $* (rounds $ROUNDS, $SECS s, warmup $WARMUP, clients $CONC)"
for sc in "$@"; do run $sc || log "scenario $sc failed"; done
log "drive end"
