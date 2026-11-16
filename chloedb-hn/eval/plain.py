#!/usr/bin/env python3
"""The simplest searches ChloeDB can run, against hn.algolia.com, on the same
queries: BM25 of the titles with every word required (top 20, and the
count), and the titles' substring index (FM-index) with no ranking at all
(`title ILIKE '%query%'`, the first 20, and the count). Server time as each
reports it; the app's Fast mode is asked too, from end to end.

Each statement is asked twice. Every request carries a parameter of its own
that the statement does not use, so the server's answer cache never answers
it; the second ask still finds the filter cache warm, so compare first asks.

    CHLOEDB_KEY=... python3 eval/plain.py --strata http://127.0.0.1:7700 \
      --app https://hn.chloetech.ai --queries eval/results/queries-2026-11-16.txt \
      --out eval/results/plain-2026-11-16.json
"""
import argparse, http.client, json, math, os, random, statistics, sys, time, urllib.parse

KEY = os.environ.get("CHLOEDB_KEY", "")

STMTS = {
    'bm25': "SELECT id, title, points FROM hn_stories WHERE MATCH(title, :q, min => '100%') ORDER BY bm25(title, :q) DESC LIMIT 20",
    'bm25_count': "SELECT COUNT(*) AS n FROM hn_stories WHERE MATCH(title, :q, min => '100%')",
    'fm': "SELECT id, title, points FROM hn_stories WHERE title ILIKE :p LIMIT 20",
    'fm_count': "SELECT COUNT(*) AS n FROM hn_stories WHERE title ILIKE :p",
}


class Conn:
    """One kept-alive connection to a host."""

    def __init__(self, https, host, port=None):
        self.https, self.host, self.port, self.c = https, host, port, None

    def req(self, method, path, body=None, hdr=None):
        for attempt in range(3):
            if self.c is None:
                cls = http.client.HTTPSConnection if self.https else http.client.HTTPConnection
                self.c = cls(self.host, self.port, timeout=120)
            try:
                t0 = time.perf_counter()
                self.c.request(method, path, body=body, headers=hdr or {})
                r = self.c.getresponse()
                b = r.read()
                return r.status, json.loads(b), (time.perf_counter() - t0) * 1000
            except (OSError, http.client.HTTPException, ValueError):
                self.c = None
                if attempt == 2:
                    raise
                time.sleep(1)


def pct(xs, p):
    xs = sorted(x for x in xs if x is not None)
    if not xs:
        return None
    k = (len(xs) - 1) * p
    f, c = math.floor(k), math.ceil(k)
    return round(xs[f] + (xs[c] - xs[f]) * (k - f), 1)


def stats(xs):
    xs = [x for x in xs if x is not None]
    return {'n': len(xs), 'p50': pct(xs, .5), 'p90': pct(xs, .9), 'p99': pct(xs, .99), 'mean': round(statistics.mean(xs), 1) if xs else None}


def conn(url):
    u = urllib.parse.urlparse(url)
    return Conn(u.scheme == 'https', u.hostname, u.port)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--strata', default='http://127.0.0.1:7700', help="the cluster's coordinator, for the statements")
    ap.add_argument('--app', default='http://127.0.0.1:7850', help='the app, for its Fast mode')
    ap.add_argument('--queries', required=True, help='a file of one query a line')
    ap.add_argument('--out', default='eval/results/plain.json')
    a = ap.parse_args()
    QS = [l.strip() for l in open(a.queries) if l.strip()]
    db, app, alg = conn(a.strata), conn(a.app), conn('https://hn.algolia.com')
    rows = []
    cached = 0
    for i, q in enumerate(QS):
        rec = {'q': q}
        for run in (1, 2):
            for name, sql in STMTS.items():
                params = {'q': q, 'p': '%' + q.replace('%', '_') + '%', 'nonce': random.random()}
                st, j, ms = db.req('POST', '/v1/query', json.dumps({'sql': sql, 'params': params, 'timeout_ms': 30000}),
                                   {'Authorization': 'Bearer ' + KEY, 'content-type': 'application/json'})
                if 'answer cache' in json.dumps(j.get('stats', {})):
                    cached += 1
                rec[f'{name}_{run}'] = j.get('took_ms') if st == 200 else None
                if st != 200:
                    rec[f'{name}_{run}_err'] = str(j)[:200]
                if name.endswith('count') and st == 200 and j.get('rows'):
                    rec[name] = j['rows'][0][0]
                if name in ('bm25', 'fm') and st == 200:
                    rec[name + '_hits'] = len(j.get('rows', []))
                time.sleep(0.1)
            e = urllib.parse.quote(q)
            st, j, ms = app.req('GET', f'/v1/search?q={e}&only=story&facets=0&mode=fast', None, {'user-agent': 'chloedb-hn-latency'})
            rec[f'app_fast_{run}_server'] = j.get('took_ms') if st == 200 else None
            rec[f'app_fast_{run}_e2e'] = ms
            time.sleep(0.2)
            st, j, ms = alg.req('GET', f'/api/v1/search?query={e}&tags=story&hitsPerPage=20', None, {'user-agent': 'chloedb-hn-latency'})
            rec[f'algolia_{run}_server'] = j.get('processingTimeMS') if st == 200 else None
            rec[f'algolia_{run}_e2e'] = ms
            rec['algolia_hits'] = j.get('nbHits') if st == 200 else None
            time.sleep(0.2)
        rows.append(rec)
        print(i, q, {k: (round(v, 1) if isinstance(v, float) else v) for k, v in rec.items() if k != 'q' and not k.endswith('_err')}, file=sys.stderr, flush=True)
    keys = [f'{n}_{r}' for n in STMTS for r in (1, 2)] + [f'app_fast_{r}_{k}' for r in (1, 2) for k in ('server', 'e2e')] + [f'algolia_{r}_{k}' for r in (1, 2) for k in ('server', 'e2e')]
    out = {'when': time.strftime('%Y-%m-%d %H:%M:%S UTC', time.gmtime()), 'queries': len(rows), 'answer_cache_hits': cached,
           'statements': STMTS, 'stats': {k: stats([r.get(k) for r in rows]) for k in keys}, 'rows': rows}
    json.dump(out, open(a.out, 'w'), indent=1)
    print(json.dumps({k: v for k, v in out.items() if k != 'rows'}, indent=1))


if __name__ == '__main__':
    main()
