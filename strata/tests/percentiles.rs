//! Percentiles: `MEDIAN(x)`, `PERCENTILE_CONT(p) WITHIN GROUP (ORDER BY x)`
//! and `PERCENTILE_DISC(..)`, with PostgreSQL's answers (of n values in
//! order, `PERCENTILE_DISC(p)` is the one at position ⌈p·n⌉ from 1, and
//! `PERCENTILE_CONT(p)` the one at p·(n − 1) from 0, interpolated). A
//! `GROUP BY` of a table is the executor's, read from the columns; its
//! answer is the one the operators give row by row, and the one computed
//! here from the rows themselves. Checked in the memtable, across flushed
//! segments, after deletes and updates, and after a merge.

use std::collections::{BTreeMap, HashMap};

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use strata::db::{Db, SqlResult};
use strata::query::exec::{Engine, ExecOptions};
use strata::query::ir::{self, Plan};
use strata::query::Params;
use strata::schema::Schema;
use strata::storage::{DbOptions, Layout, MergePolicy};
use strata::timestamp;
use strata::value::Value;

fn opts() -> DbOptions {
    DbOptions { sync: false, layout: Layout::SingleFile, segment_max_docs: 150, ..DbOptions::default() }
}

fn sql(db: &Db, s: &str) -> Vec<Vec<Value>> {
    match db.sql(s, &Params::new()).unwrap_or_else(|e| panic!("{s}: {e}")) {
        SqlResult::Rows(r) => r.rows,
        SqlResult::Written(_) => Vec::new(),
    }
}

fn open(dir: &std::path::Path) -> Db {
    let seed = Schema::new("seed", vec![strata::schema::Field::new("id", strata::schema::FieldType::Keyword)], Some("id")).unwrap();
    let db = Db::create(&dir.join("d.strata"), seed, opts()).unwrap();
    sql(&db, "CREATE TABLE snaps (id INT PRIMARY KEY, item KEYWORD, price FLOAT, qty INT, at TIMESTAMP)");
    db
}

/// A row as the test keeps it: item, price, qty.
type Row = (String, Option<f64>, Option<i64>);

/// Inserts rows `from..to` (a few prices and quantities NULL, many equal),
/// flushing every `flush` rows (0: never).
fn fill(db: &Db, rows: &mut BTreeMap<i64, Row>, rng: &mut StdRng, from: i64, to: i64, flush: i64) {
    for i in from..to {
        let item = format!("item{}", rng.gen_range(0..7));
        let price = (rng.gen_range(0..25) != 0).then(|| format!("{:.2}", rng.gen_range(-50.0..900.0)).parse::<f64>().unwrap());
        let qty = (rng.gen_range(0..25) != 0).then(|| rng.gen_range(-3..40i64));
        let p = price.map_or("NULL".into(), |p| format!("{p:.2}"));
        let q = qty.map_or("NULL".into(), |q| q.to_string());
        let at = timestamp::format(1_785_542_400_000_000 + i * 3_600_000_000);
        sql(db, &format!("INSERT INTO snaps VALUES ({i}, '{item}', {p}, {q}, '{at}')"));
        rows.insert(i, (item, price, qty));
        if flush > 0 && (i + 1) % flush == 0 {
            db.storage().flush().unwrap();
        }
    }
}

/// PostgreSQL's percentile of `xs`: NULL without values.
fn pg(xs: &[f64], p: f64, cont: bool) -> Option<f64> {
    let mut xs = xs.to_vec();
    xs.sort_by(f64::total_cmp);
    let n = xs.len();
    if n == 0 {
        return None;
    }
    if cont {
        let at = p * (n - 1) as f64;
        let (lo, hi) = (at.floor(), at.ceil());
        let (a, b) = (xs[lo as usize], xs[hi as usize]);
        return Some(if lo == hi { a } else { a + (at - lo) * (b - a) });
    }
    Some(xs[((p * n as f64).ceil() as usize).max(1) - 1])
}

const FRACTIONS: [f64; 6] = [0.0, 0.1, 0.25, 0.5, 0.9, 1.0];

/// Each item's percentiles of price and qty, from the rows.
fn expected(rows: &BTreeMap<i64, Row>) -> Vec<Vec<Value>> {
    let mut by: BTreeMap<&str, (Vec<f64>, Vec<f64>)> = BTreeMap::new();
    for (item, price, qty) in rows.values() {
        let e = by.entry(item).or_default();
        e.0.extend(price);
        e.1.extend(qty.map(|q| q as f64));
    }
    let float = |x: Option<f64>| x.map_or(Value::Null, Value::Float);
    let int = |x: Option<f64>| x.map_or(Value::Null, |x| Value::Int(x as i64));
    by.into_iter()
        .map(|(item, (prices, qtys))| {
            let mut row = vec![Value::Str(item.to_string()), float(pg(&prices, 0.5, true))];
            for p in FRACTIONS {
                row.extend([float(pg(&prices, p, true)), float(pg(&prices, p, false)), float(pg(&qtys, p, true)), int(pg(&qtys, p, false))]);
            }
            row
        })
        .collect()
}

fn grouped_query() -> String {
    let mut cols = vec!["item".to_string(), "MEDIAN(price)".to_string()];
    for p in FRACTIONS {
        cols.push(format!("PERCENTILE_CONT({p}) WITHIN GROUP (ORDER BY price)"));
        cols.push(format!("percentile_disc({p}) within group (order by price)"));
        cols.push(format!("Percentile_Cont({p}) WITHIN GROUP (ORDER BY qty ASC)"));
        cols.push(format!("PERCENTILE_DISC({p}) WITHIN GROUP (ORDER BY qty NULLS LAST)"));
    }
    format!("SELECT {} FROM snaps GROUP BY item ORDER BY item", cols.join(", "))
}

fn engines() -> Vec<Engine> {
    let mut out = vec![Engine::Operators];
    if cfg!(feature = "datafusion") {
        out.push(Engine::DataFusion);
    }
    out
}

fn sorted(mut rows: Vec<Vec<Value>>) -> Vec<String> {
    let mut out: Vec<String> = rows.drain(..).map(|r| format!("{r:?}")).collect();
    out.sort();
    out
}

/// The grouped percentiles against the rows, and every query's answer
/// against the operators' over the rows of a subquery, which compute it
/// row by row.
fn check(db: &Db, rows: &BTreeMap<i64, Row>, stage: &str) {
    let q = grouped_query();
    let got = db.query(&q, &Params::new()).unwrap_or_else(|e| panic!("{stage}: {q}: {e}"));
    assert!(got.stats.plan.iter().any(|l| l.contains("rows into groups") && l.contains("from the columns")), "{stage}: {:?}", got.stats.plan);
    assert_eq!(got.rows, expected(rows), "{stage}");
    for q in [
        q.as_str(),
        "SELECT MEDIAN(price), PERCENTILE_DISC(0.5) WITHIN GROUP (ORDER BY qty), PERCENTILE_DISC(0.5) WITHIN GROUP (ORDER BY at), COUNT(*) FROM snaps",
        "SELECT item, MEDIAN(price) AS m, AVG(price) FROM snaps WHERE qty > 5 GROUP BY item HAVING MEDIAN(price) > 300 ORDER BY m DESC",
        "SELECT item, qty, PERCENTILE_CONT(0.75) WITHIN GROUP (ORDER BY price) AS p75 FROM snaps WHERE qty < 4 GROUP BY item, qty ORDER BY p75 DESC, item, qty LIMIT 9",
        "SELECT qty, MEDIAN(price), PERCENTILE_DISC(0.9) WITHIN GROUP (ORDER BY at) FROM snaps GROUP BY qty",
    ] {
        let got = db.query(q, &Params::new()).unwrap_or_else(|e| panic!("{stage}: {q}: {e}"));
        let plan = db.explain(q, &Params::new()).unwrap();
        assert!(plan.features.iter().any(|f| f == "percentiles"), "{stage}: {q}: {:?}", plan.features);
        let by_rows = q.replace("FROM snaps", "FROM (SELECT id, item, price, qty, at FROM snaps) AS s");
        for engine in engines() {
            let opts = ExecOptions { engine, ..db.exec.clone() };
            let want = db.query_with(&by_rows, &Params::new(), &opts).unwrap_or_else(|e| panic!("{stage}: {by_rows} ({engine:?}): {e}"));
            assert!(!want.stats.plan.iter().any(|l| l.contains("rows into groups")), "{stage}: {q}: by rows: {:?}", want.stats.plan);
            assert_eq!(got.columns, want.columns, "{stage}: {q}");
            assert!(!got.rows.is_empty(), "{stage}: {q}");
            // Ordered by the query's own ORDER BY, not a WITHIN GROUP's.
            if q.rsplit_once("FROM snaps").is_some_and(|(_, tail)| tail.contains("ORDER BY")) {
                assert_eq!(got.rows, want.rows, "{stage}: {q} ({engine:?})");
            } else {
                assert_eq!(sorted(got.rows.clone()), sorted(want.rows), "{stage}: {q} ({engine:?})");
            }
        }
    }
}

#[test]
fn percentiles_of_groups_answer_as_postgres() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path());
    let mut rng = StdRng::seed_from_u64(7);
    let mut rows = BTreeMap::new();
    fill(&db, &mut rows, &mut rng, 0, 120, 0);
    check(&db, &rows, "memtable");
    fill(&db, &mut rows, &mut rng, 120, 1500, 150);
    check(&db, &rows, "segments");
    sql(&db, "DELETE FROM snaps WHERE id BETWEEN 300 AND 420 OR id IN (5, 77, 140)");
    rows.retain(|&i, _| !((300..=420).contains(&i) || [5, 77, 140].contains(&i)));
    sql(&db, "UPDATE snaps SET price = 1.5 WHERE id BETWEEN 600 AND 700");
    for (_, r) in rows.range_mut(600..=700) {
        r.1 = Some(1.5);
    }
    sql(&db, "UPDATE snaps SET qty = qty + 100 WHERE item = 'item3' AND id < 200");
    for (_, r) in rows.range_mut(..200).filter(|(_, r)| r.0 == "item3") {
        r.2 = r.2.map(|q| q + 100);
    }
    check(&db, &rows, "deletes and updates");
    db.storage().merge(&MergePolicy { merge_factor: 2, floor_docs: 10_000, ..MergePolicy::default() }).unwrap();
    check(&db, &rows, "merged");
}

#[test]
fn percentiles_in_sql() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path());
    sql(&db, "CREATE TABLE items (item KEYWORD PRIMARY KEY, title TEXT)");
    sql(&db, "INSERT INTO items VALUES ('a', 'Tent'), ('b', 'Stove'), ('c', 'Lamp')");
    sql(
        &db,
        "INSERT INTO snaps VALUES (1, 'a', 10, 1, '2026-01-01T00:00:00Z'), (2, 'a', 1000, 2, '2026-01-03T00:00:00Z'), (3, 'a', 20, 3, '2026-01-02T00:00:00Z'), \
         (4, 'b', 5, 4, '2026-02-01T00:00:00Z'), (5, 'b', NULL, NULL, NULL), (6, 'c', NULL, NULL, NULL)",
    );
    let ts = |s: &str| Value::Timestamp(timestamp::parse(s).unwrap());
    // The median is not moved by the outlier the mean is.
    assert_eq!(
        sql(&db, "SELECT item, MEDIAN(price), AVG(price), PERCENTILE_DISC(0.5) WITHIN GROUP (ORDER BY qty), PERCENTILE_DISC(0.5) WITHIN GROUP (ORDER BY at) FROM snaps GROUP BY item ORDER BY item"),
        vec![
            vec![Value::Str("a".into()), Value::Float(20.0), Value::Float(1030.0 / 3.0), Value::Int(2), ts("2026-01-02")],
            vec![Value::Str("b".into()), Value::Float(5.0), Value::Float(5.0), Value::Int(4), ts("2026-02-01")],
            vec![Value::Str("c".into()), Value::Null, Value::Null, Value::Null, Value::Null],
        ]
    );
    // An INT's continuous percentile is a FLOAT; without groups, one row.
    assert_eq!(sql(&db, "SELECT MEDIAN(qty), PERCENTILE_CONT(0.75) WITHIN GROUP (ORDER BY qty) FROM snaps"), vec![vec![Value::Float(2.5), Value::Float(3.25)]]);
    assert_eq!(sql(&db, "SELECT MEDIAN(price) FROM snaps WHERE id > 100"), vec![vec![Value::Null]]);
    // In HAVING and ORDER BY, by alias and by call.
    assert_eq!(sql(&db, "SELECT item, MEDIAN(price) AS m FROM snaps GROUP BY item HAVING MEDIAN(price) > 6 ORDER BY m DESC"), vec![vec![Value::Str("a".into()), Value::Float(20.0)]]);
    assert_eq!(
        sql(&db, "SELECT item FROM snaps GROUP BY item ORDER BY PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY price) DESC"),
        vec![vec![Value::Str("a".into())], vec![Value::Str("b".into())], vec![Value::Str("c".into())]]
    );
    // Over a join and in a WITH, the operators': of expressions too, and
    // with the fraction a parameter.
    assert_eq!(
        sql(&db, "SELECT i.title, MEDIAN(s.price * 2) AS m FROM snaps s JOIN items i ON s.item = i.item GROUP BY i.title ORDER BY i.title"),
        vec![vec![Value::Str("Lamp".into()), Value::Null], vec![Value::Str("Stove".into()), Value::Float(10.0)], vec![Value::Str("Tent".into()), Value::Float(40.0)]]
    );
    let with = "WITH t AS (SELECT item, price FROM snaps WHERE price IS NOT NULL) SELECT PERCENTILE_DISC(:p) WITHIN GROUP (ORDER BY price) FROM t";
    let rows = match db.sql(with, &Params::new().set("p", Value::Float(0.75))).unwrap() {
        SqlResult::Rows(r) => r.rows,
        _ => panic!("rows"),
    };
    assert_eq!(rows, vec![vec![Value::Float(20.0)]]);
    // The plan names its feature, which an older Strata refuses; its
    // fraction travels in its JSON; EXPLAIN says what it computes.
    let plan = db.explain("SELECT item, MEDIAN(price), PERCENTILE_DISC(0.1) WITHIN GROUP (ORDER BY qty) FROM snaps GROUP BY item", &Params::new()).unwrap();
    assert!(plan.features.iter().any(|f| f == ir::PERCENTILES), "{:?}", plan.features);
    assert_eq!(Plan::from_json(&plan.to_json().unwrap()).unwrap(), plan);
    let features: Vec<&str> = plan.features.iter().map(String::as_str).collect();
    let older: Vec<&str> = ir::FEATURES.iter().copied().filter(|f| *f != ir::PERCENTILES).collect();
    let e = Plan::readable_by(plan.version as u64, &features, &older).err().expect("refused").to_string();
    assert!(e.contains("\"percentiles\""), "{e}");
    let inner = db.explain("SELECT item FROM items WHERE item IN (SELECT item FROM snaps GROUP BY item HAVING MEDIAN(price) > 10)", &Params::new()).unwrap();
    assert!(inner.features.iter().any(|f| f == ir::PERCENTILES), "{:?}", inner.features);
    let text = sql(&db, "EXPLAIN SELECT item, MEDIAN(price), PERCENTILE_DISC(0.25) WITHIN GROUP (ORDER BY qty) FROM snaps GROUP BY item");
    let text = format!("{text:?}");
    assert!(text.contains("PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY price)") && text.contains("PERCENTILE_DISC(0.25) WITHIN GROUP (ORDER BY qty)"), "{text}");

    // What is wrong, said.
    for (q, says) in [
        ("SELECT PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY price DESC) FROM snaps", "ascending only"),
        ("SELECT PERCENTILE_CONT(1.5) WITHIN GROUP (ORDER BY price) FROM snaps", "from 0 to 1"),
        ("SELECT PERCENTILE_DISC(price) WITHIN GROUP (ORDER BY qty) FROM snaps", "fraction"),
        ("SELECT PERCENTILE_CONT(0.5) FROM snaps", "WITHIN GROUP (ORDER BY"),
        ("SELECT MEDIAN(price, 0.5) FROM snaps", "MEDIAN takes one"),
        ("SELECT MEDIAN(item) FROM snaps", "KEYWORD field"),
        ("SELECT MEDIAN(at) FROM snaps", "PERCENTILE_DISC picks one of a TIMESTAMP"),
        ("SELECT MEDIAN(DISTINCT price) FROM snaps", "DISTINCT works in COUNT only"),
        ("SELECT id, MEDIAN(price) OVER (PARTITION BY item) FROM snaps", "not a window function"),
        ("WITH t AS (SELECT price FROM snaps) SELECT PERCENTILE_CONT(2) WITHIN GROUP (ORDER BY price) FROM t", "from 0 to 1"),
    ] {
        let e = db.query(q, &Params::new()).err().unwrap_or_else(|| panic!("{q} is an error")).to_string();
        assert!(e.contains(says), "{q}: {e}");
    }
}

#[test]
fn a_percentile_counts_its_values_against_the_query_budget() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path());
    let values: Vec<String> = (0..20_000).map(|i| format!("({i}, 'item{}', {i}.5, {i}, NULL)", i % 3)).collect();
    for chunk in values.chunks(2000) {
        sql(&db, &format!("INSERT INTO snaps VALUES {}", chunk.join(", ")));
        db.storage().flush().unwrap();
    }
    // 0, 3, .., 19998; 1, 4, .., 19999; 2, 5, .., 19997 (an even count).
    let want: HashMap<&str, Value> = [("item0", 9999.0), ("item1", 10000.0), ("item2", 9999.5)].into_iter().map(|(k, v)| (k, Value::Float(v))).collect();
    for q in ["SELECT item, MEDIAN(qty) FROM snaps GROUP BY item", "WITH t AS (SELECT item, qty FROM snaps) SELECT item, MEDIAN(qty) FROM t GROUP BY item"] {
        let budget = strata::query::memory::MemoryBudget::new(64 << 20);
        let rs = db.query_with(q, &Params::new(), &ExecOptions { memory: Some(budget.clone()), ..db.exec.clone() }).unwrap();
        for r in &rs.rows {
            let Value::Str(item) = &r[0] else { panic!("{r:?}") };
            assert_eq!(r[1], want[item.as_str()], "{q}");
        }
        assert_eq!(budget.used(), 0, "given back when the query ends");
        // 20,000 values of 8 bytes do not fit in 64 KiB.
        let small = strata::query::memory::MemoryBudget::new(64 << 10);
        match db.query_with(q, &Params::new(), &ExecOptions { memory: Some(small.clone()), ..db.exec.clone() }) {
            Err(strata::Error::Memory(_)) => {}
            other => panic!("{q}: {other:?}"),
        }
        assert!(small.trips() >= 1, "{q}");
    }
}
