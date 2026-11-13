//! Approximate percentiles: `approx_percentile(x, p)`, a DDSketch of each
//! group's values ([`chloedb::query::agg::sketch`]). Each answer is within
//! 1% of the value at rank ⌊p·(n − 1)⌋ of the group's values, as counted
//! here from the rows; the executor's `GROUP BY` over the columns gives the
//! answer the operators give row by row (sketches merge exactly); and a
//! group holds a few KB of sketch however many rows it has. Checked in the
//! memtable, across flushed segments, after deletes and updates, and after
//! a merge.

use std::collections::BTreeMap;

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use chloedb::db::{Db, SqlResult};
use chloedb::query::exec::ExecOptions;
use chloedb::query::ir::{self, Plan};
use chloedb::query::Params;
use chloedb::schema::Schema;
use chloedb::storage::{DbOptions, Layout, MergePolicy};
use chloedb::timestamp;
use chloedb::value::Value;

fn opts() -> DbOptions {
    DbOptions { sync: false, layout: Layout::SingleFile, segment_max_docs: 400, ..DbOptions::default() }
}

fn sql(db: &Db, s: &str) -> Vec<Vec<Value>> {
    match db.sql(s, &Params::new()).unwrap_or_else(|e| panic!("{s}: {e}")) {
        SqlResult::Rows(r) => r.rows,
        SqlResult::Written(_) => Vec::new(),
    }
}

fn open(dir: &std::path::Path) -> Db {
    let seed = Schema::new("seed", vec![chloedb::schema::Field::new("id", chloedb::schema::FieldType::Keyword)], Some("id")).unwrap();
    let db = Db::create(&dir.join("d.strata"), seed, opts()).unwrap();
    sql(&db, "CREATE TABLE logs (id INT PRIMARY KEY, service KEYWORD, latency INT, delta FLOAT, ts TIMESTAMP)");
    db
}

/// A row as the test keeps it: service, latency, delta, time.
type Row = (String, Option<i64>, Option<f64>, i64);

/// 2026-08-01.
const START: i64 = 1_785_542_400_000_000;

/// Inserts rows `from..to`: latencies in microseconds with a long tail,
/// deltas of both signs, some NULL; flushing every `flush` rows.
fn fill(db: &Db, rows: &mut BTreeMap<i64, Row>, rng: &mut StdRng, from: i64, to: i64, flush: i64) {
    let mut values = Vec::new();
    for i in from..to {
        let service = format!("svc{}", rng.gen_range(0..5));
        let latency = (rng.gen_range(0..30) != 0).then(|| ((rng.gen::<f64>() * 9.0).exp() * 40.0) as i64 + rng.gen_range(0..3));
        let delta = (rng.gen_range(0..30) != 0).then(|| format!("{:.3}", rng.gen_range(-5000.0..5000.0) * rng.gen::<f64>().powi(3)).parse::<f64>().unwrap());
        let ts = START + i * 7_000_000;
        let l = latency.map_or("NULL".into(), |l| l.to_string());
        let d = delta.map_or("NULL".into(), |d| format!("{d:.3}"));
        values.push(format!("({i}, '{service}', {l}, {d}, '{}')", timestamp::format(ts)));
        rows.insert(i, (service, latency, delta, ts));
        if flush > 0 && (i + 1) % flush == 0 {
            sql(db, &format!("INSERT INTO logs VALUES {}", values.join(", ")));
            values.clear();
            db.storage().flush().unwrap();
        }
    }
    if !values.is_empty() {
        sql(db, &format!("INSERT INTO logs VALUES {}", values.join(", ")));
    }
}

/// The value at rank ⌊p·(n − 1)⌋ of `xs`; None without values.
fn at_rank(xs: &[f64], p: f64) -> Option<f64> {
    let mut xs = xs.to_vec();
    xs.sort_by(f64::total_cmp);
    (!xs.is_empty()).then(|| xs[(p * (xs.len() - 1) as f64).floor() as usize])
}

/// Whether `got` is within 1% of `want`.
fn within(got: &Value, want: Option<f64>) -> bool {
    match (got, want) {
        (Value::Null, None) => true,
        (Value::Float(g), Some(w)) => (g - w).abs() <= 0.01 * w.abs() + 1e-9,
        _ => false,
    }
}

const FRACTIONS: [f64; 7] = [0.0, 0.05, 0.5, 0.9, 0.95, 0.99, 1.0];

fn grouped_query() -> String {
    let mut cols = vec!["service".to_string(), "COUNT(*)".to_string()];
    for p in FRACTIONS {
        cols.push(format!("approx_percentile(latency, {p})"));
        cols.push(format!("APPROX_PERCENTILE(delta, {p})"));
    }
    format!("SELECT {} FROM logs GROUP BY service ORDER BY service", cols.join(", "))
}

fn sorted(mut rows: Vec<Vec<Value>>) -> Vec<String> {
    let mut out: Vec<String> = rows.drain(..).map(|r| format!("{r:?}")).collect();
    out.sort();
    out
}

/// The grouped percentiles against the rows' exact ones, and every query's
/// answer against the operators' over a subquery's rows.
fn check(db: &Db, rows: &BTreeMap<i64, Row>, stage: &str) {
    let q = grouped_query();
    let got = db.query(&q, &Params::new()).unwrap_or_else(|e| panic!("{stage}: {q}: {e}"));
    assert!(got.stats.plan.iter().any(|l| l.contains("rows into groups") && l.contains("from the columns")), "{stage}: {:?}", got.stats.plan);
    let mut by: BTreeMap<&str, (Vec<f64>, Vec<f64>)> = BTreeMap::new();
    for (service, latency, delta, _) in rows.values() {
        let e = by.entry(service).or_default();
        e.0.extend(latency.map(|l| l as f64));
        e.1.extend(delta);
    }
    assert_eq!(got.rows.len(), by.len(), "{stage}");
    for (row, (service, (lat, del))) in got.rows.iter().zip(&by) {
        assert_eq!(row[0], Value::Str(service.to_string()), "{stage}");
        for (k, p) in FRACTIONS.iter().enumerate() {
            assert!(within(&row[2 + 2 * k], at_rank(lat, *p)), "{stage}: {service} latency p{p}: {:?} for {:?}", row[2 + 2 * k], at_rank(lat, *p));
            assert!(within(&row[3 + 2 * k], at_rank(del, *p)), "{stage}: {service} delta p{p}: {:?} for {:?}", row[3 + 2 * k], at_rank(del, *p));
        }
    }
    for q in [
        q.as_str(),
        "SELECT approx_percentile(latency, 0.99), approx_percentile(delta, 0.5), COUNT(*) FROM logs",
        "SELECT date_bin('1 hour', ts) AS h, approx_percentile(latency, 0.95) AS p95, MAX(latency) FROM logs GROUP BY h ORDER BY h",
        "SELECT service, approx_percentile(latency, 0.9) AS p FROM logs WHERE delta > 0 GROUP BY service HAVING approx_percentile(latency, 0.9) > 100 ORDER BY p DESC",
        "SELECT service, date_bin('15 minutes', ts) AS b, approx_percentile(latency, 0.99) FROM logs GROUP BY service, b",
    ] {
        let got = db.query(q, &Params::new()).unwrap_or_else(|e| panic!("{stage}: {q}: {e}"));
        let plan = db.explain(q, &Params::new()).unwrap();
        assert!(plan.features.iter().any(|f| f == ir::APPROX_PERCENTILES), "{stage}: {q}: {:?}", plan.features);
        assert!(!plan.features.iter().any(|f| f == ir::PERCENTILES), "{stage}: {q}: {:?}", plan.features);
        let by_rows = q.replace("FROM logs", "FROM (SELECT id, service, latency, delta, ts FROM logs) AS s");
        let want = db.query_with(&by_rows, &Params::new(), &db.exec.clone()).unwrap_or_else(|e| panic!("{stage}: {by_rows}: {e}"));
        assert!(!want.stats.plan.iter().any(|l| l.contains("rows into groups")), "{stage}: {q}: by rows: {:?}", want.stats.plan);
        assert_eq!(got.columns, want.columns, "{stage}: {q}");
        assert!(!got.rows.is_empty(), "{stage}: {q}");
        if q.rsplit_once("FROM logs").is_some_and(|(_, tail)| tail.contains("ORDER BY")) {
            assert_eq!(got.rows, want.rows, "{stage}: {q}");
        } else {
            assert_eq!(sorted(got.rows.clone()), sorted(want.rows), "{stage}: {q}");
        }
    }
}

#[test]
fn approximate_percentiles_are_within_one_percent() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path());
    let mut rng = StdRng::seed_from_u64(5);
    let mut rows = BTreeMap::new();
    fill(&db, &mut rows, &mut rng, 0, 300, 0);
    check(&db, &rows, "memtable");
    fill(&db, &mut rows, &mut rng, 300, 6000, 400);
    check(&db, &rows, "segments");
    sql(&db, "DELETE FROM logs WHERE id BETWEEN 1000 AND 1400 OR id IN (5, 77, 140)");
    rows.retain(|&i, _| !((1000..=1400).contains(&i) || [5, 77, 140].contains(&i)));
    sql(&db, "UPDATE logs SET latency = 7 WHERE id BETWEEN 2000 AND 2300");
    for (_, r) in rows.range_mut(2000..=2300) {
        r.1 = Some(7);
    }
    check(&db, &rows, "deletes and updates");
    db.storage().merge(&MergePolicy { merge_factor: 2, floor_docs: 100_000, ..MergePolicy::default() }).unwrap();
    check(&db, &rows, "merged");
}

#[test]
fn approximate_percentiles_in_sql() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path());
    sql(&db, "INSERT INTO logs VALUES (1, 'a', 10, 1.5, '2026-01-01'), (2, 'a', 1000, -2, '2026-01-02'), (3, 'a', 20, 0, '2026-01-03'), (4, 'b', NULL, NULL, '2026-01-04')");
    // Of three values, the median is the middle one within 1%; an INT's is a FLOAT.
    let rows = sql(&db, "SELECT service, approx_percentile(latency, 0.5), approx_percentile(latency, 1), approx_percentile(delta, 0) FROM logs GROUP BY service ORDER BY service");
    assert_eq!(rows[1], vec![Value::Str("b".into()), Value::Null, Value::Null, Value::Null]);
    let Value::Float(m) = rows[0][1] else { panic!("{rows:?}") };
    assert!((m - 20.0).abs() <= 0.2, "{m}");
    assert_eq!(rows[0][2..], [Value::Float(1000.0), Value::Float(-2.0)]);
    // Over a join or a WITH, of an expression, with the fraction a parameter.
    let with = "WITH t AS (SELECT latency * 2 AS l FROM logs) SELECT approx_percentile(l, :p) FROM t";
    let rows = match db.sql(with, &Params::new().set("p", Value::Float(1.0))).unwrap() {
        SqlResult::Rows(r) => r.rows,
        _ => panic!("rows"),
    };
    assert_eq!(rows, vec![vec![Value::Float(2000.0)]]);
    // The plan names its feature, which an older ChloeDB refuses; its
    // fraction travels in its JSON; EXPLAIN says what it computes.
    let plan = db.explain("SELECT service, approx_percentile(latency, 0.99) FROM logs GROUP BY service", &Params::new()).unwrap();
    assert!(plan.features.iter().any(|f| f == ir::APPROX_PERCENTILES), "{:?}", plan.features);
    assert_eq!(Plan::from_json(&plan.to_json().unwrap()).unwrap(), plan);
    let features: Vec<&str> = plan.features.iter().map(String::as_str).collect();
    let older: Vec<&str> = ir::FEATURES.iter().copied().filter(|f| *f != ir::APPROX_PERCENTILES).collect();
    let e = Plan::readable_by(plan.version as u64, &features, &older).err().expect("refused").to_string();
    assert!(e.contains("\"approx_percentiles\""), "{e}");
    let inner = db.explain("SELECT id FROM logs WHERE service IN (SELECT service FROM logs GROUP BY service HAVING approx_percentile(latency, 0.5) > 10)", &Params::new()).unwrap();
    assert!(inner.features.iter().any(|f| f == ir::APPROX_PERCENTILES), "{:?}", inner.features);
    let text = format!("{:?}", sql(&db, "EXPLAIN SELECT service, approx_percentile(latency, 0.95) FROM logs GROUP BY service"));
    assert!(text.contains("APPROX_PERCENTILE(latency, 0.95)"), "{text}");

    // What is wrong, said.
    for (q, says) in [
        ("SELECT approx_percentile(latency, 1.5) FROM logs", "from 0 to 1"),
        ("SELECT approx_percentile(latency) FROM logs", "a field and a fraction"),
        ("SELECT approx_percentile(latency, delta) FROM logs", "fraction"),
        ("SELECT approx_percentile(service, 0.5) FROM logs", "KEYWORD field"),
        ("SELECT approx_percentile(ts, 0.5) FROM logs", "TIMESTAMP"),
        ("SELECT approx_percentile(DISTINCT latency, 0.5) FROM logs", "DISTINCT works in COUNT only"),
        ("SELECT id, approx_percentile(latency, 0.5) OVER (PARTITION BY service) FROM logs", "not a window function"),
    ] {
        let e = db.query(q, &Params::new()).err().unwrap_or_else(|| panic!("{q} is an error")).to_string();
        assert!(e.contains(says), "{q}: {e}");
    }
}

#[test]
fn a_sketch_holds_a_few_kilobytes_however_many_rows() {
    let dir = tempfile::tempdir().unwrap();
    // Segments of 20,000 rows: each segment's groups keep sketches of their own until they merge.
    let seed = Schema::new("seed", vec![chloedb::schema::Field::new("id", chloedb::schema::FieldType::Keyword)], Some("id")).unwrap();
    let db = Db::create(&dir.path().join("d.strata"), seed, DbOptions { segment_max_docs: 20_000, ..opts() }).unwrap();
    sql(&db, "CREATE TABLE logs (id INT PRIMARY KEY, service KEYWORD, latency INT, delta FLOAT, ts TIMESTAMP)");
    let values: Vec<String> = (0..200_000).map(|i| format!("({i}, 'svc{}', {}, NULL, NULL)", i % 2, 100 + (i * 7919) % 900_000)).collect();
    for chunk in values.chunks(20_000) {
        sql(&db, &format!("INSERT INTO logs VALUES {}", chunk.join(", ")));
        db.storage().flush().unwrap();
    }
    // 100,000 values a group: the exact percentile needs 800 KB each, which
    // a 1 MiB budget refuses; the sketches fit.
    let small = || chloedb::query::memory::MemoryBudget::new(1 << 20);
    // (Over a WITH's rows, the rows themselves are what a budget holds.)
    for q in ["SELECT service, approx_percentile(latency, 0.99) FROM logs GROUP BY service", "SELECT service, date_bin('1 hour', ts) AS h, approx_percentile(latency, 0.5) FROM logs GROUP BY service, h"] {
        let budget = small();
        let rs = db.query_with(q, &Params::new(), &ExecOptions { memory: Some(budget.clone()), ..db.exec.clone() }).unwrap_or_else(|e| panic!("{q}: {e}"));
        assert_eq!(rs.rows.len(), 2, "{q}");
        assert_eq!(budget.used(), 0, "given back when the query ends");
    }
    let exact = "SELECT service, PERCENTILE_DISC(0.99) WITHIN GROUP (ORDER BY latency) FROM logs GROUP BY service";
    let budget = small();
    assert!(matches!(db.query_with(exact, &Params::new(), &ExecOptions { memory: Some(budget), ..db.exec.clone() }), Err(chloedb::Error::Memory(_))));
}
