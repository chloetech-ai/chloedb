"""Strata from Python.

`Database` runs the database in this process (a file, or a directory of
segment files); `Client` talks to a running `strata-server` over HTTP. Both
answer queries with the same `Result` and write rows the same way, so code
can move from one to the other::

    import strata

    db = strata.open("books.strata", schema={
        "table": "books", "key": "id",
        "fields": [
            {"name": "id", "type": "keyword"},
            {"name": "title", "type": "text"},
            {"name": "year", "type": "int"},
        ],
    })
    db.upsert([{"id": "b1", "title": "The river and the dragon", "year": 1999}])
    for row in db.search("dragon"):
        print(row["id"], row["title"])
    db.query("SELECT year, COUNT(*) FROM books GROUP BY year").rows

Rows are dicts of field name to value: str, int, float, None, a list for a
list field, a list of floats (or a numpy array) for a vector, a dict of term
weights for a sparse field. Query parameters (``:name`` in the SQL) are
passed as a dict or as keyword arguments; ``{"embed": "some text"}`` binds
the text's vector by an embedded field's model, ``{"vector_of": key}`` a
stored row's vector.
"""

from __future__ import annotations

import builtins
import json
import os
from typing import Any, Iterable, Mapping

from . import ngrams, train
from ._native import _strata
from ._search import Search, ordered_schema, params as _params
from .build import (
    Expr,
    Query,
    bm25,
    case,
    clamp,
    coalesce,
    at_least,
    col,
    count,
    count_true,
    exp,
    features,
    gbdt,
    cross_encoder,
    greatest,
    hops,
    least,
    lit,
    ln,
    log,
    match,
    maxsim,
    minmax,
    path,
    ppr,
    rank,
    ranked,
    reachable,
    rrf,
    score,
    tmm,
    zscore,
    sigmoid,
    similarity,
    sqrt,
    when,
)
from ._native import StrataError
from .client import Client
from .frame import Frame, FrameStore, GroupBy, concat, len_, _arrow_table
from .nearby import Nearby, NearbyResult, nearby
from .ngrams import next_items
from .result import ArrowResult, Result

__all__ = [
    "Database",
    "Client",
    "Nearby",
    "NearbyResult",
    "nearby",
    "Result",
    "ArrowResult",
    "StrataError",
    "open",
    "connect",
    "from_sqlite",
    "plan_sqlite",
    "from_parquet",
    "plan_parquet",
    "Query",
    "Expr",
    "col",
    "lit",
    "match",
    "at_least",
    "bm25",
    "similarity",
    "maxsim",
    "rrf",
    "ranked",
    "minmax",
    "tmm",
    "zscore",
    "rank",
    "gbdt",
    "cross_encoder",
    "ln",
    "log",
    "exp",
    "sqrt",
    "sigmoid",
    "clamp",
    "least",
    "greatest",
    "coalesce",
    "case",
    "count_true",
    "score",
    "hops",
    "features",
    "count",
    "reachable",
    "path",
    "ppr",
    "train",
    "ngrams",
    "next_items",
    "upgrade",
    "Frame",
    "GroupBy",
    "concat",
    "when",
    "len",
]


def __getattr__(name: str) -> Any:
    # `strata.len()`, as Polars' `pl.len()`, without hiding the builtin
    # `len` inside this module.
    if name == "len":
        return len_
    raise AttributeError(f"module 'strata' has no attribute {name!r}")


def upgrade(path: str | os.PathLike) -> tuple[str, int, int] | None:
    """Copy the database at `path`, written by a version of Strata from
    before field ids (which no longer opens), into this version's format in
    its place. The old one is kept as ``<path>.before-field-ids``. Returns
    None when it needs no upgrade, else (the old database's path, tables
    copied, rows copied)."""
    r = _strata.upgrade(os.fspath(path))
    return None if r is None else (os.fspath(r[0]), r[1], r[2])


def _schema_json(schema: Any) -> str:
    """A schema given as a dict, JSON text or the path of a JSON file."""
    if isinstance(schema, Mapping):
        return json.dumps(schema)
    if isinstance(schema, (str, os.PathLike)):
        text = os.fspath(schema)
        if text.lstrip().startswith("{"):
            return text
        with builtins.open(text, encoding="utf-8") as f:
            return f.read()
    raise TypeError("a schema is a dict, JSON text or the path of a JSON file")


def _schemas_json(schemas: Any) -> str | None:
    """Schemas for a SQLite import: a dict, a list of them, a plan
    (`plan_sqlite`), JSON text or the path of a JSON file."""
    if schemas is None:
        return None
    if isinstance(schemas, (Mapping, list)):
        return json.dumps(schemas)
    if isinstance(schemas, (str, os.PathLike)):
        text = os.fspath(schemas)
        if text.lstrip().startswith(("{", "[")):
            return text
        with builtins.open(text, encoding="utf-8") as f:
            return f.read()
    raise TypeError("schemas are a dict, a list, JSON text or the path of a JSON file")


def _names(tables: str | Iterable[str] | None) -> list[str] | None:
    if tables is None:
        return None
    return [tables] if isinstance(tables, str) else list(tables)


class _Pending:
    """A database `connect` was given the path of, not made yet: its
    first `create_table` makes it."""

    def __init__(self, path: str, directory: bool, models: Any, threads: int | None):
        self.path, self.directory, self.models, self.threads = path, directory, models, threads

    def __getattr__(self, name: str) -> Any:
        raise StrataError(f"{self.path} has no tables yet: make one with create_table(name, data)")

    def closed(self) -> bool:
        return False

    def close(self) -> None:
        pass


class Database(Search, FrameStore):
    """A Strata database opened in this process.

    Create one with `Database.create` (or `strata.open` with a schema), open
    an existing one with `Database.open`, or `strata.connect(path)` for
    either. Only one process may have a database open: to share one, serve
    it with `strata-server` and use `Client`. Calls release the GIL, so
    threads can query at once. `db["table"]` is a lazy `Frame` of a table.
    """

    def __init__(self, inner: Any):
        self._db = inner
        #: What the last SQLite or Parquet import into it did (`from_sqlite`,
        #: `import_sqlite`, `from_parquet`, `import_parquet`).
        self.last_import: dict | None = None

    @classmethod
    def create(cls, path: str | os.PathLike, schema: Any, *, directory: bool = False, models: str | os.PathLike | None = None, threads: int | None = None) -> "Database":
        """Creates a database at `path` with `schema` (a dict, JSON text, or
        the path of a JSON file; the short form or the long one).
        `directory` keeps it as a directory of segment files instead of one
        file. `models` is where embedded fields' models are (default
        $STRATA_MODELS or ~/.local/share/strata/models; see `strata-server
        fetch-model`). `threads` is how many cores vector indexes build on
        and each query searches segments on (default: every core)."""
        return cls(_strata.Database.create(os.fspath(path), _schema_json(schema), directory, None if models is None else os.fspath(models), threads))

    @classmethod
    def open(cls, path: str | os.PathLike, *, models: str | os.PathLike | None = None, threads: int | None = None, read_only: bool = False) -> "Database":
        """Opens the database at `path`; `threads` as `create` takes it.
        `read_only` opens it to read while another process writes it (a
        web server's other workers, a script): it sees each commit of the
        writer, and every write fails. Without it only one process may have
        the database open."""
        return cls(_strata.Database.open(os.fspath(path), None if models is None else os.fspath(models), threads, read_only))

    @property
    def read_only(self) -> bool:
        """Whether it was opened with `read_only`."""
        return self._db.read_only()

    def refresh(self) -> bool:
        """Opened `read_only`: reads what the writer committed since the
        last query, now (each query does so by itself); whether anything
        changed. False for the writer."""
        return self._db.refresh()

    @property
    def schema(self) -> dict:
        """The first table's schema in its short form: {table, key, fields}."""
        return ordered_schema(json.loads(self._db.schema_json()))

    def tables(self) -> list[dict]:
        """Every table's schema in its short form, the first first (none
        before `connect`'s new file has its first table)."""
        if isinstance(self._db, _Pending):
            return []
        return [ordered_schema(json.loads(t)) for t in self._db.tables_json()]

    def add_column(self, name: str, type: str, *, table: str | None = None, **options: Any) -> None:
        """Adds a column to the first table or `table`, as `ALTER TABLE ...
        ADD COLUMN` does: `options` are the field's, in the short form of a
        schema (`default`, `not_null`, `indexed`, `embed` and `model`, ...).
        Rows written before it read it as its default, or None.

            db.add_column("price", "float", default=0.0)
            db.add_column("emb", "vector", embed=["title"], model="potion-retrieval-32M")
        """
        self._db.alter_table_json(json.dumps([{"add": {"name": name, "type": type, **options}}]), table)

    def drop_column(self, name: str, *, table: str | None = None) -> None:
        """Drops a column of the first table or `table`, with its values,
        as `ALTER TABLE ... DROP COLUMN` does. The key, and a column another
        field or the table's options name, stay."""
        self._db.alter_table_json(json.dumps([{"drop": name}]), table)

    def rename_column(self, old: str, new: str, *, table: str | None = None) -> None:
        """Renames a column of the first table or `table`, keeping its
        values; the fields and options that name it follow."""
        self._db.alter_table_json(json.dumps([{"rename": [old, new]}]), table)

    @property
    def embed_errors(self) -> dict[str, str]:
        """Embedded fields whose model did not open, with the reason: rows
        written here must bring those fields' vectors."""
        return dict(self._db.embed_errors)

    def _embedded(self) -> list[str]:
        broken = self.embed_errors
        return [f["name"] for f in self.schema["fields"] if f.get("embed") and f["name"] not in broken]

    def __len__(self) -> int:
        return self._db.count()

    def query(self, sql: str, params: Mapping[str, Any] | None = None, **kw: Any) -> Result:
        """Runs a SELECT."""
        return Result._of(self._db.query(sql, _params(params, kw)))

    def query_arrow(self, sql: str, params: Mapping[str, Any] | None = None, **kw: Any) -> ArrowResult:
        """Runs a SELECT: its answer as Arrow columns, which pyarrow,
        polars and pandas read without copying (`ArrowResult`)."""
        if not hasattr(self._db, "query_arrow"):
            raise StrataError("this build of strata has no Arrow: build it with the feature datafusion (the default)")
        return ArrowResult(self._db.query_arrow(sql, _params(params, kw)))

    def sql(self, sql: str, params: Mapping[str, Any] | None = None, **kw: Any) -> Result | int:
        """Runs any statement: a SELECT's `Result`, or the number of rows an
        INSERT, UPDATE or DELETE wrote."""
        r = self._db.sql(sql, _params(params, kw))
        return r if isinstance(r, int) else Result._of(r)

    def _query_logged(self, sql: str, params: Mapping[str, Any] | None, log: dict) -> Result:
        return Result._of(self._db.query_logged(sql, _params(params, {}), json.dumps(log)))

    def _export_rank_log(self, opts: dict) -> list[dict]:
        return json.loads(self._db.export_rank_log(json.dumps(opts)))

    def plan(self, sql: str, params: Mapping[str, Any] | None = None, **kw: Any) -> dict:
        """The plan of a SELECT, INSERT, UPDATE or DELETE, without running
        it: the plan IR as JSON, which `run` takes."""
        return json.loads(self._db.plan(sql, _params(params, kw)))

    def run(self, plan: Mapping[str, Any] | Query) -> Result | int:
        """Runs a plan (as `plan`, a `Query` or a server's /v1/explain gives
        it): a read's `Result`, or the number of rows a write wrote."""
        if isinstance(plan, Query):
            plan = plan.plan()
        r = self._db.run(json.dumps(plan))
        return r if isinstance(r, int) else Result._of(r)

    def insert(self, rows: Mapping[str, Any] | Iterable[Mapping[str, Any]], *, table: str | None = None) -> int:
        """Inserts a row or rows, whose keys must be new, into the first
        table or `table`; all or none are written. Returns how many. A
        vector may be a numpy array (float32 or float64), read from its
        bytes."""
        return self._db.insert(rows, table)

    def upsert(self, rows: Mapping[str, Any] | Iterable[Mapping[str, Any]], *, table: str | None = None) -> int:
        """Inserts a row or rows, replacing the rows that have their keys,
        in transactions of 10,000 rows. Returns how many."""
        return self._db.upsert(rows, table)

    def get(self, key: Any, *, table: str | None = None) -> dict | None:
        """The row with this primary key in the first table or `table`, read
        without SQL: a dict of the columns `SELECT *` answers, or None when
        no live row has the key (or its `expire_by` time has passed)."""
        return self._db.get_many([key], table)[0]

    def get_many(self, keys: Iterable[Any], *, table: str | None = None) -> list[dict | None]:
        """`get` of several keys at once, in their order."""
        return self._db.get_many(list(keys), table)

    def expire(self) -> int:
        """Deletes the rows whose `expire_by` time has passed, in every
        table with one; returns how many. Reads leave them out already."""
        return self._db.expire()

    def delete(self, keys: Any, *, table: str | None = None) -> int:
        """Deletes the rows with this key or these keys; returns how many
        there were."""
        return self._db.delete(keys, table)

    def write(self, ops: list[dict]) -> int:
        """Write operations as `Client.write` and /v1/write take them
        ({"op": "insert", "doc": {...}}, {"op": "add", "key": k, "field":
        "n", "value": 1}, ...), in one transaction. Returns the rows
        written."""
        return self._db.write(ops)

    def import_file(self, path: str | os.PathLike, format: str | None = None, *, table: str | None = None) -> dict:
        """Loads a JSON Lines, JSON (an array), CSV or TSV file, in `format`
        or the one its extension names, into the first table or `table`.
        Into an empty table the rows are bulk-loaded in one commit;
        otherwise they are upserted. Returns {rows, bulk, skipped_fields}:
        the columns the schema lacks."""
        rows, bulk, skipped = self._db.import_file(os.fspath(path), format, table)
        return {"rows": rows, "bulk": bulk, "skipped_fields": skipped}

    def import_sqlite(self, path: str | os.PathLike, *, tables: str | Iterable[str] | None = None, schemas: Any = None, blobs_base64: bool = False) -> dict:
        """Imports the tables of a SQLite database file (all, or `tables`).
        Each goes into the table of its name: an existing table takes the
        columns whose names match its fields (rows upserted by key, or
        bulk-loaded when it is empty); a missing one is created from the
        SQLite table's declarations and values, or from the schema
        `schemas` gives for it (see `plan_sqlite`). BLOB columns are left
        out unless `blobs_base64`. Returns {tables: [{source, table, rows,
        created, bulk, notes}], skipped: {name: why}}; the notes say what
        did not carry over."""
        self.last_import = json.loads(self._db.import_sqlite(os.fspath(path), _names(tables), _schemas_json(schemas), blobs_base64))
        return self.last_import

    def export_sqlite(self, path: str | os.PathLike, *, tables: str | Iterable[str] | None = None, fts: bool = True, indexes: bool = True, schemas: bool = True) -> dict:
        """Writes the tables (all, or `tables`) to a new SQLite file, from
        one snapshot: rows, keys, references, NOT NULL and DEFAULT, B-tree
        indexes (`indexes`), an FTS5 index over each table's text fields
        kept in step by triggers (`fts`), and the Strata schemas in table
        `_strata_tables` (`schemas`), which an import restores. Vectors are
        float32 blobs (`F32_BLOB(n)`), lists, edges and sparse weights JSON,
        timestamps UTC text. Returns {tables: [{table, rows, indexes, fts,
        notes}]}."""
        return json.loads(self._db.export_sqlite(os.fspath(path), _names(tables), fts, indexes, schemas))

    def import_parquet(self, path: str | os.PathLike, *, table: str | None = None, schema: Any = None, key: str | None = None, blobs_base64: bool = False) -> dict:
        """Imports a Parquet file, or a directory of them as Spark, DuckDB,
        polars and pandas write them (part files, Hive partitions
        `key=value/` read as columns), into one table; `path` may be an
        `s3://` URL (an object, or every object under a prefix). The rows
        go into `table` (by default, the table an export kept in the file,
        else one named for the file): an existing table takes the columns
        whose names match its fields (rows upserted by key, or bulk-loaded
        when it is empty); a missing one is created with `schema` (see
        `plan_parquet`) or from the columns' types and values, keyed by
        `key` (default: a column named id; "" for none). Binary columns
        are left out unless `blobs_base64`. Returns {table, rows, files,
        created, bulk, skipped: {column: why}, notes}."""
        self.last_import = json.loads(self._db.import_parquet(os.fspath(path), table, None if schema is None else _schema_json(schema), key, blobs_base64))
        return self.last_import

    def export_parquet(self, path: str | os.PathLike, *, tables: str | Iterable[str] | None = None, schemas: bool = True) -> dict:
        """Writes the tables (all, or `tables`) to Parquet, from one
        snapshot: to the file `path` when it ends in .parquet (one table),
        else to a new directory of `<table>.parquet` files; `path` may be an
        `s3://` URL. Each file is compressed with Zstandard and keeps the
        table's Strata schema (unless not `schemas`), which `import_parquet`
        restores; any Parquet
        reader (pyarrow, polars, DuckDB, Spark) reads the columns: vectors
        as fixed-size lists of float32, timestamps as UTC microseconds,
        lists as lists, JSON, edges and sparse weights as JSON text.
        Returns {tables: [{table, file, rows, bytes, notes}]}."""
        return json.loads(self._db.export_parquet(os.fspath(path), _names(tables), schemas))

    def import_arrow(self, data: Any, *, table: str | None = None) -> dict:
        """Writes rows from anything that gives an Arrow stream: a pyarrow
        Table, RecordBatch or RecordBatchReader, a polars DataFrame, a
        DuckDB relation, a pandas DataFrame (through pyarrow), into the
        first table or `table`. Each column fills the field of its name;
        the others are left out. Into an empty table the rows are
        bulk-loaded in one commit; otherwise they are upserted by key.
        Returns {table, rows, bulk, skipped, notes}."""
        if not hasattr(data, "__arrow_c_stream__") and type(data).__module__.startswith("pandas"):
            import pyarrow

            data = pyarrow.Table.from_pandas(data, preserve_index=False)
        return json.loads(self._db.import_arrow(data, table))

    def embed(self, text: str | None = None, field: str | None = None, *, table: str | None = None, image: bytes | None = None, audio: bytes | None = None) -> list[float]:
        """The query vector of `text` by an embedded field's model (of the
        first table, or of `table`), or of `image` or `audio`, the bytes of
        an image or sound file, by a model of that."""
        return self._db.embed(text, field, table, None if image is None else bytes(image), None if audio is None else bytes(audio))

    def embed_tokens(self, text: str, field: str, *, table: str | None = None) -> list[list[float]]:
        """The vectors of `text`'s tokens as a query, by the model of
        multi-vector field `field` (a ColBERT model, `VECTOR(d)[]` embedded
        from text), as `maxsim` takes them."""
        return self._db.embed_tokens(text, field, table)

    def sid_next(self, prefixes: Iterable[str], *, field: str, where: str | None = None, params: Mapping[str, Any] | None = None, table: str | None = None, **kw: Any) -> dict[str, dict[str, int]]:
        """The codes that can follow each prefix of semantic-ID field
        `field` (``""`` for the first number) among the rows SQL condition
        `where` matches, or all rows: {prefix: {code: rows}}, the codes of
        each prefix in the order of their last numbers. At most 64
        prefixes."""
        prefixes = builtins.list(prefixes)
        got = self._db.sid_next(field, prefixes, where, _params(params, kw), table)
        return {p: dict(cs) for p, cs in zip(prefixes, got)}

    def optimize(self) -> None:
        """Readies a table that is done changing for queries: writes the
        rows still in memory into segments, merges its segments into as few
        as it can (one, up to 2,000,000 rows; a query visits each), drops
        deleted rows, and returns the space this frees to the file system."""
        self._db.optimize()

    def vacuum(self) -> int:
        """Returns the space merges freed to the file system (they reuse it
        otherwise); the number of bytes the file shrank by."""
        return self._db.vacuum()

    def close(self) -> None:
        """Merges small segments, gives the file system back the space that
        merges freed when it is an eighth of the file or more, and closes
        the database. (Segments are merged in the background as writes add
        them, so a table written to for a long time stays in few segments.)"""
        self._db.close()

    @property
    def closed(self) -> bool:
        return self._db.closed()

    def __enter__(self) -> "Database":
        return self

    def __exit__(self, *exc: Any) -> None:
        self.close()

    def __repr__(self) -> str:
        if isinstance(self._db, _Pending):
            return f"<strata.Database {self._db.path} (no tables yet)>"
        if self.closed:
            return "<strata.Database (closed)>"
        return f"<strata.Database {self.schema['table']}: {len(self)} rows>"

    # What frames need of a store (FrameStore).

    def _frame_arrow(self, sql: str, params: Mapping[str, Any], types: Mapping[str, dict]) -> ArrowResult:
        return self.query_arrow(sql, params)

    def _explain(self, sql: str, params: Mapping[str, Any]) -> list[str]:
        return [str(row[0]) for row in self.sql("EXPLAIN " + sql, params).rows]

    def _plan_parquet_file(self, path: str, table: str, key: str | None) -> dict:
        return plan_parquet(path, table=table, key=key, db=None if isinstance(self._db, _Pending) else self)

    def _create(self, schema: Mapping[str, Any]) -> None:
        if isinstance(self._db, _Pending):
            p = self._db
            self._db = _strata.Database.create(p.path, json.dumps(schema), p.directory, None if p.models is None else os.fspath(p.models), p.threads)
        else:
            self._db.sql(_strata.create_sql(json.dumps(schema)), None)

    def _load(self, table: str, data: Any) -> dict:
        if not hasattr(data, "__arrow_c_stream__") and not type(data).__module__.startswith("pandas"):
            data = _arrow_table(data)
        return self.import_arrow(data, table=table)


def open(path: str | os.PathLike, schema: Any = None, *, directory: bool = False, models: str | os.PathLike | None = None, threads: int | None = None, read_only: bool = False) -> Database:
    """Opens the database at `path`, creating it with `schema` if it does
    not exist. `threads`: how many cores vector indexes build on and each
    query searches segments on (default: every core). `read_only`: opens
    it to read while another process writes it (see `Database.open`)."""
    if os.path.exists(path) or read_only:
        return Database.open(path, models=models, threads=threads, read_only=read_only)
    if schema is None:
        raise StrataError(f"{os.fspath(path)} does not exist: pass a schema to create it")
    return Database.create(path, schema, directory=directory, models=models, threads=threads)


def from_sqlite(sqlite_path: str | os.PathLike, path: str | os.PathLike, *, tables: str | Iterable[str] | None = None, schemas: Any = None, blobs_base64: bool = False, directory: bool = False, models: str | os.PathLike | None = None, threads: int | None = None) -> Database:
    """Creates a database at `path` from the tables of a SQLite database
    file (all, or `tables`), with schemas made from the SQLite tables or
    given in `schemas` (see `plan_sqlite`). What the import did, as
    `Database.import_sqlite` returns it, is the database's `last_import`."""
    inner, report = _strata.Database.from_sqlite(os.fspath(sqlite_path), os.fspath(path), _names(tables), _schemas_json(schemas), blobs_base64, directory, None if models is None else os.fspath(models), threads)
    db = Database(inner)
    db.last_import = json.loads(report)
    return db


def plan_sqlite(sqlite_path: str | os.PathLike, *, tables: str | Iterable[str] | None = None, schemas: Any = None, blobs_base64: bool = False) -> dict:
    """What `from_sqlite` would make of a SQLite file, without writing:
    {tables: [...], skipped: {name: why}}, each table the schema of the
    Strata table it becomes (with its `source`, `columns` and `notes`). Edit
    the plan and pass it to `from_sqlite` as `schemas`."""
    return json.loads(_strata.Database.plan_sqlite(os.fspath(sqlite_path), _names(tables), _schemas_json(schemas), blobs_base64))


def from_parquet(parquet_path: str | os.PathLike, path: str | os.PathLike, *, table: str | None = None, schema: Any = None, key: str | None = None, blobs_base64: bool = False, directory: bool = False, models: str | os.PathLike | None = None) -> Database:
    """Creates a database at `path` from a Parquet file or directory (or an
    `s3://` URL), its one table made from the columns, kept by an export,
    or given as `schema` (see `plan_parquet`). What the import did, as
    `Database.import_parquet` returns it, is the database's
    `last_import`."""
    inner, report = _strata.Database.from_parquet(os.fspath(parquet_path), os.fspath(path), table, None if schema is None else _schema_json(schema), key, blobs_base64, directory, None if models is None else os.fspath(models))
    db = Database(inner)
    db.last_import = json.loads(report)
    return db


def plan_parquet(parquet_path: str | os.PathLike, *, table: str | None = None, schema: Any = None, key: str | None = None, blobs_base64: bool = False, db: "Database | None" = None) -> dict:
    """What `from_parquet` (or, with `db`, `db.import_parquet`) would make
    of a Parquet file or directory, without writing: the table's schema
    with whether it `exists`, the `files` and `rows`, the `columns` read
    and the fields they fill, the Hive `partitions`, what is `skipped` and
    `notes`. Edit the plan and pass it back as `schema`."""
    return json.loads(_strata.Database.plan_parquet(os.fspath(parquet_path), table, None if schema is None else _schema_json(schema), key, blobs_base64, None if db is None else db._db))


def connect(url: str | os.PathLike = "http://127.0.0.1:7700", *, token: str | None = None, timeout: float = 30.0, retry_for: float = 10.0, models: str | os.PathLike | None = None, threads: int | None = None, directory: bool = False) -> Client | Database:
    """A store, for frames and queries alike: a `Client` of the
    strata-server (or coordinator) at an `http://` or `https://` URL, or
    the `Database` at a path, in this process. A path that does not exist
    yet makes a database at its first `create_table`. The code after it is
    the same either way: `connect(...)["orders"]` is a frame of a table."""
    target = os.fspath(url)
    if target.startswith(("http://", "https://")):
        return Client(target, token=token, timeout=timeout, retry_for=retry_for)
    if "://" in target:
        raise StrataError(f"connect takes a path or an http(s):// URL, not {target!r}")
    if target.startswith("file:"):
        target = target[len("file:"):]
    if os.path.exists(target):
        return Database.open(target, models=models, threads=threads)
    return Database(_Pending(target, directory, models, threads))

