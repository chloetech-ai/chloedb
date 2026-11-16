"""The same single-row writes on SQLite as A1 opens it (WAL, synchronous FULL or NORMAL)."""
import sys, time, os, shutil, sqlite3
d = sys.argv[1]; n = int(sys.argv[2]) if len(sys.argv) > 2 else 300; sync = sys.argv[3] if len(sys.argv) > 3 else "FULL"
shutil.rmtree(d, ignore_errors=True); os.makedirs(d)
c = sqlite3.connect(os.path.join(d, "app.db"), isolation_level=None)
c.execute("PRAGMA journal_mode = WAL"); c.execute(f"PRAGMA synchronous = {sync}")
c.execute("PRAGMA cache_size = -262144"); c.execute("PRAGMA mmap_size = 1073741824")
c.execute("CREATE TABLE products (id TEXT PRIMARY KEY, price REAL, stock INT, title TEXT)")
c.execute("CREATE TABLE reviews (id TEXT PRIMARY KEY, user_id TEXT, product_id TEXT, rating INT, title TEXT, text TEXT, ts INT, month INT, helpful INT, verified INT)")
c.execute("BEGIN"); c.executemany("INSERT INTO products VALUES (?,?,?,?)", [(f"p{i}", 1.0 * i, i, "x") for i in range(20000)])
c.executemany("INSERT INTO reviews VALUES (?,?,?,?,?,?,?,?,?,?)", [(f"r{i}", f"u{i%500}", f"p{i%20000}", 1 + i % 5, "t", "a review", i, 202601, 0, 1) for i in range(50000)]); c.execute("COMMIT")
mark = os.environ.get("MARK")
def timeit(label, f):
    if mark: open(mark, "a").write(f"=== {label} start\n")
    ts = []
    for i in range(n):
        t = time.perf_counter(); f(i); ts.append((time.perf_counter() - t) * 1000)
    if mark: open(mark, "a").write(f"=== {label} end\n")
    s = sorted(ts); print(f"{label:18} p50 {s[len(s)//2]:.3f} p90 {s[int(len(s)*.9)]:.3f} p99 {s[int(len(s)*.99)]:.3f} max {s[-1]:.1f} ms  {n / (sum(ts) / 1000):,.0f}/s  slow(>20ms) {sum(t > 20 for t in ts)}")
timeit("insert one", lambda i: c.execute("INSERT INTO reviews VALUES (?,?,?,?,?,?,?,?,?,?)", (f"new{i}", "u1", "p1", 1 + i % 5, "new", "a review written during the benchmark", 1_800_000_000_000 + i, 202701, 0, 1)))
timeit("update price", lambda i: c.execute("UPDATE products SET price = ?, stock = ? WHERE id = ?", (2.0 * i, i, f"p{i}")))
timeit("delete by id", lambda i: c.execute("DELETE FROM reviews WHERE id = ?", (f"new{i}",)))
print("sqlite", sqlite3.sqlite_version, sync)
