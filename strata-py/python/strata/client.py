"""A client of a running strata-server, over its HTTP JSON API, with the
same methods as `Database` where the server offers them. It needs only the
standard library."""

from __future__ import annotations

import base64
import json
import os
import shutil
import time
import urllib.error
import urllib.parse
import urllib.request
from typing import Any, Iterable, Mapping

from ._search import Search, ordered_schema, params as _params
from ._native import StrataError
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


def _retry_after(headers: Any) -> float:
    """The seconds a 429's `Retry-After` says to wait (1 when it says none)."""
    try:
        return min(max(float(headers.get("retry-after") or 1), 0.0), 10.0)
    except (TypeError, ValueError):
        return 1.0


class Client(Search, FrameStore):
    """A strata-server at `url` ("http://host:7700"). `token` is the
    server's --token, if it has one. Errors the server reports raise
    `StrataError`; one it cannot be reached raises `urllib.error.URLError`.

    A request the server refuses for its load (429: behind on flushing its
    writes, its write queue full, too many queries running) did nothing,
    and is sent again after the `Retry-After` it gives, for up to
    `retry_for` seconds (0: never). A write that failed partway is not,
    unless it has an idempotency key.
    """

    def __init__(self, url: str = "http://127.0.0.1:7700", *, token: str | None = None, timeout: float = 30.0, retry_for: float = 10.0):
        self.url = url.rstrip("/")
        self.token = token if token is not None else os.environ.get("STRATA_TOKEN")
        self.timeout = timeout
        self.retry_for = retry_for
        self._schema: dict | None = None
        self._json_only = False  # a server that answers no Arrow

    def _call(self, method: str, path: str, body: Any = None, *, data: Any = None, headers: Mapping[str, str] | None = None, text: bool = False, out: Any = None) -> Any:
        """The answer's JSON, or its text with `text`; with `out`, a binary
        file, the answer is copied into it and its headers returned."""
        h = dict(headers or {})
        if body is not None:
            data = json.dumps(body, default=_default).encode()
            h["content-type"] = "application/json"
        if self.token:
            h["authorization"] = f"Bearer {self.token}"
        # A file is read once: an import is not sent again.
        again = isinstance(data, (bytes, type(None)))
        keyed = isinstance(body, Mapping) and bool(body.get("idempotency_key"))
        give_up = time.monotonic() + self.retry_for
        while True:
            req = urllib.request.Request(self.url + path, data=data, method=method, headers=h)
            try:
                with urllib.request.urlopen(req, timeout=self.timeout) as r:
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
                err = StrataError(f"{msg} (HTTP {e.code})")
                err.status = e.code
                err.answer = answer
                raise err from None

    def _schema_answer(self) -> dict:
        if self._schema is None:
            self._schema = self._call("GET", "/v1/schema")
        return self._schema

    @property
    def schema(self) -> dict:
        """The first table's schema in its short form: {table, key, fields}."""
        return ordered_schema(self._schema_answer()["short"])

    def tables(self) -> list[dict]:
        """Every table's schema in its short form, the first first."""
        return [ordered_schema(t["short"]) for t in self._call("GET", "/v1/tables")["tables"]]

    def _embedded(self) -> list[str]:
        return [e["field"] for e in self._schema_answer().get("embeddings", []) if e.get("loaded")]

    def health(self) -> dict:
        """{status, docs, segments, role, ...}, also when the status is "down"
        or "failing" (which the server answers with 503)."""
        try:
            return self._call("GET", "/v1/health")
        except StrataError as e:
            if e.status == 503 and isinstance(e.answer, dict) and "status" in e.answer:
                return e.answer
            raise

    def stats(self) -> dict:
        return self._call("GET", "/v1/stats")

    def __len__(self) -> int:
        return int(self.health()["docs"])

    def query(self, sql: str, params: Mapping[str, Any] | None = None, *, timeout_ms: int | None = None, **kw: Any) -> Result:
        """Runs a SELECT."""
        body: dict[str, Any] = {"sql": sql, "params": _params(params, kw)}
        if timeout_ms:
            body["timeout_ms"] = timeout_ms
        return Result._of(self._call("POST", "/v1/query", body))

    def query_arrow(self, sql: str, params: Mapping[str, Any] | None = None, *, timeout_ms: int | None = None, **kw: Any):
        """Runs a SELECT and returns its answer as a pyarrow Table, sent by
        the server as an Arrow IPC stream (no JSON either way), its columns
        typed as `Database.query_arrow` types them. Needs pyarrow. A server
        too old for Arrow answers JSON, which this types from the values."""
        return self._arrow_answer(sql, _params(params, kw), timeout_ms, {})

    def _arrow_answer(self, sql: str, params: Mapping[str, Any], timeout_ms: int | None, types: Mapping[str, dict]):
        import io

        from .frame import _pyarrow

        _pyarrow()
        if self._json_only:
            r = self.query(sql, params, timeout_ms=timeout_ms)
            return _client_arrow(r, types)
        body: dict[str, Any] = {"sql": sql, "params": params, "format": "arrow"}
        if timeout_ms:
            body["timeout_ms"] = timeout_ms
        buf = io.BytesIO()
        headers = self._call("POST", "/v1/query", body, out=buf)
        if "arrow" not in (headers.get("content-type") or ""):
            # A server from before Arrow answers ignores "format".
            self._json_only = True
            return _client_arrow(Result._of(json.loads(buf.getvalue() or b"null")), types)
        import pyarrow.ipc

        return pyarrow.ipc.open_stream(buf.getvalue()).read_all()

    def sql(self, sql: str, params: Mapping[str, Any] | None = None, *, idempotency_key: str | None = None, **kw: Any) -> Result | int:
        """Runs any statement: a SELECT's `Result`, or the number of rows an
        INSERT, UPDATE or DELETE wrote. With `idempotency_key`, a write sent
        again is not applied twice (see `write`)."""
        body: dict[str, Any] = {"sql": sql, "params": _params(params, kw)}
        if idempotency_key is not None:
            body["idempotency_key"] = idempotency_key
        r = self._call("POST", "/v1/sql", body)
        return Result._of(r) if "rows" in r else int(r["written"])

    def _query_logged(self, sql: str, params: Mapping[str, Any] | None, log: dict) -> Result:
        return Result._of(self._call("POST", "/v1/query", {"sql": sql, "params": _params(params, {}), "log": log}))

    def _export_rank_log(self, opts: dict) -> list[dict]:
        # NDJSON, a record a line; a failure partway is the last line.
        out = [json.loads(line) for line in self._call("POST", "/v1/rank_log/export", opts, text=True).splitlines() if line.strip()]
        if out and "error" in out[-1] and "request" not in out[-1]:
            err = StrataError(f"{out[-1]['error']} (HTTP {out[-1].get('status', 500)})")
            err.status, err.answer = out[-1].get("status", 500), out[-1]
            raise err
        return out

    def explain(self, sql: str, params: Mapping[str, Any] | None = None, **kw: Any) -> dict:
        """Runs the query and returns its plan: the plan IR as JSON ("ir")
        and as lines ("tree"), the executor's plan lines ("plan") and the
        bound query ("logical")."""
        return self._call("POST", "/v1/explain", {"sql": sql, "params": _params(params, kw)})

    def run(self, plan: Mapping[str, Any] | Query) -> Result | int:
        """Runs a plan (the "ir" of `explain`, or a `Query`): a read's
        `Result`, or the number of rows a write wrote."""
        if isinstance(plan, Query):
            plan = plan.plan()
        r = self._call("POST", "/v1/plan", {"plan": plan})
        return Result._of(r) if "rows" in r else int(r["written"])

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

    def sid_next(self, prefixes: Iterable[str], *, field: str, where: str | None = None, params: Mapping[str, Any] | None = None, table: str | None = None, **kw: Any) -> dict[str, dict[str, int]]:
        """The codes that can follow each prefix of semantic-ID field
        `field` among the rows `where` matches (POST /v1/sid/next), summed
        over the shards on a coordinator: {prefix: {code: rows}}."""
        body = {"field": field, "prefixes": list(prefixes), "where": where, "params": _params(params, kw), "table": table}
        got = self._call("POST", "/v1/sid/next", {k: v for k, v in body.items() if v is not None})
        return {n["prefix"]: {c["code"]: c["count"] for c in n["next"]} for n in got["next"]}

    def write(self, ops: list[dict], *, idempotency_key: str | None = None) -> int:
        """Write operations as /v1/write takes them ({"op": "insert",
        "doc": {...}}, {"op": "delete", "key": k}, ...), in one request.
        With `idempotency_key`, the same request sent again (after a lost
        answer, or with the `idempotency_key` of an error whose `answer`
        says it failed partway through a coordinator, `"partial": True`)
        is not applied twice."""
        body: dict[str, Any] = {"ops": ops}
        if idempotency_key is not None:
            body["idempotency_key"] = idempotency_key
        return int(self._call("POST", "/v1/write", body)["written"])

    def _ops(self, op: str, rows: list, table: str | None) -> int:
        named = {} if table is None else {"table": table}
        n = 0
        for i in range(0, len(rows), WRITE_BATCH):
            n += self.write([{"op": op, **named, "doc": r} for r in rows[i : i + WRITE_BATCH]])
        return n

    def insert(self, rows: Mapping[str, Any] | Iterable[Mapping[str, Any]], *, table: str | None = None) -> int:
        """Inserts a row or rows, whose keys must be new, into the first
        table or `table`, 1,000 rows a request. Returns how many."""
        return self._ops("insert", _rows(rows), table)

    def upsert(self, rows: Mapping[str, Any] | Iterable[Mapping[str, Any]], *, table: str | None = None) -> int:
        """Inserts a row or rows, replacing the rows that have their keys,
        1,000 rows a request. Returns how many."""
        return self._ops("upsert", _rows(rows), table)

    def delete(self, keys: Any, *, table: str | None = None, routing: Any = None) -> int:
        """Deletes the rows with this key or these keys. Through a
        coordinator, `routing` gives the `shard_by` value of a table placed
        by a column."""
        keys = [keys] if isinstance(keys, (str, int)) else list(keys)
        named = {k: v for k, v in {"table": table, "routing": routing}.items() if v is not None}
        n = 0
        for i in range(0, len(keys), WRITE_BATCH):
            n += self.write([{"op": "delete", **named, "key": k} for k in keys[i : i + WRITE_BATCH]])
        return n

    def _table(self, table: str | None) -> str:
        return table if table is not None else self.schema["table"]

    def get(self, key: Any, *, table: str | None = None) -> dict | None:
        """The row with this primary key in the first table or `table`, read
        without SQL (GET /v1/kv/:table/:key): a dict of the columns `SELECT
        *` answers, or None when no live row has the key (or its
        `expire_by` time has passed). A coordinator asks the key's shard."""
        path = f"/v1/kv/{urllib.parse.quote(self._table(table), safe='')}/{urllib.parse.quote(str(key), safe='')}"
        try:
            return self._call("GET", path)["row"]
        except StrataError as e:
            if e.status == 404 and isinstance(e.answer, dict) and e.answer.get("found") is False:
                return None
            raise

    def get_many(self, keys: Iterable[Any], *, table: str | None = None, columns: Iterable[str] | None = None) -> list[dict | None]:
        """`get` of several keys (POST /v1/kv/get), 10,000 a request, in
        their order; with `columns`, each row of those columns alone (a
        ranker's features, say), which reads no other."""
        keys = list(keys)
        body: dict[str, Any] = {"table": self._table(table)}
        if columns is not None:
            body["columns"] = list(columns)
        out: list[dict | None] = []
        for i in range(0, len(keys), GET_BATCH):
            out += self._call("POST", "/v1/kv/get", {**body, "keys": keys[i : i + GET_BATCH]})["rows"]
        return out

    def put(self, key: Any, row: Mapping[str, Any], *, table: str | None = None, if_seq_no: int | None = None) -> int:
        """Writes `row` as the row with this key (PUT /v1/kv/:table/:key),
        in place of any that has it; with `if_seq_no`, only while that
        row's sequence number is that. Returns the rows written."""
        path = f"/v1/kv/{urllib.parse.quote(self._table(table), safe='')}/{urllib.parse.quote(str(key), safe='')}"
        if if_seq_no is not None:
            path += f"?if_seq_no={int(if_seq_no)}"
        return int(self._call("PUT", path, dict(row))["written"])

    def stream(self, events: Iterable[Mapping[str, Any]]) -> dict:
        """Writes `events` through /v1/stream as they come, in batches the
        server makes: each an operation, or one event's operations as
        {"ops": [...], "source": {"name": ..., "offset": n, "ts": ...}},
        which a batch never splits. A source's offsets increase along the
        stream; one the server has reached already (a replay's) is skipped,
        so resuming from `source(name) + 1`, or from before it, applies
        each event once. Returns the last answer, {"done": True, "lines",
        "written", "skipped", "batches"}; a failure raises StrataError,
        whose `answer["lines"]` counts the events written before it.
        `strata.ingest` reads Kafka into a stream."""

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
        req = urllib.request.Request(self.url + "/v1/stream", data=chunks, method="POST", headers=h)
        last: dict = {}
        try:
            with urllib.request.urlopen(req, timeout=self.timeout) as r:
                for raw in r:
                    if raw.strip():
                        try:
                            last = json.loads(raw)
                        except ValueError:
                            last = {}  # cut short
                            break
        except urllib.error.HTTPError as e:
            raw = e.read()
            err = StrataError(f"{raw.decode(errors='replace')} (HTTP {e.code})")
            err.status, err.answer = e.code, None
            raise err from None
        if "error" in last or not last.get("done"):
            err = StrataError(f"{last.get('error', 'the stream ended early')} (HTTP {last.get('status', 500)})")
            err.status, err.answer = last.get("status", 500), last
            raise err
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
        (`fts`) and the Strata schemas (`schemas`). Returns the tables."""
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
        return [t for t in (headers.get("x-strata-tables") or "").split(",") if t]

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
        not exist; the file keeps the table's Strata schema. Returns the
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
        return headers.get("x-strata-tables") or ""

    # What frames need of a store (FrameStore). Until the server answers in
    # Arrow, a frame's answer comes as JSON and is typed here by the
    # fields its columns read; rows go up as Parquet.

    def _frame_arrow(self, sql: str, params: Mapping[str, Any], types: Mapping[str, dict]) -> Any:
        try:
            return self._arrow_answer(sql, _params(params, {}), None, types)
        except StrataError as e:
            # A server answers at most --max-result-window rows a query.
            if "a query returns at most" not in str(e) and "the most a query returns" not in str(e):
                raise
            err = StrataError(f"{e}; a frame reads its rows in one query, so a server that holds more for it needs a larger --max-result-window (0: none)")
            err.status, err.answer = getattr(e, "status", None), getattr(e, "answer", None)
            raise err from None

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
        return f"<strata.Client {self.url}>"
