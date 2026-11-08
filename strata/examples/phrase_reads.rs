//! What reading positions costs: on a database the log-bytes probe made
//! (`examples/log_bytes.rs`, table `logs` with a `body` TEXT field), times
//! phrase queries, which read the positions of every row holding all of a
//! phrase's terms, counting their matches and returning the first ten, and
//! a query of the same terms without the phrase, which reads none. Run it
//! on two databases of the same lines, one with positions as varints and
//! one packed, to compare the layouts. It opens them read-only and
//! without the filter cache, so every run reads the positions. Prints a
//! JSON line per phrase.
//!
//! ```text
//! phrase_reads DB 'PHRASE' ['PHRASE' ...]
//! ```
use std::time::Instant;

use strata::db::{Db, SqlResult};
use strata::query::Params;
use strata::storage::DbOptions;

const RUNS: usize = 7;

fn rows(db: &Db, q: &str) -> Vec<Vec<strata::Value>> {
    match db.sql(q, &Params::new()).unwrap_or_else(|e| panic!("{q}: {e}")) {
        SqlResult::Rows(r) => r.rows,
        SqlResult::Written(_) => unreachable!(),
    }
}

/// The median time of `q` over [`RUNS`] runs after one to warm the cache,
/// in milliseconds, and its first answer.
fn timed(db: &Db, q: &str) -> (f64, Vec<Vec<strata::Value>>) {
    let first = rows(db, q);
    let mut ms: Vec<f64> = (0..RUNS)
        .map(|_| {
            let t = Instant::now();
            rows(db, q);
            t.elapsed().as_secs_f64() * 1e3
        })
        .collect();
    ms.sort_by(|a, b| a.total_cmp(b));
    ((ms[RUNS / 2] * 100.0).round() / 100.0, first)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // Read-only: it changes nothing in the probe's databases. Without the
    // filter cache, which would keep a phrase's rows after the first run
    // and read no positions again.
    let mut db = Db::open(std::path::Path::new(&args[1]), DbOptions { read_only: true, ..DbOptions::default() }).unwrap();
    db.exec.filter_cache = None;
    let layout = {
        let snap = db.storage().snapshot();
        let t = snap.table("logs").unwrap();
        let packed = t.segments().iter().filter(|s| s.file.section_sizes().iter().any(|(n, _)| n.ends_with(".ppos"))).count();
        format!("{packed} of {} segments packed", t.segments().len())
    };
    for phrase in &args[2..] {
        let words = phrase.replace('\'', "''");
        let (count_ms, n) = timed(&db, &format!("SELECT count(*) FROM logs WHERE MATCH(body, '\"{words}\"')"));
        let (first_ms, _) = timed(&db, &format!("SELECT id FROM logs WHERE MATCH(body, '\"{words}\"') LIMIT 10"));
        let (terms_ms, all) = timed(&db, &format!("SELECT count(*) FROM logs WHERE MATCH(body, '{words}')"));
        println!(
            "{}",
            serde_json::json!({
                "db": args[1],
                "layout": layout,
                "phrase": phrase,
                "matches": n[0][0].as_i64(),
                "rows_with_the_terms": all[0][0].as_i64(),
                "count_ms": count_ms,
                "first_10_ms": first_ms,
                "terms_alone_ms": terms_ms,
            })
        );
    }
}
