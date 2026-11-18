# chloedb: ChloeDB from Python

`import chloedb` runs a ChloeDB database in your process (`chloedb.open`,
`chloedb.Database`), the way `sqlite3` runs SQLite, or talks to a running
`chloedb-server` (`chloedb.connect`, `chloedb.Client`), with the same
methods: `insert`, `upsert`, `delete`, `import_file`, `query`, `sql`,
`search`, and `table()` for queries built from Python values instead of SQL
text. A database in your process also answers with Arrow columns
(`query_arrow`), which pyarrow, polars and pandas read without copying them.

```sh
pip install chloedb               # prebuilt for CPython 3.10+, on macOS 11+ and Linux with glibc 2.28+; no Rust needed
pip install "chloedb[server]"     # the same, and chloedb-server of the same release (from 0.3.7)
pip install ./chloedb-py          # or from source, from the repository's root; needs Rust
```

## A first search

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
db.upsert([
    {"id": "b1", "title": "The river and the dragon", "year": 1999},
    {"id": "b2", "title": "Winter garden", "year": 2005},
    {"id": "b3", "title": "Dragons of the northern rivers", "year": 2011},
])

# Words, ranked by BM25, with a filter
for row in db.query("SELECT id, title FROM books WHERE MATCH(title, :q) AND year > :y "
                    "ORDER BY bm25(title, :q) DESC", q="dragon", y=2000):
    print(row["id"], row["title"])

# A typo, forgiven
for row in db.query("SELECT id, title FROM books ORDER BY bm25(title, :q, fuzzy => 'auto') DESC LIMIT 2", q="dragn"):
    print(row["id"], row["title"])

db.close()
```

```text
b3 Dragons of the northern rivers
b1 The river and the dragon
b3 Dragons of the northern rivers
```

`chloedb.open(path, schema)` creates the database the first time and opens
it after. `upsert` replaces whole rows by their keys; to change one field,
use `db.sql("UPDATE books SET year = 2000 WHERE id = 'b1'")`.

- **More like this**: `{"vector_of": key}` is a stored row's vector, so
  `ORDER BY similarity(emb, :v)` with `v={"vector_of": "b1"}` ranks the rows
  nearest that row in meaning (on a table with a vector field, such as one
  embedded by `potion-retrieval-32M`).
- **A server**: `chloedb.serve()` starts a `chloedb-server` on this machine
  (from the `server` extra) and returns a client of it, which stops it when
  closed; `chloedb.connect("http://127.0.0.1:7700")` is a client of one
  already running. Both have the methods above. A web app whose processes
  all write (a reloader, several workers) uses a server, since one process
  at a time may open a file to write it.

Frames are lazy dataframes over tables, run where the data is (a build
from source, with pyarrow: `pip install "./chloedb-py[frame]"`; release 0.1.0
has no frames):

```python
import chloedb
from chloedb import col

shop = chloedb.connect("shop.chloedb")      # a file, or a server: chloedb.connect("https://...")
(shop["orders"].filter(col("status") == "paid")
     .group_by("country").agg(revenue=col("amount").sum(), n=chloedb.len())
     .sort("revenue", descending=True).to_polars())
```

LangChain, LangGraph and LlamaIndex take ChloeDB as their vector store,
checkpointer and memory store (`chloedb.langchain`, `chloedb.langgraph`,
`chloedb.llama_index`, with the extras `[langchain]`, `[langgraph]` and
`[llama-index]`), and `chloedb.dbapi` is a DB-API 2.0 driver, for
`pandas.read_sql` and other tools: see
[the frameworks page](https://chloetech-ai.github.io/chloedb/site/docs/frameworks-reference.html).

`import strata`, ChloeDB's old name, imports the same package for a release,
with a `DeprecationWarning`; release 0.1.0, from before the rename, was
published as `strata-db` and imports as `strata` only.

## When something fails

- `BusyError: busy: the database is open elsewhere`: the file is open to
  write already, in another process (Flask's debug reloader, a second
  worker, a chloedb-server) or by an earlier `open` in this one, and the
  message goes on to say which, and what to do. Share one
  `db` in a process, open it with `read_only=True` in the others, or serve
  it with `chloedb-server` and `chloedb.connect` to it.
- `urllib.error.URLError: <urlopen error [Errno 111] Connection refused>`:
  no server answers at the client's URL. It is an `OSError`, not a
  `chloedb.ChloeDBError`.
- `BadRequestError: schema error: row 1: schema error: bad value for minutes: "five"`:
  a value of the wrong type for its field; convert form input before writing.

The rest, with what to do, are in
[the Python page's common errors](https://chloetech-ai.github.io/chloedb/site/docs/python.html#common-errors)
and [troubleshooting](https://chloetech-ai.github.io/chloedb/site/docs/troubleshooting.html).

## Docs and tests

The guide is [the Python page](https://chloetech-ai.github.io/chloedb/site/docs/python.html)
([docs/chloedb/python.md](https://github.com/chloetech-ai/chloedb/blob/main/docs/chloedb/python.md)
in the repository), with [the search guide](https://chloetech-ai.github.io/chloedb/site/docs/search.html)
and [Build a search page](https://chloetech-ai.github.io/chloedb/site/docs/search-page.html)
for searches, and [DataFrames](https://chloetech-ai.github.io/chloedb/site/docs/frames.html)
for frames. Tests: `maturin develop --release && pytest tests` in a
virtualenv (the client tests use `../chloedb-server/target/release/chloedb-server`,
or `$CHLOEDB_SERVER`, and are skipped without it; the Kafka consumer's test
needs a broker at `$CHLOEDB_KAFKA`, such as Redpanda in Docker, and
`confluent-kafka`; the Arrow tests need `pyarrow`, and `polars` and `pandas`
for theirs).
