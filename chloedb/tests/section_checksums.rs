//! Each section of a segment has a crc32, checked the first time a query
//! reads the section. Damage that a section's structure checks let through
//! (a value in a column) made queries answer wrongly without a word; now
//! the queries that read the section fail, and the others go on. Images
//! attached from elsewhere are checked whole.

mod storage_util;

use std::fs::OpenOptions;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};

use storage_util::*;
use chloedb::db::Db;
use chloedb::query::Params;
use chloedb::storage::manifest::Manifest;
use chloedb::storage::pager::{read_extent, read_metas, PAGE};
use chloedb::storage::{DbOptions, Layout};
use chloedb::Error;

const DOCS: u64 = 300;

/// The section of column `n`, named by the field's id.
fn col_n() -> String {
    format!("col.{}", schema().key_of("n").unwrap())
}

fn options(layout: Layout) -> DbOptions {
    DbOptions { sync: false, layout, ..DbOptions::default() }
}

/// The file holding the first segment, and the image's offset in it.
fn first_segment(path: &Path) -> (PathBuf, u64) {
    let manifest = if path.is_dir() {
        chloedb::storage::dir::read_current(path).unwrap().1
    } else {
        let f = std::fs::File::open(path).unwrap();
        let (meta, _) = read_metas(&f).unwrap()[0];
        Manifest::decode(&read_extent(&f, meta.manifest, meta.manifest_crc).unwrap()).unwrap()
    };
    let s = manifest.segments().next().unwrap();
    match &s.file {
        Some(f) => (path.join(f), 0),
        None => (path.to_path_buf(), s.extent.page * PAGE),
    }
}

/// Offset and length of section `name` within the image at `at`.
fn section_at(file: &Path, at: u64, name: &str) -> (u64, u64) {
    let f = std::fs::File::open(file).unwrap();
    let mut head = [0u8; 24];
    f.read_exact_at(&mut head, at).unwrap();
    let mut dir = vec![0u8; u64::from_le_bytes(head[16..24].try_into().unwrap()) as usize];
    f.read_exact_at(&mut dir, at + 24).unwrap();
    let dir: Vec<serde_json::Value> = serde_json::from_slice(&dir).unwrap();
    let e = dir.iter().find(|e| e["name"] == name).unwrap_or_else(|| panic!("no section {name}"));
    assert!(e["crc32"].is_u64(), "{e}");
    (e["offset"].as_u64().unwrap(), e["len"].as_u64().unwrap())
}

fn flip(file: &Path, at: u64) {
    let f = OpenOptions::new().read(true).write(true).open(file).unwrap();
    let mut b = [0u8];
    f.read_exact_at(&mut b, at).unwrap();
    f.write_all_at(&[b[0] ^ 0x40], at).unwrap();
}

fn ids(db: &Db, sql: &str) -> chloedb::Result<Vec<String>> {
    let rs = db.query(sql, &Params::new())?;
    let mut ids: Vec<String> = rs.rows.iter().map(|r| r[0].as_str().unwrap().to_string()).collect();
    ids.sort();
    Ok(ids)
}

const TENS: &str = "SELECT id FROM t WHERE n IN (3, 13, 23, 33, 43, 53, 63, 73, 83, 93, 103, 113, 123, 133, 143, 153, 163, 173, 183, 193, 203, 213, 223, 233, 243, 253, 263, 273, 283, 293) LIMIT 1000";

/// A database of `DOCS` documents (`n` = the document's number) in one
/// segment, with a byte of its `col.n` section flipped where the column's
/// structure checks let the damage through: without section checks, `TENS`
/// answers wrongly and without an error.
fn damaged(dir: &Path, layout: Layout) -> PathBuf {
    let path = dir.join(if layout == Layout::Directory { "db.dir" } else { "db.strata" });
    {
        let db = Db::create(&path, schema(), options(layout)).unwrap();
        db.load((0..DOCS).map(|i| doc(i, i as i64))).unwrap();
        assert_eq!(ids(&db, TENS).unwrap(), (0..DOCS).filter(|i| i % 10 == 3).map(key).collect::<Vec<_>>());
    }
    let truth = ids(&Db::open(&path, options(layout)).unwrap(), TENS).unwrap();
    let (file, at) = first_segment(&path);
    let (off, len) = section_at(&file, at, &col_n());
    for i in (len / 2..len).chain(0..len / 2) {
        flip(&file, at + off + i);
        let db = Db::open(&path, DbOptions { check_sections: false, ..options(layout) }).unwrap();
        if ids(&db, TENS).is_ok_and(|got| got != truth) {
            return path;
        }
        flip(&file, at + off + i);
    }
    panic!("no byte of col.n changes the answer unseen");
}

#[test]
fn a_damaged_section_fails_the_queries_that_read_it() {
    for layout in [Layout::SingleFile, Layout::Directory] {
        let dir = tempfile::tempdir().unwrap();
        let path = damaged(dir.path(), layout);
        let db = Db::open(&path, options(layout)).unwrap();
        for _ in 0..2 {
            match ids(&db, TENS) {
                Err(Error::Corrupt(m)) => assert!(m.contains(&format!("section {} fails its checksum", col_n())), "{m}"),
                other => panic!("{layout:?}: {other:?}"),
            }
        }
        // Queries that do not read the column go on.
        assert_eq!(ids(&db, "SELECT id FROM t WHERE id = 'k000042'").unwrap(), [key(42)]);
        assert_eq!(ids(&db, "SELECT id FROM t WHERE MATCH(body, 'document') LIMIT 5").unwrap().len(), 5);
    }
}

// A row's value read from a column whose section fails its checksum came
// back NULL, while a filter on the same column failed: one flipped byte
// in `kw.<k>.post` made `SELECT id, k` answer NULL for every row of the
// segment (PR-2). Reading a value now fails as the filter does.
#[test]
fn a_damaged_keyword_section_fails_the_rows_that_read_it() {
    use chloedb::{Field, FieldType, Schema, Value};
    let schema = Schema::new("t", vec![Field::new("id", FieldType::Keyword), Field::new("n", FieldType::Int), Field::new("k", FieldType::Keyword)], Some("id")).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.strata");
    {
        let db = Db::create(&path, schema.clone(), options(Layout::SingleFile)).unwrap();
        db.load((0..DOCS).map(|i| vec![Value::Str(key(i)), Value::Int(i as i64), Value::Str(format!("k{}", i % 7))])).unwrap();
    }
    let (file, at) = first_segment(&path);
    for part in ["post", "codes", "dict"] {
        let name = format!("kw.{}.{part}", schema.key_of("k").unwrap());
        let (off, len) = section_at(&file, at, &name);
        flip(&file, at + off + len - 1);
        let db = Db::open(&path, options(Layout::SingleFile)).unwrap();
        for sql in ["SELECT id, k FROM t WHERE n < 5", "SELECT k FROM t WHERE id = 'k000003'", "SELECT id, n, k FROM t LIMIT 1000"] {
            match db.query(sql, &Params::new()) {
                Err(Error::Corrupt(m)) => assert!(m.contains(&format!("section {name} fails its checksum")), "{sql}: {m}"),
                Ok(rs) => panic!("{name}: {sql} answered {} rows, e.g. {:?}", rs.rows.len(), rs.rows.first()),
                Err(e) => panic!("{name}: {sql}: {e}"),
            }
        }
        drop(db);
        flip(&file, at + off + len - 1);
    }
    let db = Db::open(&path, options(Layout::SingleFile)).unwrap();
    let rs = db.query("SELECT k FROM t WHERE id = 'k000003'", &Params::new()).unwrap();
    assert_eq!(rs.rows[0][0], Value::Str("k3".into()));
}

#[test]
fn attaching_a_damaged_image_fails() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::create(&dir.path().join("db.strata"), schema(), options(Layout::SingleFile)).unwrap();
    let image = dir.path().join("seg.img");
    Db::write_segment(&schema(), (0..DOCS).map(|i| doc(i, i as i64)).collect(), &Default::default(), &image).unwrap();
    let (off, len) = section_at(&image, 0, &col_n());
    flip(&image, off + len - 1);
    match db.attach_segments(&[image.clone()]) {
        Err(Error::Schema(m)) => assert!(m.contains(&format!("section {} fails its checksum", col_n())) && m.contains("seg.img"), "{m}"),
        other => panic!("{other:?}"),
    }
    flip(&image, off + len - 1);
    db.attach_segments(&[image]).unwrap();
    assert_eq!(ids(&db, "SELECT id FROM t WHERE n = 7").unwrap(), [key(7)]);
}

/// Damage anywhere in a text field's postings, with the sections checked
/// (the default), is an error of the queries that read them, never a
/// panic (audit S-6: the text reader panics on a list it finds corrupt,
/// which section checks keep it from seeing).
#[test]
fn damaged_text_postings_fail_queries_without_a_panic() {
    let post = format!("text.{}.post", schema().key_of("body").unwrap());
    for layout in [Layout::SingleFile, Layout::Directory] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(if layout == Layout::Directory { "db.dir" } else { "db.strata" });
        Db::create(&path, schema(), options(layout)).unwrap().load((0..DOCS).map(|i| doc(i, i as i64))).unwrap();
        let (file, at) = first_segment(&path);
        let (off, len) = section_at(&file, at, &post);
        let mut failed = 0;
        for i in (0..len).step_by((len as usize / 48).max(1)) {
            flip(&file, at + off + i);
            let db = Db::open(&path, options(layout)).unwrap();
            for sql in ["SELECT id FROM t WHERE MATCH(body, 'document') LIMIT 500", "SELECT id FROM t ORDER BY bm25(body, 'lorem 42') DESC LIMIT 5"] {
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ids(&db, sql))) {
                    Ok(Ok(_)) => {}
                    Ok(Err(Error::Corrupt(m))) => {
                        assert!(m.contains(&post), "{m}");
                        failed += 1;
                    }
                    Ok(Err(e)) => panic!("{layout:?} byte {i}: {sql}: {e}"),
                    Err(_) => panic!("{layout:?} byte {i}: {sql} panicked"),
                }
            }
            drop(db);
            flip(&file, at + off + i);
        }
        assert!(failed > 0, "{layout:?}");
    }
}
