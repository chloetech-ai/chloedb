//! How long graph walks and recommendations take through a coordinator,
//! against one database holding the same rows, and against an
//! application walking the graph itself, a read by key per hop. A
//! benchmark, run by hand:
//!
//! `cargo test --profile ci --test all graph_bench:: -- --ignored --nocapture`
//!
//! It writes `results/graph/cluster.json` (`CHLOEDB_BENCH_OUT` to write
//! elsewhere). The shards and the coordinator run in this process, over
//! gRPC on loopback: the times are the walk's rounds and the shards' work,
//! with no network between machines.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chloedb::db::Db;
use chloedb::query::Params;
use chloedb::storage::DbOptions;
use chloedb::Schema;
use chloedb_server::auth::Access;
use chloedb_server::coord::Coordinator;
use chloedb_server::keys::Principal;
use chloedb_server::service::{After, AgentConfig, Backend, QueryReq, Service};
use chloedb_server::server;
use serde_json::{json, Value as Json};
use tokio::net::TcpListener;

const ITEMS: usize = 20_000;
const LINKS: usize = 5;
const USERS: usize = 2_000;
const PER_USER: usize = 12;
const GOODS: usize = 500;
const RUNS: usize = 25;

struct Lcg(u64);

impl Lcg {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) % n as u64) as usize
    }
}

fn schema() -> Schema {
    Schema::from_json(
        r#"{"table": "items", "key": "id", "fields": [
            {"name": "id", "type": "keyword"},
            {"name": "title", "type": "text"},
            {"name": "cat", "type": "keyword"},
            {"name": "cooc", "type": "edges"}]}"#,
    )
    .unwrap()
}

fn service(backend: Backend, schema: Schema) -> Arc<Service> {
    let agent = AgentConfig { api_key: None, model: chloedb_server::agent::DEFAULT_MODEL.into(), base_url: "http://unused".into() };
    Arc::new(Service::new(backend, schema, agent, 64, Duration::from_secs(60)))
}

fn local(dir: &std::path::Path, name: &str) -> Arc<Service> {
    let db = Db::create(&dir.join(name), schema(), DbOptions { publish_snapshots: true, ..DbOptions::default() }).unwrap();
    let s = (*db.schema()).clone();
    service(Backend::Local(Arc::new(db)), s)
}

async fn grpc(svc: Arc<Service>) -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(server::serve_grpc(svc, l, Access::default()));
    url
}

async fn sql(svc: &Arc<Service>, s: &str) {
    svc.sql(&Principal::open(), s, &Params::new(), After::Any, 0).await.unwrap_or_else(|e| panic!("{}: {}", &s[..s.len().min(120)], e.message));
}

async fn rows(svc: &Arc<Service>, s: &str) -> Vec<Vec<chloedb::value::Value>> {
    svc.query(QueryReq { sql: s.into(), ..Default::default() }).await.unwrap_or_else(|e| panic!("{s}: {}", e.message)).rs.rows
}

/// Links: each item to `LINKS` others, half of them to the first tenth
/// (popular items), so that hops widen as on real graphs.
fn links() -> Vec<Vec<usize>> {
    let mut g = Lcg(11);
    (0..ITEMS).map(|_| (0..LINKS).map(|j| if j % 2 == 0 { g.below(ITEMS / 10) } else { g.below(ITEMS) }).collect()).collect()
}

async fn load(svc: &Arc<Service>, links: &[Vec<usize>]) {
    let mut g = Lcg(5);
    for chunk in (0..ITEMS).collect::<Vec<_>>().chunks(1000) {
        let ops: Vec<Json> = chunk
            .iter()
            .map(|&i| json!({"op": "upsert", "doc": {"id": format!("i{i}"), "title": format!("item {i}"), "cat": format!("c{}", i % 7), "cooc": links[i].iter().map(|t| format!("i{t}")).collect::<Vec<_>>()}}))
            .collect();
        svc.write_ops(ops, None).await.unwrap();
    }
    sql(svc, "CREATE TABLE goods (id KEYWORD PRIMARY KEY, title TEXT)").await;
    sql(svc, &format!("INSERT INTO goods VALUES {}", (0..GOODS).map(|i| format!("('g{i}', 'good {i}')")).collect::<Vec<_>>().join(", "))).await;
    sql(svc, "CREATE TABLE events (id KEYWORD PRIMARY KEY, usr KEYWORD NOT NULL, item KEYWORD, ts INT) OPTIONS(shard_by = 'usr')").await;
    let evs: Vec<String> = (0..USERS)
        .flat_map(|u| (0..PER_USER).map(move |j| (u, j)))
        .map(|(u, j)| {
            let item = if j % 3 == 0 { (u * 7 + j * 13) % GOODS } else { (u * 7 + j * 13 + g.below(40)) % GOODS };
            format!("('u{u}-{j}', 'u{u}', 'g{item}', {})", u * 100 + j)
        })
        .collect();
    for c in evs.chunks(2000) {
        sql(svc, &format!("INSERT INTO events VALUES {}", c.join(", "))).await;
    }
    sql(svc, "CREATE RECOMMENDER shop ON events(usr, item, ts) ITEMS goods").await;
}

/// The application's own walk through `svc`: a read of the frontier's
/// links per hop, as an app did before a coordinator walked graphs.
async fn app_walk(svc: &Arc<Service>, seed: &str, hops: usize) -> usize {
    let mut seen: HashSet<String> = HashSet::from([seed.to_string()]);
    let mut frontier = vec![seed.to_string()];
    for _ in 0..hops {
        let mut next = Vec::new();
        for chunk in frontier.chunks(2000) {
            let list = chunk.iter().map(|k| format!("'{k}'")).collect::<Vec<_>>().join(", ");
            for r in rows(svc, &format!("SELECT id, cooc FROM items WHERE id IN ({list}) LIMIT 100000")).await {
                let Some(text) = r.get(1).and_then(|v| v.as_str()) else { continue };
                let to: Vec<String> = serde_json::from_str(text).unwrap_or_default();
                for t in to {
                    if seen.insert(t.clone()) {
                        next.push(t);
                    }
                }
            }
        }
        frontier = next;
    }
    seen.len() - 1
}

fn stats(mut ms: Vec<f64>) -> Json {
    ms.sort_by(f64::total_cmp);
    let at = |q: f64| ms[((ms.len() - 1) as f64 * q).round() as usize];
    json!({"p50_ms": (at(0.5) * 100.0).round() / 100.0, "p90_ms": (at(0.9) * 100.0).round() / 100.0, "runs": ms.len()})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "a benchmark: loads 20,000 linked rows and 24,000 events three ways"]
async fn graph_walks_and_recommendations_through_a_coordinator() {
    let dir = tempfile::tempdir().unwrap();
    let whole = local(dir.path(), "whole.strata");
    let mut urls = Vec::new();
    for s in 0..3 {
        urls.push(grpc(local(dir.path(), &format!("s{s}.strata"))).await);
    }
    let c = Coordinator::connect(urls).await.unwrap();
    let cs = c.schema.clone();
    let three = service(Backend::Cluster(Arc::new(c)), cs);
    let c = Coordinator::connect(vec![grpc(local(dir.path(), "only.strata")).await]).await.unwrap();
    let cs = c.schema.clone();
    let one = service(Backend::Cluster(Arc::new(c)), cs);
    let ls = links();
    let t0 = Instant::now();
    for svc in [&whole, &three, &one] {
        load(svc, &ls).await;
    }
    eprintln!("loaded in {:.1} s", t0.elapsed().as_secs_f64());
    let seed = |r: usize| format!("i{}", ITEMS / 10 + (r * 7919) % (ITEMS - ITEMS / 10));
    let queries: Vec<(&str, Box<dyn Fn(usize) -> String>)> = vec![
        ("REACHABLE 1 hop, COUNT", Box::new(move |r| format!("SELECT COUNT(*) FROM items WHERE REACHABLE(cooc, '{}', hops => 1)", seed(r)))),
        ("REACHABLE 2 hops, COUNT", Box::new(move |r| format!("SELECT COUNT(*) FROM items WHERE REACHABLE(cooc, '{}', hops => 2)", seed(r)))),
        ("REACHABLE 3 hops, COUNT", Box::new(move |r| format!("SELECT COUNT(*) FROM items WHERE REACHABLE(cooc, '{}', hops => 3)", seed(r)))),
        ("REACHABLE 2 hops, filter and top 10", Box::new(move |r| format!("SELECT id FROM items WHERE REACHABLE(cooc, '{}', hops => 2) AND cat = 'c3' ORDER BY id LIMIT 10", seed(r)))),
        ("PATH, both ways", Box::new(move |r| format!("SELECT id FROM items WHERE PATH(cooc, '{}', '{}', max_hops => 8, dir => 'both') ORDER BY hops()", seed(r), seed(r + 1000)))),
        ("ppr eps 1e-4, top 10", Box::new(move |r| format!("SELECT id FROM items ORDER BY ppr(cooc, '{}', eps => 1e-4) DESC LIMIT 10", seed(r)))),
        ("recommend, ORDER BY over a table", Box::new(|r| format!("SELECT id FROM goods ORDER BY recommend('shop', 'u{}') DESC LIMIT 10", (r * 37) % USERS))),
        ("FROM recommend(...)", Box::new(|r| format!("SELECT * FROM recommend('shop', 'u{}', k => 10)", (r * 53) % USERS))),
    ];
    let mut out = serde_json::Map::new();
    for (name, q) in &queries {
        let mut by: serde_json::Map<String, Json> = serde_json::Map::new();
        for (what, svc) in [("one database", &whole), ("coordinator, 3 shards", &three), ("coordinator, 1 shard", &one)] {
            rows(svc, &q(RUNS + 1)).await;
            let mut ms = Vec::new();
            let mut rounds = Vec::new();
            for r in 0..RUNS {
                let t = Instant::now();
                let a = svc.query(QueryReq { sql: q(r), ..Default::default() }).await.unwrap_or_else(|e| panic!("{}: {}", q(r), e.message));
                ms.push(t.elapsed().as_secs_f64() * 1e3);
                rounds.extend(a.rs.stats.plan.iter().find_map(|l| l.split("walked by the coordinator in ").nth(1)?.split(' ').next()?.parse::<usize>().ok()));
            }
            let mut s = stats(ms);
            if !rounds.is_empty() {
                rounds.sort();
                s["rounds_p50"] = json!(rounds[rounds.len() / 2]);
            }
            eprintln!("{name:40} {what:24} {s}");
            by.insert(what.into(), s);
        }
        out.insert((*name).into(), Json::Object(by));
    }
    // An application's own walk through the coordinator, a read per hop.
    for hops in [2, 3] {
        let mut ms = Vec::new();
        let mut agree = 0;
        for r in 0..RUNS {
            let t = Instant::now();
            let n = app_walk(&three, &seed(r), hops).await;
            ms.push(t.elapsed().as_secs_f64() * 1e3);
            let q = format!("SELECT COUNT(*) FROM items WHERE REACHABLE(cooc, '{}', hops => {hops})", seed(r));
            let want = rows(&whole, &q).await[0][0].as_i64().unwrap_or(-1) as usize;
            agree += usize::from(want == n || want == n + 1);
        }
        let s = stats(ms);
        eprintln!("{:40} {:24} {s} (agrees with REACHABLE {agree}/{RUNS})", format!("app walk {hops} hops, COUNT"), "coordinator, 3 shards");
        out.insert(format!("application's own walk, {hops} hops (reads only)"), json!({"coordinator, 3 shards": s}));
    }
    let machine = std::fs::read_to_string("/proc/cpuinfo").ok().and_then(|c| c.lines().find(|l| l.starts_with("model name")).map(|l| l.split(':').nth(1).unwrap_or("").trim().to_string()));
    let report = json!({
        "what": "graph walks and recommendations: one database, a coordinator of 3 shards and of 1 shard, all in one process over gRPC on loopback",
        "rows": {"items": ITEMS, "links_per_item": LINKS, "users": USERS, "events": USERS * PER_USER, "goods": GOODS},
        "machine": {"cpu": machine, "cores": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0)},
        "runs_per_query": RUNS,
        "results": out,
    });
    let path = std::env::var("CHLOEDB_BENCH_OUT").unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/results/graph/cluster.json").into());
    if let Some(d) = std::path::Path::new(&path).parent() {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::write(&path, serde_json::to_string_pretty(&report).unwrap() + "\n").unwrap();
}
