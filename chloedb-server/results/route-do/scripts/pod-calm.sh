#!/bin/bash
# calm.sh: no slowness left on this pod (stress, stalls, netem).
pkill -x stress-ng 2>/dev/null
pkill -f stall-loop 2>/dev/null
tc qdisc del dev eth0 root 2>/dev/null
[ -f /work/server.pid ] && kill -CONT $(cat /work/server.pid) 2>/dev/null
true
