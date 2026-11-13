//! `ASOF JOIN`: each row of the left side beside the latest row of the
//! right side, of the same keys, at or before its time; checked against
//! the same computed over the rows.

use std::collections::BTreeMap;

use chloedb::db::{Db, SqlResult};
use chloedb::query::Params;
use chloedb::storage::DbOptions;
use chloedb::value::Value;
use chloedb::{Field, FieldType, Schema};
use rand::{Rng, SeedableRng};

const USERS: usize = 30;

struct Data {
    // id, user, time, label
    spine: Vec<(String, String, i64, i64)>,
    // id, user, time, clicks, kind
    feats: Vec<(String, String, i64, f64, String)>,
}

fn data() -> Data {
    let mut rng = rand::rngs::StdRng::seed_from_u64(7);
    let spine = (0..400).map(|i| (format!("s{i:04}"), format!("u{}", rng.gen_range(0..USERS)), rng.gen_range(0..10_000), rng.gen_range(0..2))).collect();
    // Times repeat, so ties at a time and rows at exactly a spine's time
    // both happen.
    let feats = (0..2_000)
        .map(|i| (format!("f{i:05}"), format!("u{}", rng.gen_range(0..USERS + 5)), rng.gen_range(0..2_000) * 5, rng.gen_range(0..1000) as f64, if i % 3 == 0 { "a" } else { "b" }.to_string()))
        .collect();
    Data { spine, feats }
}

fn open(dir: &std::path::Path, d: &Data) -> Db {
    let opts = DbOptions { segment_max_docs: 150, sync: false, ..DbOptions::default() };
    let schema = Schema::new("spine", vec![Field::new("id", FieldType::Keyword), Field::new("usr", FieldType::Keyword), Field::new("ts", FieldType::Int), Field::new("label", FieldType::Int)], Some("id")).unwrap();
    let db = Db::create(&dir.join("t.chloedb"), schema, opts).unwrap();
    db.load(d.spine.iter().map(|(id, u, t, l)| vec![Value::Str(id.clone()), Value::Str(u.clone()), Value::Int(*t), Value::Int(*l)])).unwrap();
    sql(&db, "CREATE TABLE feats (id KEYWORD PRIMARY KEY, usr KEYWORD, ts INT, clicks FLOAT, kind KEYWORD)");
    db.load_into("feats", d.feats.iter().map(|(id, u, t, c, k)| vec![Value::Str(id.clone()), Value::Str(u.clone()), Value::Int(*t), Value::Float(*c), Value::Str(k.clone())])).unwrap();
    db
}

fn sql(db: &Db, q: &str) -> SqlResult {
    db.sql(q, &Params::new()).unwrap_or_else(|e| panic!("{q}: {e}"))
}

fn rows(db: &Db, q: &str) -> Vec<Vec<Value>> {
    match sql(db, q) {
        SqlResult::Rows(r) => r.rows,
        SqlResult::Written(_) => panic!("a read"),
    }
}

/// The spine's id → the feature row's id the join should pick, by
/// brute force: the latest time at or before (or before, `strict`) the
/// spine's, ties to the highest id (the last read, as rows are read in
/// key order here: one key per row and keys in load order).
fn want(d: &Data, strict: bool, kind: Option<&str>) -> BTreeMap<String, Option<String>> {
    d.spine
        .iter()
        .map(|(id, u, t, _)| {
            let best = d
                .feats
                .iter()
                .filter(|f| &f.1 == u && (if strict { f.2 < *t } else { f.2 <= *t }) && kind.is_none_or(|k| f.4 == k))
                .max_by(|a, b| a.2.cmp(&b.2).then(a.0.cmp(&b.0)))
                .map(|f| f.0.clone());
            (id.clone(), best)
        })
        .collect()
}

fn got(db: &Db, q: &str) -> BTreeMap<String, Option<String>> {
    rows(db, q)
        .into_iter()
        .map(|r| {
            let Value::Str(id) = &r[0] else { panic!("an id: {r:?}") };
            let f = match &r[1] {
                Value::Str(s) => Some(s.clone()),
                Value::Null => None,
                v => panic!("a feature id: {v:?}"),
            };
            (id.clone(), f)
        })
        .collect()
}

#[test]
fn asof_join_takes_the_latest_row_at_or_before() {
    let dir = tempfile::tempdir().unwrap();
    let d = data();
    let db = open(dir.path(), &d);
    let all = want(&d, false, None);
    assert!(all.values().any(Option::is_none) && all.values().any(Option::is_some), "the data has rows with and without a match");
    let q = "SELECT s.id, f.id FROM spine s ASOF JOIN feats f MATCH_CONDITION (s.ts >= f.ts) ON s.usr = f.usr";
    let g = got(&db, q);
    assert_eq!(g.len(), d.spine.len(), "one row per spine row");
    // Ties at the same time: any of them is right; compare the times.
    let time: BTreeMap<String, i64> = d.feats.iter().map(|f| (f.0.clone(), f.2)).collect();
    for (id, w) in &all {
        assert_eq!(g[id].as_ref().map(|f| time[f]), w.as_ref().map(|f| time[f]), "spine {id}");
    }
    // The other spelling, and strictly before.
    let g2 = got(&db, "SELECT s.id, f.id FROM spine s ASOF JOIN feats f MATCH_CONDITION (f.ts <= s.ts) ON s.usr = f.usr");
    assert_eq!(g, g2);
    let strict = want(&d, true, None);
    let g3 = got(&db, "SELECT s.id, f.id FROM spine s ASOF JOIN feats f MATCH_CONDITION (s.ts > f.ts) ON s.usr = f.usr");
    for (id, w) in &strict {
        assert_eq!(g3[id].as_ref().map(|f| time[f]), w.as_ref().map(|f| time[f]), "spine {id}, strictly before");
    }
    // A condition of the right side alone in ON filters it first.
    let only_a = want(&d, false, Some("a"));
    let g4 = got(&db, "SELECT s.id, f.id FROM spine s ASOF JOIN feats f MATCH_CONDITION (s.ts >= f.ts) ON s.usr = f.usr AND f.kind = 'a'");
    for (id, w) in &only_a {
        assert_eq!(g4[id].as_ref().map(|f| time[f]), w.as_ref().map(|f| time[f]), "spine {id}, kind a");
    }
}

#[test]
fn asof_join_feeds_a_training_set() {
    let dir = tempfile::tempdir().unwrap();
    let d = data();
    let db = open(dir.path(), &d);
    // Columns of both sides, a WHERE on the spine, an order and a cut.
    let r = rows(&db, "SELECT s.id, s.label, f.clicks, s.ts - f.ts AS age FROM spine s ASOF JOIN feats f MATCH_CONDITION (s.ts >= f.ts) ON s.usr = f.usr WHERE s.ts >= 5000 ORDER BY s.id LIMIT 50");
    assert_eq!(r.len(), 50);
    for row in &r {
        if let Value::Int(age) = &row[3] {
            assert!(*age >= 0, "a feature from the future: {row:?}");
        }
    }
    // Aggregates over the joined rows.
    let n = rows(&db, "SELECT COUNT(*), COUNT(f.id) FROM spine s ASOF JOIN feats f MATCH_CONDITION (s.ts >= f.ts) ON s.usr = f.usr");
    let matched = want(&d, false, None).values().filter(|v| v.is_some()).count() as i64;
    assert_eq!(n[0], vec![Value::Int(d.spine.len() as i64), Value::Int(matched)]);
    // EXPLAIN names it.
    let plan = rows(&db, "EXPLAIN SELECT s.id, f.clicks FROM spine s ASOF JOIN feats f MATCH_CONDITION (s.ts >= f.ts) ON s.usr = f.usr");
    let text: String = plan.iter().map(|r| format!("{:?}\n", r)).collect();
    assert!(text.contains("As-of join"), "{text}");
}

#[test]
fn asof_join_refuses_what_it_cannot_mean() {
    let dir = tempfile::tempdir().unwrap();
    let d = data();
    let db = open(dir.path(), &d);
    for (q, says) in [
        ("SELECT s.id FROM spine s ASOF JOIN feats f MATCH_CONDITION (s.ts <= f.ts) ON s.usr = f.usr", "MATCH_CONDITION"),
        ("SELECT s.id FROM spine s ASOF JOIN feats f MATCH_CONDITION (s.ts = f.ts) ON s.usr = f.usr", "MATCH_CONDITION"),
        ("SELECT s.id FROM spine s ASOF JOIN feats f MATCH_CONDITION (s.ts >= f.ts) ON s.usr = f.usr AND s.label < f.clicks", "MATCH_CONDITION"),
    ] {
        let e = db.sql(q, &Params::new()).err().unwrap_or_else(|| panic!("{q} should fail")).to_string();
        assert!(e.contains(says), "{q}: {e}");
    }
}
