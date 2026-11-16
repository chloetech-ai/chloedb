"""`AsyncClient`: `Client` for asyncio (FastAPI, aiohttp, any event loop).

It has the client's methods, each a coroutine: the request runs in a thread
of the loop's default executor (`asyncio.to_thread`), so the client stays
standard library only and the loop is never blocked. What reads the server
and is a property or a dunder on `Client` is a coroutine here::

    import chloedb

    async with chloedb.AsyncClient("http://127.0.0.1:7700", token=key) as db:
        hits = await db.search("waterproof jacket", table="products", limit=5)
        schema = await db.schema()            # Client.schema
        rows = await db.count()               # len(client)
        async for page in db.scroll("SELECT id FROM products"):
            ...
        q = (await db.table("products")).where(col("price") < 50).limit(10)
        result = await db.run(q)              # not q.run(), which blocks

`session()` gives an `AsyncSession`, whose reads see its own writes, as
`Client.session()` does. `client` is the `Client` underneath, for anything
that is fine to run blocking.
"""

from __future__ import annotations

import asyncio
import functools
from typing import Any, AsyncIterator, Callable

from .client import Client, Session

__all__ = ["AsyncClient", "AsyncSession"]

# Client's methods that are coroutines here, by the same name and arguments.
_METHODS = (
    "ask",
    "create_table",
    "delete",
    "embed",
    "embed_tokens",
    "explain",
    "export_parquet",
    "export_rank_log",
    "export_sqlite",
    "get",
    "get_many",
    "health",
    "import_file",
    "import_parquet",
    "import_sqlite",
    "insert",
    "plan_parquet",
    "plan_sqlite",
    "put",
    "query",
    "query_arrow",
    "query_logged",
    "run",
    "search",
    "sid_next",
    "source",
    "sources",
    "sql",
    "stats",
    "stream",
    "table",
    "tables",
    "upsert",
    "write",
)


def _async(name: str) -> Callable[..., Any]:
    method = getattr(Client, name)

    @functools.wraps(method)
    async def call(self: "AsyncClient", *args: Any, **kw: Any) -> Any:
        return await asyncio.to_thread(getattr(self.client, name), *args, **kw)

    return call


class AsyncClient:
    """A chloedb-server at `url`, from asyncio: `Client`'s arguments
    (`token`, `timeout`, `retry_for`) and methods, each awaited. Errors are
    the same `ChloeDBError` subclasses. Safe to share between tasks."""

    def __init__(self, url: str = "http://127.0.0.1:7700", **options: Any):
        self.client: Client = self._make(url, **options)

    @staticmethod
    def _make(url: str, **options: Any) -> Client:
        return Client(url, **options)

    @property
    def url(self) -> str:
        return self.client.url

    async def schema(self) -> dict:
        """The first table's schema in its short form (`Client.schema`)."""
        return await asyncio.to_thread(lambda: self.client.schema)

    async def count(self) -> int:
        """The rows the server holds (`len(client)`)."""
        return await asyncio.to_thread(len, self.client)

    async def scroll(self, sql: str, params: Any = None, **kw: Any) -> AsyncIterator[Any]:
        """`Client.scroll` as an async iterator: each page is read in a
        thread, when it is asked for."""
        it = await asyncio.to_thread(lambda: iter(self.client.scroll(sql, params, **kw)))
        done = object()
        try:
            while True:
                item = await asyncio.to_thread(next, it, done)
                if item is done:
                    return
                yield item
        finally:
            close = getattr(it, "close", None)
            if close is not None:
                await asyncio.to_thread(close)

    def session(self) -> "AsyncSession":
        """An `AsyncSession` on this server, with this client's token,
        timeout and retries: its reads see its own writes."""
        c = self.client
        return AsyncSession(c.url, token=c.token, timeout=c.timeout, retry_for=c.retry_for)

    async def close(self) -> None:
        self.client.close()

    async def __aenter__(self) -> "AsyncClient":
        return self

    async def __aexit__(self, *exc: Any) -> None:
        await self.close()

    def __repr__(self) -> str:
        return f"<chloedb.AsyncClient {self.url}>"


for _name in _METHODS:
    setattr(AsyncClient, _name, _async(_name))


class AsyncSession(AsyncClient):
    """`Session` for asyncio: an `AsyncClient` whose reads see its own
    writes (`observe`, `seq_no` and `seq_nos` are the Session's)."""

    client: Session

    @staticmethod
    def _make(url: str, **options: Any) -> Session:
        return Session(url, **options)

    def observe(self, written: Any) -> None:
        self.client.observe(written)

    @property
    def seq_no(self) -> int | None:
        return self.client.seq_no

    @property
    def seq_nos(self) -> list[int] | None:
        return self.client.seq_nos

    def session(self) -> "AsyncSession":
        return self

    def __repr__(self) -> str:
        return f"<chloedb.AsyncSession {self.url} seq_no={self.seq_no} seq_nos={self.seq_nos}>"
