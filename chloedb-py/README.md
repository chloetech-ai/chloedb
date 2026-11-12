# chloedb: ChloeDB from Python

`import strata` (or `import chloedb`, the same package by its new name) runs a ChloeDB database in your process (`chloedb.open`,
`chloedb.Database`), or talks to a running `chloedb-server` (`chloedb.connect`,
`chloedb.Client`), with the same methods: `insert`, `upsert`, `delete`,
`import_file`, `query`, `sql`, `search`, and `table()` for queries built
from Python values instead of SQL text. A database in your process also
answers with Arrow columns (`query_arrow`), which pyarrow, polars and pandas
read without copying them.

```sh
# a release's prebuilt wheel (Linux glibc 2.35+, macOS 11+); no Rust needed
pip install strata-db -f https://github.com/chloetech-ai/chloedb/releases/expanded_assets/strata-py-v0.1.0
pip install ./chloedb-py          # or from source, from the repository's root; needs Rust
```

Release 0.1.0, the only prebuilt wheel so far, predates the rename: its module
is `strata` only, so write `import strata as chloedb` in the examples below
(`import chloedb` fails on it), and it has no frames. A build from source is
today's package, and imports as both.

```python
import chloedb

db = chloedb.open("books.strata", schema={
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

shop = chloedb.connect("shop.strata")       # or a server: chloedb.connect("https://...")
(shop["orders"].filter(col("status") == "paid")
     .group_by("country").agg(revenue=col("amount").sum(), n=chloedb.len())
     .sort("revenue", descending=True).to_polars())
```

The guide is [docs/chloedb/python.md](../docs/chloedb/python.md), and frames
have [their own page](../docs/chloedb/frames.md). Tests:
`maturin develop --release && pytest tests` in a virtualenv (the client
tests use `../chloedb-server/target/release/chloedb-server`, or
`$CHLOEDB_SERVER`, and are skipped without it; the Kafka consumer's test
needs a broker at `$CHLOEDB_KAFKA`, such as Redpanda in Docker, and
`confluent-kafka`; the Arrow tests need `pyarrow`, and `polars` and `pandas`
for theirs).
