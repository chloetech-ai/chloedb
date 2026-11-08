//! How long the first search after a write takes, with the server's
//! database options, by the size of the pieces the memtable's last chunk
//! is built in (`DbOptions::memtable_tail_docs`; 0: one piece). Ignored by
//! default: it loads 200,000 of the Simulator's test documents per setting
//! and prints JSON lines (strata/results/storage/memtable-tail/):
//!
//! `cargo test --profile ci --test all write_stall:: -- --ignored --nocapture`

use std::sync::Arc;
use std::time::{Duration, Instant};

use rand::SeedableRng;
use serde_json::json;
use strata::db::Db;
use strata_server::keys::Principal;
use strata_server::service::{AgentConfig, After, Backend, QueryReq, Service};
use strata_server::traffic::data::{self, Ask, Corpus, Picks, TABLE};
use strata_server::traffic::dist::Rand;

fn agent() -> AgentConfig {
    AgentConfig { api_key: None, model: strata_server::agent::DEFAULT_MODEL.into(), base_url: "http://unused".into() }
}

async fn search(svc: &Arc<Service>, a: Ask) -> f64 {
    let Ask::Sql { sql, params } = a else { unreachable!("searches only") };
    let t0 = Instant::now();
    let params = svc.params(Some(&params)).await.unwrap();
    svc.query(QueryReq { sql, params, timeout_ms: 30_000, quiet: true, ..Default::default() }).await.unwrap();
    t0.elapsed().as_secs_f64() * 1000.0
}

fn quantiles(v: &mut [f64]) -> serde_json::Value {
    v.sort_by(|a, b| a.total_cmp(b));
    let at = |q: f64| (v[((v.len() as f64 * q) as usize).min(v.len() - 1)] * 1000.0).round() / 1000.0;
    json!({"p50_ms": at(0.5), "p90_ms": at(0.9), "p99_ms": at(0.99), "n": v.len()})
}

async fn run(tail: usize) {
    const ROWS: u64 = 200_000;
    let dir = tempfile::tempdir().unwrap();
    let mut opts = strata_server::server::db_options(false);
    opts.memtable_tail_docs = tail;
    let db = Arc::new(Db::create(&dir.path().join("w.strata"), strata_server::demo::schema(10), opts).unwrap());
    let schema = (*db.schema()).clone();
    let svc = Arc::new(Service::new(Backend::Local(db), schema, agent(), 64, Duration::from_secs(30)));
    svc.sql_in(&Principal::open(), &data::create_sql(64), &Default::default(), After::Any, 60_000, None, None).await.unwrap();
    let corpus = Corpus::new(1, 64);
    let t0 = Instant::now();
    for s in (0..ROWS).step_by(500) {
        let ops = (s..s + 500).map(|i| json!({"op": "upsert", "table": TABLE, "doc": corpus.doc(i)})).collect();
        svc.write_request(ops, None).await.unwrap();
    }
    let load_s = t0.elapsed().as_secs_f64();
    tokio::time::sleep(Duration::from_secs(5)).await;
    let mut picks = Picks::new(ROWS, 2000, 0.8, 0.5, 0.0);
    let mut rng = Rand::seed_from_u64(7);
    let mut next = ROWS;
    let mut out = json!({"tail_docs": tail, "rows": ROWS, "load_s": (load_s * 10.0).round() / 10.0});
    // One-row writes, each followed by a search, and a search with no
    // write before it; the memtable's last chunk grows from 0 to 600 rows.
    for kind in ["bm25", "vector", "hybrid (RRF)"] {
        let (mut quiet, mut after, mut write) = (Vec::new(), Vec::new(), Vec::new());
        for _ in 0..200 {
            quiet.push(search(&svc, data::ask(kind, &corpus, &mut picks, &mut rng).unwrap()).await);
            let t = Instant::now();
            svc.write_request(vec![json!({"op": "upsert", "table": TABLE, "doc": corpus.doc(next)})], None).await.unwrap();
            write.push(t.elapsed().as_secs_f64() * 1000.0);
            next += 1;
            after.push(search(&svc, data::ask(kind, &corpus, &mut picks, &mut rng).unwrap()).await);
        }
        out[kind] = json!({"no_write_before": quantiles(&mut quiet), "first_after_a_write": quantiles(&mut after), "the_write": quantiles(&mut write)});
    }
    // Searches after 9,000 rows more, with no writes: a memtable of about
    // 9,600 rows (it flushes at 10,000), nine full chunks and a last one
    // of about 384 rows.
    for s in (next..next + 9000).step_by(500) {
        let ops = (s..s + 500).map(|i| json!({"op": "upsert", "table": TABLE, "doc": corpus.doc(i)})).collect();
        svc.write_request(ops, None).await.unwrap();
    }
    for kind in ["bm25", "bm25 + filter", "vector", "hybrid (RRF)", "facet"] {
        let mut v = Vec::new();
        for _ in 0..300 {
            v.push(search(&svc, data::ask(kind, &corpus, &mut picks, &mut rng).unwrap()).await);
        }
        out["full_memtable"][kind] = quantiles(&mut v);
    }
    println!("{out}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "a benchmark: loads 200,000 rows per setting"]
async fn first_search_after_a_write() {
    for tail in [0, 128, 256] {
        run(tail).await;
    }
}
