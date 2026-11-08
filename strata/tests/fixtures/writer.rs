//! The writes behind the fixtures of formats 2 and 3, of numbered writes,
//! of rank profiles and of stored functions in this directory (README.md):
//! `examples/format_fixtures.rs` writes them with this module,
//! and `tests/format_fixtures.rs` writes them again to check that Strata
//! still writes the same bytes. The writes are deterministic: two runs give
//! the same manifests, segments, delete sets, updates and WAL records, and
//! differ only in the database id (the meta pages, `CURRENT` and the WAL
//! header).

#![allow(dead_code)]

use std::path::Path;

use strata::schema::{Analyzer, Field, FieldType, Schema};
use strata::query::function::{self, Change, FunctionSet};
use strata::storage::{Database, DbOptions, Layout, RankProfile, FUNCTIONS};
use strata::value::Value;

/// 2026-10-01T00:00:00Z in microseconds.
pub const OCT1: i64 = 1_790_812_800_000_000;
const DAY: i64 = 86_400_000_000;

fn key(i: u64) -> Value {
    Value::Str(format!("k{i:06}"))
}

/// The first table, as in the format 1 fixtures.
fn t() -> Schema {
    Schema::new("t", vec![Field::new("id", FieldType::Keyword), Field::new("n", FieldType::Int), Field::new("body", FieldType::Text { positions: false })], Some("id")).unwrap()
}

fn t_doc(i: u64, v: i64) -> Vec<Value> {
    vec![key(i), Value::Int(v), Value::Str(format!("document {i} version {v} lorem ipsum"))]
}

/// A second table, whose rows name rows of the first.
fn u() -> Schema {
    let mut parent = Field::new("parent", FieldType::Keyword);
    parent.references = Some("t".into());
    Schema::new("u", vec![Field::new("id", FieldType::Int), parent, Field::new("note", FieldType::Text { positions: true })], Some("id")).unwrap()
}

fn u_doc(i: i64) -> Vec<Value> {
    vec![Value::Int(i), key(i as u64 % 7), Value::Str(format!("note {i} about the running cats"))]
}

/// A table with every format 3 feature: BOOL, TIMESTAMP, NOT NULL,
/// DEFAULT and the simple analyzer.
pub fn tasks() -> Schema {
    Schema::new(
        "tasks",
        vec![
            Field::new("id", FieldType::Keyword),
            Field::new("title", FieldType::Text { positions: true }).analyzer(Analyzer::Simple),
            Field::new("body", FieldType::Text { positions: true }),
            Field::boolean("done").not_null().default(serde_json::json!(false)),
            Field::timestamp("due"),
            Field::new("priority", FieldType::Int).not_null().default(serde_json::json!(2)),
            Field::new("state", FieldType::Keyword).default(serde_json::json!("open")),
        ],
        Some("id"),
    )
    .unwrap()
}

/// A task row as stored: BOOL as 1 or 0, TIMESTAMP in microseconds.
pub fn task(i: u64, priority: i64) -> Vec<Value> {
    let title = ["The Running Cats", "they run", "Über alles", "a cat sat"][i as usize % 4];
    vec![
        key(i),
        Value::Str(format!("{title} {i}")),
        Value::Str(format!("{title} {i}")),
        Value::Int((i % 3 == 0) as i64),
        if i % 5 == 4 { Value::Null } else { Value::Int(OCT1 + i as i64 * DAY) },
        Value::Int(priority),
        Value::Str(["open", "closed"][i as usize % 2].into()),
    ]
}

fn create(path: &Path, schema: Schema, layout: Layout) -> Database {
    Database::create(path, schema, DbOptions { layout, sync: true, ..DbOptions::default() }).unwrap()
}

/// Format 2: two tables, each written and flushed, then deletes and a
/// value set in place (flushed again), and a transaction on both tables
/// left in the WAL.
fn format2(path: &Path, layout: Layout) {
    let db = create(path, t(), layout);
    db.create_table(u()).unwrap();
    let mut tx = db.begin().unwrap();
    for i in 0..30 {
        tx.insert(t_doc(i, 0)).unwrap();
    }
    for i in 0..10 {
        tx.table("u").unwrap().insert(u_doc(i)).unwrap();
    }
    tx.commit().unwrap();
    db.flush().unwrap();
    let mut tx = db.begin().unwrap();
    for i in 0..5 {
        tx.delete(&key(i)).unwrap();
    }
    for i in 5..8 {
        tx.set(&key(i), "n", Value::Int(7)).unwrap();
    }
    tx.table("u").unwrap().delete(&Value::Int(9)).unwrap();
    tx.commit().unwrap();
    db.flush().unwrap();
    // Left in the WAL.
    let mut tx = db.begin().unwrap();
    for i in 30..40 {
        tx.insert(t_doc(i, 1)).unwrap();
    }
    tx.delete(&key(10)).unwrap();
    tx.upsert(t_doc(11, 2)).unwrap();
    let mut ut = tx.table("u").unwrap();
    ut.insert(u_doc(10)).unwrap();
    ut.upsert(u_doc(3)).unwrap();
    tx.commit().unwrap();
}

/// Format 3: the tasks table alone, written and flushed, then deletes and
/// a value set in place (flushed again), and a transaction left in the
/// WAL.
fn format3(path: &Path, layout: Layout) {
    let db = create(path, tasks(), layout);
    let mut tx = db.begin().unwrap();
    for i in 0..30 {
        tx.insert(task(i, 2)).unwrap();
    }
    tx.commit().unwrap();
    db.flush().unwrap();
    let mut tx = db.begin().unwrap();
    for i in 0..5 {
        tx.delete(&key(i)).unwrap();
    }
    for i in 5..8 {
        tx.set(&key(i), "priority", Value::Int(7)).unwrap();
    }
    tx.commit().unwrap();
    db.flush().unwrap();
    // Left in the WAL.
    let mut tx = db.begin().unwrap();
    for i in 30..40 {
        tx.insert(task(i, 1)).unwrap();
    }
    tx.delete(&key(10)).unwrap();
    tx.upsert(task(11, 3)).unwrap();
    tx.commit().unwrap();
}

/// Numbered writes (format 3 with `seq_no`): the first table, with the
/// rows of format 2's: a write before numbering started, then under term 2
/// rows written and flushed, deletes and a value set in place (flushed
/// again), and a transaction left in the WAL. Five transactions are
/// numbered, 1 to 5.
fn seqno(path: &Path, layout: Layout) {
    let db = create(path, t(), layout);
    let mut tx = db.begin().unwrap();
    for i in 0..10 {
        tx.insert(t_doc(i, 0)).unwrap();
    }
    tx.commit().unwrap();
    db.start_seq_no().unwrap();
    db.begin_term(2).unwrap();
    for r in [10..20, 20..30] {
        let mut tx = db.begin().unwrap();
        for i in r {
            tx.insert(t_doc(i, 0)).unwrap();
        }
        tx.commit().unwrap();
    }
    db.flush().unwrap();
    let mut tx = db.begin().unwrap();
    for i in 0..5 {
        tx.delete(&key(i)).unwrap();
    }
    tx.commit().unwrap();
    let mut tx = db.begin().unwrap();
    for i in 5..8 {
        tx.set(&key(i), "n", Value::Int(7)).unwrap();
    }
    tx.commit().unwrap();
    db.flush().unwrap();
    // Left in the WAL.
    let mut tx = db.begin().unwrap();
    for i in 30..40 {
        tx.insert(t_doc(i, 1)).unwrap();
    }
    tx.delete(&key(10)).unwrap();
    tx.upsert(t_doc(11, 2)).unwrap();
    tx.commit().unwrap();
}

pub fn profile(name: &str, ranking: &str, defaults: &[(&str, serde_json::Value)]) -> RankProfile {
    RankProfile { name: name.into(), ranking: ranking.into(), defaults: defaults.iter().map(|(k, v)| (k.to_string(), v.clone())).collect() }
}

/// Rank profiles (format 3 with `rank_profiles`): format 2's writes, with
/// two profiles of `t` logged before its deletes and values set in place,
/// and flushed with them into the manifest; and logged before the
/// transaction left in the WAL, a profile of `u`, `t`'s first profile
/// replaced and its second dropped.
fn profiles(path: &Path, layout: Layout) {
    let db = create(path, t(), layout);
    db.create_table(u()).unwrap();
    let mut tx = db.begin().unwrap();
    for i in 0..30 {
        tx.insert(t_doc(i, 0)).unwrap();
    }
    for i in 0..10 {
        tx.table("u").unwrap().insert(u_doc(i)).unwrap();
    }
    tx.commit().unwrap();
    db.flush().unwrap();
    db.set_rank_profile("t", profile("by_n", "SELECT * FROM t ORDER BY n DESC", &[]), false).unwrap();
    let phased = "WITH top AS (SELECT * FROM t ORDER BY bm25(body, :q) DESC LIMIT 20) SELECT * FROM top ORDER BY n * :w DESC";
    db.set_rank_profile("t", profile("phased", phased, &[("q", serde_json::json!("lorem")), ("w", serde_json::json!(0.5))]), false).unwrap();
    let mut tx = db.begin().unwrap();
    for i in 0..5 {
        tx.delete(&key(i)).unwrap();
    }
    for i in 5..8 {
        tx.set(&key(i), "n", Value::Int(7)).unwrap();
    }
    tx.table("u").unwrap().delete(&Value::Int(9)).unwrap();
    tx.commit().unwrap();
    db.flush().unwrap();
    // Left in the WAL.
    db.set_rank_profile("u", profile("notes", "SELECT * FROM u ORDER BY bm25(note, :q) DESC", &[("q", serde_json::json!("cats"))]), false).unwrap();
    db.set_rank_profile("t", profile("by_n", "SELECT * FROM t ORDER BY n", &[]), true).unwrap();
    assert!(db.drop_rank_profile("t", "phased").unwrap());
    let mut tx = db.begin().unwrap();
    for i in 30..40 {
        tx.insert(t_doc(i, 1)).unwrap();
    }
    tx.delete(&key(10)).unwrap();
    tx.upsert(t_doc(11, 2)).unwrap();
    let mut ut = tx.table("u").unwrap();
    ut.insert(u_doc(10)).unwrap();
    ut.upsert(u_doc(3)).unwrap();
    tx.commit().unwrap();
}

/// Writes a stored function's statement, as `Db::sql` does, into a
/// database whose versions are `have`, at 2026-10-01 by `writer`:
/// deterministic, where `Db::sql` takes the time now.
pub fn define(db: &Database, have: &mut Vec<function::Function>, text: &str) {
    let stmt = function::parse_statement(text).unwrap().expect("a function's statement");
    db.ensure_functions().unwrap();
    let mut tx = db.begin().unwrap();
    match function::change(&FunctionSet::of(have.clone()), &stmt, Some(OCT1), Some("writer")).unwrap() {
        Change::Put(f) => {
            tx.table(FUNCTIONS).unwrap().upsert(f.to_row()).unwrap();
            have.push(f);
        }
        Change::Delete(id) => {
            assert!(tx.table(FUNCTIONS).unwrap().delete(&Value::Str(id.clone())).unwrap());
            have.retain(|f| f.id() != id);
        }
        Change::Nothing => panic!("{text} writes nothing"),
    }
    tx.commit().unwrap();
}

/// Stored functions (format 3 with `field_ids`, `compact_keys` and
/// `functions`): format 2's table `t` written and flushed; then a function
/// that returns a value (`boosted`) and one that returns rows (`top`),
/// and the deletes, flushed together; and left in the WAL, a new version
/// of `boosted`, `top` dropped, a third function (`label`) and `t`'s last
/// transaction.
fn functions(path: &Path, layout: Layout) {
    let db = create(path, t(), layout);
    let mut tx = db.begin().unwrap();
    for i in 0..30 {
        tx.insert(t_doc(i, 0)).unwrap();
    }
    tx.commit().unwrap();
    db.flush().unwrap();
    let mut have = Vec::new();
    define(&db, &mut have, "CREATE FUNCTION boosted(s FLOAT, n INT, w FLOAT DEFAULT 0.5) RETURNS FLOAT OPTIONS (description = 'a score raised by n') AS :s * (1 + :w * :n)");
    define(&db, &mut have, "CREATE FUNCTION top(q TEXT, k INT DEFAULT 5) RETURNS TABLE AS SELECT id, n FROM t WHERE MATCH(body, :q) ORDER BY bm25(body, :q) DESC, id LIMIT :k");
    let mut tx = db.begin().unwrap();
    for i in 0..5 {
        tx.delete(&key(i)).unwrap();
    }
    tx.commit().unwrap();
    db.flush().unwrap();
    // Left in the WAL.
    define(&db, &mut have, "CREATE OR REPLACE FUNCTION boosted(s FLOAT, n INT, w FLOAT DEFAULT 0.25) RETURNS FLOAT AS :s * (1 + :w * :n)");
    define(&db, &mut have, "DROP FUNCTION top");
    define(&db, &mut have, "CREATE FUNCTION label(n INT) RETURNS KEYWORD AS CASE WHEN :n > 3 THEN 'many' ELSE 'few' END");
    let mut tx = db.begin().unwrap();
    for i in 30..35 {
        tx.insert(t_doc(i, 1)).unwrap();
    }
    tx.commit().unwrap();
}

/// Writes the stored functions' set into `out`: `functions/`, with
/// `db.strata` (and its WAL) and `db.dir`.
pub fn write_functions(out: &Path) {
    let d = out.join("functions");
    std::fs::create_dir_all(&d).unwrap();
    functions(&d.join("db.strata"), Layout::SingleFile);
    functions(&d.join("db.dir"), Layout::Directory);
}

/// Writes every fixture set into `out`: `format2/`, `format3/`, `seqno/`
/// and `profiles/`, each with `db.strata` (and its WAL) and `db.dir`.
pub fn write_all(out: &Path) {
    for (dir, write) in [("format2", format2 as fn(&Path, Layout)), ("format3", format3), ("seqno", seqno), ("profiles", profiles)] {
        let d = out.join(dir);
        std::fs::create_dir_all(&d).unwrap();
        write(&d.join("db.strata"), Layout::SingleFile);
        write(&d.join("db.dir"), Layout::Directory);
    }
}
