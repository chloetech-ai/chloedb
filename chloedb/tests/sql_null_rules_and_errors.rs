//! Round 4's SQL findings (SAM-401, 403, 404, 405): NULL rules that are
//! the same in SELECT and in every UPDATE expression, errors that say what
//! is wrong, a table qualifier that names no table, and EXPLAIN of writes.

use chloedb::db::{Db, SqlResult};
use chloedb::query::Params;
use chloedb::schema::{Field, FieldType, Schema};
use chloedb::storage::DbOptions;
use chloedb::value::Value;

fn db(dir: &std::path::Path) -> Db {
    let seed = Schema::new("seed", vec![Field::new("id", FieldType::Keyword)], Some("id")).unwrap();
    let db = Db::create(&dir.join("d.strata"), seed, DbOptions { sync: false, ..DbOptions::default() }).unwrap();
    for s in [
        "CREATE TABLE trips (trip_id INT PRIMARY KEY, VendorID KEYWORD, payment_type INT, total_amount FLOAT, n INT)",
        "INSERT INTO trips VALUES (1, 'a', 1, 10.5, NULL), (2, 'a', 2, 20.0, 5), (3, 'b', 1, 3.0, 7)",
        "CREATE TABLE zones (id INT PRIMARY KEY, Zone KEYWORD)",
        "INSERT INTO zones VALUES (1, 'Park Hill'), (2, 'Harbor')",
    ] {
        db.sql(s, &Params::new()).unwrap_or_else(|e| panic!("{s}: {e}"));
    }
    db
}

fn rows(db: &Db, s: &str) -> Vec<Vec<Value>> {
    match db.sql(s, &Params::new()).unwrap_or_else(|e| panic!("{s}: {e}")) {
        SqlResult::Rows(r) => r.rows,
        SqlResult::Written(_) => Vec::new(),
    }
}

fn fails(db: &Db, s: &str) -> String {
    match db.sql(s, &Params::new()) {
        Ok(_) => panic!("{s} should fail"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn update_follows_the_null_rules_of_select() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path());
    // Every form, in either order: SELECT and SET agree, and a NULL stays NULL.
    for e in ["n + 1", "1 + n", "n - 1", "1 - n", "n * 2", "2 * n", "n + 0", "0 + n", "n / 2", "-n", "n + n", "(n + 1) * 2", "total_amount - 2 + n"] {
        assert_eq!(rows(&db, &format!("SELECT {e} FROM trips WHERE trip_id = 1")), vec![vec![Value::Null]], "SELECT {e}");
        db.sql(&format!("UPDATE trips SET n = {e} WHERE trip_id = 1"), &Params::new()).unwrap_or_else(|x| panic!("{e}: {x}"));
        assert_eq!(rows(&db, "SELECT n FROM trips WHERE trip_id = 1"), vec![vec![Value::Null]], "SET n = {e}");
    }
    db.sql("UPDATE trips SET total_amount = NULL WHERE trip_id = 1", &Params::new()).unwrap();
    for e in ["total_amount - 2", "3 + total_amount", "total_amount * 2", "total_amount + 0"] {
        db.sql(&format!("UPDATE trips SET total_amount = {e} WHERE trip_id = 1"), &Params::new()).unwrap();
        assert_eq!(rows(&db, "SELECT total_amount FROM trips WHERE trip_id = 1"), vec![vec![Value::Null]], "SET total_amount = {e}");
    }
    // Values that are there still add, and coalesce spells a counter.
    db.sql("UPDATE trips SET n = n + 1 WHERE trip_id = 2", &Params::new()).unwrap();
    db.sql("UPDATE trips SET n = 1 + n WHERE trip_id = 2", &Params::new()).unwrap();
    db.sql("UPDATE trips SET n = coalesce(n, 0) + 1 WHERE trip_id = 1", &Params::new()).unwrap();
    assert_eq!(rows(&db, "SELECT n FROM trips ORDER BY trip_id"), vec![vec![Value::Int(1)], vec![Value::Int(7)], vec![Value::Int(7)]]);
}

#[test]
fn mistakes_say_what_is_wrong() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path());
    for (sql, want) in [
        ("SELECT VendorID, COUNT(*) FROM trips GROUP BY VendorID HAVING total_amount > 10", "total_amount must be in GROUP BY or inside an aggregate"),
        ("SELECT VendorID, COUNT(*) FROM trips GROUP BY VendorID HAVING payment_type = 1", "payment_type must be in GROUP BY or inside an aggregate"),
        ("SELECT VendorID FROM trips WHERE COUNT(*) > 1 GROUP BY VendorID", "an aggregate is not allowed in WHERE: use HAVING"),
        ("SELECT COUNT(*) FROM zones WHERE Zone ILIKE ANY (ARRAY['%park%', '%hill%'])", "ILIKE ANY is not supported"),
        ("WITH RECURSIVE r(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM r WHERE n < 5) SELECT n FROM r", "WITH RECURSIVE is not supported"),
    ] {
        let e = fails(&db, sql);
        assert!(e.contains(want), "{sql}: {e}");
    }
    // What works keeps working.
    assert_eq!(rows(&db, "SELECT COUNT(*) FROM zones WHERE Zone ILIKE '%park%'"), vec![vec![Value::Int(1)]]);
    assert_eq!(rows(&db, "SELECT VendorID FROM trips GROUP BY VendorID HAVING COUNT(*) > 1"), vec![vec![Value::Str("a".into())]]);
}

#[test]
fn a_qualifier_names_a_table_of_the_query() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path());
    let e = fails(&db, "SELECT nosuch.trip_id FROM trips WHERE bar.trip_id < 3");
    assert!(e.contains("no table nosuch in FROM"), "{e}");
    let e = fails(&db, "SELECT t.trip_id FROM trips LIMIT 1");
    assert!(e.contains("no table t in FROM"), "{e}");
    let e = fails(&db, "SELECT trip_id FROM trips ORDER BY x.trip_id");
    assert!(e.contains("no table x in FROM"), "{e}");
    let e = fails(&db, "SELECT trip_id FROM trips AS t WHERE u.n > 1");
    assert!(e.contains("no table u in FROM"), "{e}");
    // The table, its alias and a subquery's own tables are fine.
    assert_eq!(rows(&db, "SELECT trips.trip_id FROM trips WHERE trips.n = 5").len(), 1);
    assert_eq!(rows(&db, "SELECT t.trip_id FROM trips AS t WHERE t.n = 5 ORDER BY t.trip_id").len(), 1);
    assert_eq!(rows(&db, "SELECT t.trip_id FROM trips t WHERE EXISTS (SELECT 1 FROM zones z WHERE z.id = t.trip_id)").len(), 2);
}

#[test]
fn explain_takes_writes() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path());
    for sql in ["EXPLAIN SELECT COUNT(*) FROM trips WHERE payment_type = 1", "EXPLAIN UPDATE trips SET n = 1 WHERE trip_id = 1", "EXPLAIN DELETE FROM trips WHERE trip_id = 1"] {
        let r = rows(&db, sql);
        assert!(!r.is_empty(), "{sql}");
    }
    // Explaining writes nothing.
    assert_eq!(rows(&db, "SELECT COUNT(*) FROM trips"), vec![vec![Value::Int(3)]]);
}
