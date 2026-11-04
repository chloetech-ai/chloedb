//! `GROUP BY` and aggregates read from the columns
//! (`query/exec/agg_columns.rs`): on a table of several segments and a
//! memtable, with NULLs, deleted rows and values written in place, every
//! group and aggregate is the one a plain fold over the rows gives, sums
//! of FLOAT values to the bit (they are added in the rows' order), for
//! keyword, INT and FLOAT groups, one group field or several, and none.

use std::collections::BTreeMap;

use rand::{Rng, SeedableRng};
use strata::db::Db;
use strata::query::Params;
use strata::storage::DbOptions;
use strata::value::Value;
use strata::{Field, FieldType, Schema};

const ROWS: usize = 3000;

fn schema() -> Schema {
    Schema::new(
        "t",
        vec![
            Field::new("id", FieldType::Keyword),
            Field::new("cat", FieldType::Keyword),
            Field::new("year", FieldType::Int),
            Field::new("n", FieldType::Int),
            Field::new("w", FieldType::Float),
            Field::new("x", FieldType::Float),
            Field::new("big", FieldType::Int),
        ],
        Some("id"),
    )
    .unwrap()
}

const ID: usize = 0;
const CAT: usize = 1;
const YEAR: usize = 2;
const N: usize = 3;
const W: usize = 4;
const X: usize = 5;
const BIG: usize = 6;

fn row(rng: &mut rand::rngs::StdRng, i: usize) -> Vec<Value> {
    let mut maybe = |v: Value| if rng.gen_range(0..7) == 0 { Value::Null } else { v };
    let cat = maybe(Value::Str(format!("c{}", i * 7 % 13)));
    let year = maybe(Value::Int(1990 + (i * 31 % 35) as i64));
    let n = maybe(Value::Int((i as i64 * 7919) % 2001 - 1000));
    let w = maybe(Value::Float(((i * 37) % 1000) as f64 / 100.0));
    let x = maybe(Value::Float(1.0 / (i as f64 + 3.0) * if i % 2 == 0 { 1e9 } else { -1.0 }));
    let big = maybe(Value::Int(i64::MAX / 3 - i as i64 * 1_000_003));
    vec![Value::Str(format!("r{i:05}")), cat, year, n, w, x, big]
}

/// The table: most rows in segments of 400, the rest in the memtable, a
/// tenth deleted, and `n`, `w` and `year` of some rows written in place.
/// Returns the database and the rows it holds, in the order they are read.
fn open(dir: &std::path::Path, in_place: bool) -> (Db, Vec<Vec<Value>>) {
    let mut rng = rand::rngs::StdRng::seed_from_u64(11);
    let mut rows: Vec<Vec<Value>> = (0..ROWS).map(|i| row(&mut rng, i)).collect();
    let opts = DbOptions { segment_max_docs: 400, sync: false, ..DbOptions::default() };
    let db = Db::create(&dir.join("t.strata"), schema(), opts).unwrap();
    db.load(rows[..2600].iter().cloned()).unwrap();
    let mut t = db.storage().begin().unwrap();
    for r in &rows[2600..] {
        t.insert(r.clone()).unwrap();
    }
    t.commit().unwrap();
    let mut t = db.storage().begin().unwrap();
    for i in (0..ROWS).step_by(10) {
        assert!(t.delete(&rows[i][ID]).unwrap());
    }
    t.commit().unwrap();
    if in_place {
        let mut t = db.storage().begin().unwrap();
        for i in (3..ROWS).step_by(17) {
            let (n, w, y) = (Value::Int(i as i64 % 50), Value::Float((i % 300) as f64 / 7.0), Value::Int(2030 + (i % 3) as i64));
            for (f, name, v) in [(N, "n", n), (W, "w", w), (YEAR, "year", y)] {
                if i % 10 != 0 {
                    assert!(t.set(&rows[i][ID], name, v.clone()).unwrap());
                    rows[i][f] = v;
                }
            }
        }
        t.commit().unwrap();
    }
    let live = rows.into_iter().enumerate().filter(|(i, _)| i % 10 != 0).map(|(_, r)| r).collect();
    (db, live)
}

/// A group's aggregates folded over its rows in order: COUNT(*), then for
/// each of `n`, `w`, `x`, `big`: COUNT, SUM, AVG, MIN, MAX.
fn fold(rows: &[&Vec<Value>]) -> Vec<Value> {
    let mut out = vec![Value::Int(rows.len() as i64)];
    for f in [N, W, X, BIG] {
        let vals: Vec<&Value> = rows.iter().map(|r| &r[f]).filter(|v| !matches!(v, Value::Null)).collect();
        out.push(Value::Int(vals.len() as i64));
        let ints = vals.iter().all(|v| matches!(v, Value::Int(_)));
        let (sum, avg) = if vals.is_empty() {
            (Value::Null, Value::Null)
        } else if ints {
            let s: i128 = vals.iter().map(|v| v.as_i64().unwrap() as i128).sum();
            (i64::try_from(s).map_or(Value::Float(s as f64), Value::Int), Value::Float(s as f64 / vals.len() as f64))
        } else {
            let mut s = 0.0;
            for v in &vals {
                s += v.as_f64().unwrap();
            }
            (Value::Float(s), Value::Float(s / vals.len() as f64))
        };
        out.push(sum);
        out.push(avg);
        let num = |v: &&Value| v.as_f64().unwrap();
        out.push(vals.iter().copied().min_by(|a, b| num(a).partial_cmp(&num(b)).unwrap()).cloned().unwrap_or(Value::Null));
        out.push(vals.iter().copied().max_by(|a, b| num(a).partial_cmp(&num(b)).unwrap()).cloned().unwrap_or(Value::Null));
    }
    out
}

const AGGS: &str = "COUNT(*), COUNT(n), SUM(n), AVG(n), MIN(n), MAX(n), COUNT(w), SUM(w), AVG(w), MIN(w), MAX(w), \
                    COUNT(x), SUM(x), AVG(x), MIN(x), MAX(x), COUNT(big), SUM(big), AVG(big), MIN(big), MAX(big)";

/// Groups by `fields`' values (`Value`s as sortable text), as the query
/// answers them: group values, then [`fold`].
fn want(rows: &[Vec<Value>], fields: &[usize], keep: impl Fn(&Vec<Value>) -> bool) -> BTreeMap<String, Vec<Value>> {
    let mut groups: BTreeMap<String, (Vec<Value>, Vec<&Vec<Value>>)> = BTreeMap::new();
    for r in rows.iter().filter(|r| keep(r)) {
        let key: Vec<Value> = fields.iter().map(|&f| r[f].clone()).collect();
        groups.entry(format!("{key:?}")).or_insert_with(|| (key, Vec::new())).1.push(r);
    }
    groups.into_iter().map(|(k, (key, rs))| (k, key.into_iter().chain(fold(&rs)).collect())).collect()
}

fn got(db: &Db, sql: &str, n_keys: usize) -> BTreeMap<String, Vec<Value>> {
    let rs = db.query(sql, &Params::new()).unwrap_or_else(|e| panic!("{sql}: {e}"));
    rs.rows.into_iter().map(|r| (format!("{:?}", &r[..n_keys]), r)).collect()
}

fn check(db: &Db, rows: &[Vec<Value>], what: &str) {
    let cases: Vec<(String, Vec<usize>, Box<dyn Fn(&Vec<Value>) -> bool>)> = vec![
        (format!("SELECT cat, {AGGS} FROM t GROUP BY cat"), vec![CAT], Box::new(|_| true)),
        (format!("SELECT year, {AGGS} FROM t WHERE year >= 2000 GROUP BY year"), vec![YEAR], Box::new(|r| r[YEAR].as_i64().is_some_and(|y| y >= 2000))),
        (format!("SELECT w, {AGGS} FROM t GROUP BY w"), vec![W], Box::new(|_| true)),
        (format!("SELECT x, {AGGS} FROM t WHERE n > 0 GROUP BY x"), vec![X], Box::new(|r| r[N].as_i64().is_some_and(|n| n > 0))),
        (format!("SELECT big, {AGGS} FROM t WHERE cat = 'c3' GROUP BY big"), vec![BIG], Box::new(|r| r[CAT].as_str() == Some("c3"))),
        (format!("SELECT cat, year, {AGGS} FROM t WHERE w < 5 GROUP BY cat, year"), vec![CAT, YEAR], Box::new(|r| r[W].as_f64().is_some_and(|w| w < 5.0))),
        (format!("SELECT year, cat, n, {AGGS} FROM t GROUP BY year, cat, n"), vec![YEAR, CAT, N], Box::new(|_| true)),
        (format!("SELECT {AGGS} FROM t WHERE cat IN ('c1', 'c2')"), vec![], Box::new(|r| matches!(r[CAT].as_str(), Some("c1" | "c2")))),
        (format!("SELECT {AGGS} FROM t WHERE year > 3000"), vec![], Box::new(|_| false)),
    ];
    for (sql, fields, keep) in &cases {
        let mut w = want(rows, fields, keep);
        if fields.is_empty() && w.is_empty() {
            // One group of no rows.
            w.insert("[]".into(), fold(&[]));
        }
        let g = got(db, sql, fields.len());
        assert_eq!(g.len(), w.len(), "{what}: {sql}: groups");
        for (k, want_row) in &w {
            let got_row = g.get(k).unwrap_or_else(|| panic!("{what}: {sql}: no group {k}"));
            assert_eq!(got_row, want_row, "{what}: {sql}: group {k}");
        }
    }
    // HAVING, an order by an aggregate, and LIMIT over the same groups.
    let sql = "SELECT cat, COUNT(*) AS c, AVG(w) FROM t WHERE year >= 1995 GROUP BY cat HAVING COUNT(*) > 100 ORDER BY 2 DESC, cat LIMIT 4";
    let rs = db.query(sql, &Params::new()).unwrap();
    let mut w: Vec<Vec<Value>> = want(rows, &[CAT], |r| r[YEAR].as_i64().is_some_and(|y| y >= 1995))
        .into_values()
        .filter(|r| r[1].as_i64().unwrap() > 100)
        .map(|r| vec![r[0].clone(), r[1].clone(), r[9].clone()])
        .collect();
    w.sort_by(|a, b| b[1].as_i64().cmp(&a[1].as_i64()).then_with(|| format!("{:?}", a[0]).cmp(&format!("{:?}", b[0]))));
    w.truncate(4);
    assert_eq!(rs.rows, w, "{what}: {sql}");
    let plan = rs.stats.plan.join("\n");
    assert!(plan.contains("from the columns"), "{what}: the columns answered: {plan}");
}

#[test]
fn groups_from_the_columns_are_the_rows_groups() {
    for in_place in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let (db, rows) = open(dir.path(), in_place);
        check(&db, &rows, if in_place { "values written in place" } else { "plain" });
    }
}

#[test]
fn a_query_the_columns_do_not_answer_goes_row_by_row() {
    let dir = tempfile::tempdir().unwrap();
    let (db, _) = open(dir.path(), false);
    for sql in ["SELECT cat, COUNT(DISTINCT year) FROM t GROUP BY cat", "SELECT cat, MAX(id) FROM t GROUP BY cat"] {
        let rs = db.query(sql, &Params::new()).unwrap();
        assert!(!rs.stats.plan.join("\n").contains("from the columns"), "{sql}");
    }
}

#[test]
fn the_columns_count_groups_against_the_query_s_cap() {
    let dir = tempfile::tempdir().unwrap();
    let (db, _) = open(dir.path(), false);
    let opts = strata::query::exec::ExecOptions { max_groups: Some(50), ..Default::default() };
    let e = db.query_with("SELECT n, COUNT(*) FROM t GROUP BY n", &Params::new(), &opts).unwrap_err();
    assert!(e.to_string().contains("more than 50 groups"), "{e}");
}
