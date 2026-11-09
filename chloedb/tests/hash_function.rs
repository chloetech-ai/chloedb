//! `hash(x [, seed])`: an INT from 0 to 2^63 - 1 that only the value and
//! the seed decide, for a seeded sample, train/test split or shuffle of
//! rows. Its values are pinned here: they were worked out by a separate
//! Python version of the function (FNV-1a of the seed's and the value's
//! bytes, SplitMix64's finalizer), and must never change, or the splits
//! people made with it would. A split by it holds every row once; a
//! column's hash is its value's; the plan says it hashes; and DataFusion
//! computes the same values as the operators.

use chloedb::db::{Db, SqlResult};
use chloedb::query::ir::{Plan, FEATURES, HASH};
use chloedb::query::Params;
use chloedb::storage::DbOptions;
use chloedb::value::Value;
use chloedb::{Field, FieldType, Schema};

fn sql(db: &Db, s: &str) -> Vec<Vec<Value>> {
    match db.sql(s, &Params::new()).unwrap_or_else(|e| panic!("{s}: {e}")) {
        SqlResult::Rows(r) => r.rows,
        SqlResult::Written(_) => Vec::new(),
    }
}

fn ints(rows: Vec<Vec<Value>>) -> Vec<i64> {
    rows.into_iter().map(|r| r[0].as_i64().unwrap_or_else(|| panic!("{r:?}"))).collect()
}

fn ids(rows: Vec<Vec<Value>>) -> Vec<String> {
    let mut v: Vec<String> = rows.into_iter().map(|r| r[0].as_str().unwrap().to_string()).collect();
    v.sort();
    v
}

const ROWS: usize = 2_000;

/// `users` of ROWS rows over flushed segments and the memtable: a text
/// key, and an INT, FLOAT, BOOL and TIMESTAMP that are NULL in some rows.
fn open(dir: &std::path::Path) -> Db {
    let seed = Schema::new("seed", vec![Field::new("id", FieldType::Keyword)], Some("id")).unwrap();
    let opts = DbOptions { sync: false, segment_max_docs: 300, ..DbOptions::default() };
    let db = Db::create(&dir.join("h.strata"), seed, opts).unwrap();
    sql(&db, "CREATE TABLE users (id KEYWORD PRIMARY KEY, n INT, x FLOAT, ok BOOL, at TIMESTAMP, tags KEYWORD[])");
    for chunk in (0..ROWS).collect::<Vec<_>>().chunks(250) {
        let values: Vec<String> = chunk
            .iter()
            .map(|&i| {
                let n = if i % 13 == 0 { "NULL".to_string() } else { (i as i64 - 700).to_string() };
                let at = chloedb::timestamp::format(1_785_542_400_000_000 + i as i64 * 3_600_000_000);
                format!("('user-{i:06}', {n}, {}, {}, '{at}', ARRAY['t{}'])", i as f64 / 4.0, i % 3 == 0, i % 5)
            })
            .collect();
        sql(&db, &format!("INSERT INTO users (id, n, x, ok, at, tags) VALUES {}", values.join(", ")));
        db.storage().flush().unwrap();
    }
    sql(&db, "INSERT INTO users (id, n) VALUES ('user-new', 5)");
    db
}

#[test]
fn hash_values_never_change() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path());
    let pinned: [(&str, i64); 14] = [
        ("hash('a')", 476372089961735745),
        ("hash('a', 1)", 6436786733673066638),
        ("hash('')", 4656582077437394441),
        ("hash('user-000042', 7)", 6461844273093359226),
        ("hash('héllo', 3)", 3366914536302459817),
        ("hash(42)", 2645380079577125725),
        ("hash(-1, 7)", 3449243486981488164),
        ("hash(9223372036854775807, -5)", 8904483932964078064),
        ("hash(2.5)", 346904539814786978),
        ("hash(-0.0)", 3763480590822581279),
        ("hash(0.0)", 3763480590822581279),
        ("hash(TRUE)", 2091210979730167326),
        ("hash(1, 0)", 2091210979730167326),
        ("hash(CAST('2026-08-01T00:00:00Z' AS TIMESTAMP))", 8939449009768427433),
    ];
    for (e, want) in pinned {
        let q = format!("SELECT {e} FROM users WHERE id = 'user-000001'");
        assert_eq!(sql(&db, &q), vec![vec![Value::Int(want)]], "{e}");
    }
    // A column's hash is its value's.
    let row = &sql(&db, "SELECT hash(id, 7), hash(n), hash(x), hash(ok), hash(at), hash(id) FROM users WHERE id = 'user-000042'")[0];
    let lit = &sql(&db, "SELECT hash('user-000042', 7), hash(-658), hash(10.5), hash(1), hash(1785693600000000), hash('user-000042') FROM users WHERE id = 'user-000001'")[0];
    assert_eq!(row, lit);
    assert_eq!(row[0], Value::Int(6461844273093359226));
    // NULL in, NULL out.
    let x = sql(&db, "SELECT hash(3.25) FROM users WHERE id = 'user-000001'")[0][0].clone();
    assert_eq!(sql(&db, "SELECT hash(n), hash(id, NULL), hash(x) FROM users WHERE id = 'user-000013'"), vec![vec![Value::Null, Value::Null, x]]);
}

#[test]
fn a_split_holds_every_row_once() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path());
    let all = ids(sql(&db, "SELECT id FROM users"));
    assert_eq!(all.len(), ROWS + 1);
    let train = ids(sql(&db, "SELECT id FROM users WHERE hash(id, 7) % 10 < 8"));
    let test = ids(sql(&db, "SELECT id FROM users WHERE hash(id, 7) % 10 >= 8"));
    assert!(train.iter().all(|t| test.binary_search(t).is_err()), "a row in both");
    let mut both: Vec<String> = train.iter().chain(&test).cloned().collect();
    both.sort();
    assert_eq!(both, all);
    // About four in five: 1,601 expected, 18 the standard deviation.
    assert!((1_540..=1_660).contains(&train.len()), "{} of {}", train.len(), all.len());
    // Another seed, another split; the same seed, the same one.
    let other = ids(sql(&db, "SELECT id FROM users WHERE hash(id, 8) % 10 < 8"));
    assert_ne!(other, train);
    assert_eq!(ids(sql(&db, "SELECT id FROM users WHERE hash(id, 7) % 10 < 8")), train);
    // The filter keeps the rows whose hash, as a column, passes it.
    let mut by_value: Vec<String> = sql(&db, "SELECT id, hash(id, 7) FROM users")
        .into_iter()
        .filter(|r| r[1].as_i64().unwrap() % 10 < 8)
        .map(|r| r[0].as_str().unwrap().to_string())
        .collect();
    by_value.sort();
    assert_eq!(by_value, train);
    // A shuffle: ORDER BY hash takes the smallest hashes first.
    let mut hashes = ints(sql(&db, "SELECT hash(id, 3) FROM users"));
    hashes.sort();
    assert_eq!(ints(sql(&db, "SELECT hash(id, 3) AS h FROM users ORDER BY hash(id, 3) LIMIT 25")), hashes[..25]);
    let first = sql(&db, "SELECT hash(id, 3) AS h, id FROM users ORDER BY h LIMIT 25");
    assert_eq!(ints(first.clone()), hashes[..25]);
    let ids_first: Vec<Value> = first.iter().map(|r| r[1].clone()).collect();
    let by_hash: Vec<Value> = sql(&db, "SELECT id FROM users ORDER BY hash(id, 3) LIMIT 25").into_iter().map(|r| r[0].clone()).collect();
    assert_eq!(by_hash, ids_first);
    let filtered: Vec<Value> = sql(&db, "SELECT id FROM users WHERE n > 0 ORDER BY hash(id, 3) DESC, id LIMIT 5").into_iter().map(|r| r[0].clone()).collect();
    assert_eq!(filtered.len(), 5);
    assert_eq!(ints(sql(&db, "WITH u AS (SELECT id FROM users) SELECT hash(id, 3) FROM u ORDER BY hash(id, 3) LIMIT 25")), hashes[..25]);
    // Rows without a value hash to NULL: in neither part.
    let some = sql(&db, "SELECT COUNT(*) FROM users WHERE hash(n, 1) % 2 = 0")[0][0].as_i64().unwrap();
    let rest = sql(&db, "SELECT COUNT(*) FROM users WHERE hash(n, 1) % 2 = 1")[0][0].as_i64().unwrap();
    assert_eq!((some + rest) as usize, ROWS + 1 - ROWS.div_ceil(13));
    // Over a join and a WITH too.
    let joined = ids(sql(&db, "WITH u AS (SELECT id FROM users WHERE n > 0) SELECT u.id FROM u JOIN users v ON u.id = v.id WHERE hash(u.id, 7) % 10 < 8"));
    let want: Vec<String> = ids(sql(&db, "SELECT id FROM users WHERE n > 0")).into_iter().filter(|i| train.binary_search(i).is_ok()).collect();
    assert_eq!(joined, want);
}

#[test]
fn hash_takes_one_value_and_an_int_seed() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path());
    for (q, msg) in [
        ("SELECT hash(id, 'x') FROM users", "hash's seed is an INT, not"),
        ("SELECT hash(id, 1.5) FROM users", "hash's seed is an INT, not"),
        ("SELECT hash(tags) FROM users", "hash takes one value"),
        ("SELECT hash() FROM users", "one or two arguments: a value and an INT seed"),
        ("SELECT hash(id, 1, 2) FROM users", "one or two arguments: a value and an INT seed"),
        ("SELECT id FROM users ORDER BY random()", "for a seeded shuffle, ORDER BY hash(id, 7)"),
        ("SELECT id FROM users ORDER BY rrf(n, hash(id, 7)) DESC", "hash() is a value of each row"),
        ("SELECT hsah(id) FROM users", "hash"),
    ] {
        let e = db.sql(q, &Params::new()).err().unwrap_or_else(|| panic!("{q} ran")).to_string();
        assert!(e.contains(msg), "{q}: {e}");
    }
}

#[test]
fn the_plan_says_it_hashes() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path());
    for q in ["SELECT id FROM users WHERE hash(id, 7) % 10 < 8", "SELECT id, hash(id) FROM users", "SELECT id FROM users ORDER BY hash(id, 2) LIMIT 5"] {
        let plan = db.explain(q, &Params::new()).unwrap();
        assert!(plan.features.iter().any(|f| f == HASH), "{q}: {:?}", plan.features);
        assert_eq!(Plan::from_json(&plan.to_json().unwrap()).unwrap(), plan, "{q}");
    }
    let plan = db.explain("SELECT id FROM users WHERE n > 3", &Params::new()).unwrap();
    assert!(!plan.features.iter().any(|f| f == HASH));
    // A Strata from before hash() refuses the plan rather than misread it.
    let before: Vec<&str> = FEATURES.iter().copied().filter(|f| *f != HASH).collect();
    let e = Plan::readable_by(1, &[HASH], &before).unwrap_err().to_string();
    assert!(e.contains("hash"), "{e}");
    assert!(Plan::readable_by(1, &[HASH], &FEATURES).is_ok());
}

#[cfg(feature = "datafusion")]
#[test]
fn datafusion_hashes_as_the_operators() {
    use chloedb::query::exec::{Engine, ExecOptions};
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path());
    let run = |q: &str, engine: Engine| {
        let opts = ExecOptions { engine, ..db.exec.clone() };
        db.query_with(q, &Params::new(), &opts).unwrap_or_else(|e| panic!("{q} ({engine:?}): {e}"))
    };
    for q in [
        "WITH u AS (SELECT id, n, x, ok, at FROM users) SELECT id, hash(id, 7), hash(n), hash(x, -3), hash(ok), hash(at) FROM u WHERE hash(id, 7) % 10 < 8 ORDER BY id",
        "SELECT a.id, hash(a.id, 9) % 100 FROM users a JOIN users b ON a.id = b.id WHERE hash(b.n, 2) % 3 = 0 ORDER BY a.id",
    ] {
        let want = run(q, Engine::Operators);
        let got = run(q, Engine::DataFusion);
        assert!(!got.stats.plan.iter().any(|l| l.starts_with("operators (not datafusion")), "{q}: {:?}", got.stats.plan.first());
        assert!(want.rows.len() > 100, "{q}: {}", want.rows.len());
        assert_eq!(want.rows, got.rows, "{q}");
    }
}
