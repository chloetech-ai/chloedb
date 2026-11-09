//! The index inspector (`GET /v1/index`): a table's statistics, freshness
//! and read counts on one server, and on a coordinator for every replica of
//! every shard, every shard, and the whole cluster, with each document
//! retrieved and each key asked for counted once, at the replica that
//! answered it.

use std::sync::Arc;
use std::time::Duration;

use reqwest::Method;
use serde_json::{json, Value as Json};
use chloedb::db::Db;
use chloedb::storage::DbOptions;
use chloedb::Value;
use chloedb_server::coord::Coordinator;
use chloedb_server::service::{shard_of, AgentConfig, Backend, Service};
use chloedb_server::{demo, server};
use tokio::net::TcpListener;

const N: usize = 1500;
const SEARCH: &str = "SELECT id FROM products ORDER BY bm25(body, 'laptop phone camera') DESC LIMIT 10";

fn key(i: usize) -> String {
    format!("p{i:07}")
}

fn open(dir: &std::path::Path, name: &str, keep: impl Fn(usize) -> bool) -> Arc<Db> {
    let opts = DbOptions { publish_snapshots: true, segment_max_docs: 400, ..DbOptions::default() };
    let db = Db::create(&dir.join(name), demo::schema(N), opts).unwrap();
    db.set_embedder("emb", Arc::new(chloedb::embed::HashEmbedder { dim: demo::DIM })).unwrap();
    db.load((0..N).filter(|&i| keep(i)).map(|i| demo::product(i, 7))).unwrap();
    Arc::new(db)
}

fn service(backend: Backend, schema: chloedb::Schema) -> Arc<Service> {
    let agent = AgentConfig { api_key: None, model: String::new(), base_url: String::new() };
    Arc::new(Service::new(backend, schema, agent, 16, Duration::from_secs(60)))
}

async fn listen() -> (TcpListener, String) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
    (l, url)
}

async fn call(method: Method, url: String, body: Option<Json>) -> (u16, Json) {
    let mut r = chloedb_server::http_client().build().unwrap().request(method, url);
    if let Some(b) = body {
        r = r.json(&b);
    }
    let r = r.send().await.unwrap();
    let status = r.status().as_u16();
    let text = r.text().await.unwrap();
    (status, serde_json::from_str(&text).unwrap_or(Json::Null))
}

/// Rows a search through `base` answered.
async fn search(base: &str) -> u64 {
    let (status, j) = call(Method::POST, format!("{base}/v1/query"), Some(json!({"sql": SEARCH}))).await;
    assert_eq!(status, 200, "{j}");
    j["rows"].as_array().unwrap().len() as u64
}

async fn index(base: &str) -> Json {
    let (status, j) = call(Method::GET, format!("{base}/v1/index?table=products&sample=400"), None).await;
    assert_eq!(status, 200, "{j}");
    j
}

fn field<'a>(scope: &'a Json, name: &str) -> &'a Json {
    scope["stats"]["fields"].as_array().unwrap().iter().find(|f| f["name"] == name).unwrap_or_else(|| panic!("no field {name}: {scope}"))
}

#[tokio::test(flavor = "multi_thread")]
async fn a_server_inspects_its_table_and_counts_its_reads() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path(), "one.strata", |_| true);
    let schema = (*db.schema()).clone();
    let (l, base) = listen().await;
    tokio::spawn(server::serve_http(service(Backend::Local(db), schema), l, Default::default()));

    let j = index(&base).await;
    assert_eq!(j["role"], "server");
    assert_eq!(j["table"], "products");
    assert_eq!(j["shards"], json!([]));
    assert_eq!(j["reads_on"], true);
    let all = &j["cluster"];
    assert_eq!(all["stats"]["docs"]["live"].as_u64(), Some(N as u64), "{all}");
    assert_eq!(all["stats"]["sample"]["docs"].as_u64().unwrap() >= 380, true, "{}", all["stats"]["sample"]);
    let total = all["stats"]["bytes"]["total"].as_u64().unwrap();
    assert!(total > 0 && all["copies_bytes"].as_u64() == Some(total), "{all}");
    // The demo's fields: a stored title, a generated body that is not stored,
    // keywords, numbers, a vector.
    assert_eq!(field(all, "title")["stored"], true);
    let body = field(all, "body");
    assert_eq!(body["stored"], false);
    assert_eq!(body["present_from"], "norms");
    assert!(body["tokens"]["mean"].as_f64().unwrap() > 1.0, "{body}");
    assert!(body["terms"]["estimate"].as_f64().unwrap() > 10.0, "{body}");
    assert!(body["terms"].get("sketch").is_none(), "an answer for people carries no sketch: {body}");
    assert!(body["index_bytes"].as_u64().unwrap() > 0);
    let emb = field(all, "emb");
    assert_eq!(emb["kind"], "vector");
    assert_eq!(emb["size"]["p50"].as_f64(), Some(demo::DIM as f64 * 4.0), "{emb}");
    assert!(field(all, "price")["max"].as_f64().unwrap() > field(all, "price")["min"].as_f64().unwrap());
    assert!(all["stats"]["doc_bytes"]["p50"].as_f64().unwrap() > 0.0);
    // Freshness: the load is the last write.
    let fresh = &all["freshness"];
    assert!(fresh["last_write_ms"].as_u64().unwrap() > 0 && fresh["writes"].as_u64().unwrap() >= N as u64, "{fresh}");
    assert!(fresh["modified_ms"].as_u64().is_some() && fresh["started_ms"].as_u64().is_some(), "{fresh}");
    assert_eq!(all["reads"]["docs"]["total"], 0);

    // Searches count the documents they return; reads by key their keys,
    // found or not.
    let mut rows = 0;
    for _ in 0..3 {
        rows += search(&base).await;
    }
    assert!(rows > 0);
    for k in [key(3), key(3), key(3), key(8), "nope".into()] {
        call(Method::GET, format!("{base}/v1/kv/products/{k}"), None).await;
    }
    let (status, _) = call(Method::POST, format!("{base}/v1/kv/get"), Some(json!({"table": "products", "keys": [key(8)]}))).await;
    assert_eq!(status, 200);
    let j = index(&base).await;
    let docs = &j["cluster"]["reads"]["docs"];
    assert_eq!(docs["total"].as_u64(), Some(rows), "{docs}");
    assert_eq!(docs["answers"].as_u64(), Some(3), "{docs}");
    // The same search three times: every document it returns, three times.
    assert_eq!(docs["top"][0]["count"].as_u64(), Some(3), "{docs}");
    assert_eq!(docs["distinct"].as_f64(), Some((rows / 3) as f64), "{docs}");
    assert_eq!(docs["exact"], true);
    let coverage = docs["coverage"].as_f64().unwrap();
    assert!((coverage - (rows / 3) as f64 / N as f64).abs() < 1e-9, "{docs}");
    let keys = &j["cluster"]["reads"]["keys"];
    assert_eq!(keys["total"].as_u64(), Some(6), "{keys}");
    assert_eq!(keys["missing"].as_u64(), Some(1), "{keys}");
    assert_eq!(keys["top"][0]["key"], key(3), "{keys}");
    assert_eq!(keys["top"][0]["count"].as_u64(), Some(3), "{keys}");
    assert_eq!(keys["top"][1]["key"], key(8), "{keys}");
    assert_eq!(keys["top"][1]["count"].as_u64(), Some(2), "{keys}");

    // A write moves the freshness.
    let before = j["cluster"]["freshness"]["writes"].as_u64().unwrap();
    let (status, w) = call(Method::POST, format!("{base}/v1/sql"), Some(json!({"sql": "INSERT INTO products (id, title, price) VALUES ('fresh1', 'A new tent', 12.5)"}))).await;
    assert_eq!(status, 200, "{w}");
    let j = index(&base).await;
    assert_eq!(j["cluster"]["freshness"]["writes"].as_u64(), Some(before + 1));
    assert_eq!(j["cluster"]["stats"]["docs"]["live"].as_u64(), Some(N as u64 + 1));

    // Counting starts again.
    let (status, _) = call(Method::POST, format!("{base}/v1/index/reads/reset"), None).await;
    assert_eq!(status, 200);
    let j = index(&base).await;
    assert_eq!(j["cluster"]["reads"]["docs"]["total"], 0);
    assert_eq!(j["cluster"]["reads"]["keys"]["total"], 0);

    // Bad parameters say what is wrong.
    let (status, e) = call(Method::GET, format!("{base}/v1/index?sample=lots"), None).await;
    assert_eq!(status, 400, "{e}");
    let (status, e) = call(Method::GET, format!("{base}/v1/index?table=nothing"), None).await;
    assert!(status >= 400, "{e}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_coordinator_inspects_every_replica_shard_and_the_cluster() {
    let dir = tempfile::tempdir().unwrap();
    let mine = |s: usize| move |i: usize| shard_of(&Value::Str(key(i)), 2) == s;
    // Shard 0 in two copies, shard 1 in one; the first copy of shard 0 also
    // answers its own clients over HTTP.
    let mut grpc = Vec::new();
    let mut direct = None;
    for (s, name) in [(0, "s0a"), (0, "s0b"), (1, "s1")] {
        let db = open(dir.path(), &format!("{name}.strata"), mine(s));
        let schema = (*db.schema()).clone();
        let svc = service(Backend::Local(db), schema);
        let (l, url) = listen().await;
        tokio::spawn(server::serve_grpc(svc.clone(), l, Default::default()));
        grpc.push(url);
        if name == "s0a" {
            let (l, url) = listen().await;
            tokio::spawn(server::serve_http(svc, l, Default::default()));
            direct = Some(url);
        }
    }
    let direct = direct.unwrap();
    let c = Coordinator::connect(vec![format!("{}|{}", grpc[0], grpc[1]), grpc[2].clone()]).await.unwrap();
    let schema = c.schema.clone();
    let (l, base) = listen().await;
    tokio::spawn(server::serve_http(service(Backend::Cluster(Arc::new(c)), schema), l, Default::default()));
    let in0 = (0..N).filter(|&i| mine(0)(i)).count() as u64;

    let j = index(&base).await;
    assert_eq!(j["role"], "coordinator");
    let shards = j["shards"].as_array().unwrap();
    assert_eq!(shards.len(), 2);
    assert_eq!(shards[0]["replicas"].as_array().unwrap().len(), 2);
    assert_eq!(shards[1]["replicas"].as_array().unwrap().len(), 1);
    assert_eq!(j["cluster"]["stats"]["docs"]["live"].as_u64(), Some(N as u64), "{}", j["cluster"]["stats"]["docs"]);
    assert_eq!(shards[0]["scope"]["stats"]["docs"]["live"].as_u64(), Some(in0));
    assert_eq!(shards[1]["scope"]["stats"]["docs"]["live"].as_u64(), Some(N as u64 - in0));
    let mut copies = 0;
    for shard in shards {
        for r in shard["replicas"].as_array().unwrap() {
            assert!(r.get("error").is_none(), "{r}");
            // Copies with no sequence numbers: how far behind is not known.
            assert!(r["behind"].is_null() || r["behind"] == 0, "{r}");
            assert_eq!(r["scope"]["stats"]["docs"]["live"], shard["scope"]["stats"]["docs"]["live"], "{r}");
            copies += r["scope"]["stats"]["bytes"]["total"].as_u64().unwrap();
        }
    }
    let cluster_bytes = j["cluster"]["stats"]["bytes"]["total"].as_u64().unwrap();
    assert_eq!(j["cluster"]["copies_bytes"].as_u64(), Some(copies));
    assert!(copies > cluster_bytes, "three copies of two shards: {copies} vs {cluster_bytes}");
    // Distinct terms merge across the shards: no more than the two shards' sum,
    // no fewer than either.
    let terms = |s: &Json| field(s, "body")["terms"]["estimate"].as_f64().unwrap();
    let (t0, t1, all) = (terms(&shards[0]["scope"]), terms(&shards[1]["scope"]), terms(&j["cluster"]));
    assert!(all >= t0.max(t1) * 0.97 && all <= (t0 + t1) * 1.03, "{t0} + {t1} -> {all}");

    // Searches through the coordinator, counted once, at the copy each
    // shard's part came from.
    let mut rows = 0;
    for _ in 0..4 {
        rows += search(&base).await;
    }
    for k in [key(1), key(1), key(1), key(2), "nope".into()] {
        call(Method::GET, format!("{base}/v1/kv/products/{k}"), None).await;
    }
    // And a search a copy answers its own client.
    let own = search(&direct).await;
    let j = index(&base).await;
    let docs = &j["cluster"]["reads"]["docs"];
    assert_eq!(docs["total"].as_u64(), Some(rows + own), "{docs}");
    let shards = j["shards"].as_array().unwrap();
    let total = |s: &Json| s["scope"]["reads"]["docs"]["total"].as_u64().unwrap();
    assert_eq!(total(&shards[0]) + total(&shards[1]), rows + own);
    for shard in shards {
        let by_copy: u64 = shard["replicas"].as_array().unwrap().iter().map(total).sum();
        assert_eq!(by_copy, total(shard), "{shard}");
    }
    let s0a = &shards[0]["replicas"][0];
    assert!(total(s0a) >= own, "{s0a}");
    let keys = &j["cluster"]["reads"]["keys"];
    assert_eq!(keys["total"].as_u64(), Some(5), "{keys}");
    assert_eq!(keys["missing"].as_u64(), Some(1), "{keys}");
    assert_eq!(keys["top"][0]["key"], key(1), "{keys}");
    assert_eq!(keys["top"][0]["count"].as_u64(), Some(3), "{keys}");
    let key_total = |s: &Json| s["scope"]["reads"]["keys"]["total"].as_u64().unwrap();
    let k1 = shard_of(&Value::Str(key(1)), 2);
    assert!(key_total(&shards[k1]) >= 3, "{}", shards[k1]["scope"]["reads"]["keys"]);

    // The copy itself counts only what it answered its own client, not the
    // coordinator's questions.
    let mine = index(&direct).await;
    assert_eq!(mine["cluster"]["reads"]["docs"]["total"].as_u64(), Some(own), "{}", mine["cluster"]["reads"]["docs"]);
    assert_eq!(mine["cluster"]["reads"]["keys"]["total"], 0);
}

/// Through a coordinator, every table's reads a minute are of the last
/// minutes and name each shard (which shard is hot), and the drill-down of
/// one table (`/v1/index`) measures them over the same window.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn each_shards_reads_a_minute_are_the_last_minutes_and_the_drill_down_agrees() {
    use chloedb_server::service::QueryReq;
    let dir = tempfile::tempdir().unwrap();
    // Shard 0 keeps all of its keys, shard 1 a quarter of its.
    let mut grpc = Vec::new();
    for s in 0..2 {
        let db = open(dir.path(), &format!("r{s}.strata"), move |i| shard_of(&Value::Str(key(i)), 2) == s && (s == 0 || i % 4 == 0));
        let schema = (*db.schema()).clone();
        let (l, url) = listen().await;
        tokio::spawn(server::serve_grpc(service(Backend::Local(db), schema), l, Default::default()));
        grpc.push(url);
    }
    let c = Arc::new(Coordinator::connect(grpc).await.unwrap());
    let svc = service(Backend::Cluster(c.clone()), c.schema.clone());
    // A look, as if a minute ago.
    let t = chloedb_server::inspect::tables(&svc).await.unwrap();
    assert!(t["tables"][0]["shards"][0].get("reads_per_min").is_some(), "{t}");
    for look in svc.tables_counts.lock().unwrap().iter_mut() {
        look.0 -= 60_000;
    }
    // Every row read, twice.
    for _ in 0..2 {
        svc.query(QueryReq { sql: format!("SELECT id FROM products LIMIT {}", 2 * N), ..Default::default() }).await.unwrap();
    }
    *svc.tables_kept.lock().await = None;
    let t = chloedb_server::inspect::tables(&svc).await.unwrap();
    let p = t["tables"].as_array().unwrap().iter().find(|t| t["table"] == "products").cloned().unwrap_or_else(|| panic!("{t}"));
    assert_eq!(p["rate_window_s"].as_u64(), Some(60), "{t}");
    let shard = |i: usize| p["shards"][i]["reads_per_min"].as_f64().unwrap_or_else(|| panic!("shard {i} has no reads a minute: {t}"));
    let all = p["reads_per_min"].as_f64().unwrap();
    assert!(all > 0.0 && (shard(0) + shard(1) - all).abs() < 0.2, "the shards add up to the table: {t}");
    assert!(shard(0) > shard(1), "{t}");
    // The drill-down, over the same window.
    let d = chloedb_server::inspect::answer(&svc, chloedb_server::inspect::Asked::of(&json!({"table": "products", "sample": 0})).unwrap()).await.unwrap();
    assert_eq!(d["rates"]["window_s"].as_u64(), Some(60), "{}", d["rates"]);
    // Measured a few milliseconds later: the same within a tenth of a percent.
    let near = |a: Option<f64>, b: f64| a.is_some_and(|a| (a - b).abs() <= b * 1e-3 + 0.1);
    assert!(near(d["rates"]["reads_per_min"].as_f64(), all), "{} against {all}", d["rates"]);
    assert!(near(d["rates"]["shards"]["0"]["reads_per_min"].as_f64(), shard(0)), "{} against {}", d["rates"], shard(0));
}
