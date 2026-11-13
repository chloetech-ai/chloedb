//! What a substring index costs and gives on a log: loads a file of log
//! records (as `log_bytes` reads them: one JSON object a line, made by
//! `results/log-bytes/parse.py` from Loghub's logs) into two tables that
//! differ in one option, `body TEXT` and `body TEXT OPTIONS(substring =
//! true)`, merges each to one segment, and runs every pattern of a file on
//! both: on the first each row's stored body is tested, on the second the
//! index answers. Prints, as JSON, the bytes of each database and of the
//! index, the seconds each took to build, and per pattern the rows it
//! selects and the median time of a count and of the first ten rows.
//!
//! ```text
//! substring_bench DIR IN.jsonl PATTERNS.txt
//! ```
//!
//! `PATTERNS.txt` has a pattern a line, as `LIKE` or `ILIKE`, a tab, and
//! the pattern (`LIKE<tab>%Failed password%`), or `WHERE`, a tab and a
//! whole condition (a pattern beside other conditions). The databases are kept in
//! `DIR` (`plain.strata`, `substring.strata`) and reused by a second run.
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::Instant;

use chloedb::db::Db;
use chloedb::query::Params;
use chloedb::storage::DbOptions;
use chloedb::{FieldType, Value};

const BATCH: usize = 500_000;
/// Runs of a query the index answers, and of one that reads every row.
const RUNS: usize = 7;
const SCAN_RUNS: usize = 3;

fn round(x: f64, digits: i32) -> f64 {
    let m = 10f64.powi(digits);
    (x * m).round() / m
}

/// Loads `input` into a new database at `path` whose `body` has `options`,
/// and merges it; the seconds of the load and of the merge, the lines and
/// the bytes of their bodies.
fn build(path: &Path, input: &Path, options: &str) -> (f64, f64, u64, u64) {
    let db = Db::create_empty(path, DbOptions { memtable_max_docs: 1_000_000_000, ..DbOptions::default() }).unwrap();
    db.sql(&format!("CREATE TABLE logs (id INT PRIMARY KEY, ts TIMESTAMP NOT NULL, host KEYWORD, service KEYWORD, severity KEYWORD, pid INT, body TEXT{options})"), &Params::new()).unwrap();
    let schema = db.catalog().into_iter().find(|s| s.table == "logs").unwrap();
    let names: Vec<(String, FieldType)> = schema.fields.iter().map(|f| (f.name.clone(), f.ty.clone())).collect();
    let t = Instant::now();
    let (mut lines, mut body_bytes) = (0u64, 0u64);
    let mut batch = Vec::with_capacity(BATCH);
    for line in BufReader::new(std::fs::File::open(input).unwrap()).lines() {
        let r: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
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
    (load_s, t.elapsed().as_secs_f64(), lines, body_bytes)
}

/// The bytes of the sections of `db`'s table whose label starts with each
/// of `prefixes` (and ends with what follows a `*` in it), and its
/// segments.
fn sizes(db: &Db, prefixes: &[&str]) -> (Vec<u64>, usize) {
    let schema = db.catalog().into_iter().find(|s| s.table == "logs").unwrap();
    let snap = db.snapshot().table("logs").unwrap();
    let mut out = vec![0u64; prefixes.len()];
    for s in snap.segments() {
        for (name, len) in s.file.section_sizes() {
            let label = chloedb::segment::label(&schema, &name);
            for (i, p) in prefixes.iter().enumerate() {
                let (start, end) = p.split_once('*').unwrap_or((p, ""));
                if label.starts_with(start) && label.ends_with(end) {
                    out[i] += len as u64;
                }
            }
        }
    }
    (out, snap.segments().len())
}

/// The parts of the substring index of `body`: how many, the bytes of
/// their text, the runs of their transforms and their samples.
fn parts(db: &Db) -> (usize, u64, u64, u64) {
    let schema = db.catalog().into_iter().find(|s| s.table == "logs").unwrap();
    let snap = db.snapshot().table("logs").unwrap();
    let body = schema.field_index("body").unwrap();
    let mut out = (0, 0, 0, 0);
    for r in db.readers(&snap).unwrap() {
        for p in r.substring(body).unwrap().map_or(&[][..], |ix| ix.parts()) {
            out = (out.0 + 1, out.1 + p.n(), out.2 + p.runs(), out.3 + p.samples());
        }
    }
    out
}

/// The median milliseconds of `sql` over `runs` runs after one to warm
/// the page cache, and its rows.
fn timed(db: &Db, sql: &str, runs: usize) -> (f64, Vec<Vec<Value>>) {
    let mut rows = db.query(sql, &Params::new()).unwrap_or_else(|e| panic!("{sql}: {e}")).rows;
    let mut ms: Vec<f64> = (0..runs)
        .map(|_| {
            let t = Instant::now();
            rows = db.query(sql, &Params::new()).unwrap().rows;
            t.elapsed().as_secs_f64() * 1e3
        })
        .collect();
    ms.sort_by(f64::total_cmp);
    (ms[ms.len() / 2], rows)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (dir, input, patterns) = (Path::new(&args[1]), Path::new(&args[2]), Path::new(&args[3]));
    std::fs::create_dir_all(dir).unwrap();
    let mut out = serde_json::json!({ "input": input.file_name().unwrap().to_string_lossy() });
    let mut dbs = Vec::new();
    for (name, options) in [("plain", ""), ("substring", " OPTIONS(substring = true)")] {
        let path = dir.join(format!("{name}.strata"));
        if !path.exists() {
            let (load_s, merge_s, lines, body_bytes) = build(&path, input, options);
            out["lines"] = serde_json::json!(lines);
            out["body_bytes"] = serde_json::json!(body_bytes);
            out[name] = serde_json::json!({ "load_seconds": round(load_s, 1), "merge_seconds": round(merge_s, 1) });
        }
        // Read-only and without the filter cache, which would keep a
        // pattern's rows from its first run.
        let mut db = Db::open(&path, DbOptions { read_only: true, ..DbOptions::default() }).unwrap();
        db.exec.filter_cache = None;
        let (bytes, segments) = sizes(&db, &["store", "text.body", "sub.body", "sub.body*.bwt", "sub.body*.sa", "sub.body*.docs"]);
        out[name]["file_bytes"] = serde_json::json!(db.storage().file_stats().file_bytes);
        out[name]["store_bytes"] = serde_json::json!(bytes[0]);
        out[name]["text_body_bytes"] = serde_json::json!(bytes[1]);
        out[name]["segments"] = serde_json::json!(segments);
        if name == "substring" {
            let (parts, text, runs, samples) = parts(&db);
            out[name]["sub_body"] = serde_json::json!({
                "bytes": bytes[2], "bwt_bytes": bytes[3], "sa_bytes": bytes[4], "docs_bytes": bytes[5],
                "parts": parts, "text_bytes": text, "runs": runs, "samples": samples,
            });
        }
        dbs.push(db);
    }
    let mut queries = Vec::new();
    for line in BufReader::new(std::fs::File::open(patterns).unwrap()).lines() {
        let line = line.unwrap();
        let Some((op, pattern)) = line.split_once('\t') else { continue };
        let cond = if op == "WHERE" { pattern.to_string() } else { format!("body {op} '{}'", pattern.replace('\'', "''")) };
        let count = format!("SELECT COUNT(*) FROM logs WHERE {cond}");
        let first = format!("SELECT id FROM logs WHERE {cond} ORDER BY id LIMIT 10");
        let (scan_ms, scanned) = timed(&dbs[0], &count, SCAN_RUNS);
        let (index_ms, found) = timed(&dbs[1], &count, RUNS);
        assert_eq!(scanned, found, "{count}");
        let (scan_first_ms, a) = timed(&dbs[0], &first, SCAN_RUNS);
        let (index_first_ms, b) = timed(&dbs[1], &first, RUNS);
        assert_eq!(a, b, "{first}");
        queries.push(serde_json::json!({
            "op": op,
            "pattern": pattern,
            "rows": found[0][0].as_i64(),
            "count_ms": { "scan": round(scan_ms, 2), "index": round(index_ms, 3) },
            "first_10_ms": { "scan": round(scan_first_ms, 2), "index": round(index_first_ms, 3) },
        }));
    }
    out["queries"] = serde_json::json!(queries);
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}
