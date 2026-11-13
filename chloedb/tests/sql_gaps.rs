//! SQL that reporting and on-call queries write and that ChloeDB once
//! refused or answered wrongly: `FILTER (WHERE ..)` on aggregates,
//! statistical aggregates, `COUNT` of a TEXT column, intervals and time
//! arithmetic, `DATE`, `RIGHT JOIN`, correlated `EXISTS`, and which
//! queries rank rows (`/v1/explain`'s hits).

use chloedb::db::{Db, SqlResult};
use chloedb::query::Params;
use chloedb::schema::Schema;
use chloedb::storage::{DbOptions, Layout};
use chloedb::timestamp;
use chloedb::value::Value;

fn db(dir: &std::path::Path) -> Db {
    let seed = Schema::new("seed", vec![chloedb::schema::Field::new("id", chloedb::schema::FieldType::Keyword)], Some("id")).unwrap();
    let db = Db::create(&dir.join("d.strata"), seed, DbOptions { sync: false, layout: Layout::SingleFile, segment_max_docs: 3, ..DbOptions::default() }).unwrap();
    for s in [
        "CREATE TABLE logs (id INT PRIMARY KEY, host KEYWORD, severity KEYWORD, msg TEXT, ts TIMESTAMP, price FLOAT, sold INT, d DATE)",
        "INSERT INTO logs VALUES (1,'a','error','link down','2026-11-13T00:00:00Z',10.0,3,'2026-11-01'),(2,'a','info','ok','2026-11-13T00:10:00Z',20.0,5,'2026-11-02')",
        "INSERT INTO logs VALUES (3,'b','error','bgp hold','2026-11-12T00:00:00Z',30.0,NULL,NULL),(4,'c','error',NULL,NULL,NULL,7,'2026-11-03')",
        "CREATE TABLE hosts (name KEYWORD PRIMARY KEY, site KEYWORD)",
        "INSERT INTO hosts VALUES ('a','ams'),('b','fra'),('z','nyc')",
    ] {
        db.sql(s, &Params::new()).unwrap_or_else(|e| panic!("{s}: {e}"));
    }
    // Rows in segments and in the memtable.
    db.storage().flush().unwrap();
    db.sql("INSERT INTO logs VALUES (5,'b','info','up again','2026-11-12T00:05:00Z',40.0,1,'2026-11-04')", &Params::new()).unwrap();
    db
}

fn rows(db: &Db, s: &str) -> Vec<Vec<Value>> {
    match db.sql(s, &Params::new()).unwrap_or_else(|e| panic!("{s}: {e}")) {
        SqlResult::Rows(r) => r.rows,
        SqlResult::Written(_) => Vec::new(),
    }
}

fn columns(db: &Db, s: &str) -> Vec<String> {
    match db.sql(s, &Params::new()).unwrap_or_else(|e| panic!("{s}: {e}")) {
        SqlResult::Rows(r) => r.columns,
        SqlResult::Written(_) => Vec::new(),
    }
}

fn fails(db: &Db, s: &str) -> String {
    match db.sql(s, &Params::new()) {
        Ok(_) => panic!("{s} should fail"),
        Err(e) => e.to_string(),
    }
}

fn i(n: i64) -> Value {
    Value::Int(n)
}
fn s(x: &str) -> Value {
    Value::Str(x.into())
}
fn f(x: &Value) -> f64 {
    x.as_f64().unwrap_or_else(|| panic!("{x:?} is not a number"))
}

#[test]
fn aggregate_filter_keeps_the_rows_it_names() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path());
    // Grouped: the errors of each host, beside all its rows.
    assert_eq!(
        rows(&db, "SELECT host, count(*) FILTER (WHERE severity = 'error') AS errs, count(*) AS total FROM logs GROUP BY host ORDER BY host"),
        vec![vec![s("a"), i(1), i(2)], vec![s("b"), i(1), i(2)], vec![s("c"), i(1), i(1)]]
    );
    // Over the whole table, several aggregates with their own conditions.
    assert_eq!(
        rows(&db, "SELECT count(*) FILTER (WHERE severity = 'error'), sum(sold) FILTER (WHERE host = 'a'), max(price) FILTER (WHERE host <> 'b'), count(DISTINCT host) FILTER (WHERE severity = 'info') FROM logs"),
        vec![vec![i(3), i(8), Value::Float(20.0), i(2)]]
    );
    // No row kept: COUNT is 0, the others NULL.
    assert_eq!(rows(&db, "SELECT count(*) FILTER (WHERE host = 'zz'), avg(price) FILTER (WHERE host = 'zz') FROM logs"), vec![vec![i(0), Value::Null]]);
    // In HAVING and ORDER BY too.
    assert_eq!(
        rows(&db, "SELECT host, count(*) FILTER (WHERE severity = 'info') AS n FROM logs GROUP BY host HAVING count(*) FILTER (WHERE severity = 'info') > 0 ORDER BY count(*) FILTER (WHERE severity = 'info') DESC, host"),
        vec![vec![s("a"), i(1)], vec![s("b"), i(1)]]
    );
    // An unnamed column keeps the name it is written with.
    assert_eq!(columns(&db, "SELECT count(*) FILTER (WHERE severity = 'error') FROM logs"), ["count(*) FILTER (WHERE severity = 'error')"]);
    // A percentile filters its values, not its fraction.
    assert_eq!(rows(&db, "SELECT median(price) FILTER (WHERE host = 'b') FROM logs"), vec![vec![Value::Float(35.0)]]);
    // A join's aggregates and a subquery's too.
    assert_eq!(
        rows(&db, "SELECT h.site, count(*) FILTER (WHERE l.severity = 'error') FROM logs l JOIN hosts h ON l.host = h.name GROUP BY h.site ORDER BY h.site"),
        vec![vec![s("ams"), i(1)], vec![s("fra"), i(1)]]
    );
    assert_eq!(rows(&db, "SELECT n FROM (SELECT count(*) FILTER (WHERE price > 15) AS n FROM logs) t"), vec![vec![i(3)]]);
    // Never ignored: FILTER on what is not an aggregate is refused.
    assert!(fails(&db, "SELECT lower(host) FILTER (WHERE severity = 'error') FROM logs").contains("FILTER applies to an aggregate"));
    assert!(fails(&db, "SELECT count(*) FILTER (WHERE severity = 'error') OVER (PARTITION BY host) FROM logs").contains("takes no FILTER"));
}

#[test]
fn statistical_aggregates() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path());
    // price: 10, 20, 30, 40 and one NULL.
    let r = &rows(&db, "SELECT stddev(price), stddev_samp(price), variance(price), var_samp(price), stddev_pop(price), var_pop(price) FROM logs")[0];
    let var: f64 = 500.0 / 3.0;
    for (v, want) in r.iter().zip([var.sqrt(), var.sqrt(), var, var, 125f64.sqrt(), 125.0]) {
        assert!((f(v) - want).abs() < 1e-9, "{r:?}");
    }
    // Pairs with both values: (10, 3), (20, 5), (40, 1).
    let (xs, ys) = ([3.0, 5.0, 1.0], [10.0, 20.0, 40.0]);
    let (mx, my) = (3.0, 70.0 / 3.0);
    let sxy: f64 = xs.iter().zip(&ys).map(|(x, y)| (x - mx) * (y - my)).sum();
    let sxx: f64 = xs.iter().map(|x| (x - mx) * (x - mx)).sum();
    let syy: f64 = ys.iter().map(|y| (y - my) * (y - my)).sum();
    let r = &rows(&db, "SELECT corr(price, sold), covar_pop(price, sold), covar_samp(price, sold) FROM logs")[0];
    for (v, want) in r.iter().zip([sxy / (sxx * syy).sqrt(), sxy / 3.0, sxy / 2.0]) {
        assert!((f(v) - want).abs() < 1e-9, "{r:?}");
    }
    // One value: the sample forms are NULL, the population ones 0.
    assert_eq!(rows(&db, "SELECT host, stddev(price), var_pop(price) FROM logs WHERE host = 'c' OR id = 1 GROUP BY host ORDER BY host"), vec![
        vec![s("a"), Value::Null, Value::Float(0.0)],
        vec![s("c"), Value::Null, Value::Null]
    ]);
    // Grouped, ordered by an alias, with FILTER, and named as written.
    let r = rows(&db, "SELECT host, stddev(price) AS sd FROM logs GROUP BY host ORDER BY sd DESC, host");
    assert_eq!(r[0][0], s("a"));
    assert!((f(&r[0][1]) - 50f64.sqrt()).abs() < 1e-9);
    assert_eq!(columns(&db, "SELECT corr(price, sold) FROM logs"), ["corr(price, sold)"]);
    assert_eq!(rows(&db, "SELECT var_pop(price) FILTER (WHERE host = 'b') FROM logs"), vec![vec![Value::Float(25.0)]]);
    assert!(fails(&db, "SELECT corr(price) FROM logs").contains("two values"));
}

#[test]
fn count_and_min_of_text() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path());
    assert_eq!(rows(&db, "SELECT count(msg), count(DISTINCT msg), min(msg) FROM logs"), vec![vec![i(4), i(4), s("bgp hold")]]);
    assert_eq!(rows(&db, "SELECT host, count(msg) FROM logs GROUP BY host ORDER BY host"), vec![vec![s("a"), i(2)], vec![s("b"), i(2)], vec![s("c"), i(0)]]);
}

#[test]
fn intervals_and_time_arithmetic() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path());
    let t = |x: &str| Value::Timestamp(timestamp::parse(x).unwrap());
    // A time written as a value, moved by an interval, is one value.
    assert_eq!(rows(&db, "SELECT TIMESTAMP '2026-11-13T00:05:00Z' - INTERVAL '10 minutes'"), vec![vec![t("2026-11-12T23:55:00Z")]]);
    assert_eq!(rows(&db, "SELECT TIMESTAMP '2026-01-31' + INTERVAL '1 month', INTERVAL '1 day 12 hours' + TIMESTAMP '2026-01-01'"), vec![vec![t("2026-02-28"), t("2026-01-02T12:00:00Z")]]);
    assert_eq!(rows(&db, "SELECT CAST('2026-03-01' AS TIMESTAMP) - INTERVAL '1' DAY"), vec![vec![t("2026-02-28")]]);
    // In WHERE, on the index's range, with a constant time or now().
    assert_eq!(rows(&db, "SELECT id FROM logs WHERE ts > TIMESTAMP '2026-11-13T00:05:00Z' - INTERVAL '10 minutes' ORDER BY id"), vec![vec![i(1)], vec![i(2)]]);
    assert_eq!(rows(&db, "SELECT id FROM logs WHERE ts >= date_trunc('day', TIMESTAMP '2026-11-12T05:00:00Z') + INTERVAL '5 minutes' ORDER BY id"), vec![vec![i(1)], vec![i(2)], vec![i(5)]]);
    assert_eq!(rows(&db, "SELECT count(*) FROM logs WHERE ts > now() - INTERVAL '100 years'"), vec![vec![i(4)]]);
    assert_eq!(rows(&db, "SELECT count(*) FROM logs WHERE ts > now() + INTERVAL '1 hour'"), vec![vec![i(0)]]);
    assert_eq!(rows(&db, "SELECT count(*) FROM logs WHERE ts BETWEEN now() - INTERVAL '100 years' AND now()"), vec![vec![i(4)]]);
    // In a join's condition on one side.
    assert_eq!(rows(&db, "SELECT count(*) FROM logs l JOIN hosts h ON l.host = h.name WHERE l.ts > now() - INTERVAL '100 years'"), vec![vec![i(4)]]);
    // A column moved by an interval, row by row.
    assert_eq!(rows(&db, "SELECT id, ts + INTERVAL '1 day' FROM logs WHERE id IN (1, 4) ORDER BY id"), vec![vec![i(1), t("2026-11-14")], vec![i(4), Value::Null]]);
    assert_eq!(rows(&db, "SELECT id FROM logs WHERE ts - INTERVAL '6 hours' < '2026-11-12T00:00:00Z' ORDER BY id"), vec![vec![i(3)], vec![i(5)]]);
    // A month has no fixed length, and a bare number no unit.
    assert!(fails(&db, "SELECT ts + INTERVAL '1 month' FROM logs").contains("no fixed length"));
    for q in ["SELECT now() - 3600", "SELECT id FROM logs WHERE ts > now() - 3600"] {
        assert!(fails(&db, q).contains("INTERVAL '3600 seconds'"), "{q}");
    }
    assert!(fails(&db, "SELECT INTERVAL '1 day'").contains("length of time"));
    assert!(fails(&db, "SELECT now() - INTERVAL '3 fortnights'").contains("not a unit"));
    // DATE is a TIMESTAMP: a date alone is its midnight UTC.
    assert_eq!(rows(&db, "SELECT id, d FROM logs WHERE d >= DATE '2026-11-03' ORDER BY id"), vec![vec![i(4), t("2026-11-03")], vec![i(5), t("2026-11-04")]]);
}

#[test]
fn right_join_and_correlated_exists() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path());
    assert_eq!(
        rows(&db, "SELECT l.id, h.site FROM logs l RIGHT JOIN hosts h ON l.host = h.name ORDER BY h.name, l.id"),
        vec![vec![i(1), s("ams")], vec![i(2), s("ams")], vec![i(3), s("fra")], vec![i(5), s("fra")], vec![Value::Null, s("nyc")]]
    );
    // `*` keeps the tables' order as written.
    let cols = columns(&db, "SELECT * FROM logs l RIGHT OUTER JOIN hosts h ON l.host = h.name");
    assert_eq!((cols.first().map(String::as_str), cols.last().map(String::as_str)), (Some("l.id"), Some("h.site")));
    assert_eq!(rows(&db, "SELECT h.name, count(l.id) FROM logs l RIGHT JOIN hosts h ON l.host = h.name GROUP BY h.name ORDER BY h.name"), vec![
        vec![s("a"), i(2)],
        vec![s("b"), i(2)],
        vec![s("z"), i(0)]
    ]);
    // EXISTS as a semi join, NOT EXISTS as an anti join.
    assert_eq!(rows(&db, "SELECT h.name FROM hosts h WHERE EXISTS (SELECT 1 FROM logs l WHERE l.host = h.name AND l.severity = 'info') ORDER BY h.name"), vec![vec![s("a")], vec![s("b")]]);
    assert_eq!(rows(&db, "SELECT h.name FROM hosts h WHERE NOT EXISTS (SELECT 1 FROM logs l WHERE h.name = l.host) ORDER BY h.name"), vec![vec![s("z")]]);
    assert_eq!(rows(&db, "SELECT name FROM hosts WHERE site <> 'ams' AND NOT EXISTS (SELECT * FROM logs WHERE logs.host = hosts.name AND logs.price > 35 LIMIT 1) ORDER BY name"), vec![vec![s("z")]]);
    // NOT EXISTS keeps an outer row whose key is NULL; NULLs inside match nothing.
    db.sql("INSERT INTO hosts VALUES ('ok', NULL)", &Params::new()).unwrap();
    db.sql("INSERT INTO logs (id, severity) VALUES (6, 'info')", &Params::new()).unwrap();
    assert_eq!(rows(&db, "SELECT l.id FROM logs l WHERE NOT EXISTS (SELECT 1 FROM hosts h WHERE h.name = l.host) ORDER BY l.id"), vec![vec![i(4)], vec![i(6)]]);
    assert_eq!(rows(&db, "SELECT h.name FROM hosts h WHERE NOT EXISTS (SELECT 1 FROM hosts g WHERE g.site = h.site AND g.name <> 'zz') ORDER BY h.name"), vec![vec![s("ok")]]);
    // What the rewrite does not cover stays refused, never answered wrongly.
    assert!(fails(&db, "SELECT l.id FROM logs l WHERE EXISTS (SELECT 1 FROM logs m WHERE m.host = l.host AND m.id <> l.id)").contains("names the outer row"));
    assert_eq!(rows(&db, "SELECT count(*) FROM logs l WHERE EXISTS (SELECT 1 FROM hosts h WHERE h.name = l.host)"), vec![vec![i(4)]]);
}

#[test]
fn which_queries_rank_rows() {
    use chloedb::query::sql::ranks;
    for q in [
        "SELECT id FROM t ORDER BY bm25(body, 'x') DESC LIMIT 5",
        "SELECT id, bm25(body, 'x') AS s FROM t ORDER BY s DESC",
        "SELECT id FROM t ORDER BY price DESC",
        "SELECT id, price AS p FROM t ORDER BY 2",
    ] {
        assert!(ranks(q), "{q}");
    }
    for q in [
        "SELECT brand, SUM(price*sold) AS revenue, COUNT(*) AS n FROM products GROUP BY brand ORDER BY revenue DESC LIMIT 10",
        "SELECT brand, COUNT(*) AS n FROM products GROUP BY brand ORDER BY n DESC",
        "SELECT year, AVG(price) FROM products GROUP BY year ORDER BY year",
        "SELECT id, price*sold AS r FROM products ORDER BY r DESC LIMIT 3",
        "SELECT id FROM products ORDER BY price * sold DESC",
        "SELECT COUNT(*) FROM products ORDER BY 1",
        "SELECT DISTINCT brand FROM products ORDER BY brand",
        "SELECT a.id FROM a JOIN b ON a.x = b.y ORDER BY a.id",
        "SELECT id FROM products",
    ] {
        assert!(!ranks(q), "{q}");
    }
}
