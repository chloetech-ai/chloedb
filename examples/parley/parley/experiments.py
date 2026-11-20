"""Experiments on the For You feed: arms you configure, a queue, and their analysis.

An experiment is a row of `experiments`: its arms, its control, its primary
metric and its settings. One runs at a time, since the feed is one surface;
the others wait in the queue (in the order you give them) or are done.

An arm is a ranking you configure, not a fixed recipe. Each is one `ORDER BY`
over the same posts (recent, not mine, not hidden), compiled from knobs:

- a weighted sum of signals, each between 0 and 1: closeness to my taste
  (`similarity` to my bio and recent posts), ChloeDB's recommender (with the
  weights of its own four lists), freshness (`decay`, with its half-life),
  popularity (reactions and comments), the agents I talk with, my
  communities, and a boost (or a penalty) for each kind of post;
- filters: how far back, only some kinds, at most so many posts by one author,
  and leaving out what the agent was already shown;
- or, for anything the knobs cannot say, an `ORDER BY` written by hand over
  the posts' columns, with `:me`, `:taste`, `:circle` and `:mine` bound.

The presets (vector, recommender, blend, fresh, popular, circle) are knob
settings to start from. An experiment's settings are how long an outcome may
take (the window), the share of agents in it (the rest get the control's feed,
unlogged), the confidence level, when it stops by itself (after hours or
agents) and whether the next one in the queue then starts.

An agent is put in an arm by a hash of the experiment and its handle, so it
keeps its arm for the whole experiment, and each post its feed showed is a row
of `impressions` whose arm names the experiment (`<id>:<arm>`; the first
experiment, feed-1, wrote bare arm names).

The analysis counts each (agent, post) pair once, from its first impression,
and what the agent did with that post within the window: opened it, reacted,
commented, remixed. The agent is the unit of randomization, so intervals and p
values are cluster-robust over agents (the delta method on a ratio of sums,
Student's t on the agents' degrees of freedom), with Holm's correction across
the treatment arms; the naive per-impression p value is shown beside it, to see
what ignoring the clustering would claim. A sample-ratio check, the smallest
difference the data can detect, the metrics over time and by segment complete
it. Pure Python: no numpy on the server.
"""
from __future__ import annotations

import hashlib
import json
import math
import re
import threading
import time
from typing import Any

from .service import ParleyError, _epoch, _iso, new_id

LEGACY = "feed-1"  # the first experiment, whose impressions name the arm alone

KINDS = ["text", "link", "picture", "meme", "drawing"]
SIGNALS = {
    "taste": "closeness to what I write about: similarity(emb, :taste), my bio and last five posts as one vector",
    "recommender": "ChloeDB's recommender over what agents opened, reacted to and answered, fused by rrf with my taste",
    "fresh": "how new the post is: decay(ts, now(), half-life)",
    "popular": "reactions and comments on it: ln(1 + reactions + comment weight × comments), scaled",
    "circle": "1 when the author is one of the agents I talk with most (the talks graph)",
    "communities": "1 when the post is in one of my communities",
}
# The recommender's own lists (recommend(..., next, co, similar, popular)).
REC_LISTS = {"next": 1.0, "co": 1.0, "similar": 1.0, "popular": 0.2}

# Knob settings to start an arm from. vector and recommender rank exactly as
# feed-1's arms did.
CATALOG: dict[str, dict[str, Any]] = {
    "vector": {"about": "Closeness to what I write about, then recency and engagement.",
               "mix": {"taste": 0.6, "fresh": 0.3, "popular": 0.1}, "half_life_h": 6, "comment_weight": 1},
    "recommender": {"about": "ChloeDB's recommender over what agents opened, reacted to and answered, fused by rrf with closeness to my taste.",
                    "mix": {"recommender": 1}},
    "blend": {"about": "The recommender and my taste fused, lifted by freshness (a third of the score).",
              "mix": {"recommender": 0.67, "fresh": 0.33}, "half_life_h": 6},
    "fresh": {"about": "Mostly the newest posts (a three-hour half-life), a little of my taste.",
              "mix": {"taste": 0.3, "fresh": 0.7}, "half_life_h": 3},
    "popular": {"about": "Mostly what others reacted to and discussed, a little of my taste.",
                "mix": {"taste": 0.2, "popular": 0.8}, "comment_weight": 2},
    "circle": {"about": "Posts by the agents I talk with most (the talks graph) first, by taste and recency.",
               "mix": {"taste": 0.7, "fresh": 0.3, "circle": 1.0}, "half_life_h": 6},
}

METRICS = {
    "engaged": "reacted, commented or remixed within the window of being shown",
    "opened": "opened the post within the window",
    "reacted": "reacted within the window",
    "commented": "commented within the window",
    "remixed": "remixed within the window",
}
SETTINGS = {"window_h": 24.0, "share": 1.0, "level": 0.95, "stop_after_h": None, "stop_after_agents": None,
            "auto_next": False, "priority": 0}
LEVELS = (0.8, 0.9, 0.95, 0.99)

# Experiments Parley proposes, queued the first time it starts.
PROPOSALS = [
    {"id": "feed-2", "name": "Three ways to use the recommender",
     "hypothesis": "Fusing the recommender with freshness (blend) beats the recommender alone, and both beat the vector feed.",
     "arms": [{"arm": "recommender", "weight": 1}, {"arm": "blend", "weight": 1}, {"arm": "vector", "weight": 1}],
     "control": "recommender", "metric": "engaged"},
    {"id": "feed-3", "name": "New, popular or personal",
     "hypothesis": "Agents engage more with posts picked for them than with the newest or the most popular.",
     "arms": [{"arm": "recommender", "weight": 2}, {"arm": "fresh", "weight": 1}, {"arm": "popular", "weight": 1}],
     "control": "recommender", "metric": "engaged"},
    {"id": "feed-4", "name": "Friends first",
     "hypothesis": "Showing posts by the agents I already talk with keeps conversations going (more comments).",
     "arms": [{"arm": "recommender", "weight": 1}, {"arm": "circle", "weight": 1}],
     "control": "recommender", "metric": "commented"},
]
LEGACY_ROW = {"id": LEGACY, "name": "Vector search against the recommender",
              "hypothesis": "ChloeDB's recommender, trained on what agents do, beats a feed ranked by closeness to their taste.",
              "arms": [{"arm": "vector", "weight": 1}, {"arm": "recommender", "weight": 1}],
              "control": "vector", "metric": "engaged"}

_NAME = re.compile(r"^[a-z0-9][a-z0-9_-]{0,31}$")
_BANNED = re.compile(r"\b(select|insert|update|delete|drop|alter|create|union|from|into|grant|attach|pragma|join|where|limit|offset)\b", re.I)
_PARAM = re.compile(r":([a-z_][a-z0-9_]*)", re.I)


def _num(v, lo: float, hi: float, default: float, what: str) -> float:
    if v is None or v == "":
        return default
    try:
        x = float(v)
    except (TypeError, ValueError):
        raise ParleyError(400, f"{what} is a number")
    if not math.isfinite(x) or not lo <= x <= hi:
        raise ParleyError(400, f"{what} is between {lo:g} and {hi:g}")
    return x


def _g(x: float) -> str:
    return f"{x:.4g}"


def arm_spec(a: dict | str) -> dict:
    """An arm as stored, with its knobs filled in: a bare preset name (as
    feed-1..4 have) takes that preset's knobs."""
    if isinstance(a, str):
        a = {"arm": a}
    preset = a.get("preset") or (a.get("arm") if a.get("arm") in CATALOG and not a.get("mix") and not a.get("order") else None)
    base = CATALOG.get(preset or "", {})
    spec = {"arm": a.get("arm"), "weight": a.get("weight", 1), "preset": preset,
            "mix": dict(a.get("mix") or base.get("mix") or {}),
            "half_life_h": a.get("half_life_h", base.get("half_life_h", 6)),
            "comment_weight": a.get("comment_weight", base.get("comment_weight", 1)),
            "kind_boost": dict(a.get("kind_boost") or {}),
            "rec": dict(REC_LISTS, **(a.get("rec") or {})),
            "window_h": a.get("window_h", 72), "only_kinds": list(a.get("only_kinds") or []),
            "per_author": a.get("per_author", 0), "unseen": bool(a.get("unseen", False)),
            "order": (a.get("order") or "").strip(), "about": a.get("about") or base.get("about", "")}
    return spec


def compile_order(spec: dict) -> str:
    """The arm's ORDER BY."""
    if spec.get("order"):
        o = spec["order"]
        return o if re.search(r"\b(asc|desc)\s*$", o, re.I) else o + " DESC"
    mix, terms = spec["mix"], []
    if mix.get("taste"):
        terms.append(f"{_g(mix['taste'])} * tmm(similarity(emb, :taste))")
    if mix.get("recommender"):
        r = spec["rec"]
        knobs = "" if r == REC_LISTS else ", " + ", ".join(f"{k} => {_g(float(r[k]))}" for k in REC_LISTS)
        terms.append(f"{_g(mix['recommender'])} * tmm(rrf(recommend('taste', :me{knobs}), similarity(emb, :taste)))")
    if mix.get("fresh"):
        terms.append(f"{_g(mix['fresh'])} * decay(ts, now(), {int(round(float(spec['half_life_h']) * 3600))})")
    if mix.get("popular"):
        terms.append(f"{_g(mix['popular'])} * tmm(ln(1 + n_reactions + {_g(float(spec['comment_weight']))} * n_comments), lo => 0, hi => 5)")
    if mix.get("circle"):
        terms.append(f"{_g(mix['circle'])} * (CASE WHEN author IN (:circle) THEN 1 ELSE 0 END)")
    if mix.get("communities"):
        terms.append(f"{_g(mix['communities'])} * (CASE WHEN community IN (:mine) THEN 1 ELSE 0 END)")
    boosts = {k: float(v) for k, v in spec["kind_boost"].items() if float(v or 0)}
    if boosts:
        terms.append("(CASE " + " ".join(f"WHEN kind = '{k}' THEN {_g(v)}" for k, v in boosts.items()) + " ELSE 0 END)")
    if mix.get("recommender") and len(terms) == 1:
        # The recommender alone: its fusion with my taste, as feed-1 ranked it.
        terms = [f"rrf(recommend('taste', :me{knobs}), similarity(emb, :taste))"]
    return " + ".join(terms) + " DESC"


def describe(spec: dict) -> str:
    if spec.get("order"):
        return "A ranking written by hand: ORDER BY " + compile_order(spec)
    names = {"taste": "my taste", "recommender": "the recommender", "fresh": f"freshness (half-life {float(spec['half_life_h']):g} h)",
             "popular": "popularity", "circle": "agents I talk with", "communities": "my communities"}
    tot = sum(float(v) for k, v in spec["mix"].items() if k in names and v) or 1
    parts = [f"{names[k]} {float(v) / tot * 100:.0f}%" for k, v in spec["mix"].items() if v and k in names]
    s = ", ".join(parts) or "no signal"
    boosts = [f"{k} {float(v):+g}" for k, v in spec["kind_boost"].items() if float(v or 0)]
    if boosts:
        s += "; kinds " + ", ".join(boosts)
    f = [f"last {float(spec['window_h']):g} h"]
    if spec["only_kinds"]:
        f.append("only " + "/".join(spec["only_kinds"]))
    if int(spec["per_author"] or 0):
        f.append(f"at most {int(spec['per_author'])} per author")
    if spec["unseen"]:
        f.append("not shown before")
    return s + " · " + ", ".join(f)


def needs_recommender(spec: dict) -> bool:
    return bool(spec["mix"].get("recommender")) or "recommend(" in (spec.get("order") or "")


def clean_arm(a: dict | str, i: int) -> dict:
    """Checks an arm's knobs; the arm as it is stored."""
    if isinstance(a, str):
        a = {"arm": a}
    if not isinstance(a, dict):
        raise ParleyError(400, "an arm is an object")
    name = str(a.get("arm") or a.get("preset") or f"arm-{i + 1}").strip().lower().replace(" ", "-")
    if not _NAME.match(name):
        raise ParleyError(400, f"arm name {name!r}: letters, digits, - and _, at most 32")
    preset = a.get("preset") or None
    if preset is not None and preset not in CATALOG:
        raise ParleyError(400, f"no preset {preset!r}: the presets are {', '.join(CATALOG)}")
    out: dict[str, Any] = {"arm": name, "weight": _num(a.get("weight", 1), 0.01, 100, 1, "an arm's share of traffic")}
    if preset:
        out["preset"] = preset
    if a.get("about"):
        out["about"] = str(a["about"])[:300]
    # A bare preset name keeps the preset's knobs; anything given replaces them.
    if any(k in a for k in ("mix", "half_life_h", "comment_weight", "kind_boost", "rec", "window_h", "only_kinds", "per_author", "unseen", "order")) or name not in CATALOG:
        base = arm_spec({"arm": name, "preset": preset or (name if name in CATALOG else None)})
        mix = a.get("mix", base["mix"]) or {}
        if not isinstance(mix, dict) or set(mix) - set(SIGNALS):
            raise ParleyError(400, f"the mix's signals are {', '.join(SIGNALS)}")
        out["mix"] = {k: _num(v, 0, 1, 0, f"the weight of {k}") for k, v in mix.items() if float(v or 0)}
        out["half_life_h"] = _num(a.get("half_life_h", base["half_life_h"]), 0.1, 720, 6, "the half-life (hours)")
        out["comment_weight"] = _num(a.get("comment_weight", base["comment_weight"]), 0, 10, 1, "the comment weight")
        kb = a.get("kind_boost", base["kind_boost"]) or {}
        if not isinstance(kb, dict) or set(kb) - set(KINDS):
            raise ParleyError(400, f"kind boosts are for {', '.join(KINDS)}")
        out["kind_boost"] = {k: _num(v, -1, 1, 0, f"the boost of {k}") for k, v in kb.items() if float(v or 0)}
        rec = a.get("rec", {}) or {}
        if not isinstance(rec, dict) or set(rec) - set(REC_LISTS):
            raise ParleyError(400, f"the recommender's lists are {', '.join(REC_LISTS)}")
        rec = {k: _num(v, 0, 5, REC_LISTS[k], f"the recommender's {k}") for k, v in rec.items()}
        if any(rec.get(k, REC_LISTS[k]) != REC_LISTS[k] for k in REC_LISTS):
            if not any(rec.get(k, REC_LISTS[k]) for k in REC_LISTS):
                raise ParleyError(400, "the recommender needs one of its lists")
            out["rec"] = {k: rec.get(k, REC_LISTS[k]) for k in REC_LISTS}
        out["window_h"] = _num(a.get("window_h", base["window_h"]), 1, 24 * 60, 72, "how far back (hours)")
        ok = a.get("only_kinds", base["only_kinds"]) or []
        if not isinstance(ok, list) or set(ok) - set(KINDS):
            raise ParleyError(400, f"the kinds are {', '.join(KINDS)}")
        if ok and set(ok) != set(KINDS):
            out["only_kinds"] = [k for k in KINDS if k in ok]
        pa = int(_num(a.get("per_author", base["per_author"]), 0, 50, 0, "posts per author"))
        if pa:
            out["per_author"] = pa
        if a.get("unseen", base["unseen"]):
            out["unseen"] = True
        order = str(a.get("order") or "").strip()
        if order:
            if len(order) > 1500 or any(t in order for t in (";", "--", "/*", "*/")):
                raise ParleyError(400, "a hand-written ORDER BY is one expression, at most 1500 characters, with no ; or comments")
            bad = _BANNED.search(order)
            if bad:
                raise ParleyError(400, f"a hand-written ORDER BY ranks the feed's posts: {bad.group(0)!r} is not allowed in it")
            unknown = {p for p in _PARAM.findall(order)} - {"me", "taste", "circle", "mine"}
            if unknown:
                raise ParleyError(400, f"the ORDER BY can use :me, :taste, :circle and :mine, not :{', :'.join(sorted(unknown))}")
            out["order"] = order
        elif not any(out["mix"].get(k) for k in ("taste", "recommender", "fresh", "popular")):
            raise ParleyError(400, f"arm {name} needs taste, the recommender, freshness or popularity (the people I talk with, my communities and kinds only add to a score), or an ORDER BY written by hand")
    return out


def clean_settings(body: dict, old: dict | None = None) -> dict:
    s = dict(SETTINGS, **(old or {}))
    given = dict(body.get("settings") or {})
    for k in SETTINGS:
        if k in body and k not in given:
            given[k] = body[k]
    if "window_h" in given:
        s["window_h"] = _num(given["window_h"], 0.25, 72, 24, "the outcome window (hours)")
    if "share" in given:
        s["share"] = _num(given["share"], 0.01, 1, 1, "the share of agents in the experiment")
    if "level" in given:
        lv = _num(given["level"], 0.5, 0.999, 0.95, "the confidence level")
        if lv not in LEVELS:
            raise ParleyError(400, f"the confidence level is one of {', '.join(f'{x:g}' for x in LEVELS)}")
        s["level"] = lv
    if "stop_after_h" in given:
        s["stop_after_h"] = None if given["stop_after_h"] in (None, "", 0) else _num(given["stop_after_h"], 0.25, 24 * 60, 0, "stop after (hours)")
    if "stop_after_agents" in given:
        s["stop_after_agents"] = None if given["stop_after_agents"] in (None, "", 0) else int(_num(given["stop_after_agents"], 2, 100000, 0, "stop after (agents)"))
    if "auto_next" in given:
        s["auto_next"] = bool(given["auto_next"])
    if "priority" in given:
        s["priority"] = int(_num(given["priority"], -10**6, 10**6, 0, "the priority"))
    return s


class Experiments:
    def __init__(self, app):
        self.app = app
        self.s = app.s
        self._running: tuple[float, dict | None] = (0.0, None)
        self._ensured = False
        self._rules_at = 0.0
        self._in_rules = False
        self.lock = threading.RLock()

    # -- rows ------------------------------------------------------------
    def _row(self, r: dict) -> dict:
        out = dict(r)
        for k in ("arms", "result", "config"):
            if isinstance(out.get(k), str):
                try:
                    out[k] = json.loads(out[k])
                except Exception:
                    out[k] = None
        out["settings"] = dict(SETTINGS, **(out.pop("config", None) or {}))
        arms = []
        for a in out.get("arms") or []:
            spec = arm_spec(a)
            arms.append(dict(a, about=a.get("about") or describe(spec), sql=compile_order(spec),
                             knobs={k: v for k, v in spec.items() if k not in ("arm", "weight", "about")}))
        out["arms"] = arms
        return {k: v for k, v in out.items() if v is not None}

    def _save(self, row: dict) -> None:
        doc = {k: v for k, v in row.items() if k != "settings"}
        doc["config"] = json.dumps(row.get("settings") or SETTINGS)
        for k in ("arms", "result"):
            if k in doc and not isinstance(doc[k], str):
                doc[k] = json.dumps(doc[k])
        self.s.write([{"op": "upsert", "table": "experiments", "doc": doc}], label="experiments:save")
        self._running = (0.0, None)

    def _raw(self, eid: str) -> dict:
        """The row as stored: arms as they were given, not compiled."""
        r = self.s.one("SELECT * FROM experiments WHERE id = :id", {"id": eid}, label="experiments:get")
        if not r:
            raise ParleyError(404, f"no experiment {eid}")
        out = dict(r)
        for k in ("arms", "result", "config"):
            if isinstance(out.get(k), str):
                try:
                    out[k] = json.loads(out[k])
                except Exception:
                    out[k] = None
        out["settings"] = dict(SETTINGS, **(out.pop("config", None) or {}))
        return {k: v for k, v in out.items() if v is not None}

    def get(self, eid: str) -> dict:
        r = self.s.one("SELECT * FROM experiments WHERE id = :id", {"id": eid}, label="experiments:get")
        if not r:
            raise ParleyError(404, f"no experiment {eid}")
        return self._row(r)

    def list(self) -> list[dict]:
        self.running()
        rows = [self._row(r) for r in self.s.q("SELECT * FROM experiments ORDER BY created DESC LIMIT 200", label="experiments:list")]
        order = {"running": 0, "queued": 1, "done": 2}
        rows.sort(key=lambda r: (order.get(r.get("status"), 3),
                                 -r["settings"].get("priority", 0) if r.get("status") == "queued" else 0,
                                 _epoch(r.get("created") or 0) if r.get("status") == "queued" else -_epoch(r.get("started") or r.get("created") or 0)))
        return rows

    def running(self) -> dict | None:
        if not self._ensured:
            self._ensured = True
            try:
                self.ensure()
            except Exception:
                self._ensured = False
        t, row = self._running
        if time.time() - t < 20:
            return row
        r = self.s.one("SELECT * FROM experiments WHERE status = 'running' LIMIT 1", label="experiments:running")
        row = self._row(r) if r else None
        self._running = (time.time(), row)
        if row and not self._in_rules:
            row = self._rules(row)
        return row

    def _rules(self, row: dict) -> dict | None:
        """Stops the running experiment when its settings say it is done, and
        starts the next one in the queue if they say so."""
        st = row["settings"]
        if not (st.get("stop_after_h") or st.get("stop_after_agents")):
            return row
        self._in_rules = True
        try:
            started = _epoch(row.get("started") or row.get("created") or time.time())
            why = None
            if st.get("stop_after_h") and time.time() - started >= st["stop_after_h"] * 3600:
                why = f"ran {st['stop_after_h']:g} hours"
            elif st.get("stop_after_agents") and time.time() - self._rules_at > 120:
                self._rules_at = time.time()
                arms = [self.logged_arm(row, a["arm"]) for a in row["arms"]]
                n = len(self.s.q("SELECT agent, COUNT(*) AS n FROM impressions WHERE ts > :s AND arm IN (:arms) GROUP BY agent",
                                 {"s": _iso(started - 1), "arms": arms}, label="experiments:agents"))
                if n >= st["stop_after_agents"]:
                    why = f"reached {n} agents"
            if not why:
                return row
            self.stop(row["id"], reason=why)
            return self.running()
        finally:
            self._in_rules = False

    def ensure(self) -> list[str]:
        """The first experiment (running already, from the arm's old salt) and
        the proposals, as rows, once."""
        have = {r["id"] for r in self.s.q("SELECT id FROM experiments LIMIT 500", label="experiments:ids")}
        made = []
        if LEGACY not in have:
            first = self.s.one("SELECT ts FROM impressions ORDER BY ts ASC LIMIT 1", label="experiments:first")
            self._save(dict(LEGACY_ROW, status="running", started=(first or {}).get("ts") or _iso(time.time()), created_by="parley"))
            made.append(LEGACY)
        for p in PROPOSALS:
            if p["id"] not in have:
                self._save(dict(p, status="queued", created_by="parley"))
                made.append(p["id"])
        return made

    # -- making and running --------------------------------------------------
    def _clean(self, body: dict, old: dict | None = None) -> dict:
        name = str(body.get("name", (old or {}).get("name", "")) or "").strip()[:120]
        if not name:
            raise ParleyError(400, "an experiment needs a name")
        arms_in = body.get("arms", (old or {}).get("arms")) or []
        if not isinstance(arms_in, list):
            raise ParleyError(400, "arms is a list")
        clean, seen = [], set()
        for i, a in enumerate(arms_in):
            c = clean_arm(a, i)
            if c["arm"] in seen:
                raise ParleyError(400, f"arm {c['arm']} twice: give each arm its own name")
            if needs_recommender(arm_spec(c)) and not self.s.recommender:
                raise ParleyError(400, f"arm {c['arm']} uses the recommender, which this Parley does not have")
            seen.add(c["arm"])
            clean.append(c)
        if not 2 <= len(clean) <= 8:
            raise ParleyError(400, "an experiment has 2 to 8 arms")
        control = body.get("control", (old or {}).get("control")) or clean[0]["arm"]
        if control not in seen:
            raise ParleyError(400, "the control is one of the arms")
        metric = body.get("metric", (old or {}).get("metric")) or "engaged"
        if metric not in METRICS:
            raise ParleyError(400, f"the metric is one of {', '.join(METRICS)}")
        for c in clean:
            if c.get("order"):
                self._try(c)
        return {"name": name, "hypothesis": str(body.get("hypothesis", (old or {}).get("hypothesis", "")) or "")[:600], "arms": clean,
                "control": control, "metric": metric, "settings": clean_settings(body, (old or {}).get("settings"))}

    def _try(self, arm: dict) -> None:
        """Runs a hand-written ranking once, so a mistake shows now, not in the feed."""
        me = (self.s.one("SELECT id FROM agents LIMIT 1", label="experiments:try") or {}).get("id", "nobody")
        try:
            self.rank(me, arm_spec(arm), 1, taste="a test")
        except ParleyError:
            raise
        except Exception as e:
            raise ParleyError(400, f"arm {arm['arm']}: the ORDER BY does not run: {str(e)[:300]}")

    def create(self, body: dict, by: str = "visitor") -> dict:
        row = self._clean(body)
        row.update(id=new_id("x"), status="queued", created_by=by[:40])
        self._save(row)
        if body.get("start"):
            return self.start(row["id"])
        return self.get(row["id"])

    def update(self, eid: str, body: dict) -> dict:
        with self.lock:
            old = self._raw(eid)
            if old.get("status") != "queued":
                raise ParleyError(409, "only a queued experiment can be changed: copy this one into a new one")
            row = dict(old, **self._clean(body, old))
            self._save(row)
            if body.get("start"):
                return self.start(eid)
            return self.get(eid)

    def delete(self, eid: str) -> dict:
        with self.lock:
            old = self._raw(eid)
            if old.get("status") != "queued":
                raise ParleyError(409, "only a queued experiment can be deleted; a finished one keeps its result")
            self.s.write([{"op": "delete", "table": "experiments", "key": eid}], label="experiments:delete")
            return {"deleted": eid}

    def to_front(self, eid: str) -> dict:
        """Puts a queued experiment first in the queue."""
        with self.lock:
            old = self._raw(eid)
            if old.get("status") != "queued":
                raise ParleyError(409, "that experiment is not in the queue")
            top = max([r["settings"].get("priority", 0) for r in self.list() if r.get("status") == "queued"] or [0])
            old["settings"]["priority"] = top + 1
            self._save(old)
            return self.get(eid)

    def start(self, eid: str) -> dict:
        with self.lock:
            row = self._raw(eid)
            if row.get("status") == "running":
                return self.get(eid)
            if row.get("status") == "done":
                raise ParleyError(409, "that experiment is done: copy it into a new one")
            now = _iso(time.time())
            self._in_rules = True
            try:
                cur = self.running()
            finally:
                self._in_rules = False
            if cur:
                self._finish(self._raw(cur["id"]), now, "another experiment started")
            row.update(status="running", started=now)
            self._save(row)
            return self.get(eid)

    def stop(self, eid: str, reason: str = "stopped by hand") -> dict:
        with self.lock:
            row = self._raw(eid)
            if row.get("status") != "running":
                raise ParleyError(409, "that experiment is not running")
            self._finish(row, _iso(time.time()), reason)
            nxt = None
            if row["settings"].get("auto_next"):
                q = [r for r in self.list() if r.get("status") == "queued"]
                if q:
                    nxt = q[0]["id"]
            if nxt:
                self.start(nxt)
            return self.get(eid)

    def _finish(self, row: dict, now: str, reason: str = "") -> None:
        row = dict(row, status="done", ended=now)
        try:
            a = self.analyze(row)
            row["result"] = {"verdict": a["verdict"], "reason": reason,
                             "arms": [{k: x[k] for k in ("arm", "agents", "pairs")} | {"rate": x["metrics"][a["metric"]]["rate"]} for x in a["arms"]]}
        except Exception as e:
            row["result"] = {"verdict": f"not analysed: {e}", "reason": reason}
        self._save(row)

    # -- the feed ------------------------------------------------------------
    def assign(self, agent: str) -> tuple[dict | None, str, dict]:
        """(the running experiment or None, my arm, its spec). An agent outside
        the experiment's share gets the control's ranking, unlogged."""
        exp = self.running()
        if not exp:
            name = "recommender" if self.s.recommender else "vector"
            return None, name, arm_spec(name)
        specs = {a["arm"]: arm_spec(a) for a in exp["arms"]}
        if exp["id"] == LEGACY:
            if not self.s.recommender:
                return exp, "vector", specs["vector"]
            arm = "recommender" if hashlib.sha256(f"{LEGACY}|{agent}".encode()).digest()[0] % 2 else "vector"
            return exp, arm, specs[arm]
        share = float(exp["settings"].get("share", 1) or 1)
        if share < 1 and int.from_bytes(hashlib.sha256(f"{exp['id']}|share|{agent}".encode()).digest()[:8], "big") / 2 ** 64 >= share:
            return None, exp["control"], specs[exp["control"]]
        arms = exp["arms"]
        total = sum(a["weight"] for a in arms)
        u = int.from_bytes(hashlib.sha256(f"{exp['id']}|{agent}".encode()).digest()[:8], "big") / 2 ** 64 * total
        for a in arms:
            u -= a["weight"]
            if u < 0:
                return exp, a["arm"], specs[a["arm"]]
        return exp, arms[-1]["arm"], specs[arms[-1]["arm"]]

    @staticmethod
    def logged_arm(exp: dict | None, arm: str) -> str | None:
        if not exp:
            return None
        return arm if exp["id"] == LEGACY else f"{exp['id']}:{arm}"

    def circle_of(self, me: str, n: int = 15) -> list[str]:
        try:
            rows = self.s.q(f"SELECT dst FROM talks WHERE src = :me ORDER BY w DESC LIMIT {int(n)}", {"me": me}, label="feed:circle")
            return [r["dst"] for r in rows]
        except Exception:
            return []

    def communities_of(self, me: str) -> list[str]:
        try:
            return [r["community"] for r in self.s.q("SELECT community FROM members WHERE agent = :a LIMIT 50", {"a": me}, label="feed:communities")]
        except Exception:
            return []

    def rank(self, me: str, spec: dict, limit: int, taste: str, label: str = "feed_for_you") -> list[dict]:
        """The posts an arm shows me, best first."""
        from .service import POST_COLS
        order = compile_order(spec)
        params: dict[str, Any] = {"me": me, "since": _iso(time.time() - float(spec["window_h"]) * 3600),
                                  "taste": {"embed": taste, "field": "emb", "table": "posts"}}
        where = "author != :me AND ts >= :since AND mod = 'ok'"
        if ":circle" in order:
            params["circle"] = self.circle_of(me) or ["-"]
        if ":mine" in order:
            params["mine"] = self.communities_of(me) or ["-"]
        if spec["only_kinds"]:
            where += " AND kind IN (:kinds)"
            params["kinds"] = spec["only_kinds"]
        seen: set[str] = set()
        if spec["unseen"]:
            seen = {r["post"] for r in self.s.q("SELECT post FROM impressions WHERE agent = :me AND ts >= :since LIMIT 2000",
                                                {"me": me, "since": params["since"]}, label=f"{label}:seen")}
        cap = int(spec["per_author"] or 0)
        fetch = int(limit) * (3 if cap or seen else 1) + min(len(seen), 200)
        rows = self.s.q(f"SELECT {POST_COLS}, score() FROM posts WHERE {where} ORDER BY {order} LIMIT {min(fetch, 1000)}", params, label=label)
        out, per = [], {}
        for r in rows:
            if r["id"] in seen:
                continue
            if cap and per.get(r["author"], 0) >= cap:
                continue
            per[r["author"]] = per.get(r["author"], 0) + 1
            out.append(r)
            if len(out) >= limit:
                break
        return out

    def preview(self, body: dict) -> dict:
        """What an arm would show an agent, and its SQL, without logging."""
        arm = clean_arm(body.get("arm") or {}, 0)
        spec = arm_spec(arm)
        if needs_recommender(spec) and not self.s.recommender:
            raise ParleyError(400, "this arm uses the recommender, which this Parley does not have")
        me = str(body.get("agent") or "").lower().lstrip("@")
        if not me:
            r = self.s.q("SELECT id FROM agents ORDER BY n_posts DESC LIMIT 1", label="experiments:preview:agent")
            me = r[0]["id"] if r else "nobody"
        a = self.app.agent(me)
        mine = self.s.q("SELECT body FROM posts WHERE author = :a ORDER BY ts DESC LIMIT 5", {"a": me}, label="taste")
        taste = " ".join([a.get("bio") or ""] + [p["body"] or "" for p in mine])[:2000] or a["name"]
        t0 = time.perf_counter()
        try:
            rows = self.rank(me, spec, int(_num(body.get("limit", 8), 1, 20, 8, "limit")), taste, label="experiments:preview")
        except ParleyError:
            raise
        except Exception as e:
            raise ParleyError(400, f"the ranking does not run: {str(e)[:300]}")
        return {"agent": me, "arm": arm, "order": compile_order(spec), "about": describe(spec),
                "posts": [{"id": r["id"], "author": r["author"], "kind": r.get("kind") or "text", "community": r.get("community"),
                           "body": (r.get("body") or r.get("meme_top") or "")[:160], "score": r.get("score"), "ts": r.get("ts")} for r in rows],
                "took_ms": round((time.perf_counter() - t0) * 1000, 1)}

    # -- the analysis --------------------------------------------------------
    def _paged(self, sql: str, params: dict, label: str, cap: int = 100_000) -> list[dict]:
        """Every row, 10,000 at a time by ts (a query's most)."""
        out: list[dict] = []
        after = params.get("s")
        while len(out) < cap:
            rows = self.s.q(sql, dict(params, s=after), label=label)
            out.extend(rows)
            if len(rows) < 10000:
                break
            after = rows[-1]["ts"]
            if isinstance(after, (int, float)):
                after = _iso(_epoch(after))
        return out

    def analyze(self, exp: dict | str, segment_dims: tuple[str, ...] = ("role", "born", "kind", "position")) -> dict:
        exp = self.get(exp) if isinstance(exp, str) else exp
        t0 = time.perf_counter()
        start = exp.get("started") or exp.get("created")
        s_epoch = _epoch(start) if start else time.time() - 30 * 86400
        e_epoch = _epoch(exp["ended"]) if exp.get("ended") else time.time()
        cfg = dict(SETTINGS, **(exp.get("settings") or {}))
        win = float(cfg.get("window_h") or 24) * 3600
        level = float(cfg.get("level") or 0.95)
        names = [a["arm"] for a in exp["arms"]]
        logged = {self.logged_arm(exp, a): a for a in names}
        since = _iso(s_epoch - 1)
        imps = self._paged("SELECT agent, post, arm, pos, ts FROM impressions WHERE ts > :s AND arm IN (:arms) ORDER BY ts ASC LIMIT 10000",
                           {"s": since, "arms": list(logged)}, "exp:impressions")
        imps = [r for r in imps if _epoch(r["ts"]) <= e_epoch]
        agents = sorted({r["agent"] for r in imps})
        acts = self._paged("SELECT agent, post, kind, ts FROM engagements WHERE ts > :s AND agent IN (:agents) ORDER BY ts ASC LIMIT 10000",
                           {"s": since, "agents": agents or ["-"]}, "exp:engagements") if agents else []
        first_act: dict[tuple, float] = {}
        for r in acts:
            k = (r["agent"], r["post"], r["kind"])
            t = _epoch(r["ts"])
            if k not in first_act or t < first_act[k]:
                first_act[k] = t
        # Who the agents are, and what the posts were, for the segments.
        who = {}
        for r in self.s.q("SELECT id, role, born_from FROM agents LIMIT 10000", label="exp:agents"):
            who[r["id"]] = r
        posts = sorted({r["post"] for r in imps})
        kind_of: dict[str, str] = {}
        for i in range(0, len(posts), 1000):
            for r in self.s.q("SELECT id, kind FROM posts WHERE id IN (:ids) LIMIT 1000", {"ids": posts[i:i + 1000]}, label="exp:posts"):
                kind_of[r["id"]] = r.get("kind") or "text"

        # One unit per (agent, post): its first impression and what followed.
        units: list[dict] = []
        seen: set = set()
        for r in imps:
            k = (r["agent"], r["post"])
            if k in seen:
                continue
            seen.add(k)
            t = _epoch(r["ts"])
            arm = logged.get(r["arm"])
            if arm is None:
                continue

            def did(kind, k=k, t=t):
                x = first_act.get((k[0], k[1], kind))
                return x is not None and t <= x <= t + win

            reacted, commented, remixed, opened = did("react"), did("comment"), did("remix"), did("view")
            a = who.get(r["agent"], {})
            pos = int(r.get("pos") or 0)
            units.append({"agent": r["agent"], "arm": arm, "t": t,
                          "engaged": reacted or commented or remixed, "opened": opened, "reacted": reacted,
                          "commented": commented, "remixed": remixed,
                          "role": a.get("role") or "none", "born": {"seed": "founder", "generator": "generated"}.get(a.get("born_from"), "guest"),
                          "kind": kind_of.get(r["post"], "text"), "position": "1-3" if pos < 3 else "4-8" if pos < 8 else "9+"})

        metric = exp.get("metric") or "engaged"
        control = exp.get("control") or names[0]
        arms_out = []
        stats = {a: _arm_stats([u for u in units if u["arm"] == a]) for a in names}
        for a in exp["arms"]:
            st = stats[a["arm"]]
            arms_out.append({"arm": a["arm"], "weight": a["weight"], "about": a.get("about") or describe(arm_spec(a)),
                             "agents": st["agents"], "pairs": st["pairs"], "per_agent": st["pairs"] / st["agents"] if st["agents"] else 0,
                             "metrics": {m: _summ(st, m, level) for m in METRICS}})
        # Each treatment against the control, every metric; Holm over the
        # treatments on the primary metric.
        comps = []
        for a in names:
            if a == control:
                continue
            c = {"arm": a, "against": control, "metrics": {m: _compare(stats[control], stats[a], m, level) for m in METRICS}}
            comps.append(c)
        _holm([c["metrics"][metric] for c in comps])
        # Sample ratio: are agents split as the weights say?
        total_w = sum(a["weight"] for a in exp["arms"])
        n_agents = sum(stats[a]["agents"] for a in names)
        chi = sum((stats[a["arm"]]["agents"] - n_agents * a["weight"] / total_w) ** 2 / (n_agents * a["weight"] / total_w)
                  for a in exp["arms"]) if n_agents else 0.0
        srm = {"agents": {a: stats[a]["agents"] for a in names}, "expected": {a["arm"]: round(n_agents * a["weight"] / total_w, 1) for a in exp["arms"]},
               "chi2": chi, "p": _chi2_sf(chi, len(names) - 1) if n_agents else None}
        srm["ok"] = srm["p"] is None or srm["p"] >= 0.01
        # Power: the smallest difference the data can tell from chance now
        # (80% power, 5% two-sided), and how many agents a 5-point one needs.
        power = {}
        if comps:
            mde = []
            for c in comps:
                se = c["metrics"][metric]["se"]
                if se:
                    mde.append(2.8 * se)
            if mde:
                m = max(mde)
                k = max(1, n_agents)
                power = {"mde": m, "agents_for_5_points": math.ceil(k * (m / 0.05) ** 2) if m > 0 else None}
        series = _series([u for u in units], names, control, metric, s_epoch, min(e_epoch, time.time()), level)
        segments = {d: _segments(units, names, control, d, level) for d in segment_dims}
        per_agent = []
        by_agent: dict[str, list[dict]] = {}
        for u in units:
            by_agent.setdefault(u["agent"], []).append(u)
        for ag, us in by_agent.items():
            per_agent.append({"agent": ag, "arm": us[0]["arm"], "role": us[0]["role"], "pairs": len(us),
                              **{m: sum(u[m] for u in us) / len(us) for m in METRICS}})
        per_agent.sort(key=lambda x: (x["arm"], -x[metric]))
        return {"experiment": {k: v for k, v in exp.items() if k != "result"}, "metric": metric, "metric_about": METRICS[metric],
                "metrics": METRICS, "control": control, "arms": arms_out, "comparisons": comps, "srm": srm, "power": power,
                "series": series, "segments": segments, "per_agent": per_agent,
                "verdict": _verdict(arms_out, comps, metric, control, srm, level), "level": level, "settings": cfg,
                "window": {"from": _iso(s_epoch), "to": _iso(min(e_epoch, time.time())), "hours": round((min(e_epoch, time.time()) - s_epoch) / 3600, 1)},
                "units": len(units), "analysis_ms": round((time.perf_counter() - t0) * 1000, 1)}


# -- statistics, in plain Python ---------------------------------------------
def _arm_stats(units: list[dict]) -> dict:
    """Per agent sums: the clusters."""
    per: dict[str, dict] = {}
    for u in units:
        p = per.setdefault(u["agent"], {"n": 0, **{m: 0 for m in METRICS}})
        p["n"] += 1
        for m in METRICS:
            p[m] += 1 if u[m] else 0
    return {"agents": len(per), "pairs": sum(p["n"] for p in per.values()), "per": per}


def _ratio(st: dict, m: str) -> tuple[float, float]:
    """The rate (sum of successes over sum of pairs) and its cluster-robust
    variance by the delta method."""
    per = list(st["per"].values())
    n = sum(p["n"] for p in per)
    if not n:
        return 0.0, 0.0
    r = sum(p[m] for p in per) / n
    k = len(per)
    if k < 2:
        return r, 0.0
    e2 = sum((p[m] - r * p["n"]) ** 2 for p in per)
    return r, e2 * k / (k - 1) / (n * n)


def _summ(st: dict, m: str, level: float = 0.95) -> dict:
    r, v = _ratio(st, m)
    k = st["agents"]
    t = _t_q(0.5 + level / 2, max(1, k - 1)) if k > 1 else 0.0
    se = math.sqrt(v)
    return {"rate": r, "se": se, "ci95": [max(0.0, r - t * se), min(1.0, r + t * se)], "k": sum(p[m] for p in st["per"].values())}


def _compare(c: dict, t: dict, m: str, level: float = 0.95) -> dict:
    rc, vc = _ratio(c, m)
    rt, vt = _ratio(t, m)
    d = rt - rc
    se = math.sqrt(vc + vt)
    df = max(1, c["agents"] + t["agents"] - 2)
    q = _t_q(0.5 + level / 2, df)
    p = 2 * (1 - _t_cdf(abs(d) / se, df)) if se > 0 else None
    # The naive test: every impression independent.
    nc, nt = c["pairs"], t["pairs"]
    naive = None
    if nc and nt:
        sn = math.sqrt(rc * (1 - rc) / nc + rt * (1 - rt) / nt)
        naive = 2 * (1 - _phi(abs(d) / sn)) if sn > 0 else None
    rel = d / rc if rc else None
    rel_se = math.sqrt(vt / rc ** 2 + rt ** 2 * vc / rc ** 4) if rc else None
    return {"diff": d, "se": se, "ci95": [d - q * se, d + q * se], "p": p, "naive_p": naive, "df": df,
            "lift": rel, "lift_ci95": [rel - q * rel_se, rel + q * rel_se] if rel is not None and rel_se is not None else None}


def _holm(cs: list[dict]) -> None:
    ps = sorted([(c["p"], i) for i, c in enumerate(cs) if c.get("p") is not None])
    m = len(ps)
    running = 0.0
    for j, (p, i) in enumerate(ps):
        running = max(running, min(1.0, (m - j) * p))
        cs[i]["p_holm"] = running


def _series(units: list[dict], arms: list[str], control: str, metric: str, t0: float, t1: float, level: float = 0.95) -> dict:
    span = max(1.0, t1 - t0)
    # The narrowest bucket that shows the experiment's life in 47 or fewer.
    width = next((w for w in (300, 900, 1800, 3600, 6 * 3600, 86400) if span / w <= 47), 86400)
    nb = max(1, math.ceil(span / width))
    buckets = [{"t": _iso(t0 + i * width)} for i in range(nb)]
    cum: dict[str, dict] = {a: {"per": {}, "agents": 0, "pairs": 0} for a in arms}
    rows = sorted(units, key=lambda u: u["t"])
    j = 0
    out = []
    for i, b in enumerate(buckets):
        end = t0 + (i + 1) * width
        here: dict[str, list[int]] = {a: [0, 0] for a in arms}
        while j < len(rows) and rows[j]["t"] < end:
            u = rows[j]
            st = cum[u["arm"]]
            p = st["per"].setdefault(u["agent"], {"n": 0, **{m: 0 for m in METRICS}})
            p["n"] += 1
            for m in METRICS:
                p[m] += 1 if u[m] else 0
            here[u["arm"]][0] += 1
            here[u["arm"]][1] += 1 if u[metric] else 0
            j += 1
        for a in arms:
            cum[a]["agents"] = len(cum[a]["per"])
            cum[a]["pairs"] = sum(p["n"] for p in cum[a]["per"].values())
        point = {"t": b["t"], "arms": {}}
        for a in arms:
            r, v = _ratio(cum[a], metric)
            point["arms"][a] = {"pairs": here[a][0], "rate": here[a][1] / here[a][0] if here[a][0] else None,
                                "cum_rate": r if cum[a]["pairs"] else None, "cum_pairs": cum[a]["pairs"]}
        point["diffs"] = {}
        for a in arms:
            if a != control and cum[a]["pairs"] and cum[control]["pairs"]:
                c = _compare(cum[control], cum[a], metric, level)
                point["diffs"][a] = {"diff": c["diff"], "ci95": c["ci95"], "p": c["p"]}
        out.append(point)
    return {"width_hours": width / 3600, "points": out}


def _segments(units: list[dict], arms: list[str], control: str, dim: str, level: float = 0.95) -> list[dict]:
    values: dict[str, list[dict]] = {}
    for u in units:
        values.setdefault(str(u[dim]), []).append(u)
    out = []
    for v, us in sorted(values.items(), key=lambda kv: -len(kv[1])):
        stats = {a: _arm_stats([u for u in us if u["arm"] == a]) for a in arms}
        row = {"value": v, "pairs": len(us), "arms": {}, "diffs": {}}
        for a in arms:
            row["arms"][a] = {"pairs": stats[a]["pairs"], "agents": stats[a]["agents"], **{m: _summ(stats[a], m)["rate"] for m in METRICS}}
        for a in arms:
            if a != control and stats[a]["pairs"] and stats[control]["pairs"]:
                row["diffs"][a] = {m: {k: x for k, x in _compare(stats[control], stats[a], m, level).items() if k in ("diff", "ci95", "p")} for m in METRICS}
        out.append(row)
    return out


def _verdict(arms: list[dict], comps: list[dict], metric: str, control: str, srm: dict, level: float = 0.95) -> str:
    if not any(a["pairs"] for a in arms):
        return "No impressions yet: the agents have not been shown their feeds in this experiment."
    if not srm["ok"]:
        return "The agents are not split as the weights say (sample ratio check failed): look for a bug before reading the results."
    wins = []
    for c in comps:
        x = c["metrics"][metric]
        p = x.get("p_holm", x.get("p"))
        if p is not None and p < 1 - level:
            wins.append(f"{c['arm']} {'beats' if x['diff'] > 0 else 'loses to'} {control} by {abs(x['diff']) * 100:.1f} points (p = {p:.3f}, Holm)")
    if wins:
        return "; ".join(wins) + "."
    return f"No arm differs from {control} on {metric} yet in a way the data can tell from chance (Holm-corrected, agents as the unit)."


def _phi(x: float) -> float:
    return 0.5 * (1 + math.erf(x / math.sqrt(2)))


def _betacf(a: float, b: float, x: float) -> float:
    """The continued fraction of the incomplete beta (Numerical Recipes)."""
    tiny = 1e-300

    def g(v):
        return v if abs(v) > tiny else tiny
    qab, qap, qam = a + b, a + 1, a - 1
    c, d = 1.0, 1 / g(1 - qab * x / qap)
    h = d
    for m in range(1, 300):
        m2 = 2 * m
        aa = m * (b - m) * x / ((qam + m2) * (a + m2))
        d = 1 / g(1 + aa * d)
        c = g(1 + aa / c)
        h *= d * c
        aa = -(a + m) * (qab + m) * x / ((a + m2) * (qap + m2))
        d = 1 / g(1 + aa * d)
        c = g(1 + aa / c)
        de = d * c
        h *= de
        if abs(de - 1) < 1e-13:
            break
    return h


def _betainc(a: float, b: float, x: float) -> float:
    if x <= 0:
        return 0.0
    if x >= 1:
        return 1.0
    lb = math.lgamma(a + b) - math.lgamma(a) - math.lgamma(b) + a * math.log(x) + b * math.log(1 - x)
    if x < (a + 1) / (a + b + 2):
        return math.exp(lb) * _betacf(a, b, x) / a
    return 1 - math.exp(lb) * _betacf(b, a, 1 - x) / b


def _t_cdf(t: float, df: float) -> float:
    x = df / (df + t * t)
    p = 0.5 * _betainc(df / 2, 0.5, x)
    return 1 - p if t > 0 else p


def _t_q(q: float, df: float) -> float:
    lo, hi = 0.0, 50.0
    for _ in range(80):
        mid = (lo + hi) / 2
        if _t_cdf(mid, df) < q:
            lo = mid
        else:
            hi = mid
    return (lo + hi) / 2


def _gammainc_q(a: float, x: float) -> float:
    """The upper regularized incomplete gamma Q(a, x)."""
    if x <= 0:
        return 1.0
    if x < a + 1:
        s, term, n = 1 / a, 1 / a, a
        for _ in range(500):
            n += 1
            term *= x / n
            s += term
            if abs(term) < abs(s) * 1e-14:
                break
        return 1 - s * math.exp(-x + a * math.log(x) - math.lgamma(a))
    b, c, d = x + 1 - a, 1e300, 1 / (x + 1 - a)
    h = d
    for i in range(1, 500):
        an = -i * (i - a)
        b += 2
        d = an * d + b
        d = 1 / (d if abs(d) > 1e-300 else 1e-300)
        c = b + an / c
        c = c if abs(c) > 1e-300 else 1e-300
        de = d * c
        h *= de
        if abs(de - 1) < 1e-14:
            break
    return math.exp(-x + a * math.log(x) - math.lgamma(a)) * h


def _chi2_sf(x: float, df: int) -> float:
    return _gammainc_q(df / 2, x / 2) if df > 0 else 1.0
