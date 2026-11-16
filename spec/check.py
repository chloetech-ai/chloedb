#!/usr/bin/env python3
"""Runs TLC on the models of ChloeDB's TLA+ specifications (spec/README.md).

Each model is a TLC configuration beside its specification and says what
TLC must find: no error, or a named invariant broken with a trace of so
many states (a bug the specification reproduces, as a regression test of
the model itself). The CI workflow Specs (.github/workflows/specs.yml) runs
the models marked `ci`, which take seconds; the others take minutes to
hours, and spec/README.md gives what each found when it was last run.

    python3 spec/check.py                 # the CI models
    python3 spec/check.py --all           # every model
    python3 spec/check.py FixStall ...    # these
    python3 spec/check.py --list

TLC comes from tla2tools.jar: `--jar PATH`, `$TLA2TOOLS`, or the release
below, downloaded into ~/.cache/chloedb-spec and checked against its
SHA-256. Needs Java 11 or newer.
"""
import argparse
import concurrent.futures
import hashlib
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.request

TLA_VERSION = "1.8.0"
TLA_URL = f"https://github.com/tlaplus/tlaplus/releases/download/v{TLA_VERSION}/tla2tools.jar"
TLA_SHA256 = "7e53aa636dac8a3cb0533f343115c0548e03fc0ce5edc7739becf58d3d0fb84f"

HERE = os.path.dirname(os.path.abspath(__file__))

# (folder, model module, configuration, expected, trace states, ci)
# expected: "ok", or the invariant TLC must find broken.
MODELS = [
    # Shard elections (replication/ShardElection.tla).
    ("replication", "MCShardElection", "CINE2Stall", "NeverStalled", 4, True),
    ("replication", "MCShardElection", "CIFixStall", "ok", None, True),
    ("replication", "MCShardElection", "CIFixNoLoss", "ok", None, True),
    ("replication", "MCShardElection", "CILostVolume", "NoLostWrite", 9, True),
    ("replication", "MCShardElection", "CIMembersLostVolume", "ok", None, True),
    ("replication", "MCShardElection", "CIMembersStall", "ok", None, True),
    ("replication", "MCShardElection", "MembersAmnesia", "OneCopyATerm", 10, True),
    ("replication", "MCShardElection", "NE2Stall", "NeverStalled", 4, False),
    ("replication", "MCShardElection", "FixStall", "ok", None, False),
    ("replication", "MCShardElection", "PreNE2LostVolume", "NoLostWrite", 7, False),
    ("replication", "MCShardElection", "NarrowLostVolume", "NoLostWrite", 9, False),
    ("replication", "MCShardElection", "FixLostVolume", "NoLostWrite", 9, False),
    ("replication", "MCShardElection", "MembersStall", "ok", None, False),
    ("replication", "MCShardElection", "MembersLostVolume", "ok", None, False),
    # Published snapshots (storage/SnapshotPublish.tla).
    ("storage", "MCSnapshotPublish", "SnapFALSE", "NeverBack", 9, True),
    ("storage", "MCSnapshotPublish", "SnapTRUE", "ok", None, True),
    # Slot moves from a replicated shard (slots/SlotMove.tla).
    ("slots", "MCSlotMove", "MoveNow", "NoLostWrite", 9, True),
    ("slots", "MCSlotMove", "MoveStuck", "NoStuckShard", 10, True),
    ("slots", "MCSlotMove", "MoveMark", "ok", None, True),
    ("slots", "MCSlotMove", "MoveMarkStale", "ok", None, True),
    # Changes of the cluster map (clustermap/MapChange.tla).
    ("clustermap", "MCMapChange", "GivesTwice", "NoFork", 10, True),
    ("clustermap", "MCMapChange", "RoutesUntaken", "RoutesHeld", 6, True),
    ("clustermap", "MCMapChange", "ForgetsRedone", "NoFork", 15, True),
    ("clustermap", "MCMapChange", "AroundSource", "NoStuck", 9, True),
    ("clustermap", "MCMapChange", "Forgetful", "NoFork", 8, True),
    ("clustermap", "MCMapChange", "Racing", "NoFork", 7, True),
    ("clustermap", "MCMapChange", "LeaseRacing", "NoFork", 9, True),
    ("clustermap", "MCMapChange", "SurveyAny", "NoFork", 10, True),
    ("clustermap", "MCMapChange", "QuorumRacing", "NoFork", 15, True),
    ("clustermap", "MCMapChange", "KeepsRacing", "ok", None, True),
    ("clustermap", "MCMapChange", "CIKeeps", "ok", None, True),
    ("clustermap", "MCMapChange", "CIOneKeeps", "ok", None, True),
    ("clustermap", "MCMapChange", "CIOneMove", "ok", None, True),
    ("clustermap", "MCMapChange", "CILeaseOne", "ok", None, True),
    ("clustermap", "MCMapChange", "CIQuorum", "ok", None, True),
    ("clustermap", "MCMapChange", "OneKeeps", "ok", None, False),
    ("clustermap", "MCMapChange", "OneMove", "ok", None, False),
    ("clustermap", "MCMapChange", "LeaseOne", "ok", None, False),
    ("clustermap", "MCMapChange", "Quorum", "ok", None, False),
    ("clustermap", "MCMapChange", "Keeps", "ok", None, False),
    # Idempotency keys across failover (idempotency/IdempotencyKeys.tla).
    ("idempotency", "MCIdempotencyKeys", "KeysNow", "NoLostWrite", 10, True),
    ("idempotency", "MCIdempotencyKeys", "KeysDS5", "NoLostWrite", 12, True),
    ("idempotency", "MCIdempotencyKeys", "KeysEvict", "AppliedOnce", 6, True),
    ("idempotency", "MCIdempotencyKeys", "KeysRowSeq", "SameSeq", 8, True),
    ("idempotency", "MCIdempotencyKeys", "CIKeysFix", "ok", None, True),
    ("idempotency", "MCIdempotencyKeys", "CIEvictFix", "ok", None, True),
    ("idempotency", "MCIdempotencyKeys", "KeysFix", "ok", None, False),
    ("idempotency", "MCIdempotencyKeys", "EvictFix", "ok", None, False),
    # Handover and the read lease (handover/Handover.tla).
    ("handover", "MCHandover", "LeaseBug", "LeaseReadsFresh", 11, True),
    ("handover", "MCHandover", "CILease", "ok", None, True),
    ("handover", "MCHandover", "Auto", "ok", None, True),
    ("handover", "MCHandover", "Restart", "LeaseReadsFresh", 11, True),
    ("handover", "MCHandover", "RestartFix", "ok", None, True),
    ("handover", "MCHandover", "Drift2", "LeaseReadsFresh", 13, True),
    ("handover", "MCHandover", "TwoWriters", "OneWriter", 8, True),
    ("handover", "MCHandover", "OrderBug", "NoOrphanedFence", 9, True),
    ("handover", "MCHandover", "Order", "ok", None, True),
    ("handover", "MCHandover", "OrderLossy", "NoOrphanedFence", 8, True),
    ("handover", "MCHandover", "OrderRace", "NoOrphanedFence", 18, True),
    ("handover", "MCHandover", "Lease", "ok", None, False),
    ("handover", "MCHandover", "Drift1", "ok", None, False),
    ("handover", "MCHandover", "HoldUnasked", "LeaseReadsFresh", 19, False),
    ("handover", "MCHandover", "HoldAllGranted", "ok", None, False),
    # Commit path and crash recovery (wal/WalRecovery.tla).
    ("wal", "MCWalRecovery", "CIRecover", "ok", None, True),
    ("wal", "MCWalRecovery", "CIMergeNoCarry", "LiveRowsCommitted", 19, True),
    ("wal", "MCWalRecovery", "CINoSyncFirst", "RecoveredPrefix", 15, True),
    ("wal", "MCWalRecovery", "CINoSync", "ok", None, True),
    ("wal", "MCWalRecovery", "CITornSlot", "OpensAfterCrash", 14, True),
    ("wal", "MCWalRecovery", "CITornSafe", "ok", None, True),
    ("wal", "MCWalRecovery", "CIRot", "NoSilentLoss", 13, True),
    ("wal", "MCWalRecovery", "CIRotTold", "ok", None, True),
    ("wal", "MCWalRecovery", "CIFixIntent", "ok", None, True),
    ("wal", "MCWalRecovery", "CIFixRot", "ok", None, True),
    ("wal", "MCWalRecovery", "CIGroups", "ok", None, True),
    ("wal", "MCWalRecovery", "Recover", "ok", None, False),
    ("wal", "MCWalRecovery", "RotTold", "ok", None, False),
    ("wal", "MCWalRecovery", "FixRot", "ok", None, False),
    ("wal", "MCWalRecovery", "FixTorn", "ok", None, False),
]


def jar(path):
    if path:
        return path
    if os.environ.get("TLA2TOOLS"):
        return os.environ["TLA2TOOLS"]
    cache = os.path.join(os.path.expanduser("~"), ".cache", "chloedb-spec")
    dest = os.path.join(cache, f"tla2tools-{TLA_VERSION}.jar")
    if not os.path.exists(dest):
        os.makedirs(cache, exist_ok=True)
        with urllib.request.urlopen(TLA_URL) as r, tempfile.NamedTemporaryFile(dir=cache, delete=False) as f:
            shutil.copyfileobj(r, f)
        os.replace(f.name, dest)
    with open(dest, "rb") as f:
        got = hashlib.sha256(f.read()).hexdigest()
    if got != TLA_SHA256:
        os.remove(dest)
        sys.exit(f"{TLA_URL}: SHA-256 {got}, not {TLA_SHA256}")
    return dest


def run(tla, model, workers):
    folder, module, cfg, expect, states, _ = model
    meta = tempfile.mkdtemp(prefix=f"tlc-{cfg}-")
    start = time.monotonic()
    try:
        p = subprocess.run(
            ["java", "-XX:+UseParallelGC", "-cp", tla, "tlc2.TLC", "-workers", str(workers), "-deadlock",
             "-noGenerateSpecTE", "-metadir", meta, "-config", f"{cfg}.cfg", f"{module}.tla"],
            cwd=os.path.join(HERE, folder), capture_output=True, text=True)
    finally:
        shutil.rmtree(meta, ignore_errors=True)
    secs = time.monotonic() - start
    out = p.stdout + p.stderr
    broken = re.search(r"Error: Invariant (\w+) is violated", out)
    trace = len(re.findall(r"^State \d+:", out, re.M))
    found = re.findall(r"([\d,]+) distinct states found", out)
    distinct = found[-1] if found else "?"
    if expect == "ok":
        ok = "Model checking completed. No error has been found." in out
        got = "no error" if ok else (f"{broken.group(1)} broken in {trace} states" if broken else "TLC failed")
    else:
        ok = bool(broken) and broken.group(1) == expect and (states is None or trace == states)
        got = f"{broken.group(1)} broken in {trace} states" if broken else ("no error" if "No error has been found" in out else "TLC failed")
    want = "no error" if expect == "ok" else f"{expect} broken in {states} states"
    line = f"{'ok  ' if ok else 'FAIL'} {folder}/{cfg}: {got} ({distinct} distinct states, {secs:.0f} s)"
    if not ok:
        line += f"; expected {want}\n" + "\n".join(out.splitlines()[-40:])
    return ok, line


def main():
    a = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    a.add_argument("models", nargs="*")
    a.add_argument("--all", action="store_true")
    a.add_argument("--list", action="store_true")
    a.add_argument("--jar")
    a.add_argument("--jobs", type=int, default=os.cpu_count() or 1)
    args = a.parse_args()
    if args.list:
        for m in MODELS:
            print(f"{m[0]}/{m[2]}: {'no error' if m[3] == 'ok' else m[3] + ' broken'}{' (ci)' if m[5] else ''}")
        return
    names = set(args.models)
    unknown = names - {m[2] for m in MODELS}
    if unknown:
        sys.exit(f"no such model: {', '.join(sorted(unknown))} (--list)")
    chosen = [m for m in MODELS if m[2] in names] if names else [m for m in MODELS if args.all or m[5]]
    tla = jar(args.jar)
    failed = 0
    with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, args.jobs)) as pool:
        for ok, line in pool.map(lambda m: run(tla, m, 1), chosen):
            print(line, flush=True)
            failed += not ok
    if failed:
        sys.exit(f"{failed} of {len(chosen)} models did not find what they should")


if __name__ == "__main__":
    main()
