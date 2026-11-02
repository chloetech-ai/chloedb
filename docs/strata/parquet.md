---
title: Parquet and S3
parent: Strata
nav_order: 7
---

# Parquet and S3
{: .no_toc }

Strata reads Parquet files, and directories of them as Spark, DuckDB, polars and pandas write them, into a table, making the table's schema from the columns' types and values when it does not exist, and writes a table to a Parquet file that any of those tools reads. An export keeps the table's Strata schema in the file, so importing it back gives the same schema and rows. Wherever a file is read or written, an `s3://` URL works too: an object, or every object under a prefix, on AWS S3 or any store that speaks its API.
{: .fs-5 .fw-300 }

1. TOC
{:toc}

## Import

From the command line, into a database no server has open:

```sh
strata-server import --db lake.strata events.parquet                    # a new database, its table named events
strata-server import --db lake.strata --table clicks out/clicks/        # a directory: every part file, one table
strata-server import --db lake.strata s3://lake/raw/events/             # every object under the prefix
strata-server import --db lake.strata --key event_id events.parquet     # the key of a table it creates
strata-server import --db lake.strata --plan events.parquet > plan.json # what it would do; writes nothing
strata-server import --db lake.strata --schema plan.json events.parquet # with the plan, edited
```

A Parquet file is recognised by its name (`.parquet`), its first bytes or `--format parquet`, and a directory by the Parquet files in it, at any depth; names starting with `_` or `.`, such as Spark's `_SUCCESS` and its `.crc` checksums, are passed over. The files of a directory make one table, read in the order of their paths, and may differ in columns: each file's columns fill the fields of their names. Parquet and SQLite files, and JSON and CSV files, can be mixed in one command. The rows go into the table `--table` names, else the one an [export](#export) kept in the file, else a table named for the file or directory (`events.snappy.parquet` and `events/` make `events`):

- **A table Strata has** keeps its schema: each column fills the field of its name (in any case), the others are left out and listed. The rows are upserted by key (inserted, in a table without one), 10,000 a transaction, or bulk-loaded when the table is empty.
- **A table Strata lacks** is created, with the schema `--schema` gives, or the one an export kept in the file, or else one made from the columns, and its rows are bulk-loaded in one commit: a value that does not fit is reported with its file and row, and nothing is written.

A database that does not exist is created with the first Parquet file's table. In Python, `strata.from_parquet("events.parquet", "lake.strata")` creates a database, `db.import_parquet("out/clicks/", table="clicks")` imports into an open one, and `strata.plan_parquet("events.parquet")` returns the plan ([Python](python.md)). From Rust it is `strata::parquet` (feature `parquet`): `import_new`, `import`, `plan` and `Source` for each step.

### How Parquet columns become fields

Parquet columns have types, so a table made from a file takes them, and reads the first 100,000 rows only to choose between a few:

| Parquet / Arrow column | Strata field |
|---|---|
| integers of any width, decimals with no scale | `INT` |
| floats, other decimals | `FLOAT` (a decimal as a double: the plan notes it) |
| booleans | `BOOL` |
| dates, timestamps of any unit or time zone | `TIMESTAMP` (a date at midnight UTC) |
| text that is the key, dictionary-encoded, or labels rather than prose | `KEYWORD` |
| other text | `TEXT` |
| fixed-size lists of numbers, or lists of numbers all of one length | `VECTOR(n)` |
| lists of text or of integers | multi-valued `KEYWORD` or `INT` |
| lists of vectors | multi-vector `VECTOR(n)[]` |
| structs, maps, other lists | `JSON` |
| binary | left out (Strata has no binary type), or base64 text in a stored `KEYWORD` with `--blobs-base64` |
| times of day, durations, intervals, columns of NULLs only | left out |

Text reads as labels when no value is over 256 bytes and the values have 2.5 words or fewer on average, or each value repeats ten times on average (as the [SQLite import](sqlite.md#how-sqlite-columns-become-fields) judges). Column names that are not Strata identifiers are respelled (`City Name` becomes `City_Name`; the plan lists each). A column named `id` (in any case) of integers or text without NULLs in the sample is the key; `--key` names another, and `--key ""` makes a table without one. A NaN or infinite float is read as NULL, and the import says how many were.

Into a table that exists, a column fills its field when its values do: any integer, float or decimal for a number, dates, timestamps and ISO 8601 text for a `TIMESTAMP`, a list of `n` numbers (or `4n` little-endian float32 bytes) for a `VECTOR(n)`, a list for a multi-valued field, structs, maps and lists for a `JSON` field. Integers that are Unix times (seconds, milliseconds or microseconds, by their size) fill a `TIMESTAMP` too.

### Directories and Hive partitions

A directory Spark writes with `partitionBy` holds its files under `key=value` directories (`year=2026/month=11/part-00000-….snappy.parquet`). Each key becomes a field, `INT` when every value is an integer and `KEYWORD` otherwise, and each row takes its file's values; `__HIVE_DEFAULT_PARTITION__` is NULL. Into a table that exists, a key fills the field of its name. A directory whose files keep different Strata tables (an export of several, below) is refused: import its files one at a time.

## Export

```sh
strata-server export --db lake.strata --tables events events.parquet    # one table, one file
strata-server export --db lake.strata out/                              # every table: out/<table>.parquet
strata-server export --db lake.strata --format parquet --tables events s3://lake/exports/events.parquet
strata-server export --db lake.strata --no-schemas events.parquet       # without the kept schema
```

An export reads the rows of one snapshot. It writes a `.parquet` file when the name says so (one table), and otherwise a new directory with a `<table>.parquet` file a table (`--format parquet`, or a name ending in `/`; without either, the export is a [SQLite file](sqlite.md#export)). The files are compressed with Zstandard, in row groups of about 128 MB, and written under another name and renamed into place when complete; an existing file or directory is an error, but for an empty directory. In Python it is `db.export_parquet("events.parquet", tables="events")`, from Rust `strata::parquet::export`.

| Strata field | Parquet / Arrow column |
|---|---|
| `INT` | `INT64` |
| `FLOAT` | `DOUBLE` |
| `BOOL` | `BOOLEAN` |
| `TIMESTAMP` | `TIMESTAMP(MICROS, UTC)` |
| `TEXT`, `KEYWORD` | `STRING` |
| `VECTOR(n)` | a list of `n` floats (Arrow `FixedSizeList(Float32, n)`, which NumPy, PyTorch and Spark's `array<float>` take) |
| `VECTOR(n)[]` | a list of those |
| multi-valued `KEYWORD`, `INT` | a list of `STRING`, of `INT64` |
| `JSON`, `SPARSE`, `EDGES`, `MEDIA` | `STRING`, the value's JSON text |

Only the key's column is not nullable. Each file keeps its table's Strata schema as JSON in its key-value metadata (`strata.schema`), which other tools ignore and an import restores: analyzers, vector indexes and their kinds, embedding models, references, defaults and generated fields' definitions, which Parquet has no place for. A field neither stored nor recoverable from an index is NULL, and a generated field holds its text; the export's notes list what does not carry over.

## S3 and S3-compatible stores

`s3://bucket/key` (or `s3a://`, as Spark writes) works wherever a file does: `strata-server import` and `export`, and in Python `from_parquet`, `import_parquet`, `export_parquet`, `from_sqlite`, `import_sqlite`, `export_sqlite` and `import_file`. A URL names one object, or every object under a prefix: one that ends in `/`, or that names no object (`s3://lake/raw/events` reads `raw/events/year=2026/part-0.parquet` and the rest). The objects are read into a temporary directory, keeping their paths under the prefix, eight at once, and imported from there; an export is written to a temporary file or directory and then sent, in parts of 16 MB for large files. An object already at the URL of an export, or under its prefix, is an error, checked before anything is sent.

The store is configured as the AWS tools are, by environment variables:

| variable | what |
|---|---|
| `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_SESSION_TOKEN` | the credentials; without them, a web identity (`AWS_WEB_IDENTITY_TOKEN_FILE` and `AWS_ROLE_ARN`, as EKS gives pods), ECS task credentials, or EC2 instance metadata |
| `AWS_REGION` (or `AWS_DEFAULT_REGION`) | the bucket's region |
| `AWS_ENDPOINT_URL` | a store other than AWS S3: `https://nyc3.digitaloceanspaces.com` for DigitalOcean Spaces, `https://<account>.r2.cloudflarestorage.com` for Cloudflare R2, MinIO's or Ceph's address |
| `AWS_ALLOW_HTTP=true` | for an endpoint over plain `http://`, such as a local MinIO |
| `AWS_VIRTUAL_HOSTED_STYLE_REQUEST=true` | `bucket.host` addresses instead of `host/bucket` |

Profiles in `~/.aws` are not read. The S3 client is [object_store](https://docs.rs/object_store)'s, in Strata's feature `s3` (on by default in strata-server and the Python package).

## Over HTTP and in the UI

`POST /v1/import` takes a Parquet file in the body (`application/vnd.apache.parquet`, or recognised by its first bytes; up to the server's `--max-body-mb`), into the table `?table=` names or the one an export kept in the file, with `?key=`, `?blobs_base64=true` and `?plan=true` as on the command line. The rows are checked before any is written and then written through the writer, so a value that does not fit writes nothing; an import that creates the table needs an admin key, and a coordinator imports into tables its shards have. `GET /v1/export?format=parquet&tables=events` answers one table as a Parquet file (a database of one table may leave out `tables`), and `?schemas=false` leaves out the kept schema. From Python's client: `client.import_parquet(path, table=None, key=None)`, `client.plan_parquet(path)` and `client.export_parquet(path, table=None)`.

The command center's Import view reads a dropped Parquet file's plan from the server: the table it goes to (named in a box, which re-plans), each column's field and type, the key and what is left out, then imports it with its progress. Its Export card writes SQLite or Parquet, a Parquet file a table chosen.

## Arrow from Python

`db.import_arrow(data, table=None)` writes the rows of anything that gives an Arrow stream (`__arrow_c_stream__`): a pyarrow Table, RecordBatch or RecordBatchReader, a polars DataFrame, a DuckDB relation, an answer of `query_arrow`, and a pandas DataFrame through pyarrow. Each column fills the field of its name, read as an import reads a Parquet column; the others are left out and listed. Into an empty table the rows are bulk-loaded in one commit, otherwise upserted by key. With `query_arrow` going the other way, a table moves between Strata and dataframe libraries without JSON in between.

## With Spark

Spark reads an export as it reads any Parquet (`spark.read.parquet("s3a://lake/exports/events.parquet")`): vectors as `array<float>`, timestamps as `timestamp`, lists as arrays, JSON as strings to `from_json`. What Spark writes (`df.write.partitionBy("year").parquet("s3a://lake/raw/events")`) imports as a directory, partitions and all. A data source that lets Spark read from and write to a running server is a later step.

## What it does not do

- **Read Parquet in place.** An import copies the rows into Strata's own files, which hold the indexes Parquet has none of; a query never reads a Parquet file.
- **Several tables from one directory.** A directory is one table, as Spark, DuckDB and polars read one; an export of several tables is a directory of files, imported a file at a time.
- **Partitioned exports.** An export writes one file a table, in row groups; it does not split a table by a column into `key=value` directories.
- **Stream from S3.** An import reads all the objects to local disk before it reads them, and an export writes the file locally before it sends it, so both need the space.
