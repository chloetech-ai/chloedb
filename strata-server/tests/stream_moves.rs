//! A stream through a coordinator applies each event once while slots
//! move between its shards: rebalancing onto a shard just added, then
//! draining the first. Every shard keeps each source's offset for the
//! slots it holds, and the offsets of moving slots go with them, so that
//! neither the batches a move's flip catches in flight nor a replay from
//! the start lose or repeat an event.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use serde_json::{json, Value as Json};
use strata::db::Db;
use strata::query::Params;
use strata::storage::DbOptions;
use strata::Schema;
use strata_server::auth::Access;
use strata_server::coord::Coordinator;
use strata_server::replication::{self, Acks};
use strata_server::service::{AgentConfig, Backend, Service};
use strata_server::{server, slots};
use tokio::net::TcpListener;

const USERS: usize = 400;
const SOURCE: &str = "clicks/0";

fn items() -> Schema {
    Schema::from_json(r#"{"table": "items", "key": "id", "fields": [{"name": "id", "type": "keyword"}, {"name": "title", "type": "text"}]}"#).unwrap()
}

fn agent() -> AgentConfig {
    AgentConfig { api_key: None, model: strata_server::agent::DEFAULT_MODEL.into(), base_url: "http://unused".into() }
}

/// A numbered database, as a replicated shard's is.
fn create(dir: &Path, name: &str) -> Arc<Db> {
    let opts = DbOptions { segment_max_docs: 2000, publish_snapshots: true, ship_log_bytes: 16 << 20, ..DbOptions::default() };
    let db = Db::create(&dir.join(name), items(), opts).unwrap();
    db.storage().start_seq_no().unwrap();
    Arc::new(db)
}

/// A shard server of `db` (at `dir/name`), keeping its cluster map beside
/// the database as `strata-server --db` does.
async fn serve(db: Arc<Db>, dir: &Path, name: &str) -> (Arc<Service>, String) {
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

async fn http(svc: Arc<Service>) -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(server::serve_http(svc, l, Access::default()));
    url
}

fn user(i: usize) -> String {
    format!("u{}", i % USERS)
}

/// Event `i` as a line of `/v1/stream`: its row, and one more on its
/// user's count, both in the user's slot.
fn line(i: usize) -> String {
    let ops = vec![
        json!({"op": "insert", "table": "events", "doc": {"id": i, "usr": user(i)}}),
        json!({"op": "add", "table": "users", "key": user(i), "field": "n", "value": 1, "doc": {"id": user(i)}}),
    ];
    format!("{}\n", json!({"source": {"name": SOURCE, "offset": i}, "ops": ops}))
}

async fn post(url: &str, path: &str, body: Json) -> Json {
    strata_server::http_client().build().unwrap().post(format!("{url}{path}")).json(&body).send().await.unwrap().json().await.unwrap()
}

async fn get(url: &str, path: &str) -> Json {
    strata_server::http_client().build().unwrap().get(format!("{url}{path}")).send().await.unwrap().json().await.unwrap()
}

/// The events, and the users' counts' sum.
async fn totals(url: &str) -> (i64, i64) {
    let one = |j: Json| j["rows"][0][0].as_i64().unwrap_or(0);
    let events = one(post(url, "/v1/sql", json!({"sql": "SELECT COUNT(*) FROM events"})).await);
    let n = one(post(url, "/v1/sql", json!({"sql": "SELECT SUM(n) FROM users"})).await);
    (events, n)
}

/// Streams events `from..` to `url` one line at a time, until `stop`
/// (after at least `min` events): the answer's last line and the events
/// sent.
async fn trickle(url: String, from: usize, min: usize, stop: Arc<AtomicBool>, sent: Arc<AtomicUsize>) -> Json {
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<String, std::io::Error>>(16);
    let feed = tokio::spawn(async move {
        let mut i = from;
        while i < from + min || !stop.load(Ordering::Relaxed) {
            if tx.send(Ok(line(i))).await.is_err() {
                break;
            }
            i += 1;
            sent.store(i - from, Ordering::Relaxed);
            tokio::time::sleep(Duration::from_micros(300)).await;
        }
    });
    let body = reqwest::Body::wrap_stream(tokio_stream::wrappers::ReceiverStream::new(rx));
    let r = strata_server::http_client().build().unwrap().post(format!("{url}/v1/stream")).header("content-type", "application/x-ndjson").body(body).send().await.unwrap();
    assert_eq!(r.status(), 200);
    let mut lines = r.bytes_stream();
    let mut text = String::new();
    while let Some(b) = lines.next().await {
        text.push_str(std::str::from_utf8(&b.unwrap()).unwrap());
    }
    feed.await.unwrap();
    serde_json::from_str(text.lines().last().unwrap()).unwrap()
}

async fn job_done(c: &Coordinator) -> Json {
    let t0 = Instant::now();
    loop {
        let m = c.moves();
        if m["job"]["state"] != "running" {
            return m;
        }
        assert!(t0.elapsed() < Duration::from_secs(120), "the job of moves runs still: {m}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stream_applies_each_event_once_while_slots_move() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let (_s0, u0) = serve(create(d, "s0.strata"), d, "s0.strata").await;
    let c = Arc::new(Coordinator::connect(vec![u0.clone()]).await.unwrap());
    let schema = c.schema.clone();
    let url = http(Arc::new(Service::new(Backend::Cluster(c.clone()), schema, agent(), 16, Duration::from_secs(10)))).await;
    for ddl in ["CREATE TABLE users (id KEYWORD PRIMARY KEY, n INT DEFAULT 0)", "CREATE TABLE events (id INT PRIMARY KEY, usr KEYWORD NOT NULL) OPTIONS(shard_by = 'usr')"] {
        let a = post(&url, "/v1/sql", json!({"sql": ddl})).await;
        assert!(a["error"].is_null(), "{a}");
    }
    // Events before the second shard joins: it has seen no offset of the
    // source when its first slots arrive.
    let stop = Arc::new(AtomicBool::new(true));
    let first = trickle(url.clone(), 0, 500, stop.clone(), Default::default()).await;
    assert_eq!((first["written"].as_u64(), first["skipped"].as_u64()), (Some(1000), Some(0)), "{first}");
    let (_s1, u1) = serve(create(d, "s1.strata"), d, "s1.strata").await;
    c.add_shard(vec![u1.clone()], Acks::All).await.unwrap();

    // The stream goes on while slots move onto the new shard and back.
    stop.store(false, Ordering::Relaxed);
    let sent = Arc::new(AtomicUsize::new(0));
    let streaming = tokio::spawn(trickle(url.clone(), 500, 200, stop.clone(), sent.clone()));
    for _ in 0..2 {
        c.rebalance(128).unwrap();
        let done = job_done(&c).await;
        assert_eq!(done["job"]["state"], "done", "{done}");
        c.drain(1, 128).unwrap();
        let done = job_done(&c).await;
        assert_eq!(done["job"]["state"], "done", "{done}");
    }
    c.rebalance(128).unwrap();
    assert_eq!(job_done(&c).await["job"]["state"], "done");
    stop.store(true, Ordering::Relaxed);
    let second = streaming.await.unwrap();
    let n = 500 + sent.load(Ordering::Relaxed);
    assert!(second["done"].as_bool().unwrap_or(false), "{second}");
    assert_eq!(second["skipped"].as_u64(), Some(0), "no event of the stream is a replay's: {second}");
    assert_eq!(totals(&url).await, (n as i64, n as i64), "each event once, after {} events", n);
    assert_eq!(get(&url, &format!("/v1/sources/{SOURCE}")).await["offset"], n as i64 - 1);

    // A replay from the start, after every move, applies nothing again.
    stop.store(true, Ordering::Relaxed);
    let replay = trickle(url.clone(), 0, n, stop.clone(), Default::default()).await;
    assert_eq!((replay["written"].as_u64(), replay["skipped"].as_u64()), (Some(0), Some(2 * n as u64)), "{replay}");
    assert_eq!(totals(&url).await, (n as i64, n as i64));
    let _ = Params::new();
}
