//! One writer per database file, whatever name it is opened by. The
//! writer's lock was a file in `<path>-readers/`, found by the path: a
//! second writer that opened the file through a symlink or a hard link, or
//! after the `-readers` directory was deleted, got a lock of its own, both
//! wrote the file, and the first then panicked reading pages the second
//! had rewritten (ST-3). The writer now also holds a lock on the file
//! itself. (In one process here: on Linux the file's lock belongs to the
//! open file description, so a second open in the same process meets it
//! as another process would.)
#![cfg(target_os = "linux")]

mod storage_util;

use std::path::Path;
use std::sync::Arc;

use storage_util::*;
use chloedb::error::Error;
use chloedb::storage::{Database, DbOptions};

fn busy(path: &Path, src: &Arc<MapSource>, what: &str) {
    match Database::open(path, opts(src)) {
        Err(Error::Busy(m)) => assert!(m.contains("open elsewhere"), "{what}: {m}"),
        Err(e) => panic!("{what}: {e}"),
        Ok(_) => panic!("{what}: a second writer opened the database"),
    }
}

fn written(path: &Path, src: &Arc<MapSource>) -> Database {
    let db = Database::create(path, schema(), opts(src)).unwrap();
    let mut tx = db.begin().unwrap();
    for i in 0..100 {
        let d = doc(i, 0);
        src.put(&d);
        tx.insert(d).unwrap();
    }
    tx.commit().unwrap();
    db.flush().unwrap();
    db
}

#[test]
fn a_second_writer_by_another_name_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.strata");
    let src = Arc::new(MapSource::default());
    let db = written(&path, &src);

    busy(&path, &src, "the same path");
    // Readers still read beside the writer.
    let r = Database::open(&path, DbOptions { read_only: true, ..opts(&src) }).unwrap();
    assert_eq!(r.snapshot().n_live_docs(), 100);
    drop(r);
    let link = dir.path().join("link.strata");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    busy(&link, &src, "a symlink");
    let hard = dir.path().join("hard.strata");
    std::fs::hard_link(&path, &hard).unwrap();
    busy(&hard, &src, "a hard link");
    std::fs::remove_dir_all(dir.path().join("a.strata-readers")).unwrap();
    busy(&path, &src, "the -readers directory deleted");

    // Once the writer is gone, any name opens it.
    drop(db);
    let db = Database::open(&link, opts(&src)).unwrap();
    assert_eq!(db.snapshot().n_live_docs(), 100);
    drop(db);
    Database::open(&path, opts(&src)).unwrap();
}
