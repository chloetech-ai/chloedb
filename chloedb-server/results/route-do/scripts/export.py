#!/usr/bin/env python3
"""Copy the route-bench results to the shared project folder for the
simulation thread: route.json/route.md per scenario, a flat rows.csv, the
per-node CPU table, the Mann-Whitney analysis and a README of the setup.
Usage: python3 -I export.py RB_DIR DEST STATUS_LINE"""
import csv, json, os, shutil, sys

rb, dest, status = sys.argv[1], sys.argv[2], sys.argv[3]
os.makedirs(dest, exist_ok=True)

FIELDS = ["cache", "dir", "scenario", "shards", "rows", "config", "route", "route_load",
          "hedge_adaptive", "prefer_caught_up", "clients", "round", "qps", "queries",
          "p50_ms", "p90_ms", "p99_ms", "p999_ms", "max_ms", "errors", "hedges",
          "refused_429", "reads_after_write", "shard0_answers_by_copy",
          "shard0_mean_inflight_by_coordinator_copy", "setup"]

rows = []
for cache, src in (("off", "out"), ("on", "out-cacheon"), ("off-extra", "out-extra")):
    base = os.path.join(rb, src)
    if not os.path.isdir(base):
        continue
    for name in sorted(os.listdir(base)):
        j = os.path.join(base, name, "route.json")
        if not os.path.isfile(j):
            continue
        out = os.path.join(dest, f"cache-{cache}", name)
        os.makedirs(out, exist_ok=True)
        for f in ("route.json", "route.md"):
            p = os.path.join(base, name, f)
            if os.path.isfile(p):
                shutil.copyfile(p, os.path.join(out, f))
        d = json.load(open(j))
        a = d.get("args", {})
        setup = d.get("machine", {}).get("setup", "")
        for r in d.get("runs", []):
            c = r.get("client", {})
            rows.append({
                "cache": cache, "dir": name, "scenario": r.get("scenario"),
                "shards": a.get("shards"), "rows": a.get("rows"),
                "config": r.get("config"), "route": r.get("route"),
                "route_load": r.get("route_load"), "hedge_adaptive": r.get("hedge_adaptive"),
                "prefer_caught_up": r.get("prefer_caught_up"), "clients": r.get("clients"),
                "round": r.get("round"), "qps": c.get("qps"), "queries": c.get("queries"),
                "p50_ms": c.get("p50_ms"), "p90_ms": c.get("p90_ms"), "p99_ms": c.get("p99_ms"),
                "p999_ms": c.get("p999_ms"), "max_ms": c.get("max_ms"), "errors": c.get("errors"),
                "hedges": r.get("hedges"), "refused_429": r.get("refused_429"),
                "reads_after_write": c.get("reads_after_write"),
                "shard0_answers_by_copy": json.dumps(r.get("shard0_answers_by_copy")),
                "shard0_mean_inflight_by_coordinator_copy": json.dumps(r.get("shard0_mean_inflight_by_coordinator_copy")),
                "setup": setup,
            })

with open(os.path.join(dest, "rows.csv"), "w", newline="") as f:
    w = csv.DictWriter(f, fieldnames=FIELDS)
    w.writeheader()
    w.writerows(rows)

for f in ("analysis.md", "cpu.md"):
    p = os.path.join(rb, f)
    if os.path.isfile(p):
        shutil.copyfile(p, os.path.join(dest, f))

readme = open(os.path.join(rb, "export-readme.md")).read().replace("{STATUS}", status)
open(os.path.join(dest, "README.md"), "w").write(readme)
print(f"{len(rows)} rows, {len(set((r['cache'], r['dir']) for r in rows))} result dirs")
