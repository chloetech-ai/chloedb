"""ChloeDB as a LangChain vector store (`pip install "chloedb[langchain]"`).

`ChloeDBVectorStore` keeps documents in a table of a ChloeDB database (a
file in this process, or a chloedb-server): their text, indexed for BM25,
their metadata, as a JSON field whose keys filter from its index, and
their vectors. A search ranks by vector, by words, or by both fused
(`mode="hybrid"`), so one store answers a semantic query and a query of
exact names and codes::

    from chloedb.langchain import ChloeDBVectorStore
    from langchain_openai import OpenAIEmbeddings

    store = ChloeDBVectorStore("docs.chloedb", embedding=OpenAIEmbeddings())
    store.add_texts(["ChloeDB keeps text and vectors", "LangChain chains calls"],
                    metadatas=[{"topic": "db"}, {"topic": "llm"}])
    store.similarity_search("what stores vectors?", k=1, filter={"topic": "db"})
    retriever = store.as_retriever(search_kwargs={"k": 4, "mode": "hybrid"})

Without an embedding model, ChloeDB embeds the texts itself with a model
it runs in the process or the server (`model="potion-retrieval-32M"`, the
default when `embedding` is None)::

    store = ChloeDBVectorStore("docs.chloedb", model="potion-retrieval-32M")

Filters are written as MongoDB's (and LangChain's other stores') are:
`{"topic": "db"}`, `{"year": {"$gte": 2020}}`, `{"$or": [{...}, {...}]}`,
with `$eq`, `$ne`, `$gt`, `$gte`, `$lt`, `$lte`, `$in`, `$nin`,
`$exists`, `$between`, `$like` (a prefix, `"abc%"`), `$contains`, `$and`,
`$or` and `$not`.
"""

from __future__ import annotations

import os
import uuid
from typing import Any, Callable, Iterable, Sequence

try:
    from langchain_core.documents import Document
    from langchain_core.embeddings import Embeddings
    from langchain_core.vectorstores import VectorStore
except ImportError as e:  # pragma: no cover - the extra is not installed
    raise ImportError('chloedb.langchain needs langchain-core: pip install "chloedb[langchain]"') from e

from ._adapters import Params, open_store, table_schema
from ._vectors import MODES, VectorTable, metadata_where

__all__ = ["ChloeDBVectorStore"]

DEFAULT_MODEL = "potion-retrieval-32M"

# Reciprocal rank fusion's constant (ChloeDB's rrf(), k = 60): a document
# first in both rankings of a hybrid search scores 2 / (60 + 1).
_RRF_K = 60
_RRF_BEST = 2.0 / (_RRF_K + 1)


def _relevance_fn(mode: str) -> Callable[[float], float]:
    """How a score of a search in `mode` reads as relevance from 0 to 1.

    By vector, the score is the cosine similarity (vectors are unit
    length), the relevance Chroma, FAISS and PGVector report for cosine:
    the similarity itself, negative ones as 0. Hybrid, the fused score
    over its best (first in both rankings). By words, BM25 has no scale
    to map to 0 to 1, so a relevance threshold is refused there."""
    if mode == "vector":
        return lambda s: max(0.0, min(1.0, float(s)))
    if mode == "hybrid":
        return lambda s: max(0.0, min(1.0, float(s) / _RRF_BEST))
    raise NotImplementedError("a search by words (mode='text') ranks by BM25, which has no scale of 0 to 1: use similarity_search_with_score, or mode='vector' or 'hybrid' for relevance scores")



class ChloeDBVectorStore(VectorStore):
    """A LangChain `VectorStore` over a ChloeDB table.

    Args:
        db: a `chloedb.Database` or `chloedb.Client`, a database's path
            (opened, or made at the first write), or a chloedb-server's
            `http(s)://` URL.
        table: the table (made when missing).
        embedding: a LangChain `Embeddings`; None for ChloeDB to embed the
            texts itself with `model`.
        model: the model ChloeDB embeds with when `embedding` is None
            (default "potion-retrieval-32M"; any model a vector field
            takes, `openai/text-embedding-3-small` included).
        dim: the vectors' dimensions, to make the table before the first
            write (else it is made then, by the first vector's length).
        distance: "cosine" (the default: vectors are made unit length) or
            "dot" (as the model writes them).
        mode: how a search ranks unless it says: "vector", "text" (BM25)
            or "hybrid" (both, fused by reciprocal rank).
        models: where a database in this process finds its models.
        token: a server's token (or $CHLOEDB_TOKEN).
    """

    def __init__(
        self,
        db: Any = "langchain.chloedb",
        *,
        table: str = "langchain",
        embedding: Embeddings | None = None,
        model: str | None = None,
        dim: int | None = None,
        distance: str = "cosine",
        mode: str = "vector",
        models: str | os.PathLike | None = None,
        token: str | None = None,
    ):
        if mode not in MODES:
            raise ValueError(f"mode is one of {', '.join(MODES)}, not {mode!r}")
        if embedding is not None and model is not None:
            raise ValueError("give embedding= (LangChain embeds) or model= (ChloeDB embeds), not both")
        self.store = open_store(db, models=models, token=token)
        self._embedding = embedding
        self.mode = mode
        if embedding is None and model is None and table_schema(self.store, table) is None:
            model = DEFAULT_MODEL
        self._table = VectorTable(self.store, table, dim=dim, model=model, distance=distance)
        if embedding is None and not self._table.embeds:
            raise ValueError(f"table {table!r} holds vectors made outside ChloeDB: pass the embedding= that made them")
        self.table = table

    @property
    def embeddings(self) -> Embeddings | None:
        return self._embedding

    # Writing.

    def add_texts(self, texts: Iterable[str], metadatas: list[dict] | None = None, *, ids: list[str] | None = None, **kwargs: Any) -> list[str]:
        """Writes texts (replacing the ones of the same ids) and returns
        their ids, new ones made for those without."""
        texts = list(texts)
        if metadatas is not None and len(metadatas) != len(texts):
            raise ValueError(f"{len(texts)} texts and {len(metadatas)} metadatas")
        if ids is not None and len(ids) != len(texts):
            raise ValueError(f"{len(texts)} texts and {len(ids)} ids")
        ids = [str(i) if i is not None else uuid.uuid4().hex for i in (ids or [None] * len(texts))]
        vectors: list[Any] = [None] * len(texts)
        if self._embedding is not None and texts:
            vectors = self._embedding.embed_documents(texts)
        self._table.upsert([{"id": i, "text": t, "metadata": (metadatas or [None] * len(texts))[n], "embedding": vectors[n]} for n, (i, t) in enumerate(zip(ids, texts))])
        return ids

    def add_documents(self, documents: list[Document], **kwargs: Any) -> list[str]:
        ids = kwargs.pop("ids", None)
        if ids is None:
            ids = [d.id for d in documents]
            if all(i is None for i in ids):
                ids = None
        return self.add_texts([d.page_content for d in documents], [d.metadata for d in documents], ids=ids, **kwargs)

    def delete(self, ids: list[str] | None = None, *, filter: dict | None = None, **kwargs: Any) -> bool | None:
        """Deletes the documents of `ids`, or those `filter` keeps (both:
        those of `ids` that it keeps)."""
        if ids is None and filter is None:
            raise ValueError("delete takes ids= or filter= (filter={} deletes every document)")
        if filter is not None:
            p = Params()
            conds = [metadata_where(filter, p)]
            if ids is not None:
                conds.append("id IN (" + ", ".join(p(str(i)) for i in ids) + ")" if ids else "FALSE")
            cond = " AND ".join(f"({c})" for c in conds if c) or "TRUE"
            self._table.delete_where(cond, p.values)
            return True
        self._table.delete([str(i) for i in ids])
        return True

    def get_by_ids(self, ids: Sequence[str], /) -> list[Document]:
        return [self._document(r) for r in self._table.get([str(i) for i in ids])]

    # Searching.

    def _query_vector(self, query: str) -> list[float]:
        if self._embedding is not None:
            return self._embedding.embed_query(query)
        return self._table.embed(query)

    def _search(self, query: str | None, k: int, *, vector: Sequence[float] | None = None, filter: dict | None = None, mode: str | None = None) -> list[tuple[Document, float]]:
        mode = mode or self.mode
        if mode == "text" and query is None:
            raise ValueError("a text search needs the query's text")
        if vector is None and mode != "text":
            vector = self._query_vector(query)
        p = Params()
        cond = metadata_where(filter, p)
        found = self._table.search(k, mode=mode if query is not None else "vector", vector=vector, text=query, cond=cond, params=p.values)
        return [(self._document(r), s) for r, s in found]

    def similarity_search(self, query: str, k: int = 4, *, filter: dict | None = None, mode: str | None = None, **kwargs: Any) -> list[Document]:
        """The `k` documents most like `query` (by `mode`), among those
        `filter` keeps."""
        return [d for d, _ in self._search(query, k, filter=filter, mode=mode)]

    def similarity_search_with_score(self, query: str, k: int = 4, *, filter: dict | None = None, mode: str | None = None, **kwargs: Any) -> list[tuple[Document, float]]:
        """As `similarity_search`, with each document's score, higher for
        closer: the cosine similarity (the dot product with distance="dot"),
        the BM25 score, or the fused reciprocal-rank score."""
        return self._search(query, k, filter=filter, mode=mode)

    def similarity_search_by_vector(self, embedding: list[float], k: int = 4, *, filter: dict | None = None, **kwargs: Any) -> list[Document]:
        return [d for d, _ in self._search(None, k, vector=embedding, filter=filter, mode="vector")]

    def similarity_search_with_score_by_vector(self, embedding: list[float], k: int = 4, *, filter: dict | None = None, **kwargs: Any) -> list[tuple[Document, float]]:
        return self._search(None, k, vector=embedding, filter=filter, mode="vector")

    def _select_relevance_score_fn(self) -> Callable[[float], float]:
        """Scores as relevance from 0 to 1, for a search in the store's own
        `mode` (a search given `mode=` maps by that mode instead)."""
        return _relevance_fn(self.mode)

    def _similarity_search_with_relevance_scores(self, query: str, k: int = 4, **kwargs: Any) -> list[tuple[Document, float]]:
        relevance = _relevance_fn(kwargs.get("mode") or self.mode)
        return [(d, relevance(s)) for d, s in self.similarity_search_with_score(query, k, **kwargs)]

    async def _asimilarity_search_with_relevance_scores(self, query: str, k: int = 4, **kwargs: Any) -> list[tuple[Document, float]]:
        relevance = _relevance_fn(kwargs.get("mode") or self.mode)
        return [(d, relevance(s)) for d, s in await self.asimilarity_search_with_score(query, k, **kwargs)]

    def max_marginal_relevance_search(self, query: str, k: int = 4, fetch_k: int = 20, lambda_mult: float = 0.5, *, filter: dict | None = None, **kwargs: Any) -> list[Document]:
        return self.max_marginal_relevance_search_by_vector(self._query_vector(query), k, fetch_k, lambda_mult, filter=filter, **kwargs)

    def max_marginal_relevance_search_by_vector(self, embedding: list[float], k: int = 4, fetch_k: int = 20, lambda_mult: float = 0.5, *, filter: dict | None = None, **kwargs: Any) -> list[Document]:
        """The `k` of the `fetch_k` nearest documents that are near the
        query and far from each other."""
        import numpy as np
        from langchain_core.vectorstores.utils import maximal_marginal_relevance

        found = self._search(None, fetch_k, vector=embedding, filter=filter, mode="vector")
        if not found:
            return []
        vectors = self._table.vectors([d.id for d, _ in found])
        kept = [(d, vectors[d.id]) for d, _ in found if d.id in vectors]
        chosen = maximal_marginal_relevance(np.array(self._table.vector(embedding), dtype=np.float32), [v for _, v in kept], lambda_mult=lambda_mult, k=k)
        return [kept[i][0] for i in chosen]

    def _document(self, row: dict) -> Document:
        return Document(id=row["id"], page_content=row.get("text") or "", metadata=row.get("metadata") or {})

    @classmethod
    def from_texts(cls, texts: list[str], embedding: Embeddings | None = None, metadatas: list[dict] | None = None, *, ids: list[str] | None = None, **kwargs: Any) -> "ChloeDBVectorStore":
        """A store (`kwargs` as the constructor takes them) holding
        `texts`."""
        store = cls(embedding=embedding, **kwargs)
        store.add_texts(texts, metadatas, ids=ids)
        return store
