//! Parquet files into Strata tables, and Strata tables out to Parquet
//! (feature `parquet`).
//!
//! [`import`] reads a Parquet file, or a directory of them (what Spark,
//! DuckDB, polars and pandas write: `part-*.parquet` files, Hive
//! partitions `key=value/` among them), into one Strata table. A table
//! Strata lacks is created, with the schema an export kept in the file
//! ([`META_KEY`]), the one given ([`ImportOptions::schema`]), or one made
//! from the Parquet columns and a sample of their values ([`SAMPLE`]
//! rows); a table it has keeps its schema, and takes the columns whose
//! names match fields. Rows go into an empty table in one bulk load (all
//! or nothing), and are otherwise upserted by key. [`plan`] says what an
//! import would do without doing it.
//!
//! | Parquet / Arrow column | Strata field |
//! |---|---|
//! | integers of any width, decimals with no scale | `INT` |
//! | floats, other decimals | `FLOAT` (decimals as doubles: the plan notes it) |
//! | booleans | `BOOL` |
//! | dates, timestamps of any unit or time zone | `TIMESTAMP` (dates at midnight UTC) |
//! | text: the key, dictionary-encoded, or labels rather than prose (none over 256 bytes, and 2.5 words or fewer on average, or each value repeated ten times on average) | `KEYWORD` |
//! | other text | `TEXT` |
//! | fixed-size lists of numbers, or lists of numbers all of one length | `VECTOR(n)` |
//! | lists of text or of integers | multi-valued `KEYWORD` or `INT` |
//! | lists of vectors | multi-vector `VECTOR(n)[]` |
//! | structs, maps, other lists | `JSON` |
//! | binary | skipped (Strata has no binary type), or base64 text with [`ImportOptions::blobs_base64`] |
//! | Hive partition `key=value` directories | a column per key: `INT` when every value is an integer, else `KEYWORD` |
//! | times of day, durations, intervals, nulls only | skipped |
//!
//! Column names that are not Strata identifiers are respelled (`x-y` is
//! `x_y`). A column named `id` (in any case) of integers or text without
//! NULLs in the sample is the key of a table the import creates;
//! [`ImportOptions::key`] names another, or none. A NaN or infinite float
//! is read as NULL, and the import says how many were.
//!
//! [`export`] writes each table to a Parquet file of its own, rows read
//! from one snapshot, compressed with Zstandard, in row groups of about
//! [`ROW_GROUP_BYTES`]:
//!
//! | Strata field | Parquet / Arrow column |
//! |---|---|
//! | `INT` | `INT64` |
//! | `FLOAT` | `DOUBLE` |
//! | `BOOL` | `BOOLEAN` |
//! | `TIMESTAMP` | `TIMESTAMP(MICROS, UTC)` |
//! | `TEXT`, `KEYWORD` | `STRING` |
//! | `VECTOR(n)` | a list of `n` `FLOAT` (Arrow: `FixedSizeList(Float32, n)`) |
//! | `VECTOR(n)[]` | a list of those |
//! | multi-valued `KEYWORD`, `INT` | a list of `STRING`, `INT64` |
//! | `JSON`, `SPARSE`, `EDGES`, `MEDIA` | `STRING`, the value's JSON text |
//!
//! Each file keeps its table's Strata schema in its key-value metadata
//! ([`META_KEY`]), so an import of it restores the table as it was:
//! analyzers, vector indexes, embedding models, references and generated
//! fields' definitions, which Parquet itself has no place for. Other tools
//! ignore the metadata.
//!
//! The conversions between rows and Arrow batches are
//! [`crate::arrow_rows`]'s.

mod export;
mod import;

pub use export::{export, ExportOptions, Exported, TableExport, ROW_GROUP_BYTES};
pub use crate::arrow_rows::nonfinite_notes;
pub use import::{import, import_new, plan, ImportOptions, Imported, Plan, Source, SAMPLE};

/// The key of the file metadata that holds the table's Strata schema, as
/// short JSON ([`crate::schema::Schema::to_short_json`]).
pub const META_KEY: &str = "strata.schema";

/// The first (and last) 4 bytes of every Parquet file.
pub const MAGIC: &[u8; 4] = b"PAR1";

/// Whether `path` starts as a Parquet file does.
pub fn is_parquet_file(path: &std::path::Path) -> bool {
    use std::io::Read;
    let mut magic = [0u8; 4];
    path.is_file() && std::fs::File::open(path).and_then(|mut f| f.read_exact(&mut magic)).is_ok() && magic == *MAGIC
}

/// Whether `path` is Parquet: a file named `.parquet` or starting as
/// Parquet does, or a directory holding such files (at any depth).
pub fn is_parquet(path: &std::path::Path) -> bool {
    if path.is_dir() {
        return import::files_under(path).is_ok_and(|f| !f.is_empty());
    }
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("parquet")) || is_parquet_file(path)
}

/// `name` as a Strata identifier: letters, digits and underscores, not
/// starting with a digit.
fn ident(name: &str) -> String {
    let mut s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
    if s.is_empty() || s.starts_with(|c: char| c.is_ascii_digit()) {
        s.insert(0, '_');
    }
    s
}

/// `name`, or `name_2`, `name_3`..: the first not in `used` (in any case),
/// which it joins.
fn distinct(name: String, used: &mut std::collections::HashSet<String>) -> String {
    let mut n = name.clone();
    let mut i = 2;
    while !used.insert(n.to_ascii_lowercase()) {
        n = format!("{name}_{i}");
        i += 1;
    }
    n
}
