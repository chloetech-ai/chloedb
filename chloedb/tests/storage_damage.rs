//! Damage that opening used to take for a crash and recover from by
//! dropping acknowledged writes, without an error:
//!
//! * a newest version (its meta slot, its manifest, or the file one byte
//!   short of it) that fails a check, where opening fell back to the
//!   version before and replayed a log that no longer held what the
//!   newer one had flushed (and cut the newer one's pages off the file);
//! * a write-ahead log whose header is damaged, or names another
//!   database, which opening replaced with an empty log.
//!
//! Each must now fail to open, naming the file and the way out, and leave
//! the files as they were; [`DbOptions::recover`] opens all the same,
//! keeping a copy first. A fallback the log does cover still opens.

mod storage_util;

use std::collections::BTreeSet;
use std::fs::OpenOptions;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use storage_util::*;
use chloedb::error::Error;
use chloedb::storage::pager::{read_metas, PAGE};
use chloedb::storage::{Database, DbOptions};

fn keys(r: impl Iterator<Item = u64>) -> BTreeSet<String> {
    r.map(key).collect()
}

fn insert(db: &Database, src: &MapSource, ids: impl Iterator<Item = u64>) {
    let mut tx = db.begin().unwrap();
    for i in ids {
        let d = doc(i, 0);
        src.put(&d);
        tx.insert(d).unwrap();
    }
    tx.commit().unwrap();
}

fn wal_of(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push("-wal");
    s.into()
}

fn flip(path: &Path, at: u64) {
    let f = OpenOptions::new().read(true).write(true).open(path).unwrap();
    let mut b = [0u8];
    f.read_exact_at(&mut b, at).unwrap();
    f.write_all_at(&[b[0] ^ 0xFF], at).unwrap();
}

fn sync_opts(src: &Arc<MapSource>) -> DbOptions {
    DbOptions { sync: true, ..opts(src) }
}

/// Rows 0..300 flushed, rows 300..600 flushed (the newest version), and
/// ten acknowledged transactions of one row each (600..610) in the log.
fn two_flushes_and_a_log(path: &Path, src: &Arc<MapSource>) {
    let db = Database::create(path, schema(), sync_opts(src)).unwrap();
    insert(&db, src, 0..300);
    db.flush().unwrap();
    insert(&db, src, 300..600);
    db.flush().unwrap();
    for i in 600..610 {
        insert(&db, src, i..i + 1);
    }
}

/// The slot of the newest version, and its manifest's page.
fn newest(path: &Path) -> (usize, u64) {
    let f = std::fs::File::open(path).unwrap();
    let (m, slot) = read_metas(&f).unwrap()[0];
    (slot, m.manifest.page)
}

fn files_of(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_file())
        .map(|p| (p.file_name().unwrap().to_string_lossy().into_owned(), std::fs::read(&p).unwrap()))
        .collect();
    v.sort();
    v
}

fn refused(path: &Path, o: DbOptions, what: &str) -> String {
    match Database::open(path, o) {
        Err(Error::Corrupt(m)) => m,
        Err(e) => panic!("{what}: {e}"),
        Ok(db) => panic!("{what}: opened with {} rows, damaged: {:?}", db.snapshot().n_live_docs(), db.damaged()),
    }
}

// One flipped byte in the newest meta slot, in the newest manifest, or a
// file one byte short: opening fell back to the version before, replayed
// the log's last ten rows on it, and lost the 300 rows the newest version
// had flushed (PR-1, ST-7). Read-only too.
#[test]
fn damage_to_the_newest_version_fails_to_open() {
    let cases: [(&str, fn(&Path)); 5] = [
        ("newest meta slot", |p| flip(p, newest(p).0 as u64 * PAGE)),
        // Slot 0, which the database was created with: zeros there are
        // no unwritten slot (the audit's last case of silent loss).
        ("newest meta slot zeroed", |p| {
            assert_eq!(newest(p).0, 0, "two flushes after creation leave the newest version in slot 0");
            let f = OpenOptions::new().read(true).write(true).open(p).unwrap();
            f.write_all_at(&vec![0u8; PAGE as usize], 0).unwrap();
        }),
        ("newest meta slot's txn", |p| flip(p, newest(p).0 as u64 * PAGE + 24)),
        ("newest manifest", |p| flip(p, newest(p).1 * PAGE + 3)),
        ("file one byte short of its page count", |p| {
            let f = OpenOptions::new().read(true).write(true).open(p).unwrap();
            let pages = read_metas(&f).unwrap()[0].0.page_count;
            f.set_len(pages * PAGE - 1).unwrap();
        }),
    ];
    for (what, damage) in cases {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db.strata");
        let src = Arc::new(MapSource::default());
        two_flushes_and_a_log(&path, &src);
        damage(&path);
        let before = files_of(dir.path());
        let m = refused(&path, sync_opts(&src), what);
        assert!(m.contains("would lose acknowledged writes") && m.contains("recover") && m.contains(&path.display().to_string()), "{what}: {m}");
        let ro = DbOptions { read_only: true, ..sync_opts(&src) };
        if what.starts_with("file") {
            // A reader does not check the length: it reads the newest
            // version, whose pages are all there but the last byte.
            assert_eq!(Database::open(&path, ro).unwrap().snapshot().n_live_docs(), 610);
        } else {
            let m = refused(&path, ro, what);
            assert!(m.contains("would lose acknowledged writes"), "{what}: {m}");
        }
        // Nothing was truncated, extended or written.
        assert!(files_of(dir.path()) == before, "{what}: opening changed the files");

        // Recovering accepts the loss, and keeps the file as it was.
        let db = Database::open(&path, DbOptions { recover: true, ..sync_opts(&src) }).unwrap();
        let why = db.damaged().expect("says what was lost").to_string();
        assert!(why.contains("losing acknowledged writes") && why.contains(".damaged-"), "{what}: {why}");
        assert_keys(&db.snapshot(), &keys((0..300).chain(600..610)));
        db.check().unwrap();
        let copies: Vec<_> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).filter(|n| n.starts_with("db.strata.damaged-")).collect();
        assert_eq!(copies.len(), 1, "{what}: {copies:?}");
        assert!(before.iter().any(|(n, b)| n == "db.strata" && *b == std::fs::read(dir.path().join(&copies[0])).unwrap()), "{what}");
    }
}

// The same damage, when the log still holds what the newest version
// flushed (a crash between the version's commit and the log's reset leaves
// that): falling back loses nothing, and opens.
#[test]
fn a_fallback_the_log_covers_opens() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.strata");
    let src = Arc::new(MapSource::default());
    let saved = dir.path().join("wal-before");
    {
        let db = Database::create(&path, schema(), sync_opts(&src)).unwrap();
        insert(&db, &src, 0..300);
        db.flush().unwrap();
        for i in 300..310 {
            insert(&db, &src, i..i + 1);
        }
        std::fs::copy(wal_of(&path), &saved).unwrap();
        db.flush().unwrap();
    }
    std::fs::copy(&saved, wal_of(&path)).unwrap();
    flip(&path, newest(&path).0 as u64 * PAGE);
    let db = Database::open(&path, sync_opts(&src)).unwrap();
    let why = db.damaged().expect("says it fell back").to_string();
    assert!(why.contains("which the write-ahead log completes"), "{why}");
    assert_keys(&db.snapshot(), &keys(0..310));
    db.check().unwrap();
}

// Damage to the older slot loses nothing: the newest version opens.
#[test]
fn damage_to_the_older_slot_opens() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.strata");
    let src = Arc::new(MapSource::default());
    two_flushes_and_a_log(&path, &src);
    let older = 1 - newest(&path).0 as u64;
    flip(&path, older * PAGE);
    let db = Database::open(&path, sync_opts(&src)).unwrap();
    assert_keys(&db.snapshot(), &keys(0..610));
}

/// Five acknowledged transactions of 100 rows, none flushed: all in the
/// log.
fn rows_in_the_log(path: &Path, src: &Arc<MapSource>) {
    drop(Database::create(path, schema(), sync_opts(src)).unwrap());
    rows_in_the_log_of(path, src);
}

fn rows_in_the_log_of(path: &Path, src: &Arc<MapSource>) {
    let db = Database::open(path, sync_opts(src)).unwrap();
    for k in 0..5u64 {
        insert(&db, src, k * 100..k * 100 + 100);
    }
}

// A log whose header was damaged (its magic, its database id) was taken
// for no log and replaced by an empty one, dropping its 500 rows (ST-8,
// PR-3); so was another database's log put in its place.
#[test]
fn a_log_with_a_damaged_header_fails_to_open() {
    let other = tempfile::tempdir().unwrap();
    let other_db = other.path().join("o.strata");
    let osrc = Arc::new(MapSource::default());
    rows_in_the_log(&other_db, &osrc);
    let cases: [(&str, Box<dyn Fn(&Path)>); 3] = [
        ("magic", Box::new(|w: &Path| flip(w, 0))),
        ("database id", Box::new(|w: &Path| flip(w, 9))),
        ("another database's log", Box::new(|w: &Path| {
            std::fs::copy(wal_of(&other_db), w).unwrap();
        })),
    ];
    for (what, damage) in cases {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db.strata");
        let src = Arc::new(MapSource::default());
        // A version of its own first, so that the database is no new one
        // a crash left beside an old log; then 500 rows in the log.
        {
            let db = Database::create(&path, schema(), sync_opts(&src)).unwrap();
            insert(&db, &src, 1000..1001);
            db.flush().unwrap();
        }
        rows_in_the_log_of(&path, &src);
        let wal = wal_of(&path);
        let rows = Database::open(&path, sync_opts(&src)).unwrap().snapshot().n_live_docs();
        assert_eq!(rows, 501, "{what}");
        damage(&wal);
        let before = files_of(dir.path());
        let m = refused(&path, sync_opts(&src), what);
        assert!(m.contains("committed transactions") && m.contains("recover") && m.contains(&wal.display().to_string()), "{what}: {m}");
        assert!(files_of(dir.path()) == before, "{what}: opening changed the files");

        let db = Database::open(&path, DbOptions { recover: true, ..sync_opts(&src) }).unwrap();
        assert!(db.damaged().unwrap().contains(".damaged"), "{what}: {:?}", db.damaged());
        assert!(before.iter().any(|(n, b)| n == "db.strata-wal" && *b == std::fs::read(dir.path().join("db.strata-wal.damaged")).unwrap()), "{what}");
    }
}

// A checkpoint copied each segment as it was and gave the copy a fresh
// checksum, so a damaged source became a backup that passed every check
// (ST-2). It now fails, and leaves no copy.
#[test]
fn a_checkpoint_of_a_damaged_segment_fails() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.strata");
    let src = Arc::new(MapSource::default());
    let db = Database::create(&path, schema(), sync_opts(&src)).unwrap();
    insert(&db, &src, 0..300);
    db.flush().unwrap();
    let f = std::fs::File::open(&path).unwrap();
    let (m, _) = read_metas(&f).unwrap()[0];
    let manifest = chloedb::storage::manifest::Manifest::decode(&chloedb::storage::pager::read_extent(&f, m.manifest, m.manifest_crc).unwrap()).unwrap();
    let seg = manifest.segments().next().unwrap().extent;
    // In the header (read once, at open), and in the middle of the image.
    for at in [seg.page * PAGE + 20, seg.page * PAGE + seg.len / 2] {
        flip(&path, at);
        let dest = dir.path().join("backup.strata");
        match db.checkpoint(&dest, &Default::default()) {
            Err(Error::Corrupt(m)) => assert!(m.contains("fails its checksum") && m.contains("checkpoint"), "{m}"),
            other => panic!("checkpointed a damaged segment: {other:?}"),
        }
        assert!(!dest.exists() && !wal_of(&dest).exists());
        flip(&path, at);
        db.checkpoint(&dest, &Default::default()).unwrap();
        std::fs::remove_file(&dest).unwrap();
    }
}

// After a clean close the rows still in memory were in the log only: a
// copy of the file alone held none of them (ST-4). `Db::close` now
// flushes, and leaves the log empty.
#[test]
fn a_closed_database_is_its_file() {
    use chloedb::db::Db;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.strata");
    let db = Db::create(&path, schema(), DbOptions { sync: true, ..DbOptions::default() }).unwrap();
    for i in 0..500u64 {
        db.storage().begin().map(|mut tx| {
            tx.insert(doc(i, 0)).unwrap();
            tx.commit().unwrap();
        }).unwrap();
    }
    db.close().unwrap();
    drop(db);
    let copy = dir.path().join("copy.strata");
    std::fs::copy(&path, &copy).unwrap();
    let wal_len = std::fs::metadata(wal_of(&path)).unwrap().len();
    assert!(wal_len <= 64, "the log holds {wal_len} bytes after a close");
    let db = Database::open(&copy, DbOptions::default()).unwrap();
    assert_eq!(db.snapshot().n_live_docs(), 500);
}

// A log a crash cut inside its header, or emptied, holds no commit: it
// opens as no log, as before.
#[test]
fn a_log_cut_inside_its_header_opens() {
    for len in [0, 7, 15] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db.strata");
        let src = Arc::new(MapSource::default());
        rows_in_the_log(&path, &src);
        OpenOptions::new().write(true).open(wal_of(&path)).unwrap().set_len(len).unwrap();
        let db = Database::open(&path, sync_opts(&src)).unwrap();
        assert_eq!(db.snapshot().n_live_docs(), 0);
    }
}

fn zero_page(path: &Path, page: u64) {
    let f = OpenOptions::new().read(true).write(true).open(path).unwrap();
    f.write_all_at(&vec![0u8; PAGE as usize], page * PAGE).unwrap();
}

/// Each meta slot's txn, `None` where it holds no valid version.
fn slots(path: &Path) -> [Option<u64>; 2] {
    let f = std::fs::File::open(path).unwrap();
    let mut out = [None; 2];
    for (m, s) in read_metas(&f).unwrap() {
        out[s] = Some(m.txn);
    }
    out
}

/// Refused read-write and read-only, the files left as they were; then
/// opened with `recover`, which says what was lost and keeps `want`.
fn refused_then_recovered(dir: &Path, path: &Path, src: &Arc<MapSource>, what: &str, want: &BTreeSet<String>) {
    let before = files_of(dir);
    let m = refused(path, sync_opts(src), what);
    assert!(m.contains("would lose acknowledged writes") && m.contains("recover") && m.contains(&path.display().to_string()) && m.contains("all zeros"), "{what}: {m}");
    let m = refused(path, DbOptions { read_only: true, ..sync_opts(src) }, what);
    assert!(m.contains("would lose acknowledged writes"), "{what}: {m}");
    assert!(files_of(dir) == before, "{what}: opening changed the files");
    let db = Database::open(path, DbOptions { recover: true, ..sync_opts(src) }).unwrap();
    let why = db.damaged().expect("says what was lost").to_string();
    assert!(why.contains("losing acknowledged writes") && why.contains("all zeros") && why.contains(".damaged-"), "{what}: {why}");
    assert_keys(&db.snapshot(), want);
    db.check().unwrap();
}

// Slot 1 zeroed under the newest version, beside an older one in slot 0:
// it was taken for a slot no commit had written, and opening fell back to
// slot 0 and lost the 300 rows the newest version had flushed. Slot 0 past
// the version the database was created with shows that slot 1 was
// written. With ten transactions in the log after it, and with none (a
// clean close).
#[test]
fn a_zeroed_slot_1_fails_to_open() {
    for log in [true, false] {
        let what = if log { "with a log" } else { "after a clean close" };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db.strata");
        let src = Arc::new(MapSource::default());
        {
            let db = Database::create(&path, schema(), sync_opts(&src)).unwrap();
            for k in 0..3 {
                insert(&db, &src, k * 300..k * 300 + 300);
                db.flush().unwrap();
            }
            if log {
                for i in 900..910 {
                    insert(&db, &src, i..i + 1);
                }
            }
        }
        let [Some(t0), Some(t1)] = slots(&path) else { panic!("{what}: both slots hold a version") };
        assert!(t1 > t0 && t0 > 1, "{what}: three flushes leave the newest version in slot 1, slot 0 past creation ({t0}, {t1})");
        zero_page(&path, 1);
        let want = keys((0..600).chain(if log { 900..910 } else { 0..0 }));
        refused_then_recovered(dir.path(), &path, &src, what, &want);
    }
}

// The database's first version in slot 1, zeroed, with rows logged after
// it: slot 0 still holds the version it was created with, which a
// database before its first commit has too; but the log starts past the
// transactions that came after that version, because the commit emptied
// it. Opening took slot 1 for unwritten, replayed the ten rows and lost
// the 300 the version had flushed.
#[test]
fn a_zeroed_first_version_fails_to_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.strata");
    let src = Arc::new(MapSource::default());
    {
        let db = Database::create(&path, schema(), sync_opts(&src)).unwrap();
        insert(&db, &src, 0..300);
        db.flush().unwrap();
        for i in 300..310 {
            insert(&db, &src, i..i + 1);
        }
    }
    assert!(matches!(slots(&path), [Some(1), Some(_)]));
    zero_page(&path, 1);
    refused_then_recovered(dir.path(), &path, &src, "first version zeroed", &keys(300..310));
}

// A checkpoint of one table is the version it was created with and one
// commit, in slot 1, and has no log until it is opened. Slot 1 zeroed
// opened it empty; the pages past slot 0's version, beside no log, show
// that a commit made them.
#[test]
fn a_checkpoint_with_its_version_zeroed_fails_to_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.strata");
    let src = Arc::new(MapSource::default());
    let backup = dir.path().join("backup.strata");
    {
        let db = Database::create(&path, schema(), sync_opts(&src)).unwrap();
        insert(&db, &src, 0..300);
        db.checkpoint(&backup, &Default::default()).unwrap();
    }
    assert!(matches!(slots(&backup), [Some(1), Some(_)]) && !wal_of(&backup).exists());
    zero_page(&backup, 1);
    refused_then_recovered(dir.path(), &backup, &src, "checkpoint", &keys(0..0));
}

// A crash in a database's first commit, after its pages and before its
// meta slot, leaves slot 1 unwritten and the file longer than slot 0
// says: that opens as before, with the log's rows (a flush) or none (a
// change of tables, which is not logged), and nothing is damaged. A first
// version zeroed after a clean close looks the same, and opens the same
// way: an empty log cannot tell the two apart.
#[test]
fn a_crash_in_the_first_commit_opens() {
    for logged in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db.strata");
        let src = Arc::new(MapSource::default());
        {
            let db = Database::create(&path, schema(), sync_opts(&src)).unwrap();
            if logged {
                insert(&db, &src, 0..300);
            }
        }
        // The pages the commit wrote before it crashed.
        let f = OpenOptions::new().read(true).write(true).open(&path).unwrap();
        let end = f.metadata().unwrap().len();
        f.write_all_at(&vec![0x5Au8; 3 * PAGE as usize], end).unwrap();
        drop(f);
        let want = keys(0..if logged { 300 } else { 0 });
        let ro = Database::open(&path, DbOptions { read_only: true, ..sync_opts(&src) }).unwrap();
        assert_keys(&ro.snapshot(), &want);
        assert_eq!(ro.damaged(), None, "logged {logged}");
        drop(ro);
        let db = Database::open(&path, sync_opts(&src)).unwrap();
        assert_keys(&db.snapshot(), &want);
        assert_eq!(db.damaged(), None, "logged {logged}");
        db.check().unwrap();
    }
}
