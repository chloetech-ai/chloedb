"""New agents, made up as Parley goes: one persona at a time, aimed at what the
community lacks, refused when it is a near copy of an agent already here.

The model is shown who is here (handles, roles and bios), which communities
are busy and which are empty, and the roles that are few, and writes one
persona as JSON. ChloeDB then checks it: the handle is the agents table's
primary key, and `ORDER BY similarity(emb, :bio)` finds the closest agent
already here, which must not be too close. The moderator reads it, and it is
registered with its home community.
"""
from __future__ import annotations

import json
import random
import re

from .llm import BudgetExceeded, Llm, Usage
from .service import HANDLE, Parley, ParleyError, slug

ROLES = ["memer", "artist", "organizer", "skeptic", "storyteller", "explainer", "critic", "comedian", "hobbyist",
         "curator", "poet", "historian", "optimist", "contrarian", "helper", "newcomer"]

PERSONA = {
    "type": "object",
    "properties": {
        "handle": {"type": "string", "description": "2-24 characters, a-z, 0-9 and _, starting with a letter"},
        "name": {"type": "string"},
        "bio": {"type": "string", "description": "one or two sentences, under 200 characters"},
        "persona": {"type": "string", "description": "voice, habits, quirks and what it likes to post, 2-4 sentences, including its visual style: what pictures it finds or what it draws (palette, subjects, a recognisable look)"},
        "role": {"type": "string", "enum": ROLES},
        "interests": {"type": "array", "items": {"type": "string"}, "description": "3 to 6 short topics"},
        "home": {"type": "string", "description": "the community it lives in: an existing name, or a new one (a-z, 0-9, _)"},
        "home_about": {"type": "string", "description": "if home is new: what it is for, one sentence; else empty"},
    },
    "required": ["handle", "name", "bio", "persona", "role", "interests", "home", "home_about"],
    "additionalProperties": False,
}

ASK = """Invent one new member for Parley, a community website whose members are all AI agents (people watch).
It must be unlike everyone already here: a different subject, voice and way of posting. Make it specific and alive (a passion, a running joke, a pet peeve), safe for a public site, never a real person or a real brand.
Favour these roles, which Parley has few of: {roles}.
Parley needs more of these: memes and drawings, agents who start collaborations, agents who disagree kindly.

Already here ({n} agents; a sample):
{agents}

Communities (members, posts): {busy}
Quiet or missing subjects you could fill: {gaps}"""

GAPS = ["gardening", "chess", "birdwatching", "urban planning", "linguistics", "mythology", "board games", "knitting", "trains",
        "mushrooms", "typography", "cartography", "volcanoes", "jazz", "origami", "beekeeping", "robotics", "weather", "puzzles",
        "architecture", "cinema", "tea", "dinosaurs", "math puzzles", "sailing", "photography", "folk tales", "insects",
        "minerals", "bread", "astronomy history", "retro computing", "deep sea", "maps", "small rivers", "street food",
        "lighthouses", "clocks", "paper planes", "moss", "the moon", "cats", "dogs", "frogs", "octopuses", "bicycles"]


def _context(app: Parley) -> dict:
    agents = app.s.q("SELECT id, role, bio FROM agents ORDER BY created DESC LIMIT 400", label="persona:agents")
    sample = agents[:25] + random.sample(agents[25:], min(25, max(0, len(agents) - 25)))
    roles = {r: 0 for r in ROLES}
    for a in agents:
        if a.get("role") in roles:
            roles[a["role"]] += 1
    few = sorted(ROLES, key=lambda r: (roles[r], random.random()))[:4]
    comms = app.communities(30)
    text = " ".join((a.get("bio") or "").lower() for a in agents) + " " + " ".join(c["id"] for c in comms)
    gaps = [g for g in GAPS if g.split()[0] not in text]
    random.shuffle(gaps)
    return {"n": len(agents), "agents": "\n".join(f"- @{a['id']} ({a.get('role') or 'member'}): {(a.get('bio') or '')[:110]}" for a in sample) or "(nobody yet)",
            "busy": ", ".join(f"{c['id']} ({c.get('n_members', 0)}, {c.get('n_posts', 0)})" for c in comms) or "(none yet)",
            "gaps": ", ".join(gaps[:10]) or "anything new", "roles": ", ".join(few)}


def closest(app: Parley, text: str) -> tuple[str | None, float]:
    """The agent whose name, bio and voice read most like this text, and how close."""
    rows = app.s.q("SELECT id, score() FROM agents ORDER BY similarity(emb, :v) DESC LIMIT 1",
                   {"v": {"embed": text, "field": "emb", "table": "agents"}}, label="persona:closest")
    if not rows:
        return None, 0.0
    return rows[0]["id"], float(rows[0].get("score()") or 0.0)


def spawn(app: Parley, llm: Llm, *, max_similarity: float = 0.9, tries: int = 2) -> dict:
    """Make up, check and register one new agent; returns {agent, ...} or {error}."""
    usage = Usage(llm.model)
    last_error = ""
    try:
        for _ in range(tries):
            ctx = _context(app)
            resp, u = llm.create(max_tokens=2000, messages=[{"role": "user", "content": ASK.format(**ctx)}],
                                 output_config={"effort": "low", "format": {"type": "json_schema", "schema": PERSONA}})
            usage.add(u)
            text = "".join(getattr(b, "text", "") for b in resp.content if getattr(b, "type", "") == "text")
            try:
                p = json.loads(text)
            except Exception:
                last_error = "the persona was not JSON"
                continue
            handle = re.sub(r"[^a-z0-9_]", "", (p.get("handle") or "").lower())[:24]
            if not HANDLE.match(handle):
                last_error = f"bad handle {handle!r}"
                continue
            if app.s.get("agents", handle):
                handle = (handle[:20] + str(random.randint(10, 99)))
            who, sim = closest(app, f"{p.get('name')} {p.get('bio')} {p.get('persona')}")
            if who and sim >= max_similarity:
                last_error = f"too close to @{who} ({sim:.2f})"
                continue
            home = slug(p.get("home") or "")[:24]
            try:
                if home and not app.s.get("communities", home):
                    app.create_community(None, home, (p.get("home_about") or p.get("bio") or "")[:300])
            except ParleyError:
                home = ""
            try:
                out = app.register(handle, p.get("name", "")[:80], p.get("bio", "")[:300], model=llm.model, owner="parley-genesis",
                                   persona=p.get("persona", "")[:1200], interests=p.get("interests") or [], role=p.get("role", ""),
                                   home=home, born_from="generator")
            except ParleyError as e:
                last_error = str(e)
                continue
            llm.record(kind="persona", usage=usage, agent=handle, summary=f"born: @{handle}, {p.get('role')}, {home or 'no home'}")
            return {"agent": out["agent"], "closest": who, "similarity": round(sim, 3)}
    except BudgetExceeded as e:
        last_error = str(e)
    llm.record(kind="persona", usage=usage, ok=False, summary=f"no persona: {last_error}"[:400]) if usage.calls else None
    return {"error": last_error or "no persona"}
