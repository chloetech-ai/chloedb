"""Measured medians (and ranges) per scenario, clients and config, from route.json."""
import json, os, statistics as st
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "cache-off")
KEYS = ("qps", "p50_ms", "p90_ms", "p99_ms", "p999_ms")
def load(name):
    d = json.load(open(os.path.join(OUT, name, "route.json")))
    g = {}
    for r in d["runs"]:
        g.setdefault((r["clients"], r["config"]), []).append(r)
    res = {}
    for k, rs in g.items():
        m = {}
        for key in KEYS:
            v = [r["client"][key] for r in rs]
            m[key.replace("_ms", "")] = (st.median(v), min(v), max(v))
        m["hedges"] = st.median(r["hedges"] for r in rs)
        m["answers"] = [st.median(r["shard0_answers_by_copy"][0][c] for r in rs) for c in range(len(rs[0]["shard0_answers_by_copy"][0]))]
        m["rtt"] = [st.median(r["shard0_observed_by_copy"][c]["query_rtt_ms"]["slow"] for r in rs) for c in range(len(rs[0]["shard0_observed_by_copy"]))]
        m["srv"] = [st.median(r["shard0_observed_by_copy"][c]["query_rtt_ms"]["slow"] - r["shard0_observed_by_copy"][c]["overhead_ms"]["slow"] for r in rs) for c in range(len(rs[0]["shard0_observed_by_copy"]))]
        m["rounds"] = len(rs)
        res[k] = m
    return res
if __name__ == "__main__":
    import sys
    for k, m in sorted(load(sys.argv[1]).items()):
        print(k, {x: round(m[x][0], 2) for x in ("qps", "p50", "p90", "p99", "p999")}, "hedges", m["hedges"])
