#!/usr/bin/env python3
"""Hacker News search on ChloeDB against hn.algolia.com, on the same queries.

Three sets, made from the stories both hold (ChloeDB's tables are read for
them, so run it against a loaded app):

  known   a story found again from three of its title's words
  typo    the same three words with a typo in the longest
  meant   the story described in other words (a sentence a model wrote
          without the title's words), as someone half-remembering it asks
  topic   queries about a subject; each engine's top 10 judged by a model,
          blind to which engine found what, nDCG@10 over the pooled grades

and the time each took: ChloeDB's `took_ms` (server, the whole search page:
understanding, stories, facets, highlights) and Algolia's processingTimeMS,
and both end to end from this machine.

Algolia's public API allows 10,000 requests an hour from one address; this
asks a few hundred. The model calls go through the ChloeDB server's llm(),
which holds the provider's key. Against a cluster, CHLOEDB_HN_TOKEN is the
app's token and CHLOEDB_KEY the server's key.

    python3 eval/compare.py --app http://127.0.0.1:7851 --strata http://127.0.0.1:7701 --out eval/results
"""
import argparse, json, math, os, random, re, statistics, sys, time, urllib.parse, urllib.request

ALGOLIA = "https://hn.algolia.com/api/v1/search"

# The app's token and the server's key (a cluster asks for both); from the
# environment, so that neither is on a command line.
AUTH = {"app": os.environ.get("CHLOEDB_HN_TOKEN", ""), "strata": os.environ.get("CHLOEDB_KEY", "")}

def headers(token=""):
    h = {"user-agent": "chloedb-hn-eval", "content-type": "application/json"}
    if token:
        h["authorization"] = f"Bearer {token}"
    return h

def get(url, timeout=60, token=""):
    t0 = time.perf_counter()
    with urllib.request.urlopen(urllib.request.Request(url, headers=headers(token)), timeout=timeout) as r:
        body = json.load(r)
    return body, (time.perf_counter() - t0) * 1000

def post(url, body, timeout=120):
    req = urllib.request.Request(url, data=json.dumps(body).encode(), headers=headers(AUTH["strata"]))
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.load(r)

def sql(strata, q, params=None):
    return post(f"{strata}/v1/query", {"sql": q, "params": params or {}, "objects": True})["rows"]

STOP = set("a an the of to in on for and or with by from at is are was be how why what your you my we our it its this that new using use vs into about over just can not".split())

def words_of(title):
    t = re.sub(r"^(show|ask|launch|tell) hn:\s*", "", title, flags=re.I)
    return [w for w in re.findall(r"[A-Za-z][A-Za-z0-9+#.\-]*[A-Za-z0-9+#]|[A-Za-z]", t) if w.lower() not in STOP and len(w) > 2]

def typo(word, rnd):
    if len(word) < 5:
        return word
    i = rnd.randrange(1, len(word) - 1)
    kind = rnd.choice(["drop", "swap", "double"])
    if kind == "drop":
        return word[:i] + word[i + 1:]
    if kind == "swap":
        return word[:i] + word[i + 1] + word[i] + word[i + 2:]
    return word[:i] + word[i] + word[i:]

def llm(strata, model, instructions, text, max_tokens=300):
    rows = sql(strata, "SELECT llm(:m, :i, :t, max_tokens => %d) AS a" % max_tokens, {"m": model, "i": instructions, "t": text})
    return (rows[0]["a"] if rows else "") or ""

def ours(app, q, only="story"):
    j, e2e = get(f"{app}/v1/search?q={urllib.parse.quote(q)}&only={only}&facets=0", token=AUTH["app"])
    return [int(s["id"]) for s in j.get("stories", [])], j.get("took_ms"), e2e, j

def theirs(q):
    j, e2e = get(f"{ALGOLIA}?query={urllib.parse.quote(q)}&tags=story&hitsPerPage=20")
    return [int(h["objectID"]) for h in j.get("hits", []) if h.get("objectID", "").isdigit()], j.get("processingTimeMS"), e2e, j

def rank_of(ids, target):
    try:
        return ids.index(target) + 1
    except ValueError:
        return None

def summary(ranks):
    n = len(ranks)
    hit1 = sum(1 for r in ranks if r == 1) / n
    hit10 = sum(1 for r in ranks if r and r <= 10) / n
    mrr = sum(1 / r for r in ranks if r and r <= 10) / n
    return {"n": n, "hit@1": round(hit1, 3), "hit@10": round(hit10, 3), "mrr@10": round(mrr, 3)}

def pct(xs, p):
    xs = sorted(x for x in xs if x is not None)
    if not xs:
        return None
    k = (len(xs) - 1) * p
    f, c = math.floor(k), math.ceil(k)
    return round(xs[f] + (xs[c] - xs[f]) * (k - f), 1)

TOPICS = [
    "rust async runtime", "sqlite in production", "postgres full text search", "self hosting email", "remote work productivity",
    "kubernetes is too complex", "learning to code as an adult", "startup equity for early employees", "static site generators",
    "vim vs emacs", "how to hire engineers", "open source sustainability funding", "GPU programming for beginners", "type systems in python",
    "web assembly outside the browser", "home lab servers", "e-ink displays", "mechanical keyboards", "privacy focused analytics",
    "database indexing internals", "garbage collection pauses", "monorepo tooling", "burnout in tech", "writing a compiler",
    "zero knowledge proofs explained", "electric cars battery degradation", "nuclear fusion progress", "the decline of google search",
    "local first software", "llm hallucinations", "code review best practices", "technical interviews leetcode", "on call rotations",
    "distributed consensus raft paxos", "ad blocking", "right to repair", "fast build times", "the history of unix",
    "personal knowledge management notes", "how to price a saas product",
]

JUDGE = ("You grade search results for a Hacker News search engine. Given a query and one story (title, link, and the start of its text), "
         "answer with one digit: 3 = exactly what the query looks for, 2 = clearly relevant, 1 = loosely related, 0 = not relevant. Only the digit.")

def judge(strata, model, q, story, cache):
    key = f"{q}\t{story['id']}"
    if key in cache:
        return cache[key]
    text = f"Query: {q}\n\nStory: {story.get('title','')}\nLink: {story.get('url','')}\n{(story.get('body') or '')[:400]}"
    a = llm(strata, model, JUDGE, text, max_tokens=5).strip()
    g = int(a[0]) if a[:1].isdigit() else 0
    cache[key] = g
    return g

def ndcg(grades, k=10):
    dcg = sum((2 ** g - 1) / math.log2(i + 2) for i, g in enumerate(grades[:k]))
    return dcg

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--app", default="http://127.0.0.1:7851")
    ap.add_argument("--strata", default="http://127.0.0.1:7701")
    ap.add_argument("--out", default="eval/results")
    ap.add_argument("--n", type=int, default=120, help="stories for each known-item set")
    ap.add_argument("--model", default="anthropic/claude-haiku-5-5")
    ap.add_argument("--seed", type=int, default=7)
    ap.add_argument("--sets", default="known,typo,meant,topic")
    ap.add_argument("--pause", type=float, default=0.15, help="seconds between Algolia requests")
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)
    rnd = random.Random(a.seed)
    sets = a.sets.split(",")
    # Stories both engines hold: 100+ points, every year, by a seeded hash.
    pool = sql(a.strata, "SELECT id, title, url, body, year, points FROM hn_stories WHERE points >= 100 AND kind IN ('story', 'show', 'ask') AND year >= 2007 AND year <= 2025 ORDER BY hash(id, :s) LIMIT :n", {"s": a.seed, "n": a.n * 3})
    pool = [s for s in pool if len(words_of(s["title"])) >= 3][: a.n]
    print(f"{len(pool)} stories sampled", file=sys.stderr)
    out = {"when": time.strftime("%Y-%m-%d %H:%M:%S %Z"), "sets": {}, "latency": {}, "queries": []}
    lat = {"ours_server": [], "ours_e2e": [], "algolia_server": [], "algolia_e2e": []}

    def run(name, q, target):
        oi, ot, oe, _ = ours(a.app, q)
        time.sleep(a.pause)
        ai, at, ae, _ = theirs(q)
        lat["ours_server"].append(ot); lat["ours_e2e"].append(oe); lat["algolia_server"].append(at); lat["algolia_e2e"].append(ae)
        r = {"set": name, "q": q, "target": target, "ours": rank_of(oi, target), "algolia": rank_of(ai, target), "ours_ms": ot, "algolia_ms": at}
        out["queries"].append(r)
        return r

    for name in ("known", "typo", "meant"):
        if name not in sets:
            continue
        rs = []
        for s in pool:
            w = words_of(s["title"])
            three = sorted(w, key=lambda x: -len(x))[:3]
            three = [x for x in w if x in three][:3]
            if name == "known":
                q = " ".join(three)
            elif name == "typo":
                longest = max(three, key=len)
                q = " ".join(typo(x, rnd) if x == longest else x for x in three)
            else:
                q = llm(a.strata, a.model, "Write how someone who half remembers this Hacker News story would search for it: 4 to 10 words, plain lowercase, WITHOUT any word of the title (no synonyms of its rare names either if avoidable). Only the search.", f"Title: {s['title']}\nLink: {s.get('url') or ''}", 40).strip().strip('"').splitlines()[0][:120]
            rs.append(run(name, q, int(s["id"])))
            print(name, rs[-1], file=sys.stderr)
        out["sets"][name] = {"ours": summary([r["ours"] for r in rs]), "algolia": summary([r["algolia"] for r in rs])}

    if "topic" in sets:
        cache_path = os.path.join(a.out, "judgments.json")
        cache = json.load(open(cache_path)) if os.path.exists(cache_path) else {}
        per = []
        for q in TOPICS:
            oi, ot, oe, oj = ours(a.app, q)
            time.sleep(a.pause)
            ai, at, ae, aj = theirs(q)
            lat["ours_server"].append(ot); lat["ours_e2e"].append(oe); lat["algolia_server"].append(at); lat["algolia_e2e"].append(ae)
            stories = {}
            for s in oj.get("stories", [])[:10]:
                stories[int(s["id"])] = {"id": int(s["id"]), "title": s["title"], "url": s.get("url"), "body": s.get("body")}
            for h in aj.get("hits", [])[:10]:
                i = int(h["objectID"])
                stories.setdefault(i, {"id": i, "title": h.get("title"), "url": h.get("url"), "body": h.get("story_text")})
            ids = list(stories)
            rnd.shuffle(ids)
            g = {i: judge(a.strata, a.model, q, stories[i], cache) for i in ids}
            json.dump(cache, open(cache_path, "w"))
            ideal = ndcg(sorted(g.values(), reverse=True)) or 1
            o = ndcg([g[i] for i in oi[:10]]) / ideal
            al = ndcg([g[i] for i in ai[:10]]) / ideal
            per.append({"q": q, "ours": round(o, 3), "algolia": round(al, 3)})
            print("topic", per[-1], file=sys.stderr)
        out["sets"]["topic"] = {"ours": {"ndcg@10": round(statistics.mean(p["ours"] for p in per), 3)}, "algolia": {"ndcg@10": round(statistics.mean(p["algolia"] for p in per), 3)}, "per_query": per}

    for k, v in lat.items():
        out["latency"][k] = {"p50": pct(v, 0.5), "p95": pct(v, 0.95), "n": len([x for x in v if x is not None])}
    json.dump(out, open(os.path.join(a.out, "compare.json"), "w"), indent=1)
    print(json.dumps({"sets": {k: {kk: vv for kk, vv in v.items() if kk != "per_query"} for k, v in out["sets"].items()}, "latency": out["latency"]}, indent=1))

if __name__ == "__main__":
    main()
