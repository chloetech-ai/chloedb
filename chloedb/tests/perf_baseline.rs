//! ChloeDB's write-ahead log and flush baseline: how the WAL grows, when it is
//! flushed and emptied, what a commit costs under each setting, and that what
//! it holds survives a crash.
//!
//! * **Correctness** (plain `cargo test`): the options' defaults are pinned;
//!   the log grows with commits and is empty after a flush; the memtable
//!   threshold flushes exactly where it says; the background flusher bounds
//!   the memtable under a stream of writes; commits made during a flush
//!   survive; a store dropped without a flush replays to the same rows; a
//!   torn tail, a damaged middle and `wal_drop_damaged` behave as documented;
//!   threads writing at once share commits.
//! * **Performance** (`#[ignore]`d; run on purpose): commit latency with and
//!   without fsync and by batch size, group commit, the WAL's bytes per row,
//!   flush times, writes while a flush runs, a sweep of `memtable_max_docs`,
//!   replay time by log size, and queries over a memtable against segments.
//!   The run is compared with `results/perf-baseline/baseline.json`.
//!
//! ```text
//! cd chloedb
//! cargo test --profile ci --test perf_baseline                    # correctness
//! cargo test --profile ci --test perf_baseline -- --ignored --nocapture   # compare
//! CHLOEDB_PERF=record cargo test --profile ci --test perf_baseline -- --ignored --nocapture
//! ```
//!
//! Absolute times belong to the machine in `meta.machine`; record again on
//! another machine rather than comparing across machines.
//! `CHLOEDB_PERF_SCALE=0.2` shrinks every size for a quick look (not
//! compared). fsync is the slowest thing measured, and noisy: run on a quiet
//! machine. The run's own numbers go to `$CARGO_TARGET_TMPDIR/perf_current.json`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chloedb::db::{Db, WriteOp};
use chloedb::query::Params;
use chloedb::storage::{wal, DbOptions, MergePolicy, MergerOptions};
use chloedb::{Field, FieldType, Schema, Value, VectorIndexKind};
use serde_json::{json, Value as Json};

const WAL_HEADER: u64 = 16; // "STRWAL01" and the database's id

// ------------------------------------------------------------------ data

fn schema() -> Schema {
    Schema::new(
        "docs",
        vec![Field::new("id", FieldType::Keyword), Field::new("n", FieldType::Int), Field::new("body", FieldType::Text { positions: false })],
        Some("id"),
    )
    .unwrap()
}

fn vec_schema() -> Schema {
    Schema::new(
        "docs",
        vec![
            Field::new("id", FieldType::Keyword),
            Field::new("n", FieldType::Int),
            Field::new("body", FieldType::Text { positions: false }),
            Field::new("emb", FieldType::Vector { dim: 128, index: VectorIndexKind::Flat }),
        ],
        Some("id"),
    )
    .unwrap()
}

fn letters(mut n: usize, len: usize) -> String {
    let mut s = String::new();
    for _ in 0..len {
        s.push((b'a' + (n % 26) as u8) as char);
        n /= 26;
    }
    s
}

fn key(i: usize) -> String {
    format!("k{i:08}")
}

/// A row of about 100 bytes of text, with a word only it has.
fn row(i: usize, v: i64) -> Vec<Value> {
    vec![
        Value::Str(key(i)),
        Value::Int(v),
        Value::Str(format!("wq{} the quick brown fox number {i} version {v} jumps over the lazy dog beside the river bank", letters(i, 6))),
    ]
}

/// The vector index ChloeCtx's store uses: a graph, rebuilt by every merge.
fn hnsw_schema() -> Schema {
    Schema::new(
        "docs",
        vec![
            Field::new("id", FieldType::Keyword),
            Field::new("n", FieldType::Int),
            Field::new("body", FieldType::Text { positions: false }),
            Field::new("emb", FieldType::Vector { dim: 128, index: VectorIndexKind::Hnsw { m: 16, ef_construction: 100 } }),
        ],
        Some("id"),
    )
    .unwrap()
}

fn vec_row(i: usize) -> Vec<Value> {
    let mut r = row(i, 0);
    let v: Vec<f32> = (0..128).map(|d| ((i * 31 + d * 7) % 97) as f32 / 97.0).collect();
    r.push(Value::Vector(v.into()));
    r
}

fn upsert(i: usize, v: i64) -> WriteOp {
    WriteOp::Upsert { table: "docs".into(), doc: row(i, v) }
}

fn insert(i: usize) -> WriteOp {
    WriteOp::Insert { table: "docs".into(), doc: row(i, 0) }
}

fn opts(sync: bool, max_docs: usize) -> DbOptions {
    DbOptions { sync, memtable_max_docs: max_docs, ..DbOptions::default() }
}

fn wal_path(db: &Path) -> PathBuf {
    PathBuf::from(format!("{}-wal", db.display()))
}

fn count(db: &Db) -> i64 {
    db.query("SELECT COUNT(*) FROM docs", &Params::new()).unwrap().rows[0][0].as_i64().unwrap()
}

/// Every live row, key to `n`.
fn rows(db: &Db) -> BTreeMap<String, i64> {
    let rs = db.query("SELECT id, n FROM docs LIMIT 10000000", &Params::new()).unwrap();
    rs.rows.iter().map(|r| (r[0].as_str().unwrap().to_string(), r[1].as_i64().unwrap())).collect()
}

fn needle(db: &Db, i: usize) -> Vec<String> {
    let q = format!("wq{}", letters(i, 6));
    let rs = db.query("SELECT id FROM docs ORDER BY bm25(body, :q, syntax => 'plain') DESC LIMIT 3", &Params::new().set("q", Value::Str(q))).unwrap();
    rs.rows.iter().map(|r| r[0].as_str().unwrap().to_string()).collect()
}

fn stats(db: &Db) -> (u64, usize, usize) {
    let s = db.storage().file_stats();
    (s.wal_bytes, s.memtable_docs, s.segments.len())
}

// ----------------------------------------------------------- correctness

/// What a program gets without choosing: the numbers this document's
/// description of the WAL and flushes rests on. A change to one is a change
/// of behaviour for everyone who embeds ChloeDB.
#[test]
fn defaults_are_pinned() {
    let o = DbOptions::default();
    assert_eq!(o.memtable_max_docs, 10_000, "flush threshold");
    assert!(o.sync, "fsync every commit");
    assert_eq!(o.memtable_chunk_docs, 1024);
    assert_eq!(o.memtable_tail_docs, 128);
    assert_eq!(o.segment_max_docs, 100_000);
    assert_eq!(o.write_wait, Duration::from_secs(5));
    assert!(!o.wal_drop_damaged);
    assert_eq!(o.ship_log_bytes, 0);
    assert!(!o.publish_snapshots);
}

/// The log grows with commits, holds exactly the unflushed rows, and is
/// only its header after a flush; nothing is lost either way.
#[test]
fn the_log_grows_with_commits_and_a_flush_empties_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("w.strata");
    let db = Db::create(&path, schema(), opts(false, 1_000_000)).unwrap();
    assert_eq!(stats(&db).0, WAL_HEADER, "a new log is its header");
    let mut last = WAL_HEADER;
    for i in 0..200 {
        db.write_ops(vec![insert(i)]).unwrap();
        let (bytes, mem, _) = stats(&db);
        assert!(bytes > last, "commit {i} did not grow the log");
        assert_eq!(mem, i + 1, "the memtable holds one row per commit");
        last = bytes;
    }
    assert_eq!(std::fs::metadata(wal_path(&path)).unwrap().len(), last, "wal_bytes is the file's length");
    db.storage().flush().unwrap();
    let (bytes, mem, segs) = stats(&db);
    assert_eq!((bytes, mem), (WAL_HEADER, 0), "a flush leaves an empty log and memtable");
    assert!(segs >= 1);
    assert_eq!(count(&db), 200);
    assert_eq!(std::fs::metadata(wal_path(&path)).unwrap().len(), WAL_HEADER);
    // Commits after a flush start the log again.
    db.write_ops(vec![insert(1000)]).unwrap();
    assert!(stats(&db).0 > WAL_HEADER);
    assert_eq!(count(&db), 201);
}

/// Without a flusher thread the commit that passes `memtable_max_docs`
/// flushes: the threshold is "more than", and counts rows and deletes.
#[test]
fn the_threshold_flushes_exactly_past_it() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::create(&dir.path().join("w.strata"), schema(), opts(false, 100)).unwrap();
    for i in 0..100 {
        db.write_ops(vec![insert(i)]).unwrap();
    }
    let (_, mem, segs) = stats(&db);
    assert_eq!((mem, segs), (100, 0), "at the threshold nothing is flushed yet");
    db.write_ops(vec![insert(100)]).unwrap();
    let (bytes, mem, segs) = stats(&db);
    assert_eq!(mem, 0, "the commit past it flushed the memtable");
    assert_eq!(bytes, WAL_HEADER);
    assert_eq!(segs, 1);
    // Deletes of rows in segments count too: 101 of them tip it over.
    for i in 0..100 {
        db.write_ops(vec![WriteOp::Delete { table: "docs".into(), key: Value::Str(key(i)) }]).unwrap();
    }
    assert_eq!(stats(&db).1, 0);
    assert_eq!(count(&db), 1);
    // One commit of many rows is one transaction: it flushes once, whole.
    db.write_ops((1000..1250).map(insert).collect()).unwrap();
    assert_eq!(stats(&db).1, 0, "a 250-row commit passed 100 and flushed");
    assert_eq!(count(&db), 251);
}

/// With the flusher thread the memtable stays bounded under a stream of
/// writes: commits return without flushing, and past twice the limit they
/// wait for it. Every row is there at the end, and in the log's flushes none
/// was written twice.
#[test]
fn the_flusher_bounds_the_memtable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("w.strata");
    let max = 500;
    let db = Db::create(&path, schema(), opts(false, max)).unwrap();
    db.spawn_flusher();
    let mut peak = 0;
    for i in 0..10_000 {
        db.write_ops(vec![insert(i)]).unwrap();
        if i % 50 == 0 {
            peak = peak.max(stats(&db).1);
        }
    }
    assert!(peak <= 2 * max + 100, "the memtable reached {peak} with a limit of {max}");
    assert!(peak > max / 2, "the stream never filled it ({peak}): the test measures nothing");
    assert_eq!(count(&db), 10_000);
    assert_eq!(rows(&db).len(), 10_000);
    // Let the flusher finish, then everything is in segments or the log.
    let t = Instant::now();
    while stats(&db).1 > max && t.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(stats(&db).1 <= max);
    drop(db);
    let db = Db::open(&path, opts(false, max)).unwrap();
    assert_eq!(count(&db), 10_000, "reopened");
}

/// Commits made while a flush builds its segments are not lost: they are in
/// the new log, and come back after a reopen without any flush of their own.
#[test]
fn commits_during_a_flush_survive() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("w.strata");
    {
        let db = Db::create(&path, schema(), opts(false, 1_000_000)).unwrap();
        for i in 0..300 {
            db.write_ops(vec![insert(i)]).unwrap();
        }
        db.storage()
            .background_flush_with(|| {
                // After the seal, before the install.
                for i in 300..400 {
                    db.write_ops(vec![insert(i)]).unwrap();
                }
                db.write_ops(vec![WriteOp::Delete { table: "docs".into(), key: Value::Str(key(5)) }]).unwrap();
                db.write_ops(vec![upsert(6, 77)]).unwrap();
            })
            .unwrap();
        let (bytes, mem, segs) = stats(&db);
        assert!(bytes > WAL_HEADER, "the new log holds what was written meanwhile");
        // 100 inserts and the new version of row 6; row 5's delete and row 6's
        // old version were carried into the new segment's delete set.
        assert_eq!(mem, 101, "the memtable holds the rows written meanwhile");
        assert_eq!(db.storage().file_stats().segments[0].n_deleted, 2);
        assert_eq!(segs, 1);
        assert_eq!(count(&db), 399);
    }
    let db = Db::open(&path, opts(false, 1_000_000)).unwrap();
    assert_eq!(count(&db), 399);
    let r = rows(&db);
    assert!(!r.contains_key(&key(5)));
    assert_eq!(r[&key(6)], 77);
    assert!(r.contains_key(&key(399)));
}

/// A store dropped with its rows only in the log opens to the same rows:
/// inserts, replacements and deletes alike, with and without fsync.
#[test]
fn an_unflushed_store_replays_to_the_same_rows() {
    for sync in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("w.strata");
        let mut model: BTreeMap<String, i64> = BTreeMap::new();
        {
            let db = Db::create(&path, schema(), opts(sync, 1_000_000)).unwrap();
            let mut x = 12345u64;
            for step in 0..400 {
                x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let i = (x >> 33) as usize % 120;
                match (x >> 20) % 4 {
                    0 => {
                        db.write_ops(vec![WriteOp::Delete { table: "docs".into(), key: Value::Str(key(i)) }]).unwrap();
                        model.remove(&key(i));
                    }
                    _ => {
                        db.write_ops(vec![upsert(i, step)]).unwrap();
                        model.insert(key(i), step);
                    }
                }
            }
            assert!(stats(&db).0 > WAL_HEADER);
            assert_eq!(rows(&db), model);
            // Dropped without a flush.
        }
        let before = std::fs::metadata(wal_path(&path)).unwrap().len();
        assert!(before > WAL_HEADER, "the log was emptied by the drop");
        let db = Db::open(&path, opts(sync, 1_000_000)).unwrap();
        assert_eq!(rows(&db), model, "sync={sync}: the replay differs from what was committed");
        for (i, _) in model.iter().take(5) {
            let n: usize = i[1..].parse().unwrap();
            assert!(needle(&db, n).contains(i), "sync={sync}: {i} is not found by its word after the replay");
        }
        assert_eq!(db.storage().damaged(), None);
    }
}

/// The tail of a log torn by a crash is cut: the transactions before it
/// are kept, the torn one is not, and the log is usable again.
#[test]
fn a_torn_tail_loses_only_the_last_commit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("w.strata");
    {
        let db = Db::create(&path, schema(), opts(true, 1_000_000)).unwrap();
        for i in 0..50 {
            db.write_ops(vec![insert(i)]).unwrap();
        }
    }
    let w = wal_path(&path);
    let len = std::fs::metadata(&w).unwrap().len();
    std::fs::OpenOptions::new().write(true).open(&w).unwrap().set_len(len - 5).unwrap();
    let db = Db::open(&path, opts(true, 1_000_000)).unwrap();
    assert_eq!(count(&db), 49, "the torn commit is gone and the others are not");
    assert_eq!(db.storage().damaged(), None, "a torn tail is not damage");
    assert!(std::fs::metadata(&w).unwrap().len() < len - 5 + 1, "the log is cut where the damage began");
    db.write_ops(vec![insert(49)]).unwrap();
    drop(db);
    let db = Db::open(&path, opts(true, 1_000_000)).unwrap();
    assert_eq!(count(&db), 50, "commits after the cut are kept");
}

/// Damage in the middle of a log that was durable (more than one commit
/// follows it) fails the open with `sync`, unless the caller asks to drop
/// from the damage on, which copies the log aside and says what it dropped.
/// Without `sync` a log is not promised durable, so the open drops.
#[test]
fn a_damaged_middle_fails_the_open_unless_asked_to_drop() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("w.strata");
    {
        let db = Db::create(&path, schema(), opts(true, 1_000_000)).unwrap();
        for i in 0..60 {
            db.write_ops(vec![insert(i)]).unwrap();
        }
    }
    let w = wal_path(&path);
    let mut bytes = std::fs::read(&w).unwrap();
    let at = bytes.len() / 2;
    bytes[at] ^= 0xff;
    std::fs::write(&w, &bytes).unwrap();
    let copy = |name: &str| {
        let d = dir.path().join(name);
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("w.strata");
        std::fs::copy(&path, &p).unwrap();
        std::fs::copy(&w, wal_path(&p)).unwrap();
        p
    };
    let strict = copy("strict");
    assert!(Db::open(&strict, opts(true, 1_000_000)).is_err(), "a log damaged after it was durable must not open silently");
    let drop_it = copy("drop");
    let db = Db::open(&drop_it, DbOptions { wal_drop_damaged: true, ..opts(true, 1_000_000) }).unwrap();
    let n = count(&db);
    assert!(0 < n && n < 60, "kept {n}: the rows before the damage, not those after");
    let what = db.storage().damaged().expect("says what it dropped").to_string();
    assert!(!what.is_empty());
    assert!(wal::damaged_path(&wal_path(&drop_it)).exists(), "the damaged log is copied aside first");
    let relaxed = copy("nosync");
    let db = Db::open(&relaxed, opts(false, 1_000_000)).unwrap();
    assert!(count(&db) < 60, "without sync the damaged rest is dropped");
}

/// Threads that write at once share commits: fewer groups than writes, and
/// every write is there.
#[test]
fn concurrent_writers_share_commits() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::create(&dir.path().join("w.strata"), schema(), opts(true, 1_000_000)).unwrap();
    let (threads, each) = (8usize, 40usize);
    std::thread::scope(|s| {
        for t in 0..threads {
            let db = &db;
            s.spawn(move || {
                for i in 0..each {
                    db.write_ops(vec![insert(t * 1000 + i)]).unwrap();
                }
            });
        }
    });
    let w = db.write_stats();
    assert_eq!(w.writes, (threads * each) as u64);
    assert!(w.groups < w.writes, "{} groups for {} writes: nothing was shared", w.groups, w.writes);
    assert_eq!(count(&db), (threads * each) as i64);
    let keys: BTreeSet<String> = rows(&db).into_keys().collect();
    assert_eq!(keys.len(), threads * each);
}

// ----------------------------------------------------------- performance

#[derive(Clone)]
struct Metric {
    value: f64,
    unit: &'static str,
    better: Better,
    tol: f64,
}

#[derive(Clone, Copy, PartialEq)]
enum Better {
    Lower,
    Higher,
    /// A count or a size that is part of the behaviour: may not move either
    /// way by more than `tol` (a ratio).
    Same,
}

struct Metrics {
    m: BTreeMap<String, Metric>,
}

impl Metrics {
    fn put(&mut self, name: &str, value: f64, unit: &'static str, better: Better, tol: f64) {
        println!("  {name:<46} {value:>13.3} {unit}");
        self.m.insert(name.to_string(), Metric { value, unit, better, tol });
    }
    fn lat(&mut self, name: &str, samples: &mut [f64], tols: (f64, f64, f64)) {
        samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p = |q: f64| samples[((samples.len() as f64 - 1.0) * q).round() as usize];
        self.put(&format!("{name}.p50_ms"), p(0.50), "ms", Better::Lower, tols.0);
        self.put(&format!("{name}.p95_ms"), p(0.95), "ms", Better::Lower, tols.1);
        self.put(&format!("{name}.p99_ms"), p(0.99), "ms", Better::Lower, tols.2);
    }
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn mb(b: u64) -> f64 {
    b as f64 / 1048576.0
}

fn rss_mb() -> f64 {
    let mut u: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut u) };
    let raw = u.ru_maxrss as f64;
    if cfg!(target_os = "macos") { raw / 1048576.0 } else { raw / 1024.0 }
}

fn machine() -> String {
    let sh = |c: &str| std::process::Command::new("sh").arg("-c").arg(c).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let cpu = if cfg!(target_os = "macos") { sh("sysctl -n machdep.cpu.brand_string") } else { sh("grep -m1 'model name' /proc/cpuinfo | cut -d: -f2") };
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0);
    format!("{} {} / {} / {cores} cores", std::env::consts::OS, std::env::consts::ARCH, cpu.trim())
}

fn scaled(n: usize, scale: f64) -> usize {
    ((n as f64 * scale) as usize).max(20)
}

/// Runs `n` single-row commits, returning each one's time in ms.
fn commits(db: &Db, from: usize, n: usize) -> Vec<f64> {
    (from..from + n)
        .map(|i| {
            let op = insert(i);
            let t = Instant::now();
            db.write_ops(vec![op]).unwrap();
            ms(t)
        })
        .collect()
}

fn run_perf(scale: f64) -> Metrics {
    let mut m = Metrics { m: BTreeMap::new() };
    let tmp = tempfile::tempdir().unwrap();
    let new = |name: &str, o: DbOptions| Db::create(&tmp.path().join(format!("{name}.strata")), schema(), o).unwrap();

    println!("-- commit latency, one row per commit");
    {
        let db = new("nosync", opts(false, 10_000_000));
        let n = scaled(5_000, scale);
        let t = Instant::now();
        let mut lat = commits(&db, 0, n);
        m.put("commit.nosync_1row.per_s", n as f64 / t.elapsed().as_secs_f64(), "commits/s", Better::Higher, 1.6);
        m.lat("commit.nosync_1row", &mut lat, (1.6, 2.0, 3.0));
    }
    {
        let db = new("sync", opts(true, 10_000_000));
        let n = scaled(300, scale);
        let t = Instant::now();
        let mut lat = commits(&db, 0, n);
        m.put("commit.sync_1row.per_s", n as f64 / t.elapsed().as_secs_f64(), "commits/s", Better::Higher, 1.6);
        m.lat("commit.sync_1row", &mut lat, (1.6, 2.5, 3.5));
    }

    println!("-- one fsync per transaction: rows per commit");
    for batch in [1usize, 10, 100, 1000] {
        let db = new(&format!("batch{batch}"), opts(true, 10_000_000));
        let txns = scaled(if batch >= 1000 { 40 } else { 150 }, scale).max(10);
        let t = Instant::now();
        for k in 0..txns {
            db.write_ops((0..batch).map(|j| insert(k * batch + j)).collect()).unwrap();
        }
        let secs = t.elapsed().as_secs_f64();
        m.put(&format!("commit.sync_batch{batch}.rows_per_s"), (txns * batch) as f64 / secs, "rows/s", Better::Higher, 1.6);
        m.put(&format!("commit.sync_batch{batch}.txns_per_s"), txns as f64 / secs, "txn/s", Better::Higher, 1.6);
    }

    println!("-- group commit: 8 threads, fsync");
    {
        let db = new("group", opts(true, 10_000_000));
        let each = scaled(60, scale);
        let t = Instant::now();
        std::thread::scope(|s| {
            for th in 0..8usize {
                let db = &db;
                s.spawn(move || {
                    for i in 0..each {
                        db.write_ops(vec![insert(th * 100_000 + i)]).unwrap();
                    }
                });
            }
        });
        let secs = t.elapsed().as_secs_f64();
        let w = db.write_stats();
        assert!(w.groups < w.writes, "group commit shared nothing");
        m.put("group.sync_8threads.rows_per_s", (8 * each) as f64 / secs, "rows/s", Better::Higher, 1.6);
        m.put("group.sync_8threads.writes_per_commit", w.writes as f64 / w.groups as f64, "writes", Better::Higher, 1.6);
        assert_eq!(count(&db), (8 * each) as i64);
    }

    println!("-- the log's size");
    {
        let n = 2000;
        let p = tmp.path().join("size_text.strata");
        let db = Db::create(&p, schema(), opts(false, 10_000_000)).unwrap();
        for i in 0..n {
            db.write_ops(vec![insert(i)]).unwrap();
        }
        let one = (stats(&db).0 - WAL_HEADER) as f64 / n as f64;
        m.put("wal.bytes_per_row_text_1row_commits", one, "B", Better::Same, 1.1);
        let p = tmp.path().join("size_batched.strata");
        let db = Db::create(&p, schema(), opts(false, 10_000_000)).unwrap();
        for k in 0..n / 100 {
            db.write_ops((0..100).map(|j| insert(k * 100 + j)).collect()).unwrap();
        }
        m.put("wal.bytes_per_row_text_100row_commits", (stats(&db).0 - WAL_HEADER) as f64 / n as f64, "B", Better::Same, 1.1);
        let p = tmp.path().join("size_vec.strata");
        let db = Db::create(&p, vec_schema(), opts(false, 10_000_000)).unwrap();
        for i in 0..n {
            db.write_ops(vec![WriteOp::Insert { table: "docs".into(), doc: vec_row(i) }]).unwrap();
        }
        m.put("wal.bytes_per_row_vec128", (stats(&db).0 - WAL_HEADER) as f64 / n as f64, "B", Better::Same, 1.1);
    }

    println!("-- flush");
    {
        let n = scaled(10_000, scale);
        let db = new("flush", opts(false, 10_000_000));
        for k in 0..n / 100 {
            db.write_ops((0..100).map(|j| insert(k * 100 + j)).collect()).unwrap();
        }
        let wal = stats(&db).0;
        let t = Instant::now();
        db.storage().flush().unwrap();
        m.put("flush.explicit_10k_ms", ms(t), "ms", Better::Lower, 2.0);
        let s = db.storage().file_stats();
        let seg: u64 = s.segments.iter().map(|x| x.bytes).sum();
        m.put("flush.segment_mb_10k", mb(seg), "MB", Better::Same, 1.2);
        m.put("flush.wal_to_segment_ratio", wal as f64 / seg.max(1) as f64, "x", Better::Same, 1.2);
        assert_eq!(stats(&db).1, 0);

        let db = new("bgflush", opts(false, 10_000_000));
        for k in 0..n / 100 {
            db.write_ops((0..100).map(|j| insert(k * 100 + j)).collect()).unwrap();
        }
        let t = Instant::now();
        // Writes go on while it builds: their latency is what a user sees.
        let mut during = Vec::new();
        db.storage()
            .background_flush_with(|| {
                during = commits(&db, 1_000_000, 200);
            })
            .unwrap();
        m.put("flush.background_10k_ms", ms(t), "ms", Better::Lower, 2.0);
        m.lat("flush.commit_while_sealed", &mut during, (2.0, 3.0, 4.0));
    }

    println!("-- writes under a stream, by flush threshold (flusher thread on)");
    let stream = scaled(40_000, scale);
    for max in [1_000usize, 4_096, 10_000, 50_000] {
        let p = tmp.path().join(format!("sweep{max}.strata"));
        let db = Db::create(&p, schema(), opts(false, max)).unwrap();
        db.spawn_flusher();
        let (mut lat, mut peak_mem, mut peak_wal) = (Vec::with_capacity(stream), 0usize, 0u64);
        let t = Instant::now();
        for i in 0..stream {
            let op = insert(i);
            let t1 = Instant::now();
            db.write_ops(vec![op]).unwrap();
            lat.push(ms(t1));
            if i % 500 == 0 {
                let (w, mem, _) = stats(&db);
                peak_mem = peak_mem.max(mem);
                peak_wal = peak_wal.max(w);
            }
        }
        let secs = t.elapsed().as_secs_f64();
        assert!(peak_mem <= 2 * max + 600, "max {max}: the memtable reached {peak_mem}");
        // Let the last flush finish before counting segments.
        let w0 = Instant::now();
        while stats(&db).1 > max && w0.elapsed() < Duration::from_secs(20) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let (_, _, segs) = stats(&db);
        assert_eq!(count(&db), stream as i64, "max {max}: rows lost");
        let tag = format!("sweep.max{max}");
        m.put(&format!("{tag}.rows_per_s"), stream as f64 / secs, "rows/s", Better::Higher, 1.6);
        let worst = lat.iter().cloned().fold(0.0, f64::max);
        m.lat(&tag, &mut lat, (1.6, 2.5, 4.0));
        m.put(&format!("{tag}.commit_max_ms"), worst, "ms", Better::Lower, 4.0);
        m.put(&format!("{tag}.peak_memtable_rows"), peak_mem as f64, "rows", Better::Lower, 1.5);
        m.put(&format!("{tag}.peak_wal_mb"), mb(peak_wal), "MB", Better::Lower, 1.5);
        m.put(&format!("{tag}.segments_at_end"), segs as f64, "segments", Better::Lower, 1.5);
    }

    println!("-- replay: opening a store whose log was never flushed");
    for n in [2_000usize, 10_000, 20_000] {
        let n = scaled(n, scale);
        let p = tmp.path().join(format!("replay{n}.strata"));
        {
            let db = Db::create(&p, schema(), opts(false, 10_000_000)).unwrap();
            for k in 0..n / 100 {
                db.write_ops((0..100).map(|j| insert(k * 100 + j)).collect()).unwrap();
            }
        }
        let wal = std::fs::metadata(wal_path(&p)).unwrap().len();
        assert!(wal > WAL_HEADER * 100, "the log was flushed by the drop: nothing to replay");
        let t = Instant::now();
        let db = Db::open(&p, opts(false, 10_000_000)).unwrap();
        let open = ms(t);
        // The first query after a replay builds the memtable's indexes: time
        // it before anything else reads.
        let t = Instant::now();
        assert_eq!(needle(&db, 7).first(), Some(&key(7)));
        let first = ms(t);
        assert_eq!(count(&db), n as i64, "the replay lost rows");
        let tag = format!("replay.{}k", n / 1000);
        m.put(&format!("{tag}.open_ms"), open, "ms", Better::Lower, 2.0);
        m.put(&format!("{tag}.wal_mb"), mb(wal), "MB", Better::Same, 1.1);
        m.put(&format!("{tag}.replay_rows_per_s"), n as f64 / (open / 1000.0), "rows/s", Better::Higher, 1.8);
        m.put(&format!("{tag}.first_query_ms"), first, "ms", Better::Lower, 3.0);
        // The same rows once flushed: what a clean close or an idle flush buys.
        db.storage().flush().unwrap();
        drop(db);
        let t = Instant::now();
        let db = Db::open(&p, opts(false, 10_000_000)).unwrap();
        m.put(&format!("{tag}.open_after_flush_ms"), ms(t), "ms", Better::Lower, 2.5);
        assert_eq!(count(&db), n as i64);
    }

    println!("-- replay of rows with a 128-dimension vector");
    {
        let n = scaled(4_000, scale);
        let p = tmp.path().join("replay_vec.strata");
        {
            let db = Db::create(&p, vec_schema(), opts(false, 10_000_000)).unwrap();
            for k in 0..n / 100 {
                db.write_ops((0..100).map(|j| WriteOp::Insert { table: "docs".into(), doc: vec_row(k * 100 + j) }).collect()).unwrap();
            }
        }
        let wal = std::fs::metadata(wal_path(&p)).unwrap().len();
        let t = Instant::now();
        let db = Db::open(&p, opts(false, 10_000_000)).unwrap();
        m.put("replay.vec128_4k.open_ms", ms(t), "ms", Better::Lower, 2.0);
        m.put("replay.vec128_4k.wal_mb", mb(wal), "MB", Better::Same, 1.1);
        let t = Instant::now();
        assert_eq!(needle(&db, 7).first(), Some(&key(7)));
        m.put("replay.vec128_4k.first_query_ms", ms(t), "ms", Better::Lower, 3.0);
        assert_eq!(count(&db), n as i64);
    }

    println!("-- queries over a memtable and over segments (10k rows)");
    {
        let n = scaled(10_000, scale);
        let db = new("query", opts(false, 10_000_000));
        for k in 0..n / 100 {
            db.write_ops((0..100).map(|j| insert(k * 100 + j)).collect()).unwrap();
        }
        let probe = |db: &Db, tag: &str, m: &mut Metrics| {
            let (mut text, mut keyed) = (Vec::new(), Vec::new());
            let t = Instant::now();
            assert_eq!(needle(db, 7).first(), Some(&key(7)));
            m.put(&format!("query.{tag}.first_text_ms"), ms(t), "ms", Better::Lower, 3.0);
            for i in 0..200 {
                let j = (i * 37) % n;
                let t = Instant::now();
                let r = needle(db, j);
                text.push(ms(t));
                assert_eq!(r.first(), Some(&key(j)), "{tag}: the word of row {j} did not find it");
                let t = Instant::now();
                let rs = db.query("SELECT n FROM docs WHERE id = :k", &Params::new().set("k", Value::Str(key(j)))).unwrap();
                keyed.push(ms(t));
                assert_eq!(rs.rows.len(), 1);
            }
            m.lat(&format!("query.{tag}.text"), &mut text, (1.8, 2.5, 3.5));
            m.lat(&format!("query.{tag}.by_key"), &mut keyed, (1.8, 2.5, 3.5));
        };
        probe(&db, "memtable_10k", &mut m);
        db.storage().flush().unwrap();
        probe(&db, "segments_10k", &mut m);
    }

    println!("-- merges: 8 flushed segments of 1,000 rows into one (the default policy)");
    // Segments of `per` rows each, flushed one by one; rows `i` of segment
    // `s` are `s * per + i`, and with `overwrite` the first part of each
    // segment after the first rewrites rows of the one before it.
    let segments = |db: &Db, n: usize, per: usize, vectors: bool, overwrite: usize| {
        for s in 0..n {
            let ops: Vec<WriteOp> = (0..per)
                .map(|i| {
                    let k = s * per + i;
                    match (vectors, i < overwrite && s > 0) {
                        (_, true) => WriteOp::Upsert { table: "docs".into(), doc: { let mut r = if vectors { vec_row(k - per) } else { row(k - per, 0) }; r[1] = Value::Int(s as i64); r } },
                        (true, _) => WriteOp::Insert { table: "docs".into(), doc: vec_row(k) },
                        _ => insert(k),
                    }
                })
                .collect();
            db.write_ops(ops).unwrap();
            db.storage().flush().unwrap();
        }
    };
    let seg_bytes = |db: &Db| db.storage().file_stats().segments.iter().map(|s| s.bytes).sum::<u64>();
    let merge_tag = |m: &mut Metrics, tag: &str, db: &Db, policy: &MergePolicy, rows_in: usize| {
        let (before, bytes_in) = (stats(db).2, seg_bytes(db));
        let t = Instant::now();
        let st = db.storage().merge(policy).unwrap();
        let secs = t.elapsed().as_secs_f64();
        m.put(&format!("merge.{tag}.ms"), secs * 1000.0, "ms", Better::Lower, 2.0);
        m.put(&format!("merge.{tag}.rows_per_s"), st.docs_merged as f64 / secs, "rows/s", Better::Higher, 1.8);
        m.put(&format!("merge.{tag}.write_amp"), st.bytes_written as f64 / bytes_in as f64, "x", Better::Same, 1.2);
        m.put(&format!("merge.{tag}.segments_after"), stats(db).2 as f64, "segments", Better::Same, 1.0);
        m.put(&format!("merge.{tag}.dropped"), st.docs_dropped as f64, "rows", Better::Same, 1.0);
        println!("    {before} segments in, {} merges, {} rows kept of {rows_in}", st.merges, st.docs_merged);
    };
    {
        let db = new("merge_text", opts(false, 10_000_000));
        segments(&db, 8, 1000, false, 0);
        let probe = |db: &Db| {
            let mut lat: Vec<f64> = (0..100)
                .map(|i| {
                    let j = (i * 37) % 8000;
                    let t = Instant::now();
                    assert_eq!(needle(db, j).first(), Some(&key(j)));
                    ms(t)
                })
                .collect();
            lat.sort_by(|a, b| a.partial_cmp(b).unwrap());
            lat[50]
        };
        let before = probe(&db);
        merge_tag(&mut m, "text_8x1000", &db, &MergePolicy::default(), 8000);
        assert_eq!(count(&db), 8000);
        m.put("merge.query_8_segments.p50_ms", before, "ms", Better::Lower, 2.5);
        m.put("merge.query_1_segment.p50_ms", probe(&db), "ms", Better::Lower, 2.5);
    }
    {
        let db = Db::create(&tmp.path().join("merge_vec2.strata"), vec_schema(), opts(false, 10_000_000)).unwrap();
        segments(&db, 8, 1000, true, 0);
        merge_tag(&mut m, "vec128_flat_8x1000", &db, &MergePolicy::default(), 8000);
        assert_eq!(count(&db), 8000);
    }
    {
        let db = Db::create(&tmp.path().join("merge_hnsw.strata"), hnsw_schema(), opts(false, 10_000_000)).unwrap();
        segments(&db, 8, 1000, true, 0);
        merge_tag(&mut m, "vec128_hnsw_8x1000", &db, &MergePolicy::default(), 8000);
        assert_eq!(count(&db), 8000);
    }
    {
        // Each segment rewrites the first 300 rows of the one before it: the
        // old copies are deleted, and a merge drops them.
        let db = new("merge_deletes", opts(false, 10_000_000));
        segments(&db, 8, 1000, false, 300);
        let live = count(&db);
        merge_tag(&mut m, "text_8x1000_30pct_overwritten", &db, &MergePolicy::compact(), 8000);
        assert_eq!(count(&db), live);
    }

    println!("-- commits while an HNSW merge runs (8 x 1,000 rows of 128-d vectors)");
    {
        let db = Arc::new(Db::create(&tmp.path().join("merge_beside.strata"), hnsw_schema(), opts(false, 10_000_000)).unwrap());
        segments(&db, 8, 1000, true, 0);
        let (d2, done) = (db.clone(), Arc::new(std::sync::atomic::AtomicBool::new(false)));
        let flag = done.clone();
        let t = Instant::now();
        let h = std::thread::spawn(move || {
            d2.storage().merge(&MergePolicy::default()).unwrap();
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        let (mut lat, mut i) = (Vec::new(), 100_000);
        while !done.load(std::sync::atomic::Ordering::SeqCst) {
            let op = WriteOp::Insert { table: "docs".into(), doc: vec_row(i) };
            let t = Instant::now();
            db.write_ops(vec![op]).unwrap();
            lat.push(ms(t));
            i += 1;
            std::thread::sleep(Duration::from_micros(200));
        }
        h.join().unwrap();
        println!("    merge took {:.0} ms with {} commits beside it", ms(t), lat.len());
        m.lat("merge.beside_commit", &mut lat, (2.0, 3.0, 4.0));
        m.put("merge.beside_commit.max_ms", lat.iter().cloned().fold(0.0, f64::max), "ms", Better::Lower, 4.0);
        assert_eq!(count(&db), 8000 + lat.len() as i64);
    }

    println!("-- flusher and merger threads together: 40,000 one-row commits, flush at 4,096 (as ChloeCtx)");
    {
        let n = scaled(40_000, scale);
        let db = new("merger_threads", opts(false, 4096));
        db.spawn_flusher();
        db.spawn_merger(MergerOptions { threads: 1, ..MergerOptions::default() });
        let t = Instant::now();
        let mut lat = commits(&db, 0, n);
        let secs = t.elapsed().as_secs_f64();
        let at_end = stats(&db).2;
        // The merger runs on its own schedule: let it settle.
        let t_settle = Instant::now();
        let mut last = (usize::MAX, 0);
        while t_settle.elapsed() < Duration::from_secs(20) {
            std::thread::sleep(Duration::from_millis(250));
            let (st, running) = (stats(&db).2, db.storage().merge_status().running);
            if st == last.0 && running == 0 && last.1 >= 4 {
                break;
            }
            last = (st, if st == last.0 && running == 0 { last.1 + 1 } else { 0 });
        }
        m.put("threads.rows_per_s", n as f64 / secs, "rows/s", Better::Higher, 1.8);
        m.lat("threads.commit", &mut lat, (2.0, 3.0, 4.0));
        m.put("threads.commit.max_ms", lat.iter().cloned().fold(0.0, f64::max), "ms", Better::Lower, 4.0);
        m.put("threads.segments_at_end", at_end as f64, "segments", Better::Lower, 1.5);
        m.put("threads.segments_settled", stats(&db).2 as f64, "segments", Better::Lower, 1.5);
        m.put("threads.merges_done", db.storage().merge_status().merges as f64, "merges", Better::Lower, 1.5);
        db.storage().stop_merger();
        assert_eq!(count(&db), n as i64);
    }
    m.put("rss.peak_mb", rss_mb(), "MB", Better::Lower, 1.5);
    m
}

fn baseline_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("results/perf-baseline/baseline.json")
}

fn to_json(m: &Metrics, scale: f64) -> Json {
    let metrics: serde_json::Map<String, Json> = m
        .m
        .iter()
        .map(|(k, v)| {
            let better = match v.better {
                Better::Lower => "lower",
                Better::Higher => "higher",
                Better::Same => "same",
            };
            (k.clone(), json!({"value": (v.value * 1e4).round() / 1e4, "unit": v.unit, "better": better, "tolerance": v.tol}))
        })
        .collect();
    json!({
        "meta": {
            "machine": machine(),
            "scale": scale,
            "debug_assertions": cfg!(debug_assertions),
            "defaults": {"memtable_max_docs": DbOptions::default().memtable_max_docs, "sync": DbOptions::default().sync},
            "git": std::process::Command::new("git").args(["rev-parse", "--short", "HEAD"]).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default(),
        },
        "metrics": metrics,
    })
}

fn regressions(m: &Metrics, base: &Json) -> Vec<String> {
    let mut out = Vec::new();
    for (name, b) in base["metrics"].as_object().unwrap() {
        let Some(cur) = m.m.get(name) else {
            out.push(format!("{name}: in the baseline, not measured now"));
            continue;
        };
        let bv = b["value"].as_f64().unwrap();
        let tol = b["tolerance"].as_f64().unwrap();
        // What a run-to-run difference of timing alone can move: segment and
        // memtable counts depend on when the flusher ran.
        let floor = match cur.unit {
            "ms" => 0.05,
            "segments" | "merges" => 2.0,
            "rows" => 1000.0,
            "MB" if name.contains("peak_wal") => 0.3,
            _ => 0.0,
        };
        let bad = match b["better"].as_str().unwrap() {
            "lower" => cur.value > bv * tol && cur.value - bv > floor,
            "higher" => cur.value < bv / tol,
            _ => cur.value > bv * tol || cur.value < bv / tol,
        };
        if bad {
            out.push(format!("{name}: {:.3} {} against a baseline of {bv:.3} (tolerance {tol}x)", cur.value, cur.unit));
        }
    }
    out
}

#[test]
#[ignore = "a measurement: run with --ignored, on a quiet machine"]
fn performance_against_baseline() {
    let scale: f64 = std::env::var("CHLOEDB_PERF_SCALE").ok().and_then(|s| s.parse().ok()).unwrap_or(1.0);
    println!("chloedb WAL baseline on {}", machine());
    let t = Instant::now();
    let m = run_perf(scale);
    println!("-- measured in {:.0} s", t.elapsed().as_secs_f64());
    let cur = to_json(&m, scale);
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("perf_current.json");
    std::fs::write(&out, serde_json::to_string_pretty(&cur).unwrap() + "\n").unwrap();
    println!("this run: {}", out.display());

    let path = baseline_path();
    if std::env::var("CHLOEDB_PERF").as_deref() == Ok("record") {
        assert!(scale == 1.0, "a baseline is recorded at scale 1");
        assert!(!cfg!(debug_assertions), "record the baseline with an optimised build (--profile ci or --release)");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_string_pretty(&cur).unwrap() + "\n").unwrap();
        println!("baseline written: {}", path.display());
        return;
    }
    let Ok(text) = std::fs::read_to_string(&path) else { panic!("no baseline at {}; record one with CHLOEDB_PERF=record", path.display()) };
    let base: Json = serde_json::from_str(&text).unwrap();
    if scale != 1.0 {
        println!("scale {scale}: numbers are not compared with the baseline");
        return;
    }
    assert_eq!(base["meta"]["debug_assertions"], cur["meta"]["debug_assertions"], "the baseline and this build differ in optimisation");
    if base["meta"]["machine"] != cur["meta"]["machine"] {
        println!("note: the baseline was recorded on {}; this is {}", base["meta"]["machine"], cur["meta"]["machine"]);
    }
    let bad = regressions(&m, &base);
    assert!(bad.is_empty(), "{} metric(s) regressed against {}:\n  {}", bad.len(), path.display(), bad.join("\n  "));
    println!("within the baseline's tolerances ({} metrics)", base["metrics"].as_object().unwrap().len());
}

#[allow(dead_code)]
fn _unused(_: Arc<()>) {}
