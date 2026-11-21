//! A recommender in production (NADIA-301): `recommend(...)` does not slow
//! down as events arrive, and `optimize` brings it back to where it began.
//!
//! Each event adds to the counts by writing values in place in the counts'
//! segments. Before, every read of the counts with a range on a value
//! written in place (`next > 0`, `co > 0`) gathered every value written in
//! the segment, so each call was slower than the one before until the
//! segments merged.
//!
//! What still grows: the events flush small segments of counts beside the
//! big one, and an ORDER BY on a count written in place reads it per row.
//! Each round of 1,000 events adds a few milliseconds (on a 4-core Linux
//! container, 26.5 ms before, 47.0 ms after 3,000 events; on GitHub's macOS
//! runners 11.4 ms and 25.9 ms), and `optimize` takes it back. The bound
//! below allows three times the time before; `optimize` must still bring
//! it back within one and a half. See #752.

use chloedb::db::{Db, SqlResult};
use chloedb::query::Params;
use chloedb::schema::{Field, FieldType, Schema};
use chloedb::storage::{DbOptions, Layout};
use chloedb::value::Value;
use std::time::Instant;

fn sql(db: &Db, s: &str) -> Vec<Vec<Value>> {
    match db.sql(s, &Params::new()).unwrap_or_else(|e| panic!("{s}: {e}")) {
        SqlResult::Rows(r) => r.rows,
        SqlResult::Written(_) => Vec::new(),
    }
}

/// The median time of a call for each of the first `users` users, in ms,
/// over three rounds (the best round: the machine is shared).
fn p50(db: &Db, users: usize) -> f64 {
    (0..3)
        .map(|_| {
            let mut v: Vec<f64> = (0..users)
                .map(|u| {
                    let t = Instant::now();
                    let rows = sql(db, &format!("SELECT item FROM recommend('ml', 'u{u}', k => 10)"));
                    assert!(!rows.is_empty());
                    t.elapsed().as_secs_f64() * 1e3
                })
                .collect();
            v.sort_by(|a, b| a.total_cmp(b));
            v[v.len() / 2]
        })
        .fold(f64::INFINITY, f64::min)
}

#[test]
fn recommend_keeps_its_time_as_events_arrive_and_optimize_restores_it() {
    let dir = tempfile::tempdir().unwrap();
    let seed = Schema::new("seed", vec![Field::new("id", FieldType::Keyword)], Some("id")).unwrap();
    let db = Db::create(&dir.path().join("r.strata"), seed, DbOptions { sync: false, layout: Layout::SingleFile, ..DbOptions::default() }).unwrap();
    sql(&db, "CREATE TABLE events (id KEYWORD PRIMARY KEY, usr KEYWORD NOT NULL, item KEYWORD NOT NULL, ts INT)");
    let (users, per, items) = (1000usize, 60usize, 2000usize);
    let mut n = 0;
    for chunk in (0..users).collect::<Vec<_>>().chunks(100) {
        let mut rows = Vec::new();
        for u in chunk {
            for s in 0..per {
                n += 1;
                rows.push(format!("('e{n}', 'u{u}', 'i{}', {s})", (u * 7 + s * s * 13 + s) % items));
            }
        }
        sql(&db, &format!("INSERT INTO events VALUES {}", rows.join(", ")));
    }
    sql(&db, "CREATE RECOMMENDER ml ON events(usr, item, ts) OPTIONS (window = 3, history = 20)");
    db.optimize().unwrap();
    let before = p50(&db, 200);
    // Events one a transaction, as an app writes them, each adding to
    // about nine counts in place.
    let mut after = Vec::new();
    for round in 1..=3usize {
        for u in 0..1000 {
            n += 1;
            sql(&db, &format!("INSERT INTO events VALUES ('e{n}', 'u{u}', 'i{}', {})", (n * 31) % items, per + round * 1000 + u));
        }
        after.push(p50(&db, 200));
    }
    let t = Instant::now();
    db.optimize().unwrap();
    let took = t.elapsed();
    let optimized = p50(&db, 200);
    eprintln!("recommend p50: {before:.2} ms before, {after:.2?} ms after each 1,000 events, {optimized:.2} ms after optimize ({took:.1?})");
    let worst = after.iter().copied().fold(0.0, f64::max);
    assert!(worst < before * 3.0 + 1.0, "{before:.2} ms before 3,000 events, {after:.2?} ms after");
    assert!(optimized < before * 1.5 + 1.0, "{before:.2} ms before, {optimized:.2} ms after optimize");
}
