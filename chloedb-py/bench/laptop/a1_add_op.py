"""counter + 1 through db.write's add op (no SQL), as a1_updates.py times the SQL one:
    python3 a1_add_op.py WORK"""
import json, sys, shutil
from pathlib import Path
import chloedb
sys.modules["strata"] = chloedb
sys.path.insert(0, str(Path(__file__).parent))
import a1_app
from common import timed_passes
work = Path(sys.argv[1]); shutil.rmtree(work, ignore_errors=True); work.mkdir(parents=True)
s = a1_app.Strata(work / "a1.strata"); s.create()
s.insert("products", [{"id": f"p{i}", "title": "t", "brand": "b", "platform": "web", "category": "c", "price": 9.0, "rating": 4.1, "rating_count": 0, "stock": 1, "year": 2020, "rated": "yes", "description": "d", "attrs": {}} for i in range(137_260)])
s.optimize()
op = [{"op": "add", "table": "products", "key": "p7", "field": "rating_count", "value": 1}]
cn = timed_passes(lambda _: s.db.write(op), [None], 4.0)
print(json.dumps({"write_add_per_s": 1e3 / cn["mean_ms"], "p50_ms": cn["p50_ms"], "n": cn["n"], "count": s.q("SELECT rating_count FROM products WHERE id = 'p7'")[0][0]}))
