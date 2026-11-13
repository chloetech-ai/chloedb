#!/usr/bin/env python3
"""The speed gate: two builds of ChloeDB timed against each other on one machine.

    scripts/perf-gate.py --base BASE_BIN_DIR --head HEAD_BIN_DIR --beir DIR \
        [--rounds 3] [--threshold 0.10] [--out DIR] [--summary FILE]

Each bin dir holds `chloedb-bm25-eval` (and, when `--vectors` is given,
`chloedb-server-bench`). The BEIR collections are the relevance gate's
(`chloedb/tests/fixtures/beir/fetch.sh DIR`).

A shared CI runner's speed moves from one run to the next by more than most
regressions, so the gate never compares a time with a number written down on
another day: it runs the base and the head in turns on the same machine
(base, head, then head, base, ...), `--rounds` times each, and compares
them. A measure regresses when the head's median is more than `--threshold`
above the base's AND every round of the head is slower than every round of
the base (the two ranges do not overlap), so that one slow round of noise
does not fail it.

What it times, through SQL, with `chloedb-bm25-eval` (the whole query path:
analysis, statistics, top-k):

- BM25 `plain` on SciFact, NFCorpus and ArguANA, mean ms per query;
- BM25 `fuzzy => 'auto'` on SciFact and NFCorpus (on ArguAna each of its long
  queries takes 0.7 s, as in the relevance gate);
- each collection's load: rows written and merged, seconds.

With `--vectors DIR` (collections prepared by
`chloedb/scripts/prepare_beir.py`, with their Cohere vectors) it also runs
`chloedb-server-bench --collection scifact` for each build and compares the
server's p50 per query kind over HTTP at one client. That step takes longer
and is nightly only.

nDCG@10 is printed beside the times: a change there is the relevance gate's
to fail (tests/relevance_gate.rs), this script only reports it.

Writes `<out>/perf-gate.json` and a Markdown table (`--summary`, e.g.
$GITHUB_STEP_SUMMARY). Exits 1 when a measure regressed, 2 on a failure to
run.
"""

import argparse
import json
import os
import statistics
import subprocess
import sys
import tempfile
import time

PLAIN = ["scifact", "nfcorpus", "arguana"]
FUZZY = ["scifact", "nfcorpus"]


def run(cmd, log):
    t = time.time()
    with open(log, "a") as f:
        f.write("$ " + " ".join(cmd) + "\n")
        f.flush()
        r = subprocess.run(cmd, stdout=f, stderr=subprocess.STDOUT)
    if r.returncode != 0:
        sys.exit(f"perf-gate: {' '.join(cmd)} failed ({r.returncode}); see {log}")
    return time.time() - t


def bm25_round(bindir, beir, out, tag, reps, log, tmp):
    """One round of a build: {measure: value}, {measure: ndcg}."""
    exe = os.path.join(bindir, "chloedb-bm25-eval")
    times, ndcg = {}, {}
    for variants, cols, suffix in (("plain", PLAIN, "p"), ("fuzzy", FUZZY, "f")):
        t = f"{tag}-{suffix}"
        run([exe, "--dir", beir, "--collections", ",".join(cols), "--variants", variants,
             "--reps", str(reps), "--tag", t, "--out", out, "--tmp", tmp], log)
        with open(os.path.join(out, f"bm25_{t}.json")) as f:
            d = json.load(f)
        for c, cd in d["collections"].items():
            if suffix == "p":
                times[f"load {c} (s)"] = cd["load_s"]
            for v, vd in cd["variants"].items():
                if vd.get("failed"):
                    sys.exit(f"perf-gate: {vd['failed']} queries failed in {c} {v} ({tag})")
                times[f"bm25 {v} {c} (ms/query)"] = vd["ms_mean"]
                ndcg[f"bm25 {v} {c} (ms/query)"] = vd["ndcg10"]
    return times, ndcg


def server_round(bindir, vectors, out, tag, log, work):
    """chloedb-server-bench on SciFact: {kind p50 at 1 client: ms}."""
    exe = os.path.join(bindir, "chloedb-server-bench")
    o = os.path.join(out, f"server-{tag}")
    w = os.path.join(work, f"server-{tag}")
    os.makedirs(o, exist_ok=True)
    env = dict(os.environ, CHLOEDB_DATA=vectors)
    cmd = [exe, "--collection", "scifact", "--work", w, "--out", o, "--seconds", "3", "--concurrency", "1"]
    t = time.time()
    with open(log, "a") as f:
        f.write("$ " + " ".join(cmd) + "\n")
        f.flush()
        r = subprocess.run(cmd, stdout=f, stderr=subprocess.STDOUT, env=env)
    if r.returncode != 0:
        sys.exit(f"perf-gate: {' '.join(cmd)} failed ({r.returncode}); see {log}")
    with open(os.path.join(o, "scifact.json")) as f:
        d = json.load(f)
    times = {}
    for row in d.get("http", []):
        if row.get("concurrency") == 1 and "p50_ms" in row:
            times[f"http {row.get('setup', '')} {row['kind']} p50 (ms)"] = row["p50_ms"]
    return times


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--base", required=True)
    ap.add_argument("--head", required=True)
    ap.add_argument("--beir", required=True)
    ap.add_argument("--vectors")
    ap.add_argument("--rounds", type=int, default=3)
    ap.add_argument("--reps", type=int, default=3)
    ap.add_argument("--threshold", type=float, default=0.10)
    ap.add_argument("--out", default="perf-gate")
    ap.add_argument("--summary")
    ap.add_argument("--base-label", default="base")
    ap.add_argument("--head-label", default="head")
    a = ap.parse_args()

    os.makedirs(a.out, exist_ok=True)
    log = os.path.join(a.out, "perf-gate.log")
    open(log, "w").close()
    tmp = tempfile.mkdtemp(prefix="perf-gate-")
    builds = {"base": a.base, "head": a.head}
    rounds = {"base": [], "head": []}
    ndcg = {"base": {}, "head": {}}
    for r in range(a.rounds):
        order = ["base", "head"] if r % 2 == 0 else ["head", "base"]
        for side in order:
            print(f"round {r + 1}/{a.rounds}: {side}", flush=True)
            times, n = bm25_round(builds[side], a.beir, a.out, f"{side}{r}", a.reps, log, tmp)
            if a.vectors:
                times.update(server_round(builds[side], a.vectors, a.out, f"{side}{r}", log, tmp))
            rounds[side].append(times)
            ndcg[side] = n

    measures = sorted(set(rounds["base"][0]) & set(rounds["head"][0]))
    rows, regressed = [], []
    for m in measures:
        b = [x[m] for x in rounds["base"] if m in x]
        h = [x[m] for x in rounds["head"] if m in x]
        bm, hm = statistics.median(b), statistics.median(h)
        ratio = hm / bm if bm > 0 else 1.0
        apart_slower = min(h) > max(b)
        apart_faster = max(h) < min(b)
        verdict = "ok"
        if ratio > 1 + a.threshold and apart_slower:
            verdict = "SLOWER"
            regressed.append(m)
        elif ratio < 1 - a.threshold and apart_faster:
            verdict = "faster"
        rows.append({"measure": m, "base": b, "head": h, "base_median": bm, "head_median": hm,
                     "ratio": ratio, "verdict": verdict,
                     "ndcg10_base": ndcg["base"].get(m), "ndcg10_head": ndcg["head"].get(m)})

    result = {"base": a.base_label, "head": a.head_label, "rounds": a.rounds, "reps": a.reps,
              "threshold": a.threshold, "machine": machine(), "measures": rows, "regressed": regressed}
    with open(os.path.join(a.out, "perf-gate.json"), "w") as f:
        json.dump(result, f, indent=1)

    md = [f"### Speed gate: {a.head_label} against {a.base_label}", "",
          f"{a.rounds} rounds each, in turns, on {result['machine']}. A measure fails when the head's "
          f"median is more than {a.threshold:.0%} above the base's and every head round is slower than "
          "every base round.", "",
          "| measure | base | head | change | nDCG@10 base / head | |", "|---|---:|---:|---:|---|---|"]
    for row in rows:
        nd = ""
        if row["ndcg10_base"] is not None:
            nd = f"{row['ndcg10_base']:.4f} / {row['ndcg10_head']:.4f}"
            if abs(row["ndcg10_base"] - row["ndcg10_head"]) > 0.002:
                nd += " (changed)"
        md.append(f"| {row['measure']} | {fmt(row['base_median'])} | {fmt(row['head_median'])} | "
                  f"{row['ratio'] - 1:+.1%} | {nd} | {row['verdict']} |")
    md.append("")
    md.append("**Regressed:** " + ", ".join(regressed) if regressed else "No measure regressed.")
    text = "\n".join(md) + "\n"
    print(text)
    if a.summary:
        with open(a.summary, "a") as f:
            f.write(text)
    sys.exit(1 if regressed else 0)


def fmt(x):
    return f"{x:.3f}" if x < 10 else f"{x:.1f}"


def machine():
    cpu = "?"
    try:
        with open("/proc/cpuinfo") as f:
            for line in f:
                if line.startswith("model name"):
                    cpu = line.split(":", 1)[1].strip()
                    break
    except OSError:
        pass
    return f"{cpu}, {os.cpu_count()} CPUs"


if __name__ == "__main__":
    main()
