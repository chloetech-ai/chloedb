"""A client of a running chloedb-server, over its HTTP JSON API, with the
same methods as `Database` where the server offers them. It needs only the
standard library."""

from __future__ import annotations

import base64
import http.client
import io
import json
import os
import re
import select
import shutil
import ssl
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from typing import Any, Iterable, Iterator, Mapping, Sequence

from ._search import Search, ordered_schema, params as _params
from ._native import ChloeDBError, make_error
from .build import Query
from .frame import FrameStore, _UPLOAD_ROWS, _arrow_table, _client_arrow
from .result import Result

# Rows a write request carries.
WRITE_BATCH = 1000
# Keys a read by key sends in one request (the server takes 10,000).
GET_BATCH = 10_000

FORMATS = {".jsonl": "jsonl", ".ndjson": "jsonl", ".json": "json", ".csv": "csv", ".tsv": "tsv", ".tab": "tsv"}
# The content type an import is sent with: a server refuses text/plain.
CONTENT_TYPES = {"jsonl": "application/x-ndjson", "json": "application/json", "csv": "text/csv", "tsv": "text/tab-separated-values"}


def _default(o: Any) -> Any:
    # numpy arrays and scalars
    if hasattr(o, "tolist"):
        return o.tolist()
    # datetimes and dates, as a TIMESTAMP's ISO 8601 text (naive is UTC)
    if hasattr(o, "isoformat") and hasattr(o, "year"):
        return o.isoformat()
    raise TypeError(f"cannot send a {type(o).__name__}")


def _rows(rows: Any) -> list:
    return [rows] if isinstance(rows, Mapping) else list(rows)


def _json_body(body: Any) -> bytes:
    """A request's JSON. A float that is not a number (NaN, infinity) is
    sent as null, as a database in this process stores it: JSON has no
    such number, and the server refuses one."""
    try:
        return json.dumps(body, default=_default, allow_nan=False).encode()
    except ValueError:
        text = json.dumps(body, default=_default)
        return json.dumps(json.loads(text, parse_constant=lambda _: None)).encode()


# A connection idle longer than this is not used again: a server, a proxy
# or a load balancer may have closed it meanwhile.
_IDLE_REUSE_S = 4.0

# Requests that change nothing, which may be sent again on a new
# connection when a kept one turns out closed before it answered.
_READ_PATHS = ("/v1/query", "/v1/kv/get", "/v1/explain", "/v1/embed", "/v1/schema/check", "/v1/sid/next")


class _Kept:
    """One thread's connection to a server, kept alive between requests."""

    __slots__ = ("conn", "pid", "idle_since")

    def __init__(self, conn: http.client.HTTPConnection):
        self.conn = conn
        self.pid = os.getpid()
        self.idle_since = time.monotonic()


class _Answer:
    """A response read through a kept connection: `urlopen`'s answer as
    `_call` and `_stream` read it (`read`, lines, `headers`), which gives
    the connection back for the next request once it has been read to its
    end, and drops it otherwise."""

    def __init__(self, client: "Client", kept: _Kept, resp: http.client.HTTPResponse):
        self._client, self._kept, self._resp = client, kept, resp
        self.headers = resp.headers
        self.status = resp.status

    def read(self, *n: int) -> bytes:
        return self._resp.read(*n)

    def readinto(self, b: Any) -> int:
        return self._resp.readinto(b)

    def __iter__(self) -> Iterator[bytes]:
        return iter(self._resp)

    def __enter__(self) -> "_Answer":
        return self

    def __exit__(self, *exc: Any) -> None:
        self._client._done(self._kept, self._resp)


def _retry_after(headers: Any) -> float:
    """The seconds a 429's `Retry-After` says to wait (1 when it says none)."""
    try:
        return min(max(float(headers.get("retry-after") or 1), 0.0), 10.0)
    except (TypeError, ValueError):
        return 1.0


class Written(int):
    """The rows a write wrote: an `int`, which compares, adds and prints as
    one, with the write's sequence numbers as the server answered them.
    `seq_no` is a server's (started with --seq-no), `seq_nos` a
    coordinator's, one per shard (0 where it wrote nothing); either is None
    when the answer had none. Pass them to a read as `min_seq_no=` /
    `min_seq_nos=` to read the write from any copy, or read through a
    `Session`, which does."""

    seq_no: int | None
    seq_nos: list[int] | None

    def __new__(cls, written: int = 0, seq_no: int | None = None, seq_nos: Sequence[int] | None = None) -> "Written":
        w = super().__new__(cls, written)
        w.seq_no = None if seq_no is None else int(seq_no)
        w.seq_nos = None if seq_nos is None else [int(n) for n in seq_nos]
        return w

    @classmethod
    def _of(cls, answer: Mapping[str, Any]) -> "Written":
        return cls(int(answer.get("written") or 0), answer.get("seq_no"), answer.get("seq_nos"))

    def _then(self, later: Any) -> "Written":
        """This write's rows and a later one's, with the later numbers."""
        return Written(int(self) + int(later), _later(self.seq_no, getattr(later, "seq_no", None)), _later_each(self.seq_nos, getattr(later, "seq_nos", None)))

    def __reduce__(self) -> Any:
        return (Written, (int(self), self.seq_no, self.seq_nos))


def _later(a: int | None, b: int | None) -> int | None:
    return b if a is None else a if b is None else max(a, b)


def _later_each(a: Sequence[int] | None, b: Sequence[int] | None) -> list[int] | None:
    """Per shard, the later of two lists of numbers."""
    if a is None or b is None:
        return None if a is None and b is None else list(a if b is None else b)  # type: ignore[arg-type]
    return [max(a[i] if i < len(a) else 0, b[i] if i < len(b) else 0) for i in range(max(len(a), len(b)))]


def _after_body(min_seq_no: int | None, min_seq_nos: Sequence[int] | None) -> dict:
    """A read's `min_seq_no` and `min_seq_nos` as the server takes them;
    0s ask for nothing and are left out."""
    out: dict[str, Any] = {}
    if min_seq_no:
        out["min_seq_no"] = int(min_seq_no)
    if min_seq_nos and any(min_seq_nos):
        out["min_seq_nos"] = [int(n) for n in min_seq_nos]
    return out


def _reads(sql: str) -> bool:
    """Whether a statement is a read (SELECT, WITH, EXPLAIN, ...), by its
    first word."""
    m = re.match(r"(?:\s|\(|--[^\n]*|/\*.*?\*/)*([A-Za-z]+)", sql, re.S)
    return bool(m) and m.group(1).lower() in ("select", "with", "explain", "values", "show", "describe")


# The rows a frame asks of each page of a server's answer: the server cuts
# it to its --max-result-window.
FRAME_PAGE = 100_000


class Client(Search, FrameStore):
    """A chloedb-server at `url` ("http://host:7700"). `token` is the
    server's --token, if it has one. Errors the server reports raise
    `ChloeDBError`; one it cannot be reached raises `urllib.error.URLError`.

    A request the server refuses for its load (429: behind on flushing its
    writes, its write queue full, too many queries running) did nothing,
    and is sent again after the `Retry-After` it gives, for up to
    `retry_for` seconds (0: never). A write that failed partway is not,
    unless it has an idempotency key.
    """

    def __init__(self, url: str = "http://127.0.0.1:7700", *, token: str | None = None, timeout: float = 30.0, retry_for: float = 10.0):
        self.url = url.rstrip("/")
        self.token = token if token is not None else os.environ.get("CHLOEDB_TOKEN") or os.environ.get("STRATA_TOKEN")
        self.timeout = timeout
        self.retry_for = retry_for
        # /v1/schema's answer for the first table, under None (kept until a
        # table is made here); a table named is read each time.
        self._schemas: dict[str | None, dict] = {}
        self._json_only = False  # a server that answers no Arrow
        self._conns = threading.local()

    # Connections. Each thread keeps one connection to the server alive
    # between its requests, rather than a TLS handshake and a socket per
    # request; through a proxy ($HTTPS_PROXY, $HTTP_PROXY) a request goes
    # by urllib, which knows how to use one.

    def _proxied(self) -> bool:
        parts = urllib.parse.urlsplit(self.url)
        return bool(urllib.request.getproxies().get(parts.scheme)) and not urllib.request.proxy_bypass(parts.hostname or "")

    def _new_connection(self) -> http.client.HTTPConnection:
        parts = urllib.parse.urlsplit(self.url)
        if parts.scheme == "https":
            return http.client.HTTPSConnection(parts.hostname, parts.port, timeout=self.timeout, context=ssl.create_default_context())
        return http.client.HTTPConnection(parts.hostname, parts.port, timeout=self.timeout)

    def _kept(self) -> tuple[_Kept, bool]:
        """This thread's connection, and whether it served a request
        before."""
        kept = getattr(self.__dict__.get("_conns"), "kept", None)
        if kept is not None:
            sock = kept.conn.sock
            fresh = kept.pid == os.getpid() and sock is not None and time.monotonic() - kept.idle_since < _IDLE_REUSE_S
            # A connection the server closed reads as ready (its end).
            try:
                if fresh and not select.select([sock], [], [], 0)[0]:
                    return kept, True
            except (OSError, ValueError):
                pass  # closed here
            kept.conn.close()
        kept = _Kept(self._new_connection())
        if "_conns" not in self.__dict__:
            self._conns = threading.local()
        self._conns.kept = kept
        return kept, False

    def _done(self, kept: _Kept, resp: http.client.HTTPResponse) -> None:
        """After a response: the connection is kept when the response was
        read to its end, and closed otherwise."""
        if resp.isclosed() and not resp.will_close:
            kept.idle_since = time.monotonic()
        else:
            kept.conn.close()
            if getattr(self._conns, "kept", None) is kept:
                self._conns.kept = None

    def _open(self, method: str, path: str, data: Any, headers: Mapping[str, str]) -> Any:
        """Sends a request and returns its answer, as `urlopen` does: an
        error status raises `urllib.error.HTTPError`, a server that cannot
        be reached `urllib.error.URLError`."""
        if self._proxied():
            req = urllib.request.Request(self.url + path, data=data, method=method, headers=dict(headers))
            return urllib.request.urlopen(req, timeout=self.timeout)
        prefix = urllib.parse.urlsplit(self.url).path.rstrip("/")
        replayable = isinstance(data, (bytes, type(None)))
        reads = method in ("GET", "HEAD", "DELETE") or path.split("?", 1)[0] in _READ_PATHS
        for attempt in (0, 1):
            kept, reused = self._kept()
            try:
                kept.conn.request(method, prefix + path, body=data, headers=dict(headers))
            except (ConnectionError, http.client.HTTPException, OSError) as e:
                kept.conn.close()
                self._conns.kept = None
                # Nothing was answered, so nothing was done: a kept
                # connection found closed is tried again on a new one.
                if reused and replayable and attempt == 0 and not isinstance(e, TimeoutError):
                    continue
                raise urllib.error.URLError(e) from None
            try:
                resp = kept.conn.getresponse()
            except (ConnectionError, http.client.HTTPException, OSError) as e:
                kept.conn.close()
                self._conns.kept = None
                # A kept connection closed after the request went: sent
                # again only if it changed nothing.
                if reused and replayable and reads and attempt == 0 and isinstance(e, (http.client.RemoteDisconnected, ConnectionResetError, BrokenPipeError)):
                    continue
                raise urllib.error.URLError(e) from None
            if resp.status >= 400:
                try:
                    raw = resp.read()
                finally:
                    self._done(kept, resp)
                raise urllib.error.HTTPError(self.url + path, resp.status, resp.reason, resp.headers, io.BytesIO(raw))
            return _Answer(self, kept, resp)
        raise AssertionError("unreachable")

    def __getstate__(self) -> dict:
        # Connections stay with the process and thread that made them.
        state = dict(self.__dict__)
        state.pop("_conns", None)
        state.pop("_lock", None)
        return state

    def __setstate__(self, state: dict) -> None:
        self.__dict__.update(state)
        self._conns = threading.local()
        if isinstance(self, Session):
            self._lock = threading.Lock()

    def _call(self, method: str, path: str, body: Any = None, *, data: Any = None, headers: Mapping[str, str] | None = None, text: bool = False, out: Any = None) -> Any:
        """The answer's JSON, or its text with `text`; with `out`, a binary
        file, the answer is copied into it and its headers returned."""
        h = dict(headers or {})
        if body is not None:
            data = _json_body(body)
            h["content-type"] = "application/json"
        if self.token:
            h["authorization"] = f"Bearer {self.token}"
        # A file is read once: an import is not sent again.
        again = isinstance(data, (bytes, type(None)))
        keyed = isinstance(body, Mapping) and bool(body.get("idempotency_key"))
        give_up = time.monotonic() + self.retry_for
        while True:
            try:
                with self._open(method, path, data, h) as r:
                    if out is not None:
                        shutil.copyfileobj(r, out)
                        return r.headers
                    return r.read().decode() if text else json.loads(r.read() or b"null")
            except urllib.error.HTTPError as e:
                raw = e.read()
                try:
                    answer = json.loads(raw)
                    msg = answer.get("error") or raw.decode(errors="replace")
                except (ValueError, AttributeError):
                    answer, msg = None, raw.decode(errors="replace") or str(e)
                partial = isinstance(answer, dict) and answer.get("partial") is True
                wait = _retry_after(e.headers)
                if e.code == 429 and self.retry_for > 0 and again and (keyed or not partial) and time.monotonic() + wait <= give_up:
                    time.sleep(wait)
                    continue
                raise make_error(f"{msg} (HTTP {e.code})", status=e.code, answer=answer) from None

    def _schema_answer(self, table: str | None = None) -> dict:
        """/v1/schema's answer for the first table, or for `table` alone
        (the first table is not read then: a key limited to other tables
        may not)."""
        if table is not None:
            return self._call("GET", "/v1/schema?" + urllib.parse.urlencode({"table": table}))
        if None not in self._schemas:
            self._schemas[None] = self._call("GET", "/v1/schema")
        return self._schemas[None]

    @property
    def _schema(self) -> dict | None:
        return self._schemas.get(None)

    @_schema.setter
    def _schema(self, value: dict | None) -> None:
        # Setting it to None (a table was made) forgets every table's.
        if value is None:
            self._schemas.clear()
        else:
            self._schemas[None] = value

    @property
    def schema(self) -> dict:
        """The first table's schema in its short form: {table, key, fields}."""
        return ordered_schema(self._schema_answer()["short"])

    def _table_schema(self, name: str) -> dict:
        """The schema of the table `name`, in its short form."""
        return ordered_schema(self._schema_answer(name)["short"])

    def tables(self) -> list[dict]:
        """Every table's schema in its short form, the first first."""
        return [ordered_schema(t["short"]) for t in self._call("GET", "/v1/tables")["tables"]]

    def _embedded(self, table: str | None = None) -> list[str]:
        return [e["field"] for e in self._schema_answer(table).get("embeddings", []) if e.get("loaded")]

    def health(self) -> dict:
        """{status, docs, segments, role, ...}, also when the status is "down"
        or "failing" (which the server answers with 503)."""
        try:
            return self._call("GET", "/v1/health")
        except ChloeDBError as e:
            if e.status == 503 and isinstance(e.answer, dict) and "status" in e.answer:
                return e.answer
            raise

    def stats(self) -> dict:
        return self._call("GET", "/v1/stats")

    def __len__(self) -> int:
        return int(self.health()["docs"])

    # Reads that see a write. A read takes `min_seq_no` (a server's write
    # answered it) and `min_seq_nos` (a coordinator's, one per shard): the
    # copy that answers holds those writes. A `Session` adds its own
    # writes' numbers to every read (`_after`) and keeps them from every
    # write's answer (`_note`); a plain Client sends only what it is given.

    def _after(self, min_seq_no: int | None = None, min_seq_nos: Sequence[int] | None = None) -> dict:
        return _after_body(min_seq_no, min_seq_nos)

    def _note(self, written: "Written") -> "Written":
        return written

    def session(self) -> "Session":
        """A `Session` on this client's server, with its token, timeout and
        retries: its reads see its own writes, through a coordinator or
        from a replica too."""
        return Session(self.url, token=self.token, timeout=self.timeout, retry_for=self.retry_for)

    def query(self, sql: str, params: Mapping[str, Any] | None = None, *, timeout_ms: int | None = None, min_seq_no: int | None = None, min_seq_nos: Sequence[int] | None = None, **kw: Any) -> Result:
        """Runs a SELECT. With `min_seq_no` (a server's `Written.seq_no`) or
        `min_seq_nos` (a coordinator's `Written.seq_nos`), the copy that
        answers holds those writes: a replica waits for them up to a second."""
        body: dict[str, Any] = {"sql": sql, "params": _params(params, kw), **self._after(min_seq_no, min_seq_nos)}
        if timeout_ms:
            body["timeout_ms"] = timeout_ms
        return Result._of(self._call("POST", "/v1/query", body))

    def query_arrow(self, sql: str, params: Mapping[str, Any] | None = None, *, timeout_ms: int | None = None, min_seq_no: int | None = None, min_seq_nos: Sequence[int] | None = None, **kw: Any):
        """Runs a SELECT and returns its answer as a pyarrow Table, sent by
        the server as an Arrow IPC stream (no JSON either way), its columns
        typed as `Database.query_arrow` types them. Needs pyarrow. A server
        too old for Arrow answers JSON, which this types from the values."""
        return self._arrow_answer(sql, _params(params, kw), timeout_ms, {}, self._after(min_seq_no, min_seq_nos))

    def _arrow_answer(self, sql: str, params: Mapping[str, Any], timeout_ms: int | None, types: Mapping[str, dict], after: Mapping[str, Any] | None = None):
        body: dict[str, Any] = {"sql": sql, "params": params, **(self._after() if after is None else after)}
        if timeout_ms:
            body["timeout_ms"] = timeout_ms
        return self._arrow_page(body, types)[0]

    def _arrow_page(self, body: dict, types: Mapping[str, dict]) -> tuple[Any, str | None]:
        """An answer (or a scroll's page) as a pyarrow Table, and the
        scroll's cursor while rows are left."""
        import io

        from .frame import _pyarrow

        _pyarrow()
        if self._json_only:
            j = self._call("POST", "/v1/query", body)
            return _client_arrow(Result._of(j), types), j.get("cursor")
        buf = io.BytesIO()
        headers = self._call("POST", "/v1/query", {**body, "format": "arrow"}, out=buf)
        if "arrow" not in (headers.get("content-type") or ""):
            # A server from before Arrow answers ignores "format".
            self._json_only = True
            j = json.loads(buf.getvalue() or b"null")
            return _client_arrow(Result._of(j), types), j.get("cursor")
        import pyarrow.ipc

        return pyarrow.ipc.open_stream(buf.getvalue()).read_all(), headers.get("x-chloedb-cursor") or headers.get("x-strata-cursor")

    def scroll(self, sql: str, params: Mapping[str, Any] | None = None, *, page_size: int = 10_000, arrow: bool = False, keep_alive_ms: int | None = None, timeout_ms: int | None = None, min_seq_no: int | None = None, min_seq_nos: Sequence[int] | None = None, **kw: Any) -> Iterator[Any]:
        """Runs a SELECT and yields its answer a page at a time: `Result`s,
        or with `arrow=True` pyarrow Tables, of at most `page_size` rows
        (and at most the server's --max-result-window, which caps each page
        instead of the whole answer). Every page reads the version of the
        database the first read. The server keeps the rest for
        `keep_alive_ms` after each page (default its lease TTL, 60 s);
        leaving the loop early ends it. A server from before scrolls
        answers in one page. `min_seq_no(s)` apply to the version the first
        page reads, which every page then reads."""
        pages = self._pages(sql, _params(params, kw), page_size=page_size, arrow=arrow, keep_alive_ms=keep_alive_ms, timeout_ms=timeout_ms, after=self._after(min_seq_no, min_seq_nos))
        try:
            for page, _ in pages:
                yield page
        finally:
            pages.close()  # leaving early closes the cursor

    def _pages(self, sql: str, params: Mapping[str, Any], *, page_size: int, arrow: bool = False, keep_alive_ms: int | None = None, timeout_ms: int | None = None, after: Mapping[str, Any] | None = None) -> Iterator[tuple[Any, bool]]:
        """`scroll`'s pages, each with whether more follow it."""
        body: dict[str, Any] = {"sql": sql, "params": params, "page_size": int(page_size), **(self._after() if after is None else after)}
        more: dict[str, Any] = {}
        if keep_alive_ms:
            more["keep_alive_ms"] = int(keep_alive_ms)
        if timeout_ms:
            more["timeout_ms"] = int(timeout_ms)
        body.update(more)
        cursor = None
        try:
            while True:
                if arrow:
                    page, cursor = self._arrow_page(body, {})
                else:
                    j = self._call("POST", "/v1/query", body)
                    page, cursor = Result._of(j), j.get("cursor")
                yield page, bool(cursor)
                if not cursor:
                    return
                body = {"cursor": cursor, **more}
        finally:
            if cursor:
                self._close_cursor(cursor)

    def _close_cursor(self, cursor: str) -> None:
        try:
            self._call("DELETE", f"/v1/query/cursors/{cursor}")
        except (ChloeDBError, OSError):
            pass  # it ends when its keep-alive does

    def sql(self, sql: str, params: Mapping[str, Any] | None = None, *, idempotency_key: str | None = None, min_seq_no: int | None = None, min_seq_nos: Sequence[int] | None = None, **kw: Any) -> Result | Written:
        """Runs any statement: a SELECT's `Result`, or the number of rows an
        INSERT, UPDATE or DELETE wrote (a `Written`, an int with the write's
        sequence numbers). With `idempotency_key`, a write sent again is not
        applied twice (see `write`); `min_seq_no(s)` as for `query`."""
        body: dict[str, Any] = {"sql": sql, "params": _params(params, kw)}
        if idempotency_key is not None:
            body["idempotency_key"] = idempotency_key
        # A session's numbers go with a read only: a server refuses them on
        # a write, which sees every write before it anyway.
        body.update(self._after(min_seq_no, min_seq_nos) if _reads(sql) else _after_body(min_seq_no, min_seq_nos))
        r = self._call("POST", "/v1/sql", body)
        return Result._of(r) if "rows" in r else self._note(Written._of(r))

    def _query_logged(self, sql: str, params: Mapping[str, Any] | None, log: dict) -> Result:
        return Result._of(self._call("POST", "/v1/query", {"sql": sql, "params": _params(params, {}), "log": log}))

    def _export_rank_log(self, opts: dict) -> list[dict]:
        # NDJSON, a record a line; a failure partway is the last line.
        out = [json.loads(line) for line in self._call("POST", "/v1/rank_log/export", opts, text=True).splitlines() if line.strip()]
        if out and "error" in out[-1] and "request" not in out[-1]:
            raise make_error(f"{out[-1]['error']} (HTTP {out[-1].get('status', 500)})", status=out[-1].get("status", 500), answer=out[-1])
        return out

    def explain(self, sql: str, params: Mapping[str, Any] | None = None, **kw: Any) -> dict:
        """Runs the query and returns its plan: the plan IR as JSON ("ir")
        and as lines ("tree"), the executor's plan lines ("plan") and the
        bound query ("logical")."""
        return self._call("POST", "/v1/explain", {"sql": sql, "params": _params(params, kw)})

    def run(self, plan: Mapping[str, Any] | Query, *, min_seq_no: int | None = None, min_seq_nos: Sequence[int] | None = None) -> Result | Written:
        """Runs a plan (the "ir" of `explain`, or a `Query`): a read's
        `Result`, or the number of rows a write wrote. `min_seq_no(s)` as
        for `query` (a read's plan only)."""
        if isinstance(plan, Query):
            plan = plan.plan()
        root = plan.get("root") if isinstance(plan, Mapping) else None
        write = isinstance(root, Mapping) and root.get("node") in ("insert", "update", "delete")
        r = self._call("POST", "/v1/plan", {"plan": plan, **(_after_body(min_seq_no, min_seq_nos) if write else self._after(min_seq_no, min_seq_nos))})
        return Result._of(r) if "rows" in r else self._note(Written._of(r))

    def embed(self, text: str | None = None, field: str | None = None, *, table: str | None = None, image: bytes | None = None, audio: bytes | None = None) -> list[float]:
        """The query vector of `text` by an embedded field's model (of the
        first table, or of `table`), or of `image` or `audio`, the bytes of
        an image or sound file, by a model of that, on the server."""
        b64 = lambda d: None if d is None else base64.b64encode(d).decode()
        body = {k: v for k, v in {"text": text, "image": b64(image), "audio": b64(audio), "field": field, "table": table}.items() if v is not None}
        return self._call("POST", "/v1/embed", body)["vector"]

    def embed_tokens(self, text: str, field: str, *, table: str | None = None) -> list[list[float]]:
        """The vectors of `text`'s tokens as a query, by the model of
        multi-vector field `field` (ColBERT), on the server."""
        body = {k: v for k, v in {"text": text, "field": field, "table": table}.items() if v is not None}
        return self._call("POST", "/v1/embed", body)["vectors"]

    def sid_next(self, prefixes: Iterable[str], *, field: str, where: str | None = None, params: Mapping[str, Any] | None = None, table: str | None = None, min_seq_no: int | None = None, min_seq_nos: Sequence[int] | None = None, **kw: Any) -> dict[str, dict[str, int]]:
        """The codes that can follow each prefix of semantic-ID field
        `field` among the rows `where` matches (POST /v1/sid/next), summed
        over the shards on a coordinator: {prefix: {code: rows}}."""
        body = {"field": field, "prefixes": list(prefixes), "where": where, "params": _params(params, kw), "table": table, **self._after(min_seq_no, min_seq_nos)}
        got = self._call("POST", "/v1/sid/next", {k: v for k, v in body.items() if v is not None})
        return {n["prefix"]: {c["code"]: c["count"] for c in n["next"]} for n in got["next"]}

    def write(self, ops: list[dict], *, idempotency_key: str | None = None) -> Written:
        """Write operations as /v1/write takes them ({"op": "insert",
        "doc": {...}}, {"op": "delete", "key": k}, ...), in one request.
        With `idempotency_key`, the same request sent again (after a lost
        answer, or with the `idempotency_key` of an error whose `answer`
        says it failed partway through a coordinator, `"partial": True`)
        is not applied twice. Returns the rows written, a `Written` (an int
        with the write's `seq_no` or, from a coordinator, `seq_nos`)."""
        body: dict[str, Any] = {"ops": ops}
        if idempotency_key is not None:
            body["idempotency_key"] = idempotency_key
        return self._note(Written._of(self._call("POST", "/v1/write", body)))

    def _ops(self, op: str, rows: list, table: str | None) -> Written:
        named = {} if table is None else {"table": table}
        n = Written(0)
        for i in range(0, len(rows), WRITE_BATCH):
            n = n._then(self.write([{"op": op, **named, "doc": r} for r in rows[i : i + WRITE_BATCH]]))
        return n

    def insert(self, rows: Mapping[str, Any] | Iterable[Mapping[str, Any]], *, table: str | None = None) -> Written:
        """Inserts a row or rows, whose keys must be new, into the first
        table or `table`, 1,000 rows a request. Returns how many (a
        `Written`, with the last request's sequence numbers)."""
        return self._ops("insert", _rows(rows), table)

    def upsert(self, rows: Mapping[str, Any] | Iterable[Mapping[str, Any]], *, table: str | None = None) -> Written:
        """Inserts a row or rows, replacing the rows that have their keys,
        1,000 rows a request. Returns how many (a `Written`)."""
        return self._ops("upsert", _rows(rows), table)

    def delete(self, keys: Any, *, table: str | None = None, routing: Any = None) -> Written:
        """Deletes the rows with this key or these keys. Through a
        coordinator, `routing` gives the `shard_by` value of a table placed
        by a column. Returns how many (a `Written`)."""
        keys = [keys] if isinstance(keys, (str, int)) else list(keys)
        named = {k: v for k, v in {"table": table, "routing": routing}.items() if v is not None}
        n = Written(0)
        for i in range(0, len(keys), WRITE_BATCH):
            n = n._then(self.write([{"op": "delete", **named, "key": k} for k in keys[i : i + WRITE_BATCH]]))
        return n

    def _table(self, table: str | None) -> str:
        return table if table is not None else self.schema["table"]

    def get(self, key: Any, *, table: str | None = None, min_seq_no: int | None = None, min_seq_nos: Sequence[int] | None = None) -> dict | None:
        """The row with this primary key in the first table or `table`, read
        without SQL (GET /v1/kv/:table/:key): a dict of the columns `SELECT
        *` answers, or None when no live row has the key (or its
        `expire_by` time has passed). A coordinator asks the key's shard.
        `min_seq_no(s)` as for `query`."""
        path = f"/v1/kv/{urllib.parse.quote(self._table(table), safe='')}/{urllib.parse.quote(str(key), safe='')}"
        after = self._after(min_seq_no, min_seq_nos)
        if after:
            path += "?" + urllib.parse.urlencode({k: ",".join(map(str, v)) if isinstance(v, list) else v for k, v in after.items()})
        try:
            return self._call("GET", path)["row"]
        except ChloeDBError as e:
            if e.status == 404 and isinstance(e.answer, dict) and e.answer.get("found") is False:
                return None
            raise

    def get_many(self, keys: Iterable[Any], *, table: str | None = None, columns: Iterable[str] | None = None, min_seq_no: int | None = None, min_seq_nos: Sequence[int] | None = None) -> list[dict | None]:
        """`get` of several keys (POST /v1/kv/get), 10,000 a request, in
        their order; with `columns`, each row of those columns alone (a
        ranker's features, say), which reads no other. `min_seq_no(s)` as
        for `query`."""
        keys = list(keys)
        body: dict[str, Any] = {"table": self._table(table), **self._after(min_seq_no, min_seq_nos)}
        if columns is not None:
            body["columns"] = list(columns)
        out: list[dict | None] = []
        for i in range(0, len(keys), GET_BATCH):
            out += self._call("POST", "/v1/kv/get", {**body, "keys": keys[i : i + GET_BATCH]})["rows"]
        return out

    def put(self, key: Any, row: Mapping[str, Any], *, table: str | None = None, if_seq_no: int | None = None) -> Written:
        """Writes `row` as the row with this key (PUT /v1/kv/:table/:key),
        in place of any that has it; with `if_seq_no`, only while that
        row's sequence number is that. Returns the rows written (a
        `Written`)."""
        path = f"/v1/kv/{urllib.parse.quote(self._table(table), safe='')}/{urllib.parse.quote(str(key), safe='')}"
        if if_seq_no is not None:
            path += f"?if_seq_no={int(if_seq_no)}"
        return self._note(Written._of(self._call("PUT", path, dict(row))))

    def stream(self, events: Iterable[Mapping[str, Any]]) -> dict:
        """Writes `events` through /v1/stream as they come, in batches the
        server makes: each an operation, or one event's operations as
        {"ops": [...], "source": {"name": ..., "offset": n, "ts": ...}},
        which a batch never splits. A source's offsets increase along the
        stream; one the server has reached already (a replay's) is skipped,
        so resuming from `source(name) + 1`, or from before it, applies
        each event once. Returns the last answer, {"done": True, "lines",
        "written", "skipped", "batches"}; a failure raises ChloeDBError,
        whose `answer["lines"]` counts the events written before it.
        `chloedb.ingest` reads Kafka into a stream."""

        def chunks() -> Iterable[bytes]:
            chunk: list[str] = []
            for e in events:
                chunk.append(json.dumps(e, default=_default))
                if len(chunk) >= 256:
                    yield ("\n".join(chunk) + "\n").encode()
                    chunk = []
            if chunk:
                yield ("\n".join(chunk) + "\n").encode()

        return self._stream(chunks())

    def _stream(self, chunks: Iterable[bytes]) -> dict:
        # Each chunk is some whole lines, sent as soon as it is made.
        h = {"content-type": "application/x-ndjson"}
        if self.token:
            h["authorization"] = f"Bearer {self.token}"
        last: dict = {}
        try:
            with self._open("POST", "/v1/stream", chunks, h) as r:
                for raw in r:
                    if raw.strip():
                        try:
                            last = json.loads(raw)
                        except ValueError:
                            last = {}  # cut short
                            break
        except urllib.error.HTTPError as e:
            raw = e.read()
            raise make_error(f"{raw.decode(errors='replace')} (HTTP {e.code})", status=e.code) from None
        if "error" in last or not last.get("done"):
            raise make_error(f"{last.get('error', 'the stream ended early')} (HTTP {last.get('status', 500)})", status=last.get("status", 500), answer=last)
        return last

    def source(self, name: str) -> int | None:
        """The offset of the last event written from stream source `name`,
        or None for a source not written from: resume from the next."""
        return self._call("GET", "/v1/sources/" + urllib.parse.quote(name, safe="/"))["offset"]

    def sources(self) -> list[dict]:
        """Every stream source written from: [{name, offset, updated}]."""
        return self._call("GET", "/v1/sources")["sources"]

    def import_file(self, path: str | os.PathLike, format: str | None = None, *, table: str | None = None) -> dict:
        """Uploads a JSON Lines, JSON (an array), CSV or TSV file (up to the
        server's 256 MB), in `format` or the one its extension names, into
        the first table or `table`; its rows are upserted. Returns {rows,
        bulk, skipped_fields}."""
        path = os.fspath(path)
        format = format or FORMATS.get(os.path.splitext(path)[1].lower())
        if not format:
            raise ValueError(f"name the format of {path}: jsonl, json, csv or tsv")
        with open(path, "rb") as f:
            size = os.fstat(f.fileno()).st_size
            ct = CONTENT_TYPES.get(format, "application/octet-stream")
            query = {"format": format} if table is None else {"format": format, "table": table}
            r = self._call("POST", "/v1/import?" + urllib.parse.urlencode(query), data=f, headers={"content-length": str(size), "content-type": ct})
        return {"rows": r["written"], "bulk": False, "skipped_fields": r.get("skipped_fields", [])}

    def _sqlite(self, path: str | os.PathLike, query: dict) -> dict:
        with open(os.fspath(path), "rb") as f:
            size = os.fstat(f.fileno()).st_size
            return self._call("POST", "/v1/import?" + urllib.parse.urlencode(query), data=f, headers={"content-length": str(size), "content-type": "application/vnd.sqlite3"})

    @staticmethod
    def _sqlite_query(tables: str | Iterable[str] | None, **flags: bool) -> dict:
        q: dict = {"format": "sqlite"}
        if tables is not None:
            q["tables"] = tables if isinstance(tables, str) else ",".join(tables)
        q.update({k: "true" if v else "false" for k, v in flags.items()})
        return q

    def import_sqlite(self, path: str | os.PathLike, *, tables: str | Iterable[str] | None = None, blobs_base64: bool = False) -> dict:
        """Uploads a SQLite database file (up to the server's 256 MB) and
        imports its tables (all, or `tables`), each into the table of its
        name: one the server has keeps its schema, and a missing one is
        created from the SQLite table's declarations and values; rows are
        upserted by key. BLOB columns are left out unless `blobs_base64`.
        Returns {tables: [{source, table, rows, created, notes}], skipped:
        {name: why}, written}."""
        return self._sqlite(path, self._sqlite_query(tables, blobs_base64=blobs_base64))

    def plan_sqlite(self, path: str | os.PathLike, *, tables: str | Iterable[str] | None = None, blobs_base64: bool = False) -> dict:
        """What `import_sqlite` would do, writing nothing: each table's
        schema, the columns read and what does not carry over."""
        return self._sqlite(path, self._sqlite_query(tables, blobs_base64=blobs_base64, plan=True))

    def export_sqlite(self, path: str | os.PathLike, *, tables: str | Iterable[str] | None = None, fts: bool = True, indexes: bool = True, schemas: bool = True) -> list[str]:
        """Downloads the server's tables (all, or `tables`) as a new SQLite
        file at `path`, which must not exist: the rows, keys, references,
        B-tree indexes (`indexes`), an FTS5 index over each table's text
        (`fts`) and the ChloeDB schemas (`schemas`). Returns the tables."""
        path = os.fspath(path)
        q: dict = {k: "false" for k, v in {"fts": fts, "indexes": indexes, "schemas": schemas}.items() if not v}
        if tables is not None:
            q["tables"] = tables if isinstance(tables, str) else ",".join(tables)
        part = path + ".part"
        try:
            with open(part, "xb") as f:
                headers = self._call("GET", "/v1/export?" + urllib.parse.urlencode(q), out=f)
            if os.path.exists(path):
                raise FileExistsError(path)
            os.replace(part, path)
        finally:
            if os.path.exists(part):
                os.remove(part)
        return [t for t in (headers.get("x-chloedb-tables") or headers.get("x-strata-tables") or "").split(",") if t]

    def _parquet(self, path: str | os.PathLike, query: dict) -> dict:
        with open(os.fspath(path), "rb") as f:
            size = os.fstat(f.fileno()).st_size
            return self._call("POST", "/v1/import?" + urllib.parse.urlencode(query), data=f, headers={"content-length": str(size), "content-type": "application/vnd.apache.parquet"})

    @staticmethod
    def _parquet_query(table: str | None, key: str | None, **flags: bool) -> dict:
        q: dict = {"format": "parquet"}
        if table is not None:
            q["table"] = table
        if key is not None:
            q["key"] = key
        q.update({k: "true" if v else "false" for k, v in flags.items()})
        return q

    def import_parquet(self, path: str | os.PathLike, *, table: str | None = None, key: str | None = None, blobs_base64: bool = False) -> dict:
        """Uploads a Parquet file (up to the server's 256 MB) and imports its
        rows into `table` (by default, the table an export kept in the
        file): one the server has keeps its schema and takes the columns
        whose names match its fields, rows upserted by key; a missing one is
        created from the file's columns and values, keyed by `key` (default:
        a column named id; "" for none). Returns {table, rows, created,
        skipped: {column: why}, notes, written}."""
        return self._parquet(path, self._parquet_query(table, key, blobs_base64=blobs_base64))

    def plan_parquet(self, path: str | os.PathLike, *, table: str | None = None, key: str | None = None, blobs_base64: bool = False) -> dict:
        """What `import_parquet` would do, writing nothing: the table's
        schema, the columns read and what is left out."""
        return self._parquet(path, self._parquet_query(table, key, blobs_base64=blobs_base64, plan=True))

    def export_parquet(self, path: str | os.PathLike, *, table: str | None = None, schemas: bool = True) -> str:
        """Downloads one of the server's tables (`table`, which a server of
        one table may leave out) as a new Parquet file at `path`, which must
        not exist; the file keeps the table's ChloeDB schema. Returns the
        table."""
        path = os.fspath(path)
        q: dict = {"format": "parquet"}
        if table is not None:
            q["tables"] = table
        if not schemas:
            q["schemas"] = "false"
        part = path + ".part"
        try:
            with open(part, "xb") as f:
                headers = self._call("GET", "/v1/export?" + urllib.parse.urlencode(q), out=f)
            if os.path.exists(path):
                raise FileExistsError(path)
            os.replace(part, path)
        finally:
            if os.path.exists(part):
                os.remove(part)
        return headers.get("x-chloedb-tables") or headers.get("x-strata-tables") or ""

    # What frames need of a store (FrameStore). Until the server answers in
    # Arrow, a frame's answer comes as JSON and is typed here by the
    # fields its columns read; rows go up as Parquet.

    def _frame_arrow(self, sql: str, params: Mapping[str, Any], types: Mapping[str, dict]) -> Any:
        pages = list(self._frame_batches(sql, params, types))
        if len(pages) == 1:
            return pages[0]
        import pyarrow

        # A page types each column by its values: one of NULLs only, or of
        # whole numbers where another has fractions, widens to the others'.
        return pyarrow.concat_tables(pages, promote_options="permissive")

    def _frame_batches(self, sql: str, params: Mapping[str, Any], types: Mapping[str, dict]) -> Iterator[Any]:
        """A frame's answer a page at a time (pyarrow Tables), by a scroll
        of pages as large as the server allows."""
        body: dict[str, Any] = {"sql": sql, "params": _params(params, {}), "page_size": FRAME_PAGE, **self._after()}
        cursor = None
        try:
            while True:
                try:
                    page, cursor = self._arrow_page(body, types)
                except ChloeDBError as e:
                    # A server from before scrolls answers at most
                    # --max-result-window rows a query.
                    if "a query returns at most" not in str(e) and "the most a query returns" not in str(e):
                        raise
                    raise make_error(f"{e}; this server reads a frame in one query (it is from before scrolls), so it needs a larger --max-result-window (0: none)", status=getattr(e, "status", None), answer=getattr(e, "answer", None), code=e.code) from None
                yield page
                if not cursor:
                    return
                body = {"cursor": cursor}
        finally:
            if cursor:
                self._close_cursor(cursor)

    def _explain(self, sql: str, params: Mapping[str, Any]) -> list[str]:
        return list(self.explain(sql, params).get("tree") or [])

    def _plan_parquet_file(self, path: str, table: str, key: str | None) -> dict:
        return self.plan_parquet(path, table=table, key=key)

    def _create(self, schema: Mapping[str, Any]) -> None:
        self.sql(self._call("POST", "/v1/schema/check", dict(schema))["sql"])
        self._schema = None

    def _load(self, table: str, data: Any) -> dict:
        import io

        import pyarrow.parquet

        t = _arrow_table(data)
        rows = 0
        for start in range(0, max(t.num_rows, 1), _UPLOAD_ROWS):
            part = t.slice(start, _UPLOAD_ROWS)
            if part.num_rows == 0:
                break
            buf = io.BytesIO()
            pyarrow.parquet.write_table(part, buf, compression="zstd")
            body = buf.getvalue()
            query = urllib.parse.urlencode({"format": "parquet", "table": table})
            r = self._call("POST", "/v1/import?" + query, data=body, headers={"content-length": str(len(body)), "content-type": "application/vnd.apache.parquet"})
            rows += int(r.get("rows", part.num_rows))
        return {"table": table, "rows": rows}

    def ask(self, question: str, *, model: str | None = None, effort: str | None = None, api_key: str | None = None, max_steps: int | None = None) -> dict:
        """Asks the server's agent a question about the data: {answer,
        steps (the queries it ran), ...}. Needs an Anthropic API key, the
        server's or `api_key`."""
        body = {k: v for k, v in {"question": question, "model": model, "effort": effort, "api_key": api_key, "max_steps": max_steps}.items() if v is not None}
        return self._call("POST", "/v1/agent", body)

    def close(self) -> None:
        pass

    def __enter__(self) -> "Client":
        return self

    def __exit__(self, *exc: Any) -> None:
        self.close()

    def __repr__(self) -> str:
        return f"<chloedb.Client {self.url}>"


class Session(Client):
    """A `Client` whose reads see its own writes, from any copy: it keeps
    the latest sequence numbers its writes answered (`seq_no` from a
    server, `seq_nos` from a coordinator, per shard) and sends them with
    every read (`query`, `search`, `get`, `get_many`, `scroll`, a SELECT
    by `sql`, a read plan by `run`), so a replica or a coordinator answers
    with a version that holds them::

        s = chloedb.connect("http://coordinator:7700").session()
        s.upsert({"id": "u1", "name": "Ada"})
        s.get("u1")  # sees the upsert, whichever copy answers

    The servers must number their writes (--seq-no); without it a write
    answers no number and a session reads as a plain client does. Numbers
    a read is given explicitly are sent with the session's, the later of
    each. `observe(written)` adds another client's write to what the
    session's reads must see. Safe to share between threads."""

    def __init__(self, url: str = "http://127.0.0.1:7700", **options: Any):
        super().__init__(url, **options)
        self._lock = threading.Lock()
        self.seq_no: int | None = None
        self.seq_nos: list[int] | None = None

    def observe(self, written: Any) -> None:
        """Makes later reads see this write: a `Written`, or a write's
        answer ({"seq_no": n} or {"seq_nos": [...]})."""
        seq_no = written.get("seq_no") if isinstance(written, Mapping) else getattr(written, "seq_no", None)
        seq_nos = written.get("seq_nos") if isinstance(written, Mapping) else getattr(written, "seq_nos", None)
        with self._lock:
            self.seq_no = _later(self.seq_no, None if seq_no is None else int(seq_no))
            self.seq_nos = _later_each(self.seq_nos, None if seq_nos is None else [int(n) for n in seq_nos])

    def _note(self, written: Written) -> Written:
        self.observe(written)
        return written

    def _after(self, min_seq_no: int | None = None, min_seq_nos: Sequence[int] | None = None) -> dict:
        with self._lock:
            return _after_body(_later(self.seq_no, min_seq_no), _later_each(self.seq_nos, None if min_seq_nos is None else list(min_seq_nos)))

    def session(self) -> "Session":
        return self

    def __repr__(self) -> str:
        return f"<chloedb.Session {self.url} seq_no={self.seq_no} seq_nos={self.seq_nos}>"
