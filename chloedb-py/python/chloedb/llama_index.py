"""ChloeDB as a LlamaIndex vector store (`pip install "chloedb[llama-index]"`).

`ChloeDBVectorStore` keeps nodes in a table of a ChloeDB database (a file
in this process, or a chloedb-server): their text, indexed for BM25, their
metadata, as a JSON field whose keys filter from its index, and their
vectors. A query ranks by vector (the default mode), by words
(`TEXT_SEARCH`) or by both fused by reciprocal rank (`HYBRID`)::

    from llama_index.core import StorageContext, VectorStoreIndex
    from chloedb.llama_index import ChloeDBVectorStore

    store = ChloeDBVectorStore("docs.chloedb")
    index = VectorStoreIndex.from_documents(
        documents, storage_context=StorageContext.from_defaults(vector_store=store))
    index.as_query_engine(vector_store_query_mode="hybrid").query("...")

Nodes carry the vectors LlamaIndex's embedding model made. With
`model="potion-retrieval-32M"` the table's own model embeds the nodes
that come without one, and every query's text: pass
`embed_model=store.embed_model` to the index for the nodes to be
embedded by it too, with no model of LlamaIndex's.
"""

from __future__ import annotations

import json
import os
from typing import Any, List, Optional, Sequence

try:
    from llama_index.core.base.embeddings.base import BaseEmbedding
    from llama_index.core.bridge.pydantic import PrivateAttr
    from llama_index.core.schema import BaseNode, MetadataMode, TextNode
    from llama_index.core.vector_stores.types import (
        BasePydanticVectorStore,
        FilterCondition,
        FilterOperator,
        MetadataFilter,
        MetadataFilters,
        VectorStoreQuery,
        VectorStoreQueryMode,
        VectorStoreQueryResult,
    )
    from llama_index.core.vector_stores.utils import metadata_dict_to_node, node_to_metadata_dict
except ImportError as e:  # pragma: no cover - the extra is not installed
    raise ImportError('chloedb.llama_index needs llama-index-core: pip install "chloedb[llama-index]"') from e

from ._adapters import Params, conditions, open_store, path
from ._vectors import VectorTable

__all__ = ["ChloeDBVectorStore", "ChloeDBEmbedding"]

# The node's own fields, which `node_to_metadata_dict` adds to its
# metadata: kept in the `node` column, not among the metadata to filter.
_NODE_KEYS = ("_node_content", "_node_type", "document_id", "doc_id", "ref_doc_id")


def _filter(f: MetadataFilter, p: Params) -> str:
    col = path("metadata", [f.key])
    op, v = f.operator, f.value

    def scalar(x: Any) -> str:
        if x is True:
            return "TRUE"
        if x is False:
            return "FALSE"
        return p(x)

    def listed(xs: Any) -> list:
        return list(xs) if isinstance(xs, (list, tuple)) else [xs]

    if op == FilterOperator.EQ:
        return f"{col} IS NULL" if v is None else f"{col} = {scalar(v)}"
    if op == FilterOperator.NE:
        return f"{col} IS NOT NULL" if v is None else f"({col} != {scalar(v)} OR {col} IS NULL)"
    if op in (FilterOperator.GT, FilterOperator.GTE, FilterOperator.LT, FilterOperator.LTE):
        return f"{col} {op.value} {scalar(v)}"
    if op in (FilterOperator.IN, FilterOperator.ANY):
        xs = listed(v)
        return f"{col} IN ({', '.join(scalar(x) for x in xs)})" if xs else "FALSE"
    if op == FilterOperator.NIN:
        xs = listed(v)
        return f"({col} NOT IN ({', '.join(scalar(x) for x in xs)}) OR {col} IS NULL)" if xs else "TRUE"
    if op == FilterOperator.ALL:
        xs = listed(v)
        return "(" + " AND ".join(f"{col} = {scalar(x)}" for x in xs) + ")" if xs else "TRUE"
    if op == FilterOperator.CONTAINS:
        return f"{col} = {scalar(v)}"
    if op == FilterOperator.IS_EMPTY:
        return f"{col} IS NULL"
    raise ValueError(f"ChloeDB's vector store does not filter by {op.value!r}: it filters by ==, !=, >, >=, <, <=, in, nin, any, all, contains and is_empty")


def _filters(fs: MetadataFilters | None, p: Params) -> str | None:
    if fs is None or not fs.filters:
        return None
    parts = [(_filters(f, p) if isinstance(f, MetadataFilters) else _filter(f, p)) for f in fs.filters]
    parts = [f"({x})" for x in parts if x]
    if not parts:
        return None
    cond = fs.condition or FilterCondition.AND
    if cond == FilterCondition.NOT:
        return "NOT (" + " AND ".join(parts) + ")"
    return (" OR " if cond == FilterCondition.OR else " AND ").join(parts)


class ChloeDBVectorStore(BasePydanticVectorStore):
    """A LlamaIndex vector store over a ChloeDB table.

    Args:
        db: a `chloedb.Database` or `chloedb.Client`, a database's path
            (opened, or made at the first write), or a chloedb-server's
            `http(s)://` URL.
        table: the table (made when missing).
        model: a model ChloeDB embeds with (such as
            "potion-retrieval-32M"): the nodes written without a vector,
            and the text of queries; `embed_model` is it as LlamaIndex's
            embedding model.
        dim: the vectors' dimensions, to make the table before the first
            write (else it is made then, by the first vector's length).
        distance: "cosine" (the default: vectors are made unit length) or
            "dot" (as the model writes them).
        models: where a database in this process finds its models.
        token: a server's token (or $CHLOEDB_TOKEN).
    """

    stores_text: bool = True
    flat_metadata: bool = False
    is_embedding_query: bool = True
    table: str = "llama_index"

    _store: Any = PrivateAttr()
    _vt: VectorTable = PrivateAttr()

    def __init__(self, db: Any = "llama_index.chloedb", *, table: str = "llama_index", model: Optional[str] = None, dim: Optional[int] = None, distance: str = "cosine", models: Optional[str | os.PathLike] = None, token: Optional[str] = None, **kwargs: Any):
        super().__init__(table=table, **kwargs)
        self._store = open_store(db, models=models, token=token)
        extra = [{"name": "ref_doc_id", "type": "keyword"}, {"name": "node", "type": "text", "indexed": False}]
        self._vt = VectorTable(self._store, table, dim=dim, model=model, distance=distance, extra=extra)
        self.is_embedding_query = not self._vt.embeds

    @classmethod
    def class_name(cls) -> str:
        return "ChloeDBVectorStore"

    @property
    def client(self) -> Any:
        """The `chloedb.Database` or `chloedb.Client`."""
        return self._store

    @property
    def embed_model(self) -> "ChloeDBEmbedding":
        """The table's own model as LlamaIndex's embedding model."""
        if not self._vt.embeds:
            raise ValueError("this store's table embeds nothing itself: make it with model=")
        return ChloeDBEmbedding(self)

    # Writing.

    def add(self, nodes: Sequence[BaseNode], **add_kwargs: Any) -> List[str]:
        rows = []
        for node in nodes:
            meta = node_to_metadata_dict(node, remove_text=True, flat_metadata=False)
            node_json = {k: meta.pop(k) for k in _NODE_KEYS if k in meta}
            rows.append(
                {
                    "id": node.node_id,
                    "text": node.get_content(metadata_mode=MetadataMode.NONE),
                    "metadata": meta,
                    "ref_doc_id": node.ref_doc_id,
                    "node": json.dumps(node_json, ensure_ascii=False),
                    "embedding": node.embedding,
                }
            )
        self._vt.upsert(rows)
        self.is_embedding_query = not self._vt.embeds
        return [r["id"] for r in rows]

    def delete(self, ref_doc_id: str, **delete_kwargs: Any) -> None:
        """Deletes the nodes of document `ref_doc_id`."""
        self._vt.delete_where("ref_doc_id = :r", {"r": ref_doc_id})

    def _where(self, node_ids: Optional[List[str]], filters: Optional[MetadataFilters], p: Params, doc_ids: Optional[List[str]] = None) -> str | None:
        # An empty list of ids, as None, limits nothing (as LlamaIndex's
        # retrievers pass them).
        conds = [_filters(filters, p)]
        if node_ids:
            conds.append("id IN (" + ", ".join(p(str(i)) for i in node_ids) + ")")
        if doc_ids:
            conds.append("ref_doc_id IN (" + ", ".join(p(str(i)) for i in doc_ids) + ")")
        kept = [f"({c})" for c in conds if c]
        return " AND ".join(kept) if kept else None

    def delete_nodes(self, node_ids: Optional[List[str]] = None, filters: Optional[MetadataFilters] = None, **delete_kwargs: Any) -> None:
        if not node_ids and filters is None:
            return
        if node_ids and filters is None:
            self._vt.delete([str(i) for i in node_ids])
            return
        p = Params()
        cond = self._where(node_ids, filters, p)
        self._vt.delete_where(cond or "TRUE", p.values)

    def clear(self) -> None:
        self._vt.clear()

    # Reading.

    def get_nodes(self, node_ids: Optional[List[str]] = None, filters: Optional[MetadataFilters] = None, **kwargs: Any) -> List[BaseNode]:
        if node_ids and filters is None:
            return [self._node(r) for r in self._vt.get([str(i) for i in node_ids])]
        p = Params()
        cond = self._where(node_ids, filters, p)
        out = []
        if not self._vt.ready:
            return out
        from ._adapters import rows_of
        from ._search import quote

        for r in rows_of(self._store, f"SELECT * FROM {quote(self.table)} {conditions(cond)}", p.values):
            out.append(self._node(self._vt._row(r)))
        return out

    def query(self, query: VectorStoreQuery, **kwargs: Any) -> VectorStoreQueryResult:
        mode = query.mode
        if mode in (VectorStoreQueryMode.TEXT_SEARCH, VectorStoreQueryMode.SPARSE):
            how = "text"
        elif mode == VectorStoreQueryMode.HYBRID:
            how = "hybrid"
        elif mode == VectorStoreQueryMode.DEFAULT:
            how = "vector"
        else:
            raise ValueError(f"ChloeDB's vector store queries in the modes default, text_search, sparse and hybrid, not {mode.value!r}")
        vector = query.query_embedding
        if how != "text" and vector is None:
            if not self._vt.embeds or not query.query_str:
                raise ValueError("the query has no embedding (and the table no model to make one from its text)")
            vector = self._vt.embed(query.query_str)
        if how != "vector" and not query.query_str:
            raise ValueError(f"a {how} query needs its text (query_str)")
        p = Params()
        cond = self._where(query.node_ids, query.filters, p, query.doc_ids)
        k = query.similarity_top_k
        if how == "hybrid" and query.hybrid_top_k:
            k = query.hybrid_top_k
        elif how == "text" and query.sparse_top_k:
            k = query.sparse_top_k
        found = self._vt.search(k, mode=how, vector=vector, text=query.query_str, cond=cond, params=p.values)
        return VectorStoreQueryResult(
            nodes=[self._node(r) for r, _ in found],
            similarities=[s for _, s in found],
            ids=[r["id"] for r, _ in found],
        )

    def _node(self, row: dict) -> BaseNode:
        meta = dict(row.get("metadata") or {})
        node_meta = json.loads(row["node"]) if row.get("node") else {}
        if "_node_content" in node_meta:
            node = metadata_dict_to_node({**meta, **node_meta}, text=row.get("text") or "")
            node.metadata = meta
        else:
            node = TextNode(id_=row["id"], text=row.get("text") or "", metadata=meta)
        return node


class ChloeDBEmbedding(BaseEmbedding):
    """A ChloeDB table's own embedding model (its `embedding` field's) as
    LlamaIndex's: `store.embed_model`."""

    _vt: VectorTable = PrivateAttr()

    def __init__(self, store: ChloeDBVectorStore, **kwargs: Any):
        kwargs.setdefault("model_name", "chloedb")
        kwargs.setdefault("embed_batch_size", 64)
        super().__init__(**kwargs)
        self._vt = store._vt

    @classmethod
    def class_name(cls) -> str:
        return "ChloeDBEmbedding"

    def _get_query_embedding(self, query: str) -> List[float]:
        return self._vt.embed(query)

    async def _aget_query_embedding(self, query: str) -> List[float]:
        import asyncio

        return await asyncio.to_thread(self._vt.embed, query)

    def _get_text_embedding(self, text: str) -> List[float]:
        return self._vt.embed(text)
