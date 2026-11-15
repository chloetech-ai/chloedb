#!/bin/bash
# The node's CPU counters every 2 s (/proc/stat is the host's).
while :; do echo "$(date +%s.%N) $(head -1 /proc/stat)"; sleep 2; done >> /work/cpu.log
