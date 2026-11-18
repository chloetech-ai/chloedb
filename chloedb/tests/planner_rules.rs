//! The planner's rewrites change how a query runs, never what it answers.
//! Each query a rule rewrites runs beside a reference the rule leaves as
//! it is (the same condition behind `+ 0` or `COALESCE`, a derived table
//! of `*`, the whole order sliced), on every engine, and answers alike:
//!
//! * a join's small, filtered right side read first restricts the scan of
//!   its left side to the right rows' keys (`Ops::semi`);
//! * a WHERE that drops a LEFT JOIN's rows without a match makes it inner;
//! * a condition on one side's join key holds of the other side's key;
//! * a WHERE on a derived table's plain columns filters its table;
//! * `ORDER BY a, b .. LIMIT n` reads only the rows that tie with the
//!   first key's n-th value or beat it (`top_with_ties`).
//!
//! The operators' plans show that each rule ran.

use chloedb::db::Db;
use chloedb::query::exec::{Engine, ExecOptions, ResultSet};
use chloedb::query::Params;
use chloedb::storage::DbOptions;
use chloedb::value::Value;
use chloedb::{Field, FieldType, Schema};

fn open(dir: &std::path::Path) -> Db {
    let seed = Schema::new("seed", vec![Field::new("id", FieldType::Keyword)], Some("id")).unwrap();
    let db = Db::create(&dir.join("p.chloedb"), seed, DbOptions { sync: false, memtable_chunk_docs: 512, segment_max_docs: 6000, ..DbOptions::default() }).unwrap();
    for t in [
        "CREATE TABLE big (id INT PRIMARY KEY, k INT, g KEYWORD, x FLOAT, y INT)",
        "CREATE TABLE small (id INT PRIMARY KEY, c KEYWORD, w INT)",
        "CREATE TABLE kinds (code KEYWORD PRIMARY KEY, label KEYWORD)",
    ] {
        db.sql(t, &Params::new()).unwrap();
    }
    let rows: Vec<String> = (0..20_000)
        .map(|i| {
            let k = if i % 97 == 0 { "NULL".to_string() } else { (i % 400).to_string() };
            let x = if i % 53 == 0 { "NULL".to_string() } else { format!("{}.{}", (i * 7) % 1000, i % 10) };
            format!("({i}, {k}, '{}', {x}, {})", ["a", "b", "c", "d"][i % 4], i % 13)
        })
        .collect();
    for chunk in rows.chunks(1000) {
        db.sql(&format!("INSERT INTO big (id, k, g, x, y) VALUES {}", chunk.join(", ")), &Params::new()).unwrap();
    }
    let small: Vec<String> = (0..400).map(|i| format!("({i}, 'c{}', {})", i % 20, i % 7)).collect();
    db.sql(&format!("INSERT INTO small (id, c, w) VALUES {}", small.join(", ")), &Params::new()).unwrap();
    db.sql("INSERT INTO kinds (code, label) VALUES ('a', 'L0'), ('b', 'L1'), ('c', 'L1'), ('d', 'L2'), ('e', 'L1')", &Params::new()).unwrap();
    // Deleted and updated rows, and rows still in the memtable.
    db.sql("DELETE FROM big WHERE id % 101 = 0", &Params::new()).unwrap();
    db.sql("UPDATE big SET y = 99 WHERE id % 211 = 5", &Params::new()).unwrap();
    db
}

fn engines() -> Vec<Engine> {
    let mut e = vec![Engine::Operators];
    if cfg!(feature = "datafusion") {
        e.extend([Engine::Auto, Engine::DataFusion]);
    }
    e
}

fn run(db: &Db, sql: &str, engine: Engine) -> ResultSet {
    let opts = ExecOptions { engine, ..db.exec.clone() };
    db.query_with(sql, &Params::new(), &opts).unwrap_or_else(|e| panic!("{sql} ({engine:?}): {e}"))
}

fn same_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Float(x), Value::Float(y)) => x == y || (x - y).abs() <= 1e-9 * x.abs().max(y.abs()).max(1.0),
        (Value::Int(x), Value::Float(y)) | (Value::Float(y), Value::Int(x)) => (*x as f64 - y).abs() <= 1e-9 * y.abs().max(1.0),
        _ => a == b,
    }
}

/// The rows of `rs`, sorted when their order is not the query's.
fn rows_of(rs: &ResultSet, ordered: bool) -> Vec<Vec<Value>> {
    let mut rows = rs.rows.clone();
    if !ordered {
        rows.sort_by_key(|r| format!("{r:?}"));
    }
    rows
}

fn same_rows(what: &str, want: &[Vec<Value>], got: &[Vec<Value>]) {
    assert_eq!(want.len(), got.len(), "{what}: rows\nwant {want:?}\ngot  {got:?}");
    for (i, (w, g)) in want.iter().zip(got).enumerate() {
        assert!(w.len() == g.len() && w.iter().zip(g).all(|(a, b)| same_value(a, b)), "{what}: row {i}: want {w:?}, got {g:?}");
    }
}

/// `sql` (which a rule rewrites) answers as `reference` (which none
/// does) on every engine; its operators' plan has a line `fired` holds of.
fn check(db: &Db, sql: &str, reference: &str, ordered: bool, fired: &dyn Fn(&str) -> bool) -> usize {
    let mut n = 0;
    for engine in engines() {
        let got = run(db, sql, engine);
        let want = run(db, reference, engine);
        same_rows(&format!("{sql} ({engine:?})"), &rows_of(&want, ordered), &rows_of(&got, ordered));
        if engine == Engine::Operators {
            assert!(got.stats.plan.iter().any(|l| fired(l)), "{sql}: the rule did not run: {:#?}", got.stats.plan);
            assert!(!want.stats.plan.iter().any(|l| fired(l)) || reference.contains("+ 0"), "{reference}: a rule ran: {:#?}", want.stats.plan);
        }
        n = got.rows.len();
    }
    n
}

/// The scan of `table` restricted by a join's values.
fn restricted_scan(table: &'static str) -> impl Fn(&str) -> bool {
    move |l: &str| l.starts_with("scan:") && l.contains(&format!("Search {table}")) && l.contains("(restricted to")
}

#[test]
fn a_small_filtered_side_restricts_the_big_one() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path());
    let n = check(
        &db,
        "SELECT b.id, b.x, s.c FROM big b JOIN small s ON b.k = s.id WHERE s.c = 'c3'",
        "SELECT b.id, b.x, s.c FROM big b JOIN (SELECT * FROM small) s ON b.k = s.id WHERE s.c = 'c3'",
        false,
        &restricted_scan("big"),
    );
    assert!(n > 500, "{n} rows");
    // Two dimensions, one of them restricting.
    check(
        &db,
        "SELECT b.id, s1.c, s2.w FROM big b JOIN small s1 ON b.k = s1.id JOIN small s2 ON b.y = s2.id WHERE s1.c = 'c3' AND s2.w = 2",
        "SELECT b.id, s1.c, s2.w FROM big b JOIN (SELECT * FROM small) s1 ON b.k = s1.id JOIN (SELECT * FROM small) s2 ON b.y = s2.id WHERE s1.c = 'c3' AND s2.w = 2",
        false,
        &restricted_scan("big"),
    );
    // Groups, and a page of rows in the big side's order.
    check(
        &db,
        "SELECT s.c, COUNT(*) AS n, SUM(b.x) AS sx FROM big b JOIN small s ON b.k = s.id WHERE s.c IN ('c1', 'c2') GROUP BY s.c ORDER BY s.c",
        "SELECT s.c, COUNT(*) AS n, SUM(b.x) AS sx FROM big b JOIN (SELECT * FROM small) s ON b.k = s.id WHERE s.c IN ('c1', 'c2') GROUP BY s.c ORDER BY s.c",
        true,
        &restricted_scan("big"),
    );
    check(
        &db,
        "SELECT b.id, s.c FROM big b JOIN small s ON b.k = s.id WHERE s.c = 'c5' ORDER BY b.id LIMIT 7",
        "SELECT b.id, s.c FROM big b JOIN (SELECT * FROM small) s ON b.k = s.id WHERE s.c = 'c5' ORDER BY b.id LIMIT 7",
        true,
        &restricted_scan("big"),
    );
    // A KEYWORD key.
    check(
        &db,
        "SELECT b.id, k.label FROM big b JOIN kinds k ON b.g = k.code WHERE k.label = 'L0' AND b.y = 4",
        "SELECT b.id, k.label FROM big b JOIN (SELECT * FROM kinds) k ON b.g = k.code WHERE k.label = 'L0' AND b.y = 4",
        false,
        &restricted_scan("big"),
    );
    // No right row: nothing to restrict by, and no row.
    let rs = run(&db, "SELECT b.id FROM big b JOIN small s ON b.k = s.id WHERE s.c = 'none'", Engine::Operators);
    assert!(rs.rows.is_empty());
    // A right side that keeps most keys does not restrict.
    let rs = run(&db, "SELECT COUNT(*) FROM big b JOIN small s ON b.k = s.id WHERE s.w < 6", Engine::Operators);
    assert!(!rs.stats.plan.iter().any(|l| restricted_scan("big")(l)), "{:#?}", rs.stats.plan);
}

#[test]
fn a_where_that_drops_null_rows_makes_a_left_join_inner() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path());
    check(
        &db,
        "SELECT b.id, s.c FROM big b LEFT JOIN small s ON b.k = s.id WHERE s.c = 'c4'",
        "SELECT b.id, s.c FROM big b LEFT JOIN small s ON b.k = s.id WHERE COALESCE(s.c, '-') = 'c4'",
        false,
        &|l| l.starts_with("join (inner)") || restricted_scan("big")(l),
    );
    // IS NULL keeps the rows without a match, and an OR across the sides
    // may too: both stay LEFT JOINs, and answer as one.
    for (sql, reference) in [
        ("SELECT b.id FROM big b LEFT JOIN small s ON b.k = s.id WHERE s.c IS NULL", "SELECT b.id FROM big b WHERE b.k IS NULL"),
        (
            "SELECT b.id FROM big b LEFT JOIN small s ON b.k = s.id WHERE s.c = 'c4' OR b.y = 3",
            "SELECT b.id FROM big b LEFT JOIN small s ON b.k = s.id WHERE COALESCE(s.c, '-') = 'c4' OR b.y = 3",
        ),
    ] {
        for engine in engines() {
            same_rows(sql, &rows_of(&run(&db, reference, engine), false), &rows_of(&run(&db, sql, engine), false));
        }
    }
}

#[test]
fn a_condition_on_a_join_key_holds_of_the_other_key() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path());
    let filtered = |c: &'static str| move |l: &str| l.contains("Search big") && l.contains(c);
    check(
        &db,
        "SELECT COUNT(*) AS n, SUM(b.x) AS sx FROM big b JOIN small s ON b.k = s.id WHERE s.id = 74",
        "SELECT COUNT(*) AS n, SUM(b.x) AS sx FROM big b JOIN small s ON b.k = s.id WHERE s.id + 0 = 74",
        true,
        &filtered("k = 74"),
    );
    check(
        &db,
        "SELECT b.id, s.c FROM big b JOIN small s ON b.k = s.id WHERE s.id BETWEEN 10 AND 12 AND b.y = 1",
        "SELECT b.id, s.c FROM big b JOIN small s ON b.k = s.id WHERE s.id + 0 BETWEEN 10 AND 12 AND b.y = 1",
        false,
        &filtered("k BETWEEN 10 AND 12"),
    );
    // From the big side to the small one.
    check(
        &db,
        "SELECT b.id, s.c FROM big b JOIN small s ON b.k = s.id WHERE b.k IN (5, 6) AND b.y < 3",
        "SELECT b.id, s.c FROM big b JOIN small s ON b.k = s.id WHERE b.k + 0 IN (5, 6) AND b.y < 3",
        false,
        &|l| l.contains("Search small") && l.contains("id IN (5, 6)"),
    );
}

#[test]
fn a_where_on_a_derived_table_filters_its_table() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path());
    let filtered = |c: &'static str| move |l: &str| l.starts_with("query:") && l.contains("Search big") && l.contains(c);
    check(
        &db,
        "SELECT COUNT(*) AS n, SUM(d.v) AS s FROM (SELECT k AS kk, x AS v FROM big) AS d WHERE d.kk = 7",
        "SELECT COUNT(*) AS n, SUM(d.x) AS s FROM (SELECT * FROM big) AS d WHERE d.k = 7",
        true,
        &filtered("k = 7"),
    );
    check(
        &db,
        "SELECT kk, n FROM (SELECT k AS kk, COUNT(*) AS n FROM big GROUP BY k) AS d WHERE d.kk IN (1, 2, 3) ORDER BY kk",
        "SELECT kk, n FROM (SELECT k AS kk, COUNT(*) AS n FROM big GROUP BY k) AS d WHERE d.kk + 0 IN (1, 2, 3) ORDER BY kk",
        true,
        &filtered("k IN (1, 2, 3)"),
    );
    check(
        &db,
        "SELECT COUNT(*) AS n FROM (SELECT k, y FROM big) AS d WHERE k = 3 AND y > 4",
        "SELECT COUNT(*) AS n FROM (SELECT k, y FROM big) AS d WHERE k + 0 = 3 AND y + 0 > 4",
        true,
        &filtered("k = 3"),
    );
    check(
        &db,
        "SELECT COUNT(*) AS n FROM (SELECT k AS kk FROM big) AS d WHERE d.kk IS NULL",
        "SELECT COUNT(*) AS n FROM (SELECT * FROM big) AS d WHERE d.k IS NULL",
        true,
        &filtered("k IS NULL"),
    );
    // An aggregate's column is not the table's: the groups are filtered.
    for engine in engines() {
        let sql = "SELECT kk, n FROM (SELECT k AS kk, COUNT(*) AS n FROM big GROUP BY k) AS d WHERE d.n > 49 ORDER BY kk";
        let reference = "SELECT kk, n FROM (SELECT k AS kk, COUNT(*) AS n FROM big GROUP BY k) AS d WHERE d.n + 0 > 49 ORDER BY kk";
        same_rows(sql, &run(&db, reference, engine).rows, &run(&db, sql, engine).rows);
    }
}

#[test]
fn order_by_two_keys_with_a_limit_reads_the_ties_of_the_first() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path());
    let probe = |l: &str| l.starts_with("subquery:") && l.contains("__k");
    // A total order (`id` last breaks every tie), so that the page is one
    // slice of it. ORDER BY a key and the primary key alone is the scan's
    // own top-k already.
    for (select, order, offset, limit) in [
        ("SELECT id, y, x FROM big", "y DESC, x, id", 0, 10),
        ("SELECT id, y, x FROM big", "x DESC, y, id", 3, 5),
        ("SELECT id, y, x FROM big", "x, y DESC, id DESC", 0, 6),
        ("SELECT id, y, x FROM big", "x DESC NULLS LAST, y, id", 0, 4),
        ("SELECT id, y, x FROM big WHERE g = 'b'", "y, x DESC, id", 0, 9),
        ("SELECT id, y AS yy, x FROM big", "yy DESC, x, 1", 0, 3),
        ("SELECT id, y, x FROM big", "2 DESC, 3, 1", 0, 3),
        ("SELECT id, y, x FROM big WHERE id < 5", "y DESC, x, id", 0, 50),
        ("SELECT id, x, y FROM big WHERE x IS NULL", "x DESC, y, id", 0, 3),
        ("SELECT b.id, b.y, b.x FROM big b WHERE b.g = 'a'", "b.y DESC, b.x, b.id", 0, 5),
    ] {
        let sql = format!("{select} ORDER BY {order} LIMIT {limit} OFFSET {offset}");
        let whole = format!("{select} ORDER BY {order}");
        for engine in engines() {
            let all = run(&db, &whole, engine).rows;
            let want: Vec<Vec<Value>> = all.into_iter().skip(offset).take(limit).collect();
            let got = run(&db, &sql, engine);
            same_rows(&format!("{sql} ({engine:?})"), &want, &got.rows);
            if engine == Engine::Operators {
                assert!(got.stats.plan.iter().any(|l| probe(l)), "{sql}: {:#?}", got.stats.plan);
            }
        }
    }
}
