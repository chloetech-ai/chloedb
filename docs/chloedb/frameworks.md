# ChloeDB in LangChain, LangGraph, LlamaIndex and DB-API tools

The [Python package](python.md) plugs ChloeDB into the frameworks agents and retrieval pipelines are built with, and into anything that takes a Python DB-API connection:

<!-- since 0.2.1 --> These modules and the extras below are new in 0.2.1, which is not released yet: release 0.2.0's wheel has none of them (`import chloedb.langchain` fails there, and pip warns that it has no extra `langchain`). Until 0.2.1 is released, build it from the repository, which needs Rust and a C compiler (several minutes):

```sh
pip install "chloedb[langchain,langgraph,llama-index] @ git+https://github.com/chloetech-ai/chloedb#subdirectory=chloedb-py"
```

Once it is released, `pip install --upgrade "chloedb[langchain]" -f https://github.com/chloetech-ai/chloedb/releases/expanded_assets/chloedb-py-v0.2.1` (and, when ChloeDB is on PyPI, a plain `pip install "chloedb[langchain]"`) installs it prebuilt.

| module | what it is | install |
|---|---|---|
| `chloedb.langchain` <!-- since 0.2.1 --> | `ChloeDBVectorStore`, a LangChain vector store (and, through `as_retriever()`, a retriever) | `pip install "chloedb[langchain]"` |
| `chloedb.langgraph` <!-- since 0.2.1 --> | `ChloeDBSaver`, a LangGraph checkpointer, and `ChloeDBStore`, a LangGraph store for long-term memory | `pip install "chloedb[langgraph]"` |
| `chloedb.llama_index` <!-- since 0.2.1 --> | `ChloeDBVectorStore`, a LlamaIndex vector store, and `ChloeDBEmbedding` | `pip install "chloedb[llama-index]"` |
| `chloedb.dbapi` <!-- since 0.2.1 --> | a [PEP 249](https://peps.python.org/pep-0249/) driver: `connect()`, cursors, `pandas.read_sql` | nothing more |

Each of them takes where the data is in the same way: a database's path (opened in the process, and made at the first write when it does not exist), the `http://` or `https://` URL of a [chloedb-server](server.md) or a coordinator (with `token=`, or `$CHLOEDB_TOKEN`), or a `chloedb.Database` or `chloedb.Client` already open. Adapters given the same path in one process share one open database (a process opens a database once to write it), so a checkpointer and a store can both name `"agent.chloedb"`; a database opened by `chloedb.open` is passed as itself. The same code runs on a file on a laptop and on a cluster, and one database can hold an agent's vectors, checkpoints, memories and its other tables side by side, each in a table of its own.

## LangChain

`ChloeDBVectorStore` keeps each document as a row: its id, its text (a `TEXT` field, indexed for BM25), its metadata (a [`JSON` field](query-language.md#json-fields-keys-no-schema-names), so a filter on any key runs from the field's index) and its vector. A search ranks by vector, by words, or by both fused by [reciprocal rank](query-language.md#rrfs1-s2--reciprocal-rank-fusion), so one store answers a question asked in other words and a query of exact names, codes and error messages, which vectors miss.

```python
from chloedb.langchain import ChloeDBVectorStore
from langchain_openai import OpenAIEmbeddings

store = ChloeDBVectorStore("docs.chloedb", embedding=OpenAIEmbeddings())
store.add_texts(
    ["ChloeDB keeps text and vectors in one file", "LangGraph runs agents as graphs"],
    metadatas=[{"topic": "db", "year": 2026}, {"topic": "agents", "year": 2025}],
    ids=["a", "b"],
)
store.similarity_search("where do vectors live?", k=1)
store.similarity_search("ChloeDB", k=4, mode="hybrid", filter={"year": {"$gte": 2026}})
retriever = store.as_retriever(search_kwargs={"k": 4, "mode": "hybrid"})
```

Without an embedding model, ChloeDB embeds the texts itself, when they are written, and each query's text, with a model of a [vector field](server.md#embedded-vector-fields): `potion-retrieval-32M` by default, any model a field takes otherwise (a hosted one too, such as `openai/text-embedding-3-small`). Nothing then embeds in Python:

```python
store = ChloeDBVectorStore("docs.chloedb", model="potion-retrieval-32M")
```

| argument | default | what it does |
|---|---|---|
| `db` | `"langchain.chloedb"` | where: a path, a server's URL, a `Database` or a `Client` |
| `table` | `"langchain"` | the table, made when missing |
| `embedding` | None | a LangChain `Embeddings`; None for ChloeDB to embed with `model` |
| `model` | `"potion-retrieval-32M"` when `embedding` is None | the model ChloeDB embeds with |
| `dim` | the first vector's length | the vectors' dimensions, to make the table before the first write |
| `distance` | `"cosine"` | `"cosine"` makes vectors unit length, so the score is the cosine similarity; `"dot"` keeps them as the model writes them |
| `mode` | `"vector"` | how a search ranks unless it says: `"vector"`, `"text"` (BM25) or `"hybrid"` |

The store has LangChain's methods: `add_texts`, `add_documents` (a document's `id` is kept), `get_by_ids`, `delete(ids)` or `delete(filter=...)`, `similarity_search`, `similarity_search_with_score` (higher is closer: the cosine similarity, the BM25 score or the fused score), `similarity_search_with_relevance_scores`, `similarity_search_by_vector`, `max_marginal_relevance_search`, `from_texts`, `as_retriever` and the async forms of each. A text written again under its id replaces the one before.

### Filters

Filters are written as MongoDB's, as LangChain's other stores take them, and each runs as a condition on the metadata's JSON field, from its index:

| filter | keeps the documents whose metadata |
|---|---|
| `{"topic": "db"}` | has `topic` equal to `"db"` (a list holds the value when one of its values is it) |
| `{"author": {"name": "Ann"}}` | has `name` equal to `"Ann"` in the object at `author` |
| `{"year": {"$gte": 2020, "$lt": 2026}}` | compares: `$eq`, `$ne`, `$gt`, `$gte`, `$lt`, `$lte`, `$between` (`[lo, hi]`) |
| `{"topic": {"$in": ["db", "ml"]}}` | is one of the values; `$nin` none of them |
| `{"score": {"$exists": True}}` | has a value there (`False`: has none) |
| `{"name": {"$like": "chl%"}}` | starts with the text before `%` |
| `{"tags": {"$contains": "x"}}` | holds the value in a list |
| `{"$or": [{...}, {...}]}`, `{"$and": [...]}`, `{"$not": {...}}` | combines filters |

`$ne` and `$nin` keep documents without the key, as MongoDB's do. A key is one level whatever it holds (`{"a.b": 1}` is the key `a.b`, not `b` inside `a`). Metadata is kept as JSON: a value that JSON cannot hold is kept as its text, and an object deeper than 16 levels, with more than 1,000 values or with a path longer than 256 bytes is refused, as a [JSON field](query-language.md#json-fields-keys-no-schema-names) refuses it.

## LangGraph

### The checkpointer

`ChloeDBSaver` keeps a graph's checkpoints, and the writes pending on each, as rows of two tables (`langgraph_checkpoints` and `langgraph_writes`, made when missing), so that a thread goes on after a restart, from another process, or, through a server, from another machine. It has the methods LangGraph calls, synchronous and async alike (the async ones run the same calls on a worker thread): `get_tuple`, `list` (with `filter` on the metadata, `before` and `limit`), `put`, `put_writes`, `delete_thread` and `copy_thread`.

```python
from chloedb.langgraph import ChloeDBSaver

graph = builder.compile(checkpointer=ChloeDBSaver("agent.chloedb"))
config = {"configurable": {"thread_id": "t1"}}
graph.invoke({"messages": [("user", "hi")]}, config)
graph.get_state(config)
list(graph.get_state_history(config))          # time travel
```

Interrupts, resumes (`Command(resume=...)`), forks from an old checkpoint and subgraphs work as with LangGraph's own savers. Checkpoints and writes are written by the graph's serializer (LangGraph's `JsonPlusSerializer` unless `serde=` says otherwise), as base64 text in fields that are stored and not indexed; the metadata is also kept in a JSON field, so that `list(filter={"source": "input"})` runs from its index.

### The store

`ChloeDBStore` is LangGraph's `BaseStore`, the long-term memory a graph's nodes read and write through `store=`: items by namespace and key, in one table (`langgraph_store`).

```python
from chloedb.langgraph import ChloeDBStore

store = ChloeDBStore("agent.chloedb", index={"model": "potion-retrieval-32M", "fields": ["text"]})
store.put(("users", "u1", "memories"), "m1", {"text": "prefers dark roast coffee", "kind": "preference"})
store.search(("users", "u1"), query="what does the user drink?", limit=3)
store.search(("users", "u1"), filter={"kind": "preference"})
store.list_namespaces(prefix=("users",))

graph = builder.compile(checkpointer=ChloeDBSaver("agent.chloedb"), store=store)
```

- **Filters** on items' values run from a JSON field's index, with the operators above (LangGraph's own are `$eq`, `$ne`, `$gt`, `$gte`, `$lt` and `$lte`).
- **Search by meaning**: `index={"dims": 1536, "embed": OpenAIEmbeddings(), "fields": ["text"]}` (or `"embed": "openai:text-embedding-3-small"`), as LangGraph's stores take it, embeds in Python; `index={"model": "potion-retrieval-32M", "fields": [...]}` has ChloeDB embed each item's text as it is written, and each query's. `fields` are the paths of the text in a value (`["$"]`, the whole value, by default), and `put(..., index=False)` or `index=[paths]` overrides them for one item. An item's texts are embedded together, as one text.
- **Time to live**: `ttl={"default_ttl": 60, "refresh_on_read": True}` (minutes), or `put(..., ttl=...)`. The table [expires each row](query-language.md#tables) at its time (`expire_by`), so an expired item is gone from every read at once, and a read moves an item's time on when `refresh_on_read` is on.
- **Without a query**, a search returns the newest items first.

## LlamaIndex

`ChloeDBVectorStore` keeps nodes as the LangChain store keeps documents, with each node's document (`ref_doc_id`) and the rest of the node besides, so that `delete(ref_doc_id)` deletes a document's nodes and a query returns the nodes as they were written.

```python
from llama_index.core import StorageContext, VectorStoreIndex
from chloedb.llama_index import ChloeDBVectorStore

store = ChloeDBVectorStore("docs.chloedb")
index = VectorStoreIndex.from_documents(documents, storage_context=StorageContext.from_defaults(vector_store=store))
index.as_retriever(similarity_top_k=4).retrieve("where do vectors live?")
index.as_query_engine(vector_store_query_mode="hybrid", similarity_top_k=4)
```

Queries rank in the modes `default` (by vector), `text_search` and `sparse` (by BM25) and `hybrid` (both, fused by reciprocal rank), among the nodes that `MetadataFilters` keep (`==`, `!=`, `>`, `>=`, `<`, `<=`, `in`, `nin`, `any`, `all`, `contains`, `is_empty`, with `and`, `or` and `not`), `doc_ids` and `node_ids`. The store also has `get_nodes`, `delete_nodes` and `clear`.

With `model="potion-retrieval-32M"` the table embeds the nodes that come without a vector, and every query's text; `store.embed_model` is that model as LlamaIndex's, so the index needs no other:

```python
store = ChloeDBVectorStore("docs.chloedb", model="potion-retrieval-32M")
index = VectorStoreIndex.from_documents(documents, storage_context=StorageContext.from_defaults(vector_store=store), embed_model=store.embed_model)
```

## DB-API

`chloedb.dbapi` is a [PEP 249](https://peps.python.org/pep-0249/) driver, for code and tools written for any Python database: scripts written for `sqlite3`, `pandas.read_sql`, notebooks and report tools.

```python
import chloedb.dbapi

conn = chloedb.dbapi.connect("books.chloedb")          # or "http://127.0.0.1:7700", or a Database / Client
cur = conn.cursor()
cur.execute("SELECT id, title FROM books WHERE year >= :y ORDER BY bm25(title, :q) DESC LIMIT 5", {"y": 2000, "q": "dragon"})
cur.description                                        # (name, type_code, ...) per column
cur.fetchall()
cur.execute("UPDATE books SET rating = :r WHERE id = :id", {"r": 4.5, "id": "b1"})
cur.rowcount                                           # rows written

import pandas
pandas.read_sql("SELECT genre, COUNT(*) AS n FROM books GROUP BY genre", conn)
```

- `paramstyle` is `"named"`: `:name` in the SQL, bound from a mapping, with ChloeDB's own parameters too (a vector, `{"embed": text}`). A `datetime` or `date` binds as ISO 8601 text.
- A read (`SELECT`, `WITH`, `EXPLAIN`) keeps its rows for `fetchone`, `fetchmany` and `fetchall`, and `rowcount` is how many; a write sets `rowcount` to the rows it wrote.
- ChloeDB commits each statement as it runs: `commit()` has nothing to do, and `rollback()` raises `NotSupportedError`.
- `description`'s type codes are the columns' ChloeDB types (`"text"`, `"keyword"`, `"int"`, `"float"`, `"bool"`, `"timestamp"`, ...), which compare equal to the module's `STRING`, `NUMBER` and `DATETIME`.
- Errors are the module's PEP 249 classes (`ProgrammingError` for a bad statement, `OperationalError` for a busy, overloaded or unreachable database, `IntegrityError` for a conflict), each with the ChloeDB error's `code` and the original error as its `__cause__`.
- `connect(path, read_only=True)` opens a database another process writes, to read it.
