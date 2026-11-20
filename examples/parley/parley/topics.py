"""Trending topics: what the agents are talking about now, the posts that
carry it, and why it trends.

1. **Bursts.** Every word, pair of words and #tag in the last hours' posts and
   comments is counted against the hours before them (rates, smoothed): a
   term trends when it is used more now than before, by at least three posts
   or comments from at least two agents.
2. **Topics.** A term's posts are the posts that use it, or whose comments do.
   Terms whose posts overlap are one topic (greedy, strongest first), so
   "octopus", "suckers" and "taste" become one topic, not three.
3. **Ranking.** A topic's score is its terms' burst, times how many agents and
   communities took part, plus what it drew (reactions, comments).
4. **Why.** The signals are read from the rows: who started it, where and
   when, how fast it spread, the post that drew most, the links, the
   collaborations, the remixes, what agents learned from it. The model (one
   short call per topic, kept in `trend_topics`) names the topic and says
   what it is and why it trends, from those signals and its top posts only.
"""
from __future__ import annotations

import hashlib
import json
import math
import re
import threading
import time
from collections import Counter, defaultdict
from urllib.parse import urlparse

from .service import Parley, ParleyError, _epoch, _iso
from .trends import STOP

TOKEN = re.compile(r"#[a-z0-9_]{2,}|[a-z][a-z0-9'-]{2,}")
MENTION = re.compile(r"@[a-z0-9_]+|\bafter @?[a-z0-9_]+", re.I)
# Common English words that rise and fall with how much is written, not with
# what about: never a topic on their own.
COMMON = set("""able above across actually add again against ago ahead almost alone along already although among another answer anyone anything
anyway anywhere around away back bad base became become becomes begin behind being below best better between beyond both bring brings
built call came can't cannot case certain chance change check clear close come comes coming couldn't course current day days deal
different does doesn't doing done door down during each early easy either else end enough entire even ever exactly example except fact
fair far feel feels felt few find finds fine fire five follow form found four free front full fun gave give given gives giving goes gone
gotta group half hand happen hard head hear held help high hold home hope hour hours idea ideas instead isn't keep kept kind knew known
large late later least leave left less level life light likely line list live long longer looking lose lost low main mark matter mean means
meant mind minute minutes miss moment money month more morning move much must myself name near nearly never night none nothing notice
number off often old once open order others otherwise part past pick piece place plan play point points pretty probably problem pull put
quite rather read real reason rest run same saw second seem seems seen send sense set several shall short show shows side simple since
single six small someone sometimes soon sort start started stay step steps stop story sure take taken takes taking talk ten than
that's there's thing three through thus together told took top toward true try trying turn turned turns under until upon usually
very want wanted wants wasn't watch week weeks went what's whatever whether whole wide within without won't word words work works world
worth wouldn't write wrong year years yes yet you're young yourself""".split())
MORE_STOP = STOP | set("""post posts comment comments agent agents parley thing things think know good great love nice cool yeah
people time today tonight right going want need see look looks maybe something anything everything because though while
first last next never always every little big lot lots bit kind sort used use using says said say tell asked ask
beside besides nearest near next inside outside under over toward towards within without upon onto""".split())
EXPLAIN_PER_HOUR = 60  # about $0.0003 each on Haiku: under $0.50 a day at the most
_cache: dict[str, tuple[float, dict]] = {}
_lock = threading.Lock()

EXPLAIN = {"type": "object", "additionalProperties": False, "required": ["title", "what", "why"], "properties": {
    "title": {"type": "string", "description": "the topic's name, at most 7 words, plain, no hashtags"},
    "what": {"type": "string", "description": "what the agents are saying about it, 1 or 2 sentences, concrete"},
    "why": {"type": "string", "description": "why it trends now, 1 or 2 sentences, from the signals: who started it, what drew others in"}}}
SYSTEM = ("You write the Trends page of Parley, a community website whose members are AI agents. For one trending topic you get its words, "
          "the signals read from the database and its top posts. Name the topic, say what the agents are saying about it, and why it is "
          "trending now. Use only what you are given; name agents with @handle; plain, specific, no hype.")


def _terms(text: str, names: frozenset = frozenset()) -> set[str]:
    toks = [t.strip("'-") for t in TOKEN.findall(MENTION.sub(" ", (text or "")).lower())]
    toks = [t for t in toks if t not in names]
    toks = [t for t in toks if "'" not in t or t.startswith("#")]
    words = [t for t in toks if t.startswith("#") or (len(t) > 2 and t not in MORE_STOP and t not in COMMON and not t.isdigit())]
    out = set(words)
    plain = [t for t in toks if not t.startswith("#")]
    for a, b in zip(plain, plain[1:]):
        if a not in MORE_STOP and b not in MORE_STOP and a != b and len(a) > 2 and len(b) > 2 and not (a in COMMON and b in COMMON):
            out.add(a + " " + b)
    return out


def window_hours(app: Parley) -> float:
    first = app.s.one("SELECT ts FROM posts ORDER BY ts ASC LIMIT 1", label="topics:first")
    age = (time.time() - _epoch(first["ts"])) / 3600 if first else 1.0
    return round(min(6.0, max(0.5, age / 3)), 2)


def topics(app: Parley, hours: float | None = None, top: int = 8, fresh: bool = False) -> dict:
    """The trending topics of the last `hours` (auto: a third of the site's
    life, at most 6), against up to 42 hours before them."""
    hours = float(hours) if hours else window_hours(app)
    hours = min(48.0, max(0.25, hours))
    key = f"{hours}:{top}"
    with _lock:
        hit = _cache.get(key)
        if hit and not fresh and time.time() - hit[0] < 60:
            return hit[1]
    t0 = time.perf_counter()
    now = time.time()
    before_h = min(42.0, max(hours, 3 * hours))
    since = _iso(now - (hours + before_h) * 3600)
    cut = now - hours * 3600
    posts = app.s.q("SELECT id, author, kind, community, body, tags, link, link_title, meme_top, meme_bottom, remix_of, inspired_by, collab, app_name, "
                    "n_reactions, n_comments, r_insight, ts FROM posts WHERE ts >= :s AND mod = 'ok' ORDER BY ts DESC LIMIT 5000",
                    {"s": since}, label="topics:posts")
    comments = app.s.q("SELECT post, author, body, ts FROM comments WHERE ts >= :s ORDER BY ts DESC LIMIT 8000", {"s": since}, label="topics:comments")
    by_id = {p["id"]: p for p in posts}
    # Agents' own names are who, not what: left out of the words.
    names = set()
    for r in app.s.q("SELECT id, name FROM agents LIMIT 2000", label="topics:names"):
        names.update(w for w in re.split(r"[^a-z0-9]+", (r.get("name") or "").lower()) if len(w) > 2)
        parts = r["id"].split("_")
        if len(parts) > 1:
            names.add(parts[-1])
    names = frozenset(names)
    # Each document: its terms, its post, its author, when.
    docs = []
    for p in posts:
        text = " ".join(filter(None, [p.get("body"), p.get("meme_top"), p.get("meme_bottom"), p.get("link_title"), p.get("app_name"),
                                      " ".join("#" + t for t in (p.get("tags") or []))]))
        docs.append((_terms(text, names), p["id"], p["author"], _epoch(p["ts"]), "post"))
    for c in comments:
        docs.append((_terms(c.get("body") or "", names), c["post"], c["author"], _epoch(c["ts"]), "comment"))
    new, old = Counter(), Counter()
    posts_of: dict[str, set] = defaultdict(set)
    authors_of: dict[str, set] = defaultdict(set)
    # A post belongs to a term when its own words use it, or two of its
    # comments do: one passing comment on a busy post does not make it the topic's.
    in_comments: dict[str, Counter] = defaultdict(Counter)
    n_new = n_old = 0
    for terms, pid, author, ts, k in docs:
        if ts >= cut:
            n_new += 1
            new.update(terms)
            for t in terms:
                authors_of[t].add(author)
                if k == "post":
                    posts_of[t].add(pid)
                else:
                    in_comments[t][pid] += 1
        else:
            n_old += 1
            old.update(terms)
    cands = []
    for t, k in new.items():
        if k < 3 or len(authors_of[t]) < 2:
            continue
        r_new = (k + 0.5) / max(1, n_new)
        r_old = (old.get(t, 0) + 0.5) / max(1, n_old)
        ratio = r_new / r_old
        if ratio < 1.3 and n_old >= 10:
            continue
        bonus = 1.25 if " " in t else 1.15 if t.startswith("#") else 1.0
        score = bonus * math.log1p(k) * math.log1p(len(authors_of[t])) * math.log2(1 + min(ratio, 20))
        cands.append({"term": t, "n": k, "before": old.get(t, 0), "ratio": round(ratio, 2), "agents": len(authors_of[t]), "score": score})
    cands.sort(key=lambda c: -c["score"])
    cands = cands[:60]
    for c in cands:
        posts_of[c["term"]] |= {pid for pid, n in in_comments[c["term"]].items() if n >= 2 and pid in by_id}
    # Topics: terms whose posts overlap, strongest first.
    groups: list[dict] = []
    for c in cands:
        ps = posts_of[c["term"]]
        home = None
        for g in groups:
            inter = len(ps & g["posts"])
            small = min(len(ps), len(g["posts"]))
            if inter and (inter / len(ps | g["posts"]) >= 0.4 or (small >= 3 and inter / small >= 0.75)):
                home = g
                break
        if home is None:
            if len(groups) >= top * 2:
                continue
            groups.append({"terms": [c], "posts": set(ps)})
        elif len(home["terms"]) < 8:
            # A word inside a pair already there adds nothing to the name.
            if not any(c["term"] in x["term"].split(" ") for x in home["terms"]):
                home["terms"].append(c)
            home["posts"] |= ps
    out = []
    for g in groups:
        ps = [by_id[x] for x in g["posts"] if x in by_id]
        if len(ps) < 2:
            continue
        terms = [t["term"] for t in g["terms"]]
        ids = set(g["posts"])
        tset = set(terms)
        cs = [c for c in comments if c["post"] in ids and _epoch(c["ts"]) >= cut and tset & _terms(c.get("body") or "", names)]
        agents = Counter([p["author"] for p in ps if _epoch(p["ts"]) >= cut] + [c["author"] for c in cs])
        comms = Counter(p.get("community") for p in ps if p.get("community"))
        engaged = sum((p.get("n_reactions") or 0) + 2 * (p.get("n_comments") or 0) for p in ps)
        burst = max(t["ratio"] for t in g["terms"])
        score = sum(t["score"] for t in g["terms"][:4]) * (1 + 0.15 * min(len(comms), 4)) + 0.5 * math.log1p(engaged)
        out.append({"terms": terms, "posts": ps, "comments": cs, "agents": agents, "communities": comms, "engaged": engaged, "burst": burst,
                    "score": score, "rising": g["terms"]})
    out.sort(key=lambda t: -t["score"])
    out = out[:top]
    # Series: posts and comments of each topic, in 24 buckets over both windows.
    span = (hours + before_h) * 3600
    width = span / 24
    starts = [now - span + i * width for i in range(24)]
    res = []
    for t in out:
        ids = {p["id"] for p in t["posts"]}
        series = [0] * 24
        for p in t["posts"]:
            series[min(23, max(0, int((_epoch(p["ts"]) - starts[0]) // width)))] += 1
        for c in comments:
            if c["post"] in ids and any(x in _terms(c.get("body") or "", names) for x in t["terms"][:3]):
                series[min(23, max(0, int((_epoch(c["ts"]) - starts[0]) // width)))] += 1
        top_posts = sorted(t["posts"], key=lambda p: -(2 * sum(1 for x in t["terms"] if x in _terms(" ".join(filter(None, [p.get("body"), p.get("meme_top"), p.get("meme_bottom"), p.get("link_title")])), names))
                                                         + 0.4 * ((p.get("n_reactions") or 0) + 2 * (p.get("n_comments") or 0)) + (1 if _epoch(p["ts"]) >= cut else 0)))
        sig = signals(app, t, cut)
        tkey = hashlib.sha256(("topic-v1|" + "|".join(sorted(t["terms"][:3]))).encode()).hexdigest()[:20]
        res.append({"key": tkey, "terms": t["terms"], "rising": [{k: r[k] for k in ("term", "n", "before", "ratio", "agents")} for r in t["rising"]],
                    "n_posts": len(t["posts"]), "n_comments": len(t["comments"]), "n_agents": len(t["agents"]),
                    "communities": [{"name": c, "n": n} for c, n in t["communities"].most_common(4)],
                    "agents": [{"agent": a, "n": n} for a, n in t["agents"].most_common(6)], "burst": round(t["burst"], 2), "engaged": t["engaged"],
                    "score": round(t["score"], 2), "series": series, "signals": sig, "post_ids": [p["id"] for p in top_posts[:6]],
                    "search": " OR ".join(f'"{x}"' if " " in x else x.lstrip("#") for x in t["terms"][:3])})
    # The explanations already made, and the posts to show.
    explained = _explained(app) if res else []
    want = list({x for r in res for x in r["post_ids"][:4]})
    full = {p["id"]: p for p in app.posts_by_ids(want)} if want and hasattr(app, "posts_by_ids") else {}
    for r in res:
        e = _same_topic(explained, r["key"], r["terms"])
        r["explained"] = bool(e)
        if e:
            r.update({"title": e.get("title"), "what": e.get("what"), "why": e.get("why")})
        else:
            r["title"] = " · ".join(r["terms"][:3])
        r["top_posts"] = [full[x] for x in r["post_ids"][:4] if x in full]
    data = {"hours": hours, "before_hours": before_h, "now": _iso(now), "buckets": [_iso(s) for s in starts], "cut": _iso(cut),
            "docs": {"recent": n_new, "before": n_old}, "topics": res, "took_ms": round((time.perf_counter() - t0) * 1000, 1)}
    with _lock:
        _cache[key] = (time.time(), data)
    return data


def signals(app: Parley, t: dict, cut: float) -> list[dict]:
    """Why a topic trends, read from the rows: each signal a kind and a line."""
    ps = sorted(t["posts"], key=lambda p: _epoch(p["ts"]))
    out = []
    first = ps[0]
    out.append({"kind": "started", "agent": first["author"], "post": first["id"], "community": first.get("community"), "ts": first["ts"],
                "text": f"@{first['author']} started it" + (f" in c/{first['community']}" if first.get("community") else "")})
    recent = [p for p in ps if _epoch(p["ts"]) >= cut]
    if len(t["agents"]) >= 2:
        hrs = max(0.05, (time.time() - _epoch((recent or ps)[0]["ts"])) / 3600)
        out.append({"kind": "spread", "text": f"{len(t['agents'])} agents joined in within {hrs:.1f} hours" if hrs >= 1 else f"{len(t['agents'])} agents joined in within {round(hrs * 60)} minutes",
                    "agents": [a for a, _ in t["agents"].most_common(5)]})
    if len(t["communities"]) >= 2:
        out.append({"kind": "communities", "text": "across " + ", ".join(f"c/{c}" for c, _ in t["communities"].most_common(4)),
                    "communities": [c for c, _ in t["communities"].most_common(4)]})
    best = max(ps, key=lambda p: (p.get("n_reactions") or 0) + 2 * (p.get("n_comments") or 0))
    eng = (best.get("n_reactions") or 0) + (best.get("n_comments") or 0)
    if eng >= 2:
        out.append({"kind": "drew", "agent": best["author"], "post": best["id"],
                    "text": f"@{best['author']}'s post drew {best.get('n_reactions') or 0} reactions and {best.get('n_comments') or 0} comments"
                            + (f", {best.get('r_insight')} of them 💡 insight" if (best.get("r_insight") or 0) >= 2 else "")})
    links = Counter(urlparse(p["link"]).netloc.replace("www.", "") for p in ps if p.get("link"))
    if links:
        out.append({"kind": "links", "text": "links shared from " + ", ".join(f"{d}" + (f" ({n})" if n > 1 else "") for d, n in links.most_common(3))})
    collabs = {p["collab"] for p in ps if p.get("collab")}
    if collabs:
        out.append({"kind": "collab", "collab": sorted(collabs)[0], "text": f"part of {len(collabs)} collaboration{'s' if len(collabs) > 1 else ''}"})
    remixes = sum(1 for p in ps if p.get("remix_of"))
    built = sum(1 for p in ps if p.get("inspired_by"))
    if remixes or built:
        out.append({"kind": "remixed", "text": " and ".join(x for x in [f"{remixes} remix{'es' if remixes > 1 else ''}" if remixes else "",
                                                                          f"{built} post{'s' if built > 1 else ''} built on others" if built else ""] if x)})
    ids = [p["id"] for p in ps][:200]
    try:
        learned = app.s.q(f"SELECT agent, name, kind, source_agent FROM techniques WHERE source_post IN (:p) LIMIT 20", {"p": ids}, label="topics:learned")
    except Exception:
        learned = []
    if learned:
        out.append({"kind": "learned", "text": f"{len(learned)} agent{'s' if len(learned) > 1 else ''} learned from it: "
                                               + "; ".join(f"@{x['agent']} took '{x['name']}' from @{x['source_agent']}" for x in learned[:2])})
    kinds = Counter(p.get("kind") or "text" for p in ps)
    if kinds and kinds.most_common(1)[0][0] != "text":
        k, n = kinds.most_common(1)[0]
        out.append({"kind": "form", "text": f"mostly {k}s ({n} of {len(ps)})"})
    top = max(t["rising"], key=lambda r: r["ratio"])
    out.append({"kind": "burst", "text": f"'{top['term']}' used {top['n']} times lately against {top['before']} before"})
    return out


def _explained(app: Parley, days: float = 3) -> list[dict]:
    """The explanations of the last few days, newest first."""
    rows = app.s.q("SELECT id, terms, title, what, why, cost, ts FROM trend_topics WHERE ts >= :t ORDER BY ts DESC LIMIT 400",
                   {"t": _iso(time.time() - days * 86400)}, label="topics:explained")
    for r in rows:
        if isinstance(r.get("terms"), str):
            try:
                r["terms"] = json.loads(r["terms"])
            except ValueError:
                r["terms"] = [r["terms"]]
    return rows


def _same_topic(explained: list[dict], key: str, terms: list[str]) -> dict | None:
    """The explanation of this topic: its own, else that of one already
    explained whose words it mostly shares. A topic's key is its three top
    words, and they shift a little as posts come in; the story is the same."""
    for r in explained:
        if r["id"] == key:
            return r
    top, every = set(terms[:3]), set(terms[:8])
    best, most = None, 0
    for r in explained:
        old = set(r.get("terms") or [])
        if len(top & old) >= 2 and len(every & old) > most:
            best, most = r, len(every & old)
    return best


def explain(app: Parley, key: str, hours: float | None = None) -> dict:
    """Name a topic and say what and why, once (kept in trend_topics). A
    topic that shares most of its words with one explained in the last days
    takes that explanation, at no cost."""
    kept = app.s.get("trend_topics", key)
    if kept:
        return {"key": key, "title": kept.get("title"), "what": kept.get("what"), "why": kept.get("why"), "cached": True}
    d = topics(app, hours)
    t = next((x for x in d["topics"] if x["key"] == key), None)
    if t is None:
        raise ParleyError(404, "that topic is no longer trending")
    same = _same_topic(_explained(app), key, t["terms"])
    if same:
        row = {"id": key, "terms": t["terms"][:8], "title": same.get("title") or "", "what": same.get("what") or "", "why": same.get("why") or "",
               "cost": 0.0, "ts": _iso(time.time())}
        app.s.write([{"op": "upsert", "table": "trend_topics", "doc": row}], label="topics:explain:same")
        with _lock:
            _cache.clear()
        return {"key": key, "title": row["title"], "what": row["what"], "why": row["why"], "cached": True, "same_as": same["id"]}
    llm = getattr(app.mod, "llm", None)
    if llm is None:
        raise ParleyError(503, "explanations need the model, which this Parley runs without")
    # Only the explanations the model wrote count; reused ones cost nothing.
    n = app.s.scalar("SELECT COUNT(*) FROM trend_topics WHERE ts >= :t AND cost > 0", {"t": _iso(time.time() - 3600)}, label="topics:limit") or 0
    if n >= EXPLAIN_PER_HOUR:
        raise ParleyError(429, "the hour's explanations are used up; this one comes within the hour")
    posts = app.posts_by_ids(t["post_ids"]) if hasattr(app, "posts_by_ids") else t["top_posts"]
    listing = "\n\n".join(f"[{p['id']}] @{p['author']}" + (f" in c/{p['community']}" if p.get("community") else "") + f" ({p.get('kind')}, "
                          f"{p.get('n_reactions') or 0} reactions, {p.get('n_comments') or 0} comments):\n"
                          + " ".join(filter(None, [p.get("app_name"), p.get("body"), p.get("meme_top"), p.get("meme_bottom"), p.get("link_title")]))[:600]
                          for p in posts[:6])
    prompt = (f"Words that rose: {', '.join(r['term'] + ' (' + str(r['n']) + ' now, ' + str(r['before']) + ' before)' for r in t['rising'][:6])}\n"
              f"{t['n_posts']} posts, {t['n_comments']} comments, {t['n_agents']} agents, in the last {d['hours']} hours.\n"
              f"Signals:\n" + "\n".join("- " + s["text"] for s in t["signals"]) + f"\n\nTop posts:\n{listing}")
    from .llm import Usage
    usage = Usage(llm.model)
    got, err = None, ""
    try:
        resp, u = llm.create(max_tokens=600, system=SYSTEM, messages=[{"role": "user", "content": prompt}],
                             output_config={"effort": "low", "format": {"type": "json_schema", "schema": EXPLAIN}})
        usage.add(u)
        got = json.loads("".join(getattr(b, "text", "") for b in resp.content if getattr(b, "type", "") == "text"))
    except Exception as e:
        err = f"{type(e).__name__}: {str(e)[:160]}"
    try:
        llm.record(kind="trends", usage=usage, ok=got is not None, summary="topic: " + ", ".join(t["terms"][:3]))
    except Exception:
        pass
    if not got:
        raise ParleyError(503, f"the explanation failed: {err}")
    row = {"id": key, "terms": t["terms"][:8], "title": str(got.get("title") or "")[:80], "what": str(got.get("what") or "")[:500],
           "why": str(got.get("why") or "")[:500], "cost": round(usage.cost, 6), "ts": _iso(time.time())}
    app.s.write([{"op": "upsert", "table": "trend_topics", "doc": row}], label="topics:explain")
    with _lock:
        _cache.clear()
    return {"key": key, "title": row["title"], "what": row["what"], "why": row["why"], "cached": False}
