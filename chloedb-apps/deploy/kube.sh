#!/usr/bin/env bash
# Puts the studio on the Kubernetes cluster kubectl points at (deploy/kube.yaml,
# deploy/README.md) and prints the link to share once it answers.
#
#   deploy/kube.sh --ref <full commit sha> --node <node name> \
#     [--image ghcr.io/chloetech-ai/strata-server@sha256:...] [--nodeport 30730] [--cap 25]
#
# --ref is a commit of github.com/chloetech-ai/strata that holds chloedb-apps
# (pushed: the pod fetches it). --node is the node whose disk keeps the data.
# The namespace "strata" must hold the Secret "anthropic" (key "key"); the
# gate's key is made once, in the Secret "apps-studio-gate", and kept.
set -euo pipefail
cd "$(dirname "$0")"

REF='' NODE='' IMAGE='ghcr.io/chloetech-ai/strata-server:latest' NODEPORT=30730 CAP=25 NS=strata
while [ $# -gt 0 ]; do
  case "$1" in
    --ref) REF="$2"; shift 2 ;;
    --node) NODE="$2"; shift 2 ;;
    --image) IMAGE="$2"; shift 2 ;;
    --nodeport) NODEPORT="$2"; shift 2 ;;
    --cap) CAP="$2"; shift 2 ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
done
[[ "$REF" =~ ^[0-9a-f]{40}$ ]] || { echo "--ref takes a full 40-character commit sha" >&2; exit 2; }
[ -n "$NODE" ] || { echo "--node takes a node name (kubectl get nodes)" >&2; exit 2; }
KUBECTL="${KUBECTL:-kubectl}"

"$KUBECTL" -n "$NS" get secret anthropic -o name >/dev/null || { echo "the namespace $NS has no Secret anthropic (key: key) for the model" >&2; exit 1; }
if ! "$KUBECTL" -n "$NS" get secret apps-studio-gate -o name >/dev/null 2>&1; then
  key="$(node -e "console.log(require('crypto').randomBytes(24).toString('base64url'))")"
  "$KUBECTL" -n "$NS" create secret generic apps-studio-gate --from-literal=key="$key" >/dev/null
  echo "made the gate's key (Secret $NS/apps-studio-gate)"
fi

sed -e "s|__REF__|$REF|" -e "s|__NODE__|$NODE|" -e "s|__NODEPORT__|$NODEPORT|" -e "s|__SPEND_CAP__|$CAP|" -e "s|__STRATA_IMAGE__|$IMAGE|" kube.yaml | "$KUBECTL" apply -f -
"$KUBECTL" -n "$NS" rollout status deploy/apps-studio --timeout=10m

ip="$("$KUBECTL" get node "$NODE" -o jsonpath='{.status.addresses[?(@.type=="ExternalIP")].address}')"
key="$("$KUBECTL" -n "$NS" get secret apps-studio-gate -o jsonpath='{.data.key}' | base64 -d)"
echo "the studio: http://$ip:$NODEPORT/?key=$key"
echo "(the node's firewall must let TCP $NODEPORT in: deploy/README.md)"
