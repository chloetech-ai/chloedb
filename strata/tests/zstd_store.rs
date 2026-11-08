//! A table whose doc store is compressed with zstd (`OPTIONS(compression
//! = 'zstd')`): its rows read back through the memtable, flushes, merges,
//! ALTER TABLE and reopening, in both layouts, next to a table that keeps
//! LZ4; its segments take fewer bytes; and the database needs feature
//! `zstd` while it has such a table, and only then.

use std::path::Path;

use rand::{Rng, SeedableRng};
use strata::db::{Db, SqlResult};
use strata::query::Params;
use strata::schema::{Compression, Field, FieldType, Schema};
use strata::storage::{DbOptions, Layout, MergePolicy};
use strata::value::Value;

fn run(db: &Db, sql: &str) -> SqlResult {
    db.sql(sql, &Params::new()).unwrap_or_else(|e| panic!("{sql}: {e}"))
}

fn rows(db: &Db, sql: &str) -> Vec<Vec<Value>> {
    let SqlResult::Rows(r) = run(db, sql) else { panic!("{sql}: expected rows") };
    r.rows
}

/// The features the database at `path` needs.
fn required(path: &Path) -> Vec<String> {
    strata::storage::manifest::read(path).unwrap().features()
}

/// Log-like lines: a few templates with numbers in them.
fn line(rng: &mut rand::rngs::StdRng, i: usize) -> String {
    let t = ["Receiving block blk_{} src: /10.250.{}.{}:50010", "PacketResponder {} for block blk_{} terminating", "Verification succeeded for blk_{}", "Deleting block blk_{} file /mnt/hadoop/dfs/data/current/subdir{}/blk_{}"];
    let mut s = t[rng.gen_range(0..t.len())].to_string();
    while let Some(at) = s.find("{}") {
        s.replace_range(at..at + 2, &format!("{}", rng.gen::<u32>() as usize ^ i));
    }
    s
}

/// Writes `n` rows from `from` into tables `logs` and `plain`, the same
/// rows in both, in batches of 1000.
fn add(db: &Db, from: usize, n: usize) {
    let mut rng = rand::rngs::StdRng::seed_from_u64(from as u64);
    for chunk in (from..from + n).collect::<Vec<_>>().chunks(1000) {
        let values: Vec<String> = chunk.iter().map(|&i| format!("({i}, 'svc{}', '{}')", i % 7, line(&mut rng, i).replace('\'', "''"))).collect();
        for t in ["logs", "plain"] {
            run(db, &format!("INSERT INTO {t} (id, service, body) VALUES {}", values.join(", ")));
        }
    }
}

/// Every row of both tables reads the same, by key and by scan, and
/// searches find the same rows.
fn same(db: &Db, n: usize) {
    let all = |t: &str| rows(db, &format!("SELECT id, service, body FROM {t} ORDER BY id"));
    let logs = all("logs");
    assert_eq!(logs.len(), n);
    assert_eq!(logs, all("plain"));
    for id in [0, n / 3, n - 1] {
        let q = |t: &str| rows(db, &format!("SELECT * FROM {t} WHERE id = {id}"));
        assert_eq!(q("logs"), q("plain"));
        assert_eq!(q("logs").len(), 1);
    }
    let found = |t: &str| rows(db, &format!("SELECT id FROM {t} WHERE MATCH(body, 'verification') ORDER BY id"));
    assert!(!found("logs").is_empty());
    assert_eq!(found("logs"), found("plain"));
}

/// Bytes of table `t`'s segments.
fn segment_bytes(db: &Db, t: &str) -> u64 {
    db.storage().file_stats().segments.iter().filter(|s| s.table == t).map(|s| s.bytes).sum()
}

fn through_sql(path: &Path, layout: Layout) {
    let opts = || DbOptions { sync: false, layout, ..DbOptions::default() };
    let first = Schema::new("first", vec![Field::new("id", FieldType::Int)], Some("id")).unwrap();
    let db = Db::create(path, first, opts()).unwrap();
    run(&db, "CREATE TABLE plain (id INT PRIMARY KEY, service KEYWORD, body TEXT)");
    assert_eq!(required(path), ["compact_keys", "field_ids"]);
    run(&db, "CREATE TABLE logs (id INT PRIMARY KEY, service KEYWORD, body TEXT) OPTIONS(compression = 'zstd')");
    assert_eq!(required(path), ["compact_keys", "field_ids", "zstd"]);
    assert_eq!(db.storage().table("logs").unwrap().compression, Compression::Zstd);

    // In the memtable, then in segments, then merged, then with updates
    // and deletes that rewrite rows.
    add(&db, 0, 6000);
    same(&db, 6000);
    db.storage().flush().unwrap();
    same(&db, 6000);
    add(&db, 6000, 6000);
    db.storage().flush().unwrap();
    let stats = db.storage().merge(&MergePolicy { merge_factor: 2, floor_docs: 100_000, ..MergePolicy::default() }).unwrap();
    assert!(stats.docs_merged > 0, "{stats:?}");
    same(&db, 12_000);
    for t in ["logs", "plain"] {
        run(&db, &format!("UPDATE {t} SET body = 'Verification changed' WHERE id = 7"));
        run(&db, &format!("DELETE FROM {t} WHERE id = 11999"));
    }
    // An altered table keeps its compression.
    for t in ["logs", "plain"] {
        run(&db, &format!("ALTER TABLE {t} ADD COLUMN level KEYWORD"));
    }
    assert_eq!(db.storage().table("logs").unwrap().compression, Compression::Zstd);
    assert_eq!(required(path), ["added_fields", "compact_keys", "field_ids", "zstd"]);
    db.optimize().unwrap();
    same(&db, 11_999);
    // The same rows and indexes, the doc store smaller.
    let (zstd, lz4) = (segment_bytes(&db, "logs"), segment_bytes(&db, "plain"));
    assert!(zstd > 0 && zstd < lz4, "zstd {zstd} bytes, lz4 {lz4}");
    drop(db);

    let db = Db::open(path, opts()).unwrap();
    same(&db, 11_999);
    assert_eq!(rows(&db, "SELECT body FROM logs WHERE id = 7"), vec![vec![Value::Str("Verification changed".into())]]);
    run(&db, "DROP TABLE logs");
    assert_eq!(required(path), ["added_fields", "compact_keys", "field_ids"]);
}

#[test]
fn sql_single_file() {
    let dir = tempfile::tempdir().unwrap();
    through_sql(&dir.path().join("a.strata"), Layout::SingleFile);
}

#[test]
fn sql_directory() {
    let dir = tempfile::tempdir().unwrap();
    through_sql(&dir.path().join("a"), Layout::Directory);
}
