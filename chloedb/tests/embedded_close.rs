//! A program that embeds a database: it merges segments in the background
//! as writes add them ([`Db::spawn_merger`]), and [`Db::close`], what the
//! Python, JavaScript and Java packages run when the program closes it,
//! stops the merger, merges small segments and gives the file system back
//! the space merges freed. A table written to for a long time, as a price
//! tracker writes its crawls (rows added, and every listing rewritten
//! whole each crawl), then leaves a file about the size of its rows, the
//! database opens again at once, and nothing written is lost.

use chloedb::db::{Db, SqlResult};
use chloedb::query::Params;
use chloedb::schema::{Field, FieldType, Schema};
use chloedb::storage::{DbOptions, Layout, MergerOptions};
use chloedb::value::Value;

fn opts(layout: Layout) -> DbOptions {
    DbOptions { memtable_max_docs: 600, sync: false, layout, ..DbOptions::default() }
}

fn sql(db: &Db, s: &str) -> Vec<Vec<Value>> {
    match db.sql(s, &Params::new()).unwrap_or_else(|e| panic!("{}: {e}", &s[..s.len().min(80)])) {
        SqlResult::Rows(r) => r.rows,
        SqlResult::Written(_) => Vec::new(),
    }
}

fn create(path: &std::path::Path, layout: Layout) -> Db {
    let seed = Schema::new("seed", vec![Field::new("id", FieldType::Keyword)], Some("id")).unwrap();
    let db = Db::create(path, seed, opts(layout)).unwrap();
    sql(&db, "CREATE TABLE listings (id KEYWORD PRIMARY KEY, title TEXT, seller KEYWORD, last_seen TIMESTAMP, last_total FLOAT)");
    sql(&db, "CREATE TABLE snapshots (item KEYWORD NOT NULL, ts TIMESTAMP NOT NULL, total FLOAT) OPTIONS(cluster_by = 'item')");
    db
}

const ITEMS: usize = 300;

/// Crawl `c`: a snapshot of every item, and every listing rewritten.
fn crawl(db: &Db, c: usize) {
    let ts = chloedb::timestamp::format(1_788_220_800_000_000 + c as i64 * 4 * 3_600_000_000);
    let snaps: Vec<String> = (0..ITEMS).map(|i| format!("('v1|{i}', '{ts}', {}.5)", 10 + (i * 31 + c * 7) % 500)).collect();
    sql(db, &format!("INSERT INTO snapshots VALUES {}", snaps.join(", ")));
    let lists: Vec<String> =
        (0..ITEMS).map(|i| format!("('v1|{i}', 'vintage camera lens {i} boxed and tested, works great', 'seller{}', '{ts}', {}.5)", i % 40, 10 + (i * 31 + c * 7) % 500)).collect();
    sql(db, &format!("INSERT OR REPLACE INTO listings VALUES {}", lists.join(", ")));
}

fn check(db: &Db, crawls: usize, at: &str) {
    assert_eq!(sql(db, "SELECT COUNT(*) FROM snapshots"), vec![vec![Value::Int((crawls * ITEMS) as i64)]], "{at}");
    assert_eq!(sql(db, "SELECT COUNT(*) FROM listings"), vec![vec![Value::Int(ITEMS as i64)]], "{at}");
    let last = crawls - 1;
    assert_eq!(sql(db, "SELECT last_total FROM listings WHERE id = 'v1|7'"), vec![vec![Value::Float((10 + (7 * 31 + last * 7) % 500) as f64 + 0.5)]], "{at}");
}

#[test]
fn a_program_that_writes_for_a_while_and_closes_leaves_a_file_near_its_rows() {
    for layout in [Layout::SingleFile, Layout::Directory] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.strata");
        let crawls = 80;
        let db = create(&path, layout);
        db.spawn_merger(MergerOptions { threads: 1, ..MergerOptions::default() });
        for c in 0..crawls {
            crawl(&db, c);
        }
        db.close().unwrap();
        let st = db.storage().file_stats();
        if layout == Layout::SingleFile {
            assert!((st.free_pages + st.pending_pages) * 8 < st.page_count, "{layout:?}: {} of {} pages free", st.free_pages + st.pending_pages, st.page_count);
        }
        // The rows each crawl replaced are gone, or few.
        let deleted: u64 = st.segments.iter().map(|s| s.n_deleted as u64).sum();
        let docs: u64 = st.segments.iter().map(|s| s.n_docs as u64).sum();
        assert!(deleted * 4 < docs, "{layout:?}: {deleted} of {docs} rows deleted");
        let closed = st.file_bytes;
        // The database still answers and takes writes; a second close has
        // little to give back.
        check(&db, crawls, "closed");
        crawl(&db, crawls);
        db.close().unwrap();
        check(&db, crawls + 1, "closed twice");
        drop(db);

        // It opens again at once, and an optimize, which rewrites every
        // table into one segment, finds little more to drop.
        let db = Db::open(&path, opts(layout)).unwrap();
        check(&db, crawls + 1, "reopened");
        db.optimize().unwrap();
        let optimized = db.storage().file_stats().file_bytes;
        assert!(closed < optimized * 3 / 2, "{layout:?}: closed {closed} bytes, optimized {optimized}");
        check(&db, crawls + 1, "optimized");
    }
}

#[test]
fn close_waits_for_the_merge_the_merger_runs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.strata");
    let db = create(&path, Layout::SingleFile);
    // Eight segments: one merge of them all.
    for c in 0..8 {
        for k in 0..10 {
            crawl(&db, c * 10 + k);
        }
        db.storage().flush().unwrap();
    }
    drop(db);
    let db = Db::open(&path, opts(Layout::SingleFile)).unwrap();
    // A merger that writes slowly is merging them when the database is
    // closed; the database opens again at once.
    db.spawn_merger(MergerOptions { threads: 1, write_bytes_per_sec: Some(1 << 20), ..MergerOptions::default() });
    let t = std::time::Instant::now();
    while db.storage().merge_status().running == 0 {
        assert!(t.elapsed().as_secs() < 10, "the merger merges nothing");
        std::thread::yield_now();
    }
    db.close().unwrap();
    assert_eq!(db.storage().merge_status().running, 0);
    check(&db, 80, "closed");
    drop(db);
    let db = Db::open(&path, opts(Layout::SingleFile)).unwrap();
    check(&db, 80, "reopened");
}
