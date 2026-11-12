"""A table of texts, their metadata and their vectors, as the vector
stores of `chloedb.langchain` and `chloedb.llama_index` keep them.

The table has a key `id`, the text `text` (indexed for BM25, so a search
can be by words, by vector or both), the metadata `metadata` (a JSON
field, so a filter on any of its keys runs from its index) and the vector
`embedding`. The vectors come from the caller (a framework's embedding
model, made unit length for cosine similarity unless `distance="dot"`)
or from a model ChloeDB runs itself (`model=`: the field embeds `text`
when a row is written, and a query's text by the same model)."""

from __future__ import annotations

import json
from typing import Any, Mapping, Sequence

from ._adapters import Params, chunks, conditions, ensure_table, field, json_fits, json_value, table_schema, unit, where
from ._native import ChloeDBError
from ._search import quote

MODES = ("vector", "text", "hybrid")


class VectorTable:
    """One table of a store (`Database` or `Client`) holding texts and
    vectors. `extra` are fields beyond id, text, metadata and embedding
    (short-form schema fields), such as LlamaIndex's `ref_doc_id`."""

    def __init__(self, store: Any, table: str, *, dim: int | None = None, model: str | None = None, distance: str = "cosine", extra: Sequence[Mapping[str, Any]] = ()):
        if distance not in ("cosine", "dot"):
            raise ValueError("distance is 'cosine' (vectors made unit length) or 'dot' (as written)")
        self.store = store
        self.table = table
        self.distance = distance
        self._dim = dim
        self._model = model
        self._extra = [dict(f) for f in extra]
        self._schema: dict | None = table_schema(store, table)
        if self._schema is None and (model is not None or dim is not None):
            self._make(dim)
        self._check()

    # The table.

    def _make(self, dim: int | None) -> None:
        vec: dict[str, Any] = {"name": "embedding", "type": "vector"}
        if self._model is not None:
            vec["embed"] = {"from": ["text"], "model": self._model}
            if dim is not None:
                vec["dim"] = dim
        else:
            if dim is None:
                raise ValueError("the table needs the vectors' dimensions: give dim=, model=, or write the first rows")
            vec["dim"] = int(dim)
        schema = {
            "table": self.table,
            "key": "id",
            "fields": [
                {"name": "id", "type": "keyword"},
                {"name": "text", "type": "text"},
                {"name": "metadata", "type": "json"},
                *self._extra,
                vec,
            ],
        }
        self._schema = ensure_table(self.store, schema)

    def _check(self) -> None:
        if self._schema is None:
            return
        missing = [n for n in ("id", "text", "metadata", "embedding") if field(self._schema, n) is None]
        if missing:
            raise ChloeDBError(f"table {self.table!r} is not a vector store's table: it has no {', '.join(missing)} field (make another table, or name one with table=)")
        emb = field(self._schema, "embedding")
        embedded = emb.get("embed")
        if embedded and self._model is not None:
            have = embedded.get("model") if isinstance(embedded, dict) else None
            if have is not None and have != self._model:
                raise ChloeDBError(f"table {self.table!r} embeds its text with {have!r}, not {self._model!r}")

    @property
    def ready(self) -> bool:
        return self._schema is not None

    @property
    def embeds(self) -> bool:
        """Whether ChloeDB's model makes the vectors (of rows and of
        queries)."""
        if self._schema is None:
            return self._model is not None
        return bool(field(self._schema, "embedding").get("embed"))

    @property
    def dim(self) -> int | None:
        if self._schema is None:
            return self._dim
        return field(self._schema, "embedding").get("dim")

    def vector(self, v: Sequence[float]) -> list[float]:
        return unit(v) if self.distance == "cosine" else [float(x) for x in v]

    def embed(self, text: str) -> list[float]:
        """A query's vector by the table's own model."""
        return list(self.store.embed(text, field="embedding", table=self.table))

    # Writes.

    def upsert(self, rows: Sequence[dict]) -> None:
        """Writes rows of id, text, metadata (a dict), embedding (a vector,
        or None for the table's model to make it) and the extra fields."""
        if not rows:
            return
        if self._schema is None:
            v = next((r.get("embedding") for r in rows if r.get("embedding") is not None), None)
            if v is None and self._model is None:
                raise ValueError("no vectors to write, and no model to make them: give an embedding model or model=")
            self._make(None if v is None else len(v))
        out = []
        for r in rows:
            row = {k: v for k, v in r.items() if k != "metadata" and k != "embedding"}
            meta = json_value(r.get("metadata") or {})
            if not isinstance(meta, dict):
                raise ValueError(f"metadata is a dict, not {type(meta).__name__}")
            if meta:
                if not json_fits(meta):
                    raise ValueError(f"the metadata of {r['id']!r} is past what a JSON field takes (16 levels, 1,000 values, paths of 256 bytes, no empty keys)")
                row["metadata"] = meta
            else:
                row["metadata"] = None
            if r.get("embedding") is not None:
                row["embedding"] = self.vector(r["embedding"])
            out.append(row)
        for part in chunks(out):
            self.store.upsert(list(part), table=self.table)

    def delete(self, ids: Sequence[str]) -> int:
        if self._schema is None or not ids:
            return 0
        n = 0
        for part in chunks(list(ids)):
            n += int(self.store.delete(list(part), table=self.table))
        return n

    def delete_where(self, cond: str, params: Mapping[str, Any]) -> int:
        if self._schema is None:
            return 0
        return int(self.store.sql(f"DELETE FROM {quote(self.table)} WHERE {cond}", dict(params)))

    def clear(self) -> None:
        if self._schema is not None:
            self.store.sql(f"DELETE FROM {quote(self.table)}")

    # Reads.

    def get(self, ids: Sequence[str]) -> list[dict]:
        """The rows of `ids` that exist, in that order."""
        if self._schema is None or not ids:
            return []
        out = []
        for part in chunks(list(ids)):
            out.extend(r for r in self.store.get_many(list(part), table=self.table) if r is not None)
        return [self._row(r) for r in out]

    def select(self, cond: str | None, params: Mapping[str, Any], limit: int | None = None) -> list[dict]:
        if self._schema is None:
            return []
        sql = f"SELECT * FROM {quote(self.table)} {conditions(cond)}"
        if limit is not None:
            sql += f" LIMIT {int(limit)}"
        return [self._row(r) for r in self.store.query(sql, dict(params))]

    def vectors(self, ids: Sequence[str]) -> dict[str, list[float]]:
        """The stored vectors of `ids` (for maximal marginal relevance)."""
        if self._schema is None or not ids:
            return {}
        out: dict[str, list[float]] = {}
        for part in chunks(list(ids)):
            p = Params()
            listed = ", ".join(p(i) for i in part)
            r = self.store.query(f"SELECT id, embedding FROM {quote(self.table)} WHERE id IN ({listed})", p.values)
            for row in r:
                if row.get("embedding") is not None:
                    out[row["id"]] = [float(x) for x in row["embedding"]]
        return out

    def search(self, k: int, *, mode: str = "vector", vector: Sequence[float] | None = None, text: str | None = None, cond: str | None = None, params: Mapping[str, Any] | None = None) -> list[tuple[dict, float]]:
        """The `k` best rows by `mode`: "vector" (similarity to `vector`),
        "text" (BM25 of `text`) or "hybrid" (both, fused by reciprocal
        rank), among the rows `cond` keeps; with each row its score."""
        if mode not in MODES:
            raise ValueError(f"mode is one of {', '.join(MODES)}, not {mode!r}")
        if self._schema is None or k <= 0:
            return []
        params = dict(params or {})
        scores = []
        if mode in ("vector", "hybrid"):
            if vector is None:
                raise ValueError(f"a {mode} search needs a query vector")
            params["_qv"] = self.vector(vector)
            scores.append("similarity(embedding, :_qv)")
        if mode in ("text", "hybrid"):
            if not text:
                raise ValueError(f"a {mode} search needs the query's text")
            params["_qt"] = text
            scores.append("bm25(text, :_qt, syntax => 'plain')")
        rank = scores[0] if len(scores) == 1 else f"rrf({', '.join(scores)})"
        sql = f"SELECT *, score() FROM {quote(self.table)} {conditions(cond)} ORDER BY {rank} DESC LIMIT {int(k)}"
        r = self.store.query(sql, params)
        out = []
        for row, score in zip(r, r.scores):
            row = dict(row)
            row.pop("score()", None)
            out.append((self._row(row), float(score) if score is not None else 0.0))
        return out

    @staticmethod
    def _row(r: Mapping[str, Any]) -> dict:
        row = dict(r)
        meta = row.get("metadata")
        if isinstance(meta, str):
            meta = json.loads(meta)
        row["metadata"] = meta or {}
        return row


def metadata_where(filter: Mapping[str, Any] | None, p: Params) -> str | None:
    """A MongoDB-style filter on the metadata."""
    return where(filter, "metadata", p)
