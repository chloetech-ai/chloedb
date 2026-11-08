//! Cold segments ([`strata::storage::cold`]): segments made cold answer
//! every query as they did, through a cache that survives reopening and
//! that is fetched again when wiped; writes reach their rows; eviction
//! leaves earlier snapshots reading; dropped segments' objects go, and so
//! do the leftovers of a crash; a store that fails or holds damaged bytes
//! gives errors, never other rows; a checkpoint holds them as local files.
//! The store is a directory ([`DirStore`]), wrapped where a test makes it
//! fail.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use strata::db::{Db, SqlResult};
use strata::query::Params;
use strata::storage::{CheckpointOptions, ColdOptions, DbOptions, DirStore, Layout, ObjectStore};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use strata::{Result, Value};

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("strata-cold-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn cold(store: Arc<dyn ObjectStore>) -> ColdOptions {
    ColdOptions { min_segment_bytes: 0, ..ColdOptions::new(store) }
}

fn opts(c: ColdOptions) -> DbOptions {
    DbOptions { layout: Layout::Directory, sync: false, memtable_max_docs: 1_000_000, cold: Some(c), ..DbOptions::default() }
}

fn sql(db: &Db, s: &str) -> Result<Vec<Vec<Value>>> {
    Ok(match db.sql(s, &Params::new())? {
        SqlResult::Rows(r) => r.rows,
        SqlResult::Written(_) => Vec::new(),
    })
}

fn rows(db: &Db, s: &str) -> Vec<Vec<Value>> {
    sql(db, s).unwrap_or_else(|e| panic!("{s}: {e}"))
}

const HOSTS: [&str; 4] = ["alpha", "beta", "gamma", "delta"];
const WORDS: [&str; 8] = ["connection", "refused", "timeout", "disk", "full", "retry", "accepted", "closed"];

/// Rows a segment of [`fill`]'s.
const N: i64 = 12_000;

/// A table of log lines in three segments of [`N`] rows, about 2 MB each
/// (several 256 KiB parts besides their headers and keys), and a table of
/// its own, `other`.
fn fill(db: &Db) {
    db.sql("CREATE TABLE logs (id INT PRIMARY KEY, host KEYWORD, n INT, body TEXT)", &Params::new()).unwrap();
    db.sql("CREATE TABLE other (id INT PRIMARY KEY)", &Params::new()).unwrap();
    db.sql("INSERT INTO other (id) VALUES (1)", &Params::new()).unwrap();
    let mut rng = StdRng::seed_from_u64(7);
    let vocab: Vec<String> = (0..4000).map(|_| (0..rng.gen_range(4..9)).map(|_| rng.gen_range(b'a'..=b'z') as char).collect()).collect();
    for s in 0..3 {
        let mut docs = Vec::new();
        for i in s * N..(s + 1) * N {
            let u = i as usize;
            let noise: Vec<&str> = (0..rng.gen_range(10..30)).map(|_| vocab[rng.gen_range(0..vocab.len())].as_str()).collect();
            let body = format!("{} {} on {} after {} ms {}", WORDS[u % 8], WORDS[(u / 8) % 8], HOSTS[u % 4], (u * 37) % 1000, noise.join(" "));
            docs.push(vec![Value::Int(i), Value::Str(HOSTS[(u / 3) % 4].into()), Value::Int(i % 97), Value::Str(body)]);
        }
        db.load_into("logs", docs).unwrap();
        db.storage().flush().unwrap();
    }
}

const QUERIES: [&str; 7] = [
    "SELECT count(*) FROM logs",
    "SELECT host, count(*), sum(n) FROM logs GROUP BY host ORDER BY host",
    "SELECT id FROM logs WHERE MATCH(body, 'timeout disk') ORDER BY id LIMIT 50",
    "SELECT id, body FROM logs ORDER BY bm25(body, 'refused retry') DESC, id LIMIT 10",
    "SELECT * FROM logs WHERE id = 4321",
    "SELECT id, n FROM logs WHERE host = 'gamma' AND n > 90 ORDER BY id",
    "SELECT max(n), min(id) FROM logs WHERE id >= 24000",
];

fn answers(db: &Db) -> Vec<Vec<Vec<Value>>> {
    QUERIES.iter().map(|q| rows(db, q)).collect()
}

fn segment_ids(db: &Db) -> Vec<u64> {
    db.storage().file_stats().segments.iter().filter(|s| s.table == "logs").map(|s| s.id).collect()
}

fn offload_all(db: &Db) {
    for id in segment_ids(db) {
        assert!(db.storage().offload_segment(id).unwrap(), "segment {id} made cold");
    }
    assert!(db.storage().file_stats().segments.iter().filter(|s| s.table == "logs").all(|s| s.cold));
}

/// The local files of `ids`.
fn local_segment_files(dir: &Path, ids: &[u64]) -> Vec<String> {
    ids.iter().map(|id| format!("seg-{id:010}.strata")).filter(|n| dir.join(n).exists()).collect()
}

/// Files under `root`, recursively.
fn files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(root) else { return out };
    for e in rd {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(files(&p));
        } else {
            out.push(p);
        }
    }
    out
}

fn wait_for(what: &str, mut f: impl FnMut() -> bool) {
    let t = Instant::now();
    while !f() {
        assert!(t.elapsed() < Duration::from_secs(10), "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn cold_segments_answer_as_local_ones_and_reopen_from_the_cache() {
    let d = tmp("answers");
    let store_dir = d.join("store");
    let store: Arc<dyn ObjectStore> = Arc::new(DirStore::new(&store_dir).unwrap());
    answer_and_reopen(&d, store, || files(&store_dir).len());
}

/// Makes [`fill`]'s segments cold in `store` (in a database in `d`) and
/// checks every query's answer, after reopening too, with and without the
/// cache; `objects` counts the objects in the store.
fn answer_and_reopen(d: &Path, store: Arc<dyn ObjectStore>, objects: impl Fn() -> usize) {
    let path = d.join("db");
    let db = Db::create_empty(&path, opts(cold(store.clone()))).unwrap();
    fill(&db);
    let want = answers(&db);
    let ids = segment_ids(&db);
    assert_eq!(ids.len(), 3);
    assert_eq!(local_segment_files(&path, &ids).len(), 3);
    offload_all(&db);
    // The local images are gone; the store holds each with its part table.
    assert!(local_segment_files(&path, &ids).is_empty(), "{:?}", local_segment_files(&path, &ids));
    assert_eq!(objects(), 6);
    assert_eq!(answers(&db), want);
    let s = db.storage().cold_stats().unwrap();
    assert_eq!((s.segments, s.segments_offloaded), (3, 3));
    assert!(s.fetches > 0 && s.cached_bytes < s.bytes, "{s:?}");
    db.storage().check().unwrap();
    drop(db);

    // Reopened, it finds the parts it fetched in its cache files.
    let db = Db::open(&path, opts(cold(store.clone()))).unwrap();
    assert_eq!(answers(&db), want);
    let s = db.storage().cold_stats().unwrap();
    assert_eq!(s.fetches, 0, "{s:?}");
    assert!(s.found_parts > 0, "{s:?}");
    drop(db);

    // Without its cache, it fetches again.
    std::fs::remove_dir_all(path.join("cold")).unwrap();
    let db = Db::open(&path, opts(cold(store.clone()))).unwrap();
    assert_eq!(answers(&db), want);
    assert!(db.storage().cold_stats().unwrap().fetches > 0);
    db.storage().check().unwrap();
    drop(db);

    // Without its cold tier, it does not open, and says why.
    let e = Db::open(&path, DbOptions { layout: Layout::Directory, ..DbOptions::default() }).err().expect("refused");
    assert!(e.to_string().contains("cold tier"), "{e}");

    // Dropped, its objects go.
    let db = Db::open(&path, opts(cold(store))).unwrap();
    rows(&db, "DROP TABLE logs");
    rows(&db, "SELECT id FROM other WHERE id = 1");
    wait_for("the dropped table's objects to go", || objects() == 0);
    drop(db);
    let _ = std::fs::remove_dir_all(d);
}

/// [`answer_and_reopen`] on S3 or a store that speaks its API, at the URL
/// in `STRATA_COLD_S3_URL` (`s3://bucket/prefix/`, configured by
/// environment as [`strata::objects`] says); skipped without it.
#[cfg(feature = "s3")]
#[test]
fn cold_segments_on_s3() {
    let Ok(url) = std::env::var("STRATA_COLD_S3_URL") else {
        eprintln!("STRATA_COLD_S3_URL is not set: skipped");
        return;
    };
    let url = format!("{}/{}/", url.trim_end_matches('/'), std::process::id());
    let store = strata::storage::open_store(&url).unwrap();
    let probe = strata::storage::open_store(&url).unwrap();
    let d = tmp("s3");
    let db_dir = d.clone();
    // The objects of its segments, asked for one by one.
    let objects = move || {
        let ids = db_ids(&db_dir);
        (1..=16u64)
            .flat_map(|seg| [format!("seg-{seg:010}.strata"), format!("seg-{seg:010}.strata.crc")])
            .filter(|k| ids.iter().any(|p| probe.get_range(&format!("{p}/{k}"), 0..1).is_ok()))
            .count()
    };
    answer_and_reopen(&d, store, objects);
}

/// The ids, as cold keys start with them, of the database in `d/db`.
#[cfg(feature = "s3")]
fn db_ids(d: &Path) -> Vec<String> {
    let b = std::fs::read(d.join("db").join("CURRENT")).unwrap_or_default();
    let v: serde_json::Value = serde_json::from_slice(&b).unwrap_or_default();
    v["db_id"].as_u64().map(|id| vec![format!("{id:016x}")]).unwrap_or_default()
}

#[test]
fn writes_reach_cold_rows_and_merges_leave_them_cold() {
    let d = tmp("writes");
    let path = d.join("db");
    let store: Arc<dyn ObjectStore> = Arc::new(DirStore::new(d.join("store")).unwrap());
    let db = Db::create_empty(&path, opts(cold(store.clone()))).unwrap();
    fill(&db);
    offload_all(&db);
    rows(&db, "DELETE FROM logs WHERE id < 100");
    rows(&db, "UPDATE logs SET n = 1000 WHERE id = 4321");
    rows(&db, "INSERT INTO logs (id, host, n, body) VALUES (7000, 'omega', 5, 'replaced timeout row') ON CONFLICT (id) DO UPDATE SET host = excluded.host, n = excluded.n, body = excluded.body");
    let check = |db: &Db| {
        assert_eq!(rows(db, "SELECT count(*) FROM logs"), vec![vec![Value::Int(3 * N - 100)]]);
        assert_eq!(rows(db, "SELECT n FROM logs WHERE id = 4321"), vec![vec![Value::Int(1000)]]);
        assert_eq!(rows(db, "SELECT host FROM logs WHERE id = 7000"), vec![vec![Value::Str("omega".into())]]);
        assert_eq!(rows(db, "SELECT count(*) FROM logs WHERE id < 100"), vec![vec![Value::Int(0)]]);
        assert_eq!(rows(db, "SELECT id FROM logs WHERE MATCH(body, 'replaced')"), vec![vec![Value::Int(7000)]]);
    };
    check(&db);
    db.optimize().unwrap();
    check(&db);
    // The cold segments, which still hold rows, were not merged.
    let segs = db.storage().file_stats().segments;
    assert_eq!(segs.iter().filter(|s| s.table == "logs" && s.cold).count(), 3, "{segs:?}");
    drop(db);
    let db = Db::open(&path, opts(cold(store))).unwrap();
    check(&db);
    db.storage().check().unwrap();
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn dropped_cold_segments_lose_their_objects() {
    let d = tmp("drop");
    let (path, store_dir) = (d.join("db"), d.join("store"));
    let store: Arc<dyn ObjectStore> = Arc::new(DirStore::new(&store_dir).unwrap());
    let db = Db::create_empty(&path, opts(cold(store))).unwrap();
    fill(&db);
    offload_all(&db);
    // A segment whose rows are all deleted is merged away.
    rows(&db, "DELETE FROM logs WHERE id < 12000");
    db.optimize().unwrap();
    assert_eq!(segment_ids(&db).len(), 2);
    // Queries let go of the readers cached for the segments gone.
    rows(&db, "SELECT count(*) FROM logs WHERE host = 'beta'");
    wait_for("the merged-away segment's objects to go", || files(&store_dir).len() == 4);
    rows(&db, "DROP TABLE logs");
    rows(&db, "SELECT id FROM other WHERE id = 1");
    wait_for("the dropped table's objects to go", || files(&store_dir).is_empty());
    wait_for("the cache files to go", || files(&path.join("cold")).iter().all(|f| f.file_name().unwrap() == "PENDING"));
    drop(db);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn eviction_keeps_earlier_snapshots_reading() {
    let d = tmp("evict");
    let path = d.join("db");
    let store: Arc<dyn ObjectStore> = Arc::new(DirStore::new(d.join("store")).unwrap());
    let db = Db::create_empty(&path, opts(ColdOptions { cache_bytes: 1, ..cold(store) })).unwrap();
    fill(&db);
    let want = answers(&db);
    offload_all(&db);
    let before = db.snapshot();
    assert_eq!(answers(&db), want);
    let used = db.storage().cold_stats().unwrap().cached_bytes;
    let freed = db.storage().evict_cold().unwrap();
    assert!(freed > 0);
    let s = db.storage().cold_stats().unwrap();
    assert_eq!(s.cached_bytes, s.pinned_bytes, "{s:?}");
    assert!(s.cached_bytes < used && s.evictions == 3, "{s:?}");
    // The snapshot taken before reads the old cache files, unlinked.
    let q = "SELECT id, body FROM logs ORDER BY bm25(body, 'full closed') DESC, id LIMIT 5";
    let old = db.query_at(&before, q, &Params::new(), &db.exec).unwrap().rows;
    assert_eq!(old, rows(&db, q));
    assert_eq!(answers(&db), want);
    drop(before);
    // One cache file a segment.
    let caches = files(&path.join("cold")).into_iter().filter(|f| f.extension().is_some_and(|e| e == "cache")).count();
    assert_eq!(caches, 3);
    let _ = std::fs::remove_dir_all(&d);
}

/// A directory store whose reads, or writes of part tables, can be made
/// to fail.
#[derive(Debug)]
struct Flaky {
    inner: DirStore,
    reads_fail: AtomicBool,
    tables_fail: AtomicBool,
}

impl ObjectStore for Flaky {
    fn put_file(&self, key: &str, path: &Path) -> Result<()> {
        self.inner.put_file(key, path)
    }
    fn put(&self, key: &str, bytes: &[u8]) -> Result<()> {
        if self.tables_fail.load(Ordering::Relaxed) {
            return Err(strata::Error::Io(std::io::Error::other("the store refuses writes")));
        }
        self.inner.put(key, bytes)
    }
    fn get_range(&self, key: &str, range: Range<u64>) -> Result<Vec<u8>> {
        if self.reads_fail.load(Ordering::Relaxed) {
            return Err(strata::Error::Io(std::io::Error::other("the store is down")));
        }
        self.inner.get_range(key, range)
    }
    fn delete(&self, key: &str) -> Result<()> {
        self.inner.delete(key)
    }
    fn describe(&self) -> String {
        "flaky".into()
    }
}

#[test]
fn a_failing_store_gives_errors_not_other_rows() {
    let d = tmp("flaky");
    let (path, store_dir) = (d.join("db"), d.join("store"));
    let flaky = Arc::new(Flaky { inner: DirStore::new(&store_dir).unwrap(), reads_fail: AtomicBool::new(false), tables_fail: AtomicBool::new(false) });
    let store: Arc<dyn ObjectStore> = flaky.clone();
    let db = Db::create_empty(&path, opts(cold(store.clone()))).unwrap();
    fill(&db);
    let want = answers(&db);

    // An upload that fails leaves the segment local and nothing on the store.
    let first = segment_ids(&db)[0];
    flaky.tables_fail.store(true, Ordering::Relaxed);
    assert!(db.storage().offload_segment(first).is_err());
    flaky.tables_fail.store(false, Ordering::Relaxed);
    assert!(!db.storage().file_stats().segments.iter().any(|s| s.cold));
    wait_for("the failed upload's object to go", || files(&store_dir).is_empty());

    offload_all(&db);
    drop(db);
    std::fs::remove_dir_all(path.join("cold")).unwrap();
    // The store is down: the database opens only if it can fetch the
    // segments' headers and keys.
    flaky.reads_fail.store(true, Ordering::Relaxed);
    assert!(Db::open(&path, opts(cold(store.clone()))).is_err());
    flaky.reads_fail.store(false, Ordering::Relaxed);
    let db = Db::open(&path, opts(cold(store.clone()))).unwrap();
    // Down again: a query fails or reads what it read before (from parts
    // already fetched); one that reads the text fails; a write that needs
    // only keys works.
    flaky.reads_fail.store(true, Ordering::Relaxed);
    let mut failed = 0;
    for (q, want) in QUERIES.iter().zip(&want) {
        match sql(&db, q) {
            Ok(r) => assert_eq!(&r, want, "{q} while the store is down"),
            Err(e) => {
                assert!(e.to_string().contains("the store is down"), "{q}: {e}");
                failed += 1;
            }
        }
    }
    assert!(failed >= 3, "{failed} queries failed");
    assert!(sql(&db, "SELECT id, body FROM logs ORDER BY id LIMIT 5 OFFSET 30000").is_err());
    let mut tx = db.storage().begin().unwrap();
    assert!(tx.table("logs").unwrap().delete(&Value::Int(4321)).unwrap());
    tx.commit().unwrap();
    flaky.reads_fail.store(false, Ordering::Relaxed);
    assert_eq!(rows(&db, "SELECT count(*) FROM logs"), vec![vec![Value::Int(3 * N - 1)]]);
    assert_eq!(answers(&db)[2..4], want[2..4]);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_damaged_object_is_an_error() {
    let d = tmp("damaged");
    let (path, store_dir) = (d.join("db"), d.join("store"));
    let store: Arc<dyn ObjectStore> = Arc::new(DirStore::new(&store_dir).unwrap());
    let db = Db::create_empty(&path, opts(cold(store.clone()))).unwrap();
    fill(&db);
    offload_all(&db);
    drop(db);
    std::fs::remove_dir_all(path.join("cold")).unwrap();
    // A byte in the middle of every image's doc store.
    for f in files(&store_dir).into_iter().filter(|f| f.extension().is_some_and(|e| e == "strata")) {
        let mut b = std::fs::read(&f).unwrap();
        let (_, sections) = strata::segment::directory(&b).unwrap();
        let (_, r) = sections.iter().find(|(n, _)| n == "store.data").unwrap();
        b[(r.start + r.end) / 2] ^= 0x55;
        std::fs::write(&f, b).unwrap();
    }
    let db = Db::open(&path, opts(cold(store))).unwrap();
    let e = sql(&db, "SELECT id, body FROM logs ORDER BY id LIMIT 100000").err().expect("a damaged part is refused");
    assert!(e.to_string().contains("checksum"), "{e}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn objects_a_crash_left_are_deleted_on_open() {
    let d = tmp("crash");
    let (path, store_dir) = (d.join("db"), d.join("store"));
    let store: Arc<dyn ObjectStore> = Arc::new(DirStore::new(&store_dir).unwrap());
    let db = Db::create_empty(&path, opts(cold(store.clone()))).unwrap();
    fill(&db);
    let ids = segment_ids(&db);
    assert!(db.storage().offload_segment(ids[0]).unwrap());
    drop(db);
    // An upload that a crash cut short before its commit: the object is
    // on the store and recorded as being uploaded.
    let prefix = std::fs::read_dir(&store_dir).unwrap().next().unwrap().unwrap().file_name().to_string_lossy().into_owned();
    let key = format!("{prefix}/seg-9999999999.strata");
    store.put(&key, b"half an image").unwrap();
    std::fs::write(path.join("cold").join("PENDING"), format!(r#"{{"uploading":["{key}","{key}.crc"],"deleting":[]}}"#)).unwrap();
    std::fs::write(path.join("cold").join("seg-9999999999-00000099.cache"), b"stale").unwrap();
    let db = Db::open(&path, opts(cold(store.clone()))).unwrap();
    wait_for("the leftover object to go", || files(&store_dir).len() == 2);
    assert!(!path.join("cold").join("seg-9999999999-00000099.cache").exists());
    assert_eq!(rows(&db, "SELECT count(*) FROM logs"), vec![vec![Value::Int(3 * N)]]);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_checkpoint_holds_cold_segments_as_local_files() {
    let d = tmp("checkpoint");
    let path = d.join("db");
    let store: Arc<dyn ObjectStore> = Arc::new(DirStore::new(d.join("store")).unwrap());
    let db = Db::create_empty(&path, opts(cold(store))).unwrap();
    fill(&db);
    let want = answers(&db);
    offload_all(&db);
    let copy = d.join("copy");
    db.storage().checkpoint(&copy, &CheckpointOptions { layout: Layout::Directory, link_from: None }).unwrap();
    drop(db);
    // It opens without a cold tier.
    let c = Db::open(&copy, DbOptions { layout: Layout::Directory, ..DbOptions::default() }).unwrap();
    assert_eq!(answers(&c), want);
    assert!(!c.storage().file_stats().segments.iter().any(|s| s.cold));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn offload_keeps_the_newest_segments_local() {
    let d = tmp("policy");
    let path = d.join("db");
    let store: Arc<dyn ObjectStore> = Arc::new(DirStore::new(d.join("store")).unwrap());
    let db = Db::create_empty(&path, opts(cold(store.clone()))).unwrap();
    fill(&db);
    let segs = db.storage().file_stats().segments;
    let logs: Vec<_> = segs.iter().filter(|s| s.table == "logs").collect();
    let newest = logs.last().unwrap();
    drop(db);
    // Table `other`'s segment is too small to go.
    let keep = ColdOptions { keep_local_bytes: Some(newest.bytes), min_segment_bytes: 100_000, ..cold(store) };
    let db = Arc::new(Db::open(&path, opts(keep)).unwrap());
    db.spawn_cold(Duration::from_millis(50));
    wait_for("the two older segments to go cold", || db.storage().file_stats().segments.iter().filter(|s| s.cold).count() >= 2);
    std::thread::sleep(Duration::from_millis(200));
    let segs = db.storage().file_stats().segments;
    assert!(!segs.iter().find(|s| s.id == newest.id).unwrap().cold, "{segs:?}");
    assert_eq!(segs.iter().filter(|s| s.cold).count(), 2, "{segs:?}");
    assert!(segs.iter().filter(|s| s.cold).all(|s| s.table == "logs"), "{segs:?}");
    assert_eq!(rows(&db, "SELECT count(*) FROM logs"), vec![vec![Value::Int(3 * N)]]);
    drop(db);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn writes_by_key_need_no_store_with_keyword_keys_too() {
    let d = tmp("keyword-keys");
    let path = d.join("db");
    let flaky = Arc::new(Flaky { inner: DirStore::new(d.join("store")).unwrap(), reads_fail: AtomicBool::new(false), tables_fail: AtomicBool::new(false) });
    let store: Arc<dyn ObjectStore> = flaky.clone();
    let db = Db::create_empty(&path, opts(ColdOptions { cache_bytes: 1, ..cold(store) })).unwrap();
    db.sql("CREATE TABLE users (name KEYWORD PRIMARY KEY, n INT, bio TEXT)", &Params::new()).unwrap();
    let mut rng = StdRng::seed_from_u64(3);
    for s in 0..2 {
        let docs: Vec<Vec<Value>> = (s * N..(s + 1) * N)
            .map(|i| {
                let bio: String = (0..200).map(|_| rng.gen_range(b'a'..=b'z') as char).collect();
                vec![Value::Str(format!("user-{i:08}")), Value::Int(i), Value::Str(bio)]
            })
            .collect();
        db.load_into("users", docs).unwrap();
        db.storage().flush().unwrap();
    }
    for id in db.storage().file_stats().segments.iter().filter(|s| s.table == "users").map(|s| s.id).collect::<Vec<_>>() {
        assert!(db.storage().offload_segment(id).unwrap());
    }
    rows(&db, "SELECT count(*) FROM users WHERE MATCH(bio, 'abc')");
    // The key column is kept with the keys: evicted segments open again
    // without fetching anything.
    let before = db.storage().cold_stats().unwrap();
    assert!(db.storage().evict_cold().unwrap() > 0);
    let after = db.storage().cold_stats().unwrap();
    assert_eq!((after.fetches, after.cached_bytes), (before.fetches, after.pinned_bytes), "{before:?} {after:?}");
    // The store is down: deletes and upserts by key find their rows in
    // the keys the cache keeps.
    flaky.reads_fail.store(true, Ordering::Relaxed);
    let mut tx = db.storage().begin().unwrap();
    let mut t = tx.table("users").unwrap();
    assert!(t.delete(&Value::Str("user-00000007".into())).unwrap());
    assert!(!t.delete(&Value::Str("user-99999999".into())).unwrap());
    t.upsert(vec![Value::Str("user-00012345".into()), Value::Int(-1), Value::Str("rewritten".into())]).unwrap();
    tx.commit().unwrap();
    flaky.reads_fail.store(false, Ordering::Relaxed);
    assert_eq!(rows(&db, "SELECT count(*) FROM users"), vec![vec![Value::Int(2 * N - 1)]]);
    assert_eq!(rows(&db, "SELECT n FROM users WHERE name = 'user-00012345'"), vec![vec![Value::Int(-1)]]);
    db.storage().check().unwrap();
    let _ = std::fs::remove_dir_all(&d);
}

/// The cache files are the writer's, so a read-only open of a database
/// with cold segments is refused, saying why, whether or not it names a
/// cold tier; once the segments are local again, it reads.
#[test]
fn a_read_only_open_refuses_cold_segments() {
    let d = tmp("read-only");
    let path = d.join("db");
    let store: Arc<dyn ObjectStore> = Arc::new(DirStore::new(d.join("store")).unwrap());
    let db = Db::create_empty(&path, opts(cold(store.clone()))).unwrap();
    fill(&db);
    let want = answers(&db);
    let ro = DbOptions { layout: Layout::Directory, read_only: true, ..DbOptions::default() };
    assert_eq!(answers(&Db::open(&path, ro.clone()).unwrap()), want);
    offload_all(&db);
    let e = Db::open(&path, ro.clone()).err().expect("a reader of cold segments").to_string();
    assert!(e.contains("object storage") && e.contains("read-only"), "{e}");
    let e = Db::open(&path, DbOptions { cold: Some(cold(store)), ..ro }).err().expect("a reader with a cold tier").to_string();
    assert!(e.contains("read-only") && e.contains("cold"), "{e}");
    assert_eq!(answers(&db), want);
    drop(db);
    let _ = std::fs::remove_dir_all(&d);
}
