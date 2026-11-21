"""Why did D on 4B not come back exactly after DELETE? Inspect the datastore it left: row counts,
rows left from the deleted docs, and whether HNSW answers match exact search, before and after optimize."""
import json, os, sys, numpy as np, chloedb
db = chloedb.open(f"{os.environ['S']}/knn.chloedb", sync=False); res = {}
res["count"] = db.query("SELECT COUNT(*) FROM knn").rows[0][0]
res["rest_left"] = db.query("SELECT COUNT(*) FROM knn WHERE doc LIKE 'rest/%'").rows[0][0]
res["docs"] = db.query("SELECT doc, COUNT(*) FROM knn GROUP BY doc ORDER BY doc LIMIT 100").rows
ids, keys = [], []
for a in range(0, 200000, 5000):
    rows = db.query("SELECT id, key FROM knn WHERE id >= :a AND id < :b", a=a, b=a + 5000).rows
    ids += [r[0] for r in rows]; keys += [np.array(r[1], dtype=np.float32) for r in rows]
X = np.stack(keys); IDS = np.array(ids); rng = np.random.default_rng(0)
Q = X[rng.choice(len(X), 500, replace=False)] + rng.normal(0, 0.3 * X.std(), (500, X.shape[1])).astype(np.float32)
truth = [set(IDS[t].tolist()) for t in np.argsort(-(Q @ X.T), 1)[:, :32]]
def run():
    got = [[r[0] for r in db.query("SELECT id FROM knn ORDER BY similarity(key, :v) DESC LIMIT 32", v=q).rows] for q in Q]
    return got, float(np.mean([len(truth[i] & set(g)) / 32 for i, g in enumerate(got)]))
g0, res["recall_as_left"] = run(); db.flush(); db.optimize(); g1, res["recall_after_optimize"] = run()
res["identical_after_optimize"] = sum(a == b for a, b in zip(g0, g1)); res["rows_scanned"] = len(X)
json.dump(res, open(sys.argv[1], "w"), indent=1, default=str); print(json.dumps(res, default=str)[:2000])
