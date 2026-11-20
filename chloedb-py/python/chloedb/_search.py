"""What `Database` and `Client` share: the schema's reading order,
parameters, `table`, `search` and the rank log."""

from __future__ import annotations

import re
from typing import Any, Iterable, Mapping, Sequence

from ._native import ChloeDBError
from .build import Expr, Query, bm25, hops, rrf, score, similarity
from .result import Result


def ordered_schema(s: dict) -> dict:
    """The short schema form in reading order: table, key, the table's
    options, fields, and each field's name and type first."""
    out = {"table": s["table"]}
    for k in ("key", "shard_by", "cluster_by", "retain", "retain_by", "edges"):
        if s.get(k) is not None:
            out[k] = s[k]
    out["fields"] = [{"name": f["name"], "type": f["type"], **f} for f in s["fields"]]
    return out


def params(given: Mapping[str, Any] | None, kw: Mapping[str, Any], sql: str | None = None, call: str = "query") -> dict | None:
    """Query parameters from a dict and keyword arguments. They are named,
    as ChloeDB's SQL names them (`:q`): values by position, sqlite3's `?`
    and psycopg's `%s` with a tuple, raise a TypeError that shows the call
    with names (MAYA-405)."""
    if given is not None and not hasattr(given, "keys"):
        raise TypeError(positional(given, sql, call))
    if not given and not kw:
        return None
    return {**(given or {}), **kw}


# A quoted string or identifier, or a placeholder by position: `?`, `?1`,
# `$1`, `%s`.
_POSITIONAL = re.compile(r"'(?:[^']|'')*'|\"(?:[^\"]|\"\")*\"|\?\d*|\$\d+|%s")


def positional(given: Any, sql: str | None, call: str) -> str:
    """Why parameters given by position are refused, with the call that
    gives them by name: each `?` or `%s` of `sql` becomes :p1, :p2, ..."""
    holes = [m for m in _POSITIONAL.finditer(sql or "") if m.group(0)[0] not in "'\""]
    # A tuple is the values, as sqlite3 takes them; a list too, unless the
    # statement has one placeholder and it holds several (a vector, or
    # IN (?)).
    values = list(given) if isinstance(given, tuple) or (isinstance(given, list) and (len(holes) != 1 or len(given) == 1)) else [given]
    names = [f"p{i + 1}" for i in range(max(len(values), len(holes), 1))]
    if sql and holes:
        it = iter(names)
        shown = _POSITIONAL.sub(lambda m: m.group(0) if m.group(0)[0] in "'\"" else ":" + next(it, "p"), sql)
    else:
        shown = "... :p1 ..."
    args = ", ".join(f"{n}={v!r}" for n, v in zip(names, values))
    return (
        f"query parameters are named in ChloeDB's SQL (:name) and given by keyword or in a dict, "
        f"not by position as sqlite3's ? and psycopg's %s take them: db.{call}({shown!r}, {args})"
    )


def quote(name: str) -> str:
    """A name as an SQL identifier, whatever it is."""
    return '"' + name.replace('"', '""') + '"'


def _time(t: Any) -> Any:
    """A time as the rank log takes it: ISO 8601 text or microseconds."""
    return t.isoformat() if hasattr(t, "isoformat") and hasattr(t, "year") else t


class Search:
    """`table`, `search` and the rank log, shared by `Database` and
    `Client`, which give `schema`, `_table_schema(name)` (a table's, read
    without reading the first table's), `_table_names()` and `_holder`
    (for `_check_table`), `_embedded(table)` (the embedded
    vector fields whose model is open), `embed`, `query`, `run`,
    `_query_logged` and `_export_rank_log`."""

    def query_logged(
        self,
        sql: str,
        params: Mapping[str, Any] | None = None,
        *,
        context: str | None = None,
        request: str | None = None,
        variant: str | None = None,
        candidates: int | None = None,
        ts: Any = None,
        table: str = "rank_log",
        interleave: str | None = None,
        variants: Sequence[str] | None = None,
        explore: Mapping[str, Any] | None = None,
    ) -> Result:
        """Runs a SELECT ranked by an ORDER BY and logs what it served to a
        log table, which the first query logged to it makes: one row per
        row answered, with its position, its score and the values its
        ranking read as they were computed (`features()`), for training on
        later. `context` is whom it served (a user, a session), `variant`
        the ranking's name (an A/B arm), `candidates` logs that many rows
        past the OFFSET, those past the LIMIT as not shown, and `ts` (a
        datetime or microseconds) when it was served, now by default.
        `result.request` is the request's id (`request`, or a new one),
        which labels its rows later: `UPDATE rank_log SET label = 1 WHERE
        request = ... AND row_key = ...`. `interleave`, a second SELECT of
        the same table and columns, shows both rankings in one list by
        team draft, each row logged with the name of the one that placed it
        (`variants`, ("a", "b") unless named); `result.teams` names them.
        `explore`, a logging policy (`{"policy": "softmax", "tau": 0.5,
        "top": 20}`, or epsilon, shuffle, swap), shows the top rows in the
        order it draws, seeded by the request, and logs each row's chance
        of being where it was shown; `result.propensities` has the shown
        rows'."""
        il = None if interleave is None else {"sql": interleave, **({"variants": list(variants)} if variants else {})}
        opts = {"table": table, "request": request, "context": context, "variant": variant, "candidates": candidates, "ts": _time(ts), "interleave": il, "explore": dict(explore) if explore is not None else None}
        return self._query_logged(sql, params, {k: v for k, v in opts.items() if v is not None})

    def export_rank_log(
        self,
        *,
        table: str = "rank_log",
        events: Mapping[str, str] | None = None,
        history: int = 0,
        window: str | float | None = None,
        grades: Mapping[str, float] | None = None,
        since: Any = None,
        until: Any = None,
    ) -> list[dict]:
        """The log table's rows as records for training, from one snapshot,
        each context's in the order served: the row's columns, `features` as
        served (a dict), and `label`. With `events` ({"table", "user",
        "item", "time", "action"?}: a table of what contexts did, `time` a
        TIMESTAMP or microseconds), each row gets its context's last
        `history` events before the row's time (`history_items`,
        `history_times`, `history_actions`), and with `window` (a period,
        "30 minutes", or seconds) a label: the best grade (`grades`, action
        to grade; every event 1 without) of the row's item's events by the
        context in the window after it, 0 when there is none. A label
        written on the row wins. `since` and `until` keep the rows served in
        [since, until)."""
        opts = {"table": table, "events": events, "history": history, "window": window, "grades": grades, "since": _time(since), "until": _time(until)}
        return self._export_rank_log({k: v for k, v in opts.items() if v is not None})

    def _check_table(self, table: str | None, call: str) -> None:
        """Refuses a call that names no table where there are several: it
        would read or change the first table, which is seldom the one
        meant (MAYA-403: `db.search("lasagna")` on a server of products,
        library_help and recipes answered shoes, and `db.get` of a recipe
        None). With one table, it is that one."""
        if table is not None:
            return
        names = self._table_names()
        if len(names) > 1:
            raise ChloeDBError(
                f"db.{call} names no table, and this {self._holder} has {len(names)}: {', '.join(names)}. "
                f"Pass table=, as in db.{call}(..., table={names[-1]!r})"
            )

    def table(self, name: str | None = None) -> Query:
        """A query of the table named (or of the only table), built from Python
        values (`chloedb.build`):
        `db.table().where(col("year") >= 2019).order_by(bm25("body",
        q)).limit(10)`. A table named is read alone: the first table's
        schema is not, so a key limited to other tables can use it."""
        if name is None:
            self._check_table(None, "table")
            return Query(self, self.schema)
        try:
            s = self._table_schema(name)
        except ChloeDBError as e:
            if "no such table" in str(e) or getattr(e, "code", None) == "not_found":
                raise ChloeDBError(f"no table {name}") from e
            raise
        return Query(self, s)

    def search(
        self,
        text: str,
        *,
        table: str | None = None,
        limit: int = 10,
        where: Expr | str | None = None,
        columns: str | Iterable[str] = "*",
        mode: str = "auto",
        text_fields: Iterable[str] | None = None,
        vector_field: str | None = None,
        params: Mapping[str, Any] | None = None,
        min_seq_no: int | None = None,
        min_seq_nos: Sequence[int] | None = None,
        timeout_ms: int | None = None,
        memory: int | None = None,
        typos: bool = True,
    ) -> Result:
        """The rows of `table` (or of the only table) that best match
        `text`. `min_seq_no(s)` name writes the
        answer must see, and `timeout_ms` bounds it, as `query` takes them;
        a `Database` also takes `memory`, as its `query` does.

        `mode` "text" ranks by BM25 over the text fields, "vector" by the
        similarity of an embedded vector field to the text's embedding, and
        "hybrid" fuses both by reciprocal rank (RRF). "auto" is hybrid when
        the table has an embedded field whose model is open here, and text
        otherwise. The text fields are those generated from other fields if
        there are any (a combined "body"), else every indexed text field.
        `where` is a condition (`col("year") >= 2000`), or an SQL condition
        with `params` for its placeholders. A word that is in no row is
        read as a typo of the words near it (`fuzzy => 'auto'`), as a shop's
        search box reads it; `typos=False` matches words as written.
        """
        # The limits given, as this store's query and run take them: a
        # client's take timeout_ms alone (a server's queries share its
        # memory budget).
        limits = {k: v for k, v in (("timeout_ms", timeout_ms), ("memory", memory)) if v is not None}
        if memory is not None and not getattr(self, "_local", False):
            raise TypeError(f"{type(self).__name__}.search() takes no memory: the server's queries share its --query-memory-mb")
        self._check_table(table, "search")
        s = self.schema if table is None else self._table_schema(table)
        if text_fields is None:
            texts = [f for f in s["fields"] if f["type"] == "text" and f.get("indexed", True)]
            generated = [f["name"] for f in texts if f.get("from")]
            text_fields = generated or [f["name"] for f in texts]
        text_fields = list(text_fields)
        if vector_field is None:
            embedded = self._embedded(table)
            vector_field = embedded[0] if embedded else None
        if mode == "auto":
            mode = "hybrid" if vector_field and text_fields else "vector" if vector_field else "text"
        scores = []
        if mode in ("text", "hybrid"):
            if not text_fields:
                raise ChloeDBError(f"{s['table']} has no indexed text field to search")
            fz = ", fuzzy => 'auto'" if typos else ""
            scores += [f"bm25({quote(f)}, :_q{fz})" for f in text_fields]
        if mode in ("vector", "hybrid"):
            if not vector_field:
                raise ChloeDBError(f"{s['table']} has no embedded vector field with its model open")
            scores.append(f"similarity({quote(vector_field)}, :_v)")
        if mode not in ("text", "vector", "hybrid"):
            raise ValueError('mode is "auto", "text", "vector" or "hybrid"')
        if isinstance(where, str) and not where.strip():
            where = None
        if not isinstance(where, Expr):
            # SQL, as before: the names of fields quoted, so that any name
            # works; other columns (score(), an expression) as written.
            fields = {f["name"].lower() for f in s["fields"]}
            name = lambda c: quote(c) if c.lower() in fields else c
            cols = columns if isinstance(columns, str) else ", ".join(name(c.strip()) for c in columns)
            order = scores[0] if len(scores) == 1 else f"rrf({', '.join(scores)})"
            sql = f"SELECT {cols} FROM {s['table']}"
            if where is not None:
                sql += f" WHERE {where}"
            sql += f" ORDER BY {order} DESC LIMIT {int(limit)}"
            p = dict(params or {})
            if "bm25" in order:
                p["_q"] = text
            if "similarity" in order:
                p["_v"] = {"embed": text, "field": vector_field, "table": s["table"]}
            return self.query(sql, p, min_seq_no=min_seq_no, min_seq_nos=min_seq_nos, **limits)
        # A condition built in Python: the query is a plan.
        ranks = [bm25(f, text, fuzzy="auto" if typos else None) for f in text_fields] if mode in ("text", "hybrid") else []
        if mode in ("vector", "hybrid"):
            ranks.append(similarity(vector_field, self.embed(text, vector_field, table=s["table"])))
        q = Query(self, s).where(where).order_by(ranks[0] if len(ranks) == 1 else rrf(*ranks)).limit(limit)
        if columns != "*":
            names = [c.strip() for c in columns.split(",")] if isinstance(columns, str) else list(columns)
            q = q.select(*[{"score()": score(), "hops()": hops()}.get(c.lower(), c) for c in names])
        if min_seq_no is not None or min_seq_nos is not None or limits:
            return self.run(q, min_seq_no=min_seq_no, min_seq_nos=min_seq_nos, **limits)
        return q.run()

    def _ask(
        self,
        question: str,
        *,
        table: str | None = None,
        k: int = 8,
        where: Expr | str | None = None,
        params: Mapping[str, Any] | None = None,
        model: str | None = None,
        max_tokens: int = 700,
    ) -> dict:
        """`ask` of a `Database`, and of a `Client` given a table: the
        answer to `question` from the rows `search` finds for it, citing
        them.

        Returns {"answer", "sources", "model"}: the answer cites its rows as
        [1], [2], ...; `sources` are those rows in that order, each
        {"n", "key", "score", "row"}, and the answer says so when they do
        not hold it. A language model writes it through `llm()`, where the
        query runs (this process, or the server): `model`, else
        $CHLOEDB_ASK_MODEL, else "anthropic/claude-haiku-5-5", whose key is
        ANTHROPIC_API_KEY there; "openai/<model>" takes OPENAI_API_KEY."""
        import os

        hits = self.search(question, table=table, limit=k, where=where, params=params)
        s = self.schema if table is None else self._table_schema(table)
        key = s.get("key")
        sources = []
        for i, row in enumerate(hits.dicts()):
            sources.append({"n": i + 1, "key": row.get(key) if key else (hits.keys[i] if hits.keys else None), "score": hits.scores[i] if hits.scores else None, "row": row})
        if not sources:
            return {"answer": f"No row of {s['table']} matches the question, so there is nothing to answer it from.", "sources": [], "model": None}
        fields = {f["name"]: f for f in s["fields"]}

        def shown(row: dict) -> str:
            # Text first (the words the answer quotes), then short values.
            parts = []
            for name, v in row.items():
                if v is None or name == key or isinstance(v, (list, dict)) and len(str(v)) > 200:
                    continue
                text = str(v)
                parts.append(f"{name}: {text[:1500]}")
            parts.sort(key=lambda p: fields.get(p.split(":", 1)[0], {}).get("type") != "text")
            return "\n".join(parts)

        context = "\n\n".join(f"[{x['n']}]" + (f" ({key} {x['key']})" if key else "") + "\n" + shown(x["row"]) for x in sources)
        instructions = (
            "Answer the question from the numbered rows of a database below, and from nothing else. "
            "Cite each claim with the number of the row it comes from, as [1] or [2][3]. "
            "If the rows do not hold the answer, say so in one sentence and say what they do hold. "
            "Be brief: a few sentences, no preamble."
        )
        model = model or os.environ.get("CHLOEDB_ASK_MODEL") or "anthropic/claude-haiku-5-5"
        if "'" in model:
            raise ValueError(f"model {model!r} has a quote in it")
        # llm() answers for each row a query returns, and reads one of them:
        # one row of the table, whose first field it is given and ignores.
        first = quote(s["fields"][0]["name"])
        sql = f"SELECT {first}, llm('{model}', :i, :q, :c, max_tokens => {int(max_tokens)}) AS answer FROM {quote(s['table'])} LIMIT 1"
        try:
            answer = self.query(sql, {"i": instructions, "q": "Question: " + question, "c": "Rows:\n" + context}).column("answer")[0]
        except ChloeDBError as e:
            m = str(e)
            if "API_KEY" in m or "api key" in m.lower() or "x-api-key" in m.lower():
                raise ChloeDBError(
                    f"ask needs a language model to write the answer, and {model} has no key where the query runs: "
                    f"set ANTHROPIC_API_KEY there (or pass model='openai/<model>' with OPENAI_API_KEY). "
                    f"db.search({question!r}) finds the same rows without one. ({m})"
                ) from e
            raise
        return {"answer": answer, "sources": sources, "model": model}

    ask = _ask
