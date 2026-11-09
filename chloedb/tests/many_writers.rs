//! Many threads writing to one database at once: they wait for the writer
//! instead of failing (`DbOptions::write_wait`), share commits
//! (`Db::write_ops`, SQL writes), lose no update, and a write that fails
//! fails alone.

use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use chloedb::db::{Db, Footprints, SqlResult, WriteOp};
use chloedb::query::Params;
use chloedb::storage::DbOptions;
use chloedb::value::Value;
use chloedb::{Error, Schema};

const THREADS: usize = 8;

fn schema() -> Schema {
    Schema::from_json(
        r#"{"table": "docs", "key": "id", "fields": [
            {"name": "id", "type": "keyword"}, {"name": "body", "type": "text"}, {"name": "n", "type": "int"}]}"#,
    )
    .unwrap()
}

fn other() -> Schema {
    Schema::from_json(r#"{"table": "other", "key": "id", "fields": [{"name": "id", "type": "int"}, {"name": "n", "type": "int"}]}"#).unwrap()
}

fn create(dir: &std::path::Path, opts: DbOptions) -> Db {
    let db = Db::create(&dir.join("w.strata"), schema(), opts).unwrap();
    db.create_table(other()).unwrap();
    db
}

fn doc(id: &str, n: i64) -> Vec<Value> {
    vec![Value::Str(id.into()), Value::Str(format!("the body of {id}")), Value::Int(n)]
}

fn int(db: &Db, sql: &str) -> i64 {
    db.query(sql, &Params::new()).unwrap().rows[0][0].as_i64().unwrap()
}

fn sql(db: &Db, sql: &str, params: &[(&str, Value)]) -> chloedb::Result<u64> {
    let params = params.iter().fold(Params::new(), |p, (k, v)| p.set(k, v.clone()));
    match db.sql(sql, &params)? {
        SqlResult::Written(n) => Ok(n),
        SqlResult::Rows(_) => panic!("a write"),
    }
}

/// Threads inserting and adding at once all get their writes, and while
/// one group commits, those that came meanwhile share the next commit.
#[test]
fn threads_writing_at_once_share_commits() {
    let dir = tempfile::tempdir().unwrap();
    let db = create(dir.path(), DbOptions::default());
    let ins = |id: String| WriteOp::Insert { table: "docs".into(), doc: doc(&id, 0) };
    db.write_ops((0..4).map(|k| ins(format!("c{k}"))).collect()).unwrap();
    let start = Barrier::new(THREADS);
    std::thread::scope(|s| {
        for t in 0..THREADS {
            let (db, start) = (&db, &start);
            s.spawn(move || {
                start.wait();
                for i in 0..150 {
                    let add = WriteOp::Add { table: "docs".into(), key: Value::Str(format!("c{}", i % 4)), field: "n".into(), delta: Value::Int(1) };
                    assert_eq!(db.write_ops(vec![ins(format!("t{t}-{i}")), add]).unwrap(), 2);
                }
            });
        }
    });
    assert_eq!(int(&db, "SELECT count(*) FROM docs"), 4 + (THREADS * 150) as i64);
    assert_eq!(int(&db, "SELECT sum(n) FROM docs"), (THREADS * 150) as i64);
    let w = db.write_stats();
    assert_eq!(w.writes, 1 + (THREADS * 150) as u64);
    assert!(w.groups < w.writes, "{w:?}: no two writes shared a commit");
}

/// UPDATEs of the same row from many threads lose none of their
/// increments (they never share a transaction, which would read the row
/// before the other's change); those of different rows do share them.
#[test]
fn concurrent_sql_updates_lose_no_update() {
    let dir = tempfile::tempdir().unwrap();
    let db = create(dir.path(), DbOptions::default());
    for k in 0..4 {
        sql(&db, "INSERT INTO docs (id, body, n) VALUES (:k, 'counter', 0)", &[("k", Value::Str(format!("k{k}")))]).unwrap();
    }
    let start = Barrier::new(THREADS);
    std::thread::scope(|s| {
        for t in 0..THREADS {
            let (db, start) = (&db, &start);
            s.spawn(move || {
                start.wait();
                for i in 0..100 {
                    // Half on one hot row, half spread over the others.
                    let k = if i % 2 == 0 { 0 } else { 1 + (t + i) % 3 };
                    let n = sql(db, "UPDATE docs SET n = n + 1 WHERE id = :k", &[("k", Value::Str(format!("k{k}")))]).unwrap();
                    assert_eq!(n, 1);
                    // An INSERT of a new row, and an upsert in another table.
                    sql(db, "INSERT INTO docs (id, body, n) VALUES (:k, 'new', 0)", &[("k", Value::Str(format!("n{t}-{i}")))]).unwrap();
                    sql(db, "INSERT OR REPLACE INTO other (id, n) VALUES (:k, :n)", &[("k", Value::Int((t * 1000 + i) as i64)), ("n", Value::Int(i as i64))]).unwrap();
                }
            });
        }
    });
    assert_eq!(int(&db, "SELECT n FROM docs WHERE id = 'k0'"), (THREADS * 50) as i64);
    assert_eq!(int(&db, "SELECT sum(n) FROM docs WHERE id IN ('k1', 'k2', 'k3')"), (THREADS * 50) as i64);
    assert_eq!(int(&db, "SELECT count(*) FROM docs"), 4 + (THREADS * 100) as i64);
    assert_eq!(int(&db, "SELECT count(*) FROM other"), (THREADS * 100) as i64);
    let w = db.write_stats();
    assert!(w.groups < w.writes, "{w:?}: no two SQL writes shared a commit");
}

/// A write that fails in a group fails alone: the writes sharing its
/// transaction run again without it.
#[test]
fn a_write_that_fails_fails_alone() {
    let dir = tempfile::tempdir().unwrap();
    let db = create(dir.path(), DbOptions::default());
    db.write_ops(vec![WriteOp::Insert { table: "docs".into(), doc: doc("taken", 0) }]).unwrap();
    let start = Barrier::new(THREADS);
    let failed = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|s| {
        for t in 0..THREADS {
            let (db, start, failed) = (&db, &start, &failed);
            s.spawn(move || {
                start.wait();
                for i in 0..100 {
                    let id = if t == 0 && i % 4 == 0 { "taken".to_string() } else { format!("t{t}-{i}") };
                    let r = match i % 2 {
                        0 => db.write_ops(vec![WriteOp::Insert { table: "docs".into(), doc: doc(&id, 1) }]),
                        _ => sql(db, "INSERT INTO docs (id, body, n) VALUES (:k, 'x', 1)", &[("k", Value::Str(id.clone()))]),
                    };
                    match r {
                        Ok(n) => assert_eq!((n, id.as_str() != "taken"), (1, true)),
                        Err(e) => {
                            assert_eq!(id, "taken", "{e}");
                            failed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                }
            });
        }
    });
    assert_eq!(failed.into_inner(), 25);
    assert_eq!(int(&db, "SELECT count(*) FROM docs"), 1 + (THREADS * 100 - 25) as i64);
    assert_eq!(int(&db, "SELECT n FROM docs WHERE id = 'taken'"), 0);
}

/// SQL writes share a transaction when no two may touch the same row:
/// different tables, or INSERTs and UPDATEs or DELETEs by key of
/// different keys. One by any other filter has its table to itself.
#[test]
fn which_sql_writes_may_share_a_transaction() {
    let dir = tempfile::tempdir().unwrap();
    let db = create(dir.path(), DbOptions::default());
    let p = |s: &str| db.prepare_write(s, &Params::new()).unwrap().unwrap();
    let admit = |writes: &[&str]| {
        let mut f = Footprints::default();
        writes.iter().map(|w| f.admit(&p(w))).collect::<Vec<bool>>()
    };
    let by_key = "UPDATE docs SET n = n + 1 WHERE id = 'a'";
    assert_eq!(admit(&[by_key, "UPDATE docs SET n = n + 1 WHERE id = 'b'", "DELETE FROM docs WHERE id IN ('c', 'd')", "INSERT INTO docs (id, n) VALUES ('e', 1)"]), [true; 4]);
    assert_eq!(admit(&[by_key, "UPDATE docs SET n = 0 WHERE id = 'a' AND n > 3"]), [true, false]);
    assert_eq!(admit(&[by_key, "DELETE FROM docs WHERE id = 'b' OR id = 'a'"]), [true, false]);
    assert_eq!(admit(&[by_key, "INSERT INTO docs (id, n) VALUES ('a', 1) ON CONFLICT (id) DO UPDATE SET n = n + 1"]), [true, false]);
    // A filter that does not pin the key: the whole table.
    assert_eq!(admit(&[by_key, "UPDATE docs SET n = 0 WHERE n > 3", "UPDATE other SET n = 1 WHERE n > 3"]), [true, false, true]);
    assert_eq!(admit(&["DELETE FROM docs WHERE n > 3", "INSERT INTO docs (id, n) VALUES ('z', 1)"]), [true, false]);

    // Run as one transaction: each statement's rows, one sequence number.
    for k in ["a", "b"] {
        sql(&db, "INSERT INTO docs (id, n) VALUES (:k, 1)", &[("k", Value::Str(k.into()))]).unwrap();
    }
    let (a, b, c) = (p(by_key), p("DELETE FROM docs WHERE id = 'b'"), p("INSERT INTO other (id, n) VALUES (7, 7)"));
    assert_eq!(db.write_prepared_group(&[&a, &b, &c]).unwrap().0, [1, 1, 1]);
    assert_eq!(int(&db, "SELECT n FROM docs WHERE id = 'a'"), 2);
    assert_eq!(int(&db, "SELECT count(*) FROM docs WHERE id = 'b'"), 0);
    // Writes that may not share one are refused, and none runs.
    assert!(db.write_prepared_group(&[&a, &a]).is_err());
    assert_eq!(int(&db, "SELECT n FROM docs WHERE id = 'a'"), 2);
    // One that fails fails them all.
    assert!(db.write_prepared_group(&[&a, &c]).is_err());
    assert_eq!(int(&db, "SELECT n FROM docs WHERE id = 'a'"), 2);
}

/// `begin` waits up to `write_wait` while another write holds the
/// writer, then fails with Busy; 0 fails at once.
#[test]
fn begin_waits_for_the_writer() {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(create(dir.path(), DbOptions::default()));
    let held = db.storage().begin().unwrap();
    let t0 = Instant::now();
    let waiter = {
        let db = db.clone();
        std::thread::spawn(move || {
            db.write_ops(vec![WriteOp::Insert { table: "docs".into(), doc: doc("late", 0) }]).unwrap();
            Instant::now()
        })
    };
    std::thread::sleep(Duration::from_millis(100));
    drop(held);
    assert!(waiter.join().unwrap() - t0 >= Duration::from_millis(100));
    assert_eq!(int(&db, "SELECT count(*) FROM docs WHERE id = 'late'"), 1);
    drop(db);

    for (wait, at_least) in [(Duration::ZERO, Duration::ZERO), (Duration::from_millis(50), Duration::from_millis(50))] {
        let dir = tempfile::tempdir().unwrap();
        let db = create(dir.path(), DbOptions { write_wait: wait, ..DbOptions::default() });
        let _held = db.storage().begin().unwrap();
        let t0 = Instant::now();
        assert!(matches!(db.storage().begin(), Err(Error::Busy(_))));
        let r = db.write_ops(vec![WriteOp::Delete { table: "docs".into(), key: Value::Str("x".into()) }]);
        assert!(matches!(r, Err(Error::Busy(_))), "{r:?}");
        assert!(t0.elapsed() >= at_least && t0.elapsed() < Duration::from_secs(2), "{:?}", t0.elapsed());
    }
}
