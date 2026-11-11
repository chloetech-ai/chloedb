//! The limits a database opened in a program puts on its queries by
//! default, and the ones its caller gives a query: a query that would hold
//! more than the process's memory budget fails with `Error::Memory`, naming
//! the budget and how to raise it, instead of growing until the program
//! runs out of memory; a query given a deadline, or cancelled from another
//! thread ([`Interrupts`]), stops within a few thousand rows of it.
//!
//! This binary sizes the process's budget with `CHLOEDB_QUERY_MEMORY_MB`
//! before its first database opens; every test calls [`small_budget`]
//! first.

use std::sync::Once;
use std::time::{Duration, Instant};

use chloedb::db::{Db, SqlResult};
use chloedb::error::Error;
use chloedb::query::exec::{Engine, ExecOptions, Interrupts};
use chloedb::query::memory;
use chloedb::query::Params;
use chloedb::storage::DbOptions;
use chloedb::value::Value;
use chloedb::{Field, FieldType, Schema};

const N: i64 = 200_000;

/// The process's budget is 2 MiB, read when the first database opens.
fn small_budget() {
    static SET: Once = Once::new();
    SET.call_once(|| std::env::set_var(memory::PROCESS_SETTING, "2"));
}

/// `N` rows: an integer key, `a` with a value per row, `c` (3 values) and
/// an 8 KB `note` on the first 1,000.
fn db(dir: &std::path::Path) -> Db {
    small_budget();
    let schema = Schema::new(
        "t",
        vec![
            Field::new("id", FieldType::Int),
            Field::new("a", FieldType::Int),
            Field::new("c", FieldType::Int),
            Field::new("note", FieldType::Text { positions: false }).stored_only(),
        ],
        Some("id"),
    )
    .unwrap();
    let db = Db::create(&dir.join("d.strata"), schema, DbOptions { sync: false, ..DbOptions::default() }).unwrap();
    db.load((0..N).map(|i| {
        let note = if i < 1000 { Value::Str(format!("{i:08} ").repeat(900)) } else { Value::Null };
        vec![Value::Int(i), Value::Int(i * 7919 % N), Value::Int(i % 3), note]
    }))
    .unwrap();
    db
}

fn run(db: &Db, sql: &str, opts: &ExecOptions) -> Result<usize, Error> {
    db.query_with(sql, &Params::new(), opts).map(|r| r.rows.len())
}

#[test]
fn memory_hungry_queries_fail_under_the_process_budget() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path());
    let budget = db.exec.memory.clone().expect("a database counts its queries against the process's budget");
    assert_eq!(budget.limit(), 2 << 20);
    // A second database shares it.
    let other = Db::create(&dir.path().join("e.strata"), Schema::new("u", vec![Field::new("id", FieldType::Int)], Some("id")).unwrap(), DbOptions::default()).unwrap();
    assert!(std::sync::Arc::ptr_eq(other.exec.memory.as_ref().unwrap(), &budget));
    // Every row with its text; a group per row; a percentile's values; the
    // rows of a large page.
    for sql in [
        "SELECT * FROM t",
        "SELECT a, COUNT(*) FROM t GROUP BY a LIMIT 5",
        "SELECT COUNT(DISTINCT a) FROM t",
        "SELECT PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY a), PERCENTILE_DISC(0.9) WITHIN GROUP (ORDER BY id) FROM t",
        "SELECT id, note FROM t WHERE id < 1000 LIMIT 1000",
    ] {
        let trips = budget.trips();
        match db.query(sql, &Params::new()) {
            Err(Error::Memory(m)) => {
                assert!(m.contains("2 MiB") && m.contains("CHLOEDB_QUERY_MEMORY_MB") && m.contains("memory budget of its own"), "{sql}: {m}");
            }
            r => panic!("{sql}: {:?}", r.map(|r| r.rows.len())),
        }
        // Refused once, and nothing held after.
        assert_eq!(budget.trips(), trips + 1, "{sql}");
        assert_eq!(budget.used(), 0, "{sql}");
        // A query given no budget, or a larger one of its own, answers.
        let free = run(&db, sql, &db.exec.limited(None, Some(0), None)).unwrap_or_else(|e| panic!("{sql}: {e}"));
        assert_eq!(run(&db, sql, &db.exec.limited(None, Some(1 << 30), None)).unwrap(), free, "{sql}");
        // A budget of its own smaller still names itself.
        match run(&db, sql, &db.exec.limited(None, Some(1 << 20), None)) {
            Err(Error::Memory(m)) => assert!(m.contains("1 MiB this query may hold (its memory option)"), "{sql}: {m}"),
            r => panic!("{sql}: {r:?}"),
        }
        // As does SQL run as a statement.
        assert!(matches!(db.sql(sql, &Params::new()), Err(Error::Memory(_))), "{sql}");
    }
    assert_eq!(run(&db, "SELECT * FROM t", &db.exec.limited(None, Some(0), None)).unwrap(), N as usize);
    // Light queries fit.
    for sql in ["SELECT COUNT(*) FROM t", "SELECT c, COUNT(*) FROM t GROUP BY c", "SELECT id, note FROM t WHERE id < 10", "SELECT id FROM t ORDER BY a DESC LIMIT 10"] {
        db.query(sql, &Params::new()).unwrap_or_else(|e| panic!("{sql}: {e}"));
    }
    assert_eq!(budget.used(), 0);
}

/// Every row meets the 8,000 rows of its `c`, and none passes the rest of
/// the condition: billions of comparisons holding next to nothing, on
/// Strata's operators (DataFusion answers it at once).
const SLOW: &str = "SELECT x.id, y.id FROM t x JOIN t y ON x.c = y.c AND x.a < y.a - 1000000";

fn slow_db(dir: &std::path::Path) -> Db {
    let mut db = db(dir);
    db.exec.engine = Engine::Operators;
    // Its join's tables of 24,000 rows hold more than this binary's 2 MiB.
    db.exec.memory = None;
    db.sql("DELETE FROM t WHERE id >= 24000", &Params::new()).unwrap();
    db
}

#[test]
fn a_deadline_stops_a_slow_query() {
    let dir = tempfile::tempdir().unwrap();
    let db = slow_db(dir.path());
    let t0 = Instant::now();
    let r = run(&db, SLOW, &db.exec.limited(Some(Duration::from_millis(100)), None, None));
    let took = t0.elapsed();
    assert!(matches!(r, Err(Error::Timeout(ref m)) if m.contains("deadline")), "{r:?}");
    assert!(took < Duration::from_secs(5), "stopped after {took:?}");
    // The earlier of two deadlines holds.
    let opts = ExecOptions { deadline: Some(Instant::now()), ..db.exec.clone() }.limited(Some(Duration::from_secs(3600)), None, None);
    assert!(matches!(run(&db, SLOW, &opts), Err(Error::Timeout(_))));
    // Db::sql_with and Db::run_with take the options too; a write runs as
    // Db::sql runs it.
    let past = db.exec.limited(Some(Duration::ZERO), None, None);
    assert!(matches!(db.sql_with(SLOW, &Params::new(), &past), Err(Error::Timeout(_))));
    let plan = db.explain(SLOW, &Params::new()).unwrap();
    assert!(matches!(db.run_with(&plan, &past), Err(Error::Timeout(_))));
    assert!(matches!(db.sql_with("UPDATE t SET c = 2 WHERE id = 0", &Params::new(), &past), Ok(SqlResult::Written(1))));
}

#[test]
fn another_thread_cancels_the_queries_running() {
    let dir = tempfile::tempdir().unwrap();
    let db = slow_db(dir.path());
    let running = Interrupts::default();
    let t0 = Instant::now();
    let (r, interrupted) = std::thread::scope(|s| {
        let q = s.spawn(|| run(&db, SLOW, &db.exec.limited(None, None, Some(running.start(1)))));
        // Another query, tagged otherwise, is left alone.
        let other = running.start(2);
        while running.running() < 2 {
            std::thread::yield_now();
        }
        std::thread::sleep(Duration::from_millis(100));
        let n = running.interrupt(Some(1));
        assert!(!other.load(std::sync::atomic::Ordering::Relaxed));
        drop(other);
        (q.join().unwrap(), n)
    });
    assert_eq!(interrupted, 1);
    assert!(matches!(r, Err(Error::Timeout(ref m)) if m.contains("cancelled")), "{r:?}");
    assert!(t0.elapsed() < Duration::from_secs(5), "stopped after {:?}", t0.elapsed());
    // Finished queries leave the set; one that starts after an interrupt
    // runs.
    assert_eq!(running.running(), 0);
    assert_eq!(running.interrupt(None), 0);
    assert_eq!(run(&db, "SELECT COUNT(*) FROM t", &db.exec.limited(None, None, Some(running.start(1)))).unwrap(), 1);
}
