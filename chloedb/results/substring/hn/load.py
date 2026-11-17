"""load.py URL FILE: creates hn_stories as chloedb-hn does (without the vector) and imports FILE in 32 MB chunks."""
import sys, json, http.client, urllib.parse, time
url, path = sys.argv[1], sys.argv[2]
u = urllib.parse.urlparse(url)
def req(method, p, body, ctype):
    c = http.client.HTTPConnection(u.hostname, u.port, timeout=3600)
    c.request(method, p, body=body, headers={'content-type': ctype})
    r = c.getresponse(); b = r.read()
    if r.status != 200: raise SystemExit(f"{r.status}: {b[:400]}")
    return json.loads(b)
ddl = """CREATE TABLE IF NOT EXISTS hn_stories (
    id INT PRIMARY KEY,
    title TEXT OPTIONS(substring = true),
    body TEXT,
    url KEYWORD OPTIONS(substring = true), curl KEYWORD, domain KEYWORD, site KEYWORD,
    urlwords TEXT OPTIONS(analyzer = 'simple', positions = false),
    author KEYWORD,
    created TIMESTAMP, year INT,
    points INT, comments INT,
    kind KEYWORD, topics KEYWORD[],
    pop FLOAT
)"""
print(req('POST', '/v1/sql', json.dumps({'sql': ddl}), 'application/json'))
t0 = time.time(); n = 0; buf = []; size = 0
def flush():
    global buf, size
    if buf:
        req('POST', '/v1/import?format=jsonl&table=hn_stories', ''.join(buf).encode(), 'application/x-ndjson')
        buf, size = [], 0
for line in open(path, encoding='utf-8'):
    buf.append(line); size += len(line); n += 1
    if size >= 32 << 20:
        flush(); print(n, round(time.time() - t0), 's', file=sys.stderr, flush=True)
flush()
print('loaded', n, round(time.time() - t0), 's', file=sys.stderr)
