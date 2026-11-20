"""Load the two prototype tables into a local chloedb-server on :7700.
hn_stories: Hacker News stories of July 2026 (open-index/hacker-news).
doc_chunks: ChloeDB's own docs cut into ~120-word chunks (a corpus no model has seen)."""
import json, os, re, sys, glob, urllib.parse, requests
import pyarrow.parquet as pq, pyarrow.compute as pc
S = os.environ["S"]; URL = "http://127.0.0.1:7700"
def sql(q): r = requests.post(URL + "/v1/sql", json={"sql": q}); r.raise_for_status(); return r.json()
def imp(table, rows):
    body = "\n".join(json.dumps(r) for r in rows)
    r = requests.post(f"{URL}/v1/import?table={table}&meaning=false", data=body.encode(), headers={"content-type": "application/x-ndjson"}); r.raise_for_status(); print(table, r.json().get("written"))

sql("""CREATE TABLE IF NOT EXISTS hn_stories (id INT PRIMARY KEY, title TEXT, url KEYWORD, domain KEYWORD,
  author KEYWORD, created TIMESTAMP, points INT, comments INT)""")
t = pq.read_table(f"{S}/data/data/2026/2026-07.parquet", columns=["id","type","by","time","url","score","title","descendants","dead","deleted"])
t = t.filter(pc.and_(pc.equal(t["type"], 1), pc.and_(pc.equal(t["dead"], 0), pc.equal(t["deleted"], 0))))
rows = []
for r in t.to_pylist():
    if not r["title"]: continue
    dom = urllib.parse.urlparse(r["url"] or "").netloc.lower().removeprefix("www.") or None
    rows.append({"id": r["id"], "title": r["title"], "url": r["url"], "domain": dom, "author": r["by"],
                 "created": r["time"].strftime("%Y-%m-%dT%H:%M:%SZ"), "points": r["score"] or 0, "comments": r["descendants"] or 0})
for i in range(0, len(rows), 20000): imp("hn_stories", rows[i:i+20000])

sql("CREATE TABLE IF NOT EXISTS doc_chunks (id KEYWORD PRIMARY KEY, doc KEYWORD, pos INT, body TEXT)")
rows = []
for f in sorted(glob.glob("/home/user/chloedb/docs/chloedb/*.md") + glob.glob("/home/user/chloedb/docs/chloectx/*.md") + glob.glob("/home/user/chloedb/docs/guide/*.md")):
    doc = os.path.relpath(f, "/home/user/chloedb/docs"); words = open(f).read().split()
    for k, i in enumerate(range(0, len(words), 120)):
        rows.append({"id": f"{doc}#{k}", "doc": doc, "pos": k, "body": " ".join(words[i:i+120])})
imp("doc_chunks", rows)
print(sql("SELECT COUNT(*) FROM hn_stories")["rows"], sql("SELECT COUNT(*) FROM doc_chunks")["rows"])
