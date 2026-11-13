# chloedb: ChloeDB from Python

`import chloedb` runs a ChloeDB database in your process (`chloedb.open`,
`chloedb.Database`), or talks to a running `chloedb-server` (`chloedb.connect`,
`chloedb.Client`), with the same methods: `insert`, `upsert`, `delete`,
`import_file`, `query`, `sql`, `search`, and `table()` for queries built
from Python values instead of SQL text. A database in your process also
answers with Arrow columns (`query_arrow`), which pyarrow, polars and pandas
read without copying them.

```sh
# a release's prebuilt wheel (Linux glibc 2.28+, macOS 11+); no Rust needed
pip install chloedb
pip install ./chloedb-py          # or from source, from the repository's root; needs Rust
```

`import strata`, ChloeDB's old name, imports the same package for a release,
with a `DeprecationWarning`; release 0.1.0, from before the rename, was
published as `strata-db` and imports as `strata` only.

```python
import chloedb

db = chloedb.open("books.chloedb", schema={
    "table": "books", "key": "id",
    "fields": [
        {"name": "id", "type": "keyword"},
        {"name": "title", "type": "text"},
        {"name": "year", "type": "int"},
    ],
})
db.insert({"id": "b1", "title": "The river and the dragon", "year": 1999})
print(db.search("dragon").dicts())

from chloedb import col, bm25
for row in db.table().where(col("year") >= 1990).order_by(bm25("title", "dragon")).limit(5):
    print(row["title"])
```

Frames are lazy dataframes over tables, run where the data is (a build
from source, with pyarrow: `pip install "./chloedb-py[frame]"`; release 0.1.0
has no frames):

```python
import chloedb
from chloedb import col

shop = chloedb.connect("shop.chloedb")      # or a server: chloedb.connect("https://...")
(shop["orders"].filter(col("status") == "paid")
     .group_by("country").agg(revenue=col("amount").sum(), n=chloedb.len())
     .sort("revenue", descending=True).to_polars())
```

LangChain, LangGraph and LlamaIndex take ChloeDB as their vector store,
checkpointer and memory store (`chloedb.langchain`, `chloedb.langgraph`,
`chloedb.llama_index`, with the extras `[langchain]`, `[langgraph]` and
`[llama-index]`), and `chloedb.dbapi` is a DB-API 2.0 driver, for
`pandas.read_sql` and other tools: see
[the frameworks page](../docs/chloedb/frameworks.md).

The guide is [docs/chloedb/python.md](../docs/chloedb/python.md), and frames
have [their own page](../docs/chloedb/frames.md). Tests:
`maturin develop --release && pytest tests` in a virtualenv (the client
tests use `../chloedb-server/target/release/chloedb-server`, or
`$CHLOEDB_SERVER`, and are skipped without it; the Kafka consumer's test
needs a broker at `$CHLOEDB_KAFKA`, such as Redpanda in Docker, and
`confluent-kafka`; the Arrow tests need `pyarrow`, and `polars` and `pandas`
for theirs).
