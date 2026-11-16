# SQLite in and out

ChloeDB imports the tables of a SQLite database, making a ChloeDB table of each with a schema read from SQLite's declarations and the values the columns hold, and exports a ChloeDB database to a new SQLite file that any SQLite program can read: rows, keys, references, indexes and an FTS5 index over the text. An export keeps each table's ChloeDB schema in the file, so importing it back gives the same schemas and rows.

Parquet files go in and out the same way, and a SQLite file may be an `s3://` URL too: [Parquet and S3](parquet.md).

## Import

From the command line, into a database no server has open:

```sh
chloedb-server import --db shop.chloedb shop.sqlite             # a new database from every table
chloedb-server import --db shop.chloedb --tables orders,customers shop.sqlite
chloedb-server import --db shop.chloedb --plan shop.sqlite > plan.json  # what it would do; writes nothing
chloedb-server import --db shop.chloedb --schema plan.json shop.sqlite  # with the plan, edited
```

A SQLite file is recognised by its first bytes (or `--format sqlite`), and can be mixed with JSON and CSV files in one command (`--table` names the table of those files' rows). A database that does not exist is created from the first SQLite file's tables. Each SQLite table goes into the ChloeDB table of its name, spelled as a ChloeDB identifier (`Order Details` becomes `Order_Details`, and column names likewise; a name that is already an identifier keeps it, and a respelled one that meets another takes a suffix, so `x-y` beside `x_y` is `x_y_2`):

- **A table ChloeDB has** keeps its schema: each column fills the field of its name, the others are left out and listed. The rows are upserted by key (inserted, in a table without one), 10,000 a transaction, or bulk-loaded when the table is empty.
- **A table ChloeDB lacks** is created, with the schema `--schema` gives for it, or the one an [export](#export) kept for it, or else one made from the SQLite table, and its rows are bulk-loaded in one commit: a value that does not fit is reported with its table and row, and nothing of that table is written.

Tables are created referenced tables first. In Python, `chloedb.from_sqlite("shop.sqlite", "shop.chloedb")` creates a database, `db.import_sqlite("shop.sqlite", tables=["orders"])` imports into an open one, and `chloedb.plan_sqlite("shop.sqlite")` returns the plan ([Python](python.md)). From Rust it is `chloedb::sqlite` (feature `sqlite`): `import_new`, `import`, `plan` and `Source` for each step.

### How SQLite columns become fields

SQLite has five storage classes and lets any column hold any of them, so the importer reads every row once before it loads them, to see what each column holds, and decides by its declared type and its values:

| SQLite column | ChloeDB field |
|---|---|
| a type naming `INT` (SQLite's integer affinity) | `INT` |
| `REAL`, `FLOAT`, `DOUBLE`, `DECIMAL`, `NUMERIC` | `FLOAT` (SQLite keeps `12.00` in a `DECIMAL` column as the integer 12; it stays a float) |
| `BOOL`, `BOOLEAN` holding 0, 1, `true` or `false` | `BOOL` |
| `DATE`, `DATETIME`, `TIMESTAMP` holding ISO 8601 text, Unix time or Julian days | `TIMESTAMP` |
| text in a column an FTS3, FTS4 or FTS5 table indexes | `TEXT`, analysed `english` for a `porter` tokenizer and `simple` otherwise |
| other text: a primary key, a foreign key, a column of an index, or labels rather than prose (none over 256 bytes, and 2.5 words or fewer on average, or at least 100 values repeating each value ten times on average); a word holds a letter, so phone numbers have none | `KEYWORD` |
| other text | `TEXT` |
| `F32_BLOB(n)`, `FLOAT32(n)` or `VECTOR(n)` holding little-endian float32 blobs (as libSQL and sqlite-vec write them) or JSON arrays | `VECTOR(n)` |
| `JSON` holding arrays of strings or integers | a multi-valued `KEYWORD` or `INT` |
| no declared type | from the values: `INT`, `FLOAT`, or text as above |
| other `BLOB` | left out (ChloeDB keeps no bytes), or base64 text in a stored, unindexed `KEYWORD` with `--blobs-base64` |

A column whose values break its declared type, such as `'n/a'` in an `INTEGER` column, becomes text, its numbers with it. Integers in a date column are Unix time in seconds, milliseconds or microseconds, chosen by the largest one (up to 10^11 seconds, the year 5138; up to 10^14 milliseconds); reals under 10^7 are Julian days. In a `FLOAT` column, an integer past 2^53 rounds to the nearest double (9223372036854775807 reads as 9223372036854775808), and an infinite real is `NULL` (a `NOT NULL` column holding one may hold `NULL`). A vector holding NaN or an infinity (as a float32: a JSON number past 3.4e38 is one) is `NULL` too, since a `VECTOR` holds finite numbers, and text that is not UTF-8 reads each byte that is not as U+FFFD. The plan's notes say when any of these happens, with how many values.

Constraints carry over where ChloeDB has them:

- **Keys.** A single-column `INTEGER` or text primary key is the table's key. A composite key, a key on another type, one holding NULLs (SQLite allows them outside `INTEGER PRIMARY KEY`), or one holding two values that are one `KEYWORD` (the integer 1 and the text `'1'`, which a column without a type keeps apart) leaves the table without a key, and the notes say why.
- **References.** A single-column foreign key to an imported table's key is a reference ([tables](query-language.md#tables)); integer codes referencing a text key become text to match it.
- **`NOT NULL` and `DEFAULT`.** Both carry over. A default is kept when it is a constant of the field's type, or `CURRENT_TIMESTAMP` (and `CURRENT_DATE`, `datetime('now')`) on a timestamp, which becomes `"now"`.
- **Indexes** only steer the choice of `KEYWORD` over `TEXT`: ChloeDB indexes every field.

Left out, and listed in the plan's notes: `UNIQUE` constraints other than the key (not enforced), `CHECK` constraints, triggers, collations, views, composite foreign keys, virtual tables other than FTS tables that hold their own text (an FTS table over another table's text marks those columns `TEXT`; a contentless one holds no text), and tables with no column to import (only BLOBs, without `--blobs-base64`).

A virtual table keeps its data in tables of its own, which SQLite marks as `shadow` for the modules it has (FTS3, FTS4, FTS5 and R-tree: `notes_data`, `geo_node`), and these are never imported; a table that only begins with a virtual table's name, such as `notes_archive` beside `notes`, is a table like the rest. Vector tables of sqlite-vec (`vec0`) need its extension, which the importer does not load, and SQLite cannot tell their own tables from others: beside `CREATE VIRTUAL TABLE vecs USING vec0(...)`, `vecs_chunks` and every `vecs_...` table are left out and listed, and imported when named (`--tables vecs_chunks`).

### Damaged files and files being written

The importer checks the file before it reads a row (`PRAGMA quick_check`), and refuses one that is damaged, saying where: a copy taken while a program was writing it is the usual cause, and `sqlite3 live.db ".backup copy.db"` or `VACUUM INTO 'copy.db'` makes one that is whole. A file whose writer stopped mid-transaction, with its `-journal` beside it, is refused until SQLite rolls the transaction back, which it does when a program that may write opens the file (`sqlite3 file.db 'PRAGMA quick_check'`). From the command line, the importer reads a database in WAL mode with its `-wal` file, as SQLite does; an upload is the main file alone, so a server's answer has a note when the file is in WAL mode: checkpoint it first (`PRAGMA wal_checkpoint(TRUNCATE)`) or send a copy made with `VACUUM INTO`. The importer reads each table twice, once to plan and once to load, and each read sees one moment of it: rows another program writes in between are imported though the plan did not see them, and one that does not fit the plan stops the import with its row, so import a copy of a database that is being written (`.backup` gives every table the same moment). A file another program holds locked is waited for up to 5 seconds.

### The plan

`--plan` (`chloedb.plan_sqlite` in Python, `chloedb::sqlite::plan` in Rust) prints what an import would do without writing anything:

```json
{
  "tables": [
    {
      "source": "Order Details",
      "table": "Order_Details",
      "fields": [
        {"name": "OrderID", "type": "int", "references": "Orders"},
        {"name": "ProductID", "type": "int", "references": "Products"},
        {"name": "UnitPrice", "type": "float", "not_null": true, "default": 0}
      ],
      "exists": false,
      "columns": {"OrderID": "OrderID", "ProductID": "ProductID", "UnitPrice": "UnitPrice"},
      "notes": ["table \"Order Details\" is Order_Details",
                "composite primary key (OrderID, ProductID): a key is one field, so the table has none"]
    }
  ],
  "skipped": {"Alphabetical list of products": "a view (ChloeDB has no views)"}
}
```

Each table is a [short-form schema](server.md#schemas) with its `source` table, the `columns` read (SQLite column: field) and `notes` on what did not carry over. Edit it (a field's type, analyzer, `multi`, a vector's index, the table's name) and give it back with `--schema`: a schema applies to the SQLite table its `source` names, or else to the one of its name, and the columns fill the fields of their names. `--schema` also takes one schema or an array of them.

## Over HTTP and in the UI

A running server imports a SQLite file sent to `POST /v1/import` as `?format=sqlite` (or with the content type `application/vnd.sqlite3`, or no format at all: the server knows the file by its first bytes), up to `--max-body-mb` (256 MiB):

```sh
curl -s -X POST 'localhost:7700/v1/import?format=sqlite&plan=true' -H 'content-type: application/vnd.sqlite3' --data-binary @shop.sqlite
curl -s -X POST 'localhost:7700/v1/import?format=sqlite&tables=orders,customers' -H 'content-type: application/vnd.sqlite3' --data-binary @shop.sqlite
# {"tables": [{"source": "customers", "table": "customers", "rows": 91, "created": true, "bulk": false, "notes": []}, ...], "skipped": {...}, "written": 921, "took_ms": 48.2}
```

`?plan=true` answers the plan and writes nothing, `?tables=` picks tables and `?blobs_base64=true` keeps BLOB columns. The server makes the plan and reads every row once before it writes, so that a value that does not fit writes nothing; then it creates each table it lacks, as `CREATE TABLE` does, and upserts the rows through its writer 1,000 at a time, as an import of rows does (the rows before a failed write stay written, and the error says how many). Once it writes, an import goes on to its end though the client hangs up or times out, and the server logs each import with its rows and seconds. Two imports at once of a table the server lacks both succeed, one creating it; a table without a key gets the rows of each. A table the server has keeps its schema, so to import with schemas of your own, create the tables first with `CREATE TABLE` and import into them. A coordinator creates a table it lacks on every shard, as `CREATE TABLE` through it does, and imports into it; a primary of [replicas](server.md#replication-a-primary-and-its-replicas) ships the tables it creates to them with the rows.

`GET /v1/export` sends the database as a new SQLite file, written from one snapshot to a temporary file and streamed from it: `?tables=a,b` for some tables, `?fts=false`, `?indexes=false` and `?schemas=false` as `--no-fts`, `--no-indexes` and `--no-schemas` below. A coordinator holds no files, so it exports no SQLite file; it exports one table's rows as JSON lines, CSV, Arrow or Parquet (`?format=jsonl`, `csv`, `arrow`, `parquet` with `?tables=T`, read by a scroll over its shards, with read on the table: [cluster limits](cluster-limits.md)).

```sh
curl -s 'localhost:7700/v1/export?tables=orders' -o orders.sqlite
```

In the query console (`/classic`), the **Schema and import** tab has both: a SQLite file to preview (the plan, table by table) and import, and a button that downloads the database as SQLite. The Python `Client` has `import_sqlite`, `plan_sqlite` and `export_sqlite` ([Python](python.md#a-server-from-python)).

## Export

```sh
chloedb-server export --db shop.chloedb shop.sqlite                 # every table
chloedb-server export --db shop.chloedb --tables orders --no-fts orders.sqlite
```

In Python, `db.export_sqlite("shop.sqlite")`; in Rust, `chloedb::sqlite::export`. The rows of every table come from one snapshot, and the file is written under another name and renamed into place when complete; an existing file is not overwritten. Each table becomes a SQLite table of the same name:

| ChloeDB field | SQLite column |
|---|---|
| `KEYWORD`, `TEXT` | `TEXT`; the key is `TEXT PRIMARY KEY NOT NULL` |
| `INT` | `INTEGER`; the key is `INTEGER PRIMARY KEY`, the rowid |
| `FLOAT` | `REAL` |
| `BOOL` | `BOOLEAN`, 0 or 1 |
| `TIMESTAMP` | `TIMESTAMP`, UTC text the way SQLite's date functions write it: `2026-10-01 12:30:05.250` |
| `VECTOR(n)` | `F32_BLOB(n)`: n little-endian float32 values, which libSQL's vector functions and sqlite-vec read as they are |
| multi-valued `KEYWORD` or `INT` | `JSON`, an array (`json_each` lists its values) |
| `MEDIA` | `JSON`, the file's description (`uri`, `sha256`, `mime`, ...); the files stay where their URIs point |
| `EDGES` | `JSON`, an array of keys |
| `SPARSE` | `JSON`, an object of term weights |

With them:

- **References** become `REFERENCES` clauses, and `NOT NULL` and `DEFAULT` carry over (`"now"` is `CURRENT_TIMESTAMP`). ChloeDB does not enforce references, so a row may name a key that is gone; SQLite checks foreign keys only when a connection turns them on (`PRAGMA foreign_keys = ON`), and then only on later writes.
- **B-tree indexes** on each indexed single-valued `KEYWORD`, `INT`, `FLOAT`, `BOOL` and `TIMESTAMP` field other than the key, named `<table>_<field>_idx`, and on a `MEDIA` field's `json_extract(field, '$.sha256')`, as ChloeDB indexes it (`--no-indexes` leaves them out).
- **An FTS5 index** over each table's `TEXT` fields, `<table>_fts`, with its text in the table itself (`content=`) and triggers that keep it in step with inserts, updates and deletes (`--no-fts` leaves it out). Search it as FTS5 is searched: `SELECT t.* FROM t_fts JOIN t ON t.rowid = t_fts.rowid WHERE t_fts MATCH 'wizard' ORDER BY rank`.
- **The ChloeDB schemas** in a table `_strata_tables` (`name`, `position`, `schema` in the short JSON form), which an import of the file reads to restore them (`--no-schemas` leaves it out; an import then makes schemas from the SQLite tables). A table of that name without the `name` and `schema` columns is the file's own, and imported as the others are.

### What SQLite does not hold

The data round-trips; ChloeDB's indexes and search do not, since SQLite has no counterpart:

- **Vector indexes.** HNSW and IVF indexes and the compressed codes stay behind; the vectors are exported. SQLite searches them with an extension (sqlite-vec, libSQL's vector index) or by a scan. An embedded field's model is recorded in the kept schema only.
- **Text analysis and ranking.** FTS5 tokenizes on its own: `porter unicode61` stands in for the `english` analyzer (ChloeDB stems with Snowball English and drops stopwords; Porter stems a little differently and keeps them), `unicode61` for `simple`, and a table has one tokenizer for all its text fields. FTS5's `bm25()` is not ChloeDB's BM25, and fused, decayed and boosted scores, typo tolerance and synonyms have no SQLite form.
- **Graphs.** `EDGES` are JSON arrays of keys; walks and PageRank over them are queries SQLite cannot run without recursive SQL of your own.
- **Generated fields** export their text as a plain column, not as a SQLite generated column; an import with the kept schema recomputes it.
- **Fields that are not stored** hold what the indexes give back, often NULL.
- **ChloeDB's file features**: segments, snapshots, the write-ahead log, replication and sequence numbers.
