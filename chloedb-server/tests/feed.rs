//! A cluster's feed (shard protocol 13): each shard's numbered
//! transactions as lines of JSON, read from its primary through a
//! coordinator, for another cluster to follow. Written again in order, the
//! feeds of every shard give the rows the cluster holds, while slots move
//! between its shards and writes go on (a move's copies and deletes are
//! left out, and the slots changing hands are marked on both shards); the
//! feed goes on from a shard's new primary after a failover, with no gap;
//! and a key of the `feed` scope reads the feed and nothing else.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value as Json};
use chloedb::db::Db;
use chloedb::query::Params;
use chloedb::storage::DbOptions;
use chloedb::value::Value;
use chloedb_server::auth::{Access, Secret};
use chloedb_server::coord::Coordinator;
use chloedb_server::cores::slots::Moved;
use chloedb_server::keys::{Keys, Principal};
use chloedb_server::replication::{self, Acks, PrimaryOptions};
use chloedb_server::service::{After, AgentConfig, Backend, QueryReq, Service};
use chloedb_server::{demo, pb, server, slots};
use tokio::net::TcpListener;

const N: usize = 300;

fn agent() -> AgentConfig {
    AgentConfig { api_key: None, model: chloedb_server::agent::DEFAULT_MODEL.into(), base_url: "http://unused".into() }
}

/// A numbered database at `dir/name` of the demo products `keep` keeps
/// (loaded in bulk: they take no number).
fn create(dir: &Path, name: &str, keep: impl Fn(usize) -> bool) -> Arc<Db> {
    let opts = DbOptions { publish_snapshots: true, ship_log_bytes: 16 << 20, ..DbOptions::default() };
    let db = Db::create(&dir.join(name), demo::schema(N), opts).unwrap();
    db.set_embedder("emb", Arc::new(chloedb::embed::HashEmbedder { dim: demo::DIM })).unwrap();
    let docs: Vec<_> = (0..N).filter(|&i| keep(i)).map(|i| demo::product(i, 7)).collect();
    if !docs.is_empty() {
        db.load(docs).unwrap();
    }
    db.storage().start_seq_no().unwrap();
    Arc::new(db)
}

async fn serve(db: Arc<Db>, dir: &Path, name: &str) -> (Arc<Service>, String) {
    let path = dir.join(name);
    let schema = (*db.schema()).clone();
    let mut svc = Service::new(Backend::Local(db), schema, agent(), 16, Duration::from_secs(10));
    svc.keep_slots_in(slots::file_of(&path)).unwrap();
    svc.keep_replication_in(replication::file_of(&path));
    svc.promote_options = Some(PrimaryOptions { heartbeat: Duration::from_millis(100), retry_max: Duration::from_millis(200), ..Default::default() });
    let svc = Arc::new(svc);
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(server::serve_grpc(svc.clone(), l, Access::default()));
    (svc, url)
}

async fn coordinator(urls: &[String]) -> (Arc<Coordinator>, Arc<Service>) {
    let c = Arc::new(Coordinator::connect(urls.to_vec()).await.unwrap());
    let schema = c.schema.clone();
    (c.clone(), Arc::new(Service::new(Backend::Cluster(c), schema, agent(), 16, Duration::from_secs(10))))
}

async fn sql(svc: &Arc<Service>, s: &str) {
    if let Err(e) = svc.sql(&Principal::open(), s, &Params::new(), After::Any, 0).await {
        panic!("{s}: {}", e.message);
    }
}

fn last(svc: &Service) -> u64 {
    svc.local_db().unwrap().storage().numbering().unwrap().seq
}

/// Lines of a feed from `chunks` until one is transaction `upto` (or,
/// with `upto` before `from`, the first line, which says where the feed
/// starts or why it does not).
async fn lines_until(chunks: &mut tonic::Streaming<pb::FeedChunk>, from: u64, upto: u64) -> Vec<Json> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let (mut buf, mut out) = (Vec::new(), Vec::new());
    loop {
        while let Some(p) = buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = buf.drain(..=p).collect();
            let j: Json = serde_json::from_slice(&line[..p]).unwrap();
            let done = j["seq"].as_u64() == Some(upto) || (upto < from && ["feed", "gone", "diverged"].iter().any(|k| j.get(*k).is_some()));
            out.push(j);
            if done {
                return out;
            }
        }
        let m = tokio::time::timeout_at(deadline.into(), chunks.message()).await.unwrap_or_else(|_| panic!("no transaction {upto} within 30 s: {out:?}"));
        buf.extend(m.unwrap().unwrap_or_else(|| panic!("the feed ended: {out:?}")).lines);
    }
}

/// The transactions of shard `i`'s feed from 1 to its last.
async fn feed_of(c: &Coordinator, i: usize, upto: u64) -> Vec<Json> {
    let mut s = c.feed(i, 1, 0, None).await.unwrap();
    let lines = lines_until(&mut s, 1, upto).await;
    assert!(lines[0]["feed"]["from"] == 1, "{}", lines[0]);
    let txns: Vec<Json> = lines.into_iter().filter(|l| l.get("seq").is_some()).collect();
    let seqs: Vec<u64> = txns.iter().map(|l| l["seq"].as_u64().unwrap()).collect();
    assert_eq!(seqs, (1..=upto).collect::<Vec<_>>(), "shard {i}: every number, in order");
    txns
}

/// Writes one transaction's line to `target` as a follower would: the
/// tables it makes, then its rows.
async fn apply(target: &Arc<Service>, line: &Json) {
    let mut rows = Vec::new();
    for op in line["ops"].as_array().unwrap() {
        match op["op"].as_str().unwrap() {
            "create_table" => {
                let db = target.local_db().unwrap();
                if db.storage().table(op["table"].as_str().unwrap()).is_none() {
                    let s: chloedb::Schema = serde_json::from_value(op["schema"].clone()).unwrap();
                    db.create_table(s).unwrap();
                }
            }
            "upsert" | "insert" | "delete" | "set" => rows.push(op.clone()),
            other => panic!("not in this test: {other}"),
        }
    }
    if !rows.is_empty() {
        target.write_ops_as(rows, None, Moved::No).await.unwrap_or_else(|e| panic!("{line}: {}", e.message));
    }
}

/// Writes `feeds` (each shard's transactions, in order) to `target` as a
/// follower does: a shard's transactions after slots `arrived` wait until
/// the shard they came from has passed their `left` (the same `id`), and a
/// move's copies and deletes are left out. Shards later in the list go
/// first, as far as they may, so that a missing mark would show. Returns
/// the transactions left out per shard, and the marks per shard.
async fn follow(target: &Arc<Service>, feeds: &[Vec<Json>]) -> (Vec<usize>, Vec<Vec<Json>>) {
    let n = feeds.len();
    let (mut pos, mut moved, mut marks) = (vec![0; n], vec![0; n], vec![Vec::new(); n]);
    let mut left: HashSet<String> = HashSet::new();
    while (0..n).any(|i| pos[i] < feeds[i].len()) {
        let mut went = false;
        for i in (0..n).rev() {
            while let Some(l) = feeds[i].get(pos[i]) {
                if let Some(ms) = l["slots"].as_array() {
                    if ms.iter().any(|m| m.get("arrived").is_some() && !left.contains(m["id"].as_str().unwrap())) {
                        break;
                    }
                    for m in ms {
                        if m.get("left").is_some() {
                            left.insert(m["id"].as_str().unwrap().to_string());
                        }
                        marks[i].push(m.clone());
                    }
                }
                match l.get("moved") {
                    Some(_) => moved[i] += 1,
                    None => apply(target, l).await,
                }
                pos[i] += 1;
                went = true;
            }
        }
        assert!(went, "the feeds wait on each other: at {pos:?}");
    }
    (moved, marks)
}

/// Products' sold counts, and the other rows' prices, by id.
async fn rows(svc: &Arc<Service>) -> (BTreeMap<String, i64>, BTreeMap<String, f64>) {
    let a = svc.query(QueryReq { sql: "SELECT id, sold, price FROM products LIMIT 100000".into(), ..Default::default() }).await.unwrap();
    let (mut products, mut others) = (BTreeMap::new(), BTreeMap::new());
    for r in &a.rs.rows {
        let id = r[0].as_str().unwrap().to_string();
        match id.starts_with('p') {
            true => assert!(products.insert(id, r[1].as_i64().unwrap()).is_none()),
            false => assert!(others.insert(id, r[2].as_f64().unwrap()).is_none()),
        }
    }
    (products, others)
}

async fn events(svc: &Arc<Service>) -> Vec<Vec<Value>> {
    svc.query(QueryReq { sql: "SELECT id, usr, n FROM events ORDER BY id LIMIT 10000".into(), ..Default::default() }).await.unwrap().rs.rows
}

async fn job_done(c: &Coordinator) -> Json {
    let t0 = Instant::now();
    loop {
        let m = c.moves();
        if m["job"]["state"] != "running" {
            return m;
        }
        assert!(t0.elapsed() < Duration::from_secs(120), "the job of moves runs still: {m}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Slots move onto a shard just added while rows are written and changed
/// through the coordinator; the two shards' feeds, written again to one
/// database that started as the first shard did, give the cluster's rows.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn feeds_written_again_give_the_rows_while_slots_move() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let (s0, u0) = serve(create(d, "s0.strata", |_| true), d, "s0.strata").await;
    let (a, svc_a) = coordinator(&[u0.clone()]).await;
    sql(&svc_a, "CREATE TABLE events (id INT PRIMARY KEY, usr KEYWORD NOT NULL, n INT) OPTIONS(shard_by = 'usr')").await;
    let values: Vec<String> = (0..200).map(|i| format!("({i}, 'u{}', {i})", i % 13)).collect();
    sql(&svc_a, &format!("INSERT INTO events (id, usr, n) VALUES {}", values.join(", "))).await;
    let (s1, u1) = serve(create(d, "s1.strata", |_| false), d, "s1.strata").await;
    a.add_shard(vec![u1.clone()], Acks::All).await.unwrap();

    // Rows written by key and changed on every shard while the slots move.
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut tasks = Vec::new();
    {
        let (svc, stop) = (svc_a.clone(), stop.clone());
        tasks.push(tokio::spawn(async move {
            let mut r = 0u64;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                let k = format!("w{:03}", r % 97);
                let q = match r % 7 {
                    3 => format!("DELETE FROM products WHERE id = '{k}'"),
                    5 => format!("UPDATE events SET n = n + 1 WHERE id = {}", r % 200),
                    _ => format!("INSERT OR REPLACE INTO products (id, title, description, price) VALUES ('{k}', 'Writer kettle', 'Round {r}', {r})"),
                };
                sql(&svc, &q).await;
                r += 1;
            }
            r
        }));
    }
    {
        let (svc, stop) = (svc_a.clone(), stop.clone());
        tasks.push(tokio::spawn(async move {
            let mut n = 0;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                sql(&svc, "UPDATE products SET sold = sold + 1").await;
                n += 1;
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            n
        }));
    }
    let j = a.rebalance(512).unwrap();
    assert_eq!(j["job"]["of"], 4, "{j}");
    let done = job_done(&a).await;
    assert_eq!(done["job"]["state"], "done", "{done}");
    tokio::time::sleep(Duration::from_millis(100)).await;
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    for t in tasks {
        assert!(t.await.unwrap() > 0);
    }

    let feeds = vec![feed_of(&a, 0, last(&s0)).await, feed_of(&a, 1, last(&s1)).await];
    let target_db = create(d, "follower.strata", |_| true);
    let (target, _) = serve(target_db, d, "follower.strata").await;
    let (moved, marks) = follow(&target, &feeds).await;
    assert_eq!(rows(&target).await, rows(&svc_a).await, "the feeds give the rows");
    assert_eq!(events(&target).await, events(&svc_a).await);

    // The moves' copies to shard 1 and deletes on shard 0 were left out,
    // and each flip was marked on both shards with the same slots.
    assert!(moved[0] > 0 && moved[1] > 0, "{moved:?}");
    for l in &feeds[1] {
        if l["moved"] == "copy" {
            assert!(l.get("ops").is_none() && l["epoch"].as_u64().unwrap() > 0, "{l}");
        }
    }
    assert!(feeds[0].iter().any(|l| l["moved"] == "cleanup") && !feeds[0].iter().any(|l| l["moved"] == "copy"));
    let ids = |ms: &[Json], what: &str| ms.iter().filter(|m| m.get(what).is_some()).map(|m| (m["id"].as_str().unwrap().to_string(), m["slots"].as_u64().unwrap())).collect::<Vec<_>>();
    assert_eq!(ids(&marks[0], "left").len(), 4, "{marks:?}");
    assert_eq!(ids(&marks[0], "left"), ids(&marks[1], "arrived"));
    assert!(ids(&marks[0], "arrived").is_empty() && ids(&marks[1], "left").is_empty());
    assert!(marks[0].iter().all(|m| m["to"] == 1 && m["slots"] == 512), "{marks:?}");

    // The marks are not stream sources, and `_sources` keeps none of them:
    // only the records of idempotency keys (`idem::record`).
    for s in [&s0, &s1] {
        assert!(s.sources().await.unwrap().is_empty());
        let kept = s.local_db().unwrap().sources().unwrap();
        assert!(kept.iter().all(|k| k.name.starts_with(chloedb_server::idem::ROW)), "{:?}", kept.iter().map(|k| &k.name).collect::<Vec<_>>());
    }
    let e = s0.write_ops_as(vec![json!({"op": "source", "source": {"name": "_strata/mine", "offset": 1}})], None, Moved::No).await.unwrap_err();
    assert!(e.message.contains("_strata/"), "{}", e.message);
}

/// A shard's feed goes on from its new primary after the primary changes,
/// with no gap; a reader whose last transaction the shard does not hold
/// is told so; a replica and the primary it replaced serve no feed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_feed_goes_on_from_a_new_primary() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let (s0, u0) = serve(create(d, "s0.strata", |_| true), d, "s0.strata").await;
    let (a, svc_a) = coordinator(&[u0.clone()]).await;
    let (r0, u_r0) = serve(create(d, "r0.strata", |_| false), d, "r0.strata").await;
    a.add_replica(0, &u_r0, false, false).await.unwrap();
    for i in 0..20 {
        sql(&svc_a, &format!("INSERT OR REPLACE INTO products (id, title, price) VALUES ('f{i:02}', 'Before', {i})")).await;
    }
    let before = last(&s0);
    let term = s0.local_db().unwrap().storage().numbering().unwrap().term;
    let mut old = a.feed(0, 1, 0, None).await.unwrap();
    let lines = lines_until(&mut old, 1, before).await;
    assert_eq!(lines.iter().filter(|l| l.get("seq").is_some()).count() as u64, before);
    assert!(lines.iter().any(|l| l["last"].as_u64().is_some()), "it says how far it reaches: {lines:?}");

    // A replica serves no feed.
    let e = r0.feed(1, None, None).await.unwrap_err();
    assert_eq!(e.status, 409, "{}", e.message);

    a.promote(0, &u_r0, false).await.unwrap();
    assert_eq!((r0.role(), s0.role()), (pb::Role::Primary, pb::Role::Replica));
    // The feed of the primary replaced ends.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut ended = None;
    while ended.is_none() {
        let m = tokio::time::timeout_at(deadline.into(), old.message()).await.expect("the old feed ends");
        match m {
            Ok(Some(c)) => {
                for l in String::from_utf8(c.lines).unwrap().lines() {
                    let j: Json = serde_json::from_str(l).unwrap();
                    if j.get("error").is_some() {
                        ended = Some(j);
                    }
                }
            }
            Ok(None) | Err(_) => ended = Some(Json::Null),
        }
    }
    for i in 0..10 {
        sql(&svc_a, &format!("INSERT OR REPLACE INTO products (id, title, price) VALUES ('g{i:02}', 'After', {i})")).await;
    }
    let after = last(&r0);
    assert!(after >= before + 10);

    // On from the transaction after the last read, under its term: no gap.
    let mut s = a.feed(0, before + 1, term, None).await.unwrap();
    let lines = lines_until(&mut s, before + 1, after).await;
    assert_eq!(lines[0]["feed"]["from"], before + 1, "{}", lines[0]);
    let seqs: Vec<u64> = lines.iter().filter_map(|l| l["seq"].as_u64()).collect();
    assert_eq!(seqs, (before + 1..=after).collect::<Vec<_>>());
    let titles: Vec<&str> = lines.iter().filter_map(|l| l["ops"].as_array()).flatten().filter_map(|o| o["doc"]["title"].as_str()).collect();
    assert_eq!(titles.iter().filter(|t| **t == "After").count(), 10, "{titles:?}");
    assert!(lines.iter().filter_map(|l| l["term"].as_u64()).all(|t| t >= term));

    // A reader whose last transaction has another term diverged.
    let mut s = a.feed(0, before + 1, term + 7, None).await.unwrap();
    let first = lines_until(&mut s, before + 1, 0).await;
    assert_eq!(first[0]["diverged"]["seq"], after, "{first:?}");
    // So did one ahead of the shard.
    let mut s = a.feed(0, after + 5, 0, None).await.unwrap();
    assert!(lines_until(&mut s, after + 5, 0).await[0].get("diverged").is_some());
}

/// Over HTTP: a server's own feed, and the `feed` scope, which reads it
/// and whether the server is up, and nothing else.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_feed_key_reads_the_feed_alone() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let db = create(d, "a.strata", |_| true);
    let schema = (*db.schema()).clone();
    let mut svc = Service::new(Backend::Local(db.clone()), schema, agent(), 16, Duration::from_secs(10));
    svc.keys = Arc::new(Keys::open(d.join("a.strata.keys.json")).unwrap());
    let svc = Arc::new(svc);
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
    tokio::spawn(server::serve_http(svc.clone(), l, Access { token: Some(Secret::new("root-token")), ..Default::default() }));
    let c = chloedb_server::http_client().build().unwrap();
    let call = |method: &str, path: &str, auth: &str, body: Option<Json>| {
        let r = match method {
            "GET" => c.get(format!("{base}{path}")),
            _ => c.post(format!("{base}{path}")),
        };
        let r = r.bearer_auth(auth);
        let r = match body {
            Some(b) => r.json(&b),
            None => r,
        };
        r.send()
    };
    let r = call("POST", "/v1/admin/keys", "root-token", Some(json!({"name": "follower", "scope": "feed"}))).await.unwrap();
    assert_eq!(r.status(), 200);
    let feed_key = r.json::<Json>().await.unwrap()["secret"].as_str().unwrap().to_string();
    for i in 0..5 {
        let w = json!({"ops": [{"op": "upsert", "table": "products", "doc": {"id": format!("h{i}"), "title": "Fed", "price": i}}]});
        assert_eq!(call("POST", "/v1/write", "root-token", Some(w)).await.unwrap().status(), 200);
    }
    let w = json!({"ops": [{"op": "set", "table": "products", "key": "h1", "field": "price", "value": 9.5}, {"op": "delete", "table": "products", "key": "h2"}]});
    assert_eq!(call("POST", "/v1/write", "root-token", Some(w)).await.unwrap().status(), 200);

    // The feed, as NDJSON, from the first transaction on.
    let mut r = call("GET", "/v1/feed?from=1", &feed_key, None).await.unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["content-type"], "application/x-ndjson");
    let (mut buf, mut txns) = (String::new(), Vec::new());
    let deadline = Instant::now() + Duration::from_secs(20);
    while txns.len() < 6 {
        assert!(Instant::now() < deadline, "{txns:?}");
        let chunk = tokio::time::timeout_at(deadline.into(), r.chunk()).await.unwrap().unwrap().unwrap();
        buf.push_str(std::str::from_utf8(&chunk).unwrap());
        while let Some(p) = buf.find('\n') {
            let j: Json = serde_json::from_str(&buf[..p]).unwrap();
            buf.drain(..=p);
            if j.get("seq").is_some() {
                txns.push(j);
            }
        }
    }
    drop(r);
    assert_eq!(txns[0]["ops"][0], json!({"op": "upsert", "table": "products", "doc": txns[0]["ops"][0]["doc"].clone()}));
    assert_eq!(txns[0]["ops"][0]["doc"]["title"], "Fed");
    assert_eq!(txns[5]["ops"], json!([{"op": "set", "table": "products", "key": "h1", "field": "price", "value": 9.5}, {"op": "delete", "table": "products", "key": "h2"}]));

    // The feed key reads nothing else.
    assert_eq!(call("GET", "/v1/health", &feed_key, None).await.unwrap().status(), 200);
    let me: Json = call("GET", "/v1/whoami", &feed_key, None).await.unwrap().json().await.unwrap();
    assert_eq!(me["scope"], "feed");
    let r = call("POST", "/v1/query", &feed_key, Some(json!({"sql": "SELECT id FROM products LIMIT 1"}))).await.unwrap();
    assert_eq!(r.status(), 403);
    assert!(r.json::<Json>().await.unwrap()["error"].as_str().unwrap().contains("read scope"));
    assert_eq!(call("GET", "/v1/sources", &feed_key, None).await.unwrap().status(), 403);
    assert_eq!(call("POST", "/v1/write", &feed_key, Some(json!({"ops": []}))).await.unwrap().status(), 403);
    // A server holds no other shard, and one that does not number its
    // writes has no feed.
    assert_eq!(call("GET", "/v1/feed?shard=2", &feed_key, None).await.unwrap().status(), 400);
    let plain = Db::create(&d.join("b.strata"), demo::schema(N), DbOptions::default()).unwrap();
    let plain = Service::new(Backend::Local(Arc::new(plain)), demo::schema(N), agent(), 16, Duration::from_secs(10));
    assert_eq!(Arc::new(plain).feed(1, None, None).await.unwrap_err().status, 409);
}
