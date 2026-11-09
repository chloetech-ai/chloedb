#!/usr/bin/env bash
# Part A of the laptop benchmark on DigitalOcean: one dedicated-CPU node in a
# DOKS cluster (nyc3), driven from here over the Kubernetes API (kubectl exec
# and cp), so nothing needs SSH. Everything it creates is tagged
# strata-bench, and a pod in the cluster deletes the cluster (with its
# volumes and load balancers) after HOURS even if this script never runs
# `down`.
#
#   do_part_a.sh up [HOURS]   # (token in DIGITALOCEAN_TOKEN or digitalocean)   # cluster + teardown timer + bench pod (default 14 h)
#   do_part_a.sh extend HOURS # move the teardown timer HOURS later
#   do_part_a.sh push        # this checkout (git archive of HEAD) into the pod
#   do_part_a.sh setup       # Rust, the Python libraries, chloedb-py built in release
#   do_part_a.sh prep        # Shop (Amazon Reviews 2023 Video Games) and the BEIR collections, embedded there
#   do_part_a.sh run         # A1, A2, writes, A3 in the background; logs in /work/logs
#                            # (PARTS="a1 a2 writes a3" picks parts; A1_SYSTEMS, A2_SYSTEMS,
#                            # A2_KINDS, WRITES_SYSTEMS, A3_SYSTEMS narrow them; OUT names the run)
#   do_part_a.sh status      # the last log lines and the node's load
#   do_part_a.sh fetch DIR   # the reports and logs into DIR
#   do_part_a.sh down        # delete the cluster and check nothing tagged strata-bench is left
#
# NODE_SIZE (default g-8vcpu-32gb-intel, $0.4494 an hour at list price: nyc3
# has no plain g-8vcpu-32gb) is the droplet the benchmark runs on; the
# reports name its CPU. SHOP_EMBED (default fastembed) is how prep makes the
# products' vectors: random for a run of systems that use none (Tantivy).
set -euo pipefail

API=https://api.digitalocean.com/v2
# The token: DIGITALOCEAN_TOKEN, or the environment's variable named digitalocean.
DIGITALOCEAN_TOKEN=${DIGITALOCEAN_TOKEN:-${digitalocean:-${DIGITALOCEAN:-}}}
NAME=${NAME:-strata-bench-a}
REGION=${REGION:-nyc3}
NODE_SIZE=${NODE_SIZE:-g-8vcpu-32gb-intel}
SHOP_EMBED=${SHOP_EMBED:-fastembed}
PARTS=${PARTS:-a1 a2 writes a3}
OUT=${OUT:-part-a}
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
# The timer: a pod that deletes the cluster and what it made at DEADLINE
# (seconds since the epoch), from inside it. An absolute deadline, so a
# restarted pod does not start the wait over.
timer() { # DEADLINE
    $K apply -f - <<EOF
apiVersion: v1
kind: Pod
metadata: {name: teardown-$1, labels: {app: teardown}}
spec:
  restartPolicy: OnFailure
  containers:
    - name: teardown
      image: curlimages/curl:8.17.0
      env:
        - {name: TOKEN, valueFrom: {secretKeyRef: {name: do-token, key: token}}}
      command: [sh, -c, "while [ \$(date +%s) -lt $1 ]; do sleep 60; done; until curl -sS -f -X DELETE -H \"Authorization: Bearer \$TOKEN\" $API/kubernetes/clusters/$(cluster_id)/destroy_with_associated_resources/dangerous; do sleep 60; done"]
      resources: {requests: {cpu: 10m, memory: 16Mi}}
EOF
    $K wait --for=condition=Ready "pod/teardown-$1" --timeout=300s >/dev/null
    echo "$1" >"$STATE/deadline"
}

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
    # The token for the timer.
    $K create secret generic do-token --from-literal=token="$DIGITALOCEAN_TOKEN" --dry-run=client -o yaml | $K apply -f - >/dev/null
    timer "$deadline"
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
extend)
    # Moves the timer HOURS later: the new timer runs before the old one goes.
    hours=${1:?extend HOURS}
    old=$(cat "$STATE/deadline")
    new=$(( old + hours * 3600 ))
    timer "$new"
    for p in $($K get pods -l app=teardown -o name) $($K get pod teardown -o name 2>/dev/null); do
        [ "$p" = "pod/teardown-$new" ] || $K delete "$p" --wait=false
    done
    echo "teardown moved from $(date -u -d "@$old" +%Y-%m-%dT%H:%MZ) to $(date -u -d "@$new" +%Y-%m-%dT%H:%MZ)"
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
cd /work/strata/chloedb-py && maturin develop --release -q
cd /work/strata/strata && cargo build --release --features baselines --bin chloedb-baselines -q
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
        nohup sh -c 'python3 chloedb-py/bench/laptop/shop_prep.py --meta /work/data/shop-raw/meta_Video_Games.jsonl --reviews /work/data/shop-raw/Video_Games.jsonl --out /work/data/shop --threads 8 --embed $SHOP_EMBED \
            && python3 chloedb/scripts/prepare_beir.py /work/data/beir scifact nfcorpus fiqa trec-covid quora \
            && STRATA_DATA=/work/data/beir chloedb/target/release/chloedb-baselines --collections nfcorpus --workload-only \
            && echo PREP DONE' >/work/logs/prep.log 2>&1 &
        echo prep started"
    ;;
run)
    in_pod "set -e; . /work/venv/bin/activate; cd /work/strata; export NODE_REGION=$REGION
        B=chloedb-py/bench/laptop OUT=/work/out/$OUT; mkdir -p \$OUT
        steps=''
        case ' $PARTS ' in *' a1 '*) steps=\"\$steps python3 \$B/a1_app.py --shop /work/data/shop --work /work/w/a1 --out \$OUT ${A1_SYSTEMS:+--systems $A1_SYSTEMS};\" ;; esac
        case ' $PARTS ' in *' a2 '*) steps=\"\$steps python3 \$B/a2_search.py --data /work/data/beir --collections scifact,nfcorpus,fiqa,trec-covid,quora --work /work/w/a2 --out \$OUT ${A2_SYSTEMS:+--systems $A2_SYSTEMS} ${A2_KINDS:+--kinds $A2_KINDS};\" ;; esac
        case ' $PARTS ' in *' writes '*) steps=\"\$steps python3 \$B/a_writes.py --shop /work/data/shop --work /work/w/aw --out \$OUT ${WRITES_SYSTEMS:+--systems $WRITES_SYSTEMS};\" ;; esac
        case ' $PARTS ' in *' a3 '*) steps=\"\$steps python3 \$B/a3_recsys.py --shop /work/data/shop --work /work/w/a3 --out \$OUT ${A3_SYSTEMS:+--systems $A3_SYSTEMS};\" ;; esac
        nohup sh -c \"\$steps echo RUN DONE\" >/work/logs/run-$OUT.log 2>&1 &
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
