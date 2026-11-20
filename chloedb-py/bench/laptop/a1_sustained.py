"""A1's 1,000-rows-per-transaction insert (a1_app.py's insert step), run
for minutes instead of 20,000 rows, so that it pays ChloeDB's memtable
flushes and segment merges (SQLite's WAL checkpoints): rows a second over
each 15 s window and in all.
    python3 a1_sustained.py WORK SYSTEM [SECONDS]"""
import json, random, sys, time, shutil
from pathlib import Path
try:
    import chloedb
except ImportError:
    chloedb = None
if chloedb: sys.modules["strata"] = chloedb
sys.path.insert(0, str(Path(__file__).parent))
import a1_app

work, system = Path(sys.argv[1]), sys.argv[2]
seconds = float(sys.argv[3]) if len(sys.argv) > 3 else 180.0
shutil.rmtree(work, ignore_errors=True)
work.mkdir(parents=True)
s = a1_app.Strata(work / "a1.strata") if system == "strata" else a1_app.Sqlite(work / "a1.db", system.split("-")[1].upper())
s.create()
rng = random.Random(1)
words = [f"w{i}" for i in range(5000)]
def review(i):
    ts = 1_700_000_000_000 + i * 1000
    return {"id": f"r{i}", "user_id": f"u{rng.randrange(100_000)}", "product_id": f"p{rng.randrange(137_260)}", "rating": rng.randrange(1, 6),
            "title": " ".join(rng.choices(words, k=5)), "text": " ".join(rng.choices(words, k=40)), "ts": ts, "month": a1_app.month_of(ts),
            "helpful": rng.randrange(20), "verified": rng.randrange(2)}
n, t0, windows, w0, wn = 0, time.perf_counter(), [], time.perf_counter(), 0
while time.perf_counter() - t0 < seconds:
    rows = [review(n + k) for k in range(1000)]
    s.insert("reviews", rows)
    n += 1000
    if time.perf_counter() - w0 >= 15:
        windows.append(round((n - wn) / (time.perf_counter() - w0)))
        w0, wn = time.perf_counter(), n
total = time.perf_counter() - t0
count = s.q("SELECT COUNT(*) FROM reviews")[0][0]
print(json.dumps({"system": system, "rows": n, "seconds": round(total, 1), "rows_per_s": round(n / total), "windows_15s": windows, "counted": count == n,
                  "bytes": sum(p.stat().st_size for p in work.rglob("*") if p.is_file())}))
