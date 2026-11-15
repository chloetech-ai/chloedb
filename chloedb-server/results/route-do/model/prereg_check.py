"""Blind predictions (prereg/pred-*.json) against what was measured since."""
import glob, json, os, sys, data
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "cache-off")
f = sorted(glob.glob(os.path.join(os.path.dirname(os.path.abspath(__file__)), "prereg", "pred-*.json")))[0]
rows = json.load(open(f))
res = []
for r in rows:
    sc = r["scenario"]
    if not os.path.exists(os.path.join(OUT, sc, "route.json")):
        continue
    m = data.load(sc).get((r["clients"], r["config"]))
    if not m:
        continue
    p = r["pred"]
    res.append({"scenario": sc, "clients": r["clients"], "config": r["config"],
                **{f"pred_{k}": p[k] for k in ("qps", "p50", "p99", "p999")},
                **{f"meas_{k}": m[k][0] for k in ("qps", "p50", "p99", "p999")}})
    if len(sys.argv) < 2:
        print(sc, r["clients"], r["config"], " ".join(f"{k} {p[k]:.1f}/{m[k][0]:.1f} ({(p[k]/m[k][0]-1)*100:+.0f}%)" for k in ("qps", "p50", "p99", "p999")))
if len(sys.argv) > 1:
    json.dump(res, open(sys.argv[1], "w"), indent=1)
