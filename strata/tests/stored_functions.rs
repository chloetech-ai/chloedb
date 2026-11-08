//! Stored functions (`CREATE FUNCTION`, [`strata::query::function`]): a
//! call answers as the query with the call written out, for functions
//! that return a value and functions that return rows; versions rise and
//! never change, a query pins one by name, and a rollback is a new version;
//! the versions are kept in the system table `_functions`, across a
//! reopen, in checkpoints, on a replica and in a copy it installs.

use std::path::Path;

use strata::db::{Db, SqlResult};
use strata::query::exec::ResultSet;
use strata::query::Params;
use strata::storage::{Applied, CheckpointOptions, DbOptions, Layout, LogRead};
use strata::value::Value;

fn sql(db: &Db, text: &str) -> SqlResult {
    db.sql(text, &Params::new()).unwrap_or_else(|e| panic!("{text}: {e}"))
}

fn refused(db: &Db, text: &str, says: &str) {
    let e = db.sql(text, &Params::new()).err().unwrap_or_else(|| panic!("{text} ran")).to_string();
    assert!(e.contains(says), "{text}: {e}");
}

fn query(db: &Db, text: &str, params: &Params) -> ResultSet {
    db.query(text, params).unwrap_or_else(|e| panic!("{text}: {e}"))
}

fn rows(db: &Db, text: &str) -> Vec<Vec<Value>> {
    query(db, text, &Params::new()).rows
}

fn create(path: &Path, opts: DbOptions) -> Db {
    let db = Db::create_empty(path, opts).unwrap();
    sql(&db, "CREATE TABLE memories (id INT PRIMARY KEY, body TEXT, importance FLOAT, kind KEYWORD)");
    let words = ["tokio runtime panics", "tokio channels and the runtime", "serde derive", "the runtime of a query", "kinds of runtime"];
    for i in 0..40 {
        let body = format!("{} {i}", words[i % words.len()]);
        sql(&db, &format!("INSERT INTO memories (id, body, importance, kind) VALUES ({i}, '{body}', {}, '{}')", (i % 7) as f64 / 7.0, ["fact", "rule"][i % 2]));
    }
    db
}

/// The versions `_functions` holds: (name, version, dropped).
fn versions(db: &Db) -> Vec<(String, i64, bool)> {
    db.functions().unwrap().all().map(|f| (f.def.name.clone(), f.version, f.dropped)).collect()
}

const BOOSTED: &str = "CREATE FUNCTION boosted(s FLOAT, importance FLOAT, w FLOAT DEFAULT 0.5) RETURNS FLOAT AS :s * (1 + :w * :importance)";
const RECALL: &str = "CREATE FUNCTION recall(q TEXT, k INT DEFAULT 5) RETURNS TABLE AS SELECT id, body, importance FROM memories WHERE MATCH(body, :q) ORDER BY bm25(body, :q) DESC LIMIT :k";

#[test]
fn a_call_answers_as_written_out() {
    let dir = tempfile::tempdir().unwrap();
    let db = create(&dir.path().join("f.strata"), DbOptions::default());
    sql(&db, BOOSTED);
    sql(&db, RECALL);
    let p = Params::new().set("q", Value::Str("tokio runtime".into()));
    // A function that returns a value, in ORDER BY and in the projection.
    let called = query(&db, "SELECT id, boosted(bm25(body, :q), importance) AS s FROM memories WHERE MATCH(body, :q) ORDER BY boosted(bm25(body, :q), importance) DESC LIMIT 10", &p);
    let written = query(&db, "SELECT id, (bm25(body, :q) * (1 + 0.5 * importance)) AS s FROM memories WHERE MATCH(body, :q) ORDER BY bm25(body, :q) * (1 + 0.5 * importance) DESC LIMIT 10", &p);
    assert_eq!(called.rows.len(), 10);
    assert_eq!(called.rows, written.rows);
    assert_eq!(called.columns, ["id", "s"]);
    // By name, and in WHERE.
    let named = query(&db, "SELECT id FROM memories WHERE boosted(importance, 1, w => 2) > 1.5 ORDER BY id", &p);
    let plain = query(&db, "SELECT id FROM memories WHERE importance * (1 + 2 * 1) > 1.5 ORDER BY id", &p);
    assert!(!named.rows.is_empty());
    assert_eq!(named.rows, plain.rows);

    // A function that returns rows: alone, and joined to a table.
    let body = "SELECT id, body, importance FROM memories WHERE MATCH(body, :q) ORDER BY bm25(body, :q) DESC LIMIT 3";
    assert_eq!(query(&db, "SELECT * FROM recall(:q, k => 3)", &p).rows, query(&db, body, &p).rows);
    assert_eq!(query(&db, "SELECT * FROM recall(:q)", &p).rows.len(), 5);
    let joined = query(&db, "SELECT r.id, m.kind FROM recall(:q, 4) AS r JOIN memories m ON m.id = r.id ORDER BY r.id", &p);
    let by_hand = query(&db, &format!("SELECT r.id, m.kind FROM ({}) AS r JOIN memories m ON m.id = r.id ORDER BY r.id", body.replace("LIMIT 3", "LIMIT 4")), &p);
    assert_eq!(joined.rows.len(), 4);
    assert_eq!(joined.rows, by_hand.rows);
    let counted = query(&db, "SELECT count(*) FROM recall('serde', 50)", &p);
    assert_eq!(counted.rows, vec![vec![Value::Int(8)]]);
    // In WITH, and calling another function.
    let with = query(&db, "WITH r AS (SELECT * FROM recall(:q, 10)) SELECT id FROM r ORDER BY boosted(importance, 1) DESC, id LIMIT 3", &p);
    assert_eq!(with.rows.len(), 3);

    // EXPLAIN names the versions a query calls.
    let SqlResult::Rows(plan) = db.sql("EXPLAIN SELECT id FROM memories ORDER BY boosted(importance, 2) DESC LIMIT 3", &p).unwrap() else { panic!() };
    assert_eq!(plan.rows[0][0], Value::Str("calls stored function boosted@1".into()));

    // Calls that do not fit the function.
    for (q, says) in [
        ("SELECT boosted(importance) FROM memories", "needs importance"),
        ("SELECT boosted(1, 2, 3, 4) FROM memories", "given more"),
        ("SELECT boosted(1, 2, x => 3) FROM memories", "no parameter x"),
        ("SELECT * FROM recall(5)", "is TEXT, and is given 5"),
        ("SELECT * FROM memories WHERE recall('x') > 1", "returns rows"),
        ("SELECT * FROM boosted(1, 2)", "returns a value, not rows"),
    ] {
        let e = db.query(q, &p).err().unwrap_or_else(|| panic!("{q} ran")).to_string();
        assert!(e.contains(says), "{q}: {e}");
    }
}

#[test]
fn versions_rise_and_never_change() {
    let dir = tempfile::tempdir().unwrap();
    let db = create(&dir.path().join("v.strata"), DbOptions::default());
    let one = |q: &str| rows(&db, q)[0][0].clone();
    sql(&db, "CREATE FUNCTION bump(x INT) RETURNS INT AS :x + 1");
    refused(&db, "CREATE FUNCTION bump(x INT) RETURNS INT AS :x + 2", "CREATE OR REPLACE");
    sql(&db, "CREATE OR REPLACE FUNCTION bump(x INT) RETURNS INT AS :x + 2");
    // The same definition again writes nothing.
    sql(&db, "CREATE OR REPLACE FUNCTION bump(x INT) RETURNS INT AS :x+2");
    assert_eq!(versions(&db), [("bump".into(), 1, false), ("bump".into(), 2, false)]);
    assert_eq!(one("SELECT bump(id) FROM memories WHERE id = 1"), Value::Int(3));
    assert_eq!(one("SELECT \"bump@1\"(id) FROM memories WHERE id = 1"), Value::Int(2));
    // A rollback is a new version with the old one's definition.
    sql(&db, "CREATE OR REPLACE FUNCTION bump FROM VERSION 1");
    assert_eq!(one("SELECT bump(id) FROM memories WHERE id = 1"), Value::Int(2));
    assert_eq!(db.functions().unwrap().current("bump").unwrap().version, 3);
    // An old version is deleted; the current one is not.
    refused(&db, "DROP FUNCTION bump VERSION 3", "current version");
    sql(&db, "DROP FUNCTION bump VERSION 2");
    refused(&db, "SELECT \"bump@2\"(id) FROM memories", "no version 2");
    // A drop is a version of its own, so the numbers keep rising.
    sql(&db, "DROP FUNCTION bump");
    refused(&db, "DROP FUNCTION bump", "no function named bump");
    sql(&db, "DROP FUNCTION IF EXISTS bump");
    assert!(db.query("SELECT bump(id) FROM memories", &Params::new()).is_err());
    assert_eq!(one("SELECT \"bump@1\"(id) FROM memories WHERE id = 1"), Value::Int(2), "a pinned version outlives a drop");
    sql(&db, "CREATE FUNCTION bump(x INT) RETURNS INT AS :x * 10");
    assert_eq!(versions(&db), [("bump".into(), 1, false), ("bump".into(), 3, false), ("bump".into(), 4, true), ("bump".into(), 5, false)]);
    assert_eq!(one("SELECT bump(id) FROM memories WHERE id = 2"), Value::Int(20));
    // A new version returns what the old one did.
    refused(&db, "CREATE OR REPLACE FUNCTION bump(x INT) RETURNS TABLE AS SELECT id FROM memories", "returns the same kind");
    // The table holds them, and reads as one.
    let r = rows(&db, "SELECT id, version, language, kind, dropped FROM _functions ORDER BY version");
    assert_eq!(r.len(), 4);
    assert_eq!(r[2], vec![Value::Str("bump@4".into()), Value::Int(4), Value::Str("sql".into()), Value::Str("scalar".into()), Value::Bool(true)]);
}

#[test]
fn definitions_are_checked() {
    let dir = tempfile::tempdir().unwrap();
    let db = create(&dir.path().join("c.strata"), DbOptions::default());
    for (t, says) in [
        ("CREATE FUNCTION lower(s TEXT) RETURNS TEXT AS :s", "built-in"),
        ("CREATE FUNCTION f(s TEXT) RETURNS TEXT AS body", "reads body, which is no parameter"),
        ("CREATE FUNCTION f(s TEXT) RETURNS TEXT AS :t", "not one of its parameters"),
        ("CREATE FUNCTION f(q TEXT) RETURNS TABLE AS SELECT id FROM nowhere WHERE MATCH(body, :q)", "does not plan"),
        ("CREATE FUNCTION f(q TEXT) RETURNS TABLE AS SELECT nothing FROM memories WHERE MATCH(body, :q)", "does not plan"),
        ("CREATE FUNCTION f(x INT) RETURNS INT LANGUAGE javascript AS 'x'", "not supported yet"),
        ("CREATE TABLE _functions (id KEYWORD PRIMARY KEY)", "system table"),
        ("DROP TABLE _functions", "no table"),
    ] {
        refused(&db, t, says);
    }
    assert!(versions(&db).is_empty());
    sql(&db, "CREATE FUNCTION f(q TEXT) RETURNS TABLE AS SELECT id FROM memories WHERE MATCH(body, :q)");
    // Made by the first function: one table, never one of the tables.
    assert_eq!(db.catalog().iter().map(|t| t.table.as_str()).collect::<Vec<_>>(), ["_placeholder", "memories"]);
    refused(&db, "DROP TABLE _functions", "cannot be dropped");
    refused(&db, "INSERT INTO _functions (id) VALUES ('x@1')", "_functions");
}

fn names(db: &Db) -> Vec<(String, i64, String)> {
    db.functions().unwrap().all().map(|f| (f.def.name.clone(), f.version, f.hash.clone())).collect()
}

#[test]
fn functions_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("k.strata");
    let db = create(&path, DbOptions::default());
    sql(&db, BOOSTED);
    sql(&db, RECALL);
    sql(&db, "DROP FUNCTION boosted");
    sql(&db, "CREATE FUNCTION boosted(s FLOAT, importance FLOAT) RETURNS FLOAT AS :s + :importance");
    let want = names(&db);
    assert_eq!(want.len(), 4);
    // A table made after the functions' table is a table still.
    sql(&db, "CREATE TABLE later (id INT PRIMARY KEY)");
    sql(&db, "INSERT INTO later (id) VALUES (1)");
    drop(db);
    let db = Db::open(&path, DbOptions::default()).unwrap();
    assert_eq!(names(&db), want, "a reopen replays them");
    assert_eq!(db.catalog().iter().map(|t| t.table.as_str()).collect::<Vec<_>>(), ["_placeholder", "memories", "later"]);
    db.storage().flush().unwrap();
    drop(db);
    let db = Db::open(&path, DbOptions::default()).unwrap();
    assert_eq!(names(&db), want, "flushed");
    assert_eq!(rows(&db, "SELECT boosted(1, 2) FROM later"), vec![vec![Value::Int(3)]], "the value is the body's, as written out");
    for layout in [Layout::SingleFile, Layout::Directory] {
        let to = dir.path().join(format!("copy-{layout:?}"));
        db.storage().checkpoint(&to, &CheckpointOptions { layout, ..CheckpointOptions::default() }).unwrap();
        let c = Db::open(&to, DbOptions::default()).unwrap();
        assert_eq!(names(&c), want, "{layout:?}");
        sql(&c, "CREATE OR REPLACE FUNCTION boosted(s FLOAT, importance FLOAT) RETURNS FLOAT AS :s - :importance");
        assert_eq!(rows(&c, "SELECT boosted(1, 2) FROM later"), vec![vec![Value::Int(-1)]], "{layout:?}");
        sql(&c, "INSERT INTO later (id) VALUES (2)");
    }
}

fn numbered(path: &Path) -> Db {
    let db = create(path, DbOptions { ship_log_bytes: 1 << 24, sync: false, ..DbOptions::default() });
    db.storage().start_seq_no().unwrap();
    db
}

/// Ships what `replica` lacks of `primary`'s log.
fn ship(primary: &Db, replica: &Db) {
    let at = replica.storage().numbering().unwrap().last();
    let LogRead::Txns(got) = primary.storage().log_from(at.seq + 1, 1 << 24) else { panic!("gone") };
    let bytes: Vec<&[u8]> = got.iter().map(|t| &*t.bytes).collect();
    let end = got.last().map_or(at, |t| t.seq);
    assert_eq!(replica.storage().apply_shipped(at, &bytes).unwrap(), Applied::At(end));
}

#[test]
fn a_replica_receives_functions() {
    let dir = tempfile::tempdir().unwrap();
    let primary = numbered(&dir.path().join("p.strata"));
    let replica = numbered(&dir.path().join("r.strata"));
    sql(&primary, BOOSTED);
    sql(&primary, "INSERT INTO memories (id, body, importance, kind) VALUES (100, 'tokio again', 1.0, 'fact')");
    sql(&primary, RECALL);
    ship(&primary, &replica);
    assert_eq!(names(&replica), names(&primary));
    let p = Params::new().set("q", Value::Str("tokio".into()));
    let q = "SELECT id, boosted(importance, 1) FROM recall(:q, 10) ORDER BY id";
    assert_eq!(query(&replica, q, &p).rows, query(&primary, q, &p).rows);
    sql(&primary, "CREATE OR REPLACE FUNCTION boosted(s FLOAT, importance FLOAT) RETURNS FLOAT AS :s");
    sql(&primary, "DROP FUNCTION recall");
    sql(&primary, "DROP FUNCTION boosted VERSION 1");
    ship(&primary, &replica);
    assert_eq!(names(&replica), names(&primary));
    assert!(replica.query("SELECT * FROM recall('tokio')", &Params::new()).is_err());
    // A replica that installs a copy takes the copy's functions.
    let far = numbered(&dir.path().join("far.strata"));
    sql(&far, "CREATE FUNCTION stale(x INT) RETURNS INT AS :x");
    let to = dir.path().join("copy.strata");
    primary.storage().checkpoint(&to, &CheckpointOptions::default()).unwrap();
    let copy = Db::open(&to, DbOptions::default()).unwrap();
    sql(&copy, "CREATE FUNCTION fresh(x INT) RETURNS INT AS :x * 2");
    far.storage().install_copy(copy.storage()).unwrap();
    assert_eq!(names(&far), names(&copy));
    assert!(far.functions().unwrap().current("stale").is_none());
    drop(far);
    let far = Db::open(&dir.path().join("far.strata"), DbOptions::default()).unwrap();
    assert_eq!(names(&far), names(&copy), "installed durably");
    // And a replica that had no functions' table takes one from a copy.
    let bare = numbered(&dir.path().join("bare.strata"));
    bare.storage().install_copy(copy.storage()).unwrap();
    assert_eq!(names(&bare), names(&copy));
}
