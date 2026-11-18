//! The SQL battery a data engineer ran (persona study round 2, the
//! queries of `chloedb-sql-bench`, `src/bin/chloedb-sql-bench/taxi.rs`) gives
//! one answer whichever way it runs: the default engine (DataFusion over
//! leaves that read their columns into Arrow, or the executor's own groups
//! from the columns), DataFusion for every plan it lowers, the operators
//! alone, and the plan that computes every expression row by row
//! ([`plan_read_by_rows`]). Checked across segments, after deletes and
//! values written in place (which the columns' copy into Arrow leaves to
//! the rows), and after a merge; and on a table of every kind of column
//! the copy reads (BOOL, TIMESTAMP, FLOAT, INT, KEYWORD, a KEYWORD key,
//! NULLs in each).

#![cfg(feature = "datafusion")]

use chloedb::db::{Db, SqlResult};
use chloedb::query::compound::plan_read_by_rows;
use chloedb::query::exec::{Engine, ExecOptions, ResultSet};
use chloedb::query::Params;
use chloedb::schema::Schema;
use chloedb::storage::{DbOptions, MergePolicy};
use chloedb::value::Value;

#[allow(dead_code)]
#[path = "../src/bin/chloedb-sql-bench/taxi.rs"]
mod taxi;

fn sql(db: &Db, s: &str) {
    db.sql(s, &Params::new()).unwrap_or_else(|e| panic!("{s}: {e}"));
}

fn key(row: &[Value]) -> String {
    row.iter()
        .map(|v| match v {
            Value::Float(f) => format!("{f:.6e}"),
            v => format!("{v:?}"),
        })
        .collect::<Vec<_>>()
        .join("|")
}

/// Panics unless `got` is `want`: in order under ORDER BY (outside a
/// window), as the same rows otherwise, floats within 1e-9 relative.
fn agree(what: &str, sql: &str, want: &ResultSet, got: &ResultSet) {
    assert_eq!(want.columns.len(), got.columns.len(), "{what}: {sql}: columns");
    assert_eq!(want.rows.len(), got.rows.len(), "{what}: {sql}: rows");
    let (mut a, mut b): (Vec<&Vec<Value>>, Vec<&Vec<Value>>) = (want.rows.iter().collect(), got.rows.iter().collect());
    if !sql.contains("ORDER BY") || sql.contains(" OVER (") {
        a.sort_by_key(|r| key(r));
        b.sort_by_key(|r| key(r));
    }
    for (x, y) in a.iter().zip(&b) {
        let same = x.len() == y.len()
            && x.iter().zip(y.iter()).all(|(p, q)| match (p, q) {
                (Value::Float(p), Value::Float(q)) => (p - q).abs() <= 1e-9 * p.abs().max(q.abs()).max(1.0),
                (p, q) => p == q,
            });
        assert!(same, "{what}: {sql}: {y:?} where the rows give {x:?}");
    }
}

/// Each query's answer on each engine, against the operators' of the plan
/// computed row by row. Returns how many answers DataFusion gave.
fn check(db: &Db, stage: &str, queries: &[(&str, &str)]) -> usize {
    let catalog = db.catalog();
    let tables: Vec<&Schema> = catalog.iter().map(|t| &**t).collect();
    let p = Params::new();
    let on = |engine| ExecOptions { engine, threads: 4, ..db.exec.clone() };
    let mut on_datafusion = 0;
    for (id, q) in queries {
        let by_rows = plan_read_by_rows(q, &tables, &p).unwrap_or_else(|e| panic!("{stage}: {id}: by rows: {e}"));
        let want = match db.run_with(&by_rows, &on(Engine::Operators)).unwrap_or_else(|e| panic!("{stage}: {id}: by rows: {e}")) {
            SqlResult::Rows(r) => r,
            SqlResult::Written(_) => panic!("{stage}: {id}: rows"),
        };
        for engine in [Engine::Auto, Engine::DataFusion, Engine::Operators] {
            let got = db.query_with(q, &p, &on(engine)).unwrap_or_else(|e| panic!("{stage}: {id} on {engine:?}: {e}"));
            on_datafusion += usize::from(got.stats.plan.iter().any(|l| l.starts_with("datafusion:")));
            agree(&format!("{stage}: {id} on {engine:?}"), q, &want, &got);
        }
    }
    on_datafusion
}

fn opts() -> DbOptions {
    DbOptions { sync: false, segment_max_docs: 2_500, ..DbOptions::default() }
}

#[test]
fn the_battery_answers_alike_on_every_engine() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::create(&dir.path().join("taxi.chloedb"), taxi::zones_schema(), opts()).unwrap();
    taxi::create(&db, 12_000, 7);
    db.storage().flush().unwrap();
    assert!(check(&db, "segments", taxi::QUERIES) > 0, "no query ran on DataFusion");
    sql(&db, "DELETE FROM trips WHERE id BETWEEN 3000 AND 3400 OR id IN (5, 77, 140, 9999)");
    sql(&db, "UPDATE trips SET fare_amount = 12.5, payment_type = NULL WHERE id BETWEEN 500 AND 560");
    sql(&db, "UPDATE trips SET lpep_pickup_datetime = '2024-02-29T23:59:00Z' WHERE id BETWEEN 7000 AND 7020");
    check(&db, "deletes and updates", taxi::QUERIES);
    db.storage().merge(&MergePolicy { merge_factor: 2, floor_docs: 100_000, ..MergePolicy::default() }).unwrap();
    check(&db, "merged", taxi::QUERIES);
}

/// Queries over a table of each kind of column, each read into Arrow
/// above a leaf (an expression over them keeps the groups out of the
/// executor) or grouped by the executor from the columns.
const KINDS: &[(&str, &str)] = &[
    ("groups of a BOOL", "SELECT b, COUNT(*), SUM(i * 2), ROUND(AVG(f + 1), 6), MAX(t), MIN(t2) FROM kinds GROUP BY b"),
    ("groups of a KEYWORD", "SELECT k, COUNT(*) AS n, SUM(i + 0), MAX(f * 1), MIN(t) FROM kinds GROUP BY k ORDER BY n DESC, k"),
    ("a KEYWORD key", "SELECT id, k, i, b FROM kinds WHERE i * 1 > 40 ORDER BY id LIMIT 30"),
    ("a self join by key", "SELECT a.k, COUNT(*) FROM kinds a JOIN kinds o ON a.id = o.id WHERE o.i > 10 GROUP BY a.k"),
    ("a window by key", "SELECT id, i, SUM(i) OVER (PARTITION BY k ORDER BY id) AS s FROM kinds ORDER BY id LIMIT 40"),
    ("HAVING a time against text", "SELECT k, MAX(t) AS hi FROM kinds GROUP BY k HAVING MAX(t) > '2024-03-01'"),
    ("times compared with text", "SELECT COUNT(*) FROM (SELECT t, i FROM kinds) s WHERE s.t >= '2024-02-01' AND s.t < '2024-03-01T12:00:00Z'"),
    ("a BETWEEN and an IN of times as text", "SELECT k, COUNT(*) FROM (SELECT k, t FROM kinds) s WHERE s.t BETWEEN '2024-01-10' AND '2024-02-10' OR s.t IN ('2024-01-01T00:00:00Z') GROUP BY k"),
    ("parts of times above a leaf", "SELECT EXTRACT(dow FROM t) AS d, date_part('hour', t2) AS h, COUNT(*), SUM(EXTRACT(epoch FROM t2) - EXTRACT(epoch FROM t)) FROM kinds GROUP BY d, h"),
    ("times cut and binned above a leaf", "SELECT date_trunc('week', t) AS w, date_bin('6 hours', t2, '2024-01-01T01:00:00Z') AS b, COUNT(*) FROM kinds WHERE i * 1 >= 0 GROUP BY 1, 2"),
    ("times formatted above a leaf", "SELECT strftime('%Y-%m-%d %H', t) AS d, COUNT(*) FROM kinds WHERE f * 1 > 0 GROUP BY d"),
    ("aggregates under ROUND and HAVING", "SELECT k, b, COUNT(*) AS n, ROUND(SUM(f), 3) AS s, MIN(i), MAX(t) FROM kinds GROUP BY k, b HAVING COUNT(*) > 10 ORDER BY k, b NULLS LAST"),
    ("a window over groups", "SELECT i, COUNT(*) AS n, RANK() OVER (ORDER BY COUNT(*) DESC) AS r FROM kinds GROUP BY i ORDER BY n DESC, i LIMIT 15"),
];

#[test]
fn every_kind_of_column_reads_into_arrow_as_rows_give_it() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::create(&dir.path().join("kinds.chloedb"), taxi::zones_schema(), opts()).unwrap();
    sql(&db, "CREATE TABLE kinds (id KEYWORD PRIMARY KEY, k KEYWORD, b BOOL, i INT, f FLOAT, t TIMESTAMP, t2 TIMESTAMP)");
    let mut rng = taxi::Rng::new(3);
    let jan = 1_704_067_200_000_000i64;
    let rows: Vec<Vec<Value>> = (0..9_000)
        .map(|i| {
            let t = jan + rng.below(90 * 86_400) as i64 * 1_000_000;
            let row = [
                Value::Str(["auction", "fixed", "offer", "lot"][rng.below(4) as usize].into()),
                Value::Bool(rng.below(2) == 0),
                Value::Int(rng.below(100) as i64 - 20),
                Value::Float((rng.unit() * 500.0 * 100.0).round() / 100.0),
                Value::Timestamp(t),
                Value::Timestamp(t + rng.below(3 * 3_600) as i64 * 1_000_000),
            ];
            // About one value in twelve is missing.
            let row = row.into_iter().map(|v| if rng.below(12) == 0 { Value::Null } else { v });
            std::iter::once(Value::Str(format!("r{:05}", (i * 7919) % 9_000))).chain(row).collect()
        })
        .collect();
    db.load_into("kinds", rows).unwrap();
    db.storage().flush().unwrap();
    assert!(check(&db, "segments", KINDS) > 0, "no query ran on DataFusion");
    sql(&db, "DELETE FROM kinds WHERE i = 3 OR id IN ('r00005', 'r00077')");
    sql(&db, "UPDATE kinds SET f = 1.5, t = '2024-03-02T00:00:00Z' WHERE i = 7");
    check(&db, "deletes and updates", KINDS);
    db.storage().merge(&MergePolicy { merge_factor: 2, floor_docs: 100_000, ..MergePolicy::default() }).unwrap();
    check(&db, "merged", KINDS);
}

/// Aggregates and groups of expressions, which the executor computes from
/// the columns a block at a time: integers that overflow into FLOATs,
/// division by zero, three-valued conditions, texts of a CASE as groups,
/// times against text, functions of numbers, and expressions it leaves to
/// the row evaluator (`round`, `hash`, `date_trunc`, a CASE whose branches
/// differ in type).
const EXPRS: &[(&str, &str)] = &[
    ("products that overflow", "SELECT k, SUM(i * 1000000000000), SUM(i * i), MIN(i * 3), MAX(-i), COUNT(i + 1), AVG(i - s) FROM ex GROUP BY k"),
    ("FILTER and CASE", "SELECT k, COUNT(*) FILTER (WHERE b), COUNT(*) FILTER (WHERE f > 2.5 AND NOT b), SUM(CASE WHEN f IS NULL THEN 1 ELSE 0 END), AVG(CASE WHEN b THEN f END), MAX(CASE WHEN s > 1 THEN t END) FROM ex GROUP BY k"),
    ("texts of a CASE as groups", "SELECT CASE WHEN f < 1 THEN 'low' WHEN f < 3 THEN 'mid' WHEN f IS NULL THEN NULL ELSE 'high' END AS band, COUNT(*), SUM(i) FROM ex GROUP BY band"),
    ("a CASE of a value as groups", "SELECT CASE coalesce(s, 0) WHEN 1 THEN 'one' WHEN 2 THEN 'two' WHEN 3 THEN 'one' ELSE 'other' END AS c, COUNT(*), SUM(f) FROM ex GROUP BY c ORDER BY c"),
    ("numbers and truths as groups", "SELECT i % 7 AS m, f > 2 AS big, COUNT(*), SUM(f * 2) FROM ex GROUP BY m, big"),
    ("a text and a number as groups", "SELECT CASE WHEN b THEN 'y' ELSE 'n' END AS yb, s * 2 AS d, k, COUNT(*), MIN(f) FROM ex GROUP BY yb, d, k"),
    ("FLOATs as groups", "SELECT f * 10 AS x, COUNT(*) FROM ex GROUP BY x"),
    ("division and remainder by zero", "SELECT SUM(i / s), SUM(i % s), AVG(f / (s - 1)), MAX(f % s), COUNT(s / 0) FROM ex"),
    ("times against text", "SELECT COUNT(*) FILTER (WHERE t < '2024-02-01'), COUNT(*) FILTER (WHERE t BETWEEN '2024-01-10' AND '2024-02-10'), SUM(EXTRACT(epoch FROM t)), MIN(EXTRACT(second FROM t)), MAX(EXTRACT(hour FROM t) * 60 + EXTRACT(minute FROM t)) FROM ex"),
    ("IN and BETWEEN", "SELECT k, COUNT(*) FILTER (WHERE s IN (1, 2)), COUNT(*) FILTER (WHERE i BETWEEN 0 AND 50), COUNT(*) FILTER (WHERE s NOT IN (0, NULL)), COUNT(*) FILTER (WHERE f NOT BETWEEN 1 AND 2) FROM ex GROUP BY k"),
    ("functions of numbers", "SELECT k, SUM(abs(i)), ROUND(SUM(sqrt(f)), 6), ROUND(SUM(ln(f)), 6), SUM(greatest(i, s)), SUM(least(f, 2)), SUM(pow(s, 2)), MAX(coalesce(f, s, 0.5)) FROM ex GROUP BY k"),
    ("STDDEV, VAR_POP and CORR", "SELECT k, ROUND(STDDEV(f), 6), ROUND(VAR_POP(s), 6), ROUND(CORR(f, s), 6) FROM ex GROUP BY k"),
    ("expressions the rows compute", "SELECT k, SUM(round(f, 1)), MAX(hash(s, 0)), COUNT(*) FILTER (WHERE date_trunc('month', t) = '2024-02-01'), SUM(CASE WHEN b THEN s ELSE f END), MAX(CASE WHEN b THEN 1 ELSE 0.5 END) FROM ex GROUP BY k"),
    ("NOT, OR and UNKNOWN", "SELECT COUNT(*) FILTER (WHERE NOT (b OR f > 2)), COUNT(*) FILTER (WHERE b IS NULL OR i IS NULL), COUNT(*) FILTER (WHERE (s > 1) = b), COUNT(*) FILTER (WHERE b <> (f < 1)) FROM ex"),
    ("NULL in arithmetic and signs", "SELECT SUM(s + NULL), COUNT(f * NULL), SUM(-f), SUM(-s), MIN(-(f - 1)), MAX(abs(f - 2)) FROM ex"),
    ("a WHERE beside them", "SELECT k, SUM(s * 3), COUNT(*) FILTER (WHERE b) FROM ex WHERE f > 1 GROUP BY k ORDER BY k"),
];

#[test]
fn expressions_in_aggregates_answer_as_rows_do() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::create(&dir.path().join("ex.chloedb"), taxi::zones_schema(), opts()).unwrap();
    sql(&db, "CREATE TABLE ex (id KEYWORD PRIMARY KEY, k KEYWORD, b BOOL, i INT, s INT, f FLOAT, t TIMESTAMP)");
    let mut rng = taxi::Rng::new(11);
    let jan = 1_704_067_200_000_000i64;
    let rows: Vec<Vec<Value>> = (0..8_000)
        .map(|n| {
            // Some integers near the largest, whose products overflow.
            let i = match rng.below(10) {
                0 => i64::MAX - rng.below(1000) as i64,
                1 => i64::MIN + rng.below(1000) as i64,
                2 => 4_000_000_000 + rng.below(1000) as i64,
                _ => rng.below(100) as i64 - 20,
            };
            let row = [
                Value::Str(["auction", "fixed", "offer", "lot"][rng.below(4) as usize].into()),
                Value::Bool(rng.below(2) == 0),
                Value::Int(i),
                Value::Int(rng.below(4) as i64),
                Value::Float((rng.unit() * 5.0 * 100.0).round() / 100.0),
                Value::Timestamp(jan + rng.below(60 * 86_400) as i64 * 1_000_000 + rng.below(1_000_000) as i64),
            ];
            let row = row.into_iter().map(|v| if rng.below(10) == 0 { Value::Null } else { v });
            std::iter::once(Value::Str(format!("e{n:05}"))).chain(row).collect()
        })
        .collect();
    db.load_into("ex", rows).unwrap();
    db.storage().flush().unwrap();
    // Every one of them is computed by the executor from the columns, a
    // block at a time but for the expressions the row evaluator computes
    // (and `coalesce(f, s, 0.5)`, whose type varies by row).
    for (id, q) in EXPRS {
        let rs = db.query(q, &Params::new()).unwrap_or_else(|e| panic!("{id}: {e}"));
        let line = rs.stats.plan.iter().find(|l| l.contains("expressions computed a block at a time")).unwrap_or_else(|| panic!("{id}: {:?}", rs.stats.plan));
        let rows = matches!(*id, "expressions the rows compute" | "functions of numbers");
        assert_eq!(!line.ends_with(" and 0 row by row"), rows, "{id}: {line}");
    }
    check(&db, "segments", EXPRS);
    sql(&db, "DELETE FROM ex WHERE s = 3 AND f < 1");
    sql(&db, "UPDATE ex SET i = 9223372036854775807, f = NULL WHERE s = 2 AND b");
    sql(&db, "UPDATE ex SET b = NULL, s = 0, t = '2024-02-01T00:00:00Z' WHERE i BETWEEN 5 AND 9");
    check(&db, "deletes and updates", EXPRS);
    db.storage().merge(&MergePolicy { merge_factor: 2, floor_docs: 100_000, ..MergePolicy::default() }).unwrap();
    check(&db, "merged", EXPRS);
}
