//! A plan applied on the coordinator, as a job (`POST /v1/admin/plan/apply`):
//! it runs its steps in the background, each audited; it is kept in a file
//! beside the audit log, so that a coordinator started again finds it
//! interrupted and resumes it from its step; a copy's rows count every
//! table; and a change that failed says why in the audit log and on the
//! Timeline. Shard servers are real processes; the coordinator serves HTTP
//! in this process.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value as Json};
use chloedb::db::Db;
use chloedb::storage::DbOptions;
use chloedb_server::coord::Coordinator;
use chloedb_server::keys::Audit;
use chloedb_server::service::{AgentConfig, Backend, Service};
use chloedb_server::{demo, server};
use tokio::net::TcpListener;

const N: usize = 300;

/// A port free now, of a block of 20 this test process takes alone, below
/// Linux's ephemeral range (32768 and up): a port there could be taken by
/// another test's connection between this check and the server's bind
/// ("Address already in use" on the self-hosted runner).
fn port() -> u16 {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let base = 20_000 + (std::process::id() % 600) * 20;
    loop {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        assert!(n < 20, "this test process took its 20 ports");
        let p = (base + n) as u16;
        if std::net::TcpListener::bind(("127.0.0.1", p)).is_ok() {
            return p;
        }
    }
}

/// A numbered database of the demo products (`rows` of them).
fn create(path: &Path, rows: usize) {
    let opts = DbOptions { publish_snapshots: true, ship_log_bytes: 16 << 20, ..DbOptions::default() };
    let db = Db::create(path, demo::schema(N), opts).unwrap();
    if rows > 0 {
        db.set_embedder("emb", Arc::new(chloedb::embed::HashEmbedder { dim: demo::DIM })).unwrap();
        db.load((0..rows).map(|i| demo::product(i, 7))).unwrap();
    }
    db.storage().start_seq_no().unwrap();
}

/// A `chloedb-server` process of one database.
struct Node {
    path: PathBuf,
    log: PathBuf,
    http: u16,
    grpc: u16,
    child: Option<Child>,
}

impl Node {
    fn new(dir: &Path, name: &str, rows: usize) -> Node {
        let path = dir.join(format!("{name}.strata"));
        create(&path, rows);
        Node { path, log: dir.join(format!("{name}.log")), http: port(), grpc: port(), child: None }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.grpc)
    }

    async fn start(&mut self) {
        let log = std::fs::OpenOptions::new().create(true).append(true).open(&self.log).unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_chloedb-server"))
            .arg("--db")
            .arg(&self.path)
            .args(["--listen", &format!("127.0.0.1:{}", self.http), "--grpc", &format!("127.0.0.1:{}", self.grpc)])
            .args(["--seq-no", "--log", "info", "--failover-after-ms", "600", "--log-store", "off"])
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .unwrap();
        self.child = Some(child);
        let t0 = Instant::now();
        while client().get(format!("http://127.0.0.1:{}/v1/health", self.http)).send().await.is_err() {
            assert!(t0.elapsed() < Duration::from_secs(30), "{} did not start: {}", self.url(), std::fs::read_to_string(&self.log).unwrap_or_default());
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn client() -> reqwest::Client {
    chloedb_server::http_client().build().unwrap()
}

/// A coordinator of `urls`, its audit log at `audit`, serving HTTP here.
async fn coordinator(urls: Vec<String>, audit: &Path) -> (Arc<Service>, String) {
    let c = Arc::new(Coordinator::connect(urls).await.unwrap());
    let schema = c.schema.clone();
    let agent = AgentConfig { api_key: None, model: chloedb_server::agent::DEFAULT_MODEL.into(), base_url: "http://unused".into() };
    let mut svc = Service::new(Backend::Cluster(c), schema, agent, 16, Duration::from_secs(10));
    svc.audit = Audit::new(Some(audit.to_path_buf())).unwrap();
    let svc = Arc::new(svc);
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
    tokio::spawn(server::serve_http(svc.clone(), l, Default::default()));
    (svc, base)
}

async fn get(url: String) -> Json {
    client().get(url).send().await.unwrap().json().await.unwrap()
}

async fn post(url: String, body: Json) -> (u16, Json) {
    let r = client().post(url).header("x-strata-reason", "plan test").json(&body).send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or(Json::Null))
}

/// The plan run once it is no longer running.
async fn settled(base: &str) -> Json {
    let t0 = Instant::now();
    loop {
        let r = get(format!("{base}/v1/admin/plan/run")).await;
        if r["run"]["state"] != "running" {
            return r["run"].clone();
        }
        assert!(t0.elapsed() < Duration::from_secs(60), "the plan still runs: {r}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_plan_runs_on_the_coordinator_is_kept_and_resumed_after_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let audit = dir.path().join("audit.log");
    let (mut a, mut b) = (Node::new(dir.path(), "a", N), Node::new(dir.path(), "b", 0));
    a.start().await;
    b.start().await;
    let (_svc, base) = coordinator(vec![a.url()], &audit).await;

    // A second table: a copy's rows are those of every table.
    let (s, j) = post(format!("{base}/v1/sql"), json!({"sql": "CREATE TABLE notes (id KEYWORD PRIMARY KEY, body TEXT)"})).await;
    assert_eq!(s, 200, "{j}");
    let ops: Vec<Json> = (0..50).map(|i| json!({"op": "upsert", "table": "notes", "doc": {"id": format!("n{i}"), "body": "a note"}})).collect();
    let (s, j) = post(format!("{base}/v1/write"), json!({"ops": ops})).await;
    assert_eq!(s, 200, "{j}");
    let t0 = Instant::now();
    loop {
        let h = get(format!("http://127.0.0.1:{}/v1/health", a.http)).await;
        if h["rows"] == json!(N + 50) {
            assert_eq!(h["tables"], json!({"products": N, "notes": 50}), "{h}");
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(10), "{h}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // The plan: b joins, takes over, a leaves; b is sent the rows of both tables.
    let (s, p) = post(format!("{base}/v1/admin/plan"), json!({"shards": [{"shard": 0, "copies": [b.url()]}]})).await;
    assert_eq!(s, 200, "{p}");
    let kinds: Vec<&str> = p["steps"].as_array().unwrap().iter().map(|x| x["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["add_copy", "handover", "remove_copy"], "{p}");
    assert_eq!(p["steps"][0]["rows"], json!(N + 50), "rows of every table: {p}");
    let pre: Vec<&str> = p["preflight"].as_array().unwrap().iter().filter_map(|x| x["text"].as_str()).collect();
    let b_addr = b.url().trim_start_matches("http://").to_string();
    assert!(pre.iter().any(|t| t.starts_with(&format!("{b_addr} answers: version")) && t.contains("empty")), "the new server is asked: {pre:?}");

    let (s, j) = post(format!("{base}/v1/admin/plan/apply"), json!({"steps": p["steps"]})).await;
    assert_eq!(s, 200, "{j}");
    let id = j["run"]["id"].as_str().unwrap().to_string();
    // One plan at a time (the first waits for b to be in sync).
    let (s, j) = post(format!("{base}/v1/admin/plan/apply"), json!({"steps": [{"kind": "rebalance"}]})).await;
    assert_eq!(s, 409, "a second plan while one runs is refused: {j}");
    let run = settled(&base).await;
    assert_eq!(run["state"], "done", "{run}");
    assert_eq!(run["steps"].as_array().unwrap().iter().map(|x| x["state"].as_str().unwrap()).collect::<Vec<_>>(), ["done", "done", "done"], "{run}");
    assert_eq!(run["reason"], "plan test");
    let cluster = get(format!("{base}/v1/cluster")).await;
    assert_eq!(cluster["plan"]["id"], json!(id), "every view sees the plan: {}", cluster["plan"]);
    // Each copy says the limits it runs with, for Settings.
    let limits = &cluster["shards"][0]["replicas"][0]["detail"]["limits"];
    assert_eq!((limits["max_concurrent"].as_u64().is_some(), limits["timeout_ms"].as_u64().is_some()), (true, true), "{limits}");
    assert_eq!(get(format!("http://127.0.0.1:{}/v1/health", b.http)).await["replication"]["role"], "primary");

    // Each step is in the audit log, by the person who applied the plan; its end is one event.
    let log = get(format!("{base}/v1/admin/audit?kind=change")).await;
    let entries = log["entries"].as_array().unwrap();
    for k in 1..=3 {
        let e = entries.iter().find(|e| e["request_id"] == json!(format!("{id}-{k}"))).unwrap_or_else(|| panic!("step {k}: {log}"));
        assert_eq!((e["reason"].as_str(), e["after"]["plan"].as_str(), e["status"].as_u64()), (Some("plan test"), Some(id.as_str()), Some(200)), "{e}");
    }
    assert!(entries.iter().any(|e| e["route"] == "/v1/admin/plan/apply" && e["detail"].as_str().is_some_and(|d| d.starts_with("applied a plan of 3 steps"))), "{log}");
    let ev = get(format!("{base}/v1/events?kind=change")).await;
    assert!(ev["events"].as_array().unwrap().iter().any(|e| e["title"].as_str().unwrap().starts_with("Plan applied: 3 steps") && e["detail"]["plan"] == json!(id)), "{ev}");

    // The coordinator stopped during step 2: as the file it keeps says.
    let file = dir.path().join("audit.plan.json");
    let mut kept: Json = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    kept["state"] = json!("running");
    kept["at"] = json!(1);
    kept["finished_ms"] = Json::Null;
    kept["steps"][1]["state"] = json!("running");
    kept["steps"][2]["state"] = json!("waiting");
    std::fs::write(&file, serde_json::to_vec(&kept).unwrap()).unwrap();

    // A coordinator started again finds it interrupted, refuses another plan, and resumes it.
    let (_svc2, base2) = coordinator(vec![b.url()], &audit).await;
    let run = get(format!("{base2}/v1/admin/plan/run")).await["run"].clone();
    assert_eq!((run["state"].as_str(), run["at"].as_u64(), run["steps"][1]["state"].as_str()), (Some("interrupted"), Some(1), Some("interrupted")), "{run}");
    assert!(run["steps"][1]["note"].as_str().unwrap().contains("may be partly done"), "{run}");
    let ev = get(format!("{base2}/v1/events?kind=change")).await;
    assert!(ev["events"].as_array().unwrap().iter().any(|e| e["title"].as_str().unwrap().starts_with("A plan stopped at step 2 of 3 when the coordinator restarted") && e["severity"] == "warning"), "{ev}");
    let (s, j) = post(format!("{base2}/v1/admin/plan/apply"), json!({"steps": [{"kind": "rebalance"}]})).await;
    assert_eq!(s, 409, "an interrupted plan is resumed or discarded first: {j}");
    let (s, j) = post(format!("{base2}/v1/admin/plan/run/resume"), json!({})).await;
    assert_eq!(s, 200, "{j}");
    let run = settled(&base2).await;
    // Each step looks at what is done already: b is the primary, a is gone.
    assert_eq!((run["state"].as_str(), run["resumed"].as_u64()), (Some("done"), Some(1)), "{run}");
    let audit2 = get(format!("{base2}/v1/admin/audit?kind=change")).await;
    assert!(audit2["entries"].as_array().unwrap().iter().any(|e| e["route"] == "/v1/admin/plan/run/resume" && e["object"] == json!(format!("plan {id}"))), "{audit2}");

    // Discard forgets it, and its file.
    let (s, j) = post(format!("{base2}/v1/admin/plan/run/discard"), json!({})).await;
    assert_eq!((s, j["discarded"]["id"].as_str()), (200, Some(id.as_str())), "{j}");
    assert!(!file.exists());
    assert_eq!(get(format!("{base2}/v1/admin/plan/run")).await["run"], Json::Null);

    // A change that fails says what it tried and why, in the audit log and on the Timeline.
    let (s, j) = post(format!("{base2}/v1/admin/shards/0/replicas"), json!({"url": "http://127.0.0.1:9"})).await;
    assert!(s >= 400, "{j}");
    let why = j["error"].as_str().unwrap().to_string();
    let log = get(format!("{base2}/v1/admin/audit?kind=change")).await;
    let e = log["entries"].as_array().unwrap().iter().find(|e| e["route"] == "/v1/admin/shards/:i/replicas" && e["status"].as_u64() >= Some(400)).cloned().unwrap();
    assert_eq!((e["detail"].as_str(), e["object"].as_str(), e["after"]["error"].as_str()), (Some("add http://127.0.0.1:9 to shard 0"), Some("shard 0"), Some(why.as_str())), "{e}");
    let ev = get(format!("{base2}/v1/events?kind=change")).await;
    let t = ev["events"].as_array().unwrap().iter().find(|e| e["detail"]["route"] == "/v1/admin/shards/:i/replicas").cloned().unwrap();
    assert_eq!(t["title"], json!(format!("open: add http://127.0.0.1:9 to shard 0 (it failed: {why})")), "{t}");
    assert_eq!((t["severity"].as_str(), t["where"]["shard"].as_u64()), (Some("warning"), Some(0)));
}
