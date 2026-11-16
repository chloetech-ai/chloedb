#!/usr/bin/env python3
"""Medians, spread and an exact two-sided Mann-Whitney test of each routing
mode against its default, per scenario and client count, from the route
bench's route.json files (one directory per scenario under OUT)."""
import itertools, json, statistics, sys, os

OUT = sys.argv[1]
PREF = ["equal1", "busy1", "cpuhalf1", "cpubusy1", "stall1", "sigstop1", "ryw", "rywnet", "equal", "busy", "cpubusy", "stall", "sigstop", "equal-c8", "slow-c8", "busy-c8", "cpubusy-c8", "equal1-c8", "equal1-c16"]
_have = [d for d in os.listdir(OUT) if os.path.isfile(os.path.join(OUT, d, "route.json"))]
ORDER = [d for d in PREF if d in _have] + sorted(d for d in _have if d not in PREF)
PAIRS = [  # (mode, against)
    ("least-cost", "round-robin"),
    ("round-robin adaptive", "round-robin"),
    ("least-cost adaptive", "least-cost"),
    ("least-cost no-load", "least-cost"),
    ("least-cost prefer", "least-cost"),
    ("least-cost prefer", "round-robin"),
    ("least-cost fixed", "least-cost"),
    ("least-cost fixed", "round-robin"),
]

def mwu_p(a, b):
    """Exact two-sided p of the Mann-Whitney U over every split of the pooled values."""
    pool = a + b
    n = len(a)
    def u(x, y):
        return sum((1.0 if xi > yi else 0.5 if xi == yi else 0.0) for xi in x for yi in y)
    mean = len(a) * len(b) / 2
    obs = abs(u(a, b) - mean)
    idx = range(len(pool))
    tot = hit = 0
    for c in itertools.combinations(idx, n):
        x = [pool[i] for i in c]
        y = [pool[i] for i in idx if i not in c]
        tot += 1
        hit += abs(u(x, y) - mean) >= obs - 1e-9
    return hit / tot

def load(sc):
    p = os.path.join(OUT, sc, "route.json")
    if not os.path.exists(p):
        return None
    return json.load(open(p))

METRICS = [("qps", "qps", +1), ("p50", "p50_ms", -1), ("p99", "p99_ms", -1), ("p99.9", "p999_ms", -1), ("raw p99", "raw_p99_ms", -1)]

def fmt(v):
    if v is None:
        return "-"
    return f"{v:,.0f}" if v >= 100 else f"{v:.2f}" if v < 10 else f"{v:.1f}"

def main():
    verdicts = []
    for sc in ORDER:
        d = load(sc)
        if not d:
            continue
        runs = d["runs"]
        groups = {}
        for r in runs:
            groups.setdefault((r["clients"], r["config"]), []).append(r)
        print(f"\n### {sc}\n\n{d['machine'].get('setup')}\n")
        print("| clients | config | rounds | qps | p50 ms | p99 ms | p99.9 ms | read-after-write p99 ms | hedges | errors |")
        print("|---|---|---|---|---|---|---|---|---|---|")
        for (c, cfg), rs in sorted(groups.items(), key=lambda kv: (kv[0][0], list(dict.fromkeys(r['config'] for r in runs)).index(kv[0][1]))):
            def cell(key, src="client"):
                vals = [r[src][key] if src else r[key] for r in rs]
                vals = [v for v in vals if v is not None]
                if not vals:
                    return "-"
                return f"{fmt(statistics.median(vals))} ({fmt(min(vals))}–{fmt(max(vals))})"
            print(f"| {c} | {cfg} | {len(rs)} | {cell('qps')} | {cell('p50_ms')} | {cell('p99_ms')} | {cell('p999_ms')} | {cell('raw_p99_ms')} | {cell('hedges', None)} | {cell('errors')} |")
        for mode, base in PAIRS:
            for c in sorted({k[0] for k in groups}):
                if (c, mode) not in groups or (c, base) not in groups:
                    continue
                a, b = groups[(c, mode)], groups[(c, base)]
                for label, key, sign in METRICS:
                    va = [r["client"][key] for r in a if r["client"][key] is not None]
                    vb = [r["client"][key] for r in b if r["client"][key] is not None]
                    if len(va) < 3 or len(vb) < 3:
                        continue
                    ma, mb = statistics.median(va), statistics.median(vb)
                    p = mwu_p(va, vb)
                    ratio = ma / mb if mb else float("nan")
                    better = (ma - mb) * sign > 0
                    v = ("better" if better else "worse") if p < 0.05 else "no detectable difference"
                    verdicts.append(dict(scenario=sc, clients=c, mode=mode, against=base, metric=label, mode_median=ma, base_median=mb, ratio=ratio, p=p, verdict=v, mode_range=[min(va), max(va)], base_range=[min(vb), max(vb)]))
    json.dump(verdicts, open(os.path.join(OUT, "verdicts.json"), "w"), indent=1)
    print("\n## Mode against its default (medians, exact Mann-Whitney p)\n")
    print("| scenario | clients | mode | against | metric | mode | default | ratio | p | verdict |")
    print("|---|---|---|---|---|---|---|---|---|---|")
    for v in verdicts:
        print(f"| {v['scenario']} | {v['clients']} | {v['mode']} | {v['against']} | {v['metric']} | {fmt(v['mode_median'])} | {fmt(v['base_median'])} | {v['ratio']:.2f} | {v['p']:.3f} | {v['verdict']} |")

main()
