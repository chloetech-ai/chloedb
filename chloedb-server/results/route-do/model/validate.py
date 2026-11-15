"""Predicts every measured scenario, clients and config with the fitted model
(3 seeds, median) and sets it beside the measured medians."""
import json, os, statistics as st, sys
from multiprocessing import Pool
import sim, scen, data
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "cache-off")
def one(args):
    sc, c, cfg, seed = args
    r = sim.run(scen.params(sc, c, cfg, seed), seed)
    return (sc, c, cfg, seed), {k: r[k] for k in ("qps", "p50", "p90", "p99", "p999", "hedges")} | {"answers": r["answers"][0]}
if __name__ == "__main__":
    scs = sys.argv[1].split(",")
    jobs = []
    for sc in scs:
        cfgs = sorted({k[1] for k in data.load(sc)}) if os.path.exists(os.path.join(OUT, sc, "route.json")) else list(scen.CONFIGS)
        for c in (1, 16, 64):
            for cfg in cfgs:
                for seed in (1, 2, 3):
                    jobs.append((sc, c, cfg, seed))
    with Pool(4) as pool:
        res = dict(pool.map(one, jobs))
    pred = {}
    for (sc, c, cfg, seed), r in res.items():
        pred.setdefault((sc, c, cfg), []).append(r)
    rows = []
    for (sc, c, cfg), rs in sorted(pred.items()):
        m = {k: st.median(r[k] for r in rs) for k in ("qps", "p50", "p90", "p99", "p999", "hedges")}
        m |= {k + "_lo": min(r[k] for r in rs) for k in ("qps", "p50", "p99")} | {k + "_hi": max(r[k] for r in rs) for k in ("qps", "p50", "p99")}
        meas = None
        if os.path.exists(os.path.join(OUT, sc, "route.json")):
            mm = data.load(sc).get((c, cfg))
            if mm:
                meas = {k: mm[k][0] for k in ("qps", "p50", "p90", "p99", "p999")} | {"hedges": mm["hedges"]}
        rows.append({"scenario": sc, "clients": c, "config": cfg, "pred": m, "meas": meas})
    name = sys.argv[2] if len(sys.argv) > 2 else "pred.json"
    json.dump(rows, open(name, "w"), indent=1)
    for r in rows:
        p, m = r["pred"], r["meas"]
        if m:
            err = {k: f"{(p[k] / m[k] - 1) * 100:+.0f}%" for k in ("qps", "p50", "p99")}
            print(r["scenario"], r["clients"], r["config"], {k: round(p[k], 2) for k in ("qps", "p50", "p99")}, "meas", {k: round(m[k], 2) for k in ("qps", "p50", "p99")}, err)
        else:
            print(r["scenario"], r["clients"], r["config"], {k: round(p[k], 2) for k in ("qps", "p50", "p99", "p999")})
