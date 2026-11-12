//! Joins, `WITH`, subqueries and `DISTINCT` are plan operators over
//! single-table queries ([`chloedb::query::ops`]): rows they sort in memory
//! keep the executor's order, a `WITH` result's `LIMIT` holds under
//! `DISTINCT`, and a hand-made plan that names what is not there fails
//! with an error.

use chloedb::db::Db;
use chloedb::query::compound;
use chloedb::query::exec::ExecOptions;
use chloedb::query::ir::{Expr, Node, Plan, OPERATORS};
use chloedb::query::Params;
use chloedb::storage::DbOptions;
use chloedb::value::Value;
use chloedb::{Field, FieldType, Schema};

fn schema() -> Schema {
    Schema::new(
        "p",
        vec![Field::new("id", FieldType::Keyword), Field::new("cat", FieldType::Keyword), Field::new("year", FieldType::Int), Field::new("cites", FieldType::Edges)],
        Some("id"),
    )
    .unwrap()
}

/// Twelve rows over three categories, three of them without a year.
fn open(dir: &std::path::Path) -> Db {
    let db = Db::create(&dir.join("p.strata"), schema(), DbOptions::default()).unwrap();
    let rows = (0..12).map(|i| {
        let year = if i % 4 == 1 { Value::Null } else { Value::Int(2000 + i) };
        vec![Value::Str(format!("p{i:02}")), Value::Str(["a", "b", "c"][i as usize % 3].into()), year, Value::Null]
    });
    db.load(rows).unwrap();
    db
}

fn column(db: &Db, sql: &str, c: usize) -> Vec<Value> {
    let rs = db.query(sql, &Params::new()).unwrap_or_else(|e| panic!("{sql}: {e}"));
    rs.rows.into_iter().map(|r| r[c].clone()).collect()
}

/// Rows a compound query sorts in memory put NULLs where the executor
/// does: last, ascending or descending, unless `NULLS FIRST`. They came
/// first in an ascending sort.
#[test]
fn sorts_in_memory_put_nulls_where_the_executor_does() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path());
    for order in ["year", "year DESC", "year NULLS FIRST", "year DESC NULLS FIRST"] {
        let want = column(&db, &format!("SELECT id, year FROM p ORDER BY {order} LIMIT 12"), 1);
        assert_eq!(want.iter().filter(|v| matches!(v, Value::Null)).count(), 3, "{order}");
        assert_eq!(matches!(want[0], Value::Null), order.contains("NULLS FIRST"), "{order}: {want:?}");
        // A derived table and a join hold their rows in memory.
        for sql in [
            format!("SELECT t.id, t.year FROM (SELECT id, year FROM p) AS t ORDER BY t.{order} LIMIT 12"),
            format!("SELECT a.id, a.year FROM p a JOIN p b ON a.id = b.id ORDER BY a.{order} LIMIT 12"),
        ] {
            assert_eq!(column(&db, &sql, 1), want, "{sql}");
        }
    }
}

/// `SELECT DISTINCT` over a `WITH` result of the table's rows keeps its
/// `LIMIT`; it answered every distinct value.
#[test]
fn distinct_over_a_with_result_keeps_its_limit() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path());
    for (sql, n) in [
        ("WITH r AS (SELECT id, cat FROM p WHERE year >= 2000) SELECT DISTINCT cat FROM r LIMIT 2", 2),
        ("WITH r AS (SELECT id, cat FROM p WHERE year >= 2000) SELECT DISTINCT cat FROM r", 3),
        ("WITH r AS (SELECT id, cat FROM p) SELECT cat, COUNT(*) FROM r GROUP BY cat LIMIT 1", 1),
    ] {
        assert_eq!(column(&db, sql, 0).len(), n, "{sql}");
    }
}

/// A compound query's plan needs the operators feature, reads back from
/// JSON as it was, and runs as its SQL does.
#[test]
fn compound_plans_run_from_json() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path());
    let schema = schema();
    for (sql, n) in [
        // p00's category is a: p03 (2003), p06, p09 (no year) and p00.
        ("SELECT a.id, b.id FROM p a JOIN p b ON a.cat = b.cat WHERE a.id = 'p00' AND b.year > 2002", 2),
        ("WITH r AS (SELECT cat, COUNT(*) AS n FROM p GROUP BY cat) SELECT cat FROM r WHERE n >= 4", 3),
        // The years average 2005.67: an INT field compared with a fraction.
        ("SELECT id FROM p WHERE year > (SELECT AVG(year) FROM p) AND EXISTS (SELECT id FROM p WHERE cat = 'c')", 5),
        ("SELECT id FROM p WHERE year IN (SELECT AVG(year) FROM p)", 0),
        ("SELECT id FROM p WHERE year IN (SELECT MAX(year) FROM p)", 1),
    ] {
        let plan = compound::plan(sql, &schema, &Params::new()).unwrap();
        assert_eq!(plan.features, [OPERATORS], "{sql}");
        let json = plan.to_json().unwrap();
        let back = Plan::from_json(&json).unwrap();
        assert_eq!(back, plan, "{sql}");
        let want = db.query(sql, &Params::new()).unwrap();
        assert_eq!(want.rows.len(), n, "{sql}: {:?}", want.rows);
        assert_eq!(db.run_at(&db.snapshot(), &back, &ExecOptions::default()).unwrap().rows, want.rows, "{sql}");
    }
}

/// A hand-made plan that names a `WITH` result or a column that is not
/// there fails with an error saying so.
#[test]
fn hand_made_plans_fail_with_errors() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path());
    let schema = schema();
    let run = |plan: &Plan| db.run_at(&db.snapshot(), plan, &ExecOptions::default()).err().unwrap_or_else(|| panic!("{plan:?}")).to_string();
    let join = compound::plan("SELECT a.id, b.id FROM p a JOIN p b ON a.cat = b.cat", &schema, &Params::new()).unwrap();
    let mut bad = join.clone();
    let mut keys = 0;
    visit(&mut bad.root, &mut |n| {
        if let Node::Join { keys: ks, .. } = n {
            ks[0].right = Expr::Output(9);
            keys += 1;
        }
    });
    assert_eq!(keys, 1);
    let e = run(&bad);
    assert!(e.contains("no column 9"), "{e}");
    let with = compound::plan("WITH r AS (SELECT cat, COUNT(*) AS n FROM p GROUP BY cat) SELECT cat FROM r", &schema, &Params::new()).unwrap();
    let mut bad = with.clone();
    let mut refs = 0;
    visit(&mut bad.root, &mut |n| {
        if let Node::Ref { name } = n {
            *name = "nowhere".into();
            refs += 1;
        }
    });
    assert!(refs > 0);
    let e = run(&bad);
    assert!(e.contains("no WITH result named nowhere"), "{e}");
}

/// Calls `f` on each node of a plan tree, parents first.
fn visit(n: &mut Node, f: &mut dyn FnMut(&mut Node)) {
    f(n);
    match n {
        Node::Project { input, .. } | Node::Limit { input, .. } | Node::Sort { input, .. } | Node::Filter { input, .. } | Node::Aggregate { input, .. } | Node::Distinct { input } => visit(input, f),
        Node::Join { left, right, .. } => {
            visit(left, f);
            visit(right, f);
        }
        Node::With { ctes, input } => {
            for c in ctes {
                visit(&mut c.input, f);
            }
            visit(input, f);
        }
        Node::KeyOrder { input, by } => {
            visit(input, f);
            visit(by, f);
        }
        _ => {}
    }
}

/// Under a LIMIT, a join's first table and the rows of `DISTINCT ON` are
/// read in growing slices, up to [`compound::MAX_ROWS`] rows. When the
/// last slice was cut there and still too few rows came out, the answer
/// was a short page, silently (audit Q1); the same query without LIMIT is
/// refused for reading too many rows. A cut scan is that error too.
#[test]
fn a_limit_past_the_rows_a_scan_may_read_is_an_error_not_a_short_page() {
    let dir = tempfile::tempdir().unwrap();
    let orders = Schema::new("o", vec![Field::new("id", FieldType::Keyword), Field::new("cust", FieldType::Keyword)], Some("id")).unwrap();
    let db = Db::create(&dir.path().join("o.strata"), orders, DbOptions { sync: false, segment_max_docs: 400_000, ..DbOptions::default() }).unwrap();
    db.sql("CREATE TABLE c (id KEYWORD PRIMARY KEY, country KEYWORD)", &Params::new()).unwrap();
    db.sql("INSERT INTO c (id, country) VALUES ('c0', 'XX'), ('c1', 'IS')", &Params::new()).unwrap();
    // The one order of c1 comes after the first MAX_ROWS in scan order.
    let n = compound::MAX_ROWS + 20;
    let late = compound::MAX_ROWS + 5;
    db.load((0..n).map(|i| vec![Value::Str(format!("o{i:08}")), Value::Str(if i == late { "c1" } else { "c0" }.into())])).unwrap();
    for (sql, want) in [
        ("SELECT o.id FROM o JOIN c ON o.cust = c.id WHERE c.country = 'IS' LIMIT 10", vec![format!("o{late:08}")]),
        ("SELECT DISTINCT ON (cust) id FROM o LIMIT 2", vec![format!("o{:08}", 0), format!("o{late:08}")]),
    ] {
        match db.query(sql, &Params::new()) {
            Ok(r) => {
                let got: Vec<String> = r.rows.iter().map(|r| r[0].as_str().unwrap().to_string()).collect();
                assert_eq!(got, want, "{sql}: a short page");
            }
            Err(e) => assert!(e.to_string().contains(&format!("more than {} rows", compound::MAX_ROWS)), "{sql}: {e}"),
        }
    }
}
