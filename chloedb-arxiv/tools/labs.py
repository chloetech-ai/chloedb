#!/usr/bin/env python3
"""Labs: joins the organisations of arXiv papers read from OpenAlex's public
snapshot (the JSON lines `oa_orgs.py` writes, one per paper:
{"id", "orgs": [[key, name, country, kind], ...]}) with the papers' rows
(`prepare`'s ax_papers.jsonl), and writes two tables' rows:

  ax_lab_papers.jsonl  a paper's organisations and the columns a lab page counts by
  ax_labs.jsonl        each organisation: name, country, kind, papers in all,
                       in the last twelve months and the twelve before, first year

    python3 tools/labs.py OA_PARTS_DIR ROWS/ax_papers.jsonl OUT_DIR

A paper of more than 40 organisations keeps the first 40 (big collaborations).
Standard library only."""
import datetime as dt, json, os, re, sys

parts, papers, out = sys.argv[1], sys.argv[2], sys.argv[3]
os.makedirs(out, exist_ok=True)
OLD = re.compile(r"^([a-z\-]+)(?:\.[A-Z]{2})?/(\d{7})$")


def norm(i):
    i = i.strip()
    m = OLD.match(i)
    return f"{m.group(1)}/{m.group(2)}" if m else re.sub(r"v\d+$", "", i)


orgs_of, info = {}, {}
for f in sorted(os.listdir(parts)):
    if not f.endswith(".jsonl"):
        continue
    for line in open(os.path.join(parts, f)):
        r = json.loads(line)
        keys = orgs_of.setdefault(norm(r["id"]), [])
        for k, name, cc, kind in r["orgs"]:
            k = sys.intern(k.lower())
            if k not in keys:
                keys.append(k)
            if k not in info:
                info[k] = [name or k, cc or "", kind or ""]
print(f"{len(orgs_of)} papers with organisations, {len(info)} organisations", flush=True)

now = dt.datetime.now(dt.timezone.utc)
cut1 = (now - dt.timedelta(days=365)).strftime("%Y-%m-%dT%H:%M:%SZ")
cut2 = (now - dt.timedelta(days=730)).strftime("%Y-%m-%dT%H:%M:%SZ")
stats = {}  # key -> [papers, recent, before, first]
n = kept = 0
with open(os.path.join(out, "ax_lab_papers.jsonl"), "w") as w:
    for line in open(papers):
        n += 1
        p = json.loads(line)
        keys = orgs_of.get(p["id"])
        if not keys:
            continue
        keys = keys[:40]
        kept += 1
        created = p.get("created") or ""
        year = p.get("year") or 0
        w.write(json.dumps({
            "id": p["id"], "orgs": keys, "countries": sorted({info[k][1] for k in keys if info[k][1]}),
            "created": created, "year": year, "techniques": p.get("techniques") or [], "domains": p.get("domains") or [],
            "pcat": p.get("pcat") or "", "areas": p.get("areas") or [], "people": p.get("people") or [], "cites": p.get("cites") or 0,
        }, ensure_ascii=False) + "\n")
        for k in keys:
            s = stats.setdefault(k, [0, 0, 0, 9999])
            s[0] += 1
            if created >= cut1:
                s[1] += 1
            elif created >= cut2:
                s[2] += 1
            if year and year < s[3]:
                s[3] = year
        if n % 500000 == 0:
            print(f"{n} papers read, {kept} with organisations", flush=True)
with open(os.path.join(out, "ax_labs.jsonl"), "w") as w:
    for k, s in stats.items():
        name, cc, kind = info[k]
        w.write(json.dumps({"id": k, "name": name, "country": cc, "kind": kind, "papers": s[0], "recent": s[1], "before": s[2], "first": s[3] if s[3] < 9999 else 0}, ensure_ascii=False) + "\n")
print(f"done: {n} papers, {kept} with organisations, {len(stats)} organisations", flush=True)
