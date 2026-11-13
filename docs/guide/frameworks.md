# LangChain, LangGraph, LlamaIndex and pandas

<!-- run: needs chloedb-server -->

The Python package has adapters for the frameworks agents and retrieval
pipelines are built with: a LangChain vector store, a LangGraph checkpointer
and long-term memory store, a LlamaIndex vector store, and a DB-API driver for
pandas and anything else that takes a Python database connection. Each one
takes a database's path or a chloedb-server's URL, so the code below runs
unchanged on a file and on a cluster, and one file holds an agent's documents,
checkpoints and memories side by side, each in a table of its own.

This page builds a small support assistant's storage with all four, in one
file, `support.chloedb`. Nothing on it needs an API key: ChloeDB embeds the
texts itself with a model it runs in the process. The full list of arguments,
methods and filter operators is in [the reference](../chloedb/frameworks.md).

## Install

```sh
# install
pip install "chloedb[langchain,langgraph,llama-index]" pandas
```

Each extra pulls in its framework's core package only (`langchain-core`,
`langgraph`, `llama-index-core`). The examples embed with
`potion-retrieval-32M`, a 129 MB static model that runs on a CPU in a few
microseconds a text; fetch it once:

```sh
chloedb-server fetch-model potion-retrieval-32M
```

```text output
...
```

## LangChain: a vector store and a retriever

`ChloeDBVectorStore` keeps each document as a row with its text (indexed for
BM25), its metadata (a JSON field, so a filter on any key runs from an index)
and its vector. Given `model=` instead of a LangChain `Embeddings`, ChloeDB
embeds the documents as they are written and each query's text as it runs.

```python file=kb.py
from chloedb.langchain import ChloeDBVectorStore

articles = [
    ("refunds", "Refunds go back to the original card within 5 business days.", {"topic": "billing", "updated": 2026}),
    ("invoice", "Download past invoices as PDF from Settings, then Billing.", {"topic": "billing", "updated": 2025}),
    ("sso", "Single sign-on supports Okta and Azure AD on the Business plan.", {"topic": "accounts", "updated": 2026}),
    ("reset", "Forgot your password? Use the reset link on the sign-in page.", {"topic": "accounts", "updated": 2024}),
    ("e1042", "Error E1042 means the upload was larger than 2 GB; split the file.", {"topic": "uploads", "updated": 2026}),
    ("limits", "Uploads are limited to 2 GB per file and 50 files per batch.", {"topic": "uploads", "updated": 2025}),
]

store = ChloeDBVectorStore("support.chloedb", table="articles", model="potion-retrieval-32M")
store.add_texts([t for _, t, _ in articles], metadatas=[m for _, _, m in articles], ids=[i for i, _, _ in articles])
```

```sh
python kb.py
```

A question in other words than the article's finds it by its vector:

```python
from chloedb.langchain import ChloeDBVectorStore

store = ChloeDBVectorStore("support.chloedb", table="articles", model="potion-retrieval-32M")
for doc in store.similarity_search("how do I get my money back?", k=2):
    print(doc.id, "|", doc.page_content)
```

```text output
refunds | Refunds go back to the original card within 5 business days.
reset | Forgot your password? Use the reset link on the sign-in page.
```

An exact code is what vectors miss and words find. `mode="hybrid"` ranks by
both, fused by reciprocal rank, and `filter` keeps only the documents whose
metadata matches, written as MongoDB's filters (the operators LangChain's
other stores take):

```python
from chloedb.langchain import ChloeDBVectorStore

store = ChloeDBVectorStore("support.chloedb", table="articles", model="potion-retrieval-32M")
for doc, score in store.similarity_search_with_score("E1042", k=2, mode="hybrid", filter={"updated": {"$gte": 2026}}):
    print(doc.id, "|", doc.metadata["topic"], "|", round(score, 4))
```

```text output
e1042 | uploads | 0.0328
sso | accounts | 0.0161
```

`as_retriever()` is the same store as LangChain's `Retriever`, to put in a
chain:

```python
from chloedb.langchain import ChloeDBVectorStore

store = ChloeDBVectorStore("support.chloedb", table="articles", model="potion-retrieval-32M")
retriever = store.as_retriever(search_kwargs={"k": 2, "mode": "hybrid", "filter": {"topic": "accounts"}})
print([d.id for d in retriever.invoke("login with Okta")])
```

```text output
['sso', 'reset']
```

## LangGraph: a checkpointer and long-term memory

`ChloeDBSaver` keeps a graph's checkpoints, so a conversation goes on after a
restart or from another process, and `ChloeDBStore` is LangGraph's long-term
memory: items by namespace and key, searched by meaning or filtered by value.
This graph has no LLM in it, so it runs anywhere: one node remembers what the
user says about themselves, the next recalls the memories closest to the
message.

```python file=agent.py
import operator, sys
from typing import Annotated, TypedDict

from langgraph.graph import END, START, StateGraph
from chloedb.langgraph import ChloeDBSaver, ChloeDBStore


class State(TypedDict):
    user: str
    message: str
    log: Annotated[list, operator.add]


def remember(state, *, store):
    if state["message"].lower().startswith("i "):
        n = len(store.search(("users", state["user"]), limit=100))
        store.put(("users", state["user"]), f"m{n + 1}", {"text": state["message"]})
    return {"log": [state["message"]]}


def recall(state, *, store):
    hits = store.search(("users", state["user"]), query=state["message"], limit=1)
    return {"log": [f"recalled: {hits[0].value['text']}" if hits else "recalled: nothing"]}


builder = StateGraph(State)
builder.add_node("remember", remember)
builder.add_node("recall", recall)
builder.add_edge(START, "remember")
builder.add_edge("remember", "recall")
builder.add_edge("recall", END)

graph = builder.compile(
    checkpointer=ChloeDBSaver("support.chloedb"),
    store=ChloeDBStore("support.chloedb", index={"model": "potion-retrieval-32M", "fields": ["text"]}),
)

if __name__ == "__main__":
    thread, message = sys.argv[1], sys.argv[2]
    state = graph.invoke({"user": "ana", "message": message}, {"configurable": {"thread_id": thread}})
    print(state["log"][-2:])
```

Adapters given the same path in one process share one open database. A
database file is open in one process at a time, though, and another process
that opens it while this one has it is refused as busy: processes that
work on one database at once go through [a server](../chloedb/server.md).

Each run below is a new process. The memories outlive the conversation they
were said in, and the conversation's state outlives the process:

```sh
python agent.py t1 "I use the Business plan with Okta"
python agent.py t1 "I prefer replies by email"
python agent.py t2 "which plan am I on?"
```

```text output
['I use the Business plan with Okta', 'recalled: I use the Business plan with Okta']
['I prefer replies by email', 'recalled: I prefer replies by email']
['which plan am I on?', 'recalled: I use the Business plan with Okta']
```

The checkpointer kept every step of thread `t1`, so the graph can show its
state as it is, or as it was at any step (LangGraph's time travel):

```python
from agent import graph

config = {"configurable": {"thread_id": "t1"}}
print(graph.get_state(config).values["log"])
print(len(list(graph.get_state_history(config))), "checkpoints")
```

```text output
['I use the Business plan with Okta', 'recalled: I use the Business plan with Okta', 'I prefer replies by email', 'recalled: I prefer replies by email']
8 checkpoints
```

## LlamaIndex: an index over the same articles

`chloedb.llama_index.ChloeDBVectorStore` keeps LlamaIndex's nodes. With
`model=`, `store.embed_model` is ChloeDB's model as LlamaIndex's, so the
index needs no other embedding model and no API key:

```python
from llama_index.core import Document, StorageContext, VectorStoreIndex
from chloedb.llama_index import ChloeDBVectorStore

docs = [
    Document(text="Refunds go back to the original card within 5 business days.", metadata={"topic": "billing"}),
    Document(text="Single sign-on supports Okta and Azure AD on the Business plan.", metadata={"topic": "accounts"}),
    Document(text="Error E1042 means the upload was larger than 2 GB; split the file.", metadata={"topic": "uploads"}),
]
store = ChloeDBVectorStore("support.chloedb", table="nodes", model="potion-retrieval-32M")
index = VectorStoreIndex.from_documents(
    docs, storage_context=StorageContext.from_defaults(vector_store=store), embed_model=store.embed_model
)
for hit in index.as_retriever(similarity_top_k=1).retrieve("my file is too big"):
    print(hit.node.metadata["topic"], "|", hit.node.get_content())
```

```text output
uploads | Error E1042 means the upload was larger than 2 GB; split the file.
```

`as_query_engine(vector_store_query_mode="hybrid")` ranks by words and vectors
together, as the LangChain store's `mode="hybrid"` does; a query engine needs
an LLM, which LlamaIndex takes from `Settings.llm`.

## pandas and DB-API: SQL over all of it

Everything above went into tables of one file, and `chloedb.dbapi` opens it as
a [PEP 249](https://peps.python.org/pep-0249/) connection, so pandas and any
tool written for `sqlite3` reads it with SQL, ChloeDB's search functions
included:

```python
import warnings
import chloedb.dbapi, pandas

warnings.filterwarnings("ignore", "pandas only supports SQLAlchemy")
conn = chloedb.dbapi.connect("support.chloedb", read_only=True)
cur = conn.cursor()
cur.execute("SELECT id, bm25(text, :q) AS score FROM articles WHERE MATCH(text, :q) ORDER BY bm25(text, :q) DESC LIMIT 3", {"q": "upload"})
print([d[0] for d in cur.description], [(i, round(score, 3)) for i, score in cur.fetchall()])
print(pandas.read_sql("SELECT metadata.topic AS topic, COUNT(*) AS n FROM articles GROUP BY topic ORDER BY n DESC, topic", conn))
```

```text output
['id', 'score'] [('e1042', 0.969), ('limits', 0.969)]
      topic  n
0  accounts  2
1   billing  2
2   uploads  2
```

pandas warns about every DB-API connection but `sqlite3`'s, as it has
not tested them; `read_sql` reads this one as it reads SQLite's, and the
filter above only quiets the warning.

The tables the adapters made are ordinary ChloeDB tables: each has a key
`id`, the text, the metadata as JSON and the vector. [Search with
SQL](search.md) shows what else a query can do with them.

## On a server or a cluster

Every adapter takes a chloedb-server's or a coordinator's URL where a path
goes, with the server's token as `token=` or in `$CHLOEDB_TOKEN`:

```python norun
store = ChloeDBVectorStore("https://search.example.com", table="articles", model="potion-retrieval-32M")
saver = ChloeDBSaver("https://search.example.com")
conn = chloedb.dbapi.connect("https://search.example.com")
```

Each adapter reads through a session of its own, so its reads see its own
writes from any copy: start the servers with `--seq-no`, and a LangGraph thread
resumed a moment after its last step reads the checkpoint that step wrote,
also through a coordinator, where the checkpoints of a thread sit on one
shard. A DB-API read through a server is fetched a page at a time, past the
server's result window.

[Run a server](../chloedb/server.md) says how to start one, and the server's
vector fields embed with the same models.
