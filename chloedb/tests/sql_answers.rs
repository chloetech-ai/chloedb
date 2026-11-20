//! SQL that answered differently from PostgreSQL, DuckDB and SQLite, or
//! refused in a way that sent a SQL user the wrong way, found by round 2
//! of the persona study (18 November 2026, Sam's findings):
//!
//! - SAM-201: `ORDER BY` an output column's alias after `GROUP BY` or
//!   `DISTINCT` was "no such column", where an aggregate's alias worked.
//! - SAM-202: `json_extract(x, '$.a.b')` was NULL in `SELECT`, with no
//!   error, where `'a.b'` worked and `'$.a.b'` worked in `WHERE`.
//! - SAM-204: `ORDER BY x DESC NULLS FIRST` was refused in a grouped
//!   query, though the docs recommend it to get PostgreSQL's order.
//! - SAM-208: refusals of ROLLUP, GROUPING SETS, `left`, `JOIN ...
//!   USING`, `INSERT ... SELECT`, `CREATE TABLE ... AS SELECT` (which run
//!   since SAM-8), temporary tables, a join on no equality and a time minus a time that did not say
//!   what to write, or named tables that are not in the database.
//!
//! SAM-203 (float sums on either engine) is in `datafusion_engine.rs`, which
//! CI runs with the feature `datafusion`; chloedb-server's
//! `tests/silent_answers.rs` asks the same over HTTP.

use chloedb::db::{Db, SqlResult};
use chloedb::query::Params;
use chloedb::storage::DbOptions;
use chloedb::value::Value;

/// Trips by payment type (an indexed INT, NULL on six rows) and zone (a
/// KEYWORD), and invoices by country (a TEXT column, which the row
/// evaluator groups).
fn db(dir: &std::path::Path) -> Db {
    let db = Db::create_empty(&dir.join("t.chloedb"), DbOptions { sync: false, ..DbOptions::default() }).unwrap();
    let p = Params::new();
    db.sql("CREATE TABLE trips (id INT PRIMARY KEY, payment_type INT, zone KEYWORD, fare FLOAT)", &p).unwrap();
    let mut rows = Vec::new();
    for i in 0..30 {
        // NULL on rows 0 to 5, then 1, 2 and 3 on 8 rows each.
        let pt = if i < 6 { "NULL".to_string() } else { ((i - 6) / 8 + 1).to_string() };
        let zone = ["Bronx", "Queens", "Brooklyn"][i % 3];
        rows.push(format!("({i}, {pt}, '{zone}', {}.5)", i % 7));
    }
    db.sql(&format!("INSERT INTO trips (id, payment_type, zone, fare) VALUES {}", rows.join(", ")), &p).unwrap();
    db.sql("CREATE TABLE invoice (id INT PRIMARY KEY, country TEXT, total FLOAT)", &p).unwrap();
    db.sql("INSERT INTO invoice (id, country, total) VALUES (1, 'USA', 1.98), (2, 'USA', 3.96), (3, 'Canada', 5.94), (4, 'USA', 0.99), (5, 'Brazil', 1.98), (6, 'Canada', 8.91)", &p).unwrap();
    db.sql("CREATE TABLE products (id KEYWORD PRIMARY KEY, attrs JSON, doc TEXT)", &p).unwrap();
    db.sql(r#"INSERT INTO products (id, attrs, doc) VALUES ('p1', '{"voltage": 5, "dims": {"w": 40.5}}', '{"a": {"b": 1}}'), ('p2', '{"color": "black"}', '{"a": {"b": 2}}')"#, &p).unwrap();
    db
}

fn rows(db: &Db, sql: &str) -> Vec<Vec<Value>> {
    db.query(sql, &Params::new()).unwrap_or_else(|e| panic!("{sql}: {e}")).rows
}

fn refused(db: &Db, sql: &str) -> String {
    let p = Params::new();
    match db.query(sql, &p) {
        Ok(r) => panic!("{sql} was answered ({} rows), not refused", r.rows.len()),
        Err(e) => e.to_string(),
    }
}

fn refused_write(db: &Db, sql: &str) -> String {
    match db.sql(sql, &Params::new()) {
        Ok(_) => panic!("{sql} ran, and was not refused"),
        Err(e) => e.to_string(),
    }
}

fn written(db: &Db, sql: &str) -> u64 {
    match db.sql(sql, &Params::new()) {
        Ok(SqlResult::Written(n)) => n,
        Ok(SqlResult::Rows(r)) => panic!("{sql} answered {} rows, and wrote none", r.rows.len()),
        Err(e) => panic!("{sql}: {e}"),
    }
}

fn int(x: i64) -> Value {
    Value::Int(x)
}

fn text(s: &str) -> Value {
    Value::Str(s.to_string())
}

#[test]
fn order_by_an_output_column_s_alias_after_group_by_and_distinct() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path());
    // payment_type: 1 on 8 rows, 2 on 8, 3 on 8, NULL on 6; NULL's group last.
    let by_pt = vec![vec![int(1), int(8)], vec![int(2), int(8)], vec![int(3), int(8)], vec![Value::Null, int(6)]];
    for sql in [
        "SELECT payment_type AS pt, COUNT(*) AS n FROM trips GROUP BY pt ORDER BY pt",
        "SELECT payment_type AS pt, COUNT(*) AS n FROM trips GROUP BY payment_type ORDER BY pt",
        "SELECT payment_type AS pt, COUNT(*) AS n FROM trips GROUP BY 1 ORDER BY pt ASC",
    ] {
        assert_eq!(rows(&db, sql), by_pt, "{sql}");
    }
    let desc = rows(&db, "SELECT payment_type AS pt, COUNT(*) AS n FROM trips GROUP BY pt ORDER BY pt DESC");
    assert_eq!(desc, vec![vec![int(3), int(8)], vec![int(2), int(8)], vec![int(1), int(8)], vec![Value::Null, int(6)]]);
    // After another key, as the tie-break.
    for sql in ["SELECT payment_type AS pt, COUNT(*) AS n FROM trips GROUP BY pt ORDER BY n, pt", "SELECT payment_type AS pt, COUNT(*) AS n FROM trips GROUP BY payment_type ORDER BY n, pt"] {
        assert_eq!(rows(&db, sql), vec![vec![Value::Null, int(6)], vec![int(1), int(8)], vec![int(2), int(8)], vec![int(3), int(8)]], "{sql}");
    }
    let r = rows(&db, "SELECT payment_type AS pt, COUNT(*) AS n FROM trips GROUP BY pt ORDER BY n DESC, pt DESC");
    assert_eq!(r, vec![vec![int(3), int(8)], vec![int(2), int(8)], vec![int(1), int(8)], vec![Value::Null, int(6)]]);
    // A KEYWORD's alias, and DISTINCT.
    let zones = vec![vec![text("Bronx")], vec![text("Brooklyn")], vec![text("Queens")]];
    assert_eq!(rows(&db, "SELECT DISTINCT zone AS z FROM trips ORDER BY z"), zones);
    assert_eq!(rows(&db, "SELECT zone AS z FROM trips GROUP BY z ORDER BY z"), zones);
    assert_eq!(rows(&db, "SELECT DISTINCT payment_type AS pt FROM trips ORDER BY pt DESC"), vec![vec![int(3)], vec![int(2)], vec![int(1)], vec![Value::Null]]);
    // A TEXT column, which the row evaluator groups.
    let r = rows(&db, "SELECT country AS c, COUNT(*) AS n FROM invoice GROUP BY c ORDER BY n DESC, c LIMIT 2");
    assert_eq!(r, vec![vec![text("USA"), int(3)], vec![text("Canada"), int(2)]]);
    assert_eq!(rows(&db, "SELECT DISTINCT country AS c FROM invoice ORDER BY c"), vec![vec![text("Brazil")], vec![text("Canada")], vec![text("USA")]]);
    // An output name wins over an input column's in ORDER BY, as in
    // PostgreSQL: `zone AS fare` orders by the zone.
    let r = rows(&db, "SELECT zone AS fare, COUNT(*) AS n FROM trips GROUP BY zone ORDER BY fare DESC");
    assert_eq!(r.iter().map(|r| r[0].clone()).collect::<Vec<_>>(), vec![text("Queens"), text("Brooklyn"), text("Bronx")]);
}

#[test]
fn json_extract_reads_dollar_paths_in_select() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path());
    let r = rows(&db, "SELECT id, attrs.dims.w, json_extract(attrs, 'dims.w') AS a, json_extract(attrs, '$.dims.w') AS b FROM products ORDER BY id");
    assert_eq!(r, vec![vec![text("p1"), Value::Float(40.5), Value::Float(40.5), Value::Float(40.5)], vec![text("p2"), Value::Null, Value::Null, Value::Null]]);
    // A TEXT column's JSON, and a literal's.
    assert_eq!(rows(&db, "SELECT json_extract(doc, '$.a.b') FROM products ORDER BY id"), vec![vec![int(1)], vec![int(2)]]);
    assert_eq!(rows(&db, r#"SELECT json_extract('{"a": {"b": 3}}', '$.a.b') AS v, json_extract('{"a": {"b": 3}}', 'a.b') AS w"#), vec![vec![int(3), int(3)]]);
    // `'$'` is the whole value.
    assert_eq!(rows(&db, r#"SELECT json_extract('{"a": 1}', '$'), json_extract('7', '$')"#), vec![vec![text(r#"{"a":1}"#), int(7)]]);
    // In WHERE, as before, and in GROUP BY and ORDER BY.
    assert_eq!(rows(&db, "SELECT COUNT(*) FROM products WHERE json_extract(attrs, '$.voltage') = 5"), vec![vec![int(1)]]);
    assert_eq!(rows(&db, "SELECT id FROM products ORDER BY json_extract(doc, '$.a.b') DESC"), vec![vec![text("p2")], vec![text("p1")]]);
}

#[test]
fn nulls_first_orders_groups() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path());
    let want = vec![vec![Value::Null, int(6)], vec![int(3), int(8)], vec![int(2), int(8)], vec![int(1), int(8)]];
    for sql in [
        "SELECT payment_type, COUNT(*) AS n FROM trips GROUP BY payment_type ORDER BY payment_type DESC NULLS FIRST",
        "SELECT payment_type AS pt, COUNT(*) AS n FROM trips GROUP BY pt ORDER BY pt DESC NULLS FIRST",
        "SELECT payment_type, COUNT(*) AS n FROM trips GROUP BY 1 ORDER BY 1 DESC NULLS FIRST",
    ] {
        assert_eq!(rows(&db, sql), want, "{sql}");
    }
    let r = rows(&db, "SELECT payment_type, COUNT(*) AS n FROM trips GROUP BY payment_type ORDER BY payment_type NULLS FIRST LIMIT 2");
    assert_eq!(r, vec![vec![Value::Null, int(6)], vec![int(1), int(8)]]);
    let r = rows(&db, "SELECT DISTINCT payment_type FROM trips ORDER BY payment_type DESC NULLS FIRST");
    assert_eq!(r, vec![vec![Value::Null], vec![int(3)], vec![int(2)], vec![int(1)]]);
    // NULLS LAST, said or not, is the default.
    let r = rows(&db, "SELECT payment_type, COUNT(*) AS n FROM trips GROUP BY payment_type ORDER BY payment_type DESC NULLS LAST");
    assert_eq!(r.last(), Some(&vec![Value::Null, int(6)]));
}

#[test]
fn refusals_say_what_to_write_instead() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path());
    let e = refused(&db, "SELECT country, id, SUM(total) FROM invoice GROUP BY ROLLUP (country, id)");
    assert!(e.contains("ROLLUP is not supported") && e.contains("GROUP BY country, id, GROUP BY country, no GROUP BY (the total)") && e.contains("UNION ALL"), "{e}");
    assert!(!e.contains("computes for a row"), "{e}");
    let e = refused(&db, "SELECT payment_type, COUNT(*) FROM trips GROUP BY ROLLUP (payment_type)");
    assert!(e.contains("ROLLUP is not supported") && e.contains("UNION ALL"), "{e}");
    let e = refused(&db, "SELECT country, COUNT(*) FROM invoice GROUP BY GROUPING SETS ((country), ())");
    assert!(e.contains("GROUPING SETS is not supported") && e.contains("GROUP BY country, no GROUP BY (the total)"), "{e}");
    let e = refused(&db, "SELECT country, COUNT(*) FROM invoice GROUP BY CUBE (country)");
    assert!(e.contains("CUBE is not supported"), "{e}");
    // substr and substring are functions since SAM-7; left() names substr.
    let r = rows(&db, "SELECT substr('Brazil', 1, 2), substring('Brazil', 3), SUBSTRING('Brazil' FROM 2 FOR 2)");
    assert_eq!(r, vec![vec![text("Br"), text("azil"), text("ra")]]);
    let e = refused(&db, "SELECT left(country, 2) FROM invoice");
    assert!(e.contains("unknown function left()") && e.contains("substr(s, 1, n)"), "{e}");
    let e = refused(&db, "SELECT i.id FROM invoice i JOIN trips USING (id)");
    assert!(e.contains("USING (id) is not supported") && e.contains("JOIN trips ON i.id = trips.id"), "{e}");
    assert!(!e.contains("papers"), "{e}");
    let e = refused(&db, "SELECT i.id FROM invoice i JOIN trips t ON t.id = i.id JOIN products p USING (id)");
    assert!(e.contains("JOIN products AS p ON t.id = p.id") || e.contains("JOIN products p ON t.id = p.id"), "{e}");
    let e = refused(&db, "SELECT COUNT(*) FROM invoice i JOIN trips t ON i.id < t.id");
    assert!(e.contains("JOIN t needs an equality") && e.contains("i.id = t.id") && !e.contains("cites"), "{e}");
    let e = refused(&db, "SELECT i.id FROM invoice i NATURAL JOIN trips");
    assert!(e.contains("NATURAL JOIN is not supported") && e.contains("trips"), "{e}");
    let e = refused(&db, "SELECT CAST('2026-01-02' AS TIMESTAMP) - CAST('2026-01-01' AS TIMESTAMP) AS d");
    assert!(e.contains("INTERVAL") && e.contains("date_diff('second', a, b)") && e.contains("EXTRACT(epoch FROM"), "{e}");
    // INSERT ... SELECT and CREATE TABLE ... AS SELECT run since SAM-8
    // (tests/from_select.rs has the rest).
    assert_eq!(written(&db, "INSERT INTO invoice (id, country, total) SELECT id + 100, country, total FROM invoice"), 6);
    assert_eq!(written(&db, "INSERT INTO invoice (id, country, total) SELECT id + 200, country, total FROM invoice ORDER BY id LIMIT 2"), 2);
    assert_eq!(rows(&db, "SELECT id FROM invoice WHERE id > 200 ORDER BY id"), vec![vec![int(201)], vec![int(202)]]);
    assert_eq!(written(&db, "CREATE TABLE t6 AS SELECT id, country FROM invoice"), 14);
    assert_eq!(rows(&db, "SELECT COUNT(*) FROM t6 WHERE id > 100"), vec![vec![int(8)]]);
    let e = refused_write(&db, "CREATE TEMP TABLE tmp (id INT)");
    assert!(e.contains("temporary tables (CREATE TEMP TABLE) are not supported"), "{e}");
}
