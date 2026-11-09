"""One store for every stage: a ChloeDB file in the process, one chloedb-server,
or a coordinator over shards.

`chloedb.open` and `chloedb.connect` give objects with the same methods, so most
of Parley never asks which one it has. What differs is what a cluster refuses:
graph walks (REACHABLE, ppr), and a few write forms. This class keeps the
difference in one place: `graph_in_db` says whether the database can walk the
follow graph itself, and the service walks it hop by hop when it cannot.
"""
from __future__ import annotations

import os
import threading
import time
import urllib.request
from typing import Any, Iterable

import chloedb

from . import schema


class Store:
    def __init__(self, target: str, *, token: str | None = None, vision: bool | None = None, models: str | None = None):
        self.target = target
        self.remote = target.startswith(("http://", "https://"))
        self.lock = threading.Lock()  # only for the in-process writer's bookkeeping
        self.timings: dict[str, list[float]] = {}
        if self.remote:
            self.db = chloedb.connect(target, token=token, timeout=120, retry_for=30)
            health = self.db.health()
            self.role = "coordinator" if _is_coordinator(health) else "server"
        else:
            self.role = "embedded"
            kw: dict[str, Any] = {}
            if models:
                kw["models"] = models
            if os.path.exists(target):
                self.db = chloedb.open(target, **kw)
            else:
                self.db = chloedb.open(target, schema=schema.META, **kw)
        # A coordinator refuses graph queries before asking any shard.
        self.graph_in_db = self.role != "coordinator"
        self.vision = bool(vision)

    # -- schema -----------------------------------------------------------
    def ensure_schema(self) -> list[str]:
        have = {t.get("table") or t.get("name") for t in self.tables()}
        made = []
        if self.remote and "meta" not in have and not have:
            self.db.sql(_create_meta_sql())
            made.append("meta")
        for stmt in schema.statements(vision=self.vision):
            name = stmt.split("EXISTS", 1)[1].split("(", 1)[0].strip()
            if name not in have:
                self.db.sql(stmt)
                made.append(name)
        return made

    def tables(self) -> list[dict]:
        try:
            return list(self.db.tables())
        except Exception:
            return []

    # -- reads ------------------------------------------------------------
    def q(self, sql: str, params: dict | None = None, *, label: str | None = None) -> list[dict]:
        t0 = time.perf_counter()
        res = self.db.query(sql, params or {})
        rows = [dict(r) for r in res]
        self._time(label or sql.split()[0], t0)
        return rows

    def one(self, sql: str, params: dict | None = None, **kw) -> dict | None:
        rows = self.q(sql, params, **kw)
        return rows[0] if rows else None

    def scalar(self, sql: str, params: dict | None = None, **kw) -> Any:
        row = self.one(sql, params, **kw)
        return next(iter(row.values())) if row else None

    def get(self, table: str, key: str) -> dict | None:
        t0 = time.perf_counter()
        row = self.db.get(key, table=table)
        self._time(f"get:{table}", t0)
        return row

    def get_many(self, table: str, keys: Iterable[str]) -> list[dict | None]:
        keys = list(keys)
        if not keys:
            return []
        return list(self.db.get_many(keys, table=table))

    # -- writes -----------------------------------------------------------
    def write(self, ops: list[dict], *, label: str = "write") -> int:
        t0 = time.perf_counter()
        if not self.remote:
            n = self.db.write(ops)
        else:
            try:
                n = self.db.write(ops)
            except chloedb.ChloeDBError as e:
                answer = getattr(e, "answer", None) or {}
                if not answer.get("partial"):
                    raise
                # Some shards took their part: send the rest, once.
                n = self.db.write(ops, idempotency_key=answer["idempotency_key"])
        self._time(label, t0)
        return n

    def sql(self, stmt: str, params: dict | None = None) -> Any:
        return self.db.sql(stmt, params or {})

    def embed(self, text: str, field: str | None = None, table: str | None = None) -> list[float]:
        return self.db.embed(text, field=field, table=table)

    def close(self) -> None:
        try:
            self.db.close()
        except Exception:
            pass

    # -- media ------------------------------------------------------------
    def media_bytes(self, sha256: str, shard_urls: list[str] | None = None) -> bytes | None:
        """The bytes of an image a row names. A server reads them from its media
        store; a coordinator refuses, so ask the shards (one of them has it)."""
        if not self.remote:
            return None
        bases = [self.target] if self.role != "coordinator" else (shard_urls or [])
        for base in bases:
            try:
                req = urllib.request.Request(base.rstrip("/") + f"/v1/media/{sha256}")
                token = os.environ.get("CHLOEDB_TOKEN")
                if token:
                    req.add_header("authorization", f"Bearer {token}")
                with urllib.request.urlopen(req, timeout=10) as r:
                    return r.read()
            except Exception:
                continue
        return None

    # -- bookkeeping ------------------------------------------------------
    def _time(self, label: str, t0: float) -> None:
        ms = (time.perf_counter() - t0) * 1000
        with self.lock:
            xs = self.timings.setdefault(label, [])
            xs.append(ms)
            if len(xs) > 2000:
                del xs[: len(xs) - 2000]

    def timing_summary(self) -> dict[str, dict[str, float]]:
        out = {}
        with self.lock:
            for k, xs in self.timings.items():
                s = sorted(xs)
                out[k] = {"n": len(s), "p50_ms": round(s[len(s) // 2], 2), "p95_ms": round(s[int(len(s) * 0.95) - 1 if len(s) > 1 else 0], 2)}
        return out

    def describe(self) -> dict:
        return {"target": self.target, "role": self.role, "graph_in_db": self.graph_in_db, "vision": self.vision}


def _create_meta_sql() -> str:
    return "CREATE TABLE IF NOT EXISTS meta (k KEYWORD PRIMARY KEY, v TEXT OPTIONS(indexed = false))"


def _is_coordinator(health: dict) -> bool:
    return str(health.get("role", "")).lower() == "coordinator"
