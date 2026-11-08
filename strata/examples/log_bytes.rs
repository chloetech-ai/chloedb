//! Bytes a log line takes in Strata: loads a file of log records (one
//! JSON object a line: ts in microseconds, host, service, severity, pid,
//! body; `results/log-bytes/parse.py` makes them from Loghub's logs) into
//! a table shaped like an OpenTelemetry log record, merges it as the
//! background merger would end up (`Db::optimize`), and prints the bytes
//! per line of the database and of each kind of section, as JSON.
//!
//! ```text
//! log_bytes DB IN.jsonl [COLUMNS] [OPTIONS]
//! log_bytes DB --table NAME
//! ```
//!
//! `COLUMNS` is the table's column list after `id INT PRIMARY KEY, ts
//! TIMESTAMP NOT NULL,` (default: `host KEYWORD, service KEYWORD,
//! severity KEYWORD, pid INT, body TEXT`), so variants (`body TEXT
//! OPTIONS(positions = false)`, a table without `pid`) take the same
//! records. A column the records lack is NULL. A `COLUMNS` that names
//! `ts` is the whole list (`id INT, ts TIMESTAMP NOT NULL, ...`: a table
//! without a key). `OPTIONS` are the table's (`compression = 'zstd'`).
//!
//! `--table NAME` measures a table of an existing database instead, as
//! another writer left it (strata-server's `otel_logs`, say): it is
//! merged the same way, and its lines are its rows.
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::Instant;

use strata::db::Db;
use strata::query::Params;
use strata::storage::DbOptions;
use strata::{FieldType, Value};

const DEFAULT_COLUMNS: &str = "host KEYWORD, service KEYWORD, severity KEYWORD, pid INT, body TEXT";
const BATCH: usize = 500_000;

/// The kind of a section, for a breakdown: `text.<field>.post` reads
/// `text.body.post`, the store and the keys are their own kinds.
fn kind(schema: &strata::Schema, name: &str) -> String {
    let label = strata::segment::label(schema, name);
    match label.split('.').collect::<Vec<_>>().as_slice() {
        ["store", ..] => "store".into(),
        ["pk"] | ["fields"] => label,
        [component, field, part, ..] => format!("{component}.{field}.{part}"),
        [component, field] => format!("{component}.{field}"),
        _ => label,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (db_path, input) = (Path::new(&args[1]), Path::new(&args[2]));
    if args[2] == "--table" {
        let db = Db::open(db_path, DbOptions::default()).unwrap();
        let table = &args[3];
        let schema = db.catalog().into_iter().find(|s| &s.table == table).unwrap();
        let t = Instant::now();
        db.optimize().unwrap();
        let merge_s = t.elapsed().as_secs_f64();
        let strata::db::SqlResult::Rows(n) = db.sql(&format!("SELECT COUNT(*) FROM {table}"), &Params::new()).unwrap() else { panic!("no count") };
        let Value::Int(lines) = n.rows[0][0] else { panic!("no count") };
        let lines = lines as u64;
        let mut out = report(&db, &schema, lines);
        out["input"] = serde_json::json!(format!("table {table}"));
        out["merge_seconds"] = serde_json::json!((merge_s * 10.0).round() / 10.0);
        println!("{out}");
        return;
    }
    let columns = args.get(3).map_or(DEFAULT_COLUMNS, |c| c.as_str());
    let options = args.get(4).map_or("", |o| o.as_str());
    let opts = DbOptions { memtable_max_docs: 1_000_000_000, ..DbOptions::default() };
    let db = Db::create_empty(db_path, opts).unwrap();
    let list = if columns.split(',').any(|c| c.split_whitespace().next() == Some("ts")) { columns.to_string() } else { format!("id INT PRIMARY KEY, ts TIMESTAMP NOT NULL, {columns}") };
    let with = if options.is_empty() { String::new() } else { format!(" OPTIONS({options})") };
    db.sql(&format!("CREATE TABLE logs ({list}){with}"), &Params::new()).unwrap();
    let schema = db.catalog().into_iter().find(|s| s.table == "logs").unwrap();
    let names: Vec<(String, FieldType)> = schema.fields.iter().map(|f| (f.name.clone(), f.ty.clone())).collect();

    let t = Instant::now();
    let (mut lines, mut input_bytes, mut body_bytes) = (0u64, 0u64, 0u64);
    let mut batch = Vec::with_capacity(BATCH);
    for line in BufReader::new(std::fs::File::open(input).unwrap()).lines() {
        let line = line.unwrap();
        input_bytes += line.len() as u64 + 1;
        let r: serde_json::Value = serde_json::from_str(&line).unwrap();
        let doc: Vec<Value> = names
            .iter()
            .map(|(name, ty)| match (name.as_str(), &r[name.as_str()]) {
                ("id", _) => Value::Int(lines as i64),
                ("ts", v) => Value::Timestamp(v.as_i64().unwrap()),
                (_, serde_json::Value::Null) => Value::Null,
                (_, v) => match ty {
                    FieldType::Int => Value::Int(v.as_i64().unwrap()),
                    _ => Value::Str(v.as_str().map_or_else(|| v.to_string(), str::to_string)),
                },
            })
            .collect();
        body_bytes += r["body"].as_str().map_or(0, |b| b.len() as u64);
        batch.push(doc);
        lines += 1;
        if batch.len() == BATCH {
            db.load_into("logs", std::mem::take(&mut batch)).unwrap();
        }
    }
    if !batch.is_empty() {
        db.load_into("logs", batch).unwrap();
    }
    let load_s = t.elapsed().as_secs_f64();
    let t = Instant::now();
    db.optimize().unwrap();
    let merge_s = t.elapsed().as_secs_f64();

    let mut out = report(&db, &schema, lines);
    let per_line = |b: u64| (b as f64 / lines as f64 * 10.0).round() / 10.0;
    let o = out.as_object_mut().unwrap();
    o.insert("input".into(), serde_json::json!(input.file_name().unwrap().to_string_lossy()));
    o.insert("columns".into(), serde_json::json!(list));
    o.insert("options".into(), serde_json::json!(options));
    o.insert("input_bytes_per_line".into(), serde_json::json!(per_line(input_bytes)));
    o.insert("body_bytes_per_line".into(), serde_json::json!(per_line(body_bytes)));
    o.insert("load_seconds".into(), serde_json::json!((load_s * 10.0).round() / 10.0));
    o.insert("merge_seconds".into(), serde_json::json!((merge_s * 10.0).round() / 10.0));
    println!("{out}");
}

/// The bytes per line of the database and of each kind of section of
/// `schema`'s table, merged, holding `lines` lines.
fn report(db: &Db, schema: &strata::Schema, lines: u64) -> serde_json::Value {
    let snap = db.storage().snapshot().table(&schema.table).unwrap();
    let mut by_kind: BTreeMap<String, u64> = BTreeMap::new();
    let mut image = 0u64;
    for s in snap.segments() {
        image += s.file.image().len() as u64;
        for (name, len) in s.file.section_sizes() {
            *by_kind.entry(kind(schema, &name)).or_default() += len as u64;
        }
    }
    let file_bytes = db.storage().file_stats().file_bytes;
    let per_line = |b: u64| (b as f64 / lines as f64 * 10.0).round() / 10.0;
    serde_json::json!({
        "lines": lines,
        "segments": snap.segments().len(),
        "file_bytes": file_bytes,
        "file_bytes_per_line": per_line(file_bytes),
        "image_bytes_per_line": per_line(image),
        "sections_bytes_per_line": by_kind.iter().map(|(k, b)| (k.clone(), serde_json::json!(per_line(*b)))).collect::<serde_json::Map<_, _>>(),
    })
}
