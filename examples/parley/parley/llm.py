"""Calls to Claude, with what each one cost and a cap on a day's spend.

Every call is priced from its usage and written to the `turns` table by the
caller (one row per agent turn, persona or moderation check), and the day's
spend is a SUM over that table, so the cap holds across restarts and across
every copy of the app that shares the database.

The day is a UTC day. `daily_usd` is a hard stop: once the day's spend reaches
it, `create` refuses (BudgetExceeded) until the next day.
"""
from __future__ import annotations

import os
import threading
import time
from dataclasses import dataclass

# Dollars per million tokens: (input, output). Cache writes cost 1.25 times
# input, cache reads a tenth of it.
PRICES = {
    "claude-haiku-5-5": (0.10, 0.50),
    "claude-sonnet-5-5": (2.00, 10.00),
    "claude-opus-5-5": (4.00, 20.00),
}
DEFAULT_MODEL = "claude-haiku-5-5"


class BudgetExceeded(Exception):
    pass


@dataclass
class Usage:
    model: str
    tok_in: int = 0
    tok_out: int = 0
    tok_cached: int = 0
    tok_cache_write: int = 0
    calls: int = 0
    cost: float = 0.0

    def add(self, other: "Usage") -> None:
        self.tok_in += other.tok_in
        self.tok_out += other.tok_out
        self.tok_cached += other.tok_cached
        self.tok_cache_write += other.tok_cache_write
        self.calls += other.calls
        self.cost += other.cost


def price(model: str, usage) -> Usage:
    """A response's usage in tokens and dollars."""
    pin, pout = PRICES.get(model, PRICES["claude-sonnet-5-5"])  # unknown: assume dearer
    tin = getattr(usage, "input_tokens", 0) or 0
    tout = getattr(usage, "output_tokens", 0) or 0
    cr = getattr(usage, "cache_read_input_tokens", 0) or 0
    cw = getattr(usage, "cache_creation_input_tokens", 0) or 0
    cost = (tin * pin + cw * pin * 1.25 + cr * pin * 0.1 + tout * pout) / 1e6
    return Usage(model, tin + cr + cw, tout, cr, cw, 1, cost)


def day_start(t: float | None = None) -> str:
    t = time.time() if t is None else t
    return time.strftime("%Y-%m-%dT00:00:00Z", time.gmtime(t))


class Llm:
    """One client for the app: agents' turns, the persona generator and the
    moderator all spend from the same day's budget."""

    def __init__(self, store, *, model: str = DEFAULT_MODEL, daily_usd: float = 3.0, client=None):
        self.s = store
        self.model = model
        self.daily_usd = float(daily_usd)
        self._client = client
        self._lock = threading.Lock()
        self._pending = 0.0  # spent by calls whose rows are not written yet
        self._cache: tuple[float, float, str] | None = None  # (when read, spent, day)

    @property
    def client(self):
        if self._client is None:
            import anthropic  # only the live app needs the SDK
            self._client = anthropic.Anthropic(max_retries=3, timeout=120)
        return self._client

    @staticmethod
    def available() -> bool:
        return bool(os.environ.get("ANTHROPIC_API_KEY") or os.environ.get("ANTHROPIC_AUTH_TOKEN"))

    # -- the budget -------------------------------------------------------
    def spent_today(self, fresh: bool = False) -> float:
        day = day_start()
        now = time.time()
        with self._lock:
            c = self._cache
            if not fresh and c and c[2] == day and now - c[0] < 5:
                return c[1] + self._pending
        try:
            v = self.s.scalar("SELECT SUM(cost) FROM turns WHERE ts >= :d", {"d": day}, label="spend") or 0.0
        except Exception:
            v = 0.0
        with self._lock:
            self._cache = (now, float(v), day)
            return float(v) + self._pending

    def left_today(self) -> float:
        return max(0.0, self.daily_usd - self.spent_today())

    def check(self, reserve: float = 0.0) -> None:
        if self.spent_today() + reserve >= self.daily_usd:
            raise BudgetExceeded(f"today's model budget (${self.daily_usd:.2f}) is spent; it resets at 00:00 UTC")

    # -- calls ------------------------------------------------------------
    def create(self, *, reserve: float = 0.0, **kw):
        """messages.create with the budget checked first; returns (response, Usage)."""
        self.check(reserve)
        kw.setdefault("model", self.model)
        resp = self.client.messages.create(**kw)
        u = price(kw["model"], resp.usage)
        with self._lock:
            self._pending += u.cost
        return resp, u

    def record(self, *, kind: str, usage: Usage, agent: str | None = None, ok: bool = True, actions: int = 0, summary: str = "") -> None:
        """Write one row of `turns`: what a turn, a persona or a check cost."""
        from .service import new_id
        doc = {"id": new_id("t"), "agent": agent, "kind": kind, "model": usage.model, "cost": round(usage.cost, 6),
               "tok_in": usage.tok_in, "tok_out": usage.tok_out, "tok_cached": usage.tok_cached, "calls": usage.calls,
               "actions": actions, "ok": ok, "summary": (summary or "")[:500]}
        try:
            self.s.write([{"op": "insert", "table": "turns", "doc": doc}], label="turns")
        finally:
            with self._lock:
                self._cache = None
                self._pending = max(0.0, self._pending - usage.cost)
