# ChloeDB from Python

The `chloedb` package runs a ChloeDB database inside a Python process, the way `sqlite3` runs SQLite, or talks to a running [chloedb-server](server.md). Both have the same methods, so a script written against a local file works against a server.

LangChain, LangGraph and LlamaIndex take ChloeDB as a vector store, a checkpointer and a memory store, and `chloedb.dbapi` is a DB-API 2.0 driver: see [frameworks](frameworks.md).

For dataframe work, `db["table"]` is a lazy frame written as Polars writes one, run where the data is, with the answer as Arrow: see [DataFrames](frames.md).

## Install

The package is in [`chloedb-py/`](https://github.com/chloetech-ai/chloedb/tree/main/chloedb-py): a Rust extension built with [maturin](https://www.maturin.rs), for CPython 3.10 and later.

### Prebuilt

Each `chloedb-py-v<version>` release on [GitHub](https://github.com/chloetech-ai/chloedb/releases) has a wheel per platform, for Python 3.10 and later, that installs with no Rust and no compiler. The package is not on PyPI yet, so point pip at the release and it picks the wheel for your machine:

```sh
pip install chloedb -f https://github.com/chloetech-ai/chloedb/releases/expanded_assets/chloedb-py-v0.2.0
```

The old name, `import strata`, still works for a release, with a `DeprecationWarning`; release 0.1.0 was published as `strata-db` and imports as `strata` only.

| platform | wheel |
|---|---|
| macOS 11+ on Apple silicon | `chloedb-<version>-cp310-abi3-macosx_11_0_arm64.whl` |
| macOS 11+ on Intel | `chloedb-<version>-cp310-abi3-macosx_11_0_x86_64.whl` |
| Linux x86-64, glibc 2.28+ (RHEL 8, Amazon Linux 2023, Debian 10, Ubuntu 20.04 and later) | `chloedb-<version>-cp310-abi3-manylinux_2_28_x86_64.whl` |
| Linux aarch64, glibc 2.28+ | `chloedb-<version>-cp310-abi3-manylinux_2_28_aarch64.whl` |

The workflow [`py-release.yml`](https://github.com/chloetech-ai/chloedb/blob/main/.github/workflows/py-release.yml) builds them on a tag `chloedb-py-v<version>` (or `strata-py-v<version>`; the version in `chloedb-py/pyproject.toml`), installs each one into an empty virtualenv and runs the embedded tests against it, then attaches them to the release. The Linux wheels are built in the manylinux_2_28 container (AlmaLinux 8). The same tag publishes them to PyPI once the repository is set up for it (the workflow's header says how), and `pip install chloedb` then takes them; until then the package is not on PyPI. Windows and Alpine (musl) have no wheel: build from source there (Windows through WSL 2).

### Only the client, with no build

[`chloedb.connect`](#a-server-from-python), the client of a running chloedb-server, needs only Python's standard library. Without the extension, from a checkout of the repository, put `chloedb-py/python` on the path and import it:

```sh
PYTHONPATH=chloedb-py/python python3 -c 'import chloedb; print(chloedb.connect("http://127.0.0.1:7700").health())'
```

The package imports without the extension, and `chloedb.Client`, `chloedb.connect`, `chloedb.ChloeDBError`, `Result` and the query builder work. A database in the process (`chloedb.open`, `chloedb.Database`, `from_sqlite`, the codebook training) raises `ImportError` saying what to install.

### From source

It needs Rust (stable) and a C compiler; the build takes about 12 minutes on 4 cores.

```sh
pip install ./chloedb-py                    # from the repository's root: builds and installs a wheel
```

or, while working on it:

```sh
cd chloedb-py
python3 -m venv .venv && . .venv/bin/activate
pip install maturin pytest
maturin develop --release                  # builds the extension into the virtualenv
pytest tests                               # the client tests also need chloedb-server built
```

The package includes [DataFusion](query-language.md#subqueries-with-and-joins), which runs joins, groups and sorts over many joined rows (`--no-default-features` leaves it out). Model2Vec embedding models need nothing more. For the BERT models, build with `--features bert` (`--features accelerate` on a Mac): `maturin develop --release --features bert`, or `MATURIN_PEP517_ARGS="--features bert" pip install ./chloedb-py`. The extension runs on any CPU of its architecture: ChloeDB picks its SIMD kernels (AVX2, AVX-512, the aarch64 dot product instructions) at run time. `RUSTFLAGS="-C target-cpu=native"` builds one for the building machine only.

## A database in your process

```python
import chloedb

db = chloedb.open("books.chloedb", schema={
    "table": "books",
    "key": "id",
    "fields": [
        {"name": "id", "type": "keyword"},
        {"name": "title", "type": "text"},
        {"name": "description", "type": "text"},
        {"name": "genres", "type": "keyword", "multi": True},
        {"name": "year", "type": "int"},
        {"name": "embedding", "type": "vector",
         "embed": {"from": ["title", "description"], "model": "potion-retrieval-32M"}},
    ],
})

db.insert([
    {"id": "b1", "title": "The river and the dragon", "description": "A fantasy of rivers.", "genres": ["fantasy"], "year": 1999},
    {"id": "b2", "title": "Winter garden", "description": "A quiet mystery.", "genres": ["mystery"], "year": 2005},
])

for row in db.search("dragon", limit=5):
    print(row["id"], row["title"])

db.query("SELECT year, COUNT(*) FROM books WHERE genres = :g GROUP BY year", g="fantasy").rows
db.close()
```

`chloedb.open(path, schema)` opens the database at `path`, or creates it with `schema` if it does not exist. The schema is a dict, JSON text or the path of a JSON file, in the [form the server takes](server.md#schemas). `Database.create(path, schema)` and `Database.open(path)` do one or the other. `models=` says where embedding and [ranking](query-language.md#ranking-models-gbdtmodel-features) models, and the captioners of [text made from media](server.md#text-from-media), are (default `$CHLOEDB_MODELS`, else `~/.local/share/strata/models`; `chloedb-server fetch-model NAME` downloads one there), and `directory=True` creates the database as a directory of segment files.

A database written by a version of ChloeDB from before [field ids](index.md#wal-memtable-and-snapshots) does not open: `chloedb.upgrade(path)` copies it into this version's format in its place, keeps the old one as `<path>.before-field-ids`, and returns `(old_path, tables, rows)`, or `None` when the database needs no upgrade. Nothing may have it open meanwhile.

One process at a time can have a database open to write it. Others can open it with `chloedb.open(path, read_only=True)` (or `Database.open(path, read_only=True)`) to read it while it is written, the other workers of a web server under gunicorn or uwsgi, say: each query sees the writer's commits so far, every write raises `ChloeDBError`, and `refresh()` looks for new commits at once ([how readers follow the writer](index.md#readers-in-other-processes)). Within a process, threads can query at once: every call releases the GIL while it runs. Threads can write at once too: a write waits while another holds the writer, and the writes that wait share the next commit (one disk flush for all of them), so none fails as busy. For writes from several processes, serve the database with `chloedb-server` and use the client below. Segments are merged in the background as writes add them, one merge at a time, as a server merges them, so a table written to for weeks stays in a few segments without the rows its updates replaced. A `with` block, or `close()`, writes the rows still in memory into the file (so that the file alone holds the database, [backups](#backing-up-and-damaged-files)), merges the small segments that writes left, gives the file system back the space merges freed when it is an eighth of the file or more, and closes the file. `optimize()` goes further, for a table that is done changing: one segment, up to 2,000,000 rows.

### Backing up, and damaged files

A database is its file plus `<path>-wal`, the write-ahead log, which holds the rows written since the last flush; copying the file alone while the database is open loses them. To back up a database:

- while it is open and in use: `db.checkpoint("backup.chloedb")` writes a consistent copy (the destination must not exist), checks every segment against its checksum first, so that a damaged database fails the checkpoint instead of being copied, checks the copy, and only then gives it its name. Restore by opening the copy, or renaming it into place while nothing has the database open;
- after `close()` (or a `with` block): `close()` writes the rows still in memory into the file and leaves the log empty, so the file alone is then a complete copy; `db.flush()` does the same at any moment, until the next write.

Copying a database's files while it is written is not a backup, even with its `-wal`.

When the newest version of a database or its log is damaged so that opening it would lose acknowledged writes (bit rot, a copy cut short, a log deleted while it held transactions), `chloedb.open` raises `ChloeDBError` naming the file, what is damaged and how to recover, and changes nothing: restore a backup, or open with `recover=True` to accept the loss. That keeps copies of the damaged files first (`<path>.damaged-<time>`, `<path>-wal.damaged`), opens what is left, issues a `RuntimeWarning`, and `db.damaged` says what was lost (it is `None` for a database that opened whole).

## Writing

Rows are dicts of field name to value: `str`, `int`, `float`, `None`, `bool` for a `bool` field, a `datetime` or `date` (or ISO 8601 text) for a `timestamp` field, a list for a `multi` field, a list of floats or a numpy array for a vector, a dict of term weights for a `sparse` field, a URI or a dict for a `media` field (in-process, the whole value or `{"uri": …, "data": <base64>}`: nothing is fetched, see [media fields](server.md#media-fields)). A naive `datetime` is taken as UTC. A field the schema does not have is an error. Results give a `bool` field's values as `bool` and a `timestamp` field's as ISO 8601 text in UTC (`"2026-10-01T12:30:00Z"`), in-process and from the client alike; `datetime.fromisoformat` reads them.

| method | what it does |
|---|---|
| `insert(rows)` | adds a row (a dict) or rows (any iterable of dicts) whose keys must be new; all or none are written. Fewer than 10,000 rows go through the write-ahead log into memory, which gathers the rows of many inserts into one segment once it holds 10,000; from 10,000 rows on, they are bulk-loaded straight into segments. A vector may be a numpy array of float32 or float64, which is read from its bytes rather than a Python float at a time. |
| `upsert(rows)` | adds rows, replacing the rows with their keys, in transactions of 10,000 |
| `delete(keys)` | deletes the rows with a key or keys; returns how many there were |
| `get(key)`, `get_many(keys)` | the row with a primary key (a dict of the columns `SELECT *` gives), or `None`, read without SQL ([rows by key](server.md#rows-by-key)); `get_many` answers a list in the keys' order. Each takes `table=` |
| `expire()` | deletes the rows whose [`expire_by`](query-language.md#tables) time has passed, which reads leave out already; returns how many |
| `sql("UPDATE ...")` | `INSERT`, `UPDATE` and `DELETE` statements; returns the rows written. It also runs `CREATE TABLE`, `ALTER TABLE` and `DROP TABLE` ([tables](query-language.md#tables)) and `CREATE` and `DROP RANK PROFILE` ([rank profiles](query-language.md#rank-profiles-a-ranking-kept-with-its-table)), which return 0, and writes any table |
| `add_column(name, type, **options)`, `drop_column(name)`, `rename_column(old, new)` | change a table's columns while it holds rows, as [`ALTER TABLE`](query-language.md#changing-a-tables-columns) does: `options` are the field's, in the short form of a schema (`db.add_column("price", "float", default=0.0)`, `db.add_column("emb", "vector", embed=["title"], model="potion-retrieval-32M")`); rows written before a column was added read its default, or `None`. Each takes `table=`. In-process only: through the client, send `ALTER TABLE` with `sql` |
| `import_file(path, format=None)` | loads a JSON Lines, JSON, CSV or TSV file ([how files are read](server.md#loading-rows)); returns `{rows, bulk, skipped_fields}` |
| `write(ops)` | applies write operations in one transaction, as the server's `/v1/write` takes them: `insert`, `upsert` and `delete`, `set` of a field (`{"op": "set", "key": k, "field": "status", "value": "SOLD"}`: a number in place, another by writing the row again with the value), and `add` to a number in place (`{"op": "add", "key": k, "field": "n", "value": 1}`; with a `doc` instead of a `key`, an `add` inserts the row when no row has its key); each names its `table` or writes the first; returns the rows written |
| `import_sqlite(path, tables=None, schemas=None)` | imports a SQLite database's tables, each into the table of its name, created when missing ([SQLite in and out](sqlite.md)); returns what it did and what did not carry over |
| `export_sqlite(path, tables=None, fts=True, indexes=True)` | writes the tables to a new SQLite file, with indexes, an FTS5 index over the text and the ChloeDB schemas |
| `import_parquet(path, table=None, schema=None, key=None)` | imports a Parquet file or a directory of them (Spark's part files, Hive partitions) into one table, created from the columns when missing ([Parquet and S3](parquet.md)); returns what it did |
| `export_parquet(path, tables=None)` | writes the tables to Parquet: a `.parquet` file for one table, else a directory of them, each keeping its ChloeDB schema |
| `import_arrow(data, table=None)` | writes the rows of a pyarrow Table or RecordBatchReader, a polars DataFrame, a DuckDB relation or anything with `__arrow_c_stream__`, columns into the fields of their names; bulk-loaded into an empty table, else upserted |

`chloedb.from_sqlite("shop.sqlite", "shop.chloedb")` creates a database from a SQLite file's tables, and `chloedb.plan_sqlite("shop.sqlite")` says what it would make of them without writing; edit the plan and pass it to either as `schemas`. `chloedb.from_parquet("events.parquet", "lake.chloedb")` and `chloedb.plan_parquet(...)` do the same for Parquet. Any of these paths, and `import_file`'s, may be an `s3://` URL, read with the `AWS_*` variables ([S3](parquet.md#s3-and-s3-compatible-stores)).

`insert`, `upsert`, `delete` and `import_file` write the database's first table, the one `schema` describes, or the one `table=` names (`db.insert(rows, table="events")`); `sql` names the table it writes, and `tables()` lists every table's schema. Through a coordinator, `delete` on a table placed by a column takes that column's value as `routing=` ([tables on shards](server.md#tables-on-shards)). A key a row leaves out gets its field's `default`, and a field with `not_null` refuses a row without a value ([schema options](server.md#schemas)); a `timestamp` defaulting to `"now"` takes one time for all the rows of a call.

Vectors of embedded fields that a row leaves out are computed by the field's model when it is open, here or behind an API: a field naming `openai/text-embedding-3-small`, `cohere/embed-v4.0`, `voyage/voyage-3.5`, `gemini/gemini-embedding-001`, `ollama/nomic-embed-text` or another [hosted model](server.md#hosted-models) gets its vectors from the provider, with the key in the process's environment (`OPENAI_API_KEY`, ...), so inserting plain text is enough. A write whose vectors the provider will not give (after its retries) raises `ChloeDBError` and writes nothing. `db.embed_errors` lists the embedded fields whose model did not open (a hosted model without its key says which variable to set), whose vectors rows must then bring: a write that leaves one to a model ChloeDB could run but has not open fails, and says so, unless `db.set_embed_later()` lets it store the row without its vector, to be filled in once the model opens (a server does it itself; from Rust, `Db::embed_backfill`) (before, such a row was stored without a vector, and no error said so; a server still does that, and fills them in itself). A table records the fingerprint of its model's weights (the SHA-256 of its weights file) when it is created with the model in its models directory, and a model of the same name with other weights does not open for it: it is listed in `db.embed_errors`, with what to do. It also lists the [semantic-ID fields](#semantic-id-codebooks) whose codebook did not open.

## Querying

`query(sql, params)` runs a `SELECT` in [ChloeDB's SQL](query-language.md). Parameters bind `:name` placeholders, from a dict, keyword arguments, or both:

```python
db.query("SELECT id, title FROM books WHERE year >= :y ORDER BY bm25(title, :q) DESC LIMIT 10", {"y": 2000}, q="dragon")
db.query("SELECT id FROM books ORDER BY similarity(embedding, :v) DESC LIMIT 10", v={"embed": "a quiet mystery"})
db.query("SELECT id FROM books ORDER BY similarity(embedding, :v) DESC LIMIT 10", v={"vector_of": "b1"})
```

A parameter is a number, a string, `None`, a list of numbers or a numpy array (a vector), a list, a dict of term weights, `{"embed": text}` (the text's vector by an embedded field's model; add `"field"` to pick one) or `{"vector_of": key}` (a stored row's vector).

The answer is a `Result`:

| | |
|---|---|
| `for row in result`, `result[i]` | rows as dicts of column to value |
| `result.rows`, `result.columns` | rows as lists, in column order |
| `result.keys`, `result.scores` | each row's primary key and the score it was ranked by |
| `result.column("title")`, `result.scalar()` | one column; the first value (a count, a sum) |
| `result.stats` | what the engine did |
| `result.to_pandas()` | a DataFrame (with pandas installed) |

`query_arrow(sql, params)` runs a `SELECT` as `query` does and answers with its columns as Arrow arrays, an `ArrowResult`, without making a Python object of each value; a query of one table that neither groups nor counts reads its columns from the database's segments straight into the arrays. pyarrow, polars, pandas and DuckDB read the arrays where the database wrote them, without copying them, through the [Arrow PyCapsule interface](https://arrow.apache.org/docs/format/CDataInterface/PyCapsuleInterface.html):

```python
r = db.query_arrow("SELECT id, year, rating FROM books WHERE year >= :y", y=1950)
r.to_pyarrow()            # a pyarrow Table
r.to_polars()             # a polars DataFrame (without pyarrow)
r.to_pandas()             # a pandas DataFrame of the Arrow columns (pandas.ArrowDtype); arrow_dtypes=False copies them into numpy
pyarrow.table(r)          # any library that reads Arrow streams reads r
duckdb.sql("SELECT avg(rating) FROM r")
```

| ChloeDB | Arrow |
|---|---|
| `int` | `int64` |
| `float` | `float64` |
| `keyword`, `text`, `json` | `string` (a JSON value as its text) |
| `bool` | `bool` |
| `timestamp` | `timestamp[us, tz=UTC]` |
| `vector` | `list<float32>` |
| a `multi` field | `list<int64>` or `list<string>` |

A computed column has the type of its values: a column of only NULLs (or of no rows) is `null`, and one of integers and floats `float64`. Values of other types mixed in one column (a `CASE` that answers a number in some rows and text in others) are not one Arrow array: `query_arrow` raises `ChloeDBError` naming the column, and `query` answers it. `r.columns`, `len(r)` and `r.stats` are as a `Result`'s. `query_arrow` comes with the package's default feature `datafusion`. A `Client`'s `query_arrow` asks the server for an Arrow answer (`"format": "arrow"`) and returns a pyarrow Table with the same column types; it needs pyarrow, and a server too old for Arrow answers JSON, which it types from the values.

`search(text, limit=10, where=None, columns="*", mode="auto")` writes the query for the common case. It ranks by BM25 over the text fields (the generated ones if the table has any, such as a combined `body`, else every indexed text field) fused by reciprocal rank with the similarity of the first embedded vector field whose model is open; `mode="text"` or `"vector"` uses one side only. `where` adds a condition, [built in Python](#queries-built-in-python) or in SQL with `params` for its placeholders:

```python
db.search("dragon", where=col("year") >= 1990, columns=["id", "title"])
db.search("dragon", where="year >= :y", params={"y": 1990})
```

`embed(text, field=None)` returns the query vector of a text (`embed(image=data, field="emb")` of an image file's bytes, by a [model of images](server.md#vectors-of-images), with the package built with `--features vision`, and `embed(audio=data)` of a sound file's, by a [model of sound](server.md#vectors-of-sound), built with `--features audio`), `embed_tokens(text, field)` the vectors of a query's tokens by the ColBERT model of a [multi-vector field](server.md#a-vector-a-token), as `maxsim` takes them (`{"embed": text, "field": "tok"}` as a parameter gives the same), and `len(db)` the number of rows.

`plan(sql, params)` returns a statement's [plan](query-language.md#explain) as a dict without running it, and `run(plan)` runs a plan: a read's `Result`, or the number of rows a write wrote.

```python
p = db.plan("SELECT id FROM books WHERE year >= :y LIMIT 3", y=2000)
db.run(p)
```

### Limits: memory, deadlines, interrupting

A query runs in your process, so what it builds up (its groups, the rows it orders and returns, a percentile's values, a join's tables) is held by your program. The queries of every database a process opens share a memory budget, by default a quarter of the machine's memory (the cgroup's limit in a container, else RAM), or `CHLOEDB_QUERY_MEMORY_MB` MiB when that is set when the first database opens (`0`: no budget). A query that would hold more stops and raises `ChloeDBError` with code `memory`, naming the budget and how to raise it, and gives back what it held; the program and its other queries go on. `SELECT * FROM big` on a table larger than the budget, a `GROUP BY` over a column with a value per row or a percentile over every row fail this way rather than taking the machine's memory. The counts are estimates of what a query grows, not of every allocation, and the index a query reads is mapped from the file and not counted.

A query has no deadline unless it is given one. `timeout_ms` (to `query`, `query_arrow`, `scroll`, `sql`, `search` and `run`, as a `Client`'s query takes it) stops it once it has run that long: it raises `DeadlineError`, naming the `timeout_ms`. `memory=` gives a query a budget of its own, in bytes (`memory=0`: none). A database's own limits, for every query that does not give its own, are set when it opens or later:

```python
db = chloedb.open("books.chloedb", timeout_ms=5_000, memory=512 << 20)  # 5 s, and 512 MiB its queries share
db.query("SELECT genres, COUNT(*) FROM books GROUP BY genres", timeout_ms=200)
db.query("SELECT * FROM books", memory=0)                               # this one counts nothing
db.set_limits(timeout_ms=None, memory=None)                             # back to the defaults
db.timeout_ms, db.memory                                                # the limits now (None: none)
```

`db.interrupt()`, from any thread, stops the queries of the database running now, as `sqlite3.Connection.interrupt` does: each raises `DeadlineError` within a few thousand rows, and queries that start after it run. Calls release the GIL while they run, so another thread (a watchdog, a web server's request that was cancelled) can call it:

```python
threading.Timer(2.0, db.interrupt).start()
try:
    db.query("SELECT a.id, b.id FROM books a JOIN books b ON a.year = b.year")
except chloedb.DeadlineError:
    ...
```

A query checks its deadline and `interrupt` between segments and every 1,024 rows it groups, orders, walks or joins. A write is not cut short: `sql`'s limits apply to a `SELECT`. Ctrl-C reaches Python only when a call returns, so a notebook stops a runaway query with `timeout_ms` or `interrupt()` from another thread.

## Queries built in Python

`table()` starts a query of the table built from Python values instead of SQL text, so nothing is quoted or escaped: fields are named as the schema names them (`col("order")`, `col("doc no")`), and values keep their types (a `str` is text whatever quotes it holds, a `date` or `datetime` a timestamp, UTC unless it says otherwise). Each method returns a new query; iterating one, or `run()`, runs it and gives its `Result`:

```python
from chloedb import col, match, bm25, similarity, rrf, count, score

q = (db.table()
     .select("id", "title", score())
     .where((col("year") >= 2019) & match("body", "graph neural") & ~col("tags").isin(["draft"]))
     .order_by(bm25("body", "graph neural"))
     .limit(10))
for row in q:
    print(row["id"], row["score()"])

db.table().select("genre", count().alias("n"), col("rating").avg().alias("avg")).group_by("genre").having(count() > 10).order_by(col("genre"))
db.table().order_by(rrf(bm25("title", text), similarity("emb", db.embed(text)))).limit(10)
# each video's best clip, the 10 best videos
db.table().select("video", "id", score()).distinct_on("video").order_by(bm25("transcript", text)).limit(10)
```

| | |
|---|---|
| `col(name)`, `lit(value)` | a field; a value where an expression is wanted |
| `==`, `!=`, `<`, `<=`, `>`, `>=` | comparisons with a value or another expression |
| `&`, `\|`, `~` | and, or, not (Python's `and`, `or`, `not` and chained comparisons such as `1 < col("x") < 5` raise `TypeError`) |
| `.isin(values)`, `.between(lo, hi)`, `.is_null()`, `.is_not_null()` | as SQL's `IN`, `BETWEEN` (a bound of `None` is open), `IS NULL` |
| `match(field, query, min=None, min_rows=None, max_rows=None)` | the full-text condition, in [MATCH's syntax](query-language.md#full-text-match); `min=2` or `min="75%"` asks for that many of its items, and with `max_rows` (or `min_rows`) for [as many as fit](query-language.md#as-many-as-fit-min_rows-and-max_rows) |
| `at_least(n, *conds, min_rows=None, max_rows=None)`, `count_true(*conds)` | at least `n` of the conditions hold (`n` a count, or a percentage such as `"75%"`), as SQL's [`AT_LEAST`](query-language.md#some-of-the-conditions-at_least), or with `max_rows` (or `min_rows`) [as many as fit](query-language.md#as-many-as-fit-min_rows-and-max_rows); how many hold, a number to rank by |
| `bm25(field, text, syntax=None, min=None, min_rows=None, max_rows=None)`, `similarity(field, vector)`, `rrf(*scores, k=60, depth=100)`, `ranked(keys)` | scores; `bm25` reads [weights, required and excluded words](query-language.md#weights-required-and-excluded-words) (`"+mrna vaccine^2 -animal"`) unless `syntax="plain"`, and in `order_by` with `max_rows` ranks the rows with [as many of its words as fit](query-language.md#as-many-as-fit-min_rows-and-max_rows); `similarity` takes a vector (`embed(text)` gives a text's), or `{term: weight}` for a SPARSE field; `ranked` is a [ranking from elsewhere](query-language.md#rankedkeys-a-ranking-from-elsewhere), primary keys best first |
| `+`, `-`, `*`, `/`, `%`, `**`, `abs()`, `round()`, `ln`, `log`, `exp`, `sqrt`, `sigmoid`, `clamp`, `least`, `greatest`, `coalesce`, `tmm(x, lo=None, hi=None)`, `minmax`, `zscore`, `rank`, `case((cond, value), ..., otherwise=None)` | a [ranking expression](query-language.md#ranking-expressions) of scores, fields and numbers: `bm25("body", q) * (1 + 0.2 * coalesce(col("rating"), 0))`. `+` adds raw values; `0.3 * tmm(bm25(...)) + 0.7 * tmm(similarity(...))` is weighted fusion |
| `gbdt(model, *features)` | a [ranking model](query-language.md#ranking-models-gbdtmodel-features)'s score of a row from its features: `gbdt("docs_ltr", bm25("body", q), col("rating"))`, the LightGBM or XGBoost model `<models>/docs_ltr/gbdt.json` |
| `reachable(field, seeds, hops=1, ...)`, `path(field, seeds, to, ...)`, `ppr(field, seeds, ...)` | [graph](graphs.md) conditions and PageRank as a score, with the SQL functions' options as keyword arguments |
| `score()`, `hops()` | the score a row was ranked by; its hops from a traversal's seeds |
| `features()` | in `select`, a column for each input of the ranking ([features](query-language.md#features-the-values-a-ranking-read)), named as Python writes it; any expression of scores that scores a row by itself is a column too |
| `count()`, `col(f).count(distinct=True)`, `.sum()`, `.avg()`, `.min()`, `.max()` | aggregates, for `select`, `having` and `order_by` over groups |
| `.alias(name)`, `.asc()`, `.desc(nulls_first=False)` | a column's name; an `order_by` direction |

`select(...)` names the columns (without it, the stored fields but vectors, as `SELECT *`), `where(...)` keeps rows, `order_by(...)` ranks by a score best first or orders by fields ascending, `rerank(expr, count)` ranks its top `count` rows again ([rerank phases](query-language.md#rerank-phases)), `group_by(...)` and `having(...)` group, `distinct_on(...)` keeps the [first row of each group](query-language.md#the-first-row-of-each-group-distinct-on) in the query's order, `max_per(k, field)` keeps [at most k rows of each value](query-language.md#at-most-k-rows-of-a-value-max-k-per), and `limit(n)` and `offset(n)` page (groups, after `distinct_on`). A query is a [plan](query-language.md#explain), the same JSON that `plan(sql)` gives: `q.plan()` shows it, and a `Client` runs it on a server the same way.

## The rank log

`query_logged(sql, params, context=..., variant=..., candidates=...)` runs a ranked query and writes what it served to a [rank log](query-language.md#the-rank-log-what-each-query-served): a row per row answered, with its position, its score and the values its ranking read, as they were computed. `candidates` logs that many rows from the query's `OFFSET`, those past its `LIMIT` as not shown, and `ts` (a `datetime` or microseconds) says when it was served, now by default. The `Result` is the query's, with `result.request`, the id its log rows carry, for labelling them later. `export_rank_log(...)` reads the log back as a list of dicts for training, each with its context's history and its label as of the row's time, from a table of events:

```python
r = db.query_logged("SELECT id, title FROM books ORDER BY bm25(title, :q) DESC LIMIT 10",
                    {"q": "dragon"}, context="u42", variant="b", candidates=50)
db.sql("UPDATE rank_log SET label = 3 WHERE request = :r AND row_key = :k", {"r": r.request, "k": r.keys[0]})

rows = db.export_rank_log(events={"table": "events", "user": "usr", "item": "item", "time": "ts", "action": "action"},
                          history=20, window="30 minutes", grades={"click": 1, "buy": 3})
rows[0]["features"], rows[0]["label"], rows[0]["history_items"]
```

## Semantic-ID codebooks

A [semantic-ID field](server.md#semantic-ids) gives each row a code from its vector by a codebook, which `chloedb.train.codebook` trains and saves in the models directory before a table names it:

```python
h = chloedb.train.codebook(db, "items-rq", field="emb")       # a table's vectors
h = chloedb.train.codebook(vectors, "items-rq")               # a list of lists, or a 2-D numpy array

items = chloedb.open("items.chloedb", schema={
    "table": "items", "key": "id",
    "fields": [
        {"name": "id", "type": "keyword"},
        {"name": "emb", "type": "vector", "dim": 384},
        {"name": "sid", "type": "keyword", "multi": True,
         "semantic_id": {"from": "emb", "codebook": "items-rq"}},
    ],
})
items.upsert(rows)                                            # each row gets its sid
items.query("SELECT id FROM items WHERE sid = '12.7'")
```

It returns the SHA-256 of the file, which the table records. The options are `levels` (1 to 4, default 3), `k` centroids a level (up to 256), `iterations` of k-means a level (25), `sample`, the vectors it trains on (100 a centroid by default, drawn at random), `seed` (the same vectors and options give the same codebook), `table` (a database's first by default) and `models` (the default models directory, as for `Database.create`). Pass the same `models=` to `Database.create` and `Database.open`: a codebook that does not open is listed in `db.embed_errors`, and writes that need a code then fail.

## Next items

`chloedb.ngrams` keeps counts of what came after the last one or two items in a table, and `chloedb.next_items(db, history, k=10)` reads them with backoff ([Next items](next-items.md)):

```python
from chloedb import ngrams

db = chloedb.open("next.chloedb", ngrams.schema())
db.write(ngrams.ops(["a", "b"], "c"))       # after each event: "c" after [..., "a", "b"]
chloedb.next_items(db, ["a", "b"], k=10)     # [("c", 1.0), ...]
```

## Sequence models

[`encode('model', history)`](query-language.md#sequence-models-encodemodel-history) turns a user's events into a vector of an item field, by a model that `chloedb.train.sequence` trains with PyTorch (`pip install torch`) and saves in the models directory:

```python
import chloedb.train
db = chloedb.Database.open("shop.chloedb", models="models")
out = chloedb.train.sequence(db, "next", table="items", field="emb",
                            events={"table": "events", "user": "usr", "item": "item", "action": "action", "time": "ts"},
                            learn_items=True, negatives="all", holdout=True, models="models")
chloedb.train.write_vectors(db, "next", models="models")      # the learned item vectors, into items.emb
db.query("""SELECT id FROM items ORDER BY similarity(emb, encode('next',
            (SELECT item, action, ts FROM events WHERE usr = :u ORDER BY ts DESC LIMIT 200))) DESC LIMIT 10""", {"u": "u42"})
```

- **The histories** come from one of `events` (a table of the database: its columns of users, items, and optionally actions, times, and `order` for events with equal times), `sequences` (a dict from user to a list of `(item, action, time in seconds)`, oldest first) or `records` (the [rank log's export](#the-rank-log) with `history`: each record's history is a training example, and its row the target when its label is positive or missing). The item vectors are the field's, or `vectors` (a dict from key to vector).
- **What it learns**: every event is the target of the events before it. Each item is scored as `similarity` scores it, the dot product of the model's unit vector with the item's vector, over `temperature` (0.05), and the loss is cross-entropy against the other targets of the batch (`negatives="in_batch"`, the default) or every item (`"all"`, for catalogs that fit). With `learn_items=True` the item vectors are trained too, starting from the field's, and returned normalized in `out["vectors"]`; without it the field keeps its vectors and the model learns to map histories into their space, so a new item is found as soon as it has a vector.
- **The item vectors go with the model**: it is saved with the vectors it was trained with (`items.safetensors`, part of its hash), and `encode` refuses a history whose items have other vectors in the field. `chloedb.train.write_vectors(db, name, models=None)` writes them into the field the model's config names, by an `UPDATE` of that field alone for each item (a `Database` or a `Client`); returns the rows written. `chloedb._sequence.read_items(path)` reads them back.
- **Options**: `width` (64), `layers` (2), `heads` (1), `ffn` (4 × width), `max_len` (200 events), `gap_buckets` (32), `actions` (the actions seen by default; others count as one more), `epochs` (50), `batch` (128), `lr` (1e-3), `dropout` (0.2), `seed`. With `holdout=True` each user's last event is kept out of training, and training stops when NDCG@10 on those has not risen for `patience` epochs (5), keeping the best epoch. `max_seconds` stops it after the first epoch that ends past that many seconds. `log=print` prints each epoch's loss and metrics, which `out["history"]` also holds.
- It returns `{"path", "history", "vectors"}`. Pass the same `models=` to `Database.open`, or start the server with `--models`: a coordinator needs the model, its shards don't.

## A server from Python

`chloedb.connect(url, token=None, timeout=30, retry_for=10)` returns a `Client` of a running chloedb-server, using only the standard library. It has `schema`, `tables`, `query`, `sql`, `search`, `insert`, `upsert`, `delete`, `import_file` (an upload to [`/v1/import`](server.md#loading-rows), up to the server's `--max-body-mb`, 256 MiB unless set), `import_sqlite(path, tables=None)` and `plan_sqlite(path)` (a SQLite file's tables, uploaded the same way; [SQLite in and out](sqlite.md#over-http-and-in-the-ui)), `export_sqlite(path, tables=None)` (the server's tables downloaded as a SQLite file), `import_parquet(path, table=None, key=None)`, `plan_parquet(path)` and `export_parquet(path, table=None)` ([Parquet over HTTP](parquet.md#over-http-and-in-the-ui)) and `len()`, as above, plus `get(key)`, `get_many(keys)` and `put(key, row, if_seq_no=None)` for [rows by key](server.md#rows-by-key), `write(ops)` for raw [write operations](server.md#http-api), `explain`, `run(plan)` (a plan from `explain(...)["ir"]` or a `table()` query, run by [`/v1/plan`](server.md#http-api)), `table()` ([queries built in Python](#queries-built-in-python)), `embed(text, field=None)` (a text's vector by the server's model, from [`/v1/embed`](server.md#http-api), or `embed(image=data)` an image's and `embed(audio=data)` a sound's), `embed_tokens(text, field)` (a query's vectors a token, for a multi-vector field), `query_logged` and `export_rank_log` ([the rank log](#the-rank-log)), `health`, `stats` and `ask(question)` for the [agent](server.md#the-agent).

```python
client = chloedb.connect("http://127.0.0.1:7700")   # token= or $CHLOEDB_TOKEN for a server with --token
client.upsert({"id": "b3", "title": "Star letters", "year": 2024})
client.search("letters", limit=3).dicts()
```

A server answers at most `--max-result-window` rows a query (10,000 by default). `scroll(sql, params, page_size=10_000, arrow=False, keep_alive_ms=None)` reads past it: it yields the answer a page at a time, `Result`s or with `arrow=True` pyarrow Tables, each page at most `page_size` rows and at most the window, every page from the version of the database the first read ([scrolls](server.md#reading-past-the-window-scrolls)). Leaving the loop early ends the scroll on the server. A `Database` has `scroll` too, whose pages are slices of one answer, since a database in your process has no window.

```python
for page in client.scroll("SELECT id, title FROM books WHERE year >= :y", y=1950, page_size=5000):
    handle(page.dicts())
```

The server embeds `{"embed": text}` parameters and the rows it is sent with its own models. Errors the server reports raise `chloedb.ChloeDBError` (or a subclass of it, [below](#errors)), with the HTTP status as `.status`, the JSON answer as `.answer` and a stable `.code`; a server that cannot be reached raises `urllib.error.URLError`. A request the server refuses for its load (429: [behind on flushing](server.md#streams) its writes, its write queue full, too many queries running) did nothing, so the client sends it again after the `Retry-After` the server gives, for up to `retry_for` seconds (10; 0 sends each request once), and raises the 429 after that. A write that failed partway on a cluster (an error with `"partial": true`) is not sent again, since the shards that took it keep it, unless it has an idempotency key.

`write(ops, idempotency_key=k)` and `sql(statement, idempotency_key=k)` send a write with an [idempotency key](server.md#when-a-write-fails), so that sending it again does not apply it twice. A write that failed partway through a coordinator, after some shards took their part, says so in its error's answer and names its key:

```python
try:
    client.write(ops)
except chloedb.ChloeDBError as e:
    if not (e.answer or {}).get("partial"):
        raise
    client.write(ops, idempotency_key=e.answer["idempotency_key"])   # the rest, once
```

### Reading your own writes

A read from a [replica](server.md#replication-a-primary-and-its-replicas), or through a coordinator whose shards have replicas, can be answered by a copy that has not applied a write yet. The servers fix that with sequence numbers ([reads that see a write](server.md#reads-that-see-a-write)), when they number their writes (`--seq-no`):

- A write's count is a `chloedb.Written`: an `int` (it compares, adds, prints and serialises as one, so code that used the number goes on working) with the write's `seq_no` from a server, or `seq_nos`, one per shard, from a coordinator; both are None from a server without `--seq-no`. `write`, `insert`, `upsert`, `delete`, `put`, a write by `sql` and a write plan by `run` answer one; `insert` and the others that send several requests answer the latest numbers of all of them.
- `query`, `query_arrow`, `search`, `get`, `get_many`, `scroll`, a read by `sql`, a read plan by `run` and `sid_next` take `min_seq_no=` and `min_seq_nos=`: the copy that answers holds those writes. A replica that has not applied them waits up to a second, then answers 503, and a coordinator passes the read to the shard's primary; a server refuses a number past its last write (409).
- `client.session()` (or `chloedb.Session(url, ...)`) is a client that does this itself: it keeps the latest numbers its own writes answered and sends them with each of its reads, so reading your own writes through a coordinator or from a replica is one line. A session is safe to share between threads; `session.observe(written)` adds a write made by another client. Numbers given to a read explicitly are sent with the session's, the later of each.

```python
s = chloedb.connect("http://coordinator:7700").session()
s.upsert({"id": "u1", "name": "Ada"})
s.get("u1")                                  # sees the upsert, whichever copy answers

w = client.upsert({"id": "u2", "name": "Grace"})     # or explicitly, by its numbers
client.get("u2", min_seq_no=w.seq_no, min_seq_nos=w.seq_nos)
```

The numbers go with reads only: a server refuses them on a write (a write sees every write before it), so a session leaves them off an `INSERT`, `UPDATE` or `DELETE` by `sql`, judging a statement by its first word. A `Database` takes `min_seq_no` and `min_seq_nos` and ignores them: a database in your process reads every write it committed.

### Errors

Every error ChloeDB raises is a `chloedb.ChloeDBError` (`except chloedb.ChloeDBError` catches them all) with a stable `.code`, the same in your process and from a server: the server's JSON `code` when it gives one, else the kind the engine's message starts with (`busy: ...`), else the HTTP status. The common codes are raised as subclasses:

| code | class | when |
|---|---|---|
| `bad_request` | `BadRequestError` | a bad schema, query, value or request (HTTP 400, 413, 422); sending it again fails again |
| `unauthorized` | `UnauthorizedError` | no token, or one without the rights the request needs (401, 403) |
| `not_found` | `NotFoundError` | no such route, table or cursor (404); `get` of a missing key answers None instead |
| `conflict` | `ConflictError` | refused by the state it met (409): an `if_seq_no` that no longer holds, an idempotency key still running, a `min_seq_no` past the server's last write |
| `partial_write` | `PartialWriteError` | a coordinator wrote some shards and not others (`answer["partial"]`): send it again with the same `idempotency_key` |
| `busy` | `BusyError` | the database is open in another process, or another writer holds it |
| `overloaded` | `OverloadedError` | refused for the server's load before any work (429), after the client's retries |
| `timeout` | `DeadlineError` | a deadline passed: a query's `timeout_ms`, a coordinator's wait for a shard (504), the engine's own |
| `unavailable` | `UnavailableError` | no copy can serve it now (503): a replica behind, a primary replaced, a hosted model down |

Other codes have no subclass: `memory` (a query past its memory budget, [limits](#limits-memory-deadlines-interrupting)), `corrupt`, `io`, `internal` (another 5xx) and `error`. `chloedb.error_code(message, status, answer)` gives the code of any message. A server that cannot be reached still raises `urllib.error.URLError`, and a client's own timeout `TimeoutError`, as before.

### Database and Client: what differs

A script moves between `chloedb.open` and `chloedb.connect` unchanged as long as it keeps to what both have. `timeout_ms` means the same on both: a query's deadline ([limits](#limits-memory-deadlines-interrupting)). Options that only a server can honour raise `TypeError` on a `Database` rather than being taken as query parameters: `idempotency_key` (to `sql`, `write`), `keep_alive_ms` (to `scroll`), and `timeout_ms` to a method that runs no query (`plan`); a query parameter of one of those names goes in the `params` dict. The rest of the differences:

| | `Database` (in your process) | `Client` (a server) |
|---|---|---|
| a write's count | `int` | `Written`, an `int` with `seq_no` / `seq_nos` |
| `min_seq_no`, `min_seq_nos` | taken and ignored | sent: the answering copy holds those writes |
| `timeout_ms` | the query's deadline, none by default | sent: the query's deadline, the server's `--timeout-ms` (5 s) by default |
| `memory`, `interrupt()` | a query's budget; stops the running queries | the server's `--query-memory-mb` |
| `idempotency_key`, `keep_alive_ms` | `TypeError` | sent |
| `scroll` | slices of one answer | pages of a server-side scroll, past `--max-result-window` |
| `export_sqlite` | returns a dict | returns the tables' names |
| `export_parquet` | `tables=`, returns a dict | `table=`, returns the table's name |
| `import_sqlite`, `import_parquet` | take `schemas=` / `schema=` | the server's schemas |
| a plan | `plan(sql)` | `explain(sql)["ir"]` |
| only here | `optimize`, `vacuum`, `expire`, `refresh`, `add_column`, `drop_column`, `rename_column`, `import_arrow`, `embed_errors` | `put`, `stream`, `source`, `sources`, `plan_sqlite`, `plan_parquet`, `health`, `stats`, `ask`, `session` |
| errors | `ChloeDBError` subclasses, `status` None | the same classes and codes, with `status` and `answer` |
| unreachable | | `urllib.error.URLError` |

### Streams and Kafka

`client.stream(events)` writes an iterable of [stream lines](server.md#streams) through `/v1/stream` as it is read, each an operation or one event's operations as `{"ops": [...], "source": {"name", "offset", "ts"}}`, and returns the stream's last answer (`lines`, `written`, `skipped`). `client.source(name)` is the offset of the last event a source wrote (None for none), and `client.sources()` lists them all. A server skips the operations of an offset it has reached, so a stream resumed from `source(name) + 1`, or from anywhere before it, writes each event once.

`chloedb.ingest.kafka` is a Kafka (or Redpanda) consumer built on it, which needs `confluent-kafka` (`pip install confluent-kafka`, or `pip install "./chloedb-py[kafka]"`). Each partition is a source named `<topic>/<partition>` (after `prefix=`), and a partition given to the consumer is read from after the offset the server has, not from what the consumer group committed to Kafka, so each event is written once across consumer restarts, server restarts and rebalances. Within a partition the events are written in order.

```python
import json
import chloedb
from chloedb import ingest

def click(e):                                   # an ingest.Event: topic, partition, offset, timestamp, key, value
    c = json.loads(e.value)
    return [
        {"op": "insert", "table": "events", "doc": {"id": c["id"], "usr": c["usr"], "item": c["item"]}},
        {"op": "add", "table": "users", "key": c["usr"], "field": "n_events", "value": 1},
    ]

ingest.kafka(chloedb.connect("http://127.0.0.1:7700"), "clicks", click, brokers="127.0.0.1:9092", group="chloedb")
```

The function gives an event's operations, which commit together with its offset; None writes the offset alone (for events to leave out). Without one, each value is JSON: an operation, or `{"ops": [...]}`. The consumer runs until interrupted, or until `max_events=` events were read or none came for `idle=` seconds, and returns counts (`events`, `written`, `skipped`, `streams`, `retries`). It keeps a stream open for `seconds_per_stream=` (1 by default), then commits the stream's offsets to Kafka for Kafka's own lag tools and opens the next. When the server cannot be reached, or answers 429 or 5xx, it waits and reads every partition again from the server's offsets, for up to `retry_for=` seconds (60). An event the function fails on stops it after the events before it are written, so a restart begins with that event. `config=` adds librdkafka settings, and consumers of one `group` share the partitions. `ingest.consume(client, log, to_ops)` runs the same loop over another log, through the small `ingest.Log` interface.

## Search on the device

`chloedb.nearby(url, table, dir)` holds a [slice](slices.md) of a chloedb-server's table near a user: the rows of the tiles around them in a database in `dir`, searched with the server's statistics. `move_to(lat, lon)` fetches the tiles it lacks, and `search(q, k=10, d0=1000)` answers on the device when it can prove the answer is the server's, and asks the server otherwise; the result's `where` is `"device"` or `"server"` and `why` the reason. For a [vector slice](slices.md#vector-slices-more-like-this-on-the-device), `focus(taste, rows=20_000)` holds the clusters nearest a taste vector and `similar(vector, k=10)` answers a similarity search the same way, fetching the few clusters its codes name when that proves the answer.

```python
near = chloedb.nearby("http://127.0.0.1:7700", "places", "./slice")
near.move_to(40.7484, -73.9857)
r = near.search("pizza", k=10)
r.where, r.why            # ("device", "proved")
```
