//! What reading cold segments costs: copies a database (one made by
//! `log_bytes`, a table `logs` of log lines) into the directory layout,
//! times queries on its local segments, makes them cold in a directory
//! store that answers each read after a delay and at a rate, as object
//! storage does, and times each query again: first with an empty cache
//! (after reopening, which fetches the headers and keys), then with the
//! parts it fetched. Prints JSON: local, cold-first and cold-warm times,
//! the bytes each first run fetched, and the disk the database takes.
//!
//! ```text
//! cold_bench SRC WORK [LATENCY_MS (30)] [MB_PER_SEC (100)]
//! ```
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use strata::db::{Db, SqlResult};
use strata::query::Params;
use strata::storage::{CheckpointOptions, ColdOptions, DbOptions, DirStore, Layout, ObjectStore};

/// A directory store whose reads take `latency` plus their length at
/// `rate` bytes a second, each (S3's first byte and one connection's
/// throughput; eight run at once).
#[derive(Debug)]
struct Slow {
    inner: DirStore,
    latency: Duration,
    rate: f64,
}

impl ObjectStore for Slow {
    fn put_file(&self, key: &str, path: &Path) -> strata::Result<()> {
        self.inner.put_file(key, path)
    }
    fn put(&self, key: &str, bytes: &[u8]) -> strata::Result<()> {
        self.inner.put(key, bytes)
    }
    fn get_range(&self, key: &str, range: Range<u64>) -> strata::Result<Vec<u8>> {
        let t = Instant::now();
        let b = self.inner.get_range(key, range)?;
        let due = self.latency + Duration::from_secs_f64(b.len() as f64 / self.rate);
        std::thread::sleep(due.saturating_sub(t.elapsed()));
        Ok(b)
    }
    fn delete(&self, key: &str) -> strata::Result<()> {
        self.inner.delete(key)
    }
    fn describe(&self) -> String {
        format!("{} (slowed)", self.inner.describe())
    }
}

/// The queries timed; `{lo}` and `{hi}` are the bounds of the tenth of
/// the table's time span in its middle.
const QUERIES: [(&str, &str); 7] = [
    ("count", "SELECT count(*) FROM logs"),
    ("by key", "SELECT * FROM logs WHERE id = 5123457"),
    ("keyword filter", "SELECT count(*) FROM logs WHERE severity = 'WARN'"),
    ("group by", "SELECT host, count(*) FROM logs GROUP BY host ORDER BY count(*) DESC LIMIT 10"),
    ("time range", "SELECT count(*) FROM logs WHERE ts >= {lo} AND ts < {hi}"),
    ("text match", "SELECT count(*) FROM logs WHERE MATCH(body, 'exception')"),
    ("bm25 top 10", "SELECT id, body FROM logs ORDER BY bm25(body, 'block replication failed') DESC LIMIT 10"),
];

fn run(db: &Db, q: &str) -> f64 {
    let t = Instant::now();
    match db.sql(q, &Params::new()).unwrap_or_else(|e| panic!("{q}: {e}")) {
        SqlResult::Rows(r) => assert!(!r.rows.is_empty() || q.contains("id =")),
        SqlResult::Written(_) => unreachable!(),
    }
    t.elapsed().as_secs_f64() * 1e3
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    (v[v.len() / 2] * 100.0).round() / 100.0
}

fn size(p: &Path) -> u64 {
    match std::fs::metadata(p) {
        Ok(m) if m.is_dir() => std::fs::read_dir(p).unwrap().map(|e| size(&e.unwrap().path())).sum(),
        // Sparse cache files: what they hold, not their length.
        Ok(m) => {
            use std::os::unix::fs::MetadataExt;
            m.blocks() * 512
        }
        Err(_) => 0,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (src, work) = (PathBuf::from(&args[1]), PathBuf::from(&args[2]));
    let latency = Duration::from_millis(args.get(3).map_or(30, |a| a.parse().unwrap()));
    let rate = args.get(4).map_or(100.0, |a| a.parse::<f64>().unwrap()) * 1e6;
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();
    let (path, store_dir) = (work.join("db"), work.join("store"));
    {
        let s = Db::open(&src, DbOptions::default()).unwrap();
        s.storage().checkpoint(&path, &CheckpointOptions { layout: Layout::Directory, link_from: None }).unwrap();
    }
    let store: Arc<dyn ObjectStore> = Arc::new(Slow { inner: DirStore::new(&store_dir).unwrap(), latency, rate });
    let opts = || DbOptions { layout: Layout::Directory, cold: Some(ColdOptions { min_segment_bytes: 0, ..ColdOptions::new(store.clone()) }), ..DbOptions::default() };

    let db = Db::open(&path, opts()).unwrap();
    let local_bytes = size(&path);
    let (rows, lo, hi) = match db.sql("SELECT count(*), min(ts), max(ts) FROM logs", &Params::new()).unwrap() {
        SqlResult::Rows(r) => {
            let at = |i: usize| match &r.rows[0][i] {
                strata::Value::Timestamp(t) | strata::Value::Int(t) => *t,
                v => panic!("{v:?}"),
            };
            (at(0), at(1), at(2))
        }
        _ => unreachable!(),
    };
    let span = (hi - lo) / 10;
    let queries: Vec<(&str, String)> = QUERIES.iter().map(|(n, q)| (*n, q.replace("{lo}", &format!("'{}'", strata::timestamp::format(lo + 4 * span))).replace("{hi}", &format!("'{}'", strata::timestamp::format(lo + 5 * span))))).collect();
    // Local: the OS's page cache warm (a run first), then five runs.
    let local: Vec<f64> = queries
        .iter()
        .map(|(_, q)| {
            run(&db, q);
            median((0..5).map(|_| run(&db, q)).collect())
        })
        .collect();
    let t = Instant::now();
    let ids: Vec<u64> = db.storage().file_stats().segments.iter().filter(|s| s.table == "logs").map(|s| s.id).collect();
    for id in &ids {
        assert!(db.storage().offload_segment(*id).unwrap());
    }
    let offload_s = t.elapsed().as_secs_f64();
    drop(db);

    let mut out = Vec::new();
    let mut open_ms = Vec::new();
    for ((name, q), local) in queries.iter().zip(&local) {
        // An empty cache: opening fetches the headers and keys.
        std::fs::remove_dir_all(path.join("cold")).unwrap();
        let t = Instant::now();
        let db = Db::open(&path, opts()).unwrap();
        open_ms.push(t.elapsed().as_secs_f64() * 1e3);
        let before = db.storage().cold_stats().unwrap();
        let first = run(&db, q);
        let after = db.storage().cold_stats().unwrap();
        let warm = median((0..5).map(|_| run(&db, q)).collect());
        out.push(serde_json::json!({
            "query": name,
            "sql": q,
            "local_ms": local,
            "cold_first_ms": (first * 100.0).round() / 100.0,
            "cold_warm_ms": warm,
            "fetches": after.fetches - before.fetches,
            "fetched_mb": ((after.fetched_bytes - before.fetched_bytes) as f64 / 1e6 * 10.0).round() / 10.0,
        }));
    }
    std::fs::remove_dir_all(path.join("cold")).unwrap();
    let db = Db::open(&path, opts()).unwrap();
    let s = db.storage().cold_stats().unwrap();
    let local_after = size(&path);
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "rows": rows,
            "segments": ids.len(),
            "store": {"latency_ms": latency.as_millis() as u64, "mb_per_sec_per_request": rate / 1e6, "parallel_requests": 8},
            "image_mb": (s.bytes as f64 / 1e6).round(),
            "local_mb_before": (local_bytes as f64 / 1e6).round(),
            "local_mb_after_pinned_only": (local_after as f64 / 1e6).round(),
            "pinned_mb": (s.pinned_bytes as f64 / 1e6).round(),
            "store_mb": (size(&store_dir) as f64 / 1e6).round(),
            "offload_seconds": (offload_s * 10.0).round() / 10.0,
            "open_empty_cache_ms": median(open_ms),
            "queries": out,
        }))
        .unwrap()
    );
}
