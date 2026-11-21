//! Kafka connectors (`crate::connectors::kafka`): a server reading the
//! topics of the test broker (`crate::connectors::kafka_broker`, Kafka's
//! protocol in this process) into a table while the connector is on,
//! started and stopped from its routes, each event written once across a
//! stop, a restart of the server and a replay; the dry run; a broker that
//! asks clients to sign in; events the topic's retention deleted before
//! they were read, counted and said; tombstones deleting rows. With
//! `CHLOEDB_KAFKA=host:port` (a real Kafka or Redpanda, with
//! auto-created topics), one more test produces there and reads it back.

use std::sync::Arc;
use std::time::{Duration, Instant};

use chloedb::db::Db;
use chloedb::storage::DbOptions;
use chloedb_server::auth::{Access, Secret};
use chloedb_server::connectors::kafka_broker::{Message, Options, TestBroker};
use chloedb_server::server;
use chloedb_server::service::{AgentConfig, Backend, Service};
use serde_json::{json, Value as Json};
use tokio::net::TcpListener;

const ROOT: &str = "root-token";

fn agent() -> AgentConfig {
    AgentConfig { api_key: None, model: chloedb_server::agent::DEFAULT_MODEL.into(), base_url: "http://unused".into() }
}

/// A server on the database in `dir` (made the first time), running its
/// connectors.
async fn start(dir: &std::path::Path) -> (String, Arc<Service>) {
    let path = dir.join("k.strata");
    let db = if path.exists() { Db::open(&path, DbOptions::default()).unwrap() } else { Db::create_empty(&path, DbOptions::default()).unwrap() };
    let schema = (*db.schema()).clone();
    let svc = Arc::new(Service::new(Backend::Local(Arc::new(db)), schema, agent(), 16, Duration::from_secs(20)));
    svc.connectors.keep_secrets(dir.join("k.strata.connectors.json")).unwrap();
    svc.connectors.enable();
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(server::serve_http(svc.clone(), l, Access { token: Some(Secret::new(ROOT)), ..Default::default() }));
    (url, svc)
}

async fn call(method: reqwest::Method, url: &str, body: Option<Json>) -> (u16, Json) {
    let c = reqwest::Client::new();
    let mut r = c.request(method, url).bearer_auth(ROOT).timeout(Duration::from_secs(60));
    if let Some(b) = body {
        r = r.json(&b);
    }
    let res = r.send().await.unwrap();
    let status = res.status().as_u16();
    let text = res.text().await.unwrap();
    (status, serde_json::from_str(&text).unwrap_or(Json::String(text)))
}

async fn post(url: &str, body: Json) -> (u16, Json) {
    call(reqwest::Method::POST, url, Some(body)).await
}

async fn get(url: &str) -> Json {
    let (s, j) = call(reqwest::Method::GET, url, None).await;
    assert_eq!(s, 200, "{url}: {j}");
    j
}

async fn sql(base: &str, q: &str) -> Vec<Json> {
    let (s, j) = post(&format!("{base}/v1/sql"), json!({"sql": q, "objects": true})).await;
    assert_eq!(s, 200, "{q}: {j}");
    j["rows"].as_array().cloned().unwrap_or_default()
}

async fn count(base: &str, table: &str) -> i64 {
    sql(base, &format!("SELECT COUNT(*) AS n FROM {table}")).await[0]["n"].as_i64().unwrap()
}

/// Waits up to `secs` for `f` to hold, saying `what` when it does not.
async fn until<F, Fut>(secs: u64, what: &str, mut f: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    let t0 = Instant::now();
    let mut last = String::new();
    while t0.elapsed() < Duration::from_secs(secs) {
        match f().await {
            Ok(()) => return,
            Err(e) => last = e,
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("{what} within {secs} s: {last}");
}

async fn rows_reach(base: &str, table: &str, n: i64, secs: u64) {
    until(secs, &format!("{table} has {n} rows"), || async move {
        let have = count(base, table).await;
        if have == n {
            Ok(())
        } else {
            Err(format!("it has {have}"))
        }
    })
    .await;
}

async fn connector(base: &str, name: &str) -> Json {
    get(&format!("{base}/v1/admin/connectors/{name}")).await
}

/// `n` made-up events, from `from` on, keyed by user.
fn events(from: usize, n: usize) -> Vec<Message> {
    (from..from + n)
        .map(|i| Message {
            ts: 1_763_250_000_000 + i as i64,
            key: Some(format!("u{:03}", i % 7).into_bytes()),
            value: Some(json!({"event_id": format!("e{i}"), "user": format!("u{:03}", i % 7), "n": i}).to_string().into_bytes()),
            headers: vec![("source".into(), Some(b"test".to_vec()))],
        })
        .collect()
}

async fn broker(partitions: i32) -> TestBroker {
    TestBroker::start(Options { listen: "127.0.0.1:0".parse().unwrap(), topic: "t.events".into(), partitions, rate: 0.0, ..Options::default() }).await.unwrap()
}

/// A table without a primary key: each write is a row of its own, so an
/// event written twice is two rows.
const EVENTS: &str = "CREATE TABLE ev (event_id KEYWORD, user_id KEYWORD, n INT, part INT, off INT, at TIMESTAMP)";

fn kafka_body(name: &str, brokers: &str, extra: Json) -> Json {
    let mut b = json!({
        "name": name, "kind": "kafka", "target": "ev",
        "config": {
            "allow_internal": true, "brokers": brokers, "topics": "t.events", "max_wait_ms": 200,
            "map": {"event_id": "$.value.event_id", "user_id": "$.key", "n": {"path": "$.value.n", "as": "int"}, "part": "$.partition", "off": "$.offset", "at": {"path": "$.ts", "as": "time"}},
        },
    });
    if let Json::Object(m) = extra {
        for (k, v) in m {
            if k == "config" {
                for (ck, cv) in v.as_object().unwrap() {
                    b["config"][ck] = cv.clone();
                }
            } else {
                b[k] = v;
            }
        }
    }
    b
}

/// Distinct events in `ev`, against its rows: equal when none was written
/// twice.
async fn once(base: &str) -> (i64, i64) {
    let rows = count(base, "ev").await;
    let distinct = sql(base, "SELECT COUNT(DISTINCT event_id) AS n FROM ev").await[0]["n"].as_i64().unwrap();
    (rows, distinct)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_kafka_connector_streams_starts_and_stops() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _svc) = start(dir.path()).await;
    let b = broker(3).await;
    b.produce("t.events", None, events(0, 300)).unwrap();
    sql(&base, EVENTS).await;
    let (s, j) = post(&format!("{base}/v1/admin/connectors"), kafka_body("ev-in", &b.address(), json!({}))).await;
    assert_eq!(s, 200, "{j}");
    assert_eq!(j["connector"]["streams"], true, "{j}");
    rows_reach(&base, "ev", 300, 30).await;
    let row = &sql(&base, "SELECT event_id, user_id, n, part, off, at FROM ev WHERE event_id = 'e42'").await[0];
    assert_eq!(row["user_id"], "u000", "the key: {row}");
    assert_eq!(row["n"], 42, "{row}");
    assert!(row["at"].as_str().unwrap().starts_with("2025-11-1"), "the event's time: {row}");

    // Live in the API: streaming, by partition, the offsets kept as
    // sources of their own.
    until(15, "the consumer streaming and caught up", || async {
        let c = connector(&base, "ev-in").await;
        let st = &c["stream"];
        if st["state"] == "streaming" && st["lag"] == 0 && st["partitions"].as_array().is_some_and(|p| p.len() == 3) && c["health"] == "streaming" {
            Ok(())
        } else {
            Err(c.to_string())
        }
    })
    .await;
    let src = sql(&base, "SELECT name, last_offset FROM _sources ORDER BY name").await;
    let names: Vec<&str> = src.iter().map(|r| r["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["kafka/ev-in/t.events/0", "kafka/ev-in/t.events/1", "kafka/ev-in/t.events/2"], "{src:?}");
    let ends: i64 = b.status()["topics"][0]["partitions"].as_array().unwrap().iter().map(|p| p["end"].as_i64().unwrap()).sum();
    assert_eq!(ends, 300);
    let kept: i64 = src.iter().map(|r| r["last_offset"].as_i64().unwrap() + 1).sum();
    assert_eq!(kept, 300, "each source holds its partition's last offset: {src:?}");

    // On /metrics and to the alerts (ROSA-501): up, nothing waiting.
    let m = get(&format!("{base}/metrics")).await;
    let m = m.as_str().unwrap();
    for series in [
        "chloedb_connector_up{connector=\"ev-in\",kind=\"kafka\"} 1",
        "chloedb_connector_failures{connector=\"ev-in\",kind=\"kafka\"} 0",
        "chloedb_connector_lag_events{connector=\"ev-in\",kind=\"kafka\"} 0",
        "chloedb_connector_behind_seconds{connector=\"ev-in\",kind=\"kafka\"} 0",
        "chloedb_connector_partition_lag_events{connector=\"ev-in\",topic=\"t.events\",partition=\"2\"} 0",
        "chloedb_connector_last_ok_timestamp_seconds{connector=\"ev-in\",kind=\"kafka\"} ",
    ] {
        assert!(m.contains(series), "{series} in {m}");
    }
    let g = _svc.sample().gauges;
    assert_eq!((g.get("connectors_failing"), g.get("connector_behind_s")), (Some(&0.0), Some(&0.0)), "the alerts connector-failing's and connector-behind's");

    // New events arrive while it runs.
    b.produce("t.events", None, events(300, 50)).unwrap();
    rows_reach(&base, "ev", 350, 20).await;

    // Stopped: nothing more arrives, and it says so; the row keeps it off.
    let (s, j) = post(&format!("{base}/v1/admin/connectors/ev-in/stop"), json!({})).await;
    assert_eq!(s, 200, "{j}");
    assert_eq!(j["connector"]["enabled"], false, "{j}");
    b.produce("t.events", None, events(350, 40)).unwrap();
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(count(&base, "ev").await, 350, "a stopped connector reads nothing");
    let c = connector(&base, "ev-in").await;
    assert_eq!(c["health"], "disabled", "{c}");
    assert_eq!(c["stream"]["state"], "stopped", "{c}");

    // Started again: it goes on from its offsets, writing each event once.
    let (s, j) = post(&format!("{base}/v1/admin/connectors/ev-in/start"), json!({})).await;
    assert_eq!(s, 200, "{j}");
    assert_eq!(j["runs_here"], true, "{j}");
    rows_reach(&base, "ev", 390, 20).await;
    assert_eq!(once(&base).await, (390, 390));

    // The runs table has the stream's sessions; Run now is refused.
    until(15, "a run row of the stopped session", || async {
        let r = sql(&base, "SELECT trigger, records FROM chloedb_connector_runs WHERE connector = 'ev-in'").await;
        if r.iter().any(|r| r["trigger"] == "stream" && r["records"].as_i64().unwrap_or(0) > 0) {
            Ok(())
        } else {
            Err(format!("{r:?}"))
        }
    })
    .await;
    let (s, j) = post(&format!("{base}/v1/admin/connectors/ev-in/run"), json!({})).await;
    assert_eq!(s, 400, "{j}");
    assert!(j["error"].as_str().unwrap().contains("start"), "{j}");

    // Start and stop are for connectors that stream.
    let (s, _) = post(&format!("{base}/v1/admin/connectors"), json!({"name": "hook", "kind": "webhook", "config": {}})).await;
    if s == 200 {
        let (s, j) = post(&format!("{base}/v1/admin/connectors/hook/start"), json!({})).await;
        assert_eq!(s, 400, "{j}");
    }

    // Deleted: its consumer stops.
    let (s, j) = call(reqwest::Method::DELETE, &format!("{base}/v1/admin/connectors/ev-in"), None).await;
    assert_eq!(s, 200, "{j}");
    b.produce("t.events", None, events(390, 10)).unwrap();
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(count(&base, "ev").await, 390);
}

/// A server process of its own on `db`, killed when dropped.
struct Proc(std::process::Child, String);

impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

async fn spawn_server(db: &std::path::Path, log: &std::path::Path) -> Proc {
    let (http, grpc) = (free_port(), free_port());
    let err = std::fs::OpenOptions::new().create(true).append(true).open(log).unwrap();
    let child = std::process::Command::new(env!("CARGO_BIN_EXE_chloedb-server"))
        .arg("--db")
        .arg(db)
        .args(["--listen", &format!("127.0.0.1:{http}"), "--grpc", &format!("127.0.0.1:{grpc}"), "--token", ROOT, "--log-store", "off"])
        .stdout(std::process::Stdio::null())
        .stderr(err)
        .spawn()
        .unwrap();
    let base = format!("http://127.0.0.1:{http}");
    let t0 = Instant::now();
    while reqwest::get(format!("{base}/v1/health")).await.is_err() {
        assert!(t0.elapsed() < Duration::from_secs(30), "the server did not start: {}", std::fs::read_to_string(log).unwrap_or_default());
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Proc(child, base)
}

/// A server killed (SIGKILL) while it streams, and started again on its
/// database, goes on from the offsets it kept: no event twice, none
/// missing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_server_killed_mid_stream_writes_each_event_once() {
    let dir = tempfile::tempdir().unwrap();
    let (db, log) = (dir.path().join("k.strata"), dir.path().join("server.log"));
    let b = broker(4).await;
    b.produce("t.events", None, events(0, 3000)).unwrap();
    let first = {
        let p = spawn_server(&db, &log).await;
        sql(&p.1, EVENTS).await;
        // Small batches, so it is killed part-way.
        let (s, j) = post(&format!("{}/v1/admin/connectors", p.1), kafka_body("ev-in", &b.address(), json!({"config": {"batch": 20}}))).await;
        assert_eq!(s, 200, "{j}");
        until(30, "some rows", || async { if count(&p.1, "ev").await > 0 { Ok(()) } else { Err("none".into()) } }).await;
        count(&p.1, "ev").await
    };
    assert!(first > 0);
    b.produce("t.events", None, events(3000, 500)).unwrap();
    let p = spawn_server(&db, &log).await;
    rows_reach(&p.1, "ev", 3500, 60).await;
    assert_eq!(once(&p.1).await, (3500, 3500), "each event once across the kill (killed at {first} rows)");
    until(15, "the lag back to 0", || async {
        let c = connector(&p.1, "ev-in").await;
        if c["stream"]["lag"] == 0 { Ok(()) } else { Err(c.to_string()) }
    })
    .await;
}

/// A second connector with the same prefix starts where the first is, and
/// one with another prefix reads the topic again from its start.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sources_belong_to_their_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _svc) = start(dir.path()).await;
    let b = broker(2).await;
    b.produce("t.events", None, events(0, 100)).unwrap();
    sql(&base, EVENTS).await;
    let (s, j) = post(&format!("{base}/v1/admin/connectors"), kafka_body("one", &b.address(), json!({}))).await;
    assert_eq!(s, 200, "{j}");
    rows_reach(&base, "ev", 100, 30).await;
    post(&format!("{base}/v1/admin/connectors/one/stop"), json!({})).await;
    let (s, j) = post(&format!("{base}/v1/admin/connectors"), kafka_body("two", &b.address(), json!({"config": {"prefix": "kafka/one/"}}))).await;
    assert_eq!(s, 200, "{j}");
    b.produce("t.events", None, events(100, 20)).unwrap();
    rows_reach(&base, "ev", 120, 30).await;
    assert_eq!(once(&base).await, (120, 120), "the same sources: nothing again");
    let (s, j) = post(&format!("{base}/v1/admin/connectors"), kafka_body("three", &b.address(), json!({}))).await;
    assert_eq!(s, 200, "{j}");
    rows_reach(&base, "ev", 240, 30).await;
}

/// The dry run connects, reads a few events and shows their rows, writing
/// nothing; the default target is every event as a row.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_dry_run_and_the_default_table() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _svc) = start(dir.path()).await;
    let b = broker(2).await;
    b.produce("t.events", None, events(0, 30)).unwrap();
    let body = json!({"name": "raw", "kind": "kafka", "config": {"allow_internal": true, "brokers": b.address(), "topics": ["t.events"]}});
    let (s, j) = post(&format!("{base}/v1/admin/connectors/test"), body.clone()).await;
    assert_eq!(s, 200, "{j}");
    assert_eq!(j["target"], "kafka_events", "{j}");
    assert!(j["records"].as_u64().unwrap() >= 10, "{j}");
    let r = &j["rows"][0];
    assert_eq!(r["topic"], "t.events", "{j}");
    assert!(r["id"].as_str().unwrap().starts_with("t.events/"), "{j}");
    assert!(r["value"]["event_id"].is_string(), "the value as JSON: {j}");
    assert!(j["errors"].as_array().unwrap().is_empty(), "{j}");

    // Without allow_internal, a loopback broker is refused.
    let (s, j) = post(&format!("{base}/v1/admin/connectors/test"), json!({"name": "raw", "kind": "kafka", "config": {"brokers": b.address(), "topics": "t.events"}})).await;
    assert!(s >= 400, "{s} {j}");
    assert!(j["error"].as_str().unwrap().contains("allow_internal"), "{j}");

    // A config that is not one is said at once.
    let (s, j) = post(&format!("{base}/v1/admin/connectors"), json!({"name": "bad", "kind": "kafka", "config": {"brokers": "", "topics": "x"}})).await;
    assert_eq!(s, 400, "{j}");

    // Made with its default table: every event a row.
    let (s, j) = post(&format!("{base}/v1/admin/connectors"), body.clone()).await;
    assert_eq!(s, 200, "{j}");
    rows_reach(&base, "kafka_events", 30, 30).await;
    let row = &sql(&base, "SELECT id, topic, partition_, offset_, msg_key, value, headers FROM kafka_events ORDER BY id LIMIT 1").await[0];
    assert_eq!(row["topic"], "t.events", "{row}");
    assert!(row["msg_key"].as_str().unwrap().starts_with('u'), "{row}");
    // A JSON column answers as its JSON text.
    let json_of = |v: &Json| -> Json { v.as_str().map_or(v.clone(), |t| serde_json::from_str(t).unwrap()) };
    assert!(json_of(&row["value"])["event_id"].is_string(), "{row}");
    assert_eq!(json_of(&row["headers"])["source"], "test", "{row}");
    let hits = sql(&base, "SELECT id FROM kafka_events WHERE MATCH(text, 'e7')").await;
    assert!(!hits.is_empty(), "the value's text is searched");
}

/// A broker that asks clients to sign in: refused without the password
/// (the connector failing, saying why, and the password never in an
/// answer), streaming with it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sasl_plain_sign_in() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _svc) = start(dir.path()).await;
    let b = TestBroker::start(Options { listen: "127.0.0.1:0".parse().unwrap(), topic: "t.events".into(), partitions: 1, rate: 0.0, sasl: Some(("ana".into(), "s3cret-pw".into())), ..Options::default() }).await.unwrap();
    b.produce("t.events", None, events(0, 20)).unwrap();
    sql(&base, EVENTS).await;
    let sasl = json!({"config": {"sasl": {"mechanism": "PLAIN", "username": "ana"}}});
    let mut wrong = kafka_body("signed", &b.address(), sasl.clone());
    wrong["secrets"] = json!({"password": "not-it"});
    let (s, j) = post(&format!("{base}/v1/admin/connectors"), wrong).await;
    assert_eq!(s, 200, "{j}");
    until(30, "the connector failing", || async {
        let c = connector(&base, "signed").await;
        if c["health"] == "failing" && c["stream"]["last_error"].is_string() {
            Ok(())
        } else {
            Err(c.to_string())
        }
    })
    .await;
    let c = connector(&base, "signed").await;
    assert!(!c.to_string().contains("not-it"), "the password is never in an answer: {c}");
    assert_eq!(count(&base, "ev").await, 0);
    let (s, j) = post(&format!("{base}/v1/admin/connectors/signed"), json!({"secrets": {"password": "s3cret-pw"}})).await;
    assert_eq!(s, 200, "{j}");
    rows_reach(&base, "ev", 20, 60).await;
}

/// Events the topic's retention deleted before the connector read them
/// are counted and said; it goes on from the first one left.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn events_lost_to_retention_are_said() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _svc) = start(dir.path()).await;
    let b = TestBroker::start(Options { listen: "127.0.0.1:0".parse().unwrap(), topic: "t.events".into(), partitions: 1, rate: 0.0, retain: 100, ..Options::default() }).await.unwrap();
    b.produce("t.events", None, events(0, 50)).unwrap();
    sql(&base, EVENTS).await;
    let (s, j) = post(&format!("{base}/v1/admin/connectors"), kafka_body("ev-in", &b.address(), json!({}))).await;
    assert_eq!(s, 200, "{j}");
    rows_reach(&base, "ev", 50, 30).await;
    post(&format!("{base}/v1/admin/connectors/ev-in/stop"), json!({})).await;
    // 250 more: the log keeps the last 100 (200 to 299), 150 never read.
    b.produce("t.events", None, events(50, 250)).unwrap();
    post(&format!("{base}/v1/admin/connectors/ev-in/start"), json!({})).await;
    rows_reach(&base, "ev", 150, 30).await;
    let c = connector(&base, "ev-in").await;
    assert_eq!(c["stream"]["lost"], 150, "{c}");
    assert!(c["stream"]["warning"].as_str().unwrap().contains("retention"), "{c}");
    assert_eq!(c["health"], "warning", "{c}");
    let first = sql(&base, "SELECT MIN(n) AS n FROM ev WHERE n >= 50").await;
    assert_eq!(first[0]["n"], 200, "{first:?}");
}

/// A keyed table: an event upserts the row of its key, and a tombstone
/// deletes it with `"tombstones": "delete"`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tombstones_delete_rows() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _svc) = start(dir.path()).await;
    let b = broker(1).await;
    sql(&base, "CREATE TABLE users (id KEYWORD PRIMARY KEY, name TEXT, plan KEYWORD)").await;
    let m = |k: &str, v: Option<Json>| Message { ts: 0, key: Some(k.as_bytes().to_vec()), value: v.map(|v| v.to_string().into_bytes()), headers: vec![] };
    b.produce("t.users", None, vec![m("u1", Some(json!({"name": "Ana", "plan": "free"}))), m("u2", Some(json!({"name": "Beto", "plan": "pro"}))), m("u1", Some(json!({"name": "Ana", "plan": "pro"})))]).unwrap();
    let body = json!({"name": "users-in", "kind": "kafka", "target": "users", "config": {"allow_internal": true, "brokers": b.address(), "topics": "t.users", "max_wait_ms": 200, "tombstones": "delete", "map": {"id": "$.key", "name": "$.value.name", "plan": "$.value.plan"}}});
    let (s, j) = post(&format!("{base}/v1/admin/connectors"), body).await;
    assert_eq!(s, 200, "{j}");
    rows_reach(&base, "users", 2, 30).await;
    until(15, "the last event of u1", || async {
        let r = sql(&base, "SELECT plan FROM users WHERE id = 'u1'").await;
        if r.first().is_some_and(|r| r["plan"] == "pro") {
            Ok(())
        } else {
            Err(format!("{r:?}"))
        }
    })
    .await;
    b.produce("t.users", None, vec![m("u2", None)]).unwrap();
    rows_reach(&base, "users", 1, 30).await;
    let c = connector(&base, "users-in").await;
    assert_eq!(c["stream"]["deleted"], 1, "{c}");
}

/// The test broker from the command center's routes: started on a
/// loopback address only, written to, and stopped.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_test_broker_routes() {
    let dir = tempfile::tempdir().unwrap();
    let (base, svc) = start(dir.path()).await;
    let route = format!("{base}/v1/admin/kafka/test-broker");
    assert_eq!(get(&route).await["running"], false);
    let (s, j) = post(&route, json!({"listen": "0.0.0.0:19099"})).await;
    assert_eq!(s, 400, "loopback only: {j}");
    let (s, j) = post(&route, json!({"listen": "127.0.0.1:0", "topic": "demo.clicks", "partitions": 2, "rate": 50})).await;
    assert_eq!(s, 200, "{j}");
    assert_eq!(j["running"], true, "{j}");
    let addr = j["address"].as_str().unwrap().to_string();
    let (s, j) = post(&format!("{route}/produce"), json!({"topic": "hand", "messages": [{"key": "k1", "value": {"a": 1}}, {"value": "plain text"}]})).await;
    assert_eq!(s, 200, "{j}");
    assert_eq!(j["offsets"].as_array().unwrap().len(), 2, "{j}");
    // Made-up clicks arrive at the rate asked, into a table of their own.
    let body = json!({"name": "clicks", "kind": "kafka", "target": "demo_clicks", "config": {
        "allow_internal": true, "brokers": addr, "topics": "demo.clicks", "max_wait_ms": 200, "create_target": true,
        "map": {"id": "$.value.event_id", "user_id": {"path": "$.value.user", "type": "KEYWORD"}, "action": {"path": "$.value.action", "type": "KEYWORD"}, "price": {"path": "$.value.price", "as": "float"}, "at": {"path": "$.value.at", "as": "time"}},
    }});
    let (s, j) = post(&format!("{base}/v1/admin/connectors"), body).await;
    assert_eq!(s, 200, "{j}");
    until(30, "clicks arriving", || async {
        let n = count(&base, "demo_clicks").await;
        if n >= 20 {
            Ok(())
        } else {
            Err(format!("{n}"))
        }
    })
    .await;
    let st = get(&route).await;
    assert!(st["generator"]["written"].as_u64().unwrap() >= 20, "{st}");
    assert!(st["fetches"].as_u64().unwrap() > 0, "{st}");
    let actions = sql(&base, "SELECT action, COUNT(*) AS n FROM demo_clicks GROUP BY action").await;
    assert!(actions.len() >= 2, "{actions:?}");
    // The rate set again while it runs.
    let (s, j) = post(&route, json!({"rate": 0})).await;
    assert_eq!(s, 200, "{j}");
    assert_eq!(j["generator"]["rate"], 0.0, "{j}");
    let (s, j) = call(reqwest::Method::DELETE, &route, None).await;
    assert_eq!(s, 200, "{j}");
    assert_eq!(get(&route).await["running"], false);
    until(60, "the connector failing once its broker is gone", || async {
        let c = connector(&base, "clicks").await;
        if c["health"] == "failing" {
            Ok(())
        } else {
            Err(c.to_string())
        }
    })
    .await;
    // Down on /metrics, and counted for the alert connector-failing.
    let m = get(&format!("{base}/metrics")).await;
    assert!(m.as_str().unwrap().contains("chloedb_connector_up{connector=\"clicks\",kind=\"kafka\"} 0"), "{m}");
    assert_eq!(svc.sample().gauges.get("connectors_failing"), Some(&1.0));
}

/// Against a real Kafka (`CHLOEDB_KAFKA=host:port`, topics made on first
/// write): produced with a Kafka client, read back, each event once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_real_kafka() {
    let Ok(addr) = std::env::var("CHLOEDB_KAFKA") else {
        eprintln!("CHLOEDB_KAFKA is not set: skipped");
        return;
    };
    let topic = format!("chloedb-test-{}", std::process::id());
    let client = rskafka::client::ClientBuilder::new(vec![addr.clone()]).build().await.unwrap();
    let ctl = client.controller_client().unwrap();
    ctl.create_topic(&topic, 3, 1, 10_000).await.unwrap();
    let mut sent = 0usize;
    for p in 0..3 {
        let pc = client.partition_client(topic.clone(), p, rskafka::client::partition::UnknownTopicHandling::Retry).await.unwrap();
        let recs: Vec<rskafka::record::Record> = events(sent, 100)
            .into_iter()
            .map(|m| rskafka::record::Record { key: m.key, value: m.value, headers: Default::default(), timestamp: rskafka::chrono::Utc::now() })
            .collect();
        sent += recs.len();
        pc.produce(recs, rskafka::client::partition::Compression::NoCompression).await.unwrap();
    }
    let dir = tempfile::tempdir().unwrap();
    let (base, _svc) = start(dir.path()).await;
    sql(&base, EVENTS).await;
    let mut body = kafka_body("real", &addr, json!({}));
    body["config"]["topics"] = json!(topic);
    let (s, j) = post(&format!("{base}/v1/admin/connectors"), body).await;
    assert_eq!(s, 200, "{j}");
    rows_reach(&base, "ev", 300, 60).await;
    post(&format!("{base}/v1/admin/connectors/real/stop"), json!({})).await;
    let pc = client.partition_client(topic.clone(), 1, rskafka::client::partition::UnknownTopicHandling::Retry).await.unwrap();
    let recs: Vec<rskafka::record::Record> =
        events(300, 25).into_iter().map(|m| rskafka::record::Record { key: m.key, value: m.value, headers: Default::default(), timestamp: rskafka::chrono::Utc::now() }).collect();
    pc.produce(recs, rskafka::client::partition::Compression::NoCompression).await.unwrap();
    post(&format!("{base}/v1/admin/connectors/real/start"), json!({})).await;
    rows_reach(&base, "ev", 325, 60).await;
    assert_eq!(once(&base).await, (325, 325));
    let _ = ctl.delete_topic(&topic, 10_000).await;
}
