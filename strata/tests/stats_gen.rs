//! A table's text statistics generation (`Snapshot::stats_gen`): two
//! snapshots of a table with the same generation have the same BM25
//! statistics, which a coordinator keeps by it. Values set in place and
//! deletes leave it, but for a delete that empties a memtable chunk, and so
//! do other tables' writes; a new row, a flush, a merge, a change of tables
//! and a reopen move it.

use std::collections::HashMap;

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use strata::db::Db;
use strata::query::stats::TermStats;
use strata::query::{bind, Params};
use strata::storage::{DbOptions, MergePolicy};
use strata::{Field, FieldType, Schema, Value};

const WORDS: [&str; 8] = ["apple", "pear", "plum", "fig", "kiwi", "lime", "date", "yuzu"];
const QUERY: &str = "SELECT id FROM t ORDER BY bm25(body, 'apple plum kiwi yuzu') DESC LIMIT 5";

fn schema() -> Schema {
    Schema::new(
        "t",
        vec![Field::new("id", FieldType::Keyword), Field::new("n", FieldType::Int), Field::new("r", FieldType::Float), Field::new("body", FieldType::Text { positions: false })],
        Some("id"),
    )
    .unwrap()
}

fn options(chunk: usize) -> DbOptions {
    DbOptions { memtable_max_docs: 1_000_000, memtable_chunk_docs: chunk, memtable_tail_docs: 64, segment_max_docs: 200, sync: false, ..DbOptions::default() }
}

fn key(i: u64) -> Value {
    Value::Str(format!("k{i:04}"))
}

fn row(i: u64, rng: &mut StdRng) -> Vec<Value> {
    let body: Vec<&str> = (0..rng.gen_range(1..6)).map(|_| WORDS[rng.gen_range(0..WORDS.len())]).collect();
    vec![key(i), Value::Int(i as i64), Value::Float(rng.gen_range(0.0..5.0)), Value::Str(body.join(" "))]
}

fn insert(db: &Db, rows: impl IntoIterator<Item = Vec<Value>>) {
    let mut tx = db.storage().begin().unwrap();
    for r in rows {
        tx.upsert(r).unwrap();
    }
    tx.commit().unwrap();
}

fn sql(db: &Db, sql: &str, p: &Params) {
    db.sql(sql, p).unwrap_or_else(|e| panic!("{sql}: {e}"));
}

/// The latest snapshot's generation and statistics for [`QUERY`].
fn stats(db: &Db) -> (u64, TermStats) {
    let snap = db.snapshot();
    let select = bind(QUERY, snap.schema(), &Params::new()).unwrap();
    (snap.stats_gen(), db.term_stats(&snap, &select).unwrap())
}

/// Each kind of change, one at a time.
#[test]
fn what_moves_the_generation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.strata");
    let db = Db::create(&path, schema(), options(128)).unwrap();
    let mut rng = StdRng::seed_from_u64(1);
    insert(&db, (0..300).map(|i| row(i, &mut rng)));
    db.storage().flush().unwrap();
    // 300 rows on disk; 130 in the memtable: a full chunk of 128 and 2.
    insert(&db, (300..430).map(|i| row(i, &mut rng)));
    let (g, s) = stats(&db);
    assert_ne!(g, 0);
    let same = |what: &str| {
        let (g2, s2) = stats(&db);
        assert_eq!(g2, g, "{what} keeps the generation");
        assert_eq!(s2, s, "{what}: and the statistics");
    };
    let k = |i: u64| Params::new().set("k", key(i));
    sql(&db, "UPDATE t SET n = -1, r = 9.5 WHERE id = :k", &k(5));
    same("a value set on disk");
    sql(&db, "UPDATE t SET n = -1, r = NULL WHERE id = :k", &k(310));
    same("a value set in the memtable");
    sql(&db, "DELETE FROM t WHERE id = :k", &k(7));
    same("a delete on disk");
    sql(&db, "DELETE FROM t WHERE id = :k", &k(311));
    same("a delete in the memtable's full chunk");
    sql(&db, "DELETE FROM t WHERE id = :k", &k(428));
    same("a delete in the last chunk");
    sql(&db, "DELETE FROM t WHERE id = :k", &k(4000));
    same("a delete of no row");

    // Another table's writes, and the table's creation, leave this one's.
    sql(&db, "CREATE TABLE notes (id KEYWORD PRIMARY KEY, body TEXT)", &Params::new());
    let (g1, s1) = stats(&db);
    assert_ne!(g1, g, "a change of tables");
    assert_eq!(s1, s);
    let g = g1;
    let notes = || db.snapshot().table("notes").unwrap().stats_gen();
    let n0 = notes();
    assert_ne!(n0, g);
    sql(&db, "INSERT INTO notes (id, body) VALUES ('a', 'apple plum')", &Params::new());
    assert_ne!(notes(), n0, "a new row in notes");
    let (g1, _) = stats(&db);
    assert_eq!(g1, g, "a new row in another table");

    // The last chunk's other row: it has none live, and snapshots leave it out.
    sql(&db, "DELETE FROM t WHERE id = :k", &k(429));
    let (g1, s1) = stats(&db);
    assert_ne!(g1, g, "a chunk emptied");
    assert_ne!(s1, s);
    let mut last = g1;
    let mut moved = |what: &str| {
        let (g2, _) = stats(&db);
        assert_ne!(g2, last, "{what} moves the generation");
        last = g2;
    };
    insert(&db, [row(500, &mut rng)]);
    moved("a new row");
    insert(&db, [row(500, &mut rng)]);
    moved("a row replaced");
    sql(&db, "UPDATE t SET body = 'fig' WHERE id = :k", &k(20));
    moved("a text changed");
    db.storage().flush().unwrap();
    moved("a flush");
    drop(db);
    let db = Db::open(&path, options(128)).unwrap();
    let (g2, _) = stats(&db);
    assert_ne!(g2, last, "a reopen");
}

/// Random writes, flushes and merges: whenever two snapshots share a
/// generation, their statistics are the same; and most values set in
/// place and deletes keep it.
#[test]
fn a_generation_has_one_set_of_statistics() {
    for seed in 0..4u64 {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::create(&dir.path().join("t.strata"), schema(), options([128, 192][seed as usize % 2])).unwrap();
        let mut rng = StdRng::seed_from_u64(seed);
        let mut next = 0u64;
        let mut seen: HashMap<u64, TermStats> = HashMap::new();
        let (mut kept, mut neutral) = (0, 0);
        for step in 0..600 {
            let before = stats(&db).0;
            let pick = rng.gen_range(0..=next.max(1));
            let k = Params::new().set("k", key(pick)).set("n", Value::Int(step)).set("r", Value::Float(rng.gen_range(0.0..5.0)));
            let values_only = match rng.gen_range(0..20) {
                0..=4 => {
                    let rs: Vec<_> = (0..rng.gen_range(1..4)).map(|_| {
                        next += 1;
                        row(next, &mut rng)
                    }).collect();
                    insert(&db, rs);
                    false
                }
                5..=10 => {
                    sql(&db, "UPDATE t SET n = :n, r = :r WHERE id = :k", &k);
                    true
                }
                11..=16 => {
                    sql(&db, "DELETE FROM t WHERE id = :k", &k);
                    true
                }
                17 => {
                    let lo = rng.gen_range(0..=next as i64);
                    sql(&db, "DELETE FROM t WHERE n >= :lo AND n < :hi", &Params::new().set("lo", Value::Int(lo)).set("hi", Value::Int(lo + 8)));
                    true
                }
                18 if step % 5 == 0 => {
                    db.storage().flush().unwrap();
                    false
                }
                19 if step % 7 == 0 => {
                    db.storage().merge(&MergePolicy::compact()).unwrap();
                    false
                }
                _ => false,
            };
            let (g, s) = stats(&db);
            if let Some(was) = seen.get(&g) {
                assert_eq!(was, &s, "seed {seed} step {step}: generation {g} with other statistics");
            }
            seen.insert(g, s);
            if values_only {
                neutral += 1;
                kept += (g == before) as usize;
            }
        }
        // A delete that empties a memtable chunk moves it; few others do.
        assert!(kept * 10 >= neutral * 9, "seed {seed}: {kept} of {neutral} values set and deletes kept the generation");
    }
}
