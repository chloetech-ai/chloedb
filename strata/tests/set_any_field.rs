//! `set` of a field that is not a number (a keyword, a text, a JSON value)
//! writes the row again with the value, as an UPDATE does: on rows in the
//! memtable and in segments, after values set in place and after an
//! upsert in the same transaction, and the row's other fields, vectors
//! and indexes follow. A field a model embeds is refused, and the key, a
//! multi-valued field and NULL in a NOT NULL field as before.

use std::collections::BTreeMap;

use strata::db::{Db, SqlResult};
use strata::query::Params;
use strata::schema::{Field, FieldType, Schema};
use strata::storage::{DbOptions, Layout, MergePolicy};
use strata::value::Value;

fn opts() -> DbOptions {
    DbOptions { sync: false, layout: Layout::SingleFile, memtable_max_docs: 1_000_000, ..DbOptions::default() }
}

fn sql(db: &Db, s: &str) -> Vec<Vec<Value>> {
    match db.sql(s, &Params::new()).unwrap_or_else(|e| panic!("{s}: {e}")) {
        SqlResult::Rows(r) => r.rows,
        SqlResult::Written(_) => Vec::new(),
    }
}

const TABLE: &str = "CREATE TABLE listings (id KEYWORD PRIMARY KEY, title TEXT, status KEYWORD NOT NULL, seller KEYWORD, \
    note TEXT, attrs JSON, total FLOAT, tags KEYWORD[], emb VECTOR OPTIONS(embed = 'title', model = 'hash:8', index = 'flat'))";

/// What each row should read: status, seller, note, attrs (as text), total.
type Rows = BTreeMap<String, [Value; 5]>;

const COLUMNS: &str = "SELECT id, status, seller, note, attrs, total FROM listings ORDER BY id";

fn check(db: &Db, want: &Rows, at: &str) {
    let got: Vec<Vec<Value>> = sql(db, COLUMNS);
    let want_rows: Vec<Vec<Value>> = want.iter().map(|(k, v)| std::iter::once(Value::Str(k.clone())).chain(v.iter().cloned()).collect()).collect();
    assert_eq!(got.len(), want_rows.len(), "{at}");
    for (g, w) in got.iter().zip(&want_rows) {
        assert_eq!(g[..4], w[..4], "{at}");
        // JSON comes back as its text.
        assert_eq!(format!("{:?}", g[4]).replace(' ', ""), format!("{:?}", w[4]).replace(' ', ""), "{at}");
        assert_eq!(g[5], w[5], "{at}");
    }
    // The indexes follow the values: a status the row no longer has finds
    // nothing, the new one finds it; the title's text and its vectors stay.
    for (k, v) in want {
        let Value::Str(status) = &v[0] else { panic!() };
        let by_status = sql(db, &format!("SELECT id FROM listings WHERE status = '{status}' AND id = '{k}'"));
        assert_eq!(by_status, vec![vec![Value::Str(k.clone())]], "{at}: {k} by status {status}");
    }
    let n = want.len() as i64;
    assert_eq!(sql(db, "SELECT COUNT(*) FROM listings WHERE MATCH(title, 'camera')"), vec![vec![Value::Int(n)]], "{at}");
}

/// Each row's vector.
fn vectors(db: &Db) -> BTreeMap<String, Value> {
    sql(db, "SELECT id, emb FROM listings").into_iter().map(|r| (r[0].as_str().unwrap().to_string(), r[1].clone())).collect()
}

#[test]
fn set_writes_the_row_again_with_a_value_that_is_not_a_number() {
    let dir = tempfile::tempdir().unwrap();
    let seed = Schema::new("seed", vec![Field::new("id", FieldType::Keyword)], Some("id")).unwrap();
    let db = Db::create(&dir.path().join("s.strata"), seed, opts()).unwrap();
    let models = dir.path().join("models");
    std::fs::create_dir(&models).unwrap();
    assert!(db.open_embedders(&models).is_empty());
    sql(&db, TABLE);
    let mut want = Rows::new();
    let insert = |db: &Db, want: &mut Rows, from: usize, to: usize| {
        for i in from..to {
            sql(db, &format!("INSERT INTO listings (id, title, status, seller, attrs, total, tags) VALUES ('i{i}', 'camera lens {i}', 'ACTIVE', 's{}', '{{\"mm\": {i}}}', {i}.5, ARRAY['a', 'b'])", i % 3));
            want.insert(format!("i{i}"), [Value::Str("ACTIVE".into()), Value::Str(format!("s{}", i % 3)), Value::Null, Value::Str(format!("{{\"mm\":{i}}}")), Value::Float(i as f64 + 0.5)]);
        }
    };
    // Rows in two segments, and rows in the memtable.
    insert(&db, &mut want, 0, 10);
    db.storage().flush().unwrap();
    insert(&db, &mut want, 10, 20);
    db.storage().flush().unwrap();
    insert(&db, &mut want, 20, 30);
    check(&db, &want, "written");
    let mut vecs = vectors(&db);
    assert!(vecs.values().all(|v| matches!(v, Value::Vector(_))));

    let s = |x: &str| Value::Str(x.into());
    let mut txn = db.storage().begin().unwrap();
    let mut t = txn.table("listings").unwrap();
    for (k, i) in [("i3", 0), ("i14", 1), ("i25", 2)] {
        // A keyword, a text and a JSON value, each after a number set in
        // place in the same transaction, which the rewrite keeps.
        assert!(t.set(&s(k), "total", Value::Float(1.25)).unwrap());
        assert!(t.set(&s(k), "status", s("SOLD")).unwrap());
        assert!(t.set(&s(k), "note", s("ended early")).unwrap());
        assert!(t.set(&s(k), "attrs", s(&format!("{{\"mm\": {i}, \"boxed\": true}}"))).unwrap());
        let w = want.get_mut(k).unwrap();
        w[0] = s("SOLD");
        w[2] = s("ended early");
        w[3] = s(&format!("{{\"boxed\":true,\"mm\":{i}}}"));
        w[4] = Value::Float(1.25);
    }
    // After an upsert of the row in the same transaction: the upserted row
    // is the one set.
    t.upsert(vec![s("i7"), s("camera lens 7"), s("ACTIVE"), s("s9"), Value::Null, Value::Null, Value::Float(70.0), Value::Null, vecs["i7"].clone()]).unwrap();
    assert!(t.set(&s("i7"), "seller", s("s10")).unwrap());
    want.insert("i7".into(), [s("ACTIVE"), s("s10"), Value::Null, Value::Null, Value::Float(70.0)]);
    // NULL where the field allows it.
    assert!(t.set(&s("i8"), "seller", Value::Null).unwrap());
    want.get_mut("i8").unwrap()[1] = Value::Null;
    // A key no row has, and a row deleted before.
    assert!(!t.set(&s("nope"), "status", s("SOLD")).unwrap());
    assert!(t.delete(&s("i9")).unwrap());
    assert!(!t.set(&s("i9"), "status", s("SOLD")).unwrap());
    want.remove("i9");
    vecs.remove("i9");
    drop(t);
    txn.commit().unwrap();
    check(&db, &want, "set");
    assert_eq!(vectors(&db), vecs, "set");
    // The multi-valued field the rewrite did not touch is as it was.
    assert_eq!(sql(&db, "SELECT tags FROM listings WHERE id = 'i14'"), vec![vec![Value::List(vec![s("a"), s("b")])]]);

    // What is refused, said.
    let mut txn = db.storage().begin().unwrap();
    let mut t = txn.table("listings").unwrap();
    for (field, value, says) in [
        ("title", s("new title"), "emb embeds"),
        ("id", s("i99"), "the key"),
        ("tags", Value::List(vec![s("x")]), "multi-valued"),
        ("status", Value::Null, "NOT NULL"),
    ] {
        let e = t.set(&s("i1"), field, value).unwrap_err().to_string();
        assert!(e.contains(says), "{field}: {e}");
    }
    drop(t);
    drop(txn);

    // Through a flush, a merge and a reopen.
    db.storage().flush().unwrap();
    check(&db, &want, "flushed");
    assert_eq!(vectors(&db), vecs, "flushed");
    db.storage().merge(&MergePolicy::compact()).unwrap();
    check(&db, &want, "merged");
    assert_eq!(vectors(&db), vecs, "merged");
    let path = dir.path().join("s.strata");
    drop(db);
    let db = Db::open(&path, opts()).unwrap();
    assert!(db.open_embedders(&models).is_empty());
    check(&db, &want, "reopened");
    assert_eq!(vectors(&db), vecs, "reopened");
}
