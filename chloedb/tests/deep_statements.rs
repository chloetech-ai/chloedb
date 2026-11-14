//! Statements that nest deeply (long chains of `UNION`, `||`, `+`, a
//! `GROUP BY` of a long sum) either answer or are refused with a query
//! error, on a thread with a tokio worker's 2 MiB of stack: none of them
//! overflows it, which aborts the process. Each shape runs at the depth
//! a statement plans and runs at on the thread that asks for it
//! (`INLINE_DEPTH`), at the depth the parser takes ([`MAX_EXPR_DEPTH`],
//! [`SET_OP_LEVELS`]), which runs on a thread of its own, and just past
//! it, on the operators and on DataFusion.

use chloedb::db::Db;
use chloedb::query::exec::{Engine, ExecOptions};
use chloedb::query::parse::{nests_deep, parse, MAX_EXPR_DEPTH, SET_OP_LEVELS};
use chloedb::query::Params;
use chloedb::storage::DbOptions;
use chloedb::{Error, Field, FieldType, Schema};

/// A tokio worker's stack, and a blocking thread's. A build without
/// optimisation has frames several times larger.
const STACK: usize = if cfg!(debug_assertions) { 16 << 20 } else { 2 << 20 };

fn open(dir: &std::path::Path) -> Db {
    let schema = Schema::new("t1", vec![Field::new("id", FieldType::Int), Field::new("n", FieldType::Int), Field::new("a", FieldType::Text { positions: false })], Some("id")).unwrap();
    let db = Db::create(&dir.join("t.chloedb"), schema, DbOptions { sync: false, ..DbOptions::default() }).unwrap();
    let rows: Vec<String> = (0..50).map(|i| format!("({i}, {}, 'row {i}')", i % 7)).collect();
    db.sql(&format!("INSERT INTO t1 (id, n, a) VALUES {}", rows.join(", ")), &Params::new()).unwrap();
    db
}

/// The queries joined by set operators that the parser takes at most.
fn most_joined() -> usize {
    (MAX_EXPR_DEPTH - 1) / SET_OP_LEVELS + 1
}

/// Each shape, by the number of terms (or queries) it is written with.
#[allow(clippy::type_complexity)]
fn shapes() -> Vec<(&'static str, Box<dyn Fn(usize) -> String>, usize)> {
    let chain = |sep: &'static str, term: &'static str| move |n: usize| vec![term; n].join(sep);
    let terms = MAX_EXPR_DEPTH - 10;
    vec![
        ("UNION ALL", Box::new(move |n| chain(" UNION ALL ", "SELECT 1")(n)), most_joined()),
        ("UNION", Box::new(move |n| chain(" UNION ", "SELECT id FROM t1 WHERE id = 1")(n)), most_joined()),
        ("INTERSECT", Box::new(move |n| chain(" INTERSECT ", "SELECT 1")(n)), most_joined()),
        ("EXCEPT", Box::new(move |n| chain(" EXCEPT ", "SELECT n FROM t1")(n)), most_joined()),
        ("IN (UNION ALL)", Box::new(move |n| format!("SELECT id FROM t1 WHERE id IN ({})", chain(" UNION ALL ", "SELECT 1")(n))), most_joined()),
        ("||", Box::new(move |n| format!("SELECT {} FROM t1 LIMIT 1", chain(" || ", "'a'")(n))), terms),
        ("|| of a column", Box::new(move |n| format!("SELECT {} FROM t1 LIMIT 1", chain(" || ", "a")(n))), terms),
        ("+", Box::new(move |n| format!("SELECT {} FROM t1 LIMIT 1", chain(" + ", "1")(n))), terms),
        ("GROUP BY a sum", Box::new(move |n| format!("SELECT {} AS k, COUNT(*) FROM t1 GROUP BY k", chain(" + ", "n")(n))), terms),
        ("ORDER BY a sum", Box::new(move |n| format!("SELECT id FROM t1 ORDER BY {} DESC LIMIT 1", chain(" + ", "n")(n))), terms),
        ("WHERE a sum", Box::new(move |n| format!("SELECT id FROM t1 WHERE {} >= 0", chain(" + ", "n")(n))), terms),
    ]
}

fn engines() -> Vec<Engine> {
    let mut e = vec![Engine::Operators];
    if cfg!(feature = "datafusion") {
        e.push(Engine::Auto);
    }
    e
}

#[test]
fn deep_statements_answer_or_are_refused_within_a_workers_stack() {
    let dir = tempfile::tempdir().unwrap();
    let db = std::sync::Arc::new(open(dir.path()));
    for (name, sql, at) in shapes() {
        // The longest of the shape planned and run on the thread that asks
        // (on 2 MiB), then one near the longest the parser takes (on a
        // thread of QUERY_STACK), then the shortest it refuses.
        let inline = (1..at).rev().find(|&n| !nests_deep(&sql(n))).expect("a statement short enough");
        let past = (at..).find(|&n| parse(&sql(n)).is_err()).expect("a statement too deep");
        for (n, ok) in [(inline, true), (at, true), (past, false)] {
            let text = sql(n);
            for engine in engines() {
                // Which statement it was, should one abort the process.
                eprintln!("{name} of {n} on {engine:?}");
                let db = db.clone();
                let text = text.clone();
                let r = std::thread::Builder::new()
                    .stack_size(STACK)
                    .spawn(move || {
                        let opts = ExecOptions { engine, ..db.exec.clone() };
                        db.query_with(&text, &Params::new(), &opts).map(|rs| rs.rows.len())
                    })
                    .unwrap()
                    .join()
                    .unwrap();
                match (ok, &r) {
                    (true, Ok(_)) => {}
                    (false, Err(Error::Query(m))) if m.contains("nests more than") => {}
                    _ => panic!("{name} of {n} on {engine:?}: {r:?}"),
                }
            }
        }
    }
}

#[test]
fn a_query_joining_many_queries_answers_their_rows() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path());
    let n = most_joined();
    let sql = (0..n).map(|i| format!("SELECT id FROM t1 WHERE id = {}", i % 50)).collect::<Vec<_>>().join(" UNION ALL ");
    for engine in engines() {
        let rs = db.query_with(&sql, &Params::new(), &ExecOptions { engine, ..db.exec.clone() }).unwrap();
        assert_eq!(rs.rows.len(), n, "{engine:?}");
    }
    // INTERSECT binds tighter than UNION, also along a chain.
    let rs = db.query("SELECT id FROM t1 WHERE id < 3 UNION SELECT id FROM t1 WHERE id = 10 INTERSECT SELECT id FROM t1 WHERE id > 5 EXCEPT SELECT 1", &Params::new()).unwrap();
    let mut ids: Vec<String> = rs.rows.iter().map(|r| format!("{:?}", r[0])).collect();
    ids.sort();
    assert_eq!(ids.len(), 3, "{ids:?}");
}
