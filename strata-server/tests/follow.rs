//! A cluster that follows another (`strata-server follow`, shard protocol
//! 14): it copies the source's rows, tables and rank profiles, then
//! applies each source shard's feed in an order that keeps every row's
//! last version, while slots move on both sides, tables change and the
//! follow process is killed and started again; and while it follows, the
//! follower takes no other client's writes until following stops.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value as Json};
use strata::db::Db;
use strata::query::Params;
use strata::storage::DbOptions;
use strata_server::auth::Access;
use strata_server::coord::Coordinator;
use strata_server::follow::{self, Options};
use strata_server::keys::Principal;
use strata_server::replication::Acks;
use strata_server::service::{After, AgentConfig, Backend, QueryReq, Service};
use strata_server::{demo, replication, server, slots};
use tokio::net::TcpListener;

const N: usize = 300;

fn agent() -> AgentConfig {
    AgentConfig { api_key: None, model: strata_server::agent::DEFAULT_MODEL.into(), base_url: "http://unused".into() }
}

/// A numbered database at `dir/name` of the demo products `keep` keeps.
fn create(dir: &Path, name: &str, keep: impl Fn(usize) -> bool) -> Arc<Db> {
    let opts = DbOptions { publish_snapshots: true, ship_log_bytes: 16 << 20, ..DbOptions::default() };
    let db = Db::create(&dir.join(name), demo::schema(N), opts).unwrap();
    db.set_embedder("emb", Arc::new(strata::embed::HashEmbedder { dim: demo::DIM })).unwrap();
    let docs: Vec<_> = (0..N).filter(|&i| keep(i)).map(|i| demo::product(i, 7)).collect();
    if !docs.is_empty() {
        db.load(docs).unwrap();
    }
    db.storage().start_seq_no().unwrap();
    Arc::new(db)
}

/// A shard over gRPC.
async fn shard(dir: &Path, name: &str, keep: impl Fn(usize) -> bool) -> (Arc<Service>, String) {
    let db = create(dir, name, keep);
    let path = dir.join(name);
    let schema = (*db.schema()).clone();
    let mut svc = Service::new(Backend::Local(db), schema, agent(), 16, Duration::from_secs(10));
    svc.keep_slots_in(slots::file_of(&path)).unwrap();
    svc.keep_replication_in(replication::file_of(&path));
    let svc = Arc::new(svc);
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(server::serve_grpc(svc.clone(), l, Access::default()));
    (svc, url)
}

/// A coordinator over `urls`, served over HTTP.
async fn coordinator(urls: &[String]) -> (Arc<Coordinator>, Arc<Service>, String) {
    let c = Arc::new(Coordinator::connect(urls.to_vec()).await.unwrap());
    let schema = c.schema.clone();
    let svc = Arc::new(Service::new(Backend::Cluster(c.clone()), schema, agent(), 16, Duration::from_secs(10)));
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(server::serve_http(svc.clone(), l, Access::default()));
    (c, svc, url)
}

async fn sql(svc: &Arc<Service>, s: &str) {
    if let Err(e) = svc.sql(&Principal::open(), s, &Params::new(), After::Any, 0).await {
        panic!("{s}: {}", e.message);
    }
}

async fn job_done(c: &Coordinator) {
    let t0 = Instant::now();
    loop {
        let m = c.moves();
        if m["job"]["state"] != "running" {
            assert_eq!(m["job"]["state"], "done", "{m}");
            return;
        }
        assert!(t0.elapsed() < Duration::from_secs(120), "the job of moves runs still: {m}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Every row a cluster holds that the test compares, by table.
async fn rows(svc: &Arc<Service>) -> BTreeMap<&'static str, Vec<String>> {
    let mut out = BTreeMap::new();
    for (table, q) in [
        ("products", "SELECT id, title, sold, price FROM products ORDER BY id LIMIT 100000"),
        ("events", "SELECT id, usr, n, note FROM events ORDER BY id LIMIT 100000"),
        ("notes", "SELECT id, body, stars FROM notes ORDER BY id LIMIT 100000"),
    ] {
        let rs = match svc.query(QueryReq { sql: q.into(), ..Default::default() }).await {
            Ok(a) => a.rs.rows.iter().map(|r| format!("{r:?}")).collect(),
            Err(e) => vec![format!("error: {}", e.message)],
        };
        out.insert(table, rs);
    }
    out
}

/// Waits until `follower` holds the rows `source` does.
async fn caught_up(source: &Arc<Service>, follower: &Arc<Service>, view: impl Fn() -> futures::future::BoxFuture<'static, Json>) {
    let want = rows(source).await;
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let have = rows(follower).await;
        if have == want {
            return;
        }
        if Instant::now() >= deadline {
            for (t, w) in &want {
                let h = &have[t];
                let missing: Vec<&String> = w.iter().filter(|r| !h.contains(r)).take(5).collect();
                let extra: Vec<&String> = h.iter().filter(|r| !w.contains(r)).take(5).collect();
                eprintln!("{t}: {} rows here, {} there; missing {missing:?}; extra {extra:?}", w.len(), h.len());
            }
            panic!("the follower did not catch up: {}", view().await);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn options(source: &str, target: &str) -> Options {
    Options { source: source.into(), source_key: None, source_ca: None, target: target.into(), target_token: None, target_ca: None, name: "east".into(), batch: 40, copy: false }
}

async fn get(url: &str) -> Json {
    strata_server::http_client().build().unwrap().get(url).send().await.unwrap().json().await.unwrap()
}

async fn post(url: &str, body: Json, follow: Option<&str>) -> (u16, Json) {
    let mut r = strata_server::http_client().build().unwrap().post(url).json(&body);
    if let Some(f) = follow {
        r = r.header("x-strata-follow", f);
    }
    let r = r.send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or(Json::Null))
}

/// Writes through `svc` until `stop`: rows by key (upserts, deletes, a
/// column set), and changes of a column of every row, in both tables.
fn writers(svc: &Arc<Service>, stop: &Arc<AtomicBool>, tag: &'static str) -> Vec<tokio::task::JoinHandle<u64>> {
    let mut tasks = Vec::new();
    {
        let (svc, stop) = (svc.clone(), stop.clone());
        tasks.push(tokio::spawn(async move {
            let mut r = 0u64;
            while !stop.load(Ordering::Relaxed) {
                let k = format!("w{:03}", r % 97);
                let q = match r % 9 {
                    3 => format!("DELETE FROM products WHERE id = '{k}'"),
                    5 => format!("UPDATE events SET n = n + 1 WHERE id = {}", r % 200),
                    7 => format!("DELETE FROM events WHERE id = {}", 200 + r % 50),
                    // A key keeps its place (shard_by): one that moves to
                    // another value is a limit of following.
                    8 => format!("INSERT OR REPLACE INTO events (id, usr, n) VALUES ({}, 'u{}', {r})", 200 + r % 50, (200 + r % 50) % 11),
                    _ => format!("INSERT OR REPLACE INTO products (id, title, description, price) VALUES ('{k}', '{tag} kettle', 'Round {r}', {r})"),
                };
                sql(&svc, &q).await;
                r += 1;
            }
            r
        }));
    }
    {
        let (svc, stop) = (svc.clone(), stop.clone());
        tasks.push(tokio::spawn(async move {
            let mut n = 0;
            while !stop.load(Ordering::Relaxed) {
                sql(&svc, "UPDATE products SET sold = sold + 1 WHERE price < 50").await;
                n += 1;
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            n
        }));
    }
    tasks
}

async fn stop_writers(stop: &Arc<AtomicBool>, tasks: Vec<tokio::task::JoinHandle<u64>>) {
    stop.store(true, Ordering::Relaxed);
    for t in tasks {
        assert!(t.await.unwrap() > 0);
    }
    stop.store(false, Ordering::Relaxed);
}

/// A source of one shard that becomes two, followed by a cluster of two
/// shards that becomes three: the follower copies, follows while slots
/// move on each side and rows are written, changed and deleted (a table
/// placed by a column among them), while a table gains a column and
/// another is dropped and made again, and while its follow process is
/// killed twice (once in the copy) and started again; it ends with the
/// source's rows. Meanwhile it refuses its clients' writes, until it
/// stops following, which stops its follow process too.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_cluster_follows_another_through_moves_changes_and_kills() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    // The source: one shard of the demo products, then a second.
    let (_s0, u0) = shard(d, "s0.strata", |_| true).await;
    let (a, svc_a, url_a) = coordinator(&[u0]).await;
    sql(&svc_a, "CREATE TABLE events (id INT PRIMARY KEY, usr KEYWORD NOT NULL, n INT) OPTIONS(shard_by = 'usr')").await;
    let values: Vec<String> = (0..200).map(|i| format!("({i}, 'u{}', {i})", i % 13)).collect();
    sql(&svc_a, &format!("INSERT INTO events (id, usr, n) VALUES {}", values.join(", "))).await;
    sql(&svc_a, "CREATE TABLE notes (id INT PRIMARY KEY, body TEXT)").await;
    sql(&svc_a, "INSERT INTO notes (id, body) VALUES (1, 'old one'), (2, 'old two')").await;
    sql(&svc_a, "CREATE RANK PROFILE cheap ON products AS SELECT * FROM products ORDER BY price ASC").await;

    // The follower: two shards, empty.
    let (_t0, v0) = shard(d, "t0.strata", |_| false).await;
    let (b, svc_b, url_b) = coordinator(&[v0]).await;
    let (_t1, v1) = shard(d, "t1.strata", |_| false).await;
    b.add_shard(vec![v1], Acks::All).await.unwrap();
    b.rebalance(512).unwrap();
    job_done(&b).await;

    let view_url = format!("{url_b}/v1/admin/follow");
    let view = {
        let u = view_url.clone();
        move || -> futures::future::BoxFuture<'static, Json> {
            let u = u.clone();
            Box::pin(async move { get(&u).await })
        }
    };
    // Killed in its copy, then started again: it copies again, as rows
    // are written.
    let f = tokio::spawn(follow::run(options(&url_a, &url_b)));
    tokio::time::sleep(Duration::from_millis(150)).await;
    f.abort();
    let _ = f.await;
    let stop = Arc::new(AtomicBool::new(false));
    let w = writers(&svc_a, &stop, "first");
    let f = tokio::spawn(follow::run(options(&url_a, &url_b)));
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let v = view().await;
        if v["clusters"][0]["copied"] == true {
            assert_eq!(v["following"], "east", "{v}");
            break;
        }
        if f.is_finished() {
            panic!("the follow process ended: {:?}", f.await.unwrap());
        }
        assert!(Instant::now() < deadline, "no copy: {v}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // While it follows, its clients' writes are refused; and a client may
    // not pass as its follow process.
    let one = json!({"ops": [{"op": "upsert", "table": "notes", "doc": {"id": 9, "body": "mine"}}]});
    let (status, e) = post(&format!("{url_b}/v1/write"), one.clone(), None).await;
    assert_eq!(status, 409, "{e}");
    assert!(e["error"].as_str().unwrap().contains("follows east"), "{e}");
    match svc_b.sql(&Principal::open(), "DELETE FROM products WHERE id = 'p0001'", &Params::new(), After::Any, 0).await {
        Err(e) => assert_eq!(e.status, 409, "{}", e.message),
        Ok(_) => panic!("a DELETE on the follower was taken"),
    }
    let (status, _) = post(&format!("{url_b}/v1/write"), one.clone(), Some("west")).await;
    assert_eq!(status, 409);

    // Slots move on the source onto a shard it adds, as rows are written.
    let (_s1, u1) = shard(d, "s1.strata", |_| false).await;
    a.add_shard(vec![u1], Acks::All).await.unwrap();
    a.rebalance(512).unwrap();
    job_done(&a).await;
    stop_writers(&stop, w).await;
    caught_up(&svc_a, &svc_b, &view).await;
    let v = view().await;
    assert_eq!(v["clusters"][0]["applied"].as_array().unwrap().len(), 2, "both source shards: {v}");
    assert_eq!(v["clusters"][0]["status"]["state"], "following", "{v}");

    // Tables change: a column added, a table dropped and made again with
    // other columns, a rank profile replaced; the follow process is killed
    // and started again as rows are written.
    let w = writers(&svc_a, &stop, "second");
    sql(&svc_a, "ALTER TABLE events ADD COLUMN note TEXT").await;
    sql(&svc_a, "UPDATE events SET note = 'noted' WHERE id < 20").await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    f.abort();
    let _ = f.await;
    sql(&svc_a, "DROP TABLE notes").await;
    sql(&svc_a, "CREATE TABLE notes (id INT PRIMARY KEY, body TEXT, stars INT)").await;
    sql(&svc_a, "INSERT INTO notes (id, body, stars) VALUES (1, 'new one', 5), (3, 'new three', 4)").await;
    sql(&svc_a, "CREATE OR REPLACE RANK PROFILE cheap ON products AS SELECT * FROM products ORDER BY price DESC").await;
    let f = tokio::spawn(follow::run(options(&url_a, &url_b)));

    // Slots move on the follower onto a shard it adds, as it follows, once
    // it changed its tables and rank profile (a shard added is given the
    // tables and profiles as the coordinator serves them when it starts: a
    // change of them meanwhile would miss it).
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let t: Json = get(&format!("{url_b}/v1/tables")).await;
        let sql = |name: &str| t["tables"].as_array().unwrap().iter().find(|x| x["table"] == name).map_or(String::new(), |x| x["sql"].as_str().unwrap().to_string());
        let profiles = t["tables"].as_array().unwrap().iter().find(|x| x["table"] == "products").map_or(String::new(), |x| x["profiles"].to_string());
        if sql("events").contains("note") && sql("notes").contains("stars") && profiles.contains("DESC") {
            break;
        }
        assert!(Instant::now() < deadline, "the follower did not change its tables: {}", view().await);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let (_t2, v2) = shard(d, "t2.strata", |_| false).await;
    b.add_shard(vec![v2], Acks::All).await.unwrap();
    b.rebalance(512).unwrap();
    job_done(&b).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    stop_writers(&stop, w).await;
    caught_up(&svc_a, &svc_b, &view).await;
    let tables: Json = get(&format!("{url_b}/v1/tables")).await;
    let schema = |name: &str| tables["tables"].as_array().unwrap().iter().find(|t| t["table"] == name).map(|t| t["sql"].clone());
    let theirs: Json = get(&format!("{url_a}/v1/tables")).await;
    for t in ["products", "events", "notes"] {
        assert_eq!(schema(t), theirs["tables"].as_array().unwrap().iter().find(|x| x["table"] == t).map(|x| x["sql"].clone()), "table {t}");
    }
    let profiles = tables["tables"].as_array().unwrap().iter().find(|t| t["table"] == "products").unwrap()["profiles"].clone();
    assert!(profiles.to_string().contains("DESC"), "the rank profile as the source has it: {profiles}");

    // Stopped, it takes its clients' writes, and refuses the follow
    // process's, which stops.
    let (status, r) = post(&format!("{url_b}/v1/admin/follow/stop"), json!({"name": "east"}), None).await;
    assert_eq!(status, 200, "{r}");
    let (status, e) = post(&format!("{url_b}/v1/write"), one.clone(), None).await;
    assert_eq!(status, 200, "{e}");
    sql(&svc_a, "INSERT INTO notes (id, body, stars) VALUES (4, 'after the stop', 1)").await;
    let ended = tokio::time::timeout(Duration::from_secs(30), f).await.expect("the follow process stops").unwrap();
    let e = ended.unwrap_err();
    assert!(e.message.contains("stopped following east"), "{}", e.message);
    let v = view().await;
    assert!(v["following"].is_null() && v["clusters"][0]["stopped_ms"].as_u64().is_some(), "{v}");
}
