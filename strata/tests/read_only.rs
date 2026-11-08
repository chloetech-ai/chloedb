//! Read-only opens (`DbOptions::read_only`): other processes read a
//! database while one writes it. A reader sees every commit, flush,
//! background flush, merge, vacuum and table change of the writer, also
//! across a writer that closes, crashes or opens the database again; a
//! snapshot a reader holds keeps its pages while the writer reuses the
//! rest; a reader writes nothing; a second writer, or a writer of a
//! version of Strata from before readers, is kept out. The last tests run
//! the writer in a child process, which is also killed partway.

mod storage_util;

use std::collections::BTreeSet;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use storage_util::*;
use strata::db::{Db, SqlResult};
use strata::query::Params;
use strata::storage::{Database, DbOptions, Layout, MergePolicy, Snapshot, LOCK_WAIT};
use strata::{Error, Field, FieldType, Schema, Value};

fn keys(r: std::ops::Range<u64>) -> BTreeSet<String> {
    r.map(key).collect()
}

fn path_for(dir: &Path, layout: Layout) -> PathBuf {
    dir.join(if layout == Layout::Directory { "db.dir" } else { "db.strata" })
}

fn writer_opts(src: &Arc<MapSource>, layout: Layout) -> DbOptions {
    DbOptions { memtable_max_docs: 1_000_000, memtable_chunk_docs: 64, layout, ..opts(src) }
}

fn reader_opts() -> DbOptions {
    DbOptions { read_only: true, ..DbOptions::default() }
}

fn insert(db: &Database, src: &MapSource, r: std::ops::Range<u64>, v: i64) {
    let mut tx = db.begin().unwrap();
    for i in r {
        let d = doc(i, v);
        src.put(&d);
        tx.upsert(d).unwrap();
    }
    tx.commit().unwrap();
}

fn delete(db: &Database, r: impl IntoIterator<Item = u64>) {
    let mut tx = db.begin().unwrap();
    for i in r {
        tx.delete(&Value::Str(key(i))).unwrap();
    }
    tx.commit().unwrap();
}

#[test]
fn a_reader_follows_commits_flushes_merges_and_vacuums() {
    for layout in [Layout::SingleFile, Layout::Directory] {
        let dir = tempfile::tempdir().unwrap();
        let path = path_for(dir.path(), layout);
        let src = Arc::new(MapSource::default());
        let db = Database::create(&path, schema(), writer_opts(&src, layout)).unwrap();
        insert(&db, &src, 0..100, 0);
        let r = Database::open(&path, reader_opts()).unwrap();
        assert!(r.is_read_only() && !db.is_read_only());
        assert_keys(&r.snapshot(), &keys(0..100));

        // Commits, into the memtable.
        insert(&db, &src, 100..200, 0);
        delete(&db, 0..10);
        let mut want = keys(10..200);
        assert_keys(&r.snapshot(), &want);
        // A flush: a new version and a log started again.
        db.flush().unwrap();
        assert_keys(&r.snapshot(), &want);
        // Commits after it, then a flush, so there are segments to merge.
        for t in 0..4 {
            insert(&db, &src, 200 + t * 50..250 + t * 50, 0);
            db.flush().unwrap();
        }
        want.extend(keys(200..400));
        assert_keys(&r.snapshot(), &want);
        let held = r.snapshot();
        let merged = db.merge(&MergePolicy { merge_factor: 2, floor_docs: 1000, ..MergePolicy::default() }).unwrap();
        assert!(merged.merges > 0, "{merged:?}");
        let s = r.snapshot();
        assert_keys(&s, &want);
        assert!(s.segments().len() < held.segments().len());
        // The snapshot from before reads what it did.
        assert_keys(&held, &want);
        // Upserts of new versions.
        insert(&db, &src, 50..60, 7);
        assert_keys(&r.snapshot(), &want);
        if layout == Layout::SingleFile {
            delete(&db, 300..400);
            db.flush().unwrap();
            db.merge(&MergePolicy { merge_factor: 2, floor_docs: 1000, max_deleted_ratio: 0.1, ..MergePolicy::default() }).unwrap();
            drop(held);
            db.vacuum().unwrap();
            for i in 300..400 {
                want.remove(&key(i));
            }
            assert_keys(&r.snapshot(), &want);
        }
        r.check().unwrap();
        db.check().unwrap();
    }
}

#[test]
fn a_reader_writes_nothing() {
    for layout in [Layout::SingleFile, Layout::Directory] {
        let dir = tempfile::tempdir().unwrap();
        let path = path_for(dir.path(), layout);
        let src = Arc::new(MapSource::default());
        let db = Database::create(&path, schema(), writer_opts(&src, layout)).unwrap();
        insert(&db, &src, 0..10, 0);
        let r = Arc::new(Database::open(&path, reader_opts()).unwrap());
        let read_only = |e: Error| matches!(e, Error::Query(m) if m.contains("read-only"));
        assert!(read_only(r.begin().err().unwrap()));
        assert!(read_only(r.flush().unwrap_err()));
        assert!(read_only(r.background_flush().unwrap_err()));
        assert!(read_only(r.vacuum().unwrap_err()));
        assert!(read_only(r.bulk_load(vec![doc(20, 0)]).unwrap_err()));
        assert!(read_only(r.create_table(Schema::new("u", vec![Field::new("id", FieldType::Keyword)], Some("id")).unwrap()).unwrap_err()));
        assert!(read_only(r.checkpoint(&dir.path().join("copy"), &Default::default()).unwrap_err()));
        let with_source = Database::open(&path, DbOptions { doc_source: Some(src.clone()), ..reader_opts() }).unwrap();
        assert!(read_only(with_source.merge(&MergePolicy::default()).unwrap_err()));
        drop(with_source);
        // Background threads are not started.
        r.spawn_flusher();
        r.spawn_merger(Default::default());
        // Creating is writing.
        assert!(read_only(Database::create(&dir.path().join("new"), schema(), reader_opts()).err().unwrap()));
        assert!(!dir.path().join("new").exists());
        assert_keys(&r.snapshot(), &keys(0..10));
        drop(r);
        // The writer's files are as it left them.
        insert(&db, &src, 10..20, 0);
        db.check().unwrap();
        drop(db);
        let db = Database::open(&path, writer_opts(&src, layout)).unwrap();
        assert_keys(&db.snapshot(), &keys(0..20));
    }
}

#[test]
fn one_writer_and_many_readers() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.strata");
    let src = Arc::new(MapSource::default());
    let db = Database::create(&path, schema(), writer_opts(&src, Layout::SingleFile)).unwrap();
    let readers: Vec<Database> = (0..3).map(|_| Database::open(&path, reader_opts()).unwrap()).collect();
    // A second writer is kept out, with the readers in.
    let t = Instant::now();
    assert!(matches!(Database::open(&path, opts(&src)), Err(Error::Busy(_))));
    assert!(t.elapsed() >= LOCK_WAIT);
    let stats = db.file_stats();
    assert_eq!(stats.readers.len(), 3, "{:?}", stats.readers);
    assert!(stats.readers.iter().all(|r| r.pid == std::process::id()));
    insert(&db, &src, 0..50, 0);
    for r in &readers {
        assert_keys(&r.snapshot(), &keys(0..50));
    }
    drop(readers);
    // A reader that went is no longer counted.
    assert!(db.file_stats().readers.is_empty());
    // Readers without a writer, and a writer that comes after them.
    drop(db);
    let r = Database::open(&path, reader_opts()).unwrap();
    assert_keys(&r.snapshot(), &keys(0..50));
    let db = Database::open(&path, opts(&src)).unwrap();
    insert(&db, &src, 50..60, 0);
    assert_keys(&r.snapshot(), &keys(0..60));
}

// A writer of a version from before readers takes the file exclusively:
// a reader waits for it as an open always did, then fails, and the other
// way round.
#[test]
fn an_exclusive_lock_keeps_readers_out() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.strata");
    let src = Arc::new(MapSource::default());
    drop(Database::create(&path, schema(), opts(&src)).unwrap());
    let f = std::fs::File::open(&path).unwrap();
    // SAFETY: flock on a valid descriptor.
    assert_eq!(unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) }, 0);
    let t = Instant::now();
    match Database::open(&path, reader_opts()) {
        Err(Error::Busy(m)) => assert!(m.contains("does not let other processes read it"), "{m}"),
        other => panic!("{:?}", other.map(|_| ())),
    }
    assert!(t.elapsed() >= LOCK_WAIT);
    drop(f);
    let r = Database::open(&path, reader_opts()).unwrap();
    let f = std::fs::File::open(&path).unwrap();
    // SAFETY: as above.
    assert_ne!(unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) }, 0);
    drop(r);
}

// The writer reuses freed pages; a snapshot a reader holds keeps the
// pages of its version until it goes, and an idle reader's thread lets go
// of the versions it no longer reads.
#[test]
fn a_readers_snapshot_keeps_its_pages() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.strata");
    let src = Arc::new(MapSource::default());
    let db = Database::create(&path, schema(), DbOptions { segment_max_docs: 200, ..writer_opts(&src, Layout::SingleFile) }).unwrap();
    for t in 0..5 {
        insert(&db, &src, t * 200..t * 200 + 200, 0);
        db.flush().unwrap();
    }
    let r = Database::open(&path, reader_opts()).unwrap();
    let old = r.snapshot();
    let crcs: Vec<u32> = old.segments().iter().map(|v| crc32fast::hash(v.file.image())).collect();
    // Rewrite everything, over and over: without the pin, new segments
    // land in the pages the old ones were in.
    for round in 1..6 {
        for t in 0..5 {
            insert(&db, &src, t * 200..t * 200 + 200, round);
            db.flush().unwrap();
        }
        db.merge(&MergePolicy { merge_factor: 2, floor_docs: 10_000, max_deleted_ratio: 0.1, ..MergePolicy::default() }).unwrap();
        db.vacuum().unwrap();
    }
    db.check().unwrap();
    let now: Vec<u32> = old.segments().iter().map(|v| crc32fast::hash(v.file.image())).collect();
    assert_eq!(crcs, now, "a page of the old snapshot was written over");
    assert_keys(&old, &keys(0..1000));
    let pinned = db.file_stats().readers[0].pin.unwrap();
    assert!(pinned <= old.txn(), "{pinned} > {}", old.txn());
    let pending = db.file_stats().pending_pages;
    assert!(pending > 0);
    // The snapshot goes; the reader's thread raises the pin within a
    // second or so, and the writer's next version reuses the pages.
    drop(old);
    let deadline = Instant::now() + Duration::from_secs(10);
    while db.file_stats().readers[0].pin.unwrap() <= pinned {
        assert!(Instant::now() < deadline, "the pin stayed at {pinned}");
        std::thread::sleep(Duration::from_millis(50));
    }
    insert(&db, &src, 0..1, 9);
    db.flush().unwrap();
    insert(&db, &src, 0..1, 10);
    db.flush().unwrap();
    assert!(db.file_stats().pending_pages < pending, "{} >= {pending}", db.file_stats().pending_pages);
    assert_keys(&r.snapshot(), &keys(0..1000));
    r.check().unwrap();
}

#[test]
fn a_reader_follows_background_flushes() {
    for layout in [Layout::SingleFile, Layout::Directory] {
        let dir = tempfile::tempdir().unwrap();
        let path = path_for(dir.path(), layout);
        let src = Arc::new(MapSource::default());
        let db = Arc::new(Database::create(&path, schema(), writer_opts(&src, layout)).unwrap());
        let r = Database::open(&path, reader_opts()).unwrap();
        insert(&db, &src, 0..100, 0);
        // Commits while the flush builds go to a new log, with the sealed
        // one still there.
        db.background_flush_with(|| {
            insert(&db, &src, 100..150, 0);
            assert_keys(&r.snapshot(), &keys(0..150));
        })
        .unwrap();
        assert_keys(&r.snapshot(), &keys(0..150));
        // Under a stream of commits with the flusher thread.
        drop(Arc::try_unwrap(db).ok().expect("one reference"));
        let db2 = Arc::new(Database::open(&path, DbOptions { memtable_max_docs: 100, ..writer_opts(&src, layout) }).unwrap());
        db2.spawn_flusher();
        let mut n = 150;
        while n < 3000 {
            insert(&db2, &src, n..n + 37, 0);
            n += 37;
            let s = r.snapshot();
            // Never a gap: every key committed so far, each once.
            assert_keys(&s, &keys(0..n));
        }
        r.check().unwrap();
    }
}

#[test]
fn a_reader_follows_table_changes() {
    for layout in [Layout::SingleFile, Layout::Directory] {
        let dir = tempfile::tempdir().unwrap();
        let path = path_for(dir.path(), layout);
        let src = Arc::new(MapSource::default());
        let db = Database::create(&path, schema(), writer_opts(&src, layout)).unwrap();
        insert(&db, &src, 0..10, 0);
        let r = Database::open(&path, reader_opts()).unwrap();
        let u = Schema::new("u", vec![Field::new("id", FieldType::Keyword), Field::new("x", FieldType::Int)], Some("id")).unwrap();
        db.create_table(u).unwrap();
        {
            let mut tx = db.begin().unwrap();
            tx.table("u").unwrap().insert(vec![Value::Str("a".into()), Value::Int(1)]).unwrap();
            tx.commit().unwrap();
        }
        // The tables, before any snapshot.
        assert!(r.table("u").is_some());
        let s = r.snapshot();
        assert_eq!(s.schemas().map(|s| s.table.clone()).collect::<Vec<_>>(), vec!["t".to_string(), "u".to_string()]);
        db.drop_tables(&["u"], Default::default()).unwrap();
        assert!(r.table("u").is_none());
        assert_eq!(r.tables().len(), 1);
        assert_keys(&r.snapshot(), &keys(0..10));
    }
}

// The writer closes and opens the database again (its log starts again),
// or another process takes over as the writer: the reader goes on.
#[test]
fn a_reader_outlives_its_writer() {
    for layout in [Layout::SingleFile, Layout::Directory] {
        let dir = tempfile::tempdir().unwrap();
        let path = path_for(dir.path(), layout);
        let src = Arc::new(MapSource::default());
        let mut db = Some(Database::create(&path, schema(), writer_opts(&src, layout)).unwrap());
        insert(db.as_ref().unwrap(), &src, 0..10, 0);
        let r = Database::open(&path, reader_opts()).unwrap();
        for round in 0..5u64 {
            drop(db.take());
            let w = db.insert(Database::open(&path, writer_opts(&src, layout)).unwrap());
            insert(w, &src, 10 + round * 10..20 + round * 10, 0);
            if round % 2 == 1 {
                w.flush().unwrap();
            }
            assert_keys(&r.snapshot(), &keys(0..20 + round * 10));
        }
        drop(db);
        assert_keys(&r.snapshot(), &keys(0..60));
    }
}

// Through SQL ([`Db`]): a reader answers queries over what the writer
// wrote, values written in place included, and refuses writes.
#[test]
fn a_reader_answers_sql() {
    for layout in [Layout::SingleFile, Layout::Directory] {
        let dir = tempfile::tempdir().unwrap();
        let path = path_for(dir.path(), layout);
        let w = Db::create_empty(&path, DbOptions { sync: false, layout, ..DbOptions::default() }).unwrap();
        let sql = |db: &Db, q: &str| db.sql(q, &Params::new());
        sql(&w, "CREATE TABLE items (id KEYWORD PRIMARY KEY, title TEXT, n INT)").unwrap();
        sql(&w, "INSERT INTO items (id, title, n) VALUES ('a', 'red apple', 1), ('b', 'green pear', 2)").unwrap();
        let r = Db::open(&path, reader_opts()).unwrap();
        let rows = |q: &str| match sql(&r, q).unwrap() {
            SqlResult::Rows(r) => r.rows,
            SqlResult::Written(_) => panic!("{q}: expected rows"),
        };
        assert_eq!(rows("SELECT id, n FROM items ORDER BY id"), vec![vec![s("a"), Value::Int(1)], vec![s("b"), Value::Int(2)]]);
        sql(&w, "INSERT INTO items (id, title, n) VALUES ('c', 'red cherry', 3)").unwrap();
        sql(&w, "UPDATE items SET n = 20 WHERE id = 'b'").unwrap();
        assert_eq!(rows("SELECT id FROM items WHERE MATCH(title, 'red') ORDER BY id"), vec![vec![s("a")], vec![s("c")]]);
        assert_eq!(rows("SELECT n FROM items WHERE id = 'b'"), vec![vec![Value::Int(20)]]);
        w.storage().flush().unwrap();
        sql(&w, "DELETE FROM items WHERE id = 'a'").unwrap();
        assert_eq!(rows("SELECT id, n FROM items ORDER BY id"), vec![vec![s("b"), Value::Int(20)], vec![s("c"), Value::Int(3)]]);
        let e = sql(&r, "INSERT INTO items (id, title, n) VALUES ('d', 'x', 4)").unwrap_err();
        assert!(e.to_string().contains("read-only"), "{e}");
        assert!(sql(&r, "CREATE TABLE other (id INT PRIMARY KEY)").unwrap_err().to_string().contains("read-only"));
        // A table the writer creates, queried at once.
        sql(&w, "CREATE TABLE more (id INT PRIMARY KEY, v INT)").unwrap();
        sql(&w, "INSERT INTO more (id, v) VALUES (1, 10)").unwrap();
        assert_eq!(rows("SELECT v FROM more"), vec![vec![Value::Int(10)]]);
    }
}

fn s(v: &str) -> Value {
    Value::Str(v.into())
}

// ---- across processes ----

const CHILD: &str = "STRATA_READ_ONLY_CHILD";

/// The writer: run by `writer_in_another_process` in a child process (the
/// test binary again, with this test alone); does nothing otherwise.
/// Writes keys 0, 1, 2, ... one commit of 10 each, flushing and merging
/// now and then; prints the number written after each commit.
#[test]
fn child_writer() {
    let Ok(path) = std::env::var(CHILD) else { return };
    let path = PathBuf::from(path);
    let total: u64 = std::env::var("STRATA_READ_ONLY_TOTAL").unwrap().parse().unwrap();
    let src = Arc::new(MapSource::default());
    let layout = if path.extension().is_some_and(|e| e == "dir") { Layout::Directory } else { Layout::SingleFile };
    let db = Arc::new(Database::open(&path, DbOptions { memtable_max_docs: 300, segment_max_docs: 200, ..writer_opts(&src, layout) }).unwrap());
    db.spawn_flusher();
    // Documents for merges come back from the memtable source; the
    // source must know the ones written before this process.
    for i in 0..10 {
        src.put(&doc(i, 0));
    }
    let mut n = 10;
    let mut out = std::io::stdout();
    while n < total {
        insert(&db, &src, n..n + 10, 0);
        n += 10;
        if n % 500 == 0 {
            db.merge(&MergePolicy { merge_factor: 3, floor_docs: 10_000, ..MergePolicy::default() }).unwrap();
            if layout == Layout::SingleFile && n % 1500 == 0 {
                retry(|| db.vacuum());
            }
        }
        use std::io::Write;
        writeln!(out, "{n}").unwrap();
        out.flush().unwrap();
    }
}

fn retry<T>(mut f: impl FnMut() -> strata::Result<T>) -> T {
    loop {
        match f() {
            Err(Error::Busy(_)) => std::thread::sleep(Duration::from_millis(1)),
            r => return r.unwrap(),
        }
    }
}

fn spawn_writer(path: &Path, total: u64) -> std::process::Child {
    Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child_writer", "--test-threads=1", "--nocapture", "-q"])
        .env(CHILD, path)
        .env("STRATA_READ_ONLY_TOTAL", total.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap()
}

/// Keys 0..n for some n, each once: a commit is seen whole or not at all,
/// and in order.
fn prefix(s: &Snapshot) -> u64 {
    let got = live_keys(s);
    assert!(got.values().all(|&c| c == 1), "a key is live twice");
    let n = got.len() as u64;
    assert_eq!(got.keys().next_back().cloned(), (n > 0).then(|| key(n - 1)), "a gap in {n} keys");
    assert_eq!(n % 10, 0, "a commit seen in part");
    assert_eq!(s.n_live_docs(), n);
    n
}

#[test]
fn writer_in_another_process() {
    if std::env::var(CHILD).is_ok() {
        return;
    }
    for layout in [Layout::SingleFile, Layout::Directory] {
        let dir = tempfile::tempdir().unwrap();
        let path = path_for(dir.path(), layout);
        let src = Arc::new(MapSource::default());
        let db = Database::create(&path, schema(), writer_opts(&src, layout)).unwrap();
        insert(&db, &src, 0..10, 0);
        drop(db);
        let total = 6000;
        let mut child = spawn_writer(&path, total);
        let r = Arc::new(Database::open(&path, reader_opts()).unwrap());
        // Other threads read too, each seeing the writes in order.
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let others: Vec<_> = (0..3)
            .map(|_| {
                let (r, stop) = (r.clone(), stop.clone());
                std::thread::spawn(move || {
                    let mut last = 0;
                    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                        let n = prefix(&r.snapshot());
                        assert!(n >= last, "went back from {last} to {n}");
                        last = n;
                    }
                })
            })
            .collect();
        let mut last = 0;
        let mut held: Vec<(Snapshot, u64)> = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(120);
        while last < total {
            assert!(Instant::now() < deadline, "the reader saw {last} of {total}");
            let s = r.snapshot();
            let n = prefix(&s);
            assert!(n >= last, "went back from {last} to {n}");
            last = n;
            // Some snapshots are kept a while, and read again later.
            if held.len() < 8 && n % 7 == 0 {
                held.push((s, n));
            }
            std::thread::sleep(Duration::from_millis(1));
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "the writer failed: {status}");
                r.refresh().unwrap();
                last = prefix(&r.snapshot());
                assert_eq!(last, total);
            }
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        for t in others {
            t.join().unwrap();
        }
        for (s, n) in &held {
            assert_eq!(prefix(s), *n);
        }
        assert!(child.wait().unwrap().success());
        r.refresh().unwrap();
        r.check().unwrap();
        // Its files are a database a writer opens.
        drop(held);
        drop(Arc::try_unwrap(r).ok().expect("one reference"));
        let db = Database::open(&path, opts(&src)).unwrap();
        db.check().unwrap();
        assert_eq!(prefix(&db.snapshot()), total);
    }
}

// The writer is killed partway; the reader keeps what it read, and
// follows the next writer, which cuts what a commit left half written.
#[test]
fn a_writer_killed_partway() {
    if std::env::var(CHILD).is_ok() {
        return;
    }
    for layout in [Layout::SingleFile, Layout::Directory] {
        let dir = tempfile::tempdir().unwrap();
        let path = path_for(dir.path(), layout);
        let src = Arc::new(MapSource::default());
        let db = Database::create(&path, schema(), writer_opts(&src, layout)).unwrap();
        insert(&db, &src, 0..10, 0);
        drop(db);
        let r = Database::open(&path, reader_opts()).unwrap();
        let mut child = spawn_writer(&path, 1_000_000);
        // Until it wrote a few thousand.
        let mut out = std::io::BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        loop {
            line.clear();
            assert!(std::io::BufRead::read_line(&mut out, &mut line).unwrap() > 0, "the writer ended");
            if line.trim().parse::<u64>().is_ok_and(|n| n >= 2500) {
                break;
            }
        }
        child.kill().unwrap();
        child.wait().unwrap();
        let before = prefix(&r.snapshot());
        assert!(before >= 10);
        // The next writer: what it opens with is at least what was read
        // (every commit the reader saw was in the log), and the reader
        // follows it.
        let db = Database::open(&path, writer_opts(&src, layout)).unwrap();
        let n = prefix(&db.snapshot());
        assert!(n >= before, "{n} < {before}");
        for i in 0..n {
            src.put(&doc(i, 0));
        }
        assert_eq!(prefix(&r.snapshot()), n);
        insert(&db, &src, n..n + 10, 0);
        db.flush().unwrap();
        assert_eq!(prefix(&r.snapshot()), n + 10);
        db.check().unwrap();
        r.check().unwrap();
    }
}
