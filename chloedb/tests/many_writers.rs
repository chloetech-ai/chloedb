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
    let narrowed = p("UPDATE docs SET n = 0 WHERE id = 'a' AND n > 1");
    assert!(db.write_prepared_group(&[&a, &narrowed]).is_err());
    assert_eq!(int(&db, "SELECT n FROM docs WHERE id = 'a'"), 2);
    // One that fails fails them all.
    assert!(db.write_prepared_group(&[&a, &c]).is_err());
    assert_eq!(int(&db, "SELECT n FROM docs WHERE id = 'a'"), 2);
    // UPDATEs by key alone of one key share one: each reads the row as
    // the one before left it, so a hot counter takes many + 1s a commit.
    let price = p("UPDATE docs SET n = 10 WHERE id = 'a'");
    assert_eq!(db.write_prepared_group(&[&a, &a, &price, &a]).unwrap().0, [1, 1, 1, 1]);
    assert_eq!(int(&db, "SELECT n FROM docs WHERE id = 'a'"), 11);
    // But not after an insert or delete of the key in the same group.
    let ins = p("INSERT INTO docs (id, n) VALUES ('q', 1)");
    let q = p("UPDATE docs SET n = n + 1 WHERE id = 'q'");
    assert_eq!(admit(&["INSERT INTO docs (id, n) VALUES ('q', 1)", "UPDATE docs SET n = n + 1 WHERE id = 'q'"]), [true, false]);
    assert_eq!(admit(&["UPDATE docs SET n = n + 1 WHERE id = 'q'", "DELETE FROM docs WHERE id = 'q'"]), [true, false]);
    assert!(db.write_prepared_group(&[&ins, &q]).is_err());
}

/// UPDATE ... WHERE key = :k, which reads the row by its key alone
/// (no scan, no plan of the filter), answers as the general path does: a
/// missing key writes nothing, NULL + 1 is 1, an expired row is not
/// there, a key of the wrong type goes the general way, and a sum that
/// overflows fails.
#[test]
fn updates_by_key_alone() {
    let dir = tempfile::tempdir().unwrap();
    let db = create(dir.path(), DbOptions::default());
    sql(&db, "INSERT INTO docs (id, body, n) VALUES ('a', 'x', NULL), ('b', 'y', 5)", &[]).unwrap();
    let k = |k: &str| vec![("k", Value::Str(k.into()))];
    assert_eq!(sql(&db, "UPDATE docs SET n = n + 1 WHERE id = :k", &k("a")).unwrap(), 1);
    assert_eq!(int(&db, "SELECT n FROM docs WHERE id = 'a'"), 1);
    assert_eq!(sql(&db, "UPDATE docs SET n = n - 2 WHERE id = :k", &k("b")).unwrap(), 1);
    assert_eq!(int(&db, "SELECT n FROM docs WHERE id = 'b'"), 3);
    assert_eq!(sql(&db, "UPDATE docs SET n = n + 1 WHERE id = :k", &k("missing")).unwrap(), 0);
    assert_eq!(int(&db, "SELECT count(*) FROM docs"), 2);
    // Set and add in one statement, and the same text again with other
    // parameters (it is parsed once).
    for (i, key) in ["a", "b", "a"].iter().enumerate() {
        let p = [("k", Value::Str(key.to_string())), ("v", Value::Str(format!("body {i}")))];
        assert_eq!(sql(&db, "UPDATE docs SET body = :v, n = n + 10 WHERE id = :k", &p).unwrap(), 1);
    }
    assert_eq!(int(&db, "SELECT n FROM docs WHERE id = 'a'"), 21);
    assert_eq!(int(&db, "SELECT n FROM docs WHERE id = 'b'"), 13);
    let body = db.query("SELECT body FROM docs WHERE id = 'a'", &Params::new()).unwrap().rows[0][0].clone();
    assert_eq!(body, Value::Str("body 2".into()));
    // A key of another type: the general path, which refuses it.
    let e = sql(&db, "UPDATE docs SET n = n + 1 WHERE id = 7", &[]).unwrap_err().to_string();
    assert!(e.contains("KEYWORD"), "{e}");
    // An int key.
    sql(&db, "INSERT INTO other (id, n) VALUES (1, 9223372036854775806)", &[]).unwrap();
    assert_eq!(sql(&db, "UPDATE other SET n = n + 1 WHERE id = :k", &[("k", Value::Int(1))]).unwrap(), 1);
    let e = sql(&db, "UPDATE other SET n = n + 1 WHERE id = :k", &[("k", Value::Int(1))]).unwrap_err().to_string();
    assert!(e.contains("overflow"), "{e}");
    assert_eq!(int(&db, "SELECT n FROM other WHERE id = 1"), i64::MAX);
    // A deleted row is not there.
    sql(&db, "DELETE FROM docs WHERE id = 'b'", &[]).unwrap();
    assert_eq!(sql(&db, "UPDATE docs SET n = n + 1 WHERE id = :k", &k("b")).unwrap(), 0);
    assert_eq!(int(&db, "SELECT count(*) FROM docs WHERE id = 'b'"), 0);
}

/// An expired row (`expire_by`) is not updated by key: the fast path
/// finds no row, as a read would.
#[test]
fn an_expired_row_is_not_updated_by_key() {
    let dir = tempfile::tempdir().unwrap();
    let db = create(dir.path(), DbOptions::default());
    db.sql("CREATE TABLE sessions (id KEYWORD PRIMARY KEY, hits INT, until TIMESTAMP) OPTIONS(expire_by = 'until')", &Params::new()).unwrap();
    sql(&db, "INSERT INTO sessions (id, hits, until) VALUES ('old', 1, '2001-01-01T00:00:00Z'), ('new', 1, '2999-01-01T00:00:00Z')", &[]).unwrap();
    assert_eq!(sql(&db, "UPDATE sessions SET hits = hits + 1 WHERE id = 'old'", &[]).unwrap(), 0);
    assert_eq!(sql(&db, "UPDATE sessions SET hits = hits + 1 WHERE id = 'new'", &[]).unwrap(), 1);
    assert_eq!(int(&db, "SELECT hits FROM sessions WHERE id = 'new'"), 2);
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
