"""fm_time.py URL QUERIES OUT.json: chloedb-hn/eval/plain.py's two substring statements, the same way
(each asked twice, a nonce parameter of its own, server time took_ms)."""
import sys, json, http.client, urllib.parse, random, time, math, statistics
url, qpath, out = sys.argv[1], sys.argv[2], sys.argv[3]
u = urllib.parse.urlparse(url)
STMTS = {
    'fm': "SELECT id, title, points FROM hn_stories WHERE title ILIKE :p LIMIT 20",
    'fm_count': "SELECT COUNT(*) AS n FROM hn_stories WHERE title ILIKE :p",
}
c = http.client.HTTPConnection(u.hostname, u.port, timeout=120)
def ask(sql, params):
    c.request('POST', '/v1/query', body=json.dumps({'sql': sql, 'params': params, 'timeout_ms': 30000}), headers={'content-type': 'application/json'})
    r = c.getresponse(); return r.status, json.loads(r.read())
def pct(xs, p):
    xs = sorted(xs); k = (len(xs) - 1) * p; f, cc = math.floor(k), math.ceil(k)
    return round(xs[f] + (xs[cc] - xs[f]) * (k - f), 1)
QS = [l.strip() for l in open(qpath) if l.strip()]
rows = []
for q in QS:
    rec = {'q': q}
    for run in (1, 2):
        for name, sql in STMTS.items():
            params = {'q': q, 'p': '%' + q.replace('%', '_') + '%', 'nonce': random.random()}
            st, j = ask(sql, params)
            rec[f'{name}_{run}'] = j.get('took_ms') if st == 200 else None
            if st != 200: rec[f'{name}_{run}_err'] = str(j)[:200]
            if name == 'fm_count' and st == 200: rec['count'] = j['rows'][0][0]
            if name == 'fm' and st == 200: rec['ids'] = [r[0] for r in j['rows']]
            time.sleep(0.05)
    rows.append(rec)
    print(q, {k: v for k, v in rec.items() if k not in ('q', 'ids')}, file=sys.stderr, flush=True)
keys = [f'{n}_{r}' for n in STMTS for r in (1, 2)]
st = {k: {'p50': pct([r[k] for r in rows], .5), 'p90': pct([r[k] for r in rows], .9), 'p99': pct([r[k] for r in rows], .99), 'max': max(r[k] for r in rows), 'mean': round(statistics.mean(r[k] for r in rows), 1)} for k in keys}
json.dump({'when': time.strftime('%Y-%m-%d %H:%M:%S UTC', time.gmtime()), 'statements': STMTS, 'stats': st, 'rows': rows}, open(out, 'w'), indent=1)
print(json.dumps(st, indent=1))
