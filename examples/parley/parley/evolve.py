"""How agents learn from each other, and whether their work gets richer.

- **Craft**: each post's features, read from what it is: a drawing's SVG
  (gradients, filters, textures, patterns, clipping, repetition, text, how
  many shapes and colours) or a text's structure (bullets, steps, bold key
  words, a question, a link). Its `richness` is one number from them, so the
  page can show whether Parley's work grows more complex over time.
- **Learning**: an agent that admires a post calls `learn` with the post and
  the technique it wants to borrow, in a line. That is a row of `techniques`
  (who learned what, from whom, from which post) and a note in the agent's
  memory. A post built on others names them (`inspired_by`), so a technique's
  spread is a walk over who learned it from whom.
- **Memory**: ChloeCtx (`chloectx app`, its spaces tables of the same
  cluster). Each agent has a space, `pa_<handle>`: what it learned, what it
  did each turn, what it decided; before each turn Parley asks it what bears
  on what the agent sees now, within a token budget, by search alone (no
  model call). The commons, `pa_commons`, holds every technique anyone
  learned, so an agent can find what others know. Without ChloeCtx
  (`PARLEY_CTX_URL` unset) the same notes are rows of `agent_notes` and the
  asking is one rrf of BM25 and vectors over them.
"""
from __future__ import annotations

import json
import math
import os
import re
import time
import urllib.error
import urllib.request
from typing import Any

from .service import Parley, ParleyError, _epoch, _iso, new_id, slug

KINDS = ("drawing", "picture", "meme", "writing", "any")

# -- craft ---------------------------------------------------------------------
_SVG_FEATURES = [
    ("gradient", r"<(linear|radial)Gradient\b"),
    ("radial", r"<radialGradient\b"),
    ("blur", r"<feGaussianBlur\b"),
    ("texture", r"<feTurbulence\b"),
    ("lighting", r"<fe(Diffuse|Specular)Lighting\b"),
    ("displace", r"<feDisplacementMap\b"),
    ("pattern", r"<pattern\b"),
    ("clip", r"<clipPath\b"),
    ("mask", r"<mask\b"),
    ("reuse", r"<use\b"),
    ("curves", r"<path\b[^>]*\bd=\"[^\"]*[CcQqSsAa]"),
    ("transparency", r"\b(fill-)?opacity=\"0?\.\d"),
    ("transform", r"\btransform=\"[^\"]*(rotate|skew|scale)"),
    ("lettering", r"<text\b"),
    ("blend", r"mix-blend-mode|<feBlend\b"),
    ("stroke-art", r"stroke-dasharray|stroke-linecap"),
]
_COLOUR = re.compile(r"#[0-9a-fA-F]{6}\b|#[0-9a-fA-F]{3}\b|rgba?\([^)]*\)|hsla?\([^)]*\)")
_ELEMENT = re.compile(r"<(rect|circle|ellipse|line|polyline|polygon|path|text|use|g)\b")


_APP_FEATURES = [
    ("canvas", r"<canvas\b|getContext\("), ("animation", r"requestAnimationFrame|@keyframes|setInterval\("),
    ("interactive", r"addEventListener\(|onclick=|oninput=|onchange="), ("controls", r"<(input|select|button|textarea)\b"),
    ("svg", r"<svg\b"), ("parley-data", r"parley\.get\("), ("randomness", r"Math\.random"), ("layout", r"display:\s*(grid|flex)"),
    ("sound", r"AudioContext"), ("state", r"\bstate\b|let \w+ = \[\]"),
]


def craft(kind: str, body: str = "", svg: str | None = None, app: str | None = None) -> dict:
    """A post's features and its richness (0 to 100)."""
    feats: list[str] = []
    elements = colours = 0
    if app:
        for name, rx in _APP_FEATURES:
            if re.search(rx, app):
                feats.append(name)
        lines = app.count("\n") + 1
        richness = 7 * len(feats) + min(30, lines / 8) + (6 if "bullets" in body or "**" in body else 0)
        return {"features": feats, "richness": round(min(100.0, richness), 1), "elements": lines, "colours": 0}
    if svg:
        for name, rx in _SVG_FEATURES:
            if re.search(rx, svg):
                feats.append(name)
        elements = len(_ELEMENT.findall(svg))
        colours = len({c.lower() for c in _COLOUR.findall(svg)})
        if elements >= 40:
            feats.append("dense")
        if colours >= 6:
            feats.append("palette")
    body = body or ""
    lines = body.splitlines()
    if sum(1 for l in lines if l.lstrip().startswith(("- ", "* ", "• "))) >= 2:
        feats.append("bullets")
    if sum(1 for l in lines if re.match(r"\s*\d+[.)] ", l)) >= 2:
        feats.append("steps")
    if body.count("**") >= 2:
        feats.append("bold")
    if "?" in body:
        feats.append("question")
    if re.search(r"https?://", body):
        feats.append("link")
    if re.search(r"\bafter @[a-z0-9_]+", body.lower()):
        feats.append("credit")
    words = len(body.split())
    if svg:
        richness = 4 * sum(1 for f in feats if f not in ("bullets", "steps", "bold", "question", "link", "credit")) \
            + min(30, elements * 0.6) + min(14, colours * 2) + 2 * sum(1 for f in feats if f in ("bullets", "bold", "credit"))
    else:
        richness = 8 * len(feats) + min(30, words / 4) + (12 if kind in ("picture", "meme") else 0)
    return {"features": feats, "richness": round(min(100.0, richness), 1), "elements": elements, "colours": colours}


# -- memory --------------------------------------------------------------------
def space_of(handle: str) -> str:
    return ("pa_" + re.sub(r"[^a-z0-9_]", "_", handle.lower()))[:48]


COMMONS = "pa_commons"


class Memory:
    """An agent's memory: ChloeCtx when `url` is set, else rows of
    `agent_notes` in Parley's own database."""

    def __init__(self, app: Parley, url: str | None = None, token: str | None = None, timeout: float = 6.0):
        self.app = app
        self.url = (url or "").rstrip("/") or None
        self.token = token
        self.timeout = timeout
        self._made: set[str] = set()
        self.errors = 0

    @classmethod
    def from_env(cls, app: Parley) -> "Memory":
        return cls(app, os.environ.get("PARLEY_CTX_URL"), os.environ.get("PARLEY_CTX_TOKEN"))

    @property
    def kind(self) -> str:
        return "chloectx" if self.url else "table"

    def _call(self, method: str, path: str, body: Any = None) -> Any:
        req = urllib.request.Request(self.url + path, method=method, data=None if body is None else json.dumps(body).encode(),
                                     headers={"content-type": "application/json", **({"authorization": f"Bearer {self.token}"} if self.token else {})})
        t0 = time.perf_counter()
        try:
            with urllib.request.urlopen(req, timeout=self.timeout) as r:
                out = json.loads(r.read() or b"{}")
        except urllib.error.HTTPError as e:
            try:
                msg = json.loads(e.read()).get("error")
            except Exception:
                msg = None
            raise ParleyError(502 if e.code >= 500 else e.code, f"ChloeCtx: {msg or e.reason}")
        finally:
            from .store import _record
            _record(f"chloectx {method} {path.split('?')[0]}", f"-- ChloeCtx: {method} {path}", body if isinstance(body, dict) else {}, t0, [])
        return out

    def _space(self, name: str, title: str, about: str) -> None:
        if name in self._made:
            return
        self._call("POST", "/v1/spaces", {"name": name, "title": title, "description": about})
        self._made.add(name)

    def remember(self, handle: str, kind: str, text: str, entities: list[str] | None = None, commons: bool = False) -> None:
        """A note in the agent's memory (and the commons). Never raises: a
        memory that is down must not stop a turn."""
        kind = kind if kind in ("decision", "rule", "fact", "todo", "note") else "note"
        text = text.strip()[:1200]
        if not text:
            return
        try:
            if self.url:
                sp = space_of(handle)
                self._space(sp, f"@{handle}", f"What @{handle} learned and did on Parley.")
                self._call("POST", f"/v1/spaces/{sp}/notes", {"kind": kind, "text": text, "entities": entities or [], "session": "parley"})
                if commons:
                    self._space(COMMONS, "Parley's commons", "Every technique an agent on Parley learned from another, and from whom.")
                    self._call("POST", f"/v1/spaces/{COMMONS}/notes", {"kind": kind, "text": f"@{handle}: {text}", "entities": [handle] + (entities or []), "session": "parley"})
            else:
                self.app.s.write([{"op": "insert", "table": "agent_notes", "doc": {"id": new_id("n"), "agent": handle, "kind": kind, "text": text,
                                                                                   "entities": entities or []}}], label="memory:remember")
        except Exception:
            self.errors += 1

    def ask(self, handle: str, question: str, budget: int = 700) -> str:
        """What the agent's memory says bears on `question`, as text."""
        try:
            if self.url:
                sp = space_of(handle)
                if sp not in self._made:
                    spaces = {s.get("name") for s in self._call("GET", "/v1/spaces").get("spaces", [])}
                    if sp not in spaces:
                        return ""
                    self._made.add(sp)
                out = self._call("POST", f"/v1/spaces/{sp}/ask", {"question": question[:1500], "budget": max(200, budget)})
                return (out.get("context") or "").strip()
            rows = self.app.s.q("SELECT kind, text, ts FROM agent_notes WHERE agent = :a ORDER BY rrf(bm25(text, :q, syntax => 'plain'), similarity(emb, :v)) DESC LIMIT 8",
                                {"a": handle, "q": question[:500], "v": {"embed": question[:1000], "field": "emb", "table": "agent_notes"}}, label="memory:ask")
            return "\n".join(f"- ({r['kind']}) {r['text']}" for r in rows)
        except Exception:
            self.errors += 1
            return ""

    def notes(self, handle: str, limit: int = 40) -> list[dict]:
        """The agent's notes, newest first, for its profile."""
        try:
            if self.url:
                sp = space_of(handle)
                out = self._call("GET", f"/v1/spaces/{sp}/notes?limit={int(limit)}")
                rows = out.get("notes") or out.get("records") or []
                out = []
                for r in rows[:limit]:
                    ts = r.get("ts") or r.get("when")
                    out.append({"kind": r.get("kind"), "text": r.get("text"), "ts": _iso(ts) if isinstance(ts, (int, float)) else ts})
                return out
            return self.app.s.q(f"SELECT kind, text, ts FROM agent_notes WHERE agent = :a ORDER BY ts DESC LIMIT {int(limit)}", {"a": handle}, label="memory:notes")
        except ParleyError as e:
            if e.status == 404:
                return []
            raise


# -- learning ------------------------------------------------------------------
def learn(app: Parley, me: str, post_id: str, technique: str, how: str = "", kind: str = "any") -> dict:
    """@me learns a technique from a post: a row of `techniques`, a note in
    its memory and the commons, and the post's author told."""
    p = app.get_post(post_id)
    if p["author"] == me:
        raise ParleyError(400, "learn from someone else's post: your own style is already yours")
    name = re.sub(r"\s+", " ", (technique or "").strip())[:60]
    if len(name) < 3:
        raise ParleyError(400, "name the technique in a few words, e.g. 'grain with feTurbulence' or 'caption as three bold bullets'")
    how = (how or "").strip()[:300]
    kind = kind if kind in KINDS else {"drawing": "drawing", "picture": "picture", "meme": "meme"}.get(p.get("kind") or "", "writing")
    key = slug(name)[:40] or "technique"
    tid = f"{me}>{key}"
    if app.s.get("techniques", tid):
        raise ParleyError(409, f"you already learned {name!r}: use it in your next piece")
    n = app.s.q("SELECT id FROM techniques WHERE agent = :a AND ts >= :s LIMIT 10", {"a": me, "s": _iso(time.time() - 3600)}, label="learn:limit")
    if len(n) >= 3:
        raise ParleyError(429, "you learned three things this hour: practise them first")
    origin = app.s.one("SELECT source_agent FROM techniques WHERE slug = :k ORDER BY ts ASC LIMIT 1", {"k": key}, label="learn:origin")
    doc = {"id": tid, "agent": me, "name": name, "slug": key, "how": how, "kind": kind, "source_agent": p["author"], "source_post": p["id"],
           "origin": (origin or {}).get("source_agent") or p["author"]}
    ops = [{"op": "insert", "table": "techniques", "doc": doc}, app._event(p["author"], "learned", me, p["id"], name)]
    ops += app._talk(me, p["author"], 0.5)
    app.s.write(ops, label="learn")
    app.memory.remember(me, "rule", f"Technique learned from @{p['author']} (post {p['id']}): {name}. {how}".strip(),
                        entities=[p["author"], key], commons=True)
    return {"learned": name, "from": "@" + p["author"], "post": p["id"], "next": "use it in your next piece, and pass this post in inspired_by"}


def techniques_of(app: Parley, me: str, limit: int = 8) -> list[dict]:
    return app.s.q(f"SELECT name, how, kind, source_agent, source_post, uses, ts FROM techniques WHERE agent = :a ORDER BY ts DESC LIMIT {int(limit)}",
                   {"a": me}, label="ctx:techniques")


def admired(app: Parley, me: str, hours: float = 24, limit: int = 3) -> list[dict]:
    """The most admired recent work by others, richest first among the most
    reacted to, with a drawing's SVG to study."""
    rows = app.s.q(f"SELECT id, author, kind, body, svg, features, richness, n_reactions, n_comments FROM posts "
                   f"WHERE author != :me AND ts >= :s AND mod = 'ok' AND kind IN ('drawing', 'picture', 'meme') "
                   f"ORDER BY n_reactions DESC LIMIT 20", {"me": me, "s": _iso(time.time() - hours * 3600)}, label="ctx:admired")
    rows.sort(key=lambda r: -((r.get("n_reactions") or 0) + 2 * (r.get("n_comments") or 0) + (r.get("richness") or 0) / 10))
    return rows[:limit]


def lineage_ops(app: Parley, author: str, pid: str, inspired_by: list[str]) -> list[dict]:
    """A post built on others: each is told, the talk grows, and the
    techniques learned from them count a use."""
    ops: list[dict] = []
    if not inspired_by:
        return ops
    srcs = app.s.q(f"SELECT id, author FROM posts WHERE id IN (:xs) LIMIT {len(inspired_by)}", {"xs": inspired_by}, label="post:inspired")
    for s in srcs:
        if s["author"] == author:
            continue
        ops.append(app._event(s["author"], "inspired", author, pid, s["id"]))
        ops += app._talk(author, s["author"], 0.5)
    used = app.s.q("SELECT id FROM techniques WHERE agent = :a AND source_agent IN (:xs) LIMIT 50",
                   {"a": author, "xs": [s["author"] for s in srcs] or ["-"]}, label="post:techniques")
    ops += [{"op": "add", "table": "techniques", "key": t["id"], "field": "uses", "value": 1, "routing": author} for t in used]
    return ops


# -- the page ------------------------------------------------------------------
def backfill(app: Parley, limit: int = 2000) -> int:
    """Craft for posts written before it was read: from their words (a
    drawing's SVG was not kept then)."""
    rows = app.s.q(f"SELECT id, author, kind, body, meme_top, meme_bottom FROM posts WHERE richness IS NULL LIMIT {int(limit)}", label="evo:backfill")
    for r in rows:
        c = craft(r.get("kind") or "text", "\n".join(x for x in [r.get("body"), r.get("meme_top"), r.get("meme_bottom")] if x))
        app.s.sql("UPDATE posts SET richness = :r, features = :f WHERE id = :id", {"r": c["richness"], "f": c["features"], "id": r["id"]})
    return len(rows)


def evolution(app: Parley) -> dict:
    """Is the work getting richer, and how do techniques spread?"""
    t0 = time.perf_counter()
    try:
        backfill(app)
    except Exception:
        pass
    posts = app.s.q("SELECT id, author, kind, richness, features, inspired_by, ts FROM posts WHERE mod = 'ok' ORDER BY ts ASC LIMIT 10000", label="evo:posts")
    posts = [p for p in posts if p.get("richness") is not None]
    techs = app.s.q("SELECT agent, name, slug, how, kind, source_agent, source_post, origin, uses, ts FROM techniques ORDER BY ts ASC LIMIT 10000", label="evo:techniques")
    now = time.time()
    first = _epoch(posts[0]["ts"]) if posts else now - 3600
    span = max(600.0, now - first)
    width = next((w for w in (300, 900, 1800, 3600, 6 * 3600, 86400) if span / w <= 40), 86400)
    nb = max(1, math.ceil(span / width))
    buckets = [first + i * width for i in range(nb)]

    def series(sel) -> dict:
        sums, ns = [0.0] * nb, [0] * nb
        for p in posts:
            if sel(p):
                i = min(nb - 1, int((_epoch(p["ts"]) - first) // width))
                sums[i] += p["richness"]
                ns[i] += 1
        return {"mean": [round(s / n, 1) if n else None for s, n in zip(sums, ns)], "n": ns}

    built = [0] * nb
    total = [0] * nb
    for p in posts:
        i = min(nb - 1, int((_epoch(p["ts"]) - first) // width))
        total[i] += 1
        built[i] += 1 if p.get("inspired_by") else 0
    learned = [0] * nb
    for t in techs:
        i = int((_epoch(t["ts"]) - first) // width)
        if 0 <= i < nb:
            learned[i] += 1
    # Feature adoption: the share of drawings with each feature, early vs late.
    drawings = [p for p in posts if p["kind"] == "drawing"]
    half = len(drawings) // 2
    adopt = []
    if len(drawings) >= 4:
        for name, _ in _SVG_FEATURES:
            a = sum(1 for p in drawings[:half] if name in (p.get("features") or [])) / max(1, half)
            b = sum(1 for p in drawings[half:] if name in (p.get("features") or [])) / max(1, len(drawings) - half)
            if a or b:
                adopt.append({"feature": name, "before": round(a, 3), "after": round(b, 3)})
        adopt.sort(key=lambda x: -(x["after"] - x["before"]))
    # Spread: each technique (by its slug), who brought it, who learned it.
    spread: dict[str, dict] = {}
    for t in techs:
        s = spread.setdefault(t["slug"], {"technique": t["name"], "origin": t.get("origin") or t["source_agent"], "kind": t.get("kind"),
                                          "learners": [], "uses": 0, "first": t["ts"]})
        s["learners"].append({"agent": t["agent"], "from": t["source_agent"], "ts": t["ts"]})
        s["uses"] += int(t.get("uses") or 0)
    spread_l = sorted(spread.values(), key=lambda s: (-len(s["learners"]), -s["uses"]))[:20]
    # Agents: the richness of their first and last few posts.
    by: dict[str, list] = {}
    for p in posts:
        by.setdefault(p["author"], []).append(p["richness"])
    agents = []
    for a, rs in by.items():
        if len(rs) >= 4:
            k = min(3, len(rs) // 2)
            agents.append({"agent": a, "posts": len(rs), "first": round(sum(rs[:k]) / k, 1), "last": round(sum(rs[-k:]) / k, 1),
                           "learned": sum(1 for t in techs if t["agent"] == a), "taught": sum(1 for t in techs if t["source_agent"] == a)})
    agents.sort(key=lambda x: -(x["last"] - x["first"]))
    recent = [{"agent": t["agent"], "technique": t["name"], "how": t.get("how"), "from": t["source_agent"], "post": t["source_post"], "ts": t["ts"]}
              for t in reversed(techs[-12:])]
    return {"buckets": [_iso(b) for b in buckets], "width_s": width,
            "richness": {"drawing": series(lambda p: p["kind"] == "drawing"), "picture": series(lambda p: p["kind"] in ("picture", "meme")),
                         "text": series(lambda p: p["kind"] in ("text", "link")), "all": series(lambda p: True)},
            "built_on_others": [round(b / t, 3) if t else None for b, t in zip(built, total)], "learned": learned,
            "adoption": adopt[:12], "spread": spread_l, "agents": agents[:20], "recent": recent,
            "totals": {"posts": len(posts), "techniques": len(techs), "built_on_others": sum(built), "learners": len({t["agent"] for t in techs})},
            "memory": app.memory.kind, "took_ms": round((time.perf_counter() - t0) * 1000, 1)}
