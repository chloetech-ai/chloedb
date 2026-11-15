"""Predictions beyond the measured setups, from the fitted model: each case a
dict of model inputs changed from the 1-shard copies-alike base."""
import json, math, random, statistics as st, sys
from multiprocessing import Pool
import sim, scen

def case(name, x, clients, config, shards=1, copies=3, slow=None, slow_speed=None, cores_x=1.0,
         extra_ms=None, pause_ms=None, hetero=0.0, seconds=None, fix=False):
    return dict(name=name, x=x, clients=clients, config=config, shards=shards, copies=copies,
                slow_speed=slow_speed, cores_x=cores_x, extra_ms=extra_ms, pause_ms=pause_ms,
                hetero=hetero, seconds=seconds, fix=fix)

def build(c, seed):
    p = scen.params("equal1", c["clients"], c["config"], seed, shards=c["shards"], copies=c["copies"],
                    seconds=c["seconds"])
    S, R = c["shards"], c["copies"]
    rng = random.Random(seed * 101 + 7)
    p["cores"] = [[scen.FIT["cores"] * c["cores_x"]] * R for _ in range(S)]
    # Copies alike as measured: node c01 ~8% faster; with more nodes, each
    # node's speed drawn from that spread (sd 4%) unless hetero is 0.
    p["speed"] = [[math.exp(rng.gauss(0, c["hetero"])) if c["hetero"] else 1.0 for _ in range(R)] for _ in range(S)]
    if not c["hetero"]:
        p["speed"][0][1 % R] = scen.FIT["h1"] if R > 1 else 1.0
    if c["slow_speed"] is not None:
        p["speed"][0][0] = c["slow_speed"]
    if c["extra_ms"]:
        e = c["extra_ms"]
        p["extra"] = {(0, 0): (lambda now, e=e: e)}
    if c["pause_ms"]:
        p["pause"] = {(0, 0): (1000.0, c["pause_ms"], rng.uniform(0, 1000))}
    p["slots"] = [[64] * R for _ in range(S)]
    if c.get("fix"):
        p["fix_early"] = p["fix_all_avoided"] = True
    return p

def one(a):
    c, seed = a
    r = sim.run(build(c, seed), seed)
    return {k: r[k] for k in ("qps", "p50", "p90", "p99", "p999", "hedges")}

def fix_cases():
    out = []
    for f in (1.0, 0.8, 0.6, 0.4, 0.25, 0.1):
        for cl in (16, 64):
            out.append(case("severity", f, cl, "least-cost", slow_speed=f, fix=True))
            out.append(case("severity", f, cl, "round-robin", slow_speed=f, fix=True))
    for cl in (1, 4, 16, 32, 64, 128, 256):
        out.append(case("clients-half", cl, cl, "least-cost", slow_speed=0.5, fix=True))
    for k in (1.0, 2.0, 4.0):
        out.append(case("cores", k, 256, "least-cost", cores_x=k, slow_speed=0.5, fix=True))
    for R in (2, 3, 5, 8):
        out.append(case("copies", R, round(64 * R / 3), "least-cost", copies=R, slow_speed=0.4, fix=True))
    for S in (1, 3, 10, 20):
        out.append(case("shards", S, 64, "least-cost", shards=S, slow_speed=0.4, hetero=0.04, seconds=2500.0, fix=True))
    return out

def cases():
    out = []
    RL = ("round-robin", "least-cost")
    # A. How slow copy 0 is: its speed from alike (1) to a tenth.
    for f in (1.0, 0.8, 0.6, 0.4, 0.25, 0.1):
        for cl in (16, 64):
            for cfg in RL:
                out.append(case("severity", f, cl, cfg, slow_speed=f))
    # B. Copies per shard, one of them at 40% speed, load per copy as at 64 clients over 3.
    for R in (2, 3, 5, 8):
        for cfg in RL:
            out.append(case("copies", R, round(64 * R / 3), cfg, copies=R, slow_speed=0.4))
            out.append(case("copies-alike", R, round(64 * R / 3), cfg, copies=R))
    # C. Shards in a query (fan-out), one copy of one shard at 40%, nodes drawn 4% apart.
    for S in (1, 3, 10, 20):
        for cfg in RL:
            out.append(case("shards", S, 64, cfg, shards=S, slow_speed=0.4, hetero=0.04, seconds=2500.0))
    # D. Clients, copies alike and with copy 0 at half speed.
    for cl in (1, 4, 16, 32, 64, 128, 256):
        for cfg in RL:
            out.append(case("clients-alike", cl, cl, cfg))
            out.append(case("clients-half", cl, cl, cfg, slow_speed=0.5))
    # E. Bigger copies: 2 and 4 times the vCPUs (c-4, c-8), copy 0 at half speed.
    for k in (1.0, 2.0, 4.0):
        for cl in (64, 256):
            for cfg in RL:
                out.append(case("cores", k, cl, cfg, cores_x=k, slow_speed=0.5))
    # F. Copy 0 farther away: ms added to each of its answers.
    for e in (0.0, 1.0, 5.0, 20.0, 50.0):
        for cl in (16, 64):
            for cfg in RL:
                out.append(case("distance", e, cl, cfg, extra_ms=e))
    # G. Copy 0 frozen (SIGSTOP) for longer each second, with and without adaptive hedges.
    for ms in (20.0, 60.0, 200.0, 500.0):
        for cl in (1, 16):
            for cfg in ("round-robin", "round-robin adaptive", "least-cost", "least-cost adaptive"):
                out.append(case("freeze", ms, cl, cfg, pause_ms=ms))
    return out

if __name__ == "__main__":
    cs = fix_cases() if len(sys.argv) > 2 and sys.argv[2] == "fix" else cases()
    jobs = [(c, s) for c in cs for s in (1, 2, 3)]
    with Pool(4) as pool:
        res = pool.map(one, jobs, chunksize=1)
    rows = []
    for i, c in enumerate(cs):
        rs = res[3 * i:3 * i + 3]
        m = {k: st.median(r[k] for r in rs) for k in rs[0]}
        m |= {k + "_lo": min(r[k] for r in rs) for k in ("qps", "p50", "p99", "p999")}
        m |= {k + "_hi": max(r[k] for r in rs) for k in ("qps", "p50", "p99", "p999")}
        rows.append(c | m)
    json.dump(rows, open(sys.argv[1] if len(sys.argv) > 1 else "predictions.json", "w"), indent=1)
    print(len(rows), "cases")
