#!/bin/bash
# Pull the branch, run each job of jobs.txt not run yet, publish logs and results in /root/out.
set -u
REPO=/home/user/chloedb; OUT=/root/out; export S=/root/s
cd $REPO/research/llm-weights
while true; do
  git -C $REPO pull -q --ff-only origin "$(git -C $REPO rev-parse --abbrev-ref HEAD)" >/dev/null 2>&1
  ran=0
  while IFS='|' read -r name envs script; do
    [ -z "$name" ] || [ "${name:0:1}" = "#" ] && continue
    [ -e $OUT/$name.done ] || [ -e $OUT/$name.failed ] && continue
    echo "running $name since $(date -u +%FT%TZ) at commit $(git -C $REPO rev-parse --short HEAD)" > $OUT/status.txt
    nvidia-smi --query-gpu=name,memory.used,memory.total,utilization.gpu --format=csv >> $OUT/status.txt 2>&1
    start=$(date +%s)
    if env $envs /root/venv/bin/python -u $script $OUT/$name.json > $OUT/$name.log 2>&1; then
      echo "$(( $(date +%s) - start )) s" > $OUT/$name.done
    else
      echo "$(( $(date +%s) - start )) s, exit $?" > $OUT/$name.failed
    fi
    ran=1; break
  done < gpu/jobs.txt
  if [ $ran = 0 ]; then
    { echo "idle since $(date -u +%FT%TZ) at commit $(git -C $REPO rev-parse --short HEAD)"; nvidia-smi --query-gpu=name,memory.used,utilization.gpu --format=csv; } > $OUT/status.txt 2>&1
    sleep 60
  fi
done
