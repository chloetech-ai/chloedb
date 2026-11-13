"""ChloeDB as a Python DB-API 2.0 driver (PEP 249).

`chloedb.dbapi.connect(target)` opens a connection to a database in this
process (a path) or to a chloedb-server (an `http(s)://` URL), or wraps a
`chloedb.Database` or `chloedb.Client` already open. Tools written for any
DB-API driver then work over ChloeDB: `pandas.read_sql`, scripts written
for `sqlite3`, report tools::

    import chloedb.dbapi

    conn = chloedb.dbapi.connect("books.chloedb")
    cur = conn.cursor()
    cur.execute("SELECT id, title FROM books WHERE year >= :y ORDER BY year", {"y": 2000})
    for id, title in cur:
        print(id, title)
    cur.execute("UPDATE books SET year = :y WHERE id = :id", {"y": 2001, "id": "b1"})
    cur.rowcount                      # rows written

Parameters are named, as ChloeDB's SQL names them (`paramstyle =
"named"`: `:name`, bound from a mapping); a vector, a list or
`{"embed": text}` binds as the SQL takes it. ChloeDB writes each statement
in its own transaction, as it commits: `commit()` has nothing to do, and
`rollback()` raises `NotSupportedError`, since there is nothing to undo.
`with conn:` commits at its end and leaves the connection open, as in
`sqlite3`; `contextlib.closing(conn)` closes it.

A read through a server is fetched a page at a time as the rows are
asked for, past the server's result window; a `TIMESTAMP` column comes
back as a `datetime` in UTC.
"""

from __future__ import annotations

import datetime as _dt
import math
import os
import re
import time as _time
import warnings as _warnings
from typing import Any, Iterator, Mapping, Sequence

from ._native import ChloeDBError
from ._adapters import open_store, release_store

apilevel = "2.0"
# Threads may share the module, not connections: a connection's cursor
# keeps the last answer it read.
threadsafety = 1
paramstyle = "named"

__all__ = [
    "apilevel",
    "threadsafety",
    "paramstyle",
    "connect",
    "Connection",
    "Cursor",
    "Warning",
    "Error",
    "InterfaceError",
    "DatabaseError",
    "DataError",
    "OperationalError",
    "IntegrityError",
    "InternalError",
    "ProgrammingError",
    "NotSupportedError",
    "Date",
    "Time",
    "Timestamp",
    "DateFromTicks",
    "TimeFromTicks",
    "TimestampFromTicks",
    "Binary",
    "STRING",
    "BINARY",
    "NUMBER",
    "DATETIME",
    "ROWID",
]


# Exceptions (PEP 249's hierarchy). Each carries the ChloeDB error's
# stable `code`, and the server's `status` and `answer` when it came from
# one; the error itself is the exception's `__cause__`.


class Warning(Exception):  # noqa: A001 - the name PEP 249 gives it
    pass


class Error(Exception):
    code: str | None = None
    status: int | None = None
    answer: Any = None


class InterfaceError(Error):
    pass


class DatabaseError(Error):
    pass


class DataError(DatabaseError):
    pass


class OperationalError(DatabaseError):
    pass


class IntegrityError(DatabaseError):
    pass


class InternalError(DatabaseError):
    pass


class ProgrammingError(DatabaseError):
    pass


class NotSupportedError(DatabaseError):
    pass


_BY_CODE = {
    "bad_request": ProgrammingError,
    "not_found": ProgrammingError,
    "conflict": IntegrityError,
    "unauthorized": OperationalError,
    "partial_write": OperationalError,
    "busy": OperationalError,
    "overloaded": OperationalError,
    "timeout": OperationalError,
    "unavailable": OperationalError,
    "memory": OperationalError,
    "io": OperationalError,
    "corrupt": InternalError,
    "internal": InternalError,
}


def _translate(e: BaseException) -> Error:
    if isinstance(e, Error):
        return e
    if isinstance(e, ChloeDBError):
        code = getattr(e, "code", None)
        msg = str(e)
        cls = _BY_CODE.get(code or "", DatabaseError)
        if cls is ProgrammingError and ("duplicate" in msg or "already exists" in msg and "key" in msg):
            cls = IntegrityError
        if "not null" in msg.lower() or "NOT NULL" in msg:
            cls = IntegrityError
        out = cls(msg)
    elif isinstance(e, TypeError):
        out = ProgrammingError(str(e))
    elif isinstance(e, (OSError, TimeoutError)):
        out = OperationalError(str(e))
    else:
        out = DatabaseError(str(e))
    out.code = getattr(e, "code", None)
    out.status = getattr(e, "status", None)
    out.answer = getattr(e, "answer", None)
    return out


# Types and constructors.

Date = _dt.date
Time = _dt.time
Timestamp = _dt.datetime


def DateFromTicks(ticks: float) -> _dt.date:
    return Date(*_time.localtime(ticks)[:3])


def TimeFromTicks(ticks: float) -> _dt.time:
    return Time(*_time.localtime(ticks)[3:6])


def TimestampFromTicks(ticks: float) -> _dt.datetime:
    return Timestamp(*_time.localtime(ticks)[:6])


def Binary(data: bytes) -> bytes:
    """PEP 249's constructor of binary values: ChloeDB has no binary type
    (keep bytes as base64 text, or as a MEDIA field's file)."""
    raise NotSupportedError("ChloeDB has no binary type: store bytes as base64 text (base64.b64encode(data).decode())")


class _Type:
    """A DB-API type object: equal to each of the type codes it names."""

    def __init__(self, name: str, *codes: str):
        self.name = name
        self.codes = frozenset(codes)

    def __eq__(self, other: object) -> bool:
        if isinstance(other, _Type):
            return self is other
        return other in self.codes

    def __hash__(self) -> int:
        return hash(self.name)

    def __repr__(self) -> str:
        return f"chloedb.dbapi.{self.name}"


# A column's type code is ChloeDB's type of it when the column is a
# table's field, else the type of its values.
STRING = _Type("STRING", "text", "keyword", "json", "media")
BINARY = _Type("BINARY", "binary")
NUMBER = _Type("NUMBER", "int", "float", "bool")
DATETIME = _Type("DATETIME", "timestamp")
ROWID = _Type("ROWID", "rowid")

_READS = ("select", "with", "explain", "values", "(", "show", "describe")
# Reads whose rows a server pages (a scroll); the others answer at once.
_PAGED = ("select", "with", "(")
# Statements that change a table's columns, after which their types are
# read again.
_DDL = ("create", "alter", "drop")

# The rows a page of a read through a server holds (a server's default
# --max-result-window).
PAGE_ROWS = 10_000


def _statement(sql: str) -> str:
    """The statement from its first word, past comments and blanks."""
    s = sql.lstrip()
    while s.startswith("--") or s.startswith("/*"):
        if s.startswith("--"):
            s = s.split("\n", 1)[1].lstrip() if "\n" in s else ""
        else:
            s = s.split("*/", 1)[1].lstrip() if "*/" in s else ""
    return s


def _reads(sql: str) -> bool:
    return _statement(sql)[:10].lower().startswith(_READS)


def _timestamp(v: Any) -> Any:
    """A TIMESTAMP's ISO 8601 text as a `datetime` in UTC (as text when
    its year is outside 1 to 9999)."""
    if not isinstance(v, str):
        return v
    try:
        return _dt.datetime.fromisoformat(v[:-1] + "+00:00" if v.endswith("Z") else v)
    except ValueError:
        return v


def _tables_named(sql: str) -> list[str]:
    """The tables a statement reads (after FROM and JOIN), for the types
    of its columns."""
    out = []
    for m in re.finditer(r'\b(?:from|join)\s+(?:"((?:[^"]|"")+)"|(\w+))', sql, re.I):
        out.append(m.group(1).replace('""', '"') if m.group(1) is not None else m.group(2))
    return out


def _value_type(v: Any) -> str | None:
    if v is None:
        return None
    if isinstance(v, bool):
        return "bool"
    if isinstance(v, int):
        return "int"
    if isinstance(v, float):
        return "float"
    if isinstance(v, (bytes, bytearray)):
        return "binary"
    if isinstance(v, (list, tuple)):
        return "list"
    if isinstance(v, dict):
        return "json"
    return "text"


def _bind(params: Any) -> dict | None:
    if params is None:
        return None
    if isinstance(params, Mapping):
        out = {}
        for k, v in params.items():
            if isinstance(v, _dt.datetime):
                v = v.isoformat() if v.tzinfo else v.isoformat() + "Z"
            elif isinstance(v, _dt.date):
                v = v.isoformat()
            elif isinstance(v, float) and math.isnan(v):
                v = None  # as a database in this process binds it
            elif isinstance(v, (bytes, bytearray, memoryview)):
                raise NotSupportedError(f"parameter {k!r} is bytes, and ChloeDB has no binary type: bind base64 text")
            out[k] = v
        return out
    if isinstance(params, (list, tuple)) and not params:
        return None
    raise ProgrammingError("chloedb.dbapi binds named parameters (:name) from a mapping, not a sequence (paramstyle is 'named')")


def _pending(store: Any) -> bool:
    from . import _Pending

    return isinstance(getattr(store, "_db", None), _Pending)


def _first_table(store: Any, sql: str, params: Any) -> int:
    """A database that does not exist yet is made by its first `CREATE
    TABLE`: the statement runs on a scratch database, and the database is
    made with the schema it gave the table."""
    import tempfile

    if not re.match(r"create\s+table\b", _statement(sql), re.I):
        raise ProgrammingError(f"{store._db.path} has no tables yet: the first statement makes one (CREATE TABLE)")
    from . import Database

    with tempfile.TemporaryDirectory() as d:
        with Database.create(os.path.join(d, "scratch.chloedb"), {"table": "__scratch", "key": "id", "fields": [{"name": "id", "type": "keyword"}]}) as scratch:
            scratch.sql(sql, params)
            made = [t for t in scratch.tables() if t["table"] != "__scratch"]
    if not made:
        raise ProgrammingError("the statement made no table")
    store._create(made[0])
    return 0


class Cursor:
    """A cursor of a `Connection`: `execute` a statement, then `fetchone`,
    `fetchmany` or `fetchall` its rows (tuples, in `description`'s
    order). A read through a server fetches its rows a page at a time as
    they are asked for; `rowcount` is how many rows the read answered
    once that is known (when they are all fetched, or came in one page),
    -1 until then. `messages` holds a `Warning` when the answer was cut
    short by a limit the query did not write (a ranked query without
    LIMIT answers its best 100)."""

    arraysize: int = 1

    def __init__(self, connection: "Connection"):
        self._conn = connection
        self._rows: list[tuple] = []
        self._pos = 0
        self._pages: Any = None  # the pages of a read still to fetch
        self._stamps: list[int] = []  # the columns that are TIMESTAMPs
        self._seen = 0
        self._closed = False
        self.description: list[tuple] | None = None
        self.rowcount: int = -1
        self.lastrowid = None
        self.arraysize = 1
        self.messages: list[tuple[type, Exception]] = []

    @property
    def connection(self) -> "Connection":
        return self._conn

    def _check(self) -> Any:
        if self._closed:
            raise InterfaceError("the cursor is closed")
        return self._conn._live()

    def _end_pages(self) -> None:
        if self._pages is not None:
            self._pages.close()
            self._pages = None

    def close(self) -> None:
        self._end_pages()
        self._closed = True
        self._rows = []

    def __enter__(self) -> "Cursor":
        return self

    def __exit__(self, *exc: Any) -> None:
        self.close()

    def _types(self, columns: Sequence[str], rows: Sequence[Sequence[Any]], tables: Sequence[str]) -> list[str | None]:
        fields = self._conn._fields(tables)
        out: list[str | None] = []
        for i, c in enumerate(columns):
            t = fields.get(c)
            if t is None:
                t = next((_value_type(r[i]) for r in rows if r[i] is not None), None)
            out.append(t)
        return out

    def _take(self, page: Any) -> list[tuple]:
        """A page's rows as tuples, TIMESTAMPs as datetimes; a page cut
        short says so in `messages`."""
        if getattr(page, "truncated", False) and not self.messages:
            cut = getattr(page, "implicit_limit", None)
            msg = f"the answer was cut to its best {cut} rows: a ranked query without LIMIT answers that many; give a LIMIT" if cut else "the answer holds fewer rows than matched (a page of a fused ranking past its candidates)"
            self.messages.append((Warning, Warning(msg)))
            _warnings.warn(msg, stacklevel=4)
        rows = [tuple(r) for r in page.rows]
        if self._stamps:
            for n, r in enumerate(rows):
                r = list(r)
                for i in self._stamps:
                    r[i] = _timestamp(r[i])
                rows[n] = tuple(r)
        self._seen += len(rows)
        return rows

    def execute(self, operation: str, parameters: Mapping[str, Any] | None = None) -> "Cursor":
        """Runs one statement. A read (`SELECT`, `WITH`, `EXPLAIN`) keeps
        its rows to fetch; a write (`INSERT`, `UPDATE`, `DELETE`, `CREATE
        TABLE`, ...) sets `rowcount` to the rows it wrote."""
        store = self._check()
        params = _bind(parameters)
        self._end_pages()
        self._rows, self._pos, self.description, self.rowcount, self._stamps, self._seen = [], 0, None, -1, [], 0
        self.messages = []
        more = False
        try:
            first = _statement(operation)[:10].lower()
            if first.startswith(_PAGED) and hasattr(store, "_pages"):
                # Through a server: a page now, the rest as they are asked
                # for, past the server's result window.
                self._pages = store._pages(operation, dict(params or {}), page_size=PAGE_ROWS)
                result, more = next(self._pages)
                if not more:
                    self._end_pages()
            elif _reads(operation):
                result = store.query(operation, params)
            elif _pending(store):
                result = _first_table(store, operation, params)
            else:
                result = store.sql(operation, params)
        except Exception as e:  # noqa: BLE001 - every error is a DB-API one
            self._end_pages()
            raise _translate(e) from e
        if first.startswith(_DDL):
            self._conn._schemas = None
        if hasattr(result, "columns"):
            columns = list(result.columns)
            tables = _tables_named(operation)
            types = self._types(columns, result.rows, tables)
            self._stamps = [i for i, t in enumerate(types) if t == "timestamp"]
            self.description = [(c, t, None, None, None, None, None) for c, t in zip(columns, types)]
            self._rows = self._take(result)
            self.rowcount = -1 if more else self._seen
        else:
            self.rowcount = int(result) if result is not None else -1
        return self

    def executemany(self, operation: str, seq_of_parameters: Sequence[Mapping[str, Any]]) -> "Cursor":
        """Runs the statement once for each mapping of parameters;
        `rowcount` is the rows they wrote in all."""
        total = 0
        for params in seq_of_parameters:
            self.execute(operation, params)
            if self.rowcount > 0:
                total += self.rowcount
        self._end_pages()
        self._rows, self._pos, self.description = [], 0, None
        self.rowcount = total
        return self

    def _need_rows(self) -> None:
        self._check()
        if self.description is None:
            raise ProgrammingError("the last statement answered no rows to fetch")

    def _fill(self, n: int | None) -> None:
        """Fetches pages until `n` rows past the position are held (every
        page for None)."""
        while self._pages is not None and (n is None or len(self._rows) - self._pos < n):
            try:
                page, more = next(self._pages)
            except StopIteration:
                more, page = False, None
            except Exception as e:  # noqa: BLE001
                self._end_pages()
                raise _translate(e) from e
            if self._pos:
                self._rows, self._pos = self._rows[self._pos :], 0
            if page is not None:
                self._rows.extend(self._take(page))
            if not more:
                self._end_pages()
                self.rowcount = self._seen

    def fetchone(self) -> tuple | None:
        self._need_rows()
        self._fill(1)
        if self._pos >= len(self._rows):
            return None
        row = self._rows[self._pos]
        self._pos += 1
        return row

    def fetchmany(self, size: int | None = None) -> list[tuple]:
        self._need_rows()
        n = self.arraysize if size is None else size
        self._fill(n)
        out = self._rows[self._pos : self._pos + n]
        self._pos += len(out)
        return out

    def fetchall(self) -> list[tuple]:
        self._need_rows()
        self._fill(None)
        out = self._rows[self._pos :]
        self._pos = len(self._rows)
        return out

    def __iter__(self) -> Iterator[tuple]:
        while (row := self.fetchone()) is not None:
            yield row

    def setinputsizes(self, sizes: Any) -> None:
        pass

    def setoutputsize(self, size: Any, column: Any = None) -> None:
        pass


class Connection:
    """A DB-API connection over a `chloedb.Database` or `chloedb.Client`
    (`store`). Errors are this module's (`connection.Error` too, as PEP
    249's extension has them)."""

    Warning = Warning
    Error = Error
    InterfaceError = InterfaceError
    DatabaseError = DatabaseError
    DataError = DataError
    OperationalError = OperationalError
    IntegrityError = IntegrityError
    InternalError = InternalError
    ProgrammingError = ProgrammingError
    NotSupportedError = NotSupportedError

    def __init__(self, store: Any, owned: bool):
        self.store = store
        self._owned = owned
        self._closed = False
        # The tables' schemas, read once, and again after a statement of
        # this connection that changes a table.
        self._schemas: list[dict] | None = None

    def _live(self) -> Any:
        if self._closed:
            raise InterfaceError("the connection is closed")
        return self.store

    def _fields(self, tables: Sequence[str]) -> dict[str, str]:
        """Column name to ChloeDB type, from the tables a query read (its
        first table's when the answer does not say)."""
        known = {t["table"] for t in self._schemas or ()}
        # Read again when a query names a table made since (by another
        # connection, an adapter or another process).
        if self._schemas is None or any(t not in known for t in tables):
            try:
                self._schemas = list(self.store.tables())
            except Exception:  # noqa: BLE001 - types are a courtesy
                return {}
        schemas = self._schemas
        by = {t["table"]: t for t in schemas}
        chosen = [by[t] for t in tables if t in by] or schemas[:1]
        out: dict[str, str] = {}
        for s in chosen:
            for f in s.get("fields", []):
                out.setdefault(f["name"], f["type"])
        return out

    def cursor(self) -> Cursor:
        self._live()
        return Cursor(self)

    def execute(self, operation: str, parameters: Mapping[str, Any] | None = None) -> Cursor:
        """A new cursor that has run `operation` (as `sqlite3`'s
        shortcut)."""
        return self.cursor().execute(operation, parameters)

    def commit(self) -> None:
        """Each statement committed as it ran: nothing to do."""
        self._live()

    def rollback(self) -> None:
        self._live()
        raise NotSupportedError("ChloeDB commits each statement as it runs: there is no transaction to roll back")

    def close(self) -> None:
        """Closes the connection. A database it opened by path closes with
        the last connection or adapter of this process that holds it; a
        `Database` or `Client` it was given stays open."""
        if self._closed:
            return
        self._closed = True
        if self._owned == "path":
            release_store(self.store)
        elif self._owned:
            self.store.close()

    @property
    def closed(self) -> bool:
        return self._closed

    def __enter__(self) -> "Connection":
        return self

    def __exit__(self, *exc: Any) -> None:
        """Ends a `with conn:` block as `sqlite3` does: commits (each
        statement committed as it ran) and leaves the connection open."""
        if not self._closed:
            self.commit()


def connect(target: Any = "http://127.0.0.1:7700", *, token: str | None = None, models: Any = None, read_only: bool = False) -> Connection:
    """A DB-API connection to `target`: a database's path (opened in this
    process; `read_only` to read one another process writes), an
    `http(s)://` URL of a chloedb-server (`token` or $CHLOEDB_TOKEN), or a
    `chloedb.Database` or `chloedb.Client` already open (which `close()`
    then leaves open)."""
    try:
        if read_only and not hasattr(target, "query"):
            from . import Database

            return Connection(Database.open(os.fspath(target), models=models, read_only=True), True)
        store = open_store(target, models=models, token=token)
    except Exception as e:  # noqa: BLE001
        raise _translate(e) from e
    if isinstance(target, (str, os.PathLike)) and not os.fspath(target).startswith(("http://", "https://")):
        return Connection(store, "path")
    # A client of a URL is the connection's; a Database or Client given is
    # the caller's, and so is the Session made of it.
    return Connection(store, isinstance(target, (str, os.PathLike)))
