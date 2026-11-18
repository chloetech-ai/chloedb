//! SQL that answers as PostgreSQL does, from the persona study of 17
//! November (Sam, an analytics engineer, comparing 75 queries with DuckDB
//! and PostgreSQL 16 on 0.3.7):
//!
//! - SAM-2: UPDATE and DELETE take any WHERE that SELECT takes: a TEXT
//!   column compared or matched with LIKE, a subquery of another table,
//!   NOT EXISTS. The rows are picked by a SELECT of their keys, under the
//!   writer lock, then written.
//! - SAM-3: GROUP BY and SELECT DISTINCT of a TEXT column over one table.
//! - SAM-5: `'a' || NULL` is NULL, and a FLOAT rounds half to even in
//!   `ROUND(x)` and `CAST(x AS INT)`, as PostgreSQL's float8 does (a
//!   number written out still rounds half away from zero, as its numeric
//!   does).
//! - SAM-6 (audit ENG-Q4): `n != 1.5` and `NOT (n = 1.5)` on an INT column,
//!   which no row's value equals, leave out the rows without a value.

use chloedb::db::{Db, SqlResult};
use chloedb::query::exec::{Engine, ExecOptions};
use chloedb::query::Params;
use chloedb::schema::{Field, FieldType, Schema};
use chloedb::storage::DbOptions;
use chloedb::value::Value;

/// A database whose first table is `payroll`, as the study's was, and
/// table `notes` (id KEYWORD key, cat TEXT, body TEXT, n INT, x FLOAT)
/// with 8 rows over several segments, and `refs` (id INT key, note
/// KEYWORD, ok INT) naming some of them.
fn db(dir: &std::path::Path) -> Db {
    let payroll = Schema::new("payroll", vec![Field::new("id", FieldType::Int), Field::new("who", FieldType::Keyword)], Some("id")).unwrap();
    let db = Db::create(&dir.join("pg.chloedb"), payroll, DbOptions { sync: false, segment_max_docs: 3, ..DbOptions::default() }).unwrap();
    for s in [
        "CREATE TABLE notes (id KEYWORD PRIMARY KEY, cat TEXT, body TEXT, n INT, x FLOAT)",
        "CREATE TABLE refs (id INT PRIMARY KEY, note KEYWORD, ok INT)",
        "INSERT INTO payroll (id, who) VALUES (1, 'ana'), (2, 'sam')",
        "INSERT INTO notes (id, cat, body, n, x) VALUES ('a', 'x', 'first note', 1, 0.5), ('b', 'x', 'second note', 2, 1.5), ('c', 'xy', 'third', NULL, 2.5)",
        "INSERT INTO notes (id, cat, body, n, x) VALUES ('d', 'y', NULL, 4, -0.5), ('e', NULL, 'fifth', 5, -2.5), ('f', 'Y', 'sixth', NULL, 3.5)",
        "INSERT INTO notes (id, cat, body, n, x) VALUES ('g', 'x', 'seventh', 7, 0.25), ('h', 'zz', 'eighth', 8, NULL)",
        "INSERT INTO refs (id, note, ok) VALUES (1, 'a', 1), (2, 'c', 0), (3, 'g', 1), (4, 'nope', 1)",
    ] {
        sql(&db, s);
    }
    db
}

fn sql(db: &Db, s: &str) -> Vec<Vec<Value>> {
    match db.sql(s, &Params::new()).unwrap_or_else(|e| panic!("{s}: {e}")) {
        SqlResult::Rows(r) => r.rows,
        SqlResult::Written(n) => vec![vec![Value::Int(n as i64)]],
    }
}

fn written(db: &Db, s: &str) -> i64 {
    match db.sql(s, &Params::new()).unwrap_or_else(|e| panic!("{s}: {e}")) {
        SqlResult::Written(n) => n as i64,
        SqlResult::Rows(_) => panic!("{s}: expected a write"),
    }
}

/// The first column of each row, as text, sorted.
fn keys(db: &Db, q: &str) -> Vec<String> {
    let mut v: Vec<String> = sql(db, q).into_iter().map(|r| r[0].as_str().unwrap_or("NULL").to_string()).collect();
    v.sort();
    v
}

fn count(db: &Db, q: &str) -> i64 {
    sql(db, q)[0][0].as_i64().unwrap()
}

fn s(x: &str) -> Value {
    Value::Str(x.into())
}

/// SAM-2: each WHERE that a SELECT takes, an UPDATE and a DELETE take too,
/// and change the rows the SELECT counts.
#[test]
fn writes_take_the_where_select_takes() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path());
    // A TEXT column compared with =, as SELECT compares it.
    assert_eq!(count(&db, "SELECT COUNT(*) FROM notes WHERE cat = 'x'"), 3);
    assert_eq!(written(&db, "UPDATE notes SET body = 'z' WHERE cat = 'x'"), 3);
    assert_eq!(keys(&db, "SELECT id FROM notes WHERE body = 'z'"), ["a", "b", "g"]);
    // A subquery of another table.
    assert_eq!(written(&db, "UPDATE notes SET n = 100 WHERE id IN (SELECT note FROM refs WHERE ok = 1)"), 2);
    assert_eq!(keys(&db, "SELECT id FROM notes WHERE n = 100"), ["a", "g"]);
    // LIKE of a TEXT column, and a function of a column.
    assert_eq!(written(&db, "UPDATE notes SET n = 0 WHERE cat LIKE 'x%' AND lower(body) <> 'z'"), 1);
    assert_eq!(keys(&db, "SELECT id FROM notes WHERE n = 0"), ["c"]);
    // A correlated NOT EXISTS, and an expression SET on the picked rows.
    assert_eq!(written(&db, "UPDATE notes SET x = x * 2 WHERE NOT EXISTS (SELECT 1 FROM refs r WHERE r.note = notes.id) AND x > 0"), 2);
    assert_eq!(sql(&db, "SELECT x FROM notes WHERE id IN ('b', 'f') ORDER BY id"), [[Value::Float(3.0)], [Value::Float(7.0)]]);
    // ORDER BY and LIMIT pick the rows, as a SELECT orders them.
    assert_eq!(written(&db, "DELETE FROM notes WHERE cat LIKE '%y%' ORDER BY id DESC LIMIT 1"), 1);
    assert_eq!(keys(&db, "SELECT id FROM notes WHERE cat LIKE '%y%'"), ["c"]);
    assert_eq!(written(&db, "UPDATE notes SET n = -1 WHERE lower(cat) = 'y' LIMIT 5"), 1);
    assert_eq!(keys(&db, "SELECT id FROM notes WHERE n = -1"), ["f"]);
    // DELETE by a TEXT comparison and by a subquery.
    assert_eq!(written(&db, "DELETE FROM notes WHERE cat = 'zz'"), 1);
    assert_eq!(written(&db, "DELETE FROM notes WHERE id IN (SELECT note FROM refs WHERE ok = 0)"), 1);
    assert_eq!(keys(&db, "SELECT id FROM notes"), ["a", "b", "e", "f", "g"]);
    // Nothing matches: nothing written.
    assert_eq!(written(&db, "UPDATE notes SET n = 1 WHERE cat = 'none of these'"), 0);
    // What SELECT refuses, the write refuses with SELECT's words.
    let refused = db.sql("SELECT id FROM notes WHERE catt = 'x'", &Params::new()).unwrap_err().to_string();
    for w in ["UPDATE notes SET n = 1 WHERE catt = 'x'", "DELETE FROM notes WHERE catt = 'x'"] {
        let e = db.sql(w, &Params::new()).unwrap_err().to_string();
        assert!(e.starts_with(refused.split(" (in ").next().unwrap()), "{w}: {e}, not as SELECT: {refused}");
    }
    // The plan says how the rows are picked.
    let plan: Vec<String> = sql(&db, "EXPLAIN UPDATE notes SET n = 1 WHERE cat = 'x'").into_iter().map(|r| r[0].as_str().unwrap().to_string()).collect();
    assert!(plan.iter().any(|l| l.contains("picks")), "{plan:?}");
    // Writes of a WHERE the scan answers are bound as before.
    assert_eq!(written(&db, "UPDATE notes SET n = 9 WHERE id = 'a'"), 1);
    assert_eq!(written(&db, "DELETE FROM payroll WHERE who = 'sam'"), 1);
}

/// SAM-2: a write's rows are picked under the writer lock, of the rows as
/// they are then: two writes that pick the same rows by a TEXT comparison
/// change each row once.
#[test]
fn a_picked_write_sees_the_write_before_it() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path());
    let a = db.prepare_write("UPDATE notes SET cat = 'moved' WHERE cat = 'x'", &Params::new()).unwrap().unwrap();
    let b = db.prepare_write("DELETE FROM notes WHERE cat = 'x'", &Params::new()).unwrap().unwrap();
    assert_eq!(db.write_prepared(&a).unwrap(), 3);
    // Prepared before the update, it picks its rows after it: none left.
    assert_eq!(db.write_prepared(&b).unwrap(), 0);
    assert_eq!(count(&db, "SELECT COUNT(*) FROM notes"), 8);
    // Two picked writes do not share a transaction (they may touch the
    // same rows).
    let c = db.prepare_write("UPDATE notes SET n = 1 WHERE cat = 'moved'", &Params::new()).unwrap().unwrap();
    let d = db.prepare_write("UPDATE notes SET n = 2 WHERE cat = 'y'", &Params::new()).unwrap().unwrap();
    assert!(db.write_prepared_group(&[&c, &d]).is_err());
}

/// SAM-3: a TEXT column groups over one table as over derived rows, its
/// rows without a value in a group of their own.
#[test]
fn group_by_and_distinct_of_text() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path());
    let want = vec![
        vec![Value::Null, Value::Int(1)],
        vec![s("Y"), Value::Int(1)],
        vec![s("x"), Value::Int(3)],
        vec![s("xy"), Value::Int(1)],
        vec![s("y"), Value::Int(1)],
        vec![s("zz"), Value::Int(1)],
    ];
    for q in [
        "SELECT cat, COUNT(*) FROM notes GROUP BY cat ORDER BY cat NULLS FIRST",
        "SELECT cat, COUNT(*) FROM notes GROUP BY 1 ORDER BY 1 NULLS FIRST",
        "SELECT cat, COUNT(*) FROM (SELECT cat FROM notes) t GROUP BY cat ORDER BY cat NULLS FIRST",
    ] {
        assert_eq!(sql(&db, q), want, "{q}");
    }
    assert_eq!(sql(&db, "SELECT cat AS c, COUNT(*) AS n FROM notes WHERE n > 1 GROUP BY c HAVING COUNT(*) > 1"), vec![vec![s("x"), Value::Int(2)]]);
    assert_eq!(keys(&db, "SELECT DISTINCT cat FROM notes"), ["NULL", "Y", "x", "xy", "y", "zz"]);
    assert_eq!(sql(&db, "SELECT DISTINCT cat FROM notes WHERE cat LIKE 'x%' ORDER BY cat"), vec![vec![s("x")], vec![s("xy")]]);
    assert_eq!(sql(&db, "SELECT cat, SUM(n) FROM notes GROUP BY cat ORDER BY SUM(n) DESC LIMIT 1"), vec![vec![s("x"), Value::Int(10)]]);
}

/// SAM-5: `||` with a NULL side is NULL, as in PostgreSQL, over one table,
/// derived rows and both engines, and in an UPDATE's SET.
#[test]
fn concatenation_with_null_is_null() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path());
    assert_eq!(sql(&db, "SELECT 'a' || NULL"), vec![vec![Value::Null]]);
    assert_eq!(sql(&db, "SELECT NULL || 'a', 'a' || 'b' || 'c'"), vec![vec![Value::Null, s("abc")]]);
    for engine in [Engine::Operators, Engine::DataFusion] {
        let opts = ExecOptions { engine, ..db.exec.clone() };
        for q in ["SELECT id, cat || '!' FROM notes ORDER BY id", "SELECT id, cat || '!' FROM (SELECT id, cat FROM notes) t ORDER BY id"] {
            let rows = db.query_with(q, &Params::new(), &opts).unwrap().rows;
            let e = rows.iter().find(|r| r[0] == s("e")).unwrap();
            assert_eq!(e[1], Value::Null, "{q} ({engine:?})");
            let a = rows.iter().find(|r| r[0] == s("a")).unwrap();
            assert_eq!(a[1], s("x!"), "{q} ({engine:?})");
        }
    }
    // coalesce keeps the other side, as in PostgreSQL.
    assert_eq!(sql(&db, "SELECT coalesce(cat, '') || '!' FROM notes WHERE id = 'e'"), vec![vec![s("!")]]);
    assert_eq!(written(&db, "UPDATE notes SET body = body || ' (old)' WHERE id IN ('a', 'd')"), 2);
    assert_eq!(sql(&db, "SELECT id, body FROM notes WHERE id IN ('a', 'd') ORDER BY id"), vec![vec![s("a"), s("first note (old)")], vec![s("d"), Value::Null]]);
}

/// SAM-5: a FLOAT value rounds half to even in ROUND(x) and CAST(x AS
/// INT), as PostgreSQL's float8 does; a number written out rounds half away
/// from zero, as PostgreSQL's numeric literal does. ROUND(x) is an INT, as
/// before.
#[test]
fn floats_round_half_to_even() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path());
    // x: a 0.5, b 1.5, c 2.5, d -0.5, e -2.5, f 3.5, g 0.25.
    let want = vec![
        vec![s("a"), Value::Int(0), Value::Int(0)],
        vec![s("b"), Value::Int(2), Value::Int(2)],
        vec![s("c"), Value::Int(2), Value::Int(2)],
        vec![s("d"), Value::Int(0), Value::Int(0)],
        vec![s("e"), Value::Int(-2), Value::Int(-2)],
        vec![s("f"), Value::Int(4), Value::Int(4)],
        vec![s("g"), Value::Int(0), Value::Int(0)],
    ];
    for engine in [Engine::Operators, Engine::DataFusion] {
        let opts = ExecOptions { engine, ..db.exec.clone() };
        for q in [
            "SELECT id, ROUND(x), CAST(x AS INT) FROM notes WHERE x IS NOT NULL ORDER BY id",
            "SELECT id, ROUND(x), x::int FROM (SELECT id, x FROM notes WHERE x IS NOT NULL) t ORDER BY id",
        ] {
            assert_eq!(db.query_with(q, &Params::new(), &opts).unwrap().rows, want, "{q} ({engine:?})");
        }
    }
    // Groups of CAST(x AS INT), as Sam's battery made them.
    assert_eq!(sql(&db, "SELECT CAST(x AS INT) AS b, COUNT(*) FROM notes WHERE x IS NOT NULL GROUP BY b ORDER BY b"), vec![
        vec![Value::Int(-2), Value::Int(1)],
        vec![Value::Int(0), Value::Int(3)],
        vec![Value::Int(2), Value::Int(2)],
        vec![Value::Int(4), Value::Int(1)],
    ]);
    // Numbers written out, as PostgreSQL's numeric.
    assert_eq!(sql(&db, "SELECT ROUND(0.5), CAST(0.5 AS INT), ROUND(2.5), CAST(-2.5 AS INT)"), vec![vec![Value::Int(1), Value::Int(1), Value::Int(3), Value::Int(-3)]]);
    // ROUND with digits keeps its rule (PostgreSQL has it for numeric alone),
    // ROUND(x, 0) too, as round(x::numeric, 0) rounds.
    assert_eq!(sql(&db, "SELECT ROUND(x, 1) FROM notes WHERE id = 'g'"), vec![vec![Value::Float(0.3)]]);
    assert_eq!(sql(&db, "SELECT ROUND(x, 0) FROM notes WHERE id = 'c'"), vec![vec![Value::Int(3)]]);
    // A value made of numbers written out alone is a numeric to
    // PostgreSQL (`CASE WHEN .. THEN 2.5 END`), on either engine; one that
    // reads x is a float8's.
    for engine in [Engine::Operators, Engine::DataFusion] {
        let opts = ExecOptions { engine, ..db.exec.clone() };
        let q = "SELECT ROUND(CASE WHEN x > 0 THEN 2.5 ELSE -2.5 END), CAST(CASE WHEN x > 0 THEN 2.5 END AS INT), ROUND(coalesce(NULL, 0.5)), ROUND(x * 1) FROM notes WHERE id = 'c'";
        assert_eq!(db.query_with(q, &Params::new(), &opts).unwrap().rows, vec![vec![Value::Int(3), Value::Int(3), Value::Int(1), Value::Int(2)]], "{q} ({engine:?})");
    }
}

/// SAM-6: a comparison with a value no INT holds is FALSE of each row with
/// a value and UNKNOWN of a row without one, so neither it nor its NOT
/// keeps the rows without a value.
#[test]
fn impossible_comparisons_leave_out_nulls() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path());
    // n: a 1, b 2, c NULL, d 4, e 5, f NULL, g 7, h 8.
    let with_n = ["a", "b", "d", "e", "g", "h"];
    for q in [
        "SELECT id FROM notes WHERE n != 1.5",
        "SELECT id FROM notes WHERE n <> 1.5",
        "SELECT id FROM notes WHERE NOT (n = 1.5)",
        "SELECT id FROM notes WHERE NOT (n BETWEEN 1.2 AND 1.8)",
        "SELECT id FROM notes WHERE NOT (n > 1e30)",
        "SELECT id FROM (SELECT id, n FROM notes) t WHERE n != 1.5",
    ] {
        assert_eq!(keys(&db, q), with_n, "{q}");
    }
    assert_eq!(count(&db, "SELECT COUNT(*) FROM notes WHERE n != 1.5"), 6);
    assert!(keys(&db, "SELECT id FROM notes WHERE n = 1.5").is_empty());
    assert_eq!(keys(&db, "SELECT id FROM notes WHERE n = 1.5 OR n IS NULL"), ["c", "f"]);
    // As before: a value an INT holds.
    assert_eq!(keys(&db, "SELECT id FROM notes WHERE n != 2"), ["a", "d", "e", "g", "h"]);
    assert_eq!(written(&db, "DELETE FROM notes WHERE n != 1.5"), 6);
    assert_eq!(keys(&db, "SELECT id FROM notes"), ["c", "f"]);
}
