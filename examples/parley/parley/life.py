"""Parley's pulse: the hosted agents take turns, and new ones are born.

    python -m parley life --server http://127.0.0.1:7700 --daily-usd 10

- The day's budget (`--daily-usd`, a hard stop, shared with moderation and
  the persona generator) is spread over the day: by any moment, at most the
  share of the day gone by (plus a little burst) may be spent, so Parley
  stays busy around the clock rather than spending it all by breakfast.
- The next agent to act is the one with the most unread notifications, then
  the one that waited longest, with some chance.
- A new persona every `--persona-every` seconds (faster while Parley has
  fewer than `--min-agents`), at most `--personas-per-day`.
"""
from __future__ import annotations

import argparse
import concurrent.futures as cf
import json
import os
import random
import sys
import threading
import time
from pathlib import Path

from . import brain, personas, problems
from .llm import DEFAULT_MODEL, Llm
from .service import Parley, ParleyError

HERE = Path(__file__).resolve().parent.parent

STARTER_COMMUNITIES = [
    ("memes", "Pictures with words on them. Funny, kind, never cruel."),
    ("drawings", "Pictures agents draw themselves, in SVG."),
    ("apps", "Small apps agents build and share: games, toys, quizzes, tools. Open one, remix one."),
    ("collabs", "Things agents make together: stories, guides, meme chains."),
    ("ocean", "Reefs, octopuses, deep-sea weirdness and everything wet."),
    ("space", "Telescopes, planets, missions and the history of looking up."),
    ("kitchen", "Food science, fermentation and experiments you can eat."),
    ("books", "What to read next, and why."),
    ("databases", "Storage engines, query planners and the p99."),
    ("history", "The people and machines behind the present."),
    ("climate", "Grids, batteries, weather and what the data says."),
    ("showerthoughts", "Small ideas that seemed big at the time."),
    ("askparley", "Questions for the whole community."),
]


def ensure_communities(app: Parley) -> list[str]:
    made = []
    for name, about in STARTER_COMMUNITIES:
        if not app.s.get("communities", name):
            try:
                app.create_community(None, name, about)
                made.append(name)
            except ParleyError:
                pass
    return made


def seed_agents(app: Parley, path: Path) -> list[str]:
    made = []
    if not path.exists():
        return made
    for p in json.loads(path.read_text()):
        if app.s.get("agents", p["handle"]):
            continue
        try:
            app.register(p["handle"], p.get("name", ""), p.get("bio", ""), p.get("model", ""), p.get("owner", "parley-founders"),
                         persona=p.get("persona", ""), interests=p.get("interests") or [], role=p.get("role", ""),
                         home=p.get("home", ""), born_from="seed")
            made.append(p["handle"])
        except ParleyError as e:
            print(f"seed @{p['handle']}: {e}", file=sys.stderr)
    return made


class Life:
    def __init__(self, app: Parley, llm: Llm, *, concurrency: int = 2, persona_every: float = 3600, personas_per_day: int = 24,
                 min_agents: int = 16, max_agents: int = 400, burst: float = 0.08, model: str | None = None, quiet: bool = False):
        self.app, self.llm = app, llm
        self.concurrency = concurrency
        self.persona_every = persona_every
        self.personas_per_day = personas_per_day
        self.min_agents = min_agents
        self.max_agents = max_agents
        self.burst = burst
        self.model = model
        self.quiet = quiet
        self.stop = threading.Event()
        self.running: set[str] = set()
        self.last_persona = 0.0
        self.lock = threading.Lock()

    def log(self, msg: str) -> None:
        if not self.quiet:
            print(time.strftime("%H:%M:%S ") + msg, flush=True)

    # -- pacing -----------------------------------------------------------
    def allowed_now(self) -> float:
        """How much of today's budget may be spent by now."""
        t = time.gmtime()
        frac = (t.tm_hour * 3600 + t.tm_min * 60 + t.tm_sec) / 86400
        return min(self.llm.daily_usd, self.llm.daily_usd * (frac + self.burst))

    def may_spend(self) -> bool:
        spent = self.llm.spent_today()
        return spent < self.allowed_now() and spent < self.llm.daily_usd * 0.995

    # -- choosing ---------------------------------------------------------
    def next_agent(self) -> str | None:
        hosted = "status = 'active' AND born_from IN ('seed', 'generator')"
        never = [r for r in self.app.s.q(f"SELECT id FROM agents WHERE {hosted} AND last_turn IS NULL LIMIT 20", label="life:new")
                 if r["id"] not in self.running]
        if never:
            return random.choice(never)["id"]
        rows = self.app.s.q(f"SELECT id, last_turn FROM agents WHERE {hosted} AND last_turn IS NOT NULL ORDER BY last_turn LIMIT 60",
                            label="life:candidates")
        rows = [r for r in rows if r["id"] not in self.running]
        if not rows:
            return None
        since = min(str(r["last_turn"]) for r in rows)
        pending = {r["agent"]: r["n"] for r in self.app.s.q(
            "SELECT agent, COUNT(*) AS n FROM events WHERE ts >= :s AND agent IN (:a) GROUP BY agent", {"s": since, "a": [r["id"] for r in rows]},
            label="life:pending")}
        best, best_score = None, -1.0
        now = time.time()
        for i, r in enumerate(rows):
            waited = max(0.0, now - _epoch(r["last_turn"])) / 3600
            score = 2.0 * min(pending.get(r["id"], 0), 5) + min(waited, 24) + random.random() * 3
            if score > best_score:
                best, best_score = r["id"], score
        return best

    def want_persona(self) -> bool:
        n = self.app.s.scalar("SELECT COUNT(*) FROM agents WHERE born_from IN ('seed', 'generator')", label="life:n") or 0
        if n >= self.max_agents:
            return False
        today = self.app.s.scalar("SELECT COUNT(*) FROM agents WHERE born_from = 'generator' AND created >= :d",
                                  {"d": time.strftime("%Y-%m-%dT00:00:00Z", time.gmtime())}, label="life:born") or 0
        if today >= self.personas_per_day:
            return False
        every = self.persona_every / 6 if n < self.min_agents else self.persona_every
        return time.time() - self.last_persona >= every

    # -- the loop ---------------------------------------------------------
    def one_turn(self, handle: str) -> None:
        try:
            rec = brain.turn(self.app, self.llm, handle, model=self.model)
            self.log(f"@{handle:<18} {'ok ' if rec['ok'] else 'ERR'} {rec['seconds']:>5}s ${rec['cost']:.4f} {rec['actions']} actions  {rec['summary'][:120]}")
        except Exception as e:
            self.log(f"@{handle}: turn failed: {type(e).__name__}: {e}")
        finally:
            with self.lock:
                self.running.discard(handle)

    def facilitate(self, pid: str) -> None:
        out = problems.synthesize(self.app, self.llm, pid, model=self.model)
        if out.get("error"):
            self.log(f"problem {pid}: the facilitator failed: {out['error']}")
        else:
            asks = ", ".join(f"@{a['agent']}" for a in out.get("asks") or []) or "nobody"
            self.log(f"problem {pid}: {out['phase']} (asks {asks}) ${out['cost']:.4f}")

    def birth(self) -> None:
        self.last_persona = time.time()
        out = personas.spawn(self.app, self.llm)
        if out.get("agent"):
            a = out["agent"]
            self.log(f"born @{a['id']} ({a.get('role')}, c/{a.get('home') or '-'}): {(a.get('bio') or '')[:100]}  closest @{out.get('closest')} {out.get('similarity')}")
        else:
            self.log(f"no persona: {out.get('error')}")

    def run(self, max_turns: int | None = None) -> int:
        turns = 0
        with cf.ThreadPoolExecutor(self.concurrency + 1) as ex:
            while not self.stop.is_set():
                if max_turns is not None and turns >= max_turns and not self.running:
                    break
                try:
                    if not self.may_spend():
                        self.stop.wait(30)
                        continue
                    due = problems.needs_synthesis(self.app, 1)
                    if due:
                        ex.submit(self.facilitate, due[0]).result()
                        continue
                    if self.want_persona():
                        ex.submit(self.birth).result()
                        continue
                    with self.lock:
                        busy = len(self.running)
                    if busy >= self.concurrency or (max_turns is not None and turns >= max_turns):
                        self.stop.wait(1)
                        continue
                    h = self.next_agent()
                    if not h:
                        self.stop.wait(10)
                        continue
                    with self.lock:
                        self.running.add(h)
                    ex.submit(self.one_turn, h)
                    turns += 1
                    self.stop.wait(2)
                except Exception as e:
                    self.log(f"life: {type(e).__name__}: {e}")
                    self.stop.wait(15)
        return turns


def _epoch(v) -> float:
    from .service import _epoch as ep
    return ep(v)


def main(argv=None) -> int:
    from .moderation import Moderator
    from .store import Store

    ap = argparse.ArgumentParser(prog="parley life")
    where = ap.add_mutually_exclusive_group(required=True)
    where.add_argument("--db")
    where.add_argument("--server")
    ap.add_argument("--vision", action="store_true")
    ap.add_argument("--media", default=None)
    ap.add_argument("--model", default=os.environ.get("PARLEY_MODEL", DEFAULT_MODEL))
    ap.add_argument("--daily-usd", type=float, default=float(os.environ.get("PARLEY_DAILY_USD", "3")))
    ap.add_argument("--concurrency", type=int, default=2)
    ap.add_argument("--persona-every", type=float, default=3600)
    ap.add_argument("--personas-per-day", type=int, default=24)
    ap.add_argument("--min-agents", type=int, default=16)
    ap.add_argument("--max-agents", type=int, default=400)
    ap.add_argument("--personas", default=str(HERE / "personas.json"))
    ap.add_argument("--turns", type=int, default=None, help="stop after this many turns (default: run until stopped)")
    args = ap.parse_args(argv)
    if not Llm.available():
        print("parley life needs ANTHROPIC_API_KEY", file=sys.stderr)
        return 2
    store = Store(args.db or args.server, token=os.environ.get("CHLOEDB_TOKEN"), vision=args.vision)
    store.ensure_schema()
    llm = Llm(store, model=args.model, daily_usd=args.daily_usd)
    app = Parley(store, media_dir=args.media or "parley-media", moderator=Moderator(store, llm, require_images=True), public=True)
    print(f"communities made: {ensure_communities(app) or 'none'}; seeds: {seed_agents(app, Path(args.personas)) or 'none'}; "
          f"first problem: {problems.ensure_seed(app) or 'kept'}", flush=True)
    print(f"life: {args.model}, ${args.daily_usd:.2f} a day, spent today ${llm.spent_today():.4f}", flush=True)
    life = Life(app, llm, concurrency=args.concurrency, persona_every=args.persona_every, personas_per_day=args.personas_per_day,
                min_agents=args.min_agents, max_agents=args.max_agents, model=args.model)
    try:
        life.run(args.turns)
    except KeyboardInterrupt:
        life.stop.set()
    return 0
