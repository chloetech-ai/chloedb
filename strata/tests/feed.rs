//! A database's changes read back from its ship log
//! (`Database::changes_from`, `Database::read_changes`): written again in
//! order, they give the rows the database holds, whatever ALTER TABLE did to
//! its tables' columns in between; a cursor started at any number reads
//! each transaction as one that walked there from the first; and a cursor
//! says when the log no longer reaches it, or when its reader's history is
//! not this database's.

mod storage_util;

use std::collections::{BTreeMap, HashMap};
use storage_util::*;
use strata::db::Db;
use strata::query::Params;
use strata::storage::feed::{Change, Changes, Cursor, Read, Start};
use strata::storage::DbOptions;
use strata::{Field, FieldType, Schema, Value};

fn options() -> DbOptions {
    DbOptions { memtable_max_docs: 1_000_000, sync: false, ship_log_bytes: 1 << 30, ..DbOptions::default() }
}

fn run(db: &Db, sql: &str) {
    db.sql(sql, &Params::new()).unwrap_or_else(|e| panic!("{sql}: {e}"));
}

fn cursor(db: &Db, from: u64) -> Cursor {
    match db.storage().changes_from(from, None).unwrap() {
        Start::At(c) => c,
        other => panic!("no cursor at {from}: {other:?}"),
    }
}

/// Every change from `from` on, `max_bytes` at a time.
fn read_all(db: &Db, from: u64, max_bytes: usize) -> Vec<Changes> {
    let mut c = cursor(db, from);
    let mut out = Vec::new();
    loop {
        match db.storage().read_changes(&mut c, max_bytes, u64::MAX).unwrap() {
            Read::Txns(t) if t.is_empty() => return out,
            Read::Txns(t) => out.extend(t),
            Read::Gone { oldest } => panic!("gone, from {oldest}"),
        }
    }
}

/// Rows by table name and key, each by field id: what the changes write.
type Rows = BTreeMap<String, BTreeMap<String, HashMap<u32, Value>>>;

fn key_of(v: &Value) -> String {
    format!("{v:?}")
}

/// Writes `changes` to `rows` as a follower would.
fn replay(rows: &mut Rows, changes: &[Changes]) {
    for t in changes {
        for c in &t.changes {
            match c {
                Change::Upsert { table, doc } => {
                    let pk = table.primary_key.unwrap();
                    let row = table.fields.iter().zip(doc.iter()).map(|(f, v)| (f.id, v.clone())).collect();
                    rows.get_mut(&table.table).unwrap().insert(key_of(&doc[pk]), row);
                }
                Change::Delete { table, key } => {
                    rows.get_mut(&table.table).unwrap().remove(&key_of(key));
                }
                Change::Set { table, key, field, value } => {
                    let r = rows.get_mut(&table.table).unwrap().get_mut(&key_of(key)).expect("a number set in a row written before");
                    r.insert(table.fields[*field].id, value.clone());
                }
                Change::Create { def } => assert!(rows.insert(def.table.clone(), BTreeMap::new()).is_none()),
                // Columns keep their ids; a dropped one's values go.
                Change::Alter { was, def } => {
                    let mut r = rows.remove(&was.table).unwrap();
                    for row in r.values_mut() {
                        row.retain(|id, _| def.fields.iter().any(|f| f.id == *id));
                    }
                    rows.insert(def.table.clone(), r);
                }
                Change::Drop { was } => assert!(rows.remove(&was.table).is_some()),
                Change::Profile { .. } | Change::Source { .. } => {}
            }
        }
    }
}

/// The rows `db` holds, in the same form.
fn held(db: &Db) -> Rows {
    let mut out = Rows::new();
    for s in db.catalog() {
        let rs = db.query(&format!("SELECT * FROM {} LIMIT 100000", s.table), &Params::new()).unwrap();
        let ids: Vec<u32> = rs.columns.iter().map(|c| s.fields[s.field_index(c).unwrap()].id).collect();
        let pk = rs.columns.iter().position(|c| *c == s.fields[s.primary_key.unwrap()].name).unwrap();
        let rows = rs.rows.into_iter().map(|r| (key_of(&r[pk]), ids.iter().copied().zip(r).filter(|(_, v)| *v != Value::Null).collect())).collect();
        out.insert(s.table.clone(), rows);
    }
    out
}

/// `rows` without NULLs, which a row from before a column was added lacks.
fn without_nulls(mut rows: Rows) -> Rows {
    for t in rows.values_mut() {
        for r in t.values_mut() {
            r.retain(|_, v| *v != Value::Null);
        }
    }
    rows
}

fn other() -> Schema {
    Schema::new("u", vec![Field::new("id", FieldType::Int), Field::new("x", FieldType::Float)], Some("id")).unwrap()
}

#[test]
fn changes_written_again_give_the_rows() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::create(&dir.path().join("a.strata"), schema(), options()).unwrap();
    db.create_table(other()).unwrap();
    db.storage().start_seq_no().unwrap();
    db.storage().ensure_sources().unwrap();
    let mut rows = Rows::new();
    rows.insert("t".into(), BTreeMap::new());
    rows.insert("u".into(), BTreeMap::new());

    run(&db, "INSERT INTO t (id, n, body) VALUES ('a', 1, 'one'), ('b', 2, 'two'), ('c', 3, 'three')");
    run(&db, "INSERT INTO u (id, x) VALUES (1, 0.5), (2, 1.5)");
    // An upsert of a row there, a number set in place, a delete, and a
    // stream's offset with them.
    {
        let mut t = db.storage().begin().unwrap();
        t.table("t").unwrap().upsert(vec![Value::Str("a".into()), Value::Int(10), Value::Str("one again".into())]).unwrap();
        t.table("u").unwrap().set(&Value::Int(2), "x", Value::Float(9.5)).unwrap();
        t.table("t").unwrap().delete(&Value::Str("b".into())).unwrap();
        t.set_source("events/0", 41, 0).unwrap();
        t.commit().unwrap();
    }
    run(&db, "ALTER TABLE t ADD COLUMN tag KEYWORD");
    run(&db, "INSERT INTO t (id, n, body, tag) VALUES ('d', 4, 'four', 'new')");
    run(&db, "UPDATE t SET tag = 'set' WHERE id = 'c'");
    run(&db, "ALTER TABLE t RENAME COLUMN body TO text");
    run(&db, "ALTER TABLE t DROP COLUMN n");
    run(&db, "INSERT INTO t (id, text, tag) VALUES ('e', 'five', 'x')");
    db.create_table(Schema::new("w", vec![Field::new("id", FieldType::Int), Field::new("y", FieldType::Int)], Some("id")).unwrap()).unwrap();
    run(&db, "INSERT INTO w (id, y) VALUES (1, 1), (2, 2)");
    run(&db, "DELETE FROM w WHERE id = 1");
    run(&db, "INSERT INTO u (id, x) VALUES (3, 3.5)");
    db.drop_table("w").unwrap();
    run(&db, "UPDATE u SET x = 4.5 WHERE id = 1");
    let last = db.storage().numbering().unwrap().seq;

    for max in [usize::MAX, 1] {
        let all = read_all(&db, 1, max);
        assert_eq!(all.iter().map(|t| t.seq.seq).collect::<Vec<_>>(), (1..=last).collect::<Vec<_>>(), "every number, in order");
        assert!(all.iter().all(|t| t.unknown == 0), "a cursor from the first number knows every table");
        let mut r = rows.clone();
        replay(&mut r, &all);
        assert_eq!(without_nulls(r), held(&db), "read {max} bytes at a time");
        // The upsert is one change: the delete it logs is left out.
        let third = &all[2].changes;
        assert!(matches!(&third[0], Change::Upsert { doc, .. } if doc[1] == Value::Int(10)), "{third:?}");
        assert!(third.iter().any(|c| matches!(c, Change::Source { name, offset: 41 } if name == "events/0")));
        assert_eq!(third.iter().filter(|c| matches!(c, Change::Delete { .. })).count(), 1);
        assert!(third.iter().any(|c| matches!(c, Change::Set { field: 1, value: Value::Float(x), .. } if *x == 9.5)));
    }

    // A cursor started at any number reads each transaction as the one
    // that walked there, but those of the table dropped since, which it
    // counts.
    let walked = read_all(&db, 1, usize::MAX);
    let w_made = walked.iter().position(|t| t.changes.iter().any(|c| matches!(c, Change::Create { def } if def.table == "w"))).unwrap() as u64 + 1;
    for from in 1..=last {
        let got = read_all(&db, from, usize::MAX);
        for (a, b) in got.iter().zip(&walked[from as usize - 1..]) {
            let of_w = |t: &Changes| t.changes.iter().any(|c| matches!(c, Change::Upsert { table, .. } | Change::Delete { table, .. } if table.table == "w") || matches!(c, Change::Drop { was } if was.table == "w"));
            if from > w_made && of_w(b) {
                assert!(a.unknown > 0 && !of_w(a), "from {from}: seq {} reads w's rows without w", a.seq.seq);
                continue;
            }
            assert_eq!(a, b, "from {from}: seq {}", a.seq.seq);
        }
    }
    // Replayed from just after the first ALTER, onto the rows as they were.
    let alter = walked.iter().position(|t| t.changes.iter().any(|c| matches!(c, Change::Alter { .. }))).unwrap();
    let mut r = rows.clone();
    replay(&mut r, &walked[..alter]);
    replay(&mut r, &read_all(&db, alter as u64 + 1, 64));
    assert_eq!(without_nulls(r), held(&db));

    // Read up to a number, and on from it.
    let mut c = cursor(&db, 1);
    let Read::Txns(t) = db.storage().read_changes(&mut c, usize::MAX, 3).unwrap() else { panic!() };
    assert_eq!(t.iter().map(|t| t.seq.seq).collect::<Vec<_>>(), [1, 2, 3]);
    assert_eq!(c.next(), 4);
    assert_eq!(db.storage().read_changes(&mut c, usize::MAX, 3).unwrap(), Read::Txns(Vec::new()));
    let Read::Txns(t) = db.storage().read_changes(&mut c, usize::MAX, u64::MAX).unwrap() else { panic!() };
    assert_eq!(t, walked[3..]);

    // Past the last number there is nothing yet; a reader ahead of the
    // database, or of another history, diverged.
    assert!(read_all(&db, last + 1, usize::MAX).is_empty());
    let n = db.storage().numbering().unwrap();
    assert!(matches!(db.storage().changes_from(last + 1, Some(n.term)).unwrap(), Start::At(_)));
    assert!(matches!(db.storage().changes_from(last + 1, Some(n.term + 1)).unwrap(), Start::Diverged { seq, .. } if seq == last));
    assert!(matches!(db.storage().changes_from(last + 2, None).unwrap(), Start::Diverged { seq, .. } if seq == last));
}

#[test]
fn a_cursor_says_when_the_log_no_longer_reaches_it() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::create(&dir.path().join("a.strata"), schema(), DbOptions { ship_log_bytes: 2_000, ..options() }).unwrap();
    db.storage().start_seq_no().unwrap();
    for i in 0..10 {
        run(&db, &format!("INSERT INTO t (id, n, body) VALUES ('k{i}', {i}, '{}')", "x".repeat(300)));
    }
    let mut c = cursor(&db, 9);
    match db.storage().changes_from(1, None).unwrap() {
        Start::Gone { oldest } => assert!(oldest > 1 && oldest <= 9, "{oldest}"),
        other => panic!("{other:?}"),
    }
    for i in 10..20 {
        run(&db, &format!("INSERT INTO t (id, n, body) VALUES ('k{i}', {i}, '{}')", "x".repeat(300)));
    }
    assert!(matches!(db.storage().read_changes(&mut c, usize::MAX, u64::MAX).unwrap(), Read::Gone { oldest } if oldest > 9));
    // A database that does not number its writes has no changes to read.
    let plain = Db::create(&dir.path().join("b.strata"), schema(), options()).unwrap();
    assert!(plain.storage().changes_from(1, None).is_err());
}
