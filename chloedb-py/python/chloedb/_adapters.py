"""What the framework adapters share (`chloedb.langchain`,
`chloedb.langgraph`, `chloedb.llama_index`, `chloedb.dbapi`): opening a
store from a path, a URL, a `Database` or a `Client`; making the tables
they keep; and filters on a `JSON` field, written as MongoDB writes them
(`{"year": {"$gte": 2000}, "$or": [...]}`), as SQL with parameters."""

from __future__ import annotations

import json
import math
import os
import threading
import weakref
from typing import Any, Iterable, Iterator, Mapping, Sequence

from ._native import ChloeDBError
from ._search import quote

# The limits a JSON field's index puts on an object (docs: JSON fields).
_JSON_DEPTH = 16
_JSON_VALUES = 1000
_JSON_PATH_BYTES = 256

# The databases adapters opened by path, while something holds them.
_opened: "weakref.WeakValueDictionary[str, Any]" = weakref.WeakValueDictionary()
_opened_lock = threading.Lock()

# Rows a write sends at once.
BATCH = 1000


def open_store(db: Any, *, models: str | os.PathLike | None = None, token: str | None = None) -> Any:
    """A `Database` or `Client` from what an adapter was given: one of
    those (used as it is), an `http(s)://` URL (a `Client` of that
    server), or a path (the database there, made when its first table
    is)."""
    if isinstance(db, (str, os.PathLike)):
        from . import connect

        target = os.fspath(db)
        if target.startswith(("http://", "https://")):
            return connect(target, token=token)
        # One process opens a database once to write it: adapters given the
        # same path (a checkpointer and a store, say) share one Database.
        key = os.path.realpath(target[len("file:"):] if target.startswith("file:") else target)
        with _opened_lock:
            store = _opened.get(key)
            if store is None or store.closed:
                store = connect(target, models=models)
                _opened[key] = store
            return store
    if not (hasattr(db, "query") and hasattr(db, "tables") and hasattr(db, "upsert")):
        raise TypeError(f"expected a chloedb.Database, a chloedb.Client, a path or an http(s):// URL, not {type(db).__name__}")
    return db


def _pending(store: Any) -> bool:
    from . import _Pending

    return isinstance(getattr(store, "_db", None), _Pending)


def table_schema(store: Any, name: str) -> dict | None:
    """The short schema of table `name`, or None when there is none."""
    if _pending(store):
        return None
    for t in store.tables():
        if t["table"] == name:
            return t
    return None


def ensure_table(store: Any, schema: Mapping[str, Any]) -> dict:
    """Table `schema["table"]`, made from `schema` when it is missing;
    returns the schema the table has (which may be an older one)."""
    found = table_schema(store, schema["table"])
    if found is not None:
        return found
    try:
        store._create(dict(schema))
    except ChloeDBError:
        # Another process may have made it meanwhile.
        found = table_schema(store, schema["table"])
        if found is None:
            raise
        return found
    if hasattr(store, "_schema"):
        store._schema = None
    return table_schema(store, schema["table"]) or dict(schema)


def field(schema: Mapping[str, Any], name: str) -> dict | None:
    for f in schema.get("fields", []):
        if f["name"] == name:
            return f
    return None


def chunks(items: Sequence[Any], n: int = BATCH) -> Iterator[Sequence[Any]]:
    for i in range(0, len(items), n):
        yield items[i : i + n]


def rows_of(store: Any, sql: str, params: Mapping[str, Any] | None = None) -> Iterator[dict]:
    """Every row a query answers, past a server's result window: read a
    page at a time."""
    for page in store.scroll(sql, dict(params or {}), page_size=5000):
        yield from page


def unit(v: Sequence[float]) -> list[float]:
    """`v` scaled to length 1 (so that ChloeDB's dot product is the
    cosine); a zero vector as it is."""
    v = [float(x) for x in v]
    n = math.sqrt(sum(x * x for x in v))
    return v if n == 0 else [x / n for x in v]


def json_value(v: Any) -> Any:
    """A value as JSON holds it: what `json` cannot write becomes its
    text, as `json.dumps(default=str)` would."""
    return json.loads(json.dumps(v, default=str))


def json_fits(obj: Any) -> bool:
    """Whether a JSON field takes `obj` (an object within the index's
    limits: depth, values and path length; no empty key)."""
    if not isinstance(obj, dict):
        return False
    count = 0

    def walk(v: Any, depth: int, path_bytes: int) -> bool:
        nonlocal count
        count += 1
        if count > _JSON_VALUES or depth > _JSON_DEPTH or path_bytes > _JSON_PATH_BYTES:
            return False
        if isinstance(v, dict):
            for k, x in v.items():
                if not isinstance(k, str) or k == "":
                    return False
                if not walk(x, depth + 1, path_bytes + len(k.encode()) + 1):
                    return False
        elif isinstance(v, list):
            for x in v:
                if not walk(x, depth + (1 if isinstance(x, (dict, list)) else 0), path_bytes):
                    return False
        return True

    return walk(obj, 1, 0)


def literal(s: str) -> str:
    """Text as an SQL string literal."""
    return "'" + s.replace("'", "''") + "'"


def path(fld: str, keys: Sequence[str]) -> str:
    """The SQL of the value at `keys` in JSON field `fld`: each key is one
    level, whatever it holds (a dot included)."""
    return quote(fld) + "".join(f"[{literal(k)}]" for k in keys)


class Params:
    """Query parameters named in order (`:p0`, `:p1`, ...)."""

    def __init__(self, prefix: str = "p"):
        self.values: dict[str, Any] = {}
        self._prefix = prefix

    def __call__(self, value: Any) -> str:
        name = f"{self._prefix}{len(self.values)}"
        self.values[name] = value
        return ":" + name


_COMPARE = {"$eq": "=", "$ne": "!=", "$gt": ">", "$gte": ">=", "$lt": "<", "$lte": "<="}


def _scalar(p: Params, v: Any) -> str:
    """A scalar of a comparison: booleans as SQL's TRUE and FALSE (a
    parameter would bind them as numbers), the rest as parameters."""
    if v is True:
        return "TRUE"
    if v is False:
        return "FALSE"
    if isinstance(v, float) and not math.isfinite(v):
        raise ValueError(f"a filter compares with finite numbers, not {v}")
    if not isinstance(v, (str, int, float)):
        raise ValueError(f"a filter compares with text, numbers and booleans, not {type(v).__name__}")
    return p(v)


def _equal(p: Params, col: str, v: Any, keys: Sequence[str], fld: str) -> str:
    if v is None:
        return f"{col} IS NULL"
    if isinstance(v, dict):
        # A nested object: each of its keys at its own path.
        if not v:
            return f"{col} IS NOT NULL"
        return " AND ".join(_condition(p, fld, [*keys, k], x) for k, x in v.items())
    if isinstance(v, (list, tuple)):
        # A list holds its values at its own path: it matches rows whose
        # list holds each of them.
        if not v:
            return f"{col} IS NULL"
        return "(" + " AND ".join(f"{col} = {_scalar(p, x)}" for x in v) + ")"
    return f"{col} = {_scalar(p, v)}"


def _operator(p: Params, fld: str, keys: Sequence[str], op: str, v: Any) -> str:
    col = path(fld, keys)
    if op == "$eq":
        return _equal(p, col, v, keys, fld)
    if op == "$ne":
        if v is None:
            return f"{col} IS NOT NULL"
        # As MongoDB's: a row without the value is not equal to it.
        return f"({col} != {_scalar(p, v)} OR {col} IS NULL)"
    if op in _COMPARE:
        if isinstance(v, bool) or not isinstance(v, (int, float, str)):
            raise ValueError(f"{op} compares with a number or text, not {v!r}")
        return f"{col} {_COMPARE[op]} {_scalar(p, v)}"
    if op in ("$in", "$nin"):
        if not isinstance(v, (list, tuple, set, frozenset)):
            raise ValueError(f"{op} takes a list, not {v!r}")
        values = list(v)
        if not values:
            return "FALSE" if op == "$in" else "TRUE"
        listed = ", ".join(_scalar(p, x) for x in values)
        if op == "$in":
            return f"{col} IN ({listed})"
        return f"({col} NOT IN ({listed}) OR {col} IS NULL)"
    if op == "$exists":
        return f"{col} IS NOT NULL" if v else f"{col} IS NULL"
    if op == "$between":
        lo, hi = v
        return f"{col} BETWEEN {_scalar(p, lo)} AND {_scalar(p, hi)}"
    if op == "$like":
        if not isinstance(v, str) or not v.endswith("%") or "%" in v[:-1] or "_" in v:
            raise ValueError("$like takes a prefix: text that ends with %, with no other % or _")
        return f"PREFIX({col}, {p(v[:-1])})"
    if op == "$contains":
        return f"{col} = {_scalar(p, v)}"
    raise ValueError(f"unknown filter operator {op!r}: use $eq, $ne, $gt, $gte, $lt, $lte, $in, $nin, $exists, $between, $like, $contains, $and, $or, $not")


def _condition(p: Params, fld: str, keys: Sequence[str], v: Any) -> str:
    if isinstance(v, dict) and v and all(isinstance(k, str) and k.startswith("$") for k in v):
        return " AND ".join(_operator(p, fld, keys, op, x) for op, x in v.items())
    return _equal(p, path(fld, keys), v, keys, fld)


def where(filter: Mapping[str, Any] | None, fld: str, p: Params) -> str | None:
    """The SQL condition of a MongoDB-style filter on the objects of JSON
    field `fld`, its values bound in `p`; None for no filter.

    `{"k": v}` is equality (a nested object is a condition on each of its
    keys, a list holds each of its values, None is no value); `{"k":
    {"$op": v}}` an operator, `$and`, `$or` (lists of filters) and `$not`
    (a filter) combine them."""
    if not filter:
        return None
    parts = []
    for k, v in filter.items():
        if k == "$and" or k == "$or":
            subs = [s for s in (where(x, fld, p) for x in v) if s]
            if not subs:
                parts.append("TRUE" if k == "$and" else "FALSE")
                continue
            parts.append("(" + (" AND " if k == "$and" else " OR ").join(f"({s})" for s in subs) + ")")
        elif k == "$not":
            s = where(v, fld, p)
            parts.append(f"NOT ({s})" if s else "FALSE")
        elif isinstance(k, str) and k.startswith("$"):
            raise ValueError(f"unknown filter operator {k!r} at the top of a filter: use $and, $or or $not")
        else:
            parts.append(_condition(p, fld, [k], v))
    return " AND ".join(f"({x})" for x in parts)


def conditions(*conds: str | None) -> str:
    kept = [c for c in conds if c]
    return ("WHERE " + " AND ".join(f"({c})" for c in kept)) if kept else ""


def first_row(rows: Iterable[dict]) -> dict | None:
    for r in rows:
        return r
    return None
