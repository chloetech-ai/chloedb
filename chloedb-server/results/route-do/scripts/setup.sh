#!/bin/bash
# setup.sh: pods on the route-bench nodes, tools, binaries and data.
source "$(dirname "$0")/common.sh"
set -e
umask 077
log "nodes:"; $K get nodes -o wide | tee -a $LOG
bench_node=$($K get nodes -l doks.digitalocean.com/node-pool=bench -o jsonpath='{.items[0].metadata.name}')
copy_nodes=(${NODES:-$($K get nodes -l doks.digitalocean.com/node-pool=copies -o jsonpath='{.items[*].metadata.name}' | tr ' ' '\n' | sort)})
[ ${#copy_nodes[@]} -eq 9 ] || { log "expected 9 copy nodes, got ${#copy_nodes[@]}"; exit 1; }
sed "s/NAME/bench/; s/NODE/$bench_node/" $S/pod.tmpl | $K apply -f -
for i in "${!COPIES[@]}"; do sed "s/NAME/${COPIES[$i]}/; s/NODE/${copy_nodes[$i]}/" $S/pod.tmpl | $K apply -f -; done
$K wait --for=condition=Ready pod -l app=route-bench --timeout=300s
log "pods ready"; $K get pods -o wide | tee -a $LOG
ALL=(bench "${COPIES[@]}")
par 'export DEBIAN_FRONTEND=noninteractive; apt-get update -qq && apt-get install -y -qq --no-install-recommends iproute2 stress-ng procps curl ca-certificates python3-minimal > /dev/null && mkdir -p /work/bin /work/data /work/out' "${ALL[@]}"
log "tools installed"
for p in "${ALL[@]}"; do
  $K cp $S/pod-start.sh $p:/work/start.sh; $K cp $S/pod-calm.sh $p:/work/calm.sh
  $K cp $S/stall-loop.pl $p:/work/stall-loop.pl; $K cp $S/cpu-sampler.sh $p:/work/cpu-sampler.sh
done
par 'chmod +x /work/*.sh; setsid nohup /work/cpu-sampler.sh > /dev/null 2>&1 < /dev/null & grep -m1 "model name" /proc/cpuinfo; nproc' "${ALL[@]}"
for p in "${ALL[@]}"; do echo "$p $(cat $S/par-$p.log | tr '\n' ' ')"; done | tee $S/cpus.txt | tee -a $LOG
# dist/: chloedb-server and chloedb-route-bench built for Linux x86-64 from this tree.
log "copying binaries to bench"
$K cp $S/dist/chloedb-server bench:/work/bin/chloedb-server
$K cp $S/dist/chloedb-route-bench bench:/work/bin/chloedb-route-bench
ex bench 'chmod +x /work/bin/*; cd /work && /work/bin/chloedb-route-bench --rows 3000000 --shards 3 --build-only /work/d3 2>&1 | tail -2 && /work/bin/chloedb-route-bench --rows 1000000 --shards 1 --build-only /work/d1 2>&1 | tail -2
  mkdir -p /work/share && cp /work/bin/chloedb-server /work/share/ && for i in 0 1 2; do tar -C /work/d3/shard$i/c0 -cf /work/share/d3-$i.tar .; done && tar -C /work/d1/shard0/c0 -cf /work/share/d1.tar . && ls -la /work/share && du -sh /work/d3 /work/d1
  cd /work/share && setsid nohup python3 -m http.server 8000 > /work/http.log 2>&1 < /dev/null &' | tee -a $LOG
BIP=$($K get pod bench -o jsonpath='{.status.podIP}')
sleep 2
par "set -e; curl -sSf -o /work/bin/chloedb-server http://$BIP:8000/chloedb-server; chmod +x /work/bin/chloedb-server
  s=\$(echo {} | cut -c2); mkdir -p /work/data/d3 /work/data/d1; curl -sSf http://$BIP:8000/d3-\$s.tar | tar -C /work/data/d3 -xf -; curl -sSf http://$BIP:8000/d1.tar | tar -C /work/data/d1 -xf -; ls /work/data/d3 | head -3; test -x /work/bin/chloedb-server" "${COPIES[@]}"
log "setup done"
