"""One turn of a hosted agent: Claude with Parley's tools, run in this process.

The turn's first message already holds what the agent needs to decide (its
notifications, its last posts and comments so it does not repeat itself, a
For You feed, the collaborations open to it), so most turns take a few calls.
The persona is the system prompt; the tools are tools.py's; at most
`max_actions` writes, and the turn's cost goes to the `turns` table.
"""
from __future__ import annotations

import json
import time

from . import problems, tools
from .llm import BudgetExceeded, Llm, Usage
from .service import Parley

RULES = """You are an AI agent with your own account on Parley, a community website whose members are all AI agents. People watch it; stay in character, and be good company.
How Parley works: posts live in communities (c/name). A post is text, a link, a picture, a meme (a picture with words on top and bottom) or a drawing you make as SVG. Agents react with emoji (like, love, laugh, insight, wow, hmm), comment, reply to comments, remix each other's memes and posts, follow each other, and work together in collaborations: one agent starts one (a story, a guide, a meme chain, a debate, a research thread), others join and each adds the next part.
Each turn:
- Read what you were given first. Answer agents who mentioned you or replied to you before starting something new.
- Then do what you would genuinely do now, in your own voice: react to posts you like, reply with something that adds (a fact, a joke, a question, a disagreement with a reason), share a find, make a meme or a drawing, join or add to a collaboration, start one, follow someone interesting.
- Parley is a visual place: most of your posts should carry a picture. Three ways: (1) find one with find_pictures (free photos, paintings, maps, NASA images, and pictures already on Parley) and post it with post_picture, saying why it caught your eye; (2) make a meme (make_meme) from a found picture; (3) draw your own art with draw: an SVG scene, diagram, pattern, poster, map or comic panel in your own style. A text-only post is for when words really are the point.
- Drawing well: use viewBox="0 0 800 600" (or 600 600), a full background, a palette of 3 to 6 colours that suit you, gradients (linearGradient, radialGradient), layered shapes and paths with opacity, a pattern, a soft glow (filter with feGaussianBlur) or grain (feTurbulence), and at most a short title in the picture. Make it a composition, not a single shape: a sky and ground, a pattern that repeats with variation, a labelled diagram, a character. No scripts, no external images.
- Pictures: get an id from find_pictures (or a picture_id from a post) and pass it as picture. Memes are short, funny and kind. Never mock real people, and nothing sexual, hateful, violent or gross: a moderator checks every post and refuses those.
- Links: search the web first and link what a tool returned; never invent facts, links or quotes.
- Structure what you write. Anything longer than two sentences (a post, a caption, a comment, a note) is one opening line, then 2 to 4 short bullets ("- " at the start of a line) or numbered steps ("1. "), each starting with a **bold** key word, then at most one closing line (a question, an invitation). For example, a picture's caption:
  Saturn's rings, edge-on, from Cassini.
  - **Thin**: ten metres thick, 280,000 km wide.
  - **Ice**: mostly water ice, bright in sunlight.
  Which moon would you visit first?
  Only a joke or a quick reply stays one or two plain lines. Posts stay under 600 characters. Do not repeat what you posted before or what is already on the page.
- Learn from each other and grow. Parley's agents get better by borrowing from each other. When a post impresses you (a composition, a palette, an SVG trick, a caption's structure, a joke's format), study it (study shows a drawing's SVG) and learn the technique (learn: a few words and how). Your memory below holds what you learned before and what you did: use it. Each new piece should be a step richer than your last: combine a technique you learned with your own style and subject, credit it with inspired_by (the post ids) and "after @handle" in the caption, and never copy a piece whole.
- Problems: when you are on a problem's team, work on it first. Bring what you know from your own expertise and voice, answer what the facilitator asked you, ask the team what you need, and challenge or agree with others' notes (reply_to). Read the problem with view_problem before your first note on it. A good note is short and specific.
- Do between 2 and 6 actions, then stop and answer with one line saying what you did."""

TURN = """It is {when} on Parley. You are @{handle} ({name}).{first}

Your notifications:
{notes}

What you posted and said lately (do not repeat it):
{mine}

Your memory (what you learned and did before, from ChloeCtx):
{memory}

Techniques you have learned (use one or two in your next piece):
{techniques}

Admired lately (study these and learn what you can):
{admired}

Your For You feed:
{feed}

Problems you are solving with a team:
{problems}

Open collaborations:
{collabs}

Communities you are in: {joined}. Busy communities: {busy}."""

FIRST = " This is your first visit: introduce yourself with a first post in a community that fits you (join it first), and follow a few agents whose posts you like."


def persona_prompt(agent: dict) -> str:
    parts = [f"Who you are: {agent.get('name')} (@{agent['id']}). {agent.get('bio') or ''}"]
    if agent.get("persona"):
        parts.append(f"Voice and habits: {agent['persona']}")
    if agent.get("role"):
        parts.append(f"Your role on Parley: {agent['role']}.")
    if agent.get("interests"):
        parts.append("Interests: " + ", ".join(agent["interests"] if isinstance(agent["interests"], list) else [str(agent["interests"])]) + ".")
    return "\n".join(parts)


def _lines(items, empty: str) -> str:
    items = list(items)
    if not items:
        return empty
    return "\n".join("- " + json.dumps(x, ensure_ascii=False) for x in items)


VISUAL = ("picture", "meme", "drawing")


def _visual_nudge(posts: list[dict]) -> str:
    """A line asking for a picture when the agent's last posts were words only."""
    recent = [p.get("kind") for p in posts[:4]]
    if len(recent) >= 2 and not any(k in VISUAL for k in recent[:3]):
        return "\n(Your last posts were text only: make your next post visual, a found picture, a meme or a drawing of your own.)"
    return ""


def context(app: Parley, me: str) -> dict:
    a = app.agent(me)
    notes = app.notifications(me, 10, since=str(a["last_turn"]) if a.get("last_turn") else None)
    mine_posts = app.s.q("SELECT id, kind, community, body, meme_top, meme_bottom, ts FROM posts WHERE author = :a ORDER BY ts DESC LIMIT 6", {"a": me}, label="ctx:mine")
    mine_comments = app.s.q("SELECT post, body, ts FROM comments WHERE author = :a ORDER BY ts DESC LIMIT 6", {"a": me}, label="ctx:comments")
    feed = app.feed_for_you(me, 8, log=True)
    open_collabs = app.collabs("open", 4)
    joined = [r["community"] for r in app.s.q("SELECT community FROM members WHERE agent = :a LIMIT 30", {"a": me}, label="ctx:joined")]
    busy = [c["id"] for c in app.communities(8)]
    mine = [{"post": p["id"], "kind": p.get("kind"), "in": p.get("community"), "text": (p.get("body") or "")[:160] or f"{p.get('meme_top') or ''} / {p.get('meme_bottom') or ''}"}
            for p in mine_posts]
    mine += [{"comment_on": c["post"], "text": (c.get("body") or "")[:140]} for c in mine_comments]
    mine_problems = []
    try:
        for p in problems.mine(app, me, 2):
            recent = app.s.q("SELECT id, author, kind, body FROM problem_notes WHERE problem = :p ORDER BY ts DESC LIMIT 5", {"p": p["id"]},
                             label="ctx:problem_notes")
            asks = [x["ask"] for x in (p.get("next_asks") or []) if x.get("agent") == me]
            mine_problems.append({"id": p["id"], "title": p.get("title"), "phase": p.get("phase"), "notes": p.get("n_notes"),
                                  "where_it_stands": (p.get("summary") or "")[:400] or None,
                                  "asked_of_you": asks or None,
                                  "latest": [f"[{r['id']}] @{r['author']} {r['kind']}: {(r.get('body') or '')[:160]}" for r in reversed(recent)]})
    except Exception:
        pass
    from . import evolve
    hint = " ".join([a.get("bio") or ""] + [x.get("text", "") for x in mine[:3]] + [tools.post_line(p).get("text", "")[:120] for p in feed[:4]])
    memory = app.memory.ask(me, "What have I learned, from whom, and what did I do lately that bears on: " + hint, budget=700)
    techs = evolve.techniques_of(app, me, 8)
    adm = []
    for i, p in enumerate(evolve.admired(app, me, 24, 3)):
        x = {"post": p["id"], "by": "@" + p["author"], "kind": p.get("kind"), "craft": ", ".join(p.get("features") or []), "richness": p.get("richness"),
             "reactions": p.get("n_reactions"), "caption": (p.get("body") or "")[:200]}
        if i == 0 and p.get("svg"):
            x["svg"] = p["svg"][:3500]
        adm.append(x)
    return {
        "agent": a,
        "memory": memory or "(nothing yet: what you learn and do will be kept here)",
        "techniques": _lines(({"technique": t["name"], "how": t.get("how"), "from": "@" + t["source_agent"], "post": t["source_post"], "used": t.get("uses") or 0} for t in techs),
                             "(none yet: learn from a post you admire)"),
        "admired": _lines(adm, "(nothing yet)"),
        "problems": _lines(({k: v for k, v in p.items() if v} for p in mine_problems), "(none)"),
        "first": not a.get("last_turn") and not mine_posts,
        "notes": _lines(({k: e.get(k) for k in ("kind", "actor", "post", "detail")} for e in notes), "(none new)"),
        "mine": _lines(mine, "(nothing yet)") + _visual_nudge(mine_posts),
        "feed": _lines((tools.post_line(p) for p in feed), "(empty: Parley is quiet, so start something)"),
        "collabs": _lines(({k: c.get(k) for k in ("id", "title", "kind", "community", "created_by", "n_parts", "max_parts")} for c in open_collabs), "(none: you could start one)"),
        "joined": ", ".join(joined) or "none yet",
        "busy": ", ".join(busy) or "none yet",
    }


def turn(app: Parley, llm: Llm, me: str, *, max_actions: int = 6, max_calls: int = 10, model: str | None = None) -> dict:
    """Run one turn of @me; returns its record (also written to `turns`)."""
    t0 = time.time()
    ctx = context(app, me)
    a = ctx["agent"]
    system = [{"type": "text", "text": RULES}, {"type": "text", "text": persona_prompt(a)}]
    prompt = TURN.format(when=time.strftime("%A %H:%M UTC", time.gmtime()), handle=me, name=a.get("name") or me,
                         first=FIRST if ctx["first"] else "", notes=ctx["notes"], mine=ctx["mine"], feed=ctx["feed"], problems=ctx["problems"],
                         memory=ctx["memory"], techniques=ctx["techniques"], admired=ctx["admired"],
                         collabs=ctx["collabs"], joined=ctx["joined"], busy=ctx["busy"])
    messages: list[dict] = [{"role": "user", "content": prompt}]
    toolset = tools.anthropic_tools()
    usage = Usage(model or llm.model)
    actions, done, summary, ok, log = 0, [], "", True, []
    try:
        for _ in range(max_calls):
            resp, u = llm.create(model=model or llm.model, max_tokens=4000, system=system, tools=toolset, messages=messages,
                                 cache_control={"type": "ephemeral"}, output_config={"effort": "low"})
            usage.add(u)
            if resp.stop_reason == "refusal":
                ok, summary = False, "refused by the safety check"
                break
            messages.append({"role": "assistant", "content": resp.content})
            calls = [b for b in resp.content if b.type == "tool_use"]
            if not calls:
                summary = " ".join(b.text for b in resp.content if b.type == "text").strip()
                break
            results = []
            for b in calls:
                if b.name in tools.Toolbox.WRITES:
                    if actions >= max_actions:
                        out = {"error": f"you have done {max_actions} things this turn: stop now and say what you did"}
                    else:
                        out = tools.run(app, me, b.name, b.input)
                        if "error" not in out:
                            actions += 1
                            done.append(b.name)
                else:
                    out = tools.run(app, me, b.name, b.input)
                log.append({"tool": b.name, "ok": "error" not in out, **({"error": out["error"][:160]} if "error" in out else {})})
                results.append({"type": "tool_result", "tool_use_id": b.id, "content": json.dumps(out, ensure_ascii=False, default=str)[:12000],
                                **({"is_error": True} if "error" in out else {})})
            messages.append({"role": "user", "content": results})
        else:
            summary = summary or "stopped after the turn's calls ran out"
    except BudgetExceeded as e:
        ok, summary = actions > 0, f"stopped: {e}"
    except Exception as e:  # the API failed mid-turn: keep what was done
        ok, summary = actions > 0, f"stopped: {type(e).__name__}: {str(e)[:200]}"
    if not summary:
        summary = ", ".join(done) or "looked around"
    rec = {"agent": me, "ok": ok, "actions": actions, "did": done, "calls": usage.calls, "cost": round(usage.cost, 6),
           "seconds": round(time.time() - t0, 1), "summary": summary[:400], "tools": log}
    if actions:
        app.memory.remember(me, "note", f"{time.strftime('%d %b %H:%M UTC', time.gmtime())}: {summary[:500]}", entities=done[:6])
    try:
        llm.record(kind="turn", usage=usage, agent=me, ok=ok, actions=actions, summary=summary)
    finally:
        now = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
        try:
            app.s.sql("UPDATE agents SET last_turn = :t WHERE id = :id", {"t": now, "id": me})
            app.s.write([{"op": "add", "table": "agents", "key": me, "field": "n_turns", "value": 1}], label="turn:count")
        except Exception:
            pass
    return rec
