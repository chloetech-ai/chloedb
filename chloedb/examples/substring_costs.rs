//! What each way of answering a pattern from a substring index costs, on a
//! database that has one: opens it read-only and, for every pattern of a
//! file, answers it in each segment whose field has the index twice, once
//! listing every candidate row (`locate_bytes` 0: walked to its sample,
//! and its value read and tested where the index does not settle it) and
//! once scanning every part with a candidate (each value read and tested),
//! the doc store read as a query reads it. Prints a JSON line a pattern:
//! the rows walked, the values read and the bytes and values of the
//! indexed parts, and the median milliseconds of each way: what a row
//! walked, a value tested and a byte scanned cost, which [`LOCATE_BYTES`]
//! and [`EXACT_ROWS`] stand for.
//!
//! ```text
//! substring_costs DB TABLE FIELD PATTERNS.txt [RUNS]
//! ```
//!
//! `PATTERNS.txt` has a pattern a line, `LIKE` or `ILIKE`, a tab and the
//! pattern, as `substring_bench` reads them.
//!
//! [`LOCATE_BYTES`]: chloedb::substring::like::LOCATE_BYTES
//! [`EXACT_ROWS`]: chloedb::substring::like::EXACT_ROWS
use std::time::Instant;

use chloedb::db::Db;
use chloedb::forward::StoreCache;
use chloedb::storage::DbOptions;
use chloedb::substring::like::{self, Stats};
use chloedb::substring::Like;
use chloedb::Value;

fn median(mut xs: Vec<f64>) -> f64 {
    xs.sort_by(f64::total_cmp);
    xs[xs.len() / 2]
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 5 {
        eprintln!("substring_costs DB TABLE FIELD PATTERNS.txt [RUNS]");
        std::process::exit(2);
    }
    let runs: usize = args.get(5).map_or(5, |r| r.parse().expect("RUNS"));
    let db = Db::open(std::path::Path::new(&args[1]), DbOptions { read_only: true, ..DbOptions::default() }).expect("the database");
    let schema = db.catalog().into_iter().find(|s| s.table == args[2]).expect("the table");
    let field = schema.field_index(&args[3]).expect("the field");
    let snap = db.snapshot().table(&args[2]).expect("a snapshot");
    let readers = db.readers(&snap).expect("the segments");
    let indexed: Vec<_> = readers.iter().filter(|r| r.substring(field).ok().flatten().is_some()).collect();
    let bytes: u64 = indexed.iter().map(|r| r.substring(field).unwrap().unwrap().parts().iter().map(|p| p.n()).sum::<u64>()).sum();
    let docs: u64 = indexed.iter().map(|r| r.substring(field).unwrap().unwrap().docs()).sum();
    // The doc store's rows, uncompressed: what a scan reads.
    let stored: u64 = indexed.iter().map(|r| r.store().unwrap().map_or(0, |s| s.byte_sizes().1 as u64)).sum();
    eprintln!("{} segments, {} with the index: {docs} values, {bytes} bytes, rows of {stored} bytes", readers.len(), indexed.len());
    for line in std::fs::read_to_string(&args[4]).expect("the patterns").lines() {
        let Some((op, pattern)) = line.split_once('\t') else { continue };
        let like = Like::new(pattern, op.eq_ignore_ascii_case("ILIKE"));
        let mut out = serde_json::json!({"op": op, "pattern": pattern});
        out["plan"] = match like.plan() {
            like::Plan::All => "all",
            like::Plan::Exact(_) => "exact",
            like::Plan::Spelled(_) => "spelled",
            like::Plan::Pieces(_) => "pieces",
        }
        .into();
        for (way, locate) in [("list", 0u64), ("scan", u64::MAX)] {
            let mut times = Vec::new();
            let mut stats = Stats::default();
            let mut matches = 0u64;
            for run in 0..runs + 1 {
                let mut s = Stats::default();
                let t = Instant::now();
                let mut found = 0u64;
                for r in &indexed {
                    let ix = r.substring(field).unwrap().unwrap();
                    let store = r.store().unwrap().expect("a doc store");
                    let mut cache = StoreCache::default();
                    let mut value = |d| {
                        Ok(match store.get_fields_cached(d, &[field], &mut cache)?.pop() {
                            Some(Value::Str(v)) => Some(v),
                            _ => None,
                        })
                    };
                    found += like::eval(ix, &like, r.n_docs, &mut value, locate, &mut s).unwrap().len() as u64;
                }
                // The first run warms the page cache and is not timed.
                if run > 0 {
                    times.push(t.elapsed().as_secs_f64() * 1e3);
                } else {
                    stats = s;
                    matches = found;
                }
            }
            out[way] = serde_json::json!({
                "ms": (median(times) * 1000.0).round() / 1000.0,
                "located": stats.located, "read": stats.read, "exact": stats.exact, "tested": stats.tested, "scanned": stats.scanned,
                "matches": matches,
            });
        }
        out["bytes"] = bytes.into();
        out["stored"] = stored.into();
        out["docs"] = docs.into();
        println!("{out}");
    }
}
