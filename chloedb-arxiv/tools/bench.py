#!/usr/bin/env python3
"""Search quality and speed of the arXiv app against arXiv's own search API
(export.arxiv.org/api/query) and Semantic Scholar's paper search.

  bench.py known   --app URL --rows ax_papers.jsonl --n 150 --out DIR
  bench.py topical --app URL --queries tools/bench_queries.txt --out DIR
  bench.py report  --out DIR

known: papers drawn from the rows, each looked for three ways: its title with
two typos, four of its title's words, and a sentence a model wrote about what
it does without its title (as someone who read it a while ago would ask).
Each system's top 10 is read; success@1, success@10 and MRR@10 per way.

topical: questions on a subject; every system's top 10 are pooled and a model
judges each paper 0 (off), 1 (related) or 2 (on the subject) from its title
and abstract, without knowing which system found it; nDCG@10 and P@10.

Speed is the time a client here waits for each answer (the network to each
service included), median and 95th percentile. arXiv asks for one request
every three seconds and Semantic Scholar's shared pool answers 429 often, so
both are asked slowly and retried; their failures are counted, not hidden.

Models are called through the Anthropic API directly (ANTHROPIC_API_KEY or
STRATA_ANTHROPIC_KEY), never through a session.
"""
import argparse, json, os, random, re, statistics, sys, time, urllib.parse, urllib.request, xml.etree.ElementTree as ET

UA = "chloedb-arxiv-bench/0.1 (a search comparison; one request every 3 s)"
DESCRIBE_MODEL = "claude-haiku-5-5"
JUDGE_MODEL = "claude-sonnet-5-5"


def get(url, headers=None, timeout=60):
    req = urllib.request.Request(url, headers={"user-agent": UA, **(headers or {})})
    t0 = time.time()
    with urllib.request.urlopen(req, timeout=timeout) as r:
        body = r.read()
    return body, time.time() - t0


def clean(i):
    i = re.sub(r"^https?://arxiv\.org/abs/", "", i or "")
    return re.sub(r"v\d+$", "", i)


class Ours:
    name = "ChloeDB app"
    def __init__(self, app):
        self.app = app.rstrip("/")
    def search(self, q):
        # A token when the app is not public (CHLOEDB_ARXIV_TOKEN).
        tok = os.environ.get("CHLOEDB_ARXIV_TOKEN")
        body, s = get(f"{self.app}/v1/search?q={urllib.parse.quote(q)}&facets=1", {"authorization": f"Bearer {tok}"} if tok else None)
        j = json.loads(body)
        if j.get("open"):
            return [j["open"]["paper"]], s, None, j
        return [p["id"] for p in j.get("papers", [])][:10], s, j.get("took_ms"), j


class Arxiv:
    name = "arXiv API"
    last = 0.0
    def search(self, q):
        words = [w for w in re.findall(r"[\w\-]+", q) if len(w) > 1]
        out, spent = [], 0.0
        for op in (" AND ", " OR "):  # the site ANDs the words; OR only when AND finds nothing (kinder to arXiv)
            sq = op.join(f"all:{w}" for w in words[:20])
            wait = 3.1 - (time.time() - Arxiv.last)
            if wait > 0:
                time.sleep(wait)
            for attempt in range(4):
                try:
                    body, s = get("https://export.arxiv.org/api/query?" + urllib.parse.urlencode({"search_query": sq, "max_results": 10}))
                    break
                except Exception:
                    time.sleep(5 * (attempt + 1))
            else:
                raise RuntimeError("arXiv API failed 4 times")
            Arxiv.last = time.time()
            spent += s
            root = ET.fromstring(body)
            ns = {"a": "http://www.w3.org/2005/Atom"}
            out = [{"id": clean(e.findtext("a:id", namespaces=ns)), "title": " ".join((e.findtext("a:title", namespaces=ns) or "").split()), "abstract": " ".join((e.findtext("a:summary", namespaces=ns) or "").split())} for e in root.findall("a:entry", ns)]
            if out:
                break
        return [x["id"] for x in out], spent, None, out


class S2:
    name = "Semantic Scholar"
    last = 0.0
    def search(self, q):
        url = "https://api.semanticscholar.org/graph/v1/paper/search?" + urllib.parse.urlencode({"query": q, "limit": 10, "fields": "externalIds,title,abstract"})
        for attempt in range(8):
            wait = 1.2 - (time.time() - S2.last)
            if wait > 0:
                time.sleep(wait)
            S2.last = time.time()
            try:
                body, s = get(url)
                j = json.loads(body)
                rows = j.get("data") or []
                ids = [(r.get("externalIds") or {}).get("ArXiv") or f"s2:{r.get('paperId')}" for r in rows]
                return ids, s, None, rows
            except urllib.error.HTTPError as e:
                if e.code in (429, 500, 502, 503, 504):
                    time.sleep(min(60, 3 * 2 ** attempt))
                    continue
                raise
        raise RuntimeError("Semantic Scholar answered 429 eight times")


def anthropic(model, system, user, max_tokens=300):
    key = os.environ.get("ANTHROPIC_API_KEY") or os.environ.get("STRATA_ANTHROPIC_KEY")
    if not key:
        sys.exit("needs ANTHROPIC_API_KEY or STRATA_ANTHROPIC_KEY")
    req_body = {"model": model, "max_tokens": max_tokens, "system": system, "messages": [{"role": "user", "content": user}]}
    # Short answers only: Haiku takes thinking off outright (at its default
    # effort); Sonnet 5.5 refuses "disabled" and takes "between_tools".
    if "haiku" in model:
        req_body["thinking"] = {"type": "disabled"}
    elif "sonnet-5-5" in model:
        req_body["thinking"] = {"type": "between_tools"}
    body = json.dumps(req_body).encode()
    for attempt in range(6):
        try:
            req = urllib.request.Request("https://api.anthropic.com/v1/messages", data=body, headers={"x-api-key": key, "anthropic-version": "2023-06-01", "content-type": "application/json"})
            with urllib.request.urlopen(req, timeout=120) as r:
                j = json.loads(r.read())
            text = "".join(c.get("text", "") for c in j.get("content", []) if c.get("type") == "text")
            if text.strip():
                return text
            time.sleep(1)
            continue
        except urllib.error.HTTPError as e:
            if e.code in (429, 500, 529, 503):
                time.sleep(5 * (attempt + 1))
                continue
            raise
    raise RuntimeError("model call failed")


STOP = set("a an the of on in for to and with by from via is are as at its into using towards toward under over between based new".split())


def pick(a):
    """The systems asked: --systems ours,arxiv,s2 (all three by default)."""
    names = ["ours"] if a.only_ours else [x.strip() for x in a.systems.split(",") if x.strip()]
    made = {"ours": lambda: Ours(a.app), "arxiv": Arxiv, "s2": S2}
    return [made[n]() for n in names]


def typo(title, rng):
    words = title.split()
    idx = sorted(range(len(words)), key=lambda i: -len(words[i]))[:2]
    for i in idx:
        w = words[i]
        if len(w) > 4:
            j = rng.randrange(1, len(w) - 2)
            words[i] = w[:j] + w[j + 1] + w[j] + w[j + 2:]
    return " ".join(words)


def some_words(title, rng):
    words = [w.strip(":,.;()") for w in title.split()]
    keep = [w for w in words if w.lower() not in STOP and len(w) > 2]
    rng.shuffle(keep)
    return " ".join(keep[:4]).lower()


DESCRIBE = ("You remember a research paper you read a while ago and want to find it again with a search box. "
            "Write the one search a researcher would type: 8 to 14 words saying what the paper does or found, in your own words. "
            "Do not copy any run of three or more words from the title, and do not use the title's made-up names or acronyms. "
            "Reply with the search only.")


def known(a):
    rng = random.Random(a.seed)
    rows = []
    with open(a.rows) as f:
        for l in f:
            r = json.loads(l)
            if (r.get("year") or 0) >= 2008 and len(r.get("title", "").split()) >= 5 and len(r.get("abstract", "")) > 400:
                rows.append({"id": r["id"], "title": r["title"], "abstract": r["abstract"], "year": r.get("year"), "pcat": r.get("pcat"), "cites": r.get("cites", 0)})
    rng.shuffle(rows)
    rows = rows[: a.n]
    os.makedirs(a.out, exist_ok=True)
    qpath = os.path.join(a.out, "known_queries.json")
    qs = json.load(open(qpath)) if os.path.exists(qpath) else {}
    for r in rows:
        if r["id"] in qs:
            continue
        qs[r["id"]] = {"typo": typo(r["title"], rng), "words": some_words(r["title"], rng),
                       "described": anthropic(DESCRIBE_MODEL, DESCRIBE, f"Title: {r['title']}\n\nAbstract: {r['abstract'][:1800]}", 80).strip().strip('"'),
                       "paper": r}
        json.dump(qs, open(qpath, "w"), indent=1)
    systems = pick(a)
    out = open(os.path.join(a.out, "known.jsonl"), "a")
    done = set()
    try:
        for l in open(os.path.join(a.out, "known.jsonl")):
            x = json.loads(l); done.add((x["paper"], x["way"], x["system"]))
    except FileNotFoundError:
        pass
    for k, (pid, q) in enumerate(qs.items()):
        for way in ("typo", "words", "described"):
            for s in systems:
                if (pid, way, s.name) in done:
                    continue
                try:
                    ids, secs, took, _ = s.search(q[way])
                    rank = next((i + 1 for i, x in enumerate(ids) if clean(x) == pid), None)
                    rec = {"paper": pid, "way": way, "system": s.name, "q": q[way], "rank": rank, "secs": round(secs, 3), "server_ms": took, "n": len(ids)}
                except Exception as e:
                    rec = {"paper": pid, "way": way, "system": s.name, "q": q[way], "error": str(e)[:200]}
                out.write(json.dumps(rec) + "\n"); out.flush()
        print(f"{k + 1}/{len(qs)}", file=sys.stderr)


JUDGE = ("You judge search results for researchers. Given a search and one paper's title and abstract, answer with one digit: "
         "2 if the paper is squarely about what the search asks for, 1 if it is related and somewhat useful, 0 if it is not about it. Answer the digit only.")


def topical(a):
    qs = [l.strip() for l in open(a.queries) if l.strip() and not l.startswith("#")]
    systems = pick(a)
    os.makedirs(a.out, exist_ok=True)
    rpath = os.path.join(a.out, "topical_runs.json")
    runs = json.load(open(rpath)) if os.path.exists(rpath) else {}
    for i, q in enumerate(qs):
        for s in systems:
            key = f"{s.name}\t{q}"
            if key in runs:
                continue
            try:
                ids, secs, took, raw = s.search(q)
                docs = []
                if s.name == "ChloeDB app":
                    docs = [{"id": p["id"], "title": p["title"], "abstract": p.get("abstract", "")} for p in raw.get("papers", [])[:10]]
                elif s.name == "arXiv API":
                    docs = raw[:10]
                else:
                    docs = [{"id": (r.get("externalIds") or {}).get("ArXiv") or f"s2:{r.get('paperId')}", "title": r.get("title") or "", "abstract": r.get("abstract") or ""} for r in raw[:10]]
                runs[key] = {"docs": docs, "secs": secs, "server_ms": took}
            except Exception as e:
                runs[key] = {"error": str(e)[:200]}
            json.dump(runs, open(rpath, "w"))
        print(f"{i + 1}/{len(qs)} run", file=sys.stderr)
    jpath = os.path.join(a.out, "judgments.json")
    judged = json.load(open(jpath)) if os.path.exists(jpath) else {}
    for key, run in runs.items():
        q = key.split("\t", 1)[1]
        for d in run.get("docs", []):
            jk = f"{q}\t{clean(d['id'])}"
            if jk in judged:
                continue
            text = f"Search: {q}\n\nTitle: {d['title']}\n\nAbstract: {(d.get('abstract') or '(none)')[:1800]}"
            ans = anthropic(JUDGE_MODEL, JUDGE, text, 5).strip()
            judged[jk] = int(ans[0]) if ans[:1] in "012" else 0
            json.dump(judged, open(jpath, "w"))
    print("judged", len(judged), file=sys.stderr)


def report(a):
    import math
    lines = []
    kp = os.path.join(a.out, "known.jsonl")
    if os.path.exists(kp):
        recs = [json.loads(l) for l in open(kp)]
        systems = sorted({r["system"] for r in recs})
        lines.append("## Finding a known paper\n")
        lines.append("| way | system | asked | failed | success@1 | success@10 | MRR@10 | median s | p95 s |")
        lines.append("|---|---|---|---|---|---|---|---|---|")
        for way in ("typo", "words", "described"):
            for s in systems:
                rs = [r for r in recs if r["way"] == way and r["system"] == s]
                ok = [r for r in rs if "error" not in r]
                if not rs:
                    continue
                s1 = sum(1 for r in ok if r["rank"] == 1) / max(1, len(rs))
                s10 = sum(1 for r in ok if r["rank"]) / max(1, len(rs))
                mrr = sum(1 / r["rank"] for r in ok if r["rank"]) / max(1, len(rs))
                t = sorted(r["secs"] for r in ok) or [0]
                lines.append(f"| {way} | {s} | {len(rs)} | {len(rs) - len(ok)} | {s1:.2f} | {s10:.2f} | {mrr:.3f} | {statistics.median(t):.2f} | {t[int(0.95 * (len(t) - 1))]:.2f} |")
    rp, jp = os.path.join(a.out, "topical_runs.json"), os.path.join(a.out, "judgments.json")
    if os.path.exists(rp) and os.path.exists(jp):
        runs, judged = json.load(open(rp)), json.load(open(jp))
        qs = sorted({k.split("\t", 1)[1] for k in runs})
        systems = sorted({k.split("\t", 1)[0] for k in runs})
        lines.append("\n## Topical searches, judged\n")
        lines.append("| system | queries | failed | nDCG@10 | P@10 (2s) | P@10 (1s and 2s) | median s | p95 s |")
        lines.append("|---|---|---|---|---|---|---|---|")
        for s in systems:
            nd, p2, p1, ts, fail = [], [], [], [], 0
            for q in qs:
                run = runs.get(f"{s}\t{q}")
                if not run or "error" in run:
                    fail += 1; nd.append(0); p2.append(0); p1.append(0)
                    continue
                g = [judged.get(f"{q}\t{clean(d['id'])}", 0) for d in run["docs"][:10]]
                pool = sorted([v for k, v in judged.items() if k.startswith(q + "\t")], reverse=True)[:10]
                dcg = sum((2 ** x - 1) / math.log2(i + 2) for i, x in enumerate(g))
                idcg = sum((2 ** x - 1) / math.log2(i + 2) for i, x in enumerate(pool)) or 1
                nd.append(dcg / idcg); p2.append(sum(1 for x in g if x == 2) / 10); p1.append(sum(1 for x in g if x >= 1) / 10)
                ts.append(run["secs"])
            ts = sorted(ts) or [0]
            lines.append(f"| {s} | {len(qs)} | {fail} | {statistics.mean(nd):.3f} | {statistics.mean(p2):.2f} | {statistics.mean(p1):.2f} | {statistics.median(ts):.2f} | {ts[int(0.95 * (len(ts) - 1))]:.2f} |")
    print("\n".join(lines))


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["known", "topical", "report"])
    ap.add_argument("--app", default="http://127.0.0.1:7860")
    ap.add_argument("--rows")
    ap.add_argument("--queries")
    ap.add_argument("--n", type=int, default=150)
    ap.add_argument("--seed", type=int, default=7)
    ap.add_argument("--out", required=True)
    ap.add_argument("--only-ours", action="store_true")
    ap.add_argument("--systems", default="ours,arxiv,s2", help="which to ask, of ours, arxiv and s2; Semantic Scholar's shared pool can refuse for hours, so it can run on its own")
    a = ap.parse_args()
    {"known": known, "topical": topical, "report": report}[a.cmd](a)
