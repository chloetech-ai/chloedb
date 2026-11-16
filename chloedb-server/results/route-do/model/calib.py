"""Fits the model's free parameters to the copies-alike runs (equal1) under
round-robin only; least-cost and every other scenario stay out of the fit."""
import json, math, sys, numpy as np
import sim, data
M = data.load("equal1")
W = {"qps": 2.0, "p50": 1.0, "p90": 1.0, "p99": 1.0, "p999": 0.5}
NAMES = ["net_med", "net_sigma", "d_med", "d_sigma", "cores", "n_half", "h1"]
def params(x, clients, route="round-robin", adaptive=False, seconds=None):
    seconds = seconds or (8000.0 if clients == 1 else 3000.0)
    net_med, net_sigma, d_med, d_sigma, cores, n_half, h1 = x
    return dict(shards=1, copies=3, cores=[[cores] * 3], speed=[[1.0, h1, 1.0]], slots=[[64] * 3],
                route=route, adaptive=adaptive, clients=clients, warmup=1500.0, seconds=seconds,
                net_med=net_med, net_sigma=net_sigma, d_med=d_med, d_sigma=d_sigma, n_half=n_half)
def _one(a):
    x, c, s = a
    r = sim.run(params(x, c), s)
    m = M[(c, "round-robin")]
    return sum(w * math.log(r[k] / m[k][0]) ** 2 for k, w in W.items())
POOL = None
def loss(x, seeds=(1, 2)):
    global POOL
    if min(x[:5]) <= 0 or x[5] < 0 or x[6] <= 0 or x[1] > 2 or x[3] > 3:
        return 1e9
    if POOL is None:
        from multiprocessing import Pool
        POOL = Pool(4)
    return sum(POOL.map(_one, [(list(x), c, s) for c in (1, 16, 64) for s in seeds])) / len(seeds)
def nelder_mead(f, x0, step, iters=150):
    n = len(x0)
    pts = [np.array(x0, float)] + [np.array(x0, float) + np.eye(n)[i] * step[i] for i in range(n)]
    vals = [f(p) for p in pts]
    for it in range(iters):
        o = np.argsort(vals); pts = [pts[i] for i in o]; vals = [vals[i] for i in o]
        c = sum(pts[:-1]) / n
        xr = c + (c - pts[-1]); fr = f(xr)
        if fr < vals[0]:
            xe = c + 2 * (c - pts[-1]); fe = f(xe)
            pts[-1], vals[-1] = (xe, fe) if fe < fr else (xr, fr)
        elif fr < vals[-2]:
            pts[-1], vals[-1] = xr, fr
        else:
            xc = c + 0.5 * (pts[-1] - c); fc = f(xc)
            if fc < vals[-1]:
                pts[-1], vals[-1] = xc, fc
            else:
                pts = [pts[0]] + [pts[0] + 0.5 * (p - pts[0]) for p in pts[1:]]
                vals = [vals[0]] + [f(p) for p in pts[1:]]
        if it % 10 == 0:
            print(it, round(vals[0], 4), dict(zip(NAMES, np.round(pts[0], 3))), flush=True)
    i = int(np.argmin(vals)); return pts[i], vals[i]
if __name__ == "__main__":
    x0 = [1.95, 0.25, 0.6, 0.45, 1.9, 1.0, 1.05]
    best, v = nelder_mead(loss, x0, [0.2, 0.1, 0.15, 0.15, 0.3, 0.8, 0.05], iters=int(sys.argv[1]) if len(sys.argv) > 1 else 120)
    json.dump(dict(zip(NAMES, map(float, best))) | {"loss": v}, open("fit.json", "w"), indent=1)
    print("best", v, dict(zip(NAMES, np.round(best, 3))))
