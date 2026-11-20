"""A1's update step (a1_app.py: the same classes, statements and
timed_passes) on 137,260 made-up products: ChloeDB vs SQLite FULL/NORMAL.
    python3 a1_updates.py WORK SYSTEM [SECONDS]
SYSTEM: strata | sqlite-full | sqlite-normal"""
import json, random, sys, time
from pathlib import Path
try:
    import chloedb
except ImportError:
    chloedb = None
if chloedb: sys.modules["strata"] = chloedb
sys.path.insert(0, str(Path(__file__).parent))
import a1_app
from common import timed_passes

work, system = Path(sys.argv[1]), sys.argv[2]
seconds = float(sys.argv[3]) if len(sys.argv) > 3 else 10.0
import shutil
shutil.rmtree(work, ignore_errors=True)
work.mkdir(parents=True)
s = a1_app.Strata(work / "a1.strata") if system == "strata" else a1_app.Sqlite(work / "a1.db", system.split("-")[1].upper())
s.create()
rng = random.Random(1)
N = 137_260
pids = [f"p{i}" for i in range(N)]
t_load = time.time()
for start in range(0, N, 10_000):
    s.insert("products", [{"id": pids[i], "title": f"product {i}", "brand": f"b{i % 500}", "platform": "web", "category": f"c{i % 40}",
                           "price": rng.uniform(5, 80), "rating": 4.1, "rating_count": 0, "stock": rng.randrange(200), "year": 2020,
                           "rated": "yes", "description": "a description of some words", "attrs": {"color": "red"}} for i in range(start, min(N, start + 10_000))])
t1 = time.time()
s.optimize()
print(f'loaded in {t1 - t_load:.1f} s, optimized in {time.time() - t1:.1f} s', file=sys.stderr)
pids = rng.sample(pids, 2000)  # as a1_app.py samples them
up = timed_passes(lambda k: s.x(a1_app.UPDATE_PRICE, {"p": round(rng.uniform(5, 80), 2), "s": rng.randrange(0, 200), "id": k}), pids, seconds)
cnt_id = pids[1]
before = s.q("SELECT rating_count FROM products WHERE id = :id", {"id": cnt_id})[0][0]
cn = timed_passes(lambda _: s.x(a1_app.UPDATE_COUNTER, {"id": cnt_id}), [None], seconds)
after = s.q("SELECT rating_count FROM products WHERE id = :id", {"id": cnt_id})[0][0]
out = {"system": system, "version": getattr(chloedb, "__version__", "") if system == "strata" else s.version,
       "price_stock_per_s": 1e3 / up["mean_ms"], "price_p50_ms": up["p50_ms"], "price_p99_ms": up["p99_ms"],
       "counter_per_s": 1e3 / cn["mean_ms"], "counter_p50_ms": cn["p50_ms"], "counter_p99_ms": cn["p99_ms"], "counted_right": after - before == cn["n"]}
print(json.dumps(out))
