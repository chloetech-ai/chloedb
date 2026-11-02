#!/usr/bin/env bash
# Part A of the laptop benchmark on DigitalOcean: one dedicated-CPU node in a
# DOKS cluster (nyc3), driven from here over the Kubernetes API (kubectl exec
# and cp), so nothing needs SSH. Everything it creates is tagged
# strata-bench, and a pod in the cluster deletes the cluster (with its
# volumes and load balancers) after HOURS even if this script never runs
# `down`.
#
#   do_part_a.sh up [HOURS]   # (token in DIGITALOCEAN_TOKEN or digitalocean)   # cluster + teardown timer + bench pod (default 14 h)
#   do_part_a.sh push        # this checkout (git archive of HEAD) into the pod
#   do_part_a.sh setup       # Rust, the Python libraries, strata-py built in release
#   do_part_a.sh prep        # Shop (Amazon Reviews 2023 Video Games) and the BEIR collections, embedded there
#   do_part_a.sh run         # A1, A2, writes, A3 in the background; logs in /work/logs
#   do_part_a.sh status      # the last log lines and the node's load
#   do_part_a.sh fetch DIR   # the reports and logs into DIR
#   do_part_a.sh down        # delete the cluster and check nothing tagged strata-bench is left
#
# NODE_SIZE (default g-8vcpu-32gb-intel, $0.4494 an hour at list price: nyc3
# has no plain g-8vcpu-32gb) is the droplet the benchmark runs on; the
# reports name its CPU.
set -euo pipefail

API=https://api.digitalocean.com/v2
# The token: DIGITALOCEAN_TOKEN, or the environment's variable named digitalocean.
DIGITALOCEAN_TOKEN=${DIGITALOCEAN_TOKEN:-${digitalocean:-${DIGITALOCEAN:-}}}
NAME=${NAME:-strata-bench-a}
REGION=${REGION:-nyc3}
NODE_SIZE=${NODE_SIZE:-g-8vcpu-32gb-intel}
STATE=${STATE:-$HOME/.strata-bench-a}
KUBECONFIG=$STATE/kubeconfig
export KUBECONFIG
K="kubectl -n bench"
POD=bench
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../../.." && pwd)

do_api() { # METHOD PATH [JSON]
    curl -sS -f -X "$1" -H "Authorization: Bearer ${DIGITALOCEAN_TOKEN:?set DIGITALOCEAN_TOKEN}" \
        -H "Content-Type: application/json" ${3:+--data "$3"} "$API$2"
}
cluster_id() { cat "$STATE/cluster_id"; }
in_pod() { $K exec "$POD" -- bash -lc "$1"; }

cmd=${1:-}
shift || true
case "$cmd" in
up)
    hours=${1:-14}
    deadline=$(( $(date +%s) + hours * 3600 ))
    mkdir -p "$STATE" && chmod 700 "$STATE"
    version=$(do_api GET /kubernetes/options | python3 -c 'import json, sys; print(json.load(sys.stdin)["options"]["versions"][0]["slug"])')
    body=$(python3 - "$NAME" "$REGION" "$version" "$NODE_SIZE" <<'PY'
import json, sys
name, region, version, size = sys.argv[1:]
print(json.dumps({"name": name, "region": region, "version": version, "tags": ["strata-bench"],
                  "node_pools": [{"name": "bench", "size": size, "count": 1, "tags": ["strata-bench"]}]}))
PY
)
    do_api POST /kubernetes/clusters "$body" | python3 -c 'import json, sys; print(json.load(sys.stdin)["kubernetes_cluster"]["id"])' >"$STATE/cluster_id"
    echo "cluster $(cluster_id) ($version, 1 x $NODE_SIZE in $REGION): waiting for it to run"
    until [ "$(do_api GET "/kubernetes/clusters/$(cluster_id)" | python3 -c 'import json, sys; print(json.load(sys.stdin)["kubernetes_cluster"]["status"]["state"])')" = running ]; do sleep 15; done
    do_api GET "/kubernetes/clusters/$(cluster_id)/kubeconfig" >"$KUBECONFIG"
    chmod 600 "$KUBECONFIG"
    kubectl create namespace bench --dry-run=client -o yaml | kubectl apply -f -
    echo "$deadline" >"$STATE/deadline"
    # The timer: deletes the cluster and what it made after $hours, from inside it.
    $K create secret generic do-token --from-literal=token="$DIGITALOCEAN_TOKEN" --dry-run=client -o yaml | $K apply -f - >/dev/null
    $K apply -f - <<EOF
apiVersion: v1
kind: Pod
metadata: {name: teardown}
spec:
  restartPolicy: OnFailure
  containers:
    - name: teardown
      image: curlimages/curl:8.17.0
      env:
        - {name: TOKEN, valueFrom: {secretKeyRef: {name: do-token, key: token}}}
      # An absolute deadline, so a restarted pod does not start the wait over.
      command: [sh, -c, "while [ \$(date +%s) -lt $deadline ]; do sleep 60; done; until curl -sS -f -X DELETE -H \"Authorization: Bearer \$TOKEN\" $API/kubernetes/clusters/$(cluster_id)/destroy_with_associated_resources/dangerous; do sleep 60; done"]
      resources: {requests: {cpu: 10m, memory: 16Mi}}
EOF
    # The bench pod: the whole node, its work on the node's own SSD.
    $K apply -f - <<EOF
apiVersion: v1
kind: Pod
metadata: {name: $POD}
spec:
  containers:
    - name: bench
      image: python:3.12-bookworm
      command: [sleep, infinity]
      env:
        - {name: NODE_SIZE, value: "$NODE_SIZE"}
        - {name: NODE_REGION, value: "$REGION"}
      # Requests (no limits) keep the pod from being the first one evicted.
      resources: {requests: {cpu: "6", memory: 24Gi}}
      volumeMounts:
        - {name: work, mountPath: /work}
  volumes:
    - name: work
      hostPath: {path: /var/lib/strata-bench, type: DirectoryOrCreate}
EOF
    $K wait --for=condition=Ready "pod/$POD" --timeout=600s
    echo "up: teardown at $(date -u -d "@$deadline" +%Y-%m-%dT%H:%MZ) ($hours h)"
    ;;
push)
    in_pod "rm -rf /work/strata && mkdir -p /work/strata /work/logs"
    (cd "$REPO" && git archive HEAD) | $K exec -i "$POD" -- tar -x -C /work/strata
    in_pod "echo pushed: \$(ls /work/strata | wc -l) entries"
    ;;
setup)
    # In the background (a long kubectl exec can be cut); `status` shows its log.
    in_pod "mkdir -p /work/logs; cat >/work/setup.sh <<'SH'
set -e
apt-get update -qq && apt-get install -y -qq build-essential curl git >/dev/null
command -v cargo >/dev/null || curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal >/dev/null
. \$HOME/.cargo/env
python3 -m venv /work/venv && . /work/venv/bin/activate
pip install -q maturin numpy pyarrow duckdb lancedb tantivy sqlite-vec faiss-cpu fastembed psutil
cd /work/strata/strata-py && maturin develop --release -q
cd /work/strata/strata && cargo build --release --features baselines --bin strata-baselines -q
python3 -c 'import strata, sqlite3; c = sqlite3.connect(\":memory:\"); c.enable_load_extension(True); print(\"SETUP DONE\")'
SH
        nohup bash /work/setup.sh >/work/logs/setup.log 2>&1 &
        echo setup started"
    ;;
prep)
    in_pod "set -e; . /work/venv/bin/activate; mkdir -p /work/data/shop-raw /work/data/beir; cd /work/strata
        HF=https://huggingface.co/datasets/McAuley-Lab/Amazon-Reviews-2023/resolve/main/raw
        [ -s /work/data/shop-raw/meta_Video_Games.jsonl ] || curl -sSfL -o /work/data/shop-raw/meta_Video_Games.jsonl \$HF/meta_categories/meta_Video_Games.jsonl
        [ -s /work/data/shop-raw/Video_Games.jsonl ] || curl -sSfL -o /work/data/shop-raw/Video_Games.jsonl \$HF/review_categories/Video_Games.jsonl
        nohup sh -c 'python3 strata-py/bench/laptop/shop_prep.py --meta /work/data/shop-raw/meta_Video_Games.jsonl --reviews /work/data/shop-raw/Video_Games.jsonl --out /work/data/shop --threads 8 \
            && python3 strata/scripts/prepare_beir.py /work/data/beir scifact nfcorpus fiqa trec-covid quora \
            && STRATA_DATA=/work/data/beir strata/target/release/strata-baselines --collections nfcorpus --workload-only \
            && echo PREP DONE' >/work/logs/prep.log 2>&1 &
        echo prep started"
    ;;
run)
    in_pod "set -e; . /work/venv/bin/activate; cd /work/strata; export NODE_REGION=$REGION
        B=strata-py/bench/laptop OUT=/work/out/part-a; mkdir -p \$OUT
        nohup sh -c \"python3 \$B/a1_app.py --shop /work/data/shop --work /work/w/a1 --out \$OUT \
            ; python3 \$B/a2_search.py --data /work/data/beir --collections scifact,nfcorpus,fiqa,trec-covid,quora --work /work/w/a2 --out \$OUT \
            ; python3 \$B/a_writes.py --shop /work/data/shop --work /work/w/aw --out \$OUT \
            ; python3 \$B/a3_recsys.py --shop /work/data/shop --work /work/w/a3 --out \$OUT \
            ; echo RUN DONE\" >/work/logs/run.log 2>&1 &
        echo run started"
    ;;
status)
    in_pod "for f in /work/logs/*.log; do echo \"== \$f\"; tail -n 5 \$f; done; uptime; df -h /work | tail -1"
    ;;
fetch)
    dir=${1:?fetch DIR}
    mkdir -p "$dir"
    in_pod "cd /work && tar -c out logs" | tar -x -C "$dir"
    echo "fetched into $dir"
    ;;
down)
    do_api DELETE "/kubernetes/clusters/$(cluster_id)/destroy_with_associated_resources/dangerous" >/dev/null || true
    sleep 30
    left=$(do_api GET "/droplets?tag_name=strata-bench" | python3 -c 'import json, sys; print(len(json.load(sys.stdin)["droplets"]))')
    clusters=$(do_api GET /kubernetes/clusters | python3 -c 'import json, sys; print(sum(c["name"] == "'"$NAME"'" for c in json.load(sys.stdin)["kubernetes_clusters"]))')
    echo "left: $left droplets tagged strata-bench, $clusters clusters named $NAME (droplets take a minute or two to go)"
    ;;
*)
    sed -n '2,19p' "$0"
    exit 1
    ;;
esac
