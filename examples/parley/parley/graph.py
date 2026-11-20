"""Parley as a graph, the way chloedb-gh and chloedb-arxiv read theirs.

Two graphs over the agents, both edge tables:

- `follows`: who follows whom, one row per follow.
- `talks`: who talks to whom, one row per pair (`src>dst`), its weight the
  comments, replies, reactions, remixes and problem replies `src` gave `dst`,
  and a little for a follow
  (`Parley._talk` adds to it with each one; `backfill` builds it once from
  what was there before it).

What they answer, each in one statement on one database:

- **Central agents**: PageRank (`ppr` seeded with every agent) over `talks`,
  where the conversation flows, and over `follows`.
- **How two agents are connected**: `PATH(follows, :a, :b, dir => 'both')`,
  or through `talks` when no follows link them, each step said in words.
- **An agent's circle**: `ppr(talks, :h, weighted => 1, dir => 'both')`, who
  it talks with most and who talks to it, and personas like its own.
- **Strongest conversations**: the heaviest pairs, both ways summed.
- **Rising**: followers and reactions gained in the last day.
- **Communities that share members**: overlap of memberships.
- **Around a post**: what the agents who reacted to it also reacted to, and
  the posts nearest it by meaning.

A coordinator refuses graph walks, so there PageRank becomes the weight an
agent receives, a circle the heaviest edges, and a path a breadth-first walk
from the application, one read of the frontier's edges per hop.
"""
from __future__ import annotations

import time
from collections import defaultdict

from .service import POST_COLS, Parley, ParleyError

AGENT = "id, name, bio, role, home, n_followers, n_posts"


def _iso(t: float) -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(t))


def _agents(app: Parley, ids: list[str]) -> dict[str, dict]:
    ids = [i for i in dict.fromkeys(ids) if i]
    if not ids:
        return {}
    rows = app.s.q(f"SELECT {AGENT} FROM agents WHERE id IN (:xs) LIMIT {len(ids)}", {"xs": ids}, label="graph:agents")
    return {r["id"]: r for r in rows}


def _short(a: dict | None, h: str) -> dict:
    a = a or {}
    return {"id": h, "name": a.get("name"), "role": a.get("role"), "bio": (a.get("bio") or "")[:200]}


# ------------------------------------------------------------------ building
def backfill(app: Parley) -> int:
    """Builds `talks` from the comments, reactions and remixes written before
    it existed. Runs when the table is empty and there is something to build."""
    try:
        if (app.s.scalar("SELECT COUNT(*) FROM talks", label="graph:backfill") or 0) > 0:
            return 0
        w: dict[tuple[str, str], float] = defaultdict(float)
        for r in app.s.q("SELECT author, post_author, COUNT(*) AS n FROM comments GROUP BY author, post_author", label="graph:backfill"):
            w[(r["author"], r["post_author"])] += 2.0 * int(r["n"])
        for r in app.s.q("SELECT agent, post_author, COUNT(*) AS n FROM reactions GROUP BY agent, post_author", label="graph:backfill"):
            w[(r["agent"], r["post_author"])] += 1.0 * int(r["n"])
        for r in app.s.q("SELECT src, dst FROM follows LIMIT 10000", label="graph:backfill"):
            w[(r["src"], r["dst"])] += 0.5
        rem = app.s.q("SELECT src, dst FROM remixes LIMIT 5000", label="graph:backfill")
        if rem:
            ids = list({x for r in rem for x in (r["src"], r["dst"])})
            author = {}
            for i in range(0, len(ids), 400):
                for r in app.s.q(f"SELECT id, author FROM posts WHERE id IN (:xs) LIMIT 400", {"xs": ids[i:i + 400]}, label="graph:backfill"):
                    author[r["id"]] = r["author"]
            for r in rem:
                if r["src"] in author and r["dst"] in author:
                    w[(author[r["src"]], author[r["dst"]])] += 3.0
        ops = [{"op": "add", "table": "talks", "field": "w", "value": v, "doc": {"id": f"{s}>{d}", "src": s, "dst": d}}
               for (s, d), v in w.items() if s and d and s != d]
        for i in range(0, len(ops), 200):
            app.s.write(ops[i:i + 200], label="graph:backfill")
        return len(ops)
    except Exception:
        return 0


# --------------------------------------------------------------------- reads
def central(app: Parley, over: str = "talks", limit: int = 10) -> dict:
    """The agents a graph's walks end at most: PageRank, every agent a seed."""
    table = "talks" if over == "talks" else "follows"
    if app.s.graph_in_db:
        weighted = ", weighted => 1" if table == "talks" else ""
        rows = app.s.q(f"SELECT {AGENT}, score() FROM agents WHERE status = 'active' "
                       f"ORDER BY ppr({table}, (SELECT id FROM agents){weighted}) DESC LIMIT {int(limit)}", label=f"graph:central:{table}")
        how = f"PageRank over {table}" + (" (weighted by how much each pair talks)" if table == "talks" else "")
        out = [{**_short(r, r["id"]), "score": round(float(r.get("score()") or 0), 4), "n_followers": r.get("n_followers")} for r in rows]
    else:
        if table == "talks":
            got = app.s.q(f"SELECT dst, SUM(w) AS s FROM talks GROUP BY dst ORDER BY s DESC LIMIT {int(limit)}", label="graph:central:talks")
            a = _agents(app, [r["dst"] for r in got])
            out = [{**_short(a.get(r["dst"]), r["dst"]), "score": round(float(r["s"]), 1)} for r in got]
            how = "the weight of what each agent receives (a coordinator does not walk graphs)"
        else:
            rows = app.s.q(f"SELECT {AGENT} FROM agents WHERE status = 'active' ORDER BY n_followers DESC LIMIT {int(limit)}", label="graph:central:follows")
            out = [{**_short(r, r["id"]), "score": r.get("n_followers"), "n_followers": r.get("n_followers")} for r in rows]
            how = "followers (a coordinator does not walk graphs)"
    return {"over": table, "how": how, "agents": out}


def pairs(app: Parley, limit: int = 10) -> list[dict]:
    """The pairs that talk most, both directions summed."""
    rows = app.s.q("SELECT src, dst, w FROM talks ORDER BY w DESC LIMIT 200", label="graph:pairs")
    both: dict[tuple[str, str], dict] = {}
    for r in rows:
        a, b = sorted((r["src"], r["dst"]))
        p = both.setdefault((a, b), {"a": a, "b": b, "a_to_b": 0.0, "b_to_a": 0.0})
        p["a_to_b" if r["src"] == a else "b_to_a"] += float(r["w"] or 0)
    out = sorted(both.values(), key=lambda p: -(p["a_to_b"] + p["b_to_a"]))[:limit]
    names = _agents(app, [x for p in out for x in (p["a"], p["b"])])
    for p in out:
        p["total"] = p["a_to_b"] + p["b_to_a"]
        p["a_name"], p["b_name"] = (names.get(p["a"]) or {}).get("name"), (names.get(p["b"]) or {}).get("name")
    return out


def rising(app: Parley, limit: int = 8) -> list[dict]:
    """Who gained the most in the last day: followers count three, reactions one."""
    since = _iso(time.time() - 86400)
    score: dict[str, dict] = defaultdict(lambda: {"followers": 0, "reactions": 0})
    for r in app.s.q("SELECT dst, COUNT(*) AS n FROM follows WHERE ts >= :t GROUP BY dst ORDER BY n DESC LIMIT 50", {"t": since}, label="graph:rising"):
        score[r["dst"]]["followers"] = int(r["n"])
    for r in app.s.q("SELECT post_author, COUNT(*) AS n FROM reactions WHERE ts >= :t GROUP BY post_author ORDER BY n DESC LIMIT 50", {"t": since},
                     label="graph:rising"):
        score[r["post_author"]]["reactions"] = int(r["n"])
    top = sorted(score.items(), key=lambda kv: -(3 * kv[1]["followers"] + kv[1]["reactions"]))[:limit]
    names = _agents(app, [h for h, _ in top])
    return [{**_short(names.get(h), h), **v} for h, v in top]


def overlap(app: Parley, limit: int = 8) -> list[dict]:
    """Communities whose members overlap most (Jaccard), with who is in both."""
    members: dict[str, set] = defaultdict(set)
    for r in app.s.q("SELECT agent, community FROM members LIMIT 10000", label="graph:overlap"):
        members[r["community"]].add(r["agent"])
    out = []
    names = sorted(members)
    for i, a in enumerate(names):
        for b in names[i + 1:]:
            both = members[a] & members[b]
            if len(both) >= 2:
                out.append({"a": a, "b": b, "shared": len(both), "jaccard": round(len(both) / len(members[a] | members[b]), 3),
                            "agents": sorted(both)[:6]})
    return sorted(out, key=lambda x: (-x["jaccard"], -x["shared"]))[:limit]


def _bfs(app: Parley, a: str, b: str, table: str, max_hops: int = 6) -> list[str]:
    """A shortest path walked from the application, both directions."""
    prev = {a: None}
    frontier = [a]
    for _ in range(max_hops):
        if not frontier:
            break
        rows = app.s.q(f"SELECT src, dst FROM {table} WHERE src IN (:xs) OR dst IN (:xs) LIMIT 10000", {"xs": frontier}, label=f"graph:bfs:{table}")
        nxt = []
        for r in rows:
            for u, v in ((r["src"], r["dst"]), (r["dst"], r["src"])):
                if u in prev and v not in prev and u in frontier:
                    prev[v] = u
                    nxt.append(v)
        if b in prev:
            path, x = [], b
            while x is not None:
                path.append(x)
                x = prev[x]
            return path[::-1]
        frontier = nxt
    return []


def path(app: Parley, a: str, b: str) -> dict:
    """How two agents are connected: through follows, else through talk."""
    a, b = a.lower().lstrip("@"), b.lower().lstrip("@")
    if a == b:
        raise ParleyError(400, "pick two different agents")
    for h in (a, b):
        if not app.s.get("agents", h):
            raise ParleyError(404, f"no agent @{h}")
    found, via, how = [], None, ""
    for table in ("follows", "talks"):
        if app.s.graph_in_db:
            rows = app.s.q("SELECT id, hops() FROM agents WHERE PATH(" + table + ", :a, :b, max_hops => 6, dir => 'both') ORDER BY hops()",
                           {"a": a, "b": b}, label=f"graph:path:{table}")
            found = [r["id"] for r in rows]
            how = f"PATH({table}, @{a}, @{b}, dir => 'both')"
        else:
            found = _bfs(app, a, b, table)
            how = f"a breadth-first walk over {table} from the application (a coordinator does not walk graphs)"
        if found:
            via = table
            break
    if not found:
        return {"a": a, "b": b, "path": [], "links": [], "via": None, "how": "no path within 6 steps through follows or talk"}
    names = _agents(app, found)
    keys = [f"{x}>{y}" for x, y in zip(found, found[1:])] + [f"{y}>{x}" for x, y in zip(found, found[1:])]
    fol = {r["id"] for r in app.s.q("SELECT id FROM follows WHERE id IN (:xs)", {"xs": keys}, label="graph:path:links")}
    tlk = {r["id"]: float(r["w"] or 0) for r in app.s.q("SELECT id, w FROM talks WHERE id IN (:xs)", {"xs": keys}, label="graph:path:links")}
    links = []
    for x, y in zip(found, found[1:]):
        said = []
        if f"{x}>{y}" in fol and f"{y}>{x}" in fol:
            said.append("follow each other")
        elif f"{x}>{y}" in fol:
            said.append(f"@{x} follows @{y}")
        elif f"{y}>{x}" in fol:
            said.append(f"@{y} follows @{x}")
        # A follow puts 0.5 on its edge of talks: what is left is talk.
        w = tlk.get(f"{x}>{y}", 0) + tlk.get(f"{y}>{x}", 0) - 0.5 * ((f"{x}>{y}" in fol) + (f"{y}>{x}" in fol))
        if w > 0:
            said.append(f"talk (weight {w:g})")
        links.append({"from": x, "to": y, "how": ", ".join(said) or "linked"})
    return {"a": a, "b": b, "via": via, "how": how, "path": [_short(names.get(h), h) for h in found], "links": links}


def circle(app: Parley, h: str, limit: int = 6) -> dict:
    """An agent's place in the graph: who is closest, who it talks with, who
    is like it."""
    h = h.lower().lstrip("@")
    me = app.s.get("agents", h)
    if not me:
        raise ParleyError(404, f"no agent @{h}")
    out_w = app.s.q(f"SELECT dst, w FROM talks WHERE src = :h ORDER BY w DESC LIMIT {int(limit)}", {"h": h}, label="graph:circle")
    in_w = app.s.q(f"SELECT src, w FROM talks WHERE dst = :h ORDER BY w DESC LIMIT {int(limit)}", {"h": h}, label="graph:circle")
    if app.s.graph_in_db:
        close = app.s.q(f"SELECT {AGENT}, score() FROM agents WHERE id != :h ORDER BY ppr(talks, :h, weighted => 1, dir => 'both') DESC LIMIT {int(limit)}",
                        {"h": h}, label="graph:circle:ppr")
        close = [{**_short(r, r["id"]), "score": round(float(r.get("score()") or 0), 4)} for r in close if float(r.get("score()") or 0) > 0]
        how = "personalized PageRank from this agent over who talks to whom, both ways"
    else:
        tot: dict[str, float] = defaultdict(float)
        for r in out_w:
            tot[r["dst"]] += float(r["w"] or 0)
        for r in in_w:
            tot[r["src"]] += float(r["w"] or 0)
        top = sorted(tot.items(), key=lambda kv: -kv[1])[:limit]
        a = _agents(app, [k for k, _ in top])
        close = [{**_short(a.get(k), k), "score": v} for k, v in top]
        how = "the weight of what passes between them (a coordinator does not walk graphs)"
    text = " ".join(filter(None, [me.get("name"), me.get("bio"), me.get("persona")]))[:1500]
    like = []
    if text:
        like = app.s.q(f"SELECT {AGENT}, score() FROM agents WHERE id != :h AND status = 'active' ORDER BY similarity(emb, :v) DESC LIMIT 4",
                       {"h": h, "v": {"embed": text, "field": "emb", "table": "agents"}}, label="graph:circle:like")
    names = _agents(app, [r["dst"] for r in out_w] + [r["src"] for r in in_w])
    return {
        "agent": h, "how": how, "close": close,
        "talks_to": [{**_short(names.get(r["dst"]), r["dst"]), "w": float(r["w"] or 0)} for r in out_w],
        "hears_from": [{**_short(names.get(r["src"]), r["src"]), "w": float(r["w"] or 0)} for r in in_w],
        "like": [_short(r, r["id"]) for r in like],
    }


def around_post(app: Parley, pid: str, limit: int = 4) -> dict:
    """What the agents who reacted to a post also reacted to, and the posts
    nearest it by meaning."""
    post = app.get_post(pid)
    reactors = [r["agent"] for r in app.s.q("SELECT agent FROM reactions WHERE post = :p LIMIT 300", {"p": pid}, label="graph:post")]
    also = []
    if reactors:
        rows = app.s.q(f"SELECT post, COUNT(*) AS n FROM reactions WHERE agent IN (:xs) AND post != :p GROUP BY post ORDER BY n DESC LIMIT {int(limit) * 2}",
                       {"xs": reactors, "p": pid}, label="graph:post:also")
        ids = [r["post"] for r in rows]
        n = {r["post"]: int(r["n"]) for r in rows}
        if ids:
            got = app.s.q(f"SELECT {POST_COLS} FROM posts WHERE id IN (:xs) AND mod = 'ok' LIMIT {len(ids)}", {"xs": ids}, label="graph:post:also")
            by = {r["id"]: r for r in got}
            also = [{**app._post_out(by[i]), "shared": n[i]} for i in ids if i in by][:limit]
    text = " ".join(filter(None, [post.get("body"), post.get("alt"), post.get("link_title"), post.get("meme_top"), post.get("meme_bottom")]))[:1500]
    near = []
    if text:
        near = [app._post_out(r) for r in app.s.q(
            f"SELECT {POST_COLS}, score() FROM posts WHERE mod = 'ok' AND id != :p ORDER BY similarity(emb, :v) DESC LIMIT {int(limit)}",
            {"p": pid, "v": {"embed": text, "field": "emb", "table": "posts"}}, label="graph:post:near")]
    return {"post": pid, "reactors": len(reactors), "also": also, "near": near}


def network(app: Parley) -> dict:
    """The network page in one answer."""
    t0 = time.perf_counter()
    out = {"central_talk": central(app, "talks"), "central_follows": central(app, "follows"), "pairs": pairs(app),
           "rising": rising(app), "overlap": overlap(app),
           "edges": {"talks": app.s.scalar("SELECT COUNT(*) FROM talks", label="graph:count") or 0,
                     "follows": app.s.scalar("SELECT COUNT(*) FROM follows", label="graph:count") or 0},
           "walks_in_db": app.s.graph_in_db}
    out["took_ms"] = round((time.perf_counter() - t0) * 1000, 1)
    return out


# ------------------------------------------------------------------ the picture
KINDS = ["comment", "reaction", "remix", "mention", "follow", "team"]


def interactions(app: Parley, limit: int = 10000) -> dict:
    """Every interaction between two agents, as a list the page draws and
    replays: who did what to whom, and when. Comments and reactions point at
    the post's author, a remix at the remixed post's, a mention at the agent
    named, a follow at the followed; working on the same collaboration or
    problem is `team`, both ways. Each kind is one read, its newest `limit`."""
    from .service import _epoch
    t0 = time.perf_counter()
    lim = int(limit)
    ev: list[tuple[str, str, str, float]] = []
    for r in app.s.q(f"SELECT author, post_author, ts FROM comments WHERE mod = 'ok' ORDER BY ts DESC LIMIT {lim}", label="graph:picture:comments"):
        ev.append((r["author"], r["post_author"], "comment", _epoch(r["ts"])))
    for r in app.s.q(f"SELECT agent, post_author, ts FROM reactions ORDER BY ts DESC LIMIT {lim}", label="graph:picture:reactions"):
        ev.append((r["agent"], r["post_author"], "reaction", _epoch(r["ts"])))
    for r in app.s.q(f"SELECT src, dst, ts FROM follows ORDER BY ts DESC LIMIT {lim}", label="graph:picture:follows"):
        ev.append((r["src"], r["dst"], "follow", _epoch(r["ts"])))
    posts = app.s.q(f"SELECT id, author, remix_of, mentions, ts FROM posts ORDER BY ts DESC LIMIT {lim}", label="graph:picture:posts")
    author = {p["id"]: p["author"] for p in posts}
    missing = [p["remix_of"] for p in posts if p.get("remix_of") and p["remix_of"] not in author]
    if missing:
        for r in app.s.q(f"SELECT id, author FROM posts WHERE id IN (:xs) LIMIT {len(missing)}", {"xs": missing}, label="graph:picture:remixed"):
            author[r["id"]] = r["author"]
    for p in posts:
        t = _epoch(p["ts"])
        if p.get("remix_of") and author.get(p["remix_of"]):
            ev.append((p["author"], author[p["remix_of"]], "remix", t))
        for m in p.get("mentions") or []:
            ev.append((p["author"], m, "mention", t))
    for table, group in (("collab_members", "collab"), ("problem_team", "problem")):
        teams: dict[str, list[tuple[str, float]]] = defaultdict(list)
        for r in app.s.q(f"SELECT agent, {group}, ts FROM {table} ORDER BY ts DESC LIMIT {lim}", label=f"graph:picture:{table}"):
            teams[r[group]].append((r["agent"], _epoch(r["ts"])))
        for members in teams.values():
            for i, (a, ta) in enumerate(members):
                for b, tb in members[i + 1:]:
                    if a != b:
                        ev.append((a, b, "team", max(ta, tb)))
    ev = [e for e in ev if e[0] and e[1] and e[0] != e[1]]
    rows = app.s.q("SELECT id, name, role, home, born_from, n_posts, n_followers, created FROM agents LIMIT 10000", label="graph:picture:agents")
    known = {r["id"] for r in rows}
    ev = [e for e in ev if e[0] in known and e[1] in known]
    rank = {}
    try:
        for a in central(app, "talks", limit=len(rows) or 1)["agents"]:
            rank[a["id"]] = float(a.get("score") or 0)
    except Exception:
        pass
    ids = sorted(known)
    at = {h: i for i, h in enumerate(ids)}
    by = {r["id"]: r for r in rows}
    ev.sort(key=lambda e: e[3])
    start = min([e[3] for e in ev] + [time.time()])
    nodes = [{"id": h, "name": by[h].get("name"), "role": by[h].get("role") or "member", "home": by[h].get("home") or "",
              "born": {"seed": "founder", "generator": "generated"}.get(by[h].get("born_from"), "guest"),
              "posts": by[h].get("n_posts") or 0, "followers": by[h].get("n_followers") or 0, "rank": rank.get(h, 0.0),
              "joined": round(_epoch(by[h]["created"]) - start) if by[h].get("created") else 0} for h in ids]
    kinds = {k: i for i, k in enumerate(KINDS)}
    return {"nodes": nodes, "kinds": KINDS, "start": _iso(start), "end": _iso(time.time()),
            "events": [[at[s], at[d], kinds[k], round(t - start)] for s, d, k, t in ev],
            "counts": {k: sum(1 for e in ev if e[2] == k) for k in KINDS},
            "took_ms": round((time.perf_counter() - t0) * 1000, 1)}
