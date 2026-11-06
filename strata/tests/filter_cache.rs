//! Filters kept between queries (`query/exec/filter_cache.rs`): a filter a
//! segment sees again is kept, later queries read it from the cache, and
//! every answer is the one a query without the cache gives, through
//! deletes, new rows, values written in place and merges.

use std::sync::Arc;

use strata::db::Db;
use strata::query::exec::{ExecOptions, FilterCache, FILTER_CACHE_BYTES};
use strata::query::plan::FilterMode;
use strata::query::Params;
use strata::storage::{DbOptions, MergePolicy};
use strata::value::Value;
use strata::schema::VectorIndexKind;
use strata::{Field, FieldType, Schema};

const WORDS: [&str; 12] = ["alpha", "beta", "gamma", "delta", "alder", "alpine", "bravo", "charlie", "almond", "echo", "foxtrot", "golf"];

fn schema() -> Schema {
    Schema::new(
        "t",
        vec![
            Field::new("id", FieldType::Keyword),
            Field::new("body", FieldType::Text { positions: false }),
            Field::new("cat", FieldType::Keyword),
            Field::new("year", FieldType::Int),
            Field::new("n", FieldType::Int),
        ],
        Some("id"),
    )
    .unwrap()
}

fn row(i: usize) -> Vec<Value> {
    let body: Vec<&str> = (0..1 + i % 5).map(|j| WORDS[(i * 7 + j * 5) % WORDS.len()]).collect();
    vec![
        Value::Str(format!("r{i:05}")),
        Value::Str(body.join(" ")),
        Value::Str(format!("c{}", i % 6)),
        Value::Int(1995 + (i * 13 % 30) as i64),
        Value::Int((i as i64 * 31) % 100),
    ]
}

/// 2,400 rows in segments of 500, 200 more in the memtable.
fn open(dir: &std::path::Path) -> Db {
    let opts = DbOptions { segment_max_docs: 500, sync: false, ..DbOptions::default() };
    let db = Db::create(&dir.join("t.strata"), schema(), opts).unwrap();
    db.load((0..2400).map(row)).unwrap();
    let mut t = db.storage().begin().unwrap();
    for i in 2400..2600 {
        t.insert(row(i)).unwrap();
    }
    t.commit().unwrap();
    db
}

const QUERIES: [&str; 8] = [
    "SELECT id, score() FROM t WHERE year >= 2010 AND cat IN ('c1', 'c2') ORDER BY bm25(body, 'alpha gamma echo') DESC, id LIMIT 10",
    "SELECT id, score() FROM t WHERE MATCH(body, 'al*') ORDER BY bm25(body, 'beta delta') DESC, id LIMIT 10",
    "SELECT COUNT(*) FROM t WHERE n > 20 AND NOT cat = 'c3'",
    "SELECT cat, COUNT(*), MAX(n) FROM t WHERE year BETWEEN 2000 AND 2015 GROUP BY cat ORDER BY cat",
    "SELECT id FROM t WHERE MATCH(body, 'bravo') AND year < 2005 ORDER BY id LIMIT 20",
    "SELECT id FROM t WHERE cat = 'c4' OR n < 5 ORDER BY n DESC, id LIMIT 15",
    // A fixed filter beside one that changes: the fixed one is kept.
    "SELECT id, score() FROM t WHERE year >= 2005 AND id NOT IN (:a, :b) ORDER BY bm25(body, 'gamma alpine') DESC, id LIMIT 10",
    "SELECT cat, COUNT(*) FROM t WHERE year >= 2005 AND n <> :c GROUP BY cat ORDER BY cat",
];

fn modes(db: &Db) -> Vec<(&'static str, ExecOptions)> {
    let with = db.exec.clone();
    assert!(with.filter_cache.is_some(), "a database keeps filters");
    vec![
        ("planned", with.clone()),
        ("probe", ExecOptions { filter_mode: Some(FilterMode::Probe), ..with.clone() }),
        ("materialise", ExecOptions { filter_mode: Some(FilterMode::Materialize), ..with }),
    ]
}

/// Every query, three times in each filter mode, against the same query
/// without the cache.
fn check(db: &Db, what: &str) -> usize {
    let mut from_cache = 0;
    for (mode, opts) in modes(db) {
        let plain = ExecOptions { filter_cache: None, ..opts.clone() };
        for sql in QUERIES {
            for round in 0..3 {
                let mut params = Params::new();
                for (k, v) in [("a", Value::Str(format!("r{:05}", 3 * round))), ("b", Value::Str(format!("r{:05}", 7 + round))), ("c", Value::Int(round as i64 * 11))] {
                    params.0.insert(k.into(), v);
                }
                let want = db.query_with(sql, &params, &plain).unwrap();
                let got = db.query_with(sql, &params, &opts).unwrap();
                assert_eq!(got.rows, want.rows, "{what}, {mode}: {sql}");
                assert_eq!(got.scores, want.scores, "{what}, {mode}: {sql}");
                from_cache += got.stats.plan.iter().filter(|l| l.contains("from the cache")).count();
            }
        }
    }
    from_cache
}

#[test]
fn kept_filters_answer_as_the_rows_do() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path());
    assert!(check(&db, "fresh") > 0, "later queries read their filters from the cache");
    let cache = db.exec.filter_cache.clone().unwrap();
    assert!(!cache.is_empty() && cache.hits() > 0);

    // Deletes and new rows: the kept sets stay, deletes are checked apart.
    let mut t = db.storage().begin().unwrap();
    for i in (0..2600).step_by(7) {
        t.delete(&Value::Str(format!("r{i:05}"))).unwrap();
    }
    for i in 2600..2700 {
        t.insert(row(i)).unwrap();
    }
    t.commit().unwrap();
    check(&db, "after deletes");

    // A merge makes new segments, whose filters are kept anew.
    db.storage().merge(&MergePolicy::compact()).unwrap();
    check(&db, "after a merge");
}

#[test]
fn a_field_written_in_place_is_not_kept() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path());
    let sql = "SELECT COUNT(*) FROM t WHERE year >= 2010 AND cat = 'c1'";
    for _ in 0..3 {
        db.query(sql, &Params::new()).unwrap();
    }
    assert!(db.query(sql, &Params::new()).unwrap().stats.plan.iter().any(|l| l.contains("from the cache")));
    // Rows of the on-disk segments move into and out of the filter.
    let mut t = db.storage().begin().unwrap();
    for i in (0..2400).step_by(3) {
        t.set(&Value::Str(format!("r{i:05}")), "year", Value::Int(if i % 2 == 0 { 2020 } else { 1990 })).unwrap();
    }
    t.commit().unwrap();
    let plain = ExecOptions { filter_cache: None, ..db.exec.clone() };
    let want = db.query_with(sql, &Params::new(), &plain).unwrap().rows;
    for _ in 0..3 {
        let got = db.query(sql, &Params::new()).unwrap();
        assert_eq!(got.rows, want);
        assert!(!got.stats.plan.iter().any(|l| l.contains("cache")), "{:?}", got.stats.plan);
    }
    check(&db, "values written in place");
}

#[test]
fn the_cache_keeps_to_its_limit() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = open(dir.path());
    // About five segments' bitsets.
    let cache = Arc::new(FilterCache::new(1000));
    db.exec.filter_cache = Some(cache.clone());
    for y in 1995..2025 {
        let sql = format!("SELECT cat, COUNT(*) FROM t WHERE year >= {y} GROUP BY cat");
        for _ in 0..2 {
            db.query(&sql, &Params::new()).unwrap();
        }
        assert!(cache.bytes() <= cache.limit(), "{} bytes kept, limit {}", cache.bytes(), cache.limit());
    }
    assert!(!cache.is_empty());
    let none = Arc::new(FilterCache::new(0));
    db.exec.filter_cache = Some(none.clone());
    for _ in 0..3 {
        db.query("SELECT cat, COUNT(*) FROM t WHERE year >= 2000 GROUP BY cat", &Params::new()).unwrap();
    }
    assert!(none.is_empty());

    // Too small for any set: each segment makes its filter to keep once,
    // is refused, and does not make it again.
    let small = Arc::new(FilterCache::new(400));
    db.exec.filter_cache = Some(small.clone());
    let mut made = 0;
    for _ in 0..6 {
        let got = db.query("SELECT cat, COUNT(*) FROM t WHERE year >= 2003 GROUP BY cat", &Params::new()).unwrap();
        made += got.stats.plan.iter().filter(|l| l.contains("kept in the cache")).count();
    }
    assert!(small.is_empty());
    assert!((1..=5).contains(&made), "{made} sets made to be refused");
}

/// A list of the table's keys is a request's own (the items a user has
/// seen): kept, it would be a set of nearly every row for each request.
/// What is fixed beside it is kept, and another list keeps nothing more.
#[test]
fn a_list_of_keys_is_not_kept() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path());
    for mode in [FilterMode::Probe, FilterMode::Materialize] {
        let cache = Arc::new(FilterCache::new(FILTER_CACHE_BYTES));
        let opts = ExecOptions { filter_cache: Some(cache.clone()), filter_mode: Some(mode), ..db.exec.clone() };
        let plain = ExecOptions { filter_cache: None, ..opts.clone() };
        let run = |a: usize| {
            let sql = format!("SELECT id, score() FROM t WHERE year >= 2005 AND id NOT IN ('r{a:05}', 'r{:05}') ORDER BY bm25(body, 'gamma alpine') DESC, id LIMIT 10", a + 7);
            for _ in 0..3 {
                assert_eq!(db.query_with(&sql, &Params::new(), &opts).unwrap().rows, db.query_with(&sql, &Params::new(), &plain).unwrap().rows, "{mode:?}: {sql}");
            }
        };
        run(3);
        let kept = (cache.len(), cache.bytes());
        assert!(kept.0 > 0, "{mode:?}: the fixed filter is kept");
        for a in [11, 19, 23] {
            run(a);
        }
        assert_eq!((cache.len(), cache.bytes()), kept, "{mode:?}: other lists of keys keep nothing more");
    }
}

/// A vector search looks at its filter twice, once to sample how many rows
/// near the query pass it: still one sight, so one query keeps nothing.
#[test]
fn one_query_is_one_sight() {
    let dir = tempfile::tempdir().unwrap();
    let schema = Schema::new(
        "v",
        vec![Field::new("id", FieldType::Keyword), Field::new("n", FieldType::Int), Field::new("emb", FieldType::Vector { dim: 8, index: VectorIndexKind::Hnsw { m: 8, ef_construction: 32 } })],
        Some("id"),
    )
    .unwrap();
    let mut db = Db::create(&dir.path().join("v.strata"), schema, DbOptions { sync: false, ..DbOptions::default() }).unwrap();
    let vec_of = |i: usize| -> Arc<[f32]> { (0..8).map(|j| (((i * 31 + j * 17) % 97) as f32 / 97.0) - 0.5).collect::<Vec<f32>>().into() };
    db.load((0..20_000).map(|i| vec![Value::Str(format!("v{i:05}")), Value::Int((i % 100) as i64), Value::Vector(vec_of(i * 7 + 3))])).unwrap();
    let cache = Arc::new(FilterCache::new(FILTER_CACHE_BYTES));
    db.exec.filter_cache = Some(cache.clone());
    let sql = "SELECT id FROM v WHERE n < 40 ORDER BY similarity(emb, :q) DESC LIMIT 10";
    let params = Params::new().set("q", Value::Vector(vec_of(5)));
    let first = db.query(sql, &params).unwrap();
    assert!(first.stats.plan.iter().any(|l| l.contains("near the query")), "the search samples its filter: {:?}", first.stats.plan);
    assert!(cache.is_empty(), "one query keeps nothing: {:?}", first.stats.plan);
    let second = db.query(sql, &params).unwrap();
    assert_eq!(second.rows, first.rows);
    assert!(!cache.is_empty(), "the second query keeps its filter: {:?}", second.stats.plan);
}
