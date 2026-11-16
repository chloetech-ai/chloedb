# Sourced by setup.sh and drive.sh.
# S: where the scripts, the binaries (dist/) and the results (out/) live.
S=${S:-$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)}
# KUBECONFIG: the cluster's, e.g. from `doctl kubernetes cluster kubeconfig show NAME`.
K="kubectl --request-timeout=0"
LOG=$S/drive.log
COPIES=(c00 c01 c02 c10 c11 c12 c20 c21 c22)
BENCH=bench
log() { echo "$(date -u +%H:%M:%SZ) $*" | tee -a $LOG; }
# ex POD CMD: CMD on POD. An exec the cluster's agents could not open (it never
# ran) is tried again, up to 8 times over about 2 minutes.
AGENT_ERR='No agent available|unable to upgrade connection|error dialing backend'
ex() { local p=$1; shift; local i rc err=$S/ex-err-$p-$$-$RANDOM
  for i in 1 2 3 4 5 6 7 8; do
    $K exec $p -- bash -c "$*" 2>$err; rc=$?
    if [ $rc -ne 0 ] && grep -qE "$AGENT_ERR" $err; then sleep $((i * 4)); continue; fi
    break
  done
  cat $err >&2; rm -f $err; return $rc; }
# kcp SRC DEST: kubectl cp, tried again the same way.
kcp() { local i; for i in 1 2 3 4 5 6 7 8; do $K cp "$@" >/dev/null 2>&1 && return 0; sleep $((i * 4)); done; return 1; }
# par CMD-TEMPLATE PODS...: CMD run on each pod at once ({} is the pod); fails if any failed.
par() { local cmd=$1; shift; local pids=() p rc=0
  for p in "$@"; do ex $p "${cmd//\{\}/$p}" > $S/par-$p.log 2>&1 & pids+=($!); done
  for i in "${!pids[@]}"; do wait ${pids[$i]} || { rc=1; echo "par failed on ${@:$((i+1)):1}: $(tail -3 $S/par-${@:$((i+1)):1}.log)" | tee -a $LOG; }; done
  return $rc; }
