//! Positions in the packed layout (`Schema::packed_positions`): a table
//! made now writes them (sections `text.<field>.ppos`), answers phrases as
//! one with varints does, in fewer bytes, through flushes, merges, updates
//! and reopening; a table that a database written before them holds
//! (where the schema says nothing) keeps varints, altered or not; and the
//! database needs feature `packed_positions` only while a table that
//! writes them has a field with positions.

use std::path::Path;

use strata::db::{Db, SqlResult};
use strata::query::Params;
use strata::schema::{Field, FieldType, Schema};
use strata::storage::{DbOptions, Layout, MergePolicy};
use strata::value::Value;

fn run(db: &Db, sql: &str) -> SqlResult {
    db.sql(sql, &Params::new()).unwrap_or_else(|e| panic!("{sql}: {e}"))
}

fn rows(db: &Db, sql: &str) -> Vec<Vec<Value>> {
    let SqlResult::Rows(r) = run(db, sql) else { panic!("{sql}: expected rows") };
    r.rows
}

fn required(path: &Path) -> Vec<String> {
    strata::storage::manifest::read(path).unwrap().features()
}

/// Lines of a few templates, words at the same places as in logs.
fn line(i: usize) -> String {
    match i % 4 {
        0 => format!("Receiving block blk_{} src: /10.250.{}.{}:50010 dest: /10.250.{}.{}:50010", i * 7919, i % 251, i % 13, i % 17, i % 19),
        1 => format!("PacketResponder {} for block blk_{} terminating", i % 3, i * 7919),
        2 => format!("Verification succeeded for blk_{}", i * 7919),
        _ => format!("Deleting block blk_{} file /mnt/hadoop/dfs/data/current/subdir{}/blk_{} after {} retries of the deleting block", i * 7919, i % 64, i * 7919, i % 5),
    }
}

/// Writes rows `from..from + n` into both tables, in batches.
fn add(db: &Db, from: usize, n: usize) {
    for chunk in (from..from + n).collect::<Vec<_>>().chunks(1000) {
        let values: Vec<String> = chunk.iter().map(|&i| format!("({i}, '{}')", line(i))).collect();
        for t in ["old", "new"] {
            run(db, &format!("INSERT INTO {t} (id, body) VALUES {}", values.join(", ")));
        }
    }
}

/// The section kinds (`pos`, `ppos`) of table `t`'s positions, over its
/// segments, and their bytes.
fn layout(db: &Db, t: &str) -> (Vec<String>, u64) {
    let snap = db.storage().snapshot();
    let mut kinds = Vec::new();
    let mut bytes = 0;
    for s in snap.table(t).unwrap().segments() {
        for (name, len) in s.file.section_sizes() {
            if let Some(kind) = name.strip_prefix("text.").and_then(|n| n.rsplit('.').next()).filter(|k| *k == "pos" || *k == "ppos") {
                kinds.push(kind.to_string());
                bytes += len as u64;
            }
        }
    }
    kinds.sort();
    kinds.dedup();
    (kinds, bytes)
}

/// Both tables answer every phrase, term and position query the same.
fn same(db: &Db, n: usize) {
    for q in [
        "MATCH(body, '\"verification succeeded\"')",
        "MATCH(body, '\"deleting block\"')",
        "MATCH(body, '\"block blk\"')",
        "MATCH(body, '\"packetresponder 1 for block\"')",
        "MATCH(body, '\"src 10 250\"')",
        "MATCH(body, '\"succeeded verification\"')",
        "MATCH(body, 'terminating')",
    ] {
        let found = |t: &str| rows(db, &format!("SELECT id FROM {t} WHERE {q} ORDER BY id"));
        assert_eq!(found("old"), found("new"), "{q}");
    }
    let count = |t: &str, q: &str| rows(db, &format!("SELECT count(*) FROM {t} WHERE {q}"))[0][0].clone();
    assert_eq!(count("new", "MATCH(body, '\"verification succeeded\"')"), Value::Int((0..n).filter(|i| i % 4 == 2).count() as i64));
    assert_eq!(count("new", "MATCH(body, '\"succeeded verification\"')"), Value::Int(0));
    // bm25 reads the same lists.
    let top = |t: &str| rows(db, &format!("SELECT id FROM {t} ORDER BY bm25(body, 'deleting block retries') DESC, id LIMIT 20"));
    assert_eq!(top("old"), top("new"));
}

fn through_sql(path: &Path, layout_: Layout) {
    let opts = || DbOptions { sync: false, layout: layout_, ..DbOptions::default() };
    // A table as one a database from before packed positions holds.
    let old = Schema::new("old", vec![Field::new("id", FieldType::Int), Field::new("body", FieldType::Text { positions: true })], Some("id")).unwrap().with_packed_positions(false);
    let db = Db::create(path, old, opts()).unwrap();
    assert_eq!(required(path), ["compact_keys", "field_ids"]);
    // Without positions, nothing to pack.
    run(&db, "CREATE TABLE bare (id INT PRIMARY KEY, body TEXT OPTIONS(positions = false))");
    assert_eq!(required(path), ["compact_keys", "field_ids"]);
    run(&db, "CREATE TABLE new (id INT PRIMARY KEY, body TEXT)");
    assert_eq!(required(path), ["compact_keys", "field_ids", "packed_positions"]);
    assert!(db.storage().table("new").unwrap().packed_positions && !db.storage().table("old").unwrap().packed_positions);

    add(&db, 0, 6000);
    same(&db, 6000);
    db.storage().flush().unwrap();
    same(&db, 6000);
    add(&db, 6000, 6000);
    db.storage().flush().unwrap();
    let stats = db.storage().merge(&MergePolicy { merge_factor: 2, floor_docs: 100_000, ..MergePolicy::default() }).unwrap();
    assert!(stats.docs_merged > 0, "{stats:?}");
    same(&db, 12_000);
    for t in ["old", "new"] {
        run(&db, &format!("UPDATE {t} SET body = 'Verification succeeded twice: verification succeeded' WHERE id = 1"));
        run(&db, &format!("DELETE FROM {t} WHERE id = 11998"));
    }
    // An altered table keeps its layout.
    run(&db, "ALTER TABLE old ADD COLUMN host KEYWORD");
    run(&db, "ALTER TABLE new ADD COLUMN host KEYWORD");
    assert!(db.storage().table("new").unwrap().packed_positions && !db.storage().table("old").unwrap().packed_positions);
    db.optimize().unwrap();
    same(&db, 12_000);
    let ((old_kinds, old_bytes), (new_kinds, new_bytes)) = (layout(&db, "old"), layout(&db, "new"));
    assert_eq!((old_kinds, new_kinds), (vec!["pos".to_string()], vec!["ppos".to_string()]));
    assert!(new_bytes * 2 < old_bytes, "packed {new_bytes} bytes, varints {old_bytes}");
    drop(db);

    let db = Db::open(path, opts()).unwrap();
    same(&db, 12_000);
    assert!(db.storage().table("new").unwrap().packed_positions && !db.storage().table("old").unwrap().packed_positions);
    assert_eq!(rows(&db, "SELECT id FROM new WHERE MATCH(body, '\"twice verification succeeded\"')"), vec![vec![Value::Int(1)]]);
    run(&db, "DROP TABLE new");
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

/// The schema keeps the layout through its JSON, and one read from a
/// database that wrote no such field (a version from before) keeps varints.
#[test]
fn the_schema_says_packed_and_its_absence_means_varints() {
    let s = Schema::new("t", vec![Field::new("id", FieldType::Int), Field::new("body", FieldType::Text { positions: true })], Some("id")).unwrap();
    assert!(s.packed_positions);
    let text = serde_json::to_string(&s).unwrap();
    assert!(text.contains("\"packed_positions\":true"), "{text}");
    assert_eq!(serde_json::from_str::<Schema>(&text).unwrap(), s);
    let before = text.replace(",\"packed_positions\":true", "");
    let old: Schema = serde_json::from_str(&before).unwrap();
    assert!(!old.packed_positions);
    assert!(!old.features().any(|f| f == "packed_positions") && s.features().any(|f| f == "packed_positions"));
    assert!(old.same_columns(&s), "the layout is the database's own, not the table's columns");
}
