//! The key index's layouts (`strata::storage::pk`) end to end: a KEYWORD
//! key read from the key field's keyword column, one kept in the index
//! (`indexed = false`, so there is no column) and an INT key, all with
//! 32-bit fingerprints. Reads by key, upserts and deletes across the
//! memtable and segments large enough for a Bloom filter, a merge, and
//! what each index costs on disk.

use std::collections::BTreeMap;

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use strata::db::Db;
use strata::query::Params;
use strata::storage::{DbOptions, MergePolicy};
use strata::{Field, FieldType, Schema, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Key {
    /// KEYWORD, indexed: the index reads the keys from the column (kind 3).
    Column,
    /// KEYWORD, not indexed: the index keeps the keys (kind 4).
    Own,
    /// INT (kind 5).
    Int,
}

impl Key {
    fn kind(self) -> u64 {
        match self {
            Key::Column => 3,
            Key::Own => 4,
            Key::Int => 5,
        }
    }

    fn schema(self) -> Schema {
        let id = match self {
            Key::Column => Field::new("id", FieldType::Keyword),
            Key::Own => Field { indexed: false, ..Field::new("id", FieldType::Keyword) },
            Key::Int => Field::new("id", FieldType::Int),
        };
        Schema::new("t", vec![id, Field::new("n", FieldType::Int)], Some("id")).unwrap()
    }

    /// Key number `i`: for KEYWORD, short and long ones, multibyte ones,
    /// the empty one, and keys that are prefixes of others.
    fn of(self, i: u64) -> Value {
        match self {
            Key::Int => Value::Int(i as i64 * 7919 - 50_000_000),
            _ => Value::Str(match i % 6 {
                _ if i == 0 => String::new(),
                0 => format!("k{i}"),
                1 => format!("{}{i}", "ü€".repeat(i as usize % 7)),
                2 => format!("{}{i}", "x".repeat(200 + i as usize % 50)),
                3 => format!("{i}"),
                4 => format!("{i}/"),
                _ => format!("item|{}|{i}@20260901T0000", i % 97),
            }),
        }
    }
}

/// Each on-disk segment's key index: its kind and bytes, and the
/// segment's documents.
fn indexes(db: &Db) -> Vec<(u64, u64, usize)> {
    let snap = db.snapshot();
    snap.segments()
        .iter()
        .filter(|s| !s.in_memory)
        .map(|s| {
            let pk = s.file.section("pk").unwrap().expect("a key index");
            let bytes = s.file.section_sizes().into_iter().find(|(n, _)| n == "pk").unwrap().1;
            (u64::from_le_bytes(pk[..8].try_into().unwrap()), bytes as u64, s.file.n_docs())
        })
        .collect()
}

fn check(db: &Db, key: Key, model: &BTreeMap<u64, i64>, upto: u64, what: &str) {
    let all: Vec<Value> = (0..upto).map(|i| key.of(i)).collect();
    let got = db.get_many("t", &all).unwrap();
    for (i, g) in got.iter().enumerate() {
        assert_eq!(g.as_ref().map(|r| r[1].clone()), model.get(&(i as u64)).map(|&n| Value::Int(n)), "{what}: {key:?} {i}");
    }
    for i in (0..upto).step_by(97) {
        let rs = db.query("SELECT n FROM t WHERE id = :k", &Params::new().set("k", key.of(i))).unwrap();
        assert_eq!(rs.rows, model.get(&i).map(|&n| vec![Value::Int(n)]).into_iter().collect::<Vec<_>>(), "{what}: {key:?} {i} by SQL");
    }
    // Keys that are not there: prefixes of keys, a longer one.
    let misses = match key {
        Key::Int => vec![Value::Int(-50_000_001), Value::Int(i64::MAX)],
        _ => vec![Value::Str("k".into()), Value::Str("k60x".into()), Value::Str("item|".into()), Value::Str("x".repeat(200))],
    };
    for m in misses {
        assert_eq!(db.get("t", &m).unwrap(), None, "{what}: {key:?} {m:?}");
    }
}

#[test]
fn every_kind_of_key_index_finds_its_keys() {
    for key in [Key::Column, Key::Own, Key::Int] {
        let dir = tempfile::tempdir().unwrap();
        let opts = DbOptions { memtable_max_docs: 1_000_000, sync: false, ..DbOptions::default() };
        let db = Db::create(&dir.path().join("t.strata"), key.schema(), opts).unwrap();
        let mut model = BTreeMap::new();
        // Three segments of 5,000 rows, each with a Bloom filter.
        for part in 0..3u64 {
            let mut tx = db.storage().begin().unwrap();
            for i in 5_000 * part..5_000 * (part + 1) {
                tx.insert(vec![key.of(i), Value::Int(i as i64)]).unwrap();
                model.insert(i, i as i64);
            }
            tx.commit().unwrap();
            db.storage().flush().unwrap();
        }
        check(&db, key, &model, 15_100, "segments");
        // Upserts, inserts and deletes across them, some left in the memtable.
        let mut rng = StdRng::seed_from_u64(11);
        for step in 0..3_000 {
            let i = rng.gen_range(0..16_000u64);
            let mut tx = db.storage().begin().unwrap();
            if rng.gen_bool(0.2) {
                assert_eq!(tx.delete(&key.of(i)).unwrap(), model.remove(&i).is_some(), "{key:?}: delete {i}");
            } else {
                let n = rng.gen_range(0..1_000_000);
                tx.upsert(vec![key.of(i), Value::Int(n)]).unwrap();
                model.insert(i, n);
            }
            tx.commit().unwrap();
            if step == 2_000 {
                db.storage().flush().unwrap();
            }
        }
        check(&db, key, &model, 16_100, "after writes");
        let segs = indexes(&db);
        assert!(segs.len() == 4 && segs.iter().all(|s| s.0 == key.kind()), "{key:?}: {segs:?}");

        db.storage().merge(&MergePolicy::compact()).unwrap();
        db.storage().flush().unwrap();
        db.storage().merge(&MergePolicy::compact()).unwrap();
        db.storage().check().unwrap();
        check(&db, key, &model, 16_100, "merged");
        let segs = indexes(&db);
        assert!(segs.len() == 1 && segs[0].0 == key.kind() && segs[0].2 == model.len(), "{key:?}: {segs:?}");

        // A (fingerprint, docID) entry is 8 bytes, the directory about one
        // more per row; an INT key adds its 8, a KEYWORD key without a
        // column its bytes and an 8-byte offset; one read from its column,
        // nothing.
        let per_row = segs[0].1 as f64 / segs[0].2 as f64;
        let key_bytes = (0..16_000).filter(|i| model.contains_key(i)).map(|i| key.of(i).as_str().map_or(0, str::len)).sum::<usize>() as f64 / model.len() as f64;
        let most = match key {
            Key::Column => 10.0,
            Key::Own => 18.0 + key_bytes,
            Key::Int => 18.0,
        };
        assert!(per_row < most, "{key:?}: {per_row:.1} bytes a row");
        drop(db);

        let db = Db::open(&dir.path().join("t.strata"), DbOptions { sync: false, ..DbOptions::default() }).unwrap();
        check(&db, key, &model, 16_100, "reopened");
    }
}
