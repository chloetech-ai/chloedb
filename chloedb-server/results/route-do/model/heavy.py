"""Pre-registered predictions for a measured check of the cool-off finding:
copy 0 beside k CPU-bound threads (stress-ng --cpu k), 1 shard, 64 clients,
round-robin, least-cost, and least-cost with each fix. Prints JSON."""
import json, statistics, sys
from multiprocessing import Pool
import scen, sim

def one(a):
    k, cl, cfg, fix, seed = a
    p = scen.params("cpubusy1", cl, cfg, seed)
    p["stress"] = {(0, 0): k}
    if fix in ("early", "both"):
        p["fix_early"] = True
    if fix in ("all", "both"):
        p["fix_all_avoided"] = True
    r = sim.run(p, seed)
    return (k, cl, cfg, fix), {m: r[m] for m in ("qps", "p50", "p99", "p999", "hedges")}

if __name__ == "__main__":
    ks = [int(x) for x in sys.argv[1].split(",")]
    cls = [int(x) for x in sys.argv[2].split(",")]
    cases = []
    for k in ks:
        for cl in cls:
            cases += [(k, cl, "round-robin", "none"), (k, cl, "least-cost", "none"),
                      (k, cl, "least-cost", "early"), (k, cl, "least-cost", "all"), (k, cl, "least-cost", "both")]
    jobs = [c + (s,) for c in cases for s in (1, 2, 3)]
    with Pool(4) as pool:
        res = pool.map(one, jobs, chunksize=1)
    by = {}
    for key, r in res:
        by.setdefault(key, []).append(r)
    out = []
    for key in cases:
        rs = by[key]
        med = {m: statistics.median(r[m] for r in rs) for m in rs[0]}
        out.append(dict(stress=key[0], clients=key[1], config=key[2], fix=key[3], **med))
        print(key, {m: round(v, 1) for m, v in med.items()}, flush=True)
    json.dump(out, open(sys.argv[3], "w"), indent=1)
