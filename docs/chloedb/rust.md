# ChloeDB from Rust

The `chloedb` crate is ChloeDB itself: the Python, JavaScript and Java packages and chloedb-server are built on it. A Rust program that depends on it opens a database file in its own process, with no server, and reads and writes it through SQL or through typed write operations.

## Add it

The crate is not on crates.io yet. The `chloedb` there is a placeholder that only holds the name (it still says Strata, ChloeDB's name before November 2026). Depend on the repository instead, at a release's tag:

```toml
[dependencies]
chloedb = { git = "https://github.com/chloetech-ai/chloedb", tag = "chloedb-server-v0.3.8" }
```

Cargo finds the crate in the repository's `chloedb/` folder. Building it needs Rust (stable) and a C compiler (`xcode-select --install` on macOS, `build-essential` on Debian and Ubuntu), because the tokenizer's regex library is C. The first build compiles ChloeDB and its dependencies, which takes several minutes. The default features are `embed` (embedding models run in the process) and `hosted` (embedding models behind an HTTP API); `sqlite`, `parquet`, `s3`, `bert` and `onnx` add SQLite and Parquet import and export, `s3://` URLs, and more model kinds (`chloedb/Cargo.toml` describes each one). `cargo doc -p chloedb --open` shows the API of the version you depend on.

The examples below use these imports:

```rust
use std::path::Path;

use chloedb::db::{Db, SqlResult, WriteOp};
use chloedb::query::Params;
use chloedb::storage::DbOptions;
use chloedb::{Error, Value};
```

## Open or create a database

`Db::open` opens a database that exists. On a path where there is none, it fails with `I/O error: No such file or directory (os error 2)`, which does not name the path, so create the database first. `Db::create_empty` makes a database with no tables; SQL then creates them:

```rust
fn open(path: &Path) -> chloedb::Result<Db> {
    if path.exists() {
        Db::open(path, DbOptions::default())
    } else {
        Db::create_empty(path, DbOptions::default())
    }
}

fn main() -> chloedb::Result<()> {
    let db = open(Path::new("notes.chloedb"))?;
    db.sql("CREATE TABLE IF NOT EXISTS notes (id KEYWORD PRIMARY KEY, body TEXT, year INT, v VECTOR(3))", &Params::new())?;
    // ...
    db.close()?;
    Ok(())
}
```

`CREATE TABLE IF NOT EXISTS` leaves a table that is there as it is, so the program can run again on the same file. A database can hold many tables, and every statement names the tables it reads or writes ([the query language](query-language.md#columns-and-types) has the column types). `Db::create(path, schema, options)` makes a database whose first table is given as a `Schema` instead, for example from JSON:

```rust
let schema = chloedb::Schema::from_json(
    r#"{"table": "docs", "key": "id", "fields": [
        {"name": "id", "type": "keyword"}, {"name": "body", "type": "text"}, {"name": "n", "type": "int"}]}"#,
)?;
let db = Db::create(Path::new("docs.chloedb"), schema, DbOptions::default())?;
```

Call `db.close()` before the program lets go of the database. It writes the rows still in memory into the file, so that the file alone holds the database (a copy of it is then a backup), and gives back to the file system the space that merges freed. The database still answers after it.

A program that keeps writing for a long time also merges its segments in the background, as the Python and JavaScript packages do when they open a database: `db.spawn_merger(MergerOptions { threads: 1, ..MergerOptions::default() })`, with `chloedb::storage::MergerOptions`. Without it, a table written to for a long time collects segments, and each query visits every one.

## Write

SQL writes take parameters, named `:name` in the statement and set with `Params::set`:

```rust
let p = Params::new()
    .set("id", Value::Str("n1".into()))
    .set("body", Value::Str("Invoice totals are rounded to whole cents".into()))
    .set("v", Value::Vector(vec![1.0, 0.0, 0.0].into()));
db.sql("INSERT INTO notes (id, body, year, v) VALUES (:id, :body, 2026, :v)", &p)?;
db.sql("UPDATE notes SET year = 2025 WHERE id = 'n1'", &Params::new())?;
```

`db.sql` answers `SqlResult::Written(n)` for a write, with the number of rows it wrote.

Each SQL statement is a transaction of its own; `BEGIN` and `COMMIT` are refused. To write several rows in one transaction, all or none, give `db.write_ops` a list of operations. A document is the row's values in the order of the table's columns:

```rust
let doc = |id: &str, body: &str| vec![Value::Str(id.into()), Value::Str(body.into()), Value::Int(2026), Value::Vector(vec![0.0, 1.0, 0.0].into())];
db.write_ops(vec![
    WriteOp::Insert { table: "notes".into(), doc: doc("n2", "Prices are integer cents") },
    WriteOp::Upsert { table: "notes".into(), doc: doc("n3", "The server runs Python 3.8") },
    WriteOp::Delete { table: "notes".into(), key: Value::Str("n1".into()) },
])?;
```

`Insert` fails when a row with the key is there, and `Upsert` replaces it. `Set`, `Add` and `AddOrInsert` change one numeric field of a row in place. Threads that call `write_ops` at the same time share commits.

The storage layer has its own transaction, `db.storage().begin()`, for a program that reads its own writes before it commits. Name the table there with `tx.table("notes")`: `WriteTxn::insert` and `WriteTxn::upsert` write the database's first table, which in a database made by `Db::create_empty` is a hidden placeholder, so a row for `notes` fails with `schema error: document has 4 values, table has 1 fields`.

```rust
let mut tx = db.storage().begin()?;
let mut t = tx.table("notes")?;
t.upsert(vec![Value::Str("n4".into()), Value::Str("Rules come from the client".into()), Value::Int(2026), Value::Null])?;
tx.commit()?;
```

## Search and query

`db.sql` answers a `SELECT` with `SqlResult::Rows`, and `db.query` with the rows alone. `MATCH` filters by words, `bm25` ranks by them, and `similarity` ranks by a vector field:

```rust
let p = Params::new().set("q", Value::Str("integer cents".into()));
let SqlResult::Rows(r) = db.sql("SELECT id, score() FROM notes ORDER BY bm25(body, :q) DESC LIMIT 10", &p)? else { unreachable!() };
for row in &r.rows {
    println!("{} {}", row[0].as_str().unwrap_or(""), row[1].as_f64().unwrap_or(0.0));
}

let q = Params::new().set("q", Value::Vector(vec![0.9, 0.1, 0.0].into()));
let near = db.query("SELECT id FROM notes ORDER BY similarity(v, :q) DESC LIMIT 2", &q)?;

let n = db.query("SELECT count(*) FROM notes WHERE MATCH(body, 'cents')", &Params::new())?.rows[0][0].as_i64();
```

A result has its `columns` and its `rows`, and each value is a `Value` (`Null`, `Int`, `Float`, `Str`, `Vector`, `List`, `Bool`, `Timestamp`), read with `as_str`, `as_i64` and `as_f64`. [The query language](query-language.md) has every statement and function: [full text](query-language.md#full-text-match), [scores](query-language.md#scores) and their fusion with `rrf`, filters, grouping and joins.

The examples give vectors as values. A field that ChloeDB embeds itself (`VECTOR(...) OPTIONS(embed = 'body', model = ...)`) needs its model loaded first, with `db.open_embedders(&models_dir)`, as the Python package does when it opens a database.

## Errors

Every call returns `chloedb::Result<T>`, whose error is `chloedb::Error` (it implements `std::error::Error`, so `?` also works in a function that returns `Result<T, Box<dyn std::error::Error>>`). Its kind says what to do:

| variant | when |
|---|---|
| `Error::Io` | the file system failed, or the path does not exist (`Db::open`) |
| `Error::Schema` | a document or schema does not fit the table; a key the table holds already starts with `duplicate primary key` |
| `Error::Query` | a statement failed to parse, bind or plan |
| `Error::Busy` | another process, or another open in this one, has the database open for writing (`the database is open elsewhere: ...`, which goes on to say which <!-- since 0.3.8 -->), or another write held it for longer than `DbOptions::write_wait` (5 seconds) |
| `Error::Timeout`, `Error::Memory` | a query ran past its deadline or its memory budget |
| `Error::Corrupt` | the file failed a check |
| `Error::Unavailable` | a hosted embedding model did not answer |

```rust
match db.write_ops(vec![WriteOp::Insert { table: "notes".into(), doc: doc("n2", "again") }]) {
    Err(Error::Schema(m)) if m.starts_with("duplicate primary key") => println!("n2 is there already"),
    Err(e) => return Err(e),
    Ok(_) => {}
}
```

One process writes a database at a time. Other processes on the same machine can read it while it does, opened with `DbOptions { read_only: true, ..DbOptions::default() }`; their writes fail. For many processes, or other machines, run [chloedb-server](server.md) over the file instead.

## Where the examples come from

The calls on this page are the ones ChloeDB's own tests make, which CI compiles and runs: `chloedb/tests/empty_database.rs` (a database with no tables, then `CREATE TABLE`), `tests/sql_tables.rs` (SQL over several tables), `tests/many_writers.rs` (`write_ops` and `Error::Busy`), `tests/analyzer_languages.rs` (`MATCH` and `bm25` with parameters), `tests/alter_table.rs` (vectors as parameters and `similarity`) and `tests/cold_tier.rs` (`storage().begin()` and `tx.table`). They are the next place to look for how a call is used.
