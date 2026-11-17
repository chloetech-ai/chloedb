"""Single-row writes as Part A's a1_app.py does them: open with a products schema,
create reviews by SQL, load rows, then time single-row inserts, updates and deletes."""
import sys, time, os, shutil, statistics, tempfile
try:
    import chloedb as strata
except ImportError:
    import strata
d = sys.argv[1]; n = int(sys.argv[2]) if len(sys.argv) > 2 else 300
shutil.rmtree(d, ignore_errors=True); os.makedirs(d)
path = os.path.join(d, "app.strata")
db = strata.open(path, schema={"table": "products", "key": "id", "fields": [
    {"name": "id", "type": "keyword"}, {"name": "price", "type": "float"}, {"name": "stock", "type": "int"},
    {"name": "title", "type": "text", "indexed": False}]})
db.sql("CREATE TABLE reviews (id KEYWORD PRIMARY KEY, user_id KEYWORD, product_id KEYWORD, rating INT, "
       "title TEXT OPTIONS(indexed = false), text TEXT OPTIONS(indexed = false), ts INT, month INT, helpful INT, verified INT)")
db.insert([{"id": f"p{i}", "price": 1.0 * i, "stock": i, "title": "x"} for i in range(20000)], table="products")
db.insert([{"id": f"r{i}", "user_id": f"u{i%500}", "product_id": f"p{i%20000}", "rating": 1 + i % 5, "title": "t", "text": "a review", "ts": i, "month": 202601, "helpful": 0, "verified": 1} for i in range(50000)], table="reviews")
def rev(i):
    return {"id": f"new{i}", "user_id": "u1", "product_id": "p1", "rating": 1 + i % 5, "title": "new", "text": "a review written during the benchmark", "ts": 1_800_000_000_000 + i, "month": 202701, "helpful": 0, "verified": 1}
mark = os.environ.get("MARK")
def timeit(label, f):
    if mark: open(mark, "a").write(f"=== {label} start\n")
    ts = []
    for i in range(n):
        t = time.perf_counter(); f(i); ts.append((time.perf_counter() - t) * 1000)
    if mark: open(mark, "a").write(f"=== {label} end\n")
    s = sorted(ts); print(f"{label:18} p50 {s[len(s)//2]:.3f} p90 {s[int(len(s)*.9)]:.3f} p99 {s[int(len(s)*.99)]:.3f} max {s[-1]:.1f} ms  {n / (sum(ts) / 1000):,.0f}/s  slow(>20ms) {sum(t > 20 for t in ts)}")
timeit("insert one", lambda i: db.insert([rev(i)], table="reviews"))
timeit("update price", lambda i: db.sql("UPDATE products SET price = :p, stock = :s WHERE id = :id", {"p": 2.0 * i, "s": i, "id": f"p{i}"}))
timeit("delete by id", lambda i: db.delete(f"new{i}", table="reviews"))
print("version", getattr(strata, "__version__", "?"))
