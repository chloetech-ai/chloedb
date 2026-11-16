"""Fetch HN stories (type 1/3/5, live, titled) from HF open-index/hacker-news,
reading only the needed columns, into JSONL rows shaped like chloedb-hn's
hn_stories (without the vector). Usage: fetch_stories.py OUT.jsonl"""
import sys, json, re, math, datetime, concurrent.futures as cf
import pyarrow.parquet as pq
from huggingface_hub import HfFileSystem

fs = HfFileSystem()
files = sorted(p for p in fs.glob("datasets/open-index/hacker-news/data/*/*.parquet"))
print(len(files), "files", file=sys.stderr, flush=True)
COLS = ["id", "deleted", "dead", "type", "by", "time", "url", "score", "title", "descendants"]

def domain(u):
    m = re.match(r"^[a-z]+://([^/:?#]+)", u.lower())
    return m.group(1) if m else ""

def site(d):
    if d.startswith("www."): d = d[4:]
    parts = d.split(".")
    return ".".join(parts[-2:]) if len(parts) >= 2 else d

def curl(u):
    u = u.strip().lower()
    u = re.sub(r"^[a-z]+://", "", u)
    if u.startswith("www."): u = u[4:]
    return u.rstrip("/")

def kind(t, title):
    if t == 5: return "job"
    if t == 3: return "poll"
    for k in ("Ask HN", "Show HN", "Launch HN", "Tell HN"):
        if title.startswith(k): return k.split()[0].lower()
    return "story"

def one(path):
    for attempt in range(4):
        try:
            with fs.open(path, block_size=4 << 20) as f:
                t = pq.read_table(f, columns=COLS)
            break
        except Exception as e:
            print("retry", path, e, file=sys.stderr, flush=True)
    else:
        raise RuntimeError(path)
    d = t.to_pydict()
    out = []
    for i in range(t.num_rows):
        typ = d["type"][i]
        if typ not in (1, 3, 5) or d["deleted"][i] or d["dead"][i]:
            continue
        title = d["title"][i]
        if not title:
            continue
        url = d["url"][i] or ""
        tm = d["time"][i]
        dm = domain(url)
        score = d["score"][i] or 0
        out.append(json.dumps({
            "id": d["id"][i], "title": title, "body": "", "url": url, "curl": curl(url),
            "domain": dm, "site": site(dm), "urlwords": " ".join(w for w in re.split(r"[^a-z0-9]+", url.lower()) if w and w not in ("http", "https", "www")),
            "author": d["by"][i], "created": tm.strftime("%Y-%m-%dT%H:%M:%SZ"), "year": tm.year,
            "points": score, "comments": d["descendants"][i] or 0, "kind": kind(typ, title), "topics": [],
            "pop": round(math.log(1 + max(score, 0)), 4),
        }, ensure_ascii=False))
    return path, out

with open(sys.argv[1], "w") as w, cf.ThreadPoolExecutor(8) as ex:
    n = 0
    for path, rows in ex.map(one, files):
        for r in rows:
            w.write(r + "\n")
        n += len(rows)
        print(path.split("/")[-1], len(rows), n, file=sys.stderr, flush=True)
print("stories", n, file=sys.stderr)
