//! What reading rows from a table's doc store costs: on a database the
//! log-bytes probe made (`examples/log_bytes.rs`, a table `logs` keyed by
//! `id` from 0), times reads of one row by key at random keys, whose time
//! is mostly one block's decompression, and a scan that reads every row's
//! `body` (`LIKE`, which compares stored text). Prints JSON.
//!
//! ```text
//! store_reads DB [READS (5000)]
//! ```
use std::time::Instant;

use rand::{Rng, SeedableRng};
use chloedb::db::{Db, SqlResult};
use chloedb::query::Params;
use chloedb::storage::DbOptions;

fn rows(db: &Db, q: &str) -> Vec<Vec<chloedb::Value>> {
    match db.sql(q, &Params::new()).unwrap_or_else(|e| panic!("{q}: {e}")) {
        SqlResult::Rows(r) => r.rows,
        SqlResult::Written(_) => unreachable!(),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let reads: usize = args.get(2).map_or(5000, |a| a.parse().unwrap());
    let db = Db::open(std::path::Path::new(&args[1]), DbOptions::default()).unwrap();
    let n = rows(&db, "SELECT count(*) FROM logs")[0][0].as_i64().unwrap();
    let mut rng = rand::rngs::StdRng::seed_from_u64(1);
    // The page cache warm: every row read once.
    let scan = |db: &Db| {
        let t = Instant::now();
        let hits = rows(db, "SELECT count(*) FROM logs WHERE body LIKE '%no such words here%'")[0][0].as_i64().unwrap();
        assert_eq!(hits, 0);
        t.elapsed().as_secs_f64()
    };
    scan(&db);
    let mut us: Vec<f64> = (0..reads)
        .map(|_| {
            let q = format!("SELECT * FROM logs WHERE id = {}", rng.gen_range(0..n));
            let t = Instant::now();
            assert_eq!(rows(&db, &q).len(), 1);
            t.elapsed().as_secs_f64() * 1e6
        })
        .collect();
    us.sort_by(|a, b| a.total_cmp(b));
    let mut scans: Vec<f64> = (0..3).map(|_| scan(&db)).collect();
    scans.sort_by(|a, b| a.total_cmp(b));
    let at = |q: f64| (us[((us.len() - 1) as f64 * q) as usize] * 10.0).round() / 10.0;
    println!(
        "{}",
        serde_json::json!({
            "db": args[1],
            "rows": n,
            "reads_by_key": reads,
            "read_by_key_us": {"p50": at(0.5), "p90": at(0.9), "p99": at(0.99)},
            "scan_body_seconds": (scans[1] * 100.0).round() / 100.0,
            "scan_rows_per_second_millions": (n as f64 / scans[1] / 1e4).round() / 100.0,
        })
    );
}
