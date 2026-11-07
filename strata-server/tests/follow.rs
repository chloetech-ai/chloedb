//! A cluster that follows another (`strata-server follow`, shard protocol
//! 14): it copies the source's rows, tables and rank profiles, then
//! applies each source shard's feed in an order that keeps every row's
//! last version, while slots move on both sides, tables change and the
//! follow process is killed and started again; and while it follows, the
//! follower takes no other client's writes until following stops. One
//! that fell behind the source's log catches up (protocol 15), also when
//! its catch-up is cut short, and a copy made again drops the rows the
//! source deleted meanwhile. A switch-over (protocol 16) holds the
//! source's writes and promotes the follower having lost none, after which
//! the old source follows it back without a copy; after one that was not
//! planned, the old source rejoins with a copy that deletes the keys it
//! took meanwhile.

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

/// A numbered database at `dir/name` of the demo products `keep` keeps,
/// whose log keeps `log` bytes of its last transactions.
fn create(dir: &Path, name: &str, keep: impl Fn(usize) -> bool, log: usize) -> Arc<Db> {
    let opts = DbOptions { publish_snapshots: true, ship_log_bytes: log, ..DbOptions::default() };
    let db = Db::create(&dir.join(name), demo::schema(N), opts).unwrap();
    db.set_embedder("emb", Arc::new(strata::embed::HashEmbedder { dim: demo::DIM })).unwrap();
    let docs: Vec<_> = (0..N).filter(|&i| keep(i)).map(|i| demo::product(i, 7)).collect();
    if !docs.is_empty() {
        db.load(docs).unwrap();
    }
    db.storage().start_seq_no().unwrap();
    Arc::new(db)
}

/// The bytes of a log of the source's shards that the follower falls
/// behind of, while it is stopped.
const SMALL_LOG: usize = 32 << 10;

/// A shard over gRPC.
async fn shard(dir: &Path, name: &str, keep: impl Fn(usize) -> bool) -> (Arc<Service>, String) {
    shard_with(dir, name, keep, 16 << 20).await
}

/// A shard over gRPC whose log keeps `log` bytes.
async fn shard_with(dir: &Path, name: &str, keep: impl Fn(usize) -> bool, log: usize) -> (Arc<Service>, String) {
    let db = create(dir, name, keep, log);
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

/// The follower's view of what it follows (`GET /v1/admin/follow`).
fn view_of(url: &str) -> impl Fn() -> futures::future::BoxFuture<'static, Json> + Clone {
    let u = format!("{url}/v1/admin/follow");
    move || -> futures::future::BoxFuture<'static, Json> {
        let u = u.clone();
        Box::pin(async move { get(&u).await })
    }
}

/// Waits until the follow process `f` made its copy.
async fn copied(view: &impl Fn() -> futures::future::BoxFuture<'static, Json>, f: &tokio::task::JoinHandle<Result<(), strata_server::ApiError>>) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let v = view().await;
        if v["clusters"][0]["copied"] == true {
            return;
        }
        assert!(!f.is_finished(), "the follow process ended: {v}");
        assert!(Instant::now() < deadline, "no copy: {v}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// What the follow process says once it follows from its last copy.
async fn settled(view: &impl Fn() -> futures::future::BoxFuture<'static, Json>) -> Json {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let v = view().await;
        let c = &v["clusters"][0];
        if c["status"]["state"] == "following" && c["status"]["copy_ms"] == c["copy_ms"] {
            return c["status"].clone();
        }
        assert!(Instant::now() < deadline, "it does not follow: {v}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// The marks the follower keeps of its last copy (`follow/east/<copy>/`),
/// by name, on its first shard.
async fn marks(url: &str) -> BTreeMap<String, i64> {
    let j = get(&format!("{url}/v1/sources?shards=true")).await;
    let all: Vec<(String, i64)> = j["shards"][0].as_array().unwrap().iter().map(|s| (s["name"].as_str().unwrap().to_string(), s["offset"].as_i64().unwrap())).collect();
    let copy = all.iter().find(|(n, _)| n == "follow/east/copy").map(|(_, o)| *o).unwrap();
    let prefix = format!("follow/east/{copy}/");
    all.into_iter().filter_map(|(n, o)| Some((n.strip_prefix(&prefix)?.to_string(), o))).collect()
}

/// Whether source shard `i`'s feed from transaction `from` is gone from
/// its log.
async fn gone(url: &str, i: usize, from: u64) -> bool {
    let r = strata_server::http_client().build().unwrap().get(format!("{url}/v1/feed?shard={i}&from={from}")).send().await.unwrap();
    let mut body = r.bytes_stream();
    use futures::StreamExt;
    let first = tokio::time::timeout(Duration::from_secs(5), body.next()).await.ok().flatten().map(|b| String::from_utf8_lossy(&b.unwrap()).to_string()).unwrap_or_default();
    first.contains("\"gone\"")
}

fn options(source: &str, target: &str) -> Options {
    Options { source: source.into(), source_key: None, source_ca: None, target: target.into(), target_token: None, target_ca: None, name: "east".into(), batch: 40, copy: false, reverse: false }
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

    let view = view_of(&url_b);
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

/// The source's tables as the follower compares them, and the rows of
/// those the test makes besides [`rows`].
async fn shape(url: &str, svc: &Arc<Service>) -> (Vec<String>, Vec<String>) {
    let t: Json = get(&format!("{url}/v1/tables")).await;
    let mut tables: Vec<String> = t["tables"].as_array().unwrap().iter().filter(|x| !x["table"].as_str().unwrap().starts_with('_')).map(|x| format!("{} {}", x["sql"], x["profiles"])).collect();
    tables.sort();
    let fresh = match svc.query(QueryReq { sql: "SELECT id, v FROM fresh ORDER BY id LIMIT 1000".into(), ..Default::default() }).await {
        Ok(a) => a.rs.rows.iter().map(|r| format!("{r:?}")).collect(),
        Err(e) => vec![format!("error: {}", e.message)],
    };
    (tables, fresh)
}

/// A source of two shards whose logs keep 32 KiB, with the tables the
/// catch-up tests change, and a follower of two shards, empty.
async fn small_logs(d: &Path) -> (Arc<Service>, String, Arc<Service>, String) {
    let (_s0, u0) = shard_with(d, "s0.strata", |_| true, SMALL_LOG).await;
    let (a, svc_a, url_a) = coordinator(&[u0]).await;
    sql(&svc_a, "CREATE TABLE events (id INT PRIMARY KEY, usr KEYWORD NOT NULL, n INT) OPTIONS(shard_by = 'usr')").await;
    let values: Vec<String> = (0..200).map(|i| format!("({i}, 'u{}', {i})", i % 13)).collect();
    sql(&svc_a, &format!("INSERT INTO events (id, usr, n) VALUES {}", values.join(", "))).await;
    sql(&svc_a, "CREATE TABLE notes (id INT PRIMARY KEY, body TEXT)").await;
    sql(&svc_a, "INSERT INTO notes (id, body) VALUES (1, 'old one'), (2, 'old two')").await;
    sql(&svc_a, "CREATE TABLE old (id INT PRIMARY KEY, v INT)").await;
    sql(&svc_a, "INSERT INTO old (id, v) VALUES (1, 1), (2, 2)").await;
    sql(&svc_a, "CREATE RANK PROFILE cheap ON products AS SELECT * FROM products ORDER BY price ASC").await;
    let (_s1, u1) = shard_with(d, "s1.strata", |_| false, SMALL_LOG).await;
    a.add_shard(vec![u1], Acks::All).await.unwrap();
    a.rebalance(512).unwrap();
    job_done(&a).await;
    let (_t0, v0) = shard(d, "t0.strata", |_| false).await;
    let (b, svc_b, url_b) = coordinator(&[v0]).await;
    let (_t1, v1) = shard(d, "t1.strata", |_| false).await;
    b.add_shard(vec![v1], Acks::All).await.unwrap();
    b.rebalance(512).unwrap();
    job_done(&b).await;
    (svc_a, url_a, svc_b, url_b)
}

/// Writes `n` rows of products by new keys (`c<i>`), 100 a transaction.
async fn new_products(svc: &Arc<Service>, n: usize, round: &str) {
    for chunk in (0..n).collect::<Vec<_>>().chunks(100) {
        let v: Vec<String> = chunk.iter().map(|i| format!("('c{i:05}', '{round} lamp {i}', 'A lamp of round {round}, number {i}, with a long description to fill the log', {})", i % 90)).collect();
        sql(svc, &format!("INSERT OR REPLACE INTO products (id, title, description, price) VALUES {}", v.join(", "))).await;
    }
}

/// While the follower is stopped, the source deletes rows (of a table
/// placed by a column too), adds a column to a table, drops a table and
/// makes it again with other columns, drops another, makes a new one,
/// replaces a rank profile, and writes more than its logs hold.
async fn change_meanwhile(svc_a: &Arc<Service>) {
    let ids: Vec<String> = (0..30).map(|i| format!("'p{:07}'", i * 7)).collect();
    sql(svc_a, &format!("DELETE FROM products WHERE id IN ({})", ids.join(", "))).await;
    sql(svc_a, "DELETE FROM events WHERE id < 25").await;
    sql(svc_a, "UPDATE products SET sold = sold + 3 WHERE price < 20").await;
    sql(svc_a, "ALTER TABLE events ADD COLUMN note TEXT").await;
    sql(svc_a, "UPDATE events SET note = 'noted' WHERE id < 60").await;
    sql(svc_a, "DROP TABLE notes").await;
    sql(svc_a, "CREATE TABLE notes (id INT PRIMARY KEY, body TEXT, stars INT)").await;
    sql(svc_a, "INSERT INTO notes (id, body, stars) VALUES (1, 'new one', 5), (3, 'new three', 4)").await;
    sql(svc_a, "DROP TABLE old").await;
    sql(svc_a, "CREATE TABLE fresh (id INT PRIMARY KEY, v TEXT)").await;
    sql(svc_a, "INSERT INTO fresh (id, v) VALUES (1, 'a'), (2, 'b'), (3, 'c')").await;
    sql(svc_a, "CREATE OR REPLACE RANK PROFILE cheap ON products AS SELECT * FROM products ORDER BY price DESC").await;
    new_products(svc_a, 1500, "meanwhile").await;
}

/// Waits until the follower holds the source's rows, tables and rank
/// profiles, the new table's rows among them.
async fn same(svc_a: &Arc<Service>, url_a: &str, svc_b: &Arc<Service>, url_b: &str) {
    let view = view_of(url_b);
    caught_up(svc_a, svc_b, &view).await;
    let want = shape(url_a, svc_a).await;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let have = shape(url_b, svc_b).await;
        if have == want {
            return;
        }
        assert!(Instant::now() < deadline, "the follower's tables are not the source's:\n{have:?}\nwhere the source has\n{want:?}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// A follower stopped while the source writes more than its shards' logs
/// hold: started again, it finds their feeds no longer reach what it
/// applied, and catches up: it copies the rows changed since (every row
/// of the tables that changed), deletes the keys the source deleted
/// meanwhile (the sweep), drops the table the source dropped, and follows
/// on; its lag is in its metrics.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_follower_behind_the_log_catches_up() {
    let dir = tempfile::tempdir().unwrap();
    let (svc_a, url_a, svc_b, url_b) = small_logs(dir.path()).await;
    let view = view_of(&url_b);
    let f = tokio::spawn(follow::run(options(&url_a, &url_b)));
    copied(&view, &f).await;
    same(&svc_a, &url_a, &svc_b, &url_b).await;
    f.abort();
    let _ = f.await;
    let v = view().await;
    let applied: Vec<u64> = v["clusters"][0]["applied"].as_array().unwrap().iter().map(|a| a["seq"].as_u64().unwrap()).collect();
    assert_eq!(applied.len(), 2, "{v}");

    change_meanwhile(&svc_a).await;
    let mut behind = 0;
    for (i, at) in applied.iter().enumerate() {
        behind += gone(&url_a, i, at + 1).await as usize;
    }
    assert_eq!(behind, 2, "the source's logs still hold what the follower applied: {applied:?}");

    let f = tokio::spawn(follow::run(options(&url_a, &url_b)));
    same(&svc_a, &url_a, &svc_b, &url_b).await;
    let st = &settled(&view).await;
    // The 30 products deleted (events, altered meanwhile, was copied
    // whole, and old went with its table).
    assert_eq!(st["keys_deleted"], 30, "{st}");
    let total = 300 + 1500 + 200 + 2 + 3 - 30 - 25;
    let copied_rows = st["rows_copied"].as_u64().unwrap();
    assert!(copied_rows < total - 200, "a catch-up copies the rows changed, not all {total}: {copied_rows}");
    let m = marks(&url_b).await;
    assert!(m.contains_key("copied") && m.contains_key("0/since") && m.contains_key("1/since"), "{m:?}");
    assert!(m.contains_key("whole/events") && m.contains_key("whole/notes") && m.contains_key("whole/fresh"), "{m:?}");
    let text = strata_server::http_client().build().unwrap().get(format!("{url_b}/metrics")).send().await.unwrap().text().await.unwrap();
    assert!(text.contains("strata_follow_lag_seconds{name=\"east\"}") && text.contains("strata_follow_behind_transactions{name=\"east\"}"), "{text}");

    // It follows on.
    sql(&svc_a, "INSERT INTO notes (id, body, stars) VALUES (7, 'after the catch-up', 2)").await;
    sql(&svc_a, "DELETE FROM events WHERE id = 100").await;
    same(&svc_a, &url_a, &svc_b, &url_b).await;
    assert!(!f.is_finished());
    f.abort();
}

/// A catch-up cut short (its follow process killed as it writes the rows)
/// begins again where the first began, when started again, and ends with
/// the source's rows: the tables it made again in the first are copied
/// whole in the second too.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_catch_up_cut_short_begins_again() {
    let dir = tempfile::tempdir().unwrap();
    let (svc_a, url_a, svc_b, url_b) = small_logs(dir.path()).await;
    let view = view_of(&url_b);
    let f = tokio::spawn(follow::run(options(&url_a, &url_b)));
    copied(&view, &f).await;
    same(&svc_a, &url_a, &svc_b, &url_b).await;
    f.abort();
    let _ = f.await;
    change_meanwhile(&svc_a).await;
    new_products(&svc_a, 6000, "more").await;

    // Killed once it wrote some of the rows.
    let f = tokio::spawn(follow::run(options(&url_a, &url_b)));
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        // Rows of keys the follower did not hold before.
        let q = svc_b.query(QueryReq { sql: "SELECT COUNT(*) FROM products WHERE id LIKE 'c%'".into(), ..Default::default() }).await;
        if q.is_ok_and(|a| matches!(a.rs.rows[0][0], strata::value::Value::Int(n) if n > 0)) {
            break;
        }
        assert!(!f.is_finished(), "the follow process ended: {}", view().await);
        assert!(Instant::now() < deadline, "the catch-up wrote no rows: {}", view().await);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    f.abort();
    let _ = f.await;
    let m = marks(&url_b).await;
    assert!(!m.contains_key("copied") && m.contains_key("0/since"), "cut short in its catch-up: {m:?}");
    let since = m["0/since"];

    let f = tokio::spawn(follow::run(options(&url_a, &url_b)));
    same(&svc_a, &url_a, &svc_b, &url_b).await;
    let m = marks(&url_b).await;
    assert!(m.contains_key("copied") && m["0/since"] == since, "the same catch-up: {m:?}");
    settled(&view).await;
    f.abort();
}

/// Copied again (`--copy`), a follower drops the rows the source deleted
/// while it was stopped, which no transaction after the copy deletes.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_copy_made_again_drops_rows_deleted_meanwhile() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let (_s0, u0) = shard(d, "s0.strata", |_| true).await;
    let (_a, svc_a, url_a) = coordinator(&[u0]).await;
    sql(&svc_a, "CREATE TABLE events (id INT PRIMARY KEY, usr KEYWORD NOT NULL, n INT) OPTIONS(shard_by = 'usr')").await;
    let values: Vec<String> = (0..200).map(|i| format!("({i}, 'u{}', {i})", i % 13)).collect();
    sql(&svc_a, &format!("INSERT INTO events (id, usr, n) VALUES {}", values.join(", "))).await;
    let (_t0, v0) = shard(d, "t0.strata", |_| false).await;
    let (_b, svc_b, url_b) = coordinator(&[v0]).await;
    let view = view_of(&url_b);
    let f = tokio::spawn(follow::run(options(&url_a, &url_b)));
    copied(&view, &f).await;
    caught_up(&svc_a, &svc_b, &view).await;
    f.abort();
    let _ = f.await;
    let ids: Vec<String> = (0..10).map(|i| format!("'p{:07}'", i * 11)).collect();
    sql(&svc_a, &format!("DELETE FROM products WHERE id IN ({})", ids.join(", "))).await;
    sql(&svc_a, "DELETE FROM events WHERE id >= 190").await;
    sql(&svc_a, "UPDATE events SET n = 0 WHERE id < 5").await;

    let f = tokio::spawn(follow::run(Options { copy: true, ..options(&url_a, &url_b) }));
    caught_up(&svc_a, &svc_b, &view).await;
    let st = settled(&view).await;
    assert_eq!(st["keys_deleted"], 20, "{st}");
    assert!(!marks(&url_b).await.contains_key("0/since"), "a copy of every row");
    f.abort();
}

/// A source of two shards, with a table `log` of rows by number, and a
/// follower of one shard, empty.
async fn two_into_one(d: &Path) -> (Arc<Service>, String, Arc<Service>, String) {
    let (_s0, u0) = shard(d, "s0.strata", |_| true).await;
    let (a, svc_a, url_a) = coordinator(&[u0]).await;
    sql(&svc_a, "CREATE TABLE log (id INT PRIMARY KEY, v TEXT)").await;
    let (_s1, u1) = shard(d, "s1.strata", |_| false).await;
    a.add_shard(vec![u1], Acks::All).await.unwrap();
    a.rebalance(512).unwrap();
    job_done(&a).await;
    let (_t0, v0) = shard(d, "t0.strata", |_| false).await;
    let (_b, svc_b, url_b) = coordinator(&[v0]).await;
    (svc_a, url_a, svc_b, url_b)
}

/// The rows of `log` a cluster holds, by id.
async fn log_ids(svc: &Arc<Service>) -> Vec<i64> {
    let a = svc.query(QueryReq { sql: "SELECT id FROM log ORDER BY id LIMIT 100000".into(), ..Default::default() }).await.unwrap();
    a.rs.rows.iter().map(|r| match r[0] { strata::value::Value::Int(n) => n, ref v => panic!("{v:?}") }).collect()
}

/// Waits until `follower` holds the rows of `log` that `source` does.
async fn same_log(source: &Arc<Service>, follower: &Arc<Service>) {
    let want = log_ids(source).await;
    let deadline = Instant::now() + Duration::from_secs(30);
    while log_ids(follower).await != want {
        assert!(Instant::now() < deadline, "the follower's log is not the source's: {:?} there, {:?} here", want, log_ids(follower).await);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Writes rows of `log` by new ids (`first`, then every `step`th) until
/// `stop`: to `a`, and once `a` holds its writes, to `b`, as a client
/// switched over would, until `b` takes them. Answers the ids each took.
fn switching_writer(a: Arc<Service>, b: Arc<Service>, stop: Arc<AtomicBool>, first: i64, step: i64) -> tokio::task::JoinHandle<(Vec<i64>, Vec<i64>)> {
    tokio::spawn(async move {
        let (mut on_a, mut on_b) = (Vec::new(), Vec::new());
        let mut id = first;
        let mut switched = false;
        while !stop.load(Ordering::Relaxed) {
            let q = format!("INSERT INTO log (id, v) VALUES ({id}, 'row {id}')");
            let to = if switched { &b } else { &a };
            match to.sql(&Principal::open(), &q, &Params::new(), After::Any, 0).await {
                Ok(_) => {
                    if switched { on_b.push(id) } else { on_a.push(id) }
                    id += step;
                }
                Err(e) if !switched && e.status == 503 && e.message.contains("holds its writes") => switched = true,
                // The follower takes them once promoted.
                Err(e) if switched && (e.status == 409 || e.status == 503) => tokio::time::sleep(Duration::from_millis(5)).await,
                Err(e) => panic!("{q}: {} {}", e.status, e.message),
            }
        }
        (on_a, on_b)
    })
}

/// A planned switch-over while clients write: `strata-server switch-over`
/// holds the source's writes (every client's either commits before the
/// hold or is refused), waits until the follower applied them, and
/// promotes it; the clients that move to it once the source refuses them
/// lose no write. The source refuses writes while it holds them, the
/// follow process stops, and starting it again is refused. The old source
/// then follows the new primary back from the promotion's numbers, with no
/// copy, and gets its writes.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_planned_switch_over_loses_no_write_and_the_old_source_follows_back() {
    let dir = tempfile::tempdir().unwrap();
    let (svc_a, url_a, svc_b, url_b) = two_into_one(dir.path()).await;
    let view = view_of(&url_b);
    let f = tokio::spawn(follow::run(options(&url_a, &url_b)));
    copied(&view, &f).await;
    caught_up(&svc_a, &svc_b, &view).await;

    // Four clients write to the source as the switch-over runs.
    let stop = Arc::new(AtomicBool::new(false));
    let writers: Vec<_> = (0..4).map(|w| switching_writer(svc_a.clone(), svc_b.clone(), stop.clone(), w, 4)).collect();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let o = follow::SwitchOptions { target: url_b.clone(), target_token: None, target_ca: None, name: "east".into(), source: None, source_key: None, source_ca: None, unplanned: false, wait: Duration::from_secs(60) };
    let r = follow::switch_over(o).await.unwrap();
    assert_eq!(r["planned"], true, "{r}");
    assert_eq!(r["lost"], 0, "{r}");
    assert_eq!(r["seqs"].as_array().unwrap().len(), 1, "{r}");
    tokio::time::sleep(Duration::from_millis(500)).await;
    stop.store(true, Ordering::Relaxed);
    let (mut on_a, mut on_b) = (Vec::new(), Vec::new());
    for w in writers {
        let (a, b) = w.await.unwrap();
        assert!(!b.is_empty(), "a client wrote to the new primary");
        on_a.extend(a);
        on_b.extend(b);
    }
    on_a.sort();
    let mut all = [on_a.clone(), on_b].concat();
    all.sort();
    assert!(on_a.len() > 20, "{} writes before the hold", on_a.len());
    // Every write the source took is on the new primary, and those it took.
    assert_eq!(log_ids(&svc_a).await, on_a);
    assert_eq!(log_ids(&svc_b).await, all);

    // The follow process stopped; started again, it is refused, and so is
    // a write that would start following again.
    let e = tokio::time::timeout(Duration::from_secs(30), f).await.expect("the follow process stops").unwrap().unwrap_err();
    assert!(e.message.contains("was promoted from east"), "{}", e.message);
    let e = follow::run(options(&url_a, &url_b)).await.unwrap_err();
    assert!(e.status == 409 && e.message.contains("was promoted from east"), "{}", e.message);
    let start = json!({"ops": [{"op": "source", "source": {"name": "_strata/follows east", "offset": i64::MAX - 2}}]});
    let (status, e) = post(&format!("{url_b}/v1/write"), start, Some("east")).await;
    assert!(status == 409 && e.to_string().contains("was promoted from east"), "{status} {e}");
    let v = view().await;
    let c = &v["clusters"][0];
    assert!(v["following"].is_null() && v["holding_ms"].is_null() && c["promoted_ms"].is_i64() && c["source_holds"] == true, "{v}");

    // The old source holds its writes: refused, a change of tables too, with when to try again.
    let res = strata_server::http_client().build().unwrap().post(format!("{url_a}/v1/write")).json(&json!({"ops": [{"op": "upsert", "table": "log", "doc": {"id": -1, "v": "late"}}]})).send().await.unwrap();
    assert_eq!(res.status().as_u16(), 503);
    assert!(res.headers().get("retry-after").is_some());
    assert!(res.text().await.unwrap().contains("holds its writes"));
    match svc_a.sql(&Principal::open(), "CREATE TABLE late (id INT PRIMARY KEY)", &Params::new(), After::Any, 0).await {
        Err(e) => assert!(e.status == 503 && e.message.contains("holds its writes"), "{}", e.message),
        Ok(_) => panic!("a table made while the writes are held"),
    }
    let h = get(&format!("{url_a}/v1/admin/hold-writes")).await;
    assert_eq!(h["holding"], true, "{h}");

    // It follows the new primary back, with no copy, from the promotion.
    let promoted = get(&format!("{url_b}/v1/feed/promoted")).await;
    assert_eq!(promoted["promotions"][0]["promoted"], "east", "{promoted}");
    let back = Options { name: "west".into(), reverse: true, ..options(&url_b, &url_a) };
    let g = tokio::spawn(follow::run(back));
    let view_a = view_of(&url_a);
    let st = settled(&view_a).await;
    assert_eq!(st["rows_copied"], 0, "{st}");
    let h = get(&format!("{url_a}/v1/admin/hold-writes")).await;
    assert_eq!(h["holding"], false, "following, it holds no writes: {h}");
    sql(&svc_b, "INSERT INTO log (id, v) VALUES (100000, 'after'), (100001, 'after')").await;
    sql(&svc_b, &format!("DELETE FROM log WHERE id = {}", on_a[0])).await;
    sql(&svc_b, "UPDATE products SET price = 1 WHERE price < 30").await;
    caught_up(&svc_b, &svc_a, &view_a).await;
    assert_eq!(log_ids(&svc_a).await, log_ids(&svc_b).await);
    let (status, e) = post(&format!("{url_a}/v1/write"), json!({"ops": [{"op": "upsert", "table": "log", "doc": {"id": -1, "v": "late"}}]}), None).await;
    assert!(status == 409 && e.to_string().contains("follows west"), "{status} {e}");
    assert!(!g.is_finished());
    g.abort();
}

/// A switch-over and back, as a drill does: each follow process started
/// again resumes (one following back, with `--reverse` or without, and
/// one following a cluster it was promoted from before), and each
/// switch-over loses nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_switch_over_and_back_resumes_after_restarts() {
    let dir = tempfile::tempdir().unwrap();
    let (svc_a, url_a, svc_b, url_b) = two_into_one(dir.path()).await;
    sql(&svc_a, "INSERT INTO log (id, v) VALUES (1, 'one'), (2, 'two')").await;
    let view_b = view_of(&url_b);
    let f = tokio::spawn(follow::run(options(&url_a, &url_b)));
    copied(&view_b, &f).await;
    caught_up(&svc_a, &svc_b, &view_b).await;
    let switch = |target: &str, name: &str| follow::SwitchOptions { target: target.into(), target_token: None, target_ca: None, name: name.into(), source: None, source_key: None, source_ca: None, unplanned: false, wait: Duration::from_secs(60) };
    let r = follow::switch_over(switch(&url_b, "east")).await.unwrap();
    assert_eq!((r["planned"].clone(), r["lost"].clone()), (json!(true), json!(0)), "{r}");
    let _ = f.await;

    // The old source follows back; that process started again, with
    // --reverse as a script would, or without, resumes.
    let back = Options { name: "west".into(), reverse: true, ..options(&url_b, &url_a) };
    let view_a = view_of(&url_a);
    for (k, reverse) in [true, true, false].into_iter().enumerate() {
        let before = view_a().await["clusters"][0]["status"]["started_ms"].as_u64().unwrap_or(0);
        let g = tokio::spawn(follow::run(Options { reverse, ..back.clone() }));
        let deadline = Instant::now() + Duration::from_secs(30);
        let st = loop {
            let st = settled(&view_a).await;
            if st["started_ms"].as_u64().unwrap_or(0) > before {
                break st;
            }
            assert!(!g.is_finished(), "the follow process ended: {:?}", g.await);
            assert!(Instant::now() < deadline, "it does not follow: {st}");
            tokio::time::sleep(Duration::from_millis(50)).await;
        };
        assert_eq!(st["rows_copied"], 0, "{st}");
        sql(&svc_b, &format!("INSERT INTO log (id, v) VALUES ({}, 'on west')", 100 + k)).await;
        same_log(&svc_b, &svc_a).await;
        g.abort();
        let _ = g.await;
    }
    let g = tokio::spawn(follow::run(Options { reverse: false, ..back.clone() }));
    settled(&view_a).await;
    same_log(&svc_b, &svc_a).await;

    // And back again: the old source is promoted from the new primary, and
    // the new primary follows it back, from that promotion, as a cluster
    // promoted from it once; started again, it resumes too.
    let r = follow::switch_over(switch(&url_a, "west")).await.unwrap();
    assert_eq!((r["planned"].clone(), r["lost"].clone()), (json!(true), json!(0)), "{r}");
    let _ = g.await;
    let again = Options { reverse: true, ..options(&url_a, &url_b) };
    for reverse in [true, false] {
        let h = tokio::spawn(follow::run(Options { reverse, ..again.clone() }));
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(!h.is_finished(), "the follow process ended: {:?}", h.await);
        sql(&svc_a, &format!("INSERT INTO log (id, v) VALUES ({}, 'on east again')", 200 + reverse as i64)).await;
        same_log(&svc_a, &svc_b).await;
        assert_eq!(settled(&view_b).await["rows_copied"], 0);
        h.abort();
        let _ = h.await;
    }
    assert_eq!(log_ids(&svc_a).await, log_ids(&svc_b).await);
    let (status, e) = post(&format!("{url_b}/v1/write"), json!({"ops": [{"op": "upsert", "table": "log", "doc": {"id": -1, "v": "late"}}]}), None).await;
    assert!(status == 409 && e.to_string().contains("follows east"), "{status} {e}");
}

/// After a promotion that was not planned (the source unreachable), the
/// old source, which took writes the new primary never saw, cannot follow
/// it back from the promotion's numbers; it rejoins with a copy, whose
/// sweep deletes the keys it took meanwhile.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn an_old_source_rejoins_after_an_unplanned_promotion() {
    let dir = tempfile::tempdir().unwrap();
    let (svc_a, url_a, svc_b, url_b) = two_into_one(dir.path()).await;
    sql(&svc_a, "INSERT INTO log (id, v) VALUES (1, 'one'), (2, 'two'), (3, 'three')").await;
    let view = view_of(&url_b);
    let f = tokio::spawn(follow::run(options(&url_a, &url_b)));
    copied(&view, &f).await;
    caught_up(&svc_a, &svc_b, &view).await;
    f.abort();
    let _ = f.await;
    // Writes the follower never sees: 15 new keys, 5 deleted.
    let v: Vec<String> = (0..15).map(|i| format!("({}, 'lost {i}')", 1000 + i)).collect();
    sql(&svc_a, &format!("INSERT INTO log (id, v) VALUES {}", v.join(", "))).await;
    let ids: Vec<String> = (0..5).map(|i| format!("'p{:07}'", i * 13)).collect();
    sql(&svc_a, &format!("DELETE FROM products WHERE id IN ({})", ids.join(", "))).await;

    // The source cannot be held: refused, then promoted all the same.
    let gone = follow::SwitchOptions { target: url_b.clone(), target_token: None, target_ca: None, name: "east".into(), source: Some("http://127.0.0.1:9".into()), source_key: None, source_ca: None, unplanned: false, wait: Duration::from_secs(5) };
    let e = follow::switch_over(gone.clone()).await.unwrap_err();
    assert!(e.status == 409 && e.message.contains("could not hold the writes of east"), "{}", e.message);
    let (status, e) = post(&format!("{url_b}/v1/admin/follow/promote"), json!({"name": "east"}), None).await;
    assert!(status == 409 && e.to_string().contains("did not hold its writes"), "{status} {e}");
    let r = follow::switch_over(follow::SwitchOptions { unplanned: true, ..gone }).await.unwrap();
    assert_eq!(r["planned"], false, "{r}");
    sql(&svc_b, "INSERT INTO log (id, v) VALUES (7, 'new primary')").await;

    // Its numbers do not fit the old source, which took writes since.
    let back = Options { name: "west".into(), reverse: true, ..options(&url_b, &url_a) };
    let e = follow::run(back.clone()).await.unwrap_err();
    assert!(e.status == 409 && e.message.contains("took writes after"), "{}", e.message);
    // It still takes its own writes: the refusal changed nothing.
    sql(&svc_a, "INSERT INTO log (id, v) VALUES (2000, 'still mine')").await;

    // It rejoins with a copy: the new primary's rows over its own, and the
    // keys only it has deleted.
    let g = tokio::spawn(follow::run(Options { reverse: false, copy: true, ..back }));
    let view_a = view_of(&url_a);
    let st = settled(&view_a).await;
    assert_eq!(st["keys_deleted"], 16, "{st}");
    caught_up(&svc_b, &svc_a, &view_a).await;
    assert_eq!(log_ids(&svc_a).await, vec![1, 2, 3, 7]);
    g.abort();
}
