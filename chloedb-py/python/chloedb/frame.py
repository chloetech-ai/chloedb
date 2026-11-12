"""Frames: a lazy dataframe over ChloeDB tables, written as Polars writes
one, run where the data is.

A `Frame` is a query: nothing is read until it is collected
(`to_arrow()`, `to_pandas()`, `to_polars()`, `to_numpy()`, `count()`,
iterating its batches, or showing it in a notebook). Then the whole chain
compiles to one SQL statement with its values as parameters, which runs on
a file in this process (`Database`) or on a chloedb-server or a coordinator
(`Client`), and the answer comes back as Arrow::

    import chloedb as st

    shop = st.connect("shop.chloedb")         # or st.connect("https://...")
    orders = shop["orders"]
    top = (orders
           .filter(st.col("status") == "paid")
           .group_by("country")
           .agg(revenue=st.col("amount").sum(), n=st.len())
           .sort("revenue", descending=True)
           .head(10))
    print(top.sql())                          # the SQL it runs
    top.to_pandas()

Searches are verbs of the frame too: `search` (BM25), `near` (vector
similarity, a text embedded by the field's model), `rank_by` (any ranking
expression), `rerank` and `max_per`; each adds a `score` column.

Rows have no order unless a frame sorts or ranks them, as in SQL: `head`
of an unsorted frame is any rows. Writes are explicit (`upsert`, `update`,
`delete`, `write_to`, `create_table`); a frame never changes as a side
effect.
"""

from __future__ import annotations

import datetime as _dt
import math
import os
import tempfile
from typing import Any, Iterable, Iterator, Mapping, Sequence

from ._native import ChloeDBError
from .build import (
    Expr,
    _Agg,
    _Alias,
    _Col,
    _Fn,
    _Named,
    _Pct,
    _SqlCall,
    _Template,
    _Window,
    _colish,
    _expr,
    bm25,
    col,
    similarity,
)

__all__ = ["Frame", "GroupBy", "FrameStore", "concat", "len_"]

# Rows a search, near or rank_by keeps unless asked for another number: a
# query ranked by a score always sends its limit, since ChloeDB answers one
# without a LIMIT with 100 rows.
DEFAULT_K = 100

# Row-group size of the Parquet a Client uploads, and how many rows of an
# Arrow source a new table's schema is inferred from (the Parquet import's
# own sample).
_UPLOAD_ROWS = 250_000
_SAMPLE_ROWS = 100_000


def len_() -> Expr:
    """The number of rows (`COUNT(*)`), as Polars' `pl.len()`; a frame
    names the column `len`. `chloedb.len` is this function."""
    return _Agg("count", None)


def _q(name: str) -> str:
    return '"' + name.replace('"', '""') + '"'


def _atomic(sql: str) -> bool:
    """SQL that needs no parentheses inside a larger expression: a name, a
    number, a parameter or a call, or one already in parentheses."""
    if not sql:
        return True
    if sql[0] == '"' and sql.count('"') == 2 and sql.endswith('"'):
        return True
    if sql.replace(".", "").replace('"', "").replace("_", "").isalnum() and " " not in sql:
        return True
    if sql.startswith(":") and sql[1:].isalnum():
        return True
    start = sql.find("(")
    if start >= 0 and sql.endswith(")") and (start == 0 or sql[:start].replace("_", "").isalnum()):
        depth = 0
        for i, ch in enumerate(sql[start:], start):
            depth += ch == "("
            depth -= ch == ")"
            if depth == 0:
                return i == len(sql) - 1
    return False


def _param(v: Any) -> Any:
    """A value as a query parameter: times as ISO 8601 text (UTC when
    naive), numpy values and arrays as Python ones."""
    if isinstance(v, _dt.datetime):
        return v.isoformat()
    if isinstance(v, _dt.date):
        return v.isoformat()
    if hasattr(v, "tolist") and type(v).__module__.startswith("numpy"):
        return v.tolist()
    if isinstance(v, (list, tuple)):
        return [_param(x) for x in v]
    return v


# Columns of a block: what a name in an expression means there.


class _Src:
    """A column of the block's source: a table's field or a WITH's column."""

    __slots__ = ("block", "name")

    def __init__(self, block: "_Block", name: str):
        self.block, self.name = block, name

    def sql(self, r: "_Render") -> str:
        return r.qualify(self.block.qual, self.name)


class _Def:
    """A column computed by an expression over the columns of a scope."""

    __slots__ = ("expr", "scope")

    def __init__(self, expr: Expr, scope: dict):
        self.expr, self.scope = expr, scope

    def sql(self, r: "_Render") -> str:
        text = r.render(self.expr, self.scope)
        return text if _atomic(text) else f"({text})"


class _Raw:
    """SQL as it is: `score()`, or an output's alias over groups."""

    __slots__ = ("text",)

    def __init__(self, text: str):
        self.text = text

    def sql(self, r: "_Render") -> str:
        return self.text


class _Render:
    """Writes expressions as SQL: resolves column names in a scope, and
    binds values as parameters (`:p1`, `:p2`, ...) so that nothing is
    quoted or escaped by hand."""

    def __init__(self) -> None:
        self.params: dict[str, Any] = {}
        self._scope: dict | None = None

    def render(self, e: Expr, scope: dict) -> str:
        prev, self._scope = self._scope, scope
        try:
            return e._sql(self)
        finally:
            self._scope = prev

    def col(self, name: str) -> str:
        scope = self._scope or {}
        entry = scope.get(name)
        if entry is None:
            near = [k for k in scope if k.lower() == name.lower()]
            if len(near) != 1:
                raise ChloeDBError(f"the frame has no column {name!r}: it has {', '.join(scope) or 'none'}")
            entry = scope[near[0]]
        return entry.sql(self)

    @staticmethod
    def qualify(qual: str | None, name: str) -> str:
        return _q(name) if qual is None else f"{_q(qual)}.{_q(name)}"

    def param(self, v: Any) -> str:
        name = f"p{len(self.params) + 1}"
        self.params[name] = _param(v)
        return ":" + name

    def value(self, v: Any) -> str:
        """A constant: numbers, booleans and NULL written in place, other
        values bound as parameters."""
        if hasattr(v, "item") and type(v).__module__.startswith("numpy") and getattr(v, "ndim", 0) == 0:
            v = v.item()
        if v is None:
            return "NULL"
        if isinstance(v, bool):
            return "TRUE" if v else "FALSE"
        if isinstance(v, int):
            return str(v)
        if isinstance(v, float):
            if not math.isfinite(v):
                raise ChloeDBError(f"ChloeDB's numbers are finite, not {v!r}")
            return repr(v)
        return self.param(v)

    @staticmethod
    def text(t: str) -> str:
        """A string constant written in place (a model's name)."""
        return "'" + str(t).replace("'", "''") + "'"

    def query(self, q: Any) -> str:
        """A similarity's query: a vector, or a text the field's model
        embeds where the query runs."""
        if isinstance(q, str):
            return self.param({"embed": q})
        if isinstance(q, Mapping):
            return self.param(dict(q))
        return self.param([float(x) for x in q])


# Blocks: one SELECT, built up as frame methods fold into it.


class _Block:
    def __init__(self, kind: str, source: str | None, columns: Sequence[str], *, table: str | None = None, types: Mapping[str, dict] | None = None, extra: Sequence[str] = ()):
        self.kind = kind  # "table", "derived" (a WITH) or "join"
        self.source = source  # what FROM names
        self.table = table
        self.qual: str | None = None
        self.base = {n: _Src(self, n) for n in [*columns, *extra]}
        self.out: dict[str, Any] = {n: self.base[n] for n in columns}
        self.columns = list(columns)
        self.types = dict(types or {})
        self.where: list[tuple[Expr, dict]] = []
        self.group: list[tuple[Expr, dict]] | None = None
        self.order: list[tuple[Expr, dict, bool, bool]] = []
        self.limit: int | None = None
        self.offset: int | None = None
        self.distinct = False
        self.distinct_on: list[tuple[Expr, dict]] | None = None
        self.max_per: tuple[int, Expr, dict] | None = None
        self.ranked = False
        self.windows = False
        self.join: tuple | None = None
        self.renamed_score: str | None = None

    @property
    def scope(self) -> dict:
        """What names in a condition, key or column mean: the outputs, and
        on one table its other fields too (for search functions and
        filters on a column not selected)."""
        if self.group is not None:
            return {n: _Raw(_q(n)) for n in self.columns}
        if self.kind == "table":
            return {**self.base, **self.out}
        return dict(self.out)

    def plain(self) -> bool:
        """Only conditions and columns: a condition, a projection, a group
        or a ranking may still fold in."""
        return (self.group is None and self.limit is None and self.offset is None and not self.distinct
                and self.distinct_on is None and not self.windows and not self.ranked and self.max_per is None)


class _Compiler:
    def __init__(self) -> None:
        self.r = _Render()
        self.ctes: list[tuple[str, str]] = []
        self.quals = 0

    def qual(self) -> str:
        self.quals += 1
        return f"t{self.quals}"

    def wrap(self, b: _Block, keep: set | None = None) -> _Block:
        """`b` as a WITH, read by a new block: when a step cannot fold into
        `b`'s SELECT. `keep` narrows it to the columns read after it."""
        if keep is not None and b.group is None and not b.distinct and b.distinct_on is None:
            cols = [n for n in b.columns if n in keep] or b.columns[:1]
            b.columns = cols
        name = f"q{len(self.ctes) + 1}"
        self.ctes.append((name, self.render(b)))
        nb = _Block("derived", _q(name), b.columns, types={n: t for n, t in b.types.items() if n in b.columns})
        return nb

    def render(self, b: _Block) -> str:
        r = self.r
        items = []
        for n in b.columns:
            text = b.out[n].sql(r)
            items.append(text if text == _q(n) else f"{text} AS {_q(n)}")
        head = "SELECT "
        if b.distinct:
            head += "DISTINCT "
        if b.distinct_on is not None:
            head += "DISTINCT ON (" + ", ".join(r.render(e, sc) for e, sc in b.distinct_on) + ") "
        sql = head + ", ".join(items) + " FROM " + self._from(b)
        if b.where:
            sql += " WHERE " + " AND ".join(r.render(e, sc) for e, sc in b.where)
        if b.group:
            sql += " GROUP BY " + ", ".join(r.render(e, sc) for e, sc in b.group)
        if b.order:
            keys = []
            for e, sc, desc, nulls_first in b.order:
                keys.append(r.render(e, sc) + (" DESC" if desc else "") + (" NULLS FIRST" if nulls_first else ""))
            sql += " ORDER BY " + ", ".join(keys)
            if b.max_per is not None:
                k, e, sc = b.max_per
                sql += f" MAX {k} PER {r.render(e, sc)}"
        if b.limit is not None:
            sql += f" LIMIT {b.limit}"
        if b.offset:
            sql += f" OFFSET {b.offset}"
        return sql

    def _from(self, b: _Block) -> str:
        if b.join is None:
            return b.source or ""
        how, lb, rb, keys, on_extra = b.join
        r = self.r
        cond = [f"{r.render(le, ls)} = {r.render(re_, rs)}" for le, ls, re_, rs in keys]
        cond += [r.render(e, sc) for e, sc in on_extra]
        word = "LEFT JOIN" if how == "left" else "JOIN"
        return f"{lb.source} AS {_q(lb.qual)} {word} {rb.source} AS {_q(rb.qual)} ON {' AND '.join(cond)}"

    def sql(self, b: _Block) -> str:
        body = self.render(b)
        if not self.ctes:
            return body
        return "WITH " + ", ".join(f"{_q(n)} AS ({s})" for n, s in self.ctes) + " " + body


# Expressions: what they name and hold.


def _walk(e: Any) -> Iterator[Expr]:
    yield e
    for k in e._kids():
        yield from _walk(k)


def _has_agg(e: Expr) -> bool:
    if isinstance(e, (_Agg, _Pct)):
        return True
    if isinstance(e, _Window):
        return False
    return any(_has_agg(k) for k in e._kids())


def _has_window(e: Expr) -> bool:
    return any(isinstance(x, _Window) for x in _walk(e))


def _names(e: Expr) -> set:
    """The columns an expression reads, by name."""
    out = set()
    for x in _walk(e):
        if isinstance(x, _Col):
            out.add(x.name)
        elif isinstance(x, _Fn):
            out.update(x.fields)
    return out


def _bare(e: Expr) -> Expr:
    while isinstance(e, _Alias):
        e = e.e
    return e


def _frame_name(e: Expr) -> str:
    """The name of an expression's column, as Polars gives it: the alias,
    else the first column it reads (`col("amount").sum()` is `amount`)."""
    if isinstance(e, _Alias):
        return e.name
    if isinstance(e, _Col):
        return e.name
    if isinstance(e, _Agg) and e.arg is None:
        return "len"
    if isinstance(e, _Fn) and e.fname:
        return e.fname
    if isinstance(e, _Named):
        return e.what
    for k in e._kids():
        try:
            return _frame_name(k)
        except ChloeDBError:
            pass
    raise ChloeDBError(f"give {e!r} a name: .alias(name), or name=... as a keyword argument")


def _items(exprs: Sequence[Any], named: Mapping[str, Any]) -> list[tuple[str, Expr]]:
    """`select`-style arguments as (name, expression): column names,
    expressions, lists of them, and name=expression."""
    out: list[tuple[str, Expr]] = []
    for x in exprs:
        if isinstance(x, (list, tuple)):
            out += _items(x, {})
        elif isinstance(x, str):
            out.append((x, _Col(x)))
        else:
            e = _expr(x)
            out.append((_frame_name(e), _bare(e)))
    for name, x in named.items():
        out.append((name, _bare(_colish(x))))
    seen = set()
    for name, _ in out:
        if name in seen:
            raise ChloeDBError(f"two columns are named {name!r}: rename one with .alias()")
        seen.add(name)
    return out


def _entry(e: Expr, scope: dict) -> Any:
    """The output entry of an expression: a column as it is, or computed."""
    if isinstance(e, _Col):
        entry = scope.get(e.name)
        if entry is None:
            near = [k for k in scope if k.lower() == e.name.lower()]
            if len(near) == 1:
                entry = scope[near[0]]
        if entry is None:
            raise ChloeDBError(f"the frame has no column {e.name!r}: it has {', '.join(scope) or 'none'}")
        return entry
    return _Def(e, scope)


def _type_of(e: Expr, b: _Block) -> dict | None:
    """The field type of an expression's values, where it is known: a
    column's, a time cut to a unit, a count, an extreme of a column."""
    e = _bare(e)
    if isinstance(e, _Col):
        entry = b.scope.get(e.name)
        if isinstance(entry, _Src) and entry.block is b:
            return b.types.get(entry.name)
        for n, x in b.out.items():
            if x is entry:
                return b.types.get(n)
        return None
    if isinstance(e, _Template) and e.template.startswith("date_trunc("):
        return {"type": "timestamp"}
    if isinstance(e, _Pct):
        return {"type": "float"}
    if isinstance(e, _SqlCall) and e.func == "hash":
        return {"type": "int"}
    if isinstance(e, _Agg):
        if e.func in ("count", "count_distinct"):
            return {"type": "int"}
        inner = _type_of(e.arg, b) if e.arg is not None else None
        if inner is None or inner.get("multi"):
            return None
        if e.func in ("min", "max"):
            return {"type": inner["type"]}
        if e.func in ("avg", "median"):
            return {"type": "float"}
        if e.func == "sum" and inner["type"] in ("int", "float"):
            return {"type": inner["type"]}
    return None


_GROUPABLE = ("keyword", "int", "float", "bool", "timestamp")


def _plain_column(e: Expr, b: _Block, types: tuple = _GROUPABLE) -> bool:
    if not isinstance(e, _Col):
        return False
    entry = b.scope.get(e.name)
    if not isinstance(entry, _Src):
        return False
    f = b.types.get(entry.name)
    return f is not None and f.get("type") in types and f.get("indexed", True) is not False


# Steps of a frame: each compiles itself onto its input's block.


class _Node:
    def compile(self, c: _Compiler) -> _Block:
        raise NotImplementedError


class _Table(_Node):
    def __init__(self, name: str, schema: dict):
        self.name, self.schema = name, schema

    def compile(self, c: _Compiler) -> _Block:
        fields = self.schema["fields"]
        # A generated TEXT field has no value to read, but it can be searched.
        cols = [f["name"] for f in fields if not (f["type"] == "text" and f.get("from"))]
        extra = [f["name"] for f in fields if f["name"] not in cols]
        return _Block("table", _q(self.name), cols, table=self.name, types={f["name"]: f for f in fields}, extra=extra)


class _Filter(_Node):
    def __init__(self, input: _Node, cond: Expr):
        self.input, self.cond = input, cond

    def compile(self, c: _Compiler) -> _Block:
        if _has_agg(self.cond) or _has_window(self.cond):
            raise ChloeDBError("filter takes conditions on rows: filter on an aggregate after group_by().agg(), or on a window column after with_columns()")
        b = self.input.compile(c)
        if not b.plain():
            b = c.wrap(b)
        b.where.append((self.cond, b.scope))
        return b


class _Project(_Node):
    def __init__(self, input: _Node, items: list[tuple[str, Expr]], keep: bool, drop: Sequence[str] = (), rename: Mapping[str, str] | None = None):
        self.input, self.items, self.keep, self.drop, self.rename = input, items, keep, list(drop), dict(rename or {})

    def compile(self, c: _Compiler) -> _Block:
        b = self.input.compile(c)
        aggs = any(_has_agg(e) for _, e in self.items)
        wins = any(_has_window(e) for _, e in self.items)
        reshape = bool(self.items) or bool(self.drop)
        if b.group is not None or b.distinct or b.distinct_on is not None or b.windows or ((aggs or wins) and not b.plain()):
            if reshape or self.rename:
                b = c.wrap(b)
        if aggs:
            if self.keep:
                raise ChloeDBError("with_columns of an aggregate: use .over(...) for a window, or group_by(...).agg(...)")
            loose = [n for n, e in self.items if not _has_agg(e) and _names(e)]
            if loose:
                raise ChloeDBError(f"select mixes aggregates with columns ({', '.join(loose)}): group by them with group_by(...).agg(...)")
            return _Group([], self.items).fold(c, b)
        for old, new in self.rename.items():
            if old not in b.out:
                raise ChloeDBError(f"the frame has no column {old!r} to rename")
        for name in self.drop:
            if name not in b.out:
                raise ChloeDBError(f"the frame has no column {name!r} to drop")
        scope = b.scope
        if self.keep or self.drop or self.rename:
            out = {}
            for n in b.columns:
                if n in self.drop:
                    continue
                out[self.rename.get(n, n)] = b.out[n]
            types = {self.rename.get(n, n): t for n, t in b.types.items() if n in b.out and n not in self.drop}
        else:
            out, types = {}, {}
        for name, e in self.items:
            out[name] = _entry(e, scope)
            t = _type_of(e, b)
            if t is not None:
                types[name] = t
            else:
                types.pop(name, None)
        # Columns of the source keep their types; computed ones have none here.
        b.types = {**{k: v for k, v in b.types.items() if k not in out or isinstance(out.get(k), _Src)}, **types}
        b.out = out
        b.columns = list(out)
        if wins:
            b.windows = True
        return b


class _Sort(_Node):
    def __init__(self, input: _Node, keys: list[tuple[Expr, bool, bool]]):
        self.input, self.keys = input, keys

    def compile(self, c: _Compiler) -> _Block:
        b = self.input.compile(c)
        if b.limit is not None or b.offset or b.ranked or b.max_per is not None or b.distinct_on is not None or b.windows:
            b = c.wrap(b)
        sc = b.scope
        b.order = [(e, sc, desc, not nulls_last) for e, desc, nulls_last in self.keys]
        return b


class _Slice(_Node):
    def __init__(self, input: _Node, n: int | None, offset: int = 0):
        self.input, self.n, self.offset = input, n, offset

    def compile(self, c: _Compiler) -> _Block:
        b = self.input.compile(c)
        if self.offset:
            if b.limit is not None or b.offset:
                b = c.wrap(b)
            b.offset = self.offset
            b.limit = self.n
        elif self.n is not None:
            b.limit = self.n if b.limit is None else min(b.limit, self.n)
        return b


class _Group(_Node):
    def __init__(self, keys: list[tuple[str, Expr]], aggs: list[tuple[str, Expr]], input: _Node | None = None):
        self.input, self.keys, self.aggs = input, keys, aggs

    def compile(self, c: _Compiler) -> _Block:
        return self.fold(c, self.input.compile(c))

    def fold(self, c: _Compiler, b: _Block) -> _Block:
        for name, e in self.aggs:
            if not _has_agg(e):
                raise ChloeDBError(f"agg takes aggregates (col(x).sum(), st.len(), ...), and {name} is not one")
            if _has_window(e):
                raise ChloeDBError("a window in agg: compute it with with_columns before the group_by")
        for name, e in self.keys:
            if _has_agg(e) or _has_window(e):
                raise ChloeDBError(f"group_by takes columns and expressions of rows, and {name} is not one")
        needed = set()
        for _, e in self.keys + self.aggs:
            needed |= _names(e)
        if not b.plain():
            b = c.wrap(b, keep=needed)
        if b.kind == "table" and not self._folds(b):
            b = c.wrap(b, keep=needed)
        sc = b.scope
        out = {}
        for name, e in self.keys + self.aggs:
            out[name] = _entry(e, sc)
        types = {}
        for name, e in self.keys + self.aggs:
            t = _type_of(e, b)
            if t is not None:
                types[name] = t
        b.out, b.columns, b.types = out, list(out), types
        b.group = [(e, sc) for _, e in self.keys]
        b.order = []
        return b

    def _folds(self, b: _Block) -> bool:
        """Whether one table's SELECT can group: keys that are indexed
        columns or parts of a TIMESTAMP field, aggregates of columns."""
        for _, e in self.keys:
            if isinstance(e, _Template) and _plain_column(e.e, b, ("timestamp",)):
                continue
            if not _plain_column(e, b):
                return False
        for _, e in self.aggs:
            if isinstance(e, _Agg) and (e.arg is None or _plain_column(e.arg, b, ("int", "float", "keyword", "bool", "timestamp"))):
                continue
            if isinstance(e, _Pct) and _plain_column(e.e, b, ("int", "float")):
                continue
            return False
        return True


class _Join(_Node):
    def __init__(self, left: _Node, right: _Node, left_on: list[Expr], right_on: list[Expr], how: str, suffix: str):
        self.left, self.right, self.left_on, self.right_on, self.how, self.suffix = left, right, left_on, right_on, how, suffix

    def compile(self, c: _Compiler) -> _Block:
        sides = []
        for node in (self.left, self.right):
            b = node.compile(c)
            if not b.plain() or b.order or b.kind == "join":
                b = c.wrap(b)
            b.qual = c.qual()
            sides.append(b)
        lb, rb = sides
        lsc, rsc = lb.scope, rb.scope
        keys = [(le, lsc, re_, rsc) for le, re_ in zip(self.left_on, self.right_on)]
        drop = {e.name for e in self.right_on if isinstance(e, _Col)}
        jb = _Block("join", None, [])
        out: dict[str, Any] = {}
        types: dict[str, dict] = {}
        for n in lb.columns:
            out[n] = lb.out[n]
            if n in lb.types:
                types[n] = lb.types[n]
        for n in rb.columns:
            if n in drop:
                continue
            name = n if n not in out else n + self.suffix
            if name in out:
                raise ChloeDBError(f"the join gives two columns named {name!r}: rename one first")
            out[name] = rb.out[n]
            if n in rb.types:
                types[name] = rb.types[n]
        jb.out, jb.columns, jb.types = out, list(out), types
        jb.base = dict(out)
        jb.where = list(lb.where) + (list(rb.where) if self.how == "inner" else [])
        jb.join = (self.how, lb, rb, keys, [] if self.how == "inner" else list(rb.where))
        return jb


class _Distinct(_Node):
    def __init__(self, input: _Node, subset: list[Expr] | None):
        self.input, self.subset = input, subset

    def compile(self, c: _Compiler) -> _Block:
        b = self.input.compile(c)
        if b.limit is not None or b.offset or b.group is not None or b.windows or b.distinct or b.distinct_on is not None or b.ranked or b.max_per is not None:
            b = c.wrap(b)
        if self.subset is None:
            b.distinct = True
        else:
            b.distinct_on = [(e, b.scope) for e in self.subset]
        return b


class _Rank(_Node):
    """search, near and rank_by: the rows ranked by a score, best first,
    the first `k` kept, with a score column."""

    def __init__(self, input: _Node, expr: Expr, k: int):
        self.input, self.expr, self.k = input, expr, k

    def compile(self, c: _Compiler) -> _Block:
        b = self.input.compile(c)
        if not b.plain():
            b = c.wrap(b)
        b.order = [(self.expr, b.scope, True, False)]
        b.limit = self.k
        b.ranked = True
        _add_score(b)
        return b


def _add_score(b: _Block) -> None:
    name = "score"
    while name in b.out and not (isinstance(b.out[name], _Raw) and b.out[name].text == "score()"):
        name = "_" + name
    b.out[name] = _Raw("score()")
    if name not in b.columns:
        b.columns.append(name)
    b.types.pop(name, None)


class _Rerank(_Node):
    def __init__(self, input: _Node, expr: Expr, k: int | None):
        self.input, self.expr, self.k = input, expr, k

    def compile(self, c: _Compiler) -> _Block:
        b = self.input.compile(c)
        if not b.ranked or b.kind != "table" or b.max_per is not None or b.offset:
            raise ChloeDBError("rerank follows search(), near() or rank_by() on a table: it ranks their rows again")
        cols = [n for n in b.columns if not (isinstance(b.out[n], _Raw) and b.out[n].text == "score()")]
        # A phase reads the rows of the one before, every column of them.
        star = _Block("table", b.source, [], table=b.table)
        star.where, star.order, star.limit = b.where, b.order, b.limit
        star.out, star.columns = {"*": _Raw("*")}, ["*"]
        name = f"q{len(c.ctes) + 1}"
        c.ctes.append((name, c.render(star).replace('* AS "*"', "*")))
        nb = _Block("derived", _q(name), [], types=b.types)
        nb.base = {n: _Src(nb, n) for n in b.base}
        sc = nb.base
        nb.out = {n: _Def(b.out[n].expr, sc) if isinstance(b.out[n], _Def) else sc.get(n, b.out[n]) for n in cols}
        nb.columns = list(cols)
        nb.order = [(self.expr, sc, True, False)]
        nb.limit = b.limit if self.k is None else min(self.k, b.limit or self.k)
        nb.ranked = True
        _add_score(nb)
        return nb


class _MaxPer(_Node):
    def __init__(self, input: _Node, k: int, field: Expr):
        self.input, self.k, self.field = input, k, field

    def compile(self, c: _Compiler) -> _Block:
        b = self.input.compile(c)
        if not b.order:
            raise ChloeDBError("max_per keeps the first rows of each value in an order: search, rank or sort first")
        if b.max_per is not None or b.offset:
            b = c.wrap(b)
        b.max_per = (self.k, self.field, b.scope)
        return b


class _Union(_Node):
    def __init__(self, inputs: list[_Node], distinct: bool):
        self.inputs, self.distinct = inputs, distinct

    def compile(self, c: _Compiler) -> _Block:
        parts, cols, types = [], None, {}
        for node in self.inputs:
            b = node.compile(c)
            if cols is None:
                cols = list(b.columns)
                types = dict(b.types)
            elif set(b.columns) != set(cols):
                raise ChloeDBError(f"union takes frames of the same columns: {', '.join(cols)} and {', '.join(b.columns)}")
            else:
                b.columns = list(cols)
            parts.append(c.render(b))
        name = f"q{len(c.ctes) + 1}"
        word = " UNION " if self.distinct else " UNION ALL "
        c.ctes.append((name, word.join(f"({p})" for p in parts)))
        return _Block("derived", _q(name), cols or [], types=types)


# The frame.


class Frame:
    """A lazy query of a store's table, built a step at a time as Polars
    builds a LazyFrame; each method returns a new frame.

    Make one with `store["table"]` (or `store.frame("table")`), where the
    store is a `Database` or a `Client`. See the module's documentation."""

    def __init__(self, store: Any, node: _Node):
        self._store = store
        self._node = node

    def _then(self, node: _Node) -> "Frame":
        return Frame(self._store, node)

    # Rows and columns.

    def select(self, *exprs: Any, **named: Any) -> "Frame":
        """The columns of the answer: names, expressions (named by their
        alias or the column they read) and `name=expression`. Aggregates
        alone give one row: `select(n=st.len(), total=col("x").sum())`."""
        return self._then(_Project(self._node, _items(exprs, named), keep=False))

    def with_columns(self, *exprs: Any, **named: Any) -> "Frame":
        """Every column, with these added (or replaced, by name)."""
        return self._then(_Project(self._node, _items(exprs, named), keep=True))

    def drop(self, *columns: str) -> "Frame":
        names = [n for c in columns for n in ([c] if isinstance(c, str) else c)]
        return self._then(_Project(self._node, [], keep=True, drop=names))

    def rename(self, mapping: Mapping[str, str]) -> "Frame":
        return self._then(_Project(self._node, [], keep=True, rename=mapping))

    def filter(self, *conditions: Any, **equal: Any) -> "Frame":
        """The rows where every condition holds; `name=value` for an
        equality. Conditions on indexed fields are answered by the
        indexes."""
        conds = [_expr(c) for c in conditions] + [_Col(n) == v for n, v in equal.items()]
        if not conds:
            raise ChloeDBError("filter takes a condition")
        node = self._node
        for cond in conds:
            node = _Filter(node, cond)
        return self._then(node)

    where = filter

    def sort(self, by: Any, *more: Any, descending: bool | Sequence[bool] = False, nulls_last: bool = True) -> "Frame":
        """Orders the rows by columns or expressions; rows without a value
        come last (`nulls_last=False`: first)."""
        keys = ([by] if isinstance(by, (str, Expr)) else list(by)) + list(more)
        desc = [descending] * len(keys) if isinstance(descending, bool) else list(descending)
        if len(desc) != len(keys):
            raise ChloeDBError("descending takes one bool, or one for each key")
        return self._then(_Sort(self._node, [(_colish(k), bool(d), nulls_last) for k, d in zip(keys, desc)]))

    def head(self, n: int = 5) -> "Frame":
        """The first `n` rows (any `n` rows of a frame neither sorted nor
        ranked)."""
        return self._then(_Slice(self._node, _count(n, "head")))

    limit = head

    def slice(self, offset: int, length: int | None = None) -> "Frame":
        if offset < 0:
            raise ChloeDBError("slice's offset counts from the first row, from 0")
        return self._then(_Slice(self._node, None if length is None else _count(length, "slice"), int(offset)))

    def group_by(self, *keys: Any, **named: Any) -> "GroupBy":
        """Groups by columns or expressions (`month=col("ts").dt.truncate("month")`);
        `.agg(...)` gives a row per group."""
        return GroupBy(self, _items(keys, named))

    def join(self, other: "Frame", on: Any = None, *, how: str = "inner", left_on: Any = None, right_on: Any = None, suffix: str = "_right") -> "Frame":
        """The rows of both frames whose keys are equal (`how="inner"`),
        or every row of this one with the other's where they are
        (`how="left"`). The other frame's key columns are left out; a
        column both have takes `suffix` on the other's side. Both frames
        are of one store."""
        if not isinstance(other, Frame):
            raise TypeError("join takes a Frame")
        if other._store is not self._store:
            raise ChloeDBError("join takes frames of one store: copy one with write_to first")
        if how not in ("inner", "left"):
            raise ChloeDBError(f"join's how is 'inner' or 'left', not {how!r} (ChloeDB has no right, full or cross joins)")
        if on is not None:
            left_on = right_on = on
        if left_on is None or right_on is None:
            raise ChloeDBError("join takes on=, or left_on= and right_on=")
        lk = [_colish(k) for k in ([left_on] if isinstance(left_on, (str, Expr)) else left_on)]
        rk = [_colish(k) for k in ([right_on] if isinstance(right_on, (str, Expr)) else right_on)]
        if len(lk) != len(rk) or not lk:
            raise ChloeDBError("join takes as many left keys as right keys")
        return self._then(_Join(self._node, other._node, lk, rk, how, suffix))

    def unique(self, subset: Any = None, *, keep: str = "any") -> "Frame":
        """Each row once; with `subset`, one row of each value of those
        columns (the first in the frame's order, `DISTINCT ON`)."""
        if keep not in ("any", "first"):
            raise ChloeDBError("unique's keep is 'any' or 'first' (the first in the frame's order)")
        if subset is None:
            return self._then(_Distinct(self._node, None))
        keys = [subset] if isinstance(subset, (str, Expr)) else list(subset)
        return self._then(_Distinct(self._node, [_colish(k) for k in keys]))

    def union(self, *others: "Frame", distinct: bool = False) -> "Frame":
        """The rows of this frame and the others (each row once with
        `distinct`); every frame has the same columns."""
        return concat([self, *others], distinct=distinct)

    # Samples, splits and shuffles: by a hash of each row's key (SQL's
    # hash()), so that a seed gives the same rows wherever the frame runs.

    def sample(self, n: int | None = None, *, fraction: float | None = None, seed: int = 0, on: Any = None) -> "Frame":
        """A seeded sample: `fraction=` of the rows (those whose hash falls
        under it: about that share, the rows of a smaller fraction among
        those of a larger), or `n` rows (the `n` first of `shuffle`). The
        same seed gives the same rows, from a file or a server. Rows are
        told apart by the table's key, or by the columns `on`; a row whose
        `on` is NULL is in no sample."""
        if (n is None) == (fraction is None):
            raise ChloeDBError("sample takes n= (a number of rows) or fraction= (a share of them)")
        if n is not None:
            return self.shuffle(seed=seed, on=on).head(_count(n, "sample"))
        return self.filter(_under(self._row_hash(on, seed, "sample"), fraction, "sample"))

    def split(self, fraction: float = 0.8, *, seed: int = 0, on: Any = None) -> tuple["Frame", "Frame"]:
        """Two frames of this one's rows: about `fraction` of them in the
        first and the rest in the second (`train, test = frame.split(0.8,
        seed=7)`), each row in one, the same rows for a seed wherever the
        frames run. Rows are told apart by the table's key, or by the
        columns `on` (`on="user"` keeps each user's rows on one side); a
        row whose `on` is NULL is in neither."""
        h = self._row_hash(on, seed, "split")
        under = _under(h, fraction, "split")
        return self.filter(under), self.filter(~under)

    def shuffle(self, *, seed: int = 0, on: Any = None) -> "Frame":
        """The rows in a random order that the seed fixes: by the hash of
        each row's key, or of the columns `on`."""
        return self.sort(self._row_hash(on, seed, "shuffle"))

    def _row_hash(self, on: Any, seed: Any, what: str) -> Expr:
        if isinstance(seed, bool) or not isinstance(seed, int):
            raise ChloeDBError(f"{what}'s seed is an int, not {seed!r}")
        if on is None:
            node = self._node
            while not isinstance(node, _Table) and getattr(node, "input", None) is not None:
                node = node.input
            key = node.schema.get("key") if isinstance(node, _Table) else None
            if not key or key not in self.columns:
                raise ChloeDBError(f"{what} hashes each row's key, and this frame has none: name the columns that tell its rows apart, {what}(..., on=\"user\")")
            on = key
        cols = [on] if isinstance(on, (str, Expr)) else list(on)
        if not cols:
            raise ChloeDBError(f"{what}'s on= names one or more columns")
        h: Any = seed
        for c in cols:
            h = _colish(c).hash(h)
        return h

    # Search.

    def search(self, text: str, on: str, *, k: int = DEFAULT_K, syntax: str | None = None) -> "Frame":
        """The `k` rows best by BM25 for `text` in the TEXT field `on`,
        best first, with their `score`. `text` may weigh (`word^2`),
        require (`+word`) or exclude (`-word`) words; `syntax="plain"`
        reads it as plain words."""
        return self.rank_by(bm25(on, text, syntax=syntax), k=k)

    def near(self, query: Any, on: str, *, k: int = DEFAULT_K) -> "Frame":
        """The `k` rows whose VECTOR field `on` is most similar to `query`
        (a vector, or a text the field's model embeds where the query
        runs), most similar first, with their `score`."""
        return self.rank_by(similarity(on, query), k=k)

    def rank_by(self, expr: Expr, *, k: int = DEFAULT_K) -> "Frame":
        """The `k` rows best by a ranking expression of scores and columns
        (`0.6 * st.minmax(st.bm25(...)) + 0.4 * st.minmax(st.similarity(...))`),
        best first, with their `score`."""
        return self._then(_Rank(self._node, _expr(expr), _count(k, "k")))

    def rerank(self, expr: Expr, *, k: int | None = None) -> "Frame":
        """Ranks the rows of the search before again, by `expr` (a costlier
        ranking: a cross-encoder, a model, a mix of fields), and keeps the
        best `k` (all of them by default): a rerank phase."""
        return self._then(_Rerank(self._node, _expr(expr), None if k is None else _count(k, "k")))

    def max_per(self, k: int, field: str) -> "Frame":
        """At most `k` rows of each value of `field` (a KEYWORD or number),
        the first in the frame's order: at most two clips of a video."""
        return self._then(_MaxPer(self._node, _count(k, "max_per"), _colish(field)))

    # What it is.

    def _compile(self) -> tuple[str, dict, _Block]:
        c = _Compiler()
        b = self._node.compile(c)
        return c.sql(b), c.r.params, b

    def sql(self) -> str:
        """The SQL statement the frame runs; its values are parameters
        (`params()`)."""
        return self._compile()[0]

    def params(self) -> dict:
        """The values of the SQL's parameters."""
        return self._compile()[1]

    @property
    def columns(self) -> list[str]:
        return list(self._compile()[2].columns)

    def explain(self) -> str:
        """The plan the store would run, one operator a line."""
        sql, params, _ = self._compile()
        return "\n".join(self._store._explain(sql, params))

    def __repr__(self) -> str:
        try:
            sql = self.sql()
        except ChloeDBError as e:
            sql = f"(does not compile: {e})"
        return f"<chloedb.Frame of {self._store!r}>\n{sql}"

    def _repr_html_(self) -> str:
        t = self.head(10).to_arrow()
        try:
            return t.to_pandas()._repr_html_()
        except ImportError:
            from html import escape

            head = "".join(f"<th>{escape(n)}</th>" for n in t.column_names)
            rows = "".join("<tr>" + "".join(f"<td>{escape(str(v))}</td>" for v in r.values()) + "</tr>" for r in t.to_pylist())
            return f"<table><thead><tr>{head}</tr></thead><tbody>{rows}</tbody></table>"

    # Running it.

    def _run(self) -> Any:
        """The answer, as something with `__arrow_c_stream__`."""
        sql, params, b = self._compile()
        return self._store._frame_arrow(sql, params, b.types)

    def collect(self):
        """Runs the frame: its answer as a pyarrow Table."""
        return self.to_arrow()

    def to_arrow(self):
        """Runs the frame: a pyarrow Table (pyarrow must be installed)."""
        pyarrow = _pyarrow()
        a = self._run()
        return a if isinstance(a, pyarrow.Table) else pyarrow.table(a)

    def __arrow_c_stream__(self, requested_schema: Any = None) -> Any:
        """Runs the frame, for any library that reads Arrow streams:
        `pl.DataFrame(frame)`, `pa.table(frame)`, DuckDB."""
        _pyarrow()
        return self._run().__arrow_c_stream__(requested_schema)

    def to_polars(self):
        """Runs the frame: a polars DataFrame, sharing the answer's memory."""
        import polars

        return polars.DataFrame(self._run())

    def to_pandas(self, *, arrow_dtypes: bool = True):
        """Runs the frame: a pandas DataFrame backed by the Arrow columns
        (`pandas.ArrowDtype`), or with `arrow_dtypes=False` copied into
        NumPy arrays."""
        import pandas

        t = self.to_arrow()
        return t.to_pandas(types_mapper=pandas.ArrowDtype) if arrow_dtypes else t.to_pandas()

    def to_numpy(self):
        """Runs the frame: a 2-D NumPy array of its columns (numbers), or of
        its one VECTOR column, `(rows, dimensions)` float32."""
        import numpy

        t = self.to_arrow()
        if t.num_columns == 1 and _is_list(t.schema.field(0).type):
            c = t.column(0).combine_chunks()
            if len(c) == 0:
                return numpy.zeros((0, 0), dtype=numpy.float32)
            flat = c.flatten().to_numpy(zero_copy_only=False)
            if c.null_count or len(flat) % len(c):
                raise ChloeDBError(f"{t.column_names[0]} holds vectors of different lengths or NULLs: filter them out first")
            return flat.reshape(len(c), len(flat) // len(c))
        return numpy.column_stack([t.column(i).to_numpy(zero_copy_only=False) for i in range(t.num_columns)]) if t.num_columns else numpy.zeros((t.num_rows, 0))

    def to_dicts(self) -> list[dict]:
        """Runs the frame: a dict a row."""
        return self.to_arrow().to_pylist()

    def iter_batches(self, batch_size: int = 65_536) -> Iterator[Any]:
        """Runs the frame and yields its answer as pyarrow RecordBatches of
        at most `batch_size` rows. From a server, as it reads them: a page
        of its scroll at a time, each page's columns typed by its values."""
        pages = getattr(self._store, "_frame_batches", None)
        if pages is None:
            yield from self.to_arrow().to_batches(max_chunksize=batch_size)
            return
        _pyarrow()
        sql, params, b = self._compile()
        for t in pages(sql, params, b.types):
            yield from t.to_batches(max_chunksize=batch_size)

    def count(self) -> int:
        """The number of rows the frame has (`COUNT(*)`, run where the data is)."""
        t = self.select(len_().alias("len")).to_arrow()
        return int(t.column(0)[0].as_py()) if t.num_rows else 0

    def item(self) -> Any:
        """The one value of a one-row, one-column answer (a total)."""
        t = self.to_arrow()
        if t.num_rows != 1 or t.num_columns != 1:
            raise ChloeDBError(f"item() needs one row and one column, and the answer has {t.num_rows} rows and {t.num_columns} columns")
        return t.column(0)[0].as_py()

    # Writes: explicit, on a table's frame.

    def _table(self, what: str) -> tuple[str, list[Expr]]:
        node, conds = self._node, []
        while isinstance(node, _Filter):
            conds.append(node.cond)
            node = node.input
        if not isinstance(node, _Table):
            raise ChloeDBError(f"{what} works on a table's frame and its filters: store['t'].filter(...).{what}(...)")
        return node.name, conds[::-1]

    def get(self, key: Any) -> dict | None:
        """The row with this primary key, as a dict (a read by key: no
        query)."""
        name, conds = self._table("get")
        if conds:
            raise ChloeDBError("get reads a row by key, from the table's frame itself")
        return self._store.get(key, table=name)

    def upsert(self, data: Any) -> int:
        """Writes rows into the table, replacing the rows of their keys:
        anything Arrow streams (a pyarrow Table, a polars or pandas
        DataFrame, a DuckDB relation, a frame), or a list of dicts.
        Returns how many."""
        name, conds = self._table("upsert")
        if conds:
            raise ChloeDBError("upsert writes into the table's frame itself, not a filtered one")
        return int(self._store._load(name, data)["rows"])

    def update(self, values: Mapping[str, Any] | None = None, **set: Any) -> int:
        """Sets columns of the rows the frame's filters keep, to values or
        expressions of the row: `orders.filter(col("id") == "o1").update(status="void")`.
        Returns how many rows changed."""
        name, conds = self._table("update")
        given = {**(values or {}), **set}
        if not given:
            raise ChloeDBError("update takes column=value or column=expression")
        c = _Compiler()
        b = _Table(name, self._node_schema()).compile(c)
        sc = b.scope
        parts = [f"{_q(k)} = {v._sql(_Bound(c.r, sc)) if isinstance(v, Expr) else c.r.value(v)}" for k, v in given.items()]
        sql = f"UPDATE {_q(name)} SET {', '.join(parts)}"
        if conds:
            sql += " WHERE " + " AND ".join(c.r.render(e, sc) for e in conds)
        return int(self._store.sql(sql, c.r.params))

    def delete(self, *, all: bool = False) -> int:
        """Deletes the rows the frame's filters keep; every row of the
        table only with `all=True`. Returns how many."""
        name, conds = self._table("delete")
        if not conds and not all:
            raise ChloeDBError("delete() of a frame without a filter deletes every row: pass all=True to mean it")
        c = _Compiler()
        sc = _Table(name, self._node_schema()).compile(c).scope
        sql = f"DELETE FROM {_q(name)}"
        if conds:
            sql += " WHERE " + " AND ".join(c.r.render(e, sc) for e in conds)
        return int(self._store.sql(sql, c.r.params))

    def _node_schema(self) -> dict:
        node = self._node
        while not isinstance(node, _Table):
            node = node.input
        return node.schema

    def write_to(self, store: Any, table: str, *, if_exists: str = "fail", key: str | None = None, text: Sequence[str] = (), embed: Mapping[str, Any] | None = None) -> "Frame":
        """Runs the frame and writes its rows to `table` of `store` (another
        file, a server): a new table made from the answer's columns, or
        with `if_exists="append"` added to one (upserted by key). The key
        is the source table's when the answer holds it. Returns the
        written table's frame."""
        if key is None:
            node = self._node
            while not isinstance(node, _Table) and hasattr(node, "input") and node.input is not None:
                node = node.input
            if isinstance(node, _Table) and node.schema.get("key") in self.columns:
                key = node.schema["key"]
        return store.create_table(table, self.to_arrow(), key=key, text=text, embed=embed, if_exists=if_exists)


def _under(h: Expr, fraction: Any, what: str) -> Expr:
    """The rows whose hash `h` falls under `fraction`: its low 32 bits
    against the fraction of 2^32."""
    if isinstance(fraction, bool) or not isinstance(fraction, (int, float)) or not 0 <= fraction <= 1:
        raise ChloeDBError(f"{what}'s fraction is a number from 0 to 1, not {fraction!r}")
    return (h % 4294967296) < round(fraction * 4294967296)


def _pyarrow():
    try:
        import pyarrow
    except ImportError:
        raise ImportError("frames give and take Arrow tables, so they need pyarrow: pip install pyarrow") from None
    return pyarrow


class _Bound:
    """A renderer bound to one scope, for an expression rendered alone."""

    def __init__(self, r: _Render, scope: dict):
        self.r, self.scope = r, scope

    def __getattr__(self, name: str) -> Any:
        return getattr(self.r, name)

    def col(self, name: str) -> str:
        prev, self.r._scope = self.r._scope, self.scope
        try:
            return self.r.col(name)
        finally:
            self.r._scope = prev


def _count(n: Any, what: str) -> int:
    if isinstance(n, bool) or not isinstance(n, int) or n < 0:
        raise ChloeDBError(f"{what} takes a number of rows, not {n!r}")
    return n


def _is_list(t: Any) -> bool:
    import pyarrow

    return pyarrow.types.is_list(t) or pyarrow.types.is_fixed_size_list(t) or pyarrow.types.is_large_list(t)


class GroupBy:
    """A frame grouped by keys; `agg` gives a row per group."""

    def __init__(self, frame: Frame, keys: list[tuple[str, Expr]]):
        if not keys:
            raise ChloeDBError("group_by takes columns or expressions to group by")
        self._frame, self._keys = frame, keys

    def agg(self, *aggs: Any, **named: Any) -> Frame:
        """The groups' keys and aggregates: `col("amount").sum()`,
        `.mean()`, `.median()`, `.quantile(0.9)`, `.n_unique()`,
        `.min()`, `.max()`, `st.len()`."""
        items = _items(aggs, named)
        names = {n for n, _ in self._keys}
        clash = [n for n, _ in items if n in names]
        if clash:
            raise ChloeDBError(f"an aggregate is named as a key ({', '.join(clash)}): rename it with .alias()")
        return self._frame._then(_Group(self._keys, items, self._frame._node))

    def len(self, name: str = "len") -> Frame:
        """The number of rows of each group."""
        return self.agg(len_().alias(name))


def concat(frames: Sequence[Frame], *, distinct: bool = False) -> Frame:
    """The rows of every frame, one after another (`UNION ALL`; each row
    once with `distinct`). The frames have the same columns and store."""
    frames = list(frames)
    if not frames:
        raise ChloeDBError("concat takes frames")
    store = frames[0]._store
    if any(f._store is not store for f in frames):
        raise ChloeDBError("concat takes frames of one store")
    if len(frames) == 1:
        return frames[0]
    return Frame(store, _Union([f._node for f in frames], distinct))


# Stores: what Database and Client share for frames.


def _arrow_table(data: Any):
    """Anything that gives rows as a pyarrow Table: an Arrow stream (a
    pyarrow Table or RecordBatchReader, a polars DataFrame, a DuckDB
    relation, a frame), a pandas DataFrame, or a list of dicts."""
    pyarrow = _pyarrow()
    if isinstance(data, pyarrow.Table):
        return data
    if isinstance(data, pyarrow.RecordBatch):
        return pyarrow.Table.from_batches([data])
    if isinstance(data, Frame):
        return data.to_arrow()
    if type(data).__module__.startswith("pandas"):
        return pyarrow.Table.from_pandas(data, preserve_index=False)
    if hasattr(data, "__arrow_c_stream__"):
        return pyarrow.table(data)
    if isinstance(data, Mapping):
        return pyarrow.Table.from_pylist([dict(data)])
    if isinstance(data, Iterable):
        return pyarrow.Table.from_pylist([dict(r) for r in data])
    raise TypeError(f"rows are an Arrow table or stream, a pandas DataFrame or a list of dicts, not {type(data).__name__}")


def _schema_of(plan: dict, name: str, key: str | None, text: Sequence[str], keyword: Sequence[str], embed: Mapping[str, Any] | None) -> dict:
    """A table's schema from the Parquet import's plan of its rows, with
    the text, keyword and embedded fields asked for."""
    fields = [dict(f) for f in plan["fields"]]
    names = {f["name"] for f in fields}
    for kind, which in (("text", text), ("keyword", keyword)):
        for n in [which] if isinstance(which, str) else which:
            if n not in names:
                raise ChloeDBError(f"{kind}={n!r}: the rows have no column {n!r}")
            for f in fields:
                if f["name"] == n:
                    f["type"] = kind
                    f.pop("multi", None) if kind == "text" else None
    for n, spec in (embed or {}).items():
        if n in names:
            raise ChloeDBError(f"embed={n!r}: the rows have a column {n!r} already")
        spec = dict(spec)
        if "from" not in spec or "model" not in spec:
            raise ChloeDBError(f"embed={{{n!r}: {{'from': [...], 'model': ...}}}} names the fields embedded and the model")
        spec["from"] = [spec["from"]] if isinstance(spec["from"], str) else list(spec["from"])
        fields.append({"name": n, "type": "vector", "embed": spec})
    schema: dict[str, Any] = {"table": name, "fields": fields}
    k = plan.get("key") if key is None else key
    if k:
        schema["key"] = k
    return schema


class FrameStore:
    """Frames over a store: `store["table"]`, `create_table`. `Database`
    and `Client` give `tables()`, `sql`, `_frame_arrow` (run SQL, Arrow
    out), `_plan_rows` (a schema from a Parquet sample), `_create` and
    `_load` (Arrow in)."""

    def frame(self, table: str | None = None) -> Frame:
        """A frame of every row of `table` (the first table without one)."""
        tables = self.tables()
        if table is None:
            if not tables:
                raise ChloeDBError("the store has no tables yet: create one with create_table(name, data)")
            schema = tables[0]
        else:
            match = [t for t in tables if t["table"] == table] or [t for t in tables if t["table"].lower() == table.lower()]
            if not match:
                raise ChloeDBError(f"no table {table!r}: there are {', '.join(t['table'] for t in tables) or 'none'}")
            schema = match[0]
        return Frame(self, _Table(schema["table"], schema))

    def __getitem__(self, table: str) -> Frame:
        if not isinstance(table, str):
            raise TypeError("store[...] takes a table's name")
        return self.frame(table)

    def create_table(self, name: str, data: Any = None, *, key: str | None = None, text: Sequence[str] = (), keyword: Sequence[str] = (), embed: Mapping[str, Any] | None = None, schema: Any = None, if_exists: str = "fail") -> Frame:
        """Makes table `name` from rows (anything `upsert` takes) and
        returns its frame. The schema comes from the rows' Arrow types, as
        the Parquet import makes one (strings are KEYWORD, or TEXT when
        they read as prose; lists of numbers of one length are vectors),
        with `key` the primary key (default a column named id; "" for
        none), `text` and `keyword` the columns to make TEXT (indexed for
        BM25) or KEYWORD, and `embed` vector fields a model fills from
        other fields: `embed={"emb": {"from": ["title", "body"], "model":
        "potion-retrieval-32M"}}`. `schema` gives the whole schema instead.

        If the table exists: `if_exists="fail"` (the default) refuses,
        `"append"` writes the rows into it (upserted by key), and
        `"replace"` makes it again (the first table of a database cannot be
        dropped, so its rows are deleted and the new ones written under
        its schema)."""
        if if_exists not in ("fail", "append", "replace"):
            raise ChloeDBError("if_exists is 'fail', 'append' or 'replace'")
        rows = None if data is None else _arrow_table(data)
        existing = [t["table"] for t in self.tables()]
        if name in existing:
            if if_exists == "fail":
                raise ChloeDBError(f"table {name!r} exists: if_exists='append' adds rows, 'replace' makes it again")
            if if_exists == "replace":
                if existing[0] == name:
                    self.sql(f"DELETE FROM {_q(name)}")
                else:
                    self.sql(f"DROP TABLE {_q(name)}")
                    existing.remove(name)
            if name in existing:
                if rows is not None and rows.num_rows:
                    self._load(name, rows)
                return self[name]
        if schema is None:
            if rows is None:
                raise ChloeDBError("create_table makes a schema from rows: pass data, or schema=")
            schema = _schema_of(self._plan_rows(name, rows, key), name, key, text, keyword, embed)
        elif not isinstance(schema, Mapping):
            from . import _schema_json
            import json

            schema = json.loads(_schema_json(schema))
        schema = {**schema, "table": name}
        self._create(schema)
        if rows is not None and rows.num_rows:
            self._load(name, rows)
        return self[name]

    def _plan_rows(self, name: str, rows: Any, key: str | None) -> dict:
        """The Parquet import's plan of a table for the rows' first
        `_SAMPLE_ROWS` (types, and the values that choose between a few)."""
        import pyarrow.parquet

        with tempfile.TemporaryDirectory() as d:
            path = os.path.join(d, f"{name}.parquet")
            pyarrow.parquet.write_table(rows.slice(0, _SAMPLE_ROWS), path)
            return self._plan_parquet_file(path, name, key)


def _client_arrow(result: Any, types: Mapping[str, dict]):
    """A Client's JSON answer as a pyarrow Table, typed by the fields of
    the columns read as they are (the rest as pyarrow infers them)."""
    import json
    import pyarrow

    arrays = {}
    for i, name in enumerate(result.columns):
        values = [r[i] for r in result.rows]
        f = types.get(name)
        arrays[name] = _typed(values, f) if f else _guess(values, name)
    return pyarrow.table(arrays) if arrays else pyarrow.table({})


def _typed(values: list, f: dict):
    import json
    import pyarrow as pa

    t, multi = f.get("type"), f.get("multi")
    if t in ("keyword", "text", "media"):
        base = pa.string()
        if t == "media":
            values = [v if v is None or isinstance(v, str) else json.dumps(v) for v in values]
    elif t == "int":
        base = pa.int64()
    elif t == "float":
        base = pa.float64()
    elif t == "bool":
        base = pa.bool_()
    elif t == "timestamp":
        return pa.array(values, pa.string()).cast(pa.timestamp("us", tz="UTC"))
    elif t == "vector":
        return pa.array(values, pa.list_(pa.float32()))
    elif t in ("json", "edges", "sparse"):
        return pa.array([v if v is None or isinstance(v, str) else json.dumps(v, separators=(",", ":")) for v in values], pa.string())
    else:
        return pa.array(values)
    return pa.array(values, pa.list_(base) if multi else base)


def _guess(values: list, name: str):
    import pyarrow as pa

    if name.endswith("score"):
        return pa.array(values, pa.float64())
    try:
        return pa.array(values)
    except (pa.ArrowInvalid, pa.ArrowTypeError):
        return pa.array([None if v is None else str(v) for v in values], pa.string())
