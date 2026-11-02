//! What the rebuilt command center reads of a cluster: the command
//! center's own requests tagged and left out of the clients' traffic, an
//! audit log of changes only, each shard's capability (whether it serves
//! reads and takes writes, and why not) with copies named and a dead
//! primary that is no longer anyone's primary, fleet health, events, time
//! beyond an hour, and the images a version picker offers.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value as Json};
use strata::db::Db;
use strata::storage::DbOptions;
use strata_server::coord::Coordinator;
use strata_server::service::{AgentConfig, Backend, Service};
use strata_server::{demo, server};
use tokio::net::TcpListener;

const N: usize = 300;
const AFTER_MS: u64 = 600;

/// A port free now, of a block of 20 this test process takes alone (by
/// its process id), as `tests/failover.rs` takes them.
fn port() -> u16 {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let base = 20_000 + (std::process::id() % 500) * 20;
    loop {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        assert!(n < 20, "this test process took its 20 ports");
        let p = (base + n) as u16;
        if std::net::TcpListener::bind(("127.0.0.1", p)).is_ok() {
            return p;
        }
    }
}

fn agent() -> AgentConfig {
    AgentConfig { api_key: None, model: strata_server::agent::DEFAULT_MODEL.into(), base_url: "http://unused".into() }
}

/// A numbered database of the demo products (`rows` of them).
fn create(path: &Path, rows: usize) {
    let opts = DbOptions { publish_snapshots: true, ship_log_bytes: 16 << 20, ..DbOptions::default() };
    let db = Db::create(path, demo::schema(N), opts).unwrap();
    if rows > 0 {
        db.set_embedder("emb", Arc::new(strata::embed::HashEmbedder { dim: demo::DIM })).unwrap();
        db.load((0..rows).map(|i| demo::product(i, 7))).unwrap();
    }
    db.storage().start_seq_no().unwrap();
}

/// A `strata-server` process of one database, with failover.
struct Node {
    name: String,
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
        Node { name: name.into(), path, log: dir.join(format!("{name}.log")), http: port(), grpc: port(), child: None }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.grpc)
    }

    async fn start(&mut self) {
        let log = std::fs::OpenOptions::new().create(true).append(true).open(&self.log).unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_strata-server"))
            .arg("--db")
            .arg(&self.path)
            .args(["--listen", &format!("127.0.0.1:{}", self.http), "--grpc", &format!("127.0.0.1:{}", self.grpc)])
            .args(["--seq-no", "--log", "info", "--failover-after-ms", &AFTER_MS.to_string(), "--log-store", "off"])
            .args(["--node-name", &format!("node-{}", self.name)])
            .env("STRATA_ZONE", "zone-a")
            .env("STRATA_IMAGE", format!("ghcr.io/chloetech-ai/strata-server@sha256:{}", "a".repeat(64)))
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .unwrap();
        self.child = Some(child);
        let t0 = Instant::now();
        while self.health().await.is_none() {
            assert!(t0.elapsed() < Duration::from_secs(30), "{} did not start: {}", self.url(), self.logs());
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    fn kill(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    async fn health(&self) -> Option<Json> {
        let r = strata_server::http_client().build().unwrap().get(format!("http://127.0.0.1:{}/v1/health", self.http)).send().await.ok()?;
        r.json().await.ok()
    }

    async fn replication(&self) -> Json {
        self.health().await.map_or(Json::Null, |h| h["replication"].clone())
    }

    fn logs(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        self.kill();
    }
}

async fn until<F: std::future::Future<Output = bool>>(what: &str, nodes: &[&Node], f: impl Fn() -> F) {
    let t0 = Instant::now();
    while !f().await {
        if t0.elapsed() > Duration::from_secs(30) {
            let logs: Vec<String> = nodes.iter().map(|n| format!("--- {}\n{}", n.url(), n.logs())).collect();
            panic!("waited 30 s for: {what}\n{}", logs.join("\n"));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// A coordinator of `url`'s shard, and a server of it.
async fn coordinator(url: String) -> (Arc<Coordinator>, Arc<Service>) {
    let c = Arc::new(Coordinator::connect(vec![url]).await.unwrap());
    let schema = c.schema.clone();
    (c.clone(), Arc::new(Service::new(Backend::Cluster(c), schema, agent(), 16, Duration::from_secs(10))))
}

/// The cluster as a fresh survey finds it.
async fn fresh(c: &Coordinator) -> Json {
    (*c.snapshot_within(Duration::ZERO).await).clone()
}

/// A shard of `nodes`: the first its primary, the others replicas the
/// coordinator adds, each in sync.
async fn shard_of(nodes: &mut [&mut Node]) -> (Arc<Coordinator>, Arc<Service>) {
    for n in nodes.iter_mut() {
        n.start().await;
    }
    let (coord, svc) = coordinator(nodes[0].url()).await;
    for n in nodes.iter().skip(1) {
        coord.add_replica(0, &n.url(), false).await.unwrap();
    }
    let want = nodes.len() - 1;
    let primary: &Node = nodes[0];
    until("the replicas are in sync and vote", &[primary], || async {
        let r = primary.replication().await;
        r["replicas"].as_array().is_some_and(|v| v.len() == want && v.iter().all(|f| f["in_sync"] == true && f["member"] == true))
    })
    .await;
    (coord, svc)
}

/// A shard of two copies with failover that loses its replica serves no
/// reads: the primary alone is no majority. `/v1/cluster` says so, and
/// why, rather than "degraded"; the dead copy's role is unknown.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_two_copy_shard_that_loses_a_copy_serves_no_reads() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = Node::new(dir.path(), "a", N);
    let mut b = Node::new(dir.path(), "b", 0);
    let (coord, svc) = shard_of(&mut [&mut a, &mut b]).await;

    let c = fresh(&coord).await;
    let s = &c["shards"][0];
    assert_eq!((s["state"].as_str(), s["serves_reads"].as_bool(), s["takes_writes"].as_bool(), s["majority"].as_bool(), s["failover"].as_bool()), (Some("ok"), Some(true), Some(true), Some(true), Some(true)), "{s}");
    assert_eq!(s["primary"], json!(a.url()), "{s}");
    let copies = s["replicas"].as_array().unwrap();
    assert_eq!(copies.iter().map(|r| r["short"].as_str().unwrap()).collect::<Vec<_>>(), ["s0-0", "s0-1"]);
    assert_eq!(copies[0]["name"], "node-a");
    assert_eq!(copies[0]["zone"], "zone-a");
    assert_eq!(copies[0]["version"]["digest"], json!(format!("sha256:{}", "a".repeat(64))));
    assert!(copies[0]["version"]["version"].is_string() && copies[0]["last_seen_ms"].as_u64().unwrap() > 0, "{}", copies[0]);
    assert!(c["at_ms"].as_u64().is_some());
    // One snapshot is shared: asked again at once, the same survey.
    let (x, y) = (coord.snapshot().await, coord.snapshot().await);
    assert!(Arc::ptr_eq(&x, &y));

    b.kill();
    until("the shard serves no reads", &[&a], || async { fresh(&coord).await["shards"][0]["state"] == "no_reads" }).await;
    let c = svc.cluster().await;
    let s = &c["shards"][0];
    assert_eq!((s["serves_reads"].as_bool(), s["majority"].as_bool(), s["status"].as_str()), (Some(false), Some(false), Some("degraded")), "{s}");
    let why = s["why"].as_str().unwrap();
    assert!(why.starts_with("1 of 2 copies answers") && why.contains("no majority"), "{why}");
    let dead = &s["replicas"][1];
    assert_eq!((dead["role"].as_str(), dead["last_role"].as_str(), dead["takes_writes"].as_bool()), (Some("unknown"), Some("replica"), Some(false)), "{dead}");
    assert!(dead["down_since_ms"].as_u64().unwrap() > 0);
    assert_eq!(c["status"], "down", "a shard serving no reads fails every search");
    assert!(c["cluster"].is_object(), "the cluster's name and environment");

    // Fleet health leads with it: what fails, where, and what to do.
    let f = svc.fleet_health().await;
    assert_eq!((f["status"].as_str(), f["headline"].as_str()), (Some("critical"), Some("Searches are failing: shard 0 has no majority")), "{f}");
    let p = &f["problems"][0];
    assert_eq!(p["where"], json!(["shard 0", "s0-1"]));
    assert_eq!(p["next"]["text"], "Bring s0-1 back, or remove it so s0-0 has a majority");
    assert_eq!(p["since_ms"], dead["down_since_ms"]);
    let check = |id: &str| f["checks"].as_array().unwrap().iter().find(|c| c["id"] == id).cloned().unwrap();
    assert_eq!(check("shards")["value"]["no_reads"], 1);
    assert_eq!((check("copies")["down"].as_u64(), check("copies")["total"].as_u64()), (Some(1), Some(2)));
    assert_eq!(f["coordinators"][0]["this"], true);
}

/// A primary that dies is no longer the shard's primary in `/v1/cluster`
/// (its role is unknown), and the copy elected in its place is.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_dead_primary_loses_its_role() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = Node::new(dir.path(), "a", N);
    let mut b = Node::new(dir.path(), "b", 0);
    let mut c = Node::new(dir.path(), "c", 0);
    let (coord, svc) = shard_of(&mut [&mut a, &mut b, &mut c]).await;
    assert_eq!(fresh(&coord).await["shards"][0]["primary"], json!(a.url()));
    // Writes the replicas catch up with: a copy that has not grants no votes.
    for i in 0..10 {
        svc.write_ops(vec![json!({"op": "set", "key": format!("p{i:07}"), "field": "sold", "value": i})], None).await.unwrap();
    }

    a.kill();
    until("another copy is the primary", &[&b, &c], || async {
        let s = fresh(&coord).await["shards"][0].clone();
        s["primary"].is_string() && s["primary"] != json!(a.url())
    })
    .await;
    let s = fresh(&coord).await["shards"][0].clone();
    let old = &s["replicas"][0];
    assert_eq!((old["role"].as_str(), old["last_role"].as_str(), old["takes_writes"].as_bool()), (Some("unknown"), Some("primary"), Some(false)), "{old}");
    let primaries = s["replicas"].as_array().unwrap().iter().filter(|r| r["takes_writes"] == true).count();
    assert_eq!(primaries, 1, "{s}");
    assert_eq!(s["state"], "degraded", "{s}");
    assert!(s["why"].as_str().unwrap().contains("s0-0 down"), "{s}");
}

async fn listen() -> (TcpListener, String) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
    (l, url)
}

/// A server of the demo products over HTTP, in this process.
async fn local(dir: &Path, tune: impl FnOnce(&mut Service)) -> (Arc<Service>, String) {
    let db = Db::create(&dir.join("local.strata"), demo::schema(N), DbOptions { publish_snapshots: true, ..DbOptions::default() }).unwrap();
    db.set_embedder("emb", Arc::new(strata::embed::HashEmbedder { dim: demo::DIM })).unwrap();
    db.load((0..N).map(|i| demo::product(i, 7))).unwrap();
    let schema = (*db.schema()).clone();
    let mut svc = Service::new(Backend::Local(Arc::new(db)), schema, agent(), 16, Duration::from_secs(10));
    tune(&mut svc);
    let svc = Arc::new(svc);
    let (l, url) = listen().await;
    tokio::spawn(server::serve_http(svc.clone(), l, Default::default()));
    (svc, url)
}

fn client() -> reqwest::Client {
    strata_server::http_client().build().unwrap()
}

/// The command center's requests (`x-strata-client: command-center`) are
/// left out of `/v1/routes`; reads are not audited, changes are, with the
/// client, its address and the reason it gave.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_command_centers_requests_are_tagged_and_only_changes_audited() {
    let dir = tempfile::tempdir().unwrap();
    let (_svc, base) = local(dir.path(), |s| {
        s.identity = strata_server::service::Identity { name: Some("prod/search".into()), environment: Some("production".into()), public_url: None };
    })
    .await;
    let ui = |r: reqwest::RequestBuilder| r.header("x-strata-client", "command-center");
    for _ in 0..3 {
        assert!(ui(client().get(format!("{base}/v1/queries"))).send().await.unwrap().status().is_success());
        assert!(ui(client().get(format!("{base}/v1/admin/audit"))).send().await.unwrap().status().is_success());
    }
    client().get(format!("{base}/v1/stats")).header("user-agent", "nightly-reports/2.3 (linux)").send().await.unwrap();
    let routes: Json = client().get(format!("{base}/v1/routes")).send().await.unwrap().json().await.unwrap();
    let names: Vec<&str> = routes["http"].as_array().unwrap().iter().map(|r| r["route"].as_str().unwrap()).collect();
    assert!(names.contains(&"/v1/stats") && !names.contains(&"/v1/queries") && !names.contains(&"/v1/admin/audit"), "{names:?}");

    // A change, with a reason (encoded, as the command center sends one).
    let r = ui(client().post(format!("{base}/v1/admin/checkpoint")).header("x-strata-reason", "before%20the%20upgrade%20%E2%9C%93")).json(&json!({"name": "x"})).send().await.unwrap();
    assert!(r.status().is_client_error(), "no backup dir: refused, and still audited as a change asked for");
    let audit: Json = client().get(format!("{base}/v1/admin/audit")).send().await.unwrap().json().await.unwrap();
    let entries = audit["entries"].as_array().unwrap();
    assert!(entries.iter().all(|e| e["method"] != "GET"), "reads are not audited: {entries:?}");
    let e = entries.iter().find(|e| e["route"] == "/v1/admin/checkpoint").unwrap();
    assert_eq!((e["kind"].as_str(), e["client"].as_str(), e["reason"].as_str(), e["addr"].as_str(), e["actor"].as_str()), (Some("change"), Some("command-center"), Some("before the upgrade ✓"), Some("127.0.0.1"), Some("person")), "{e}");

    let me: Json = ui(client().get(format!("{base}/v1/whoami"))).send().await.unwrap().json().await.unwrap();
    assert_eq!(me["cluster"], json!({"name": "prod/search", "environment": "production", "public_url": null}));
    assert_eq!(me["caller"]["client"], "command-center");

    // A shard server's fleet health is of itself.
    let f: Json = client().get(format!("{base}/v1/health/fleet")).send().await.unwrap().json().await.unwrap();
    assert_eq!(f["api"], json!({"ok": true, "server": "shard"}));
    let shards = f["checks"].as_array().unwrap().iter().find(|c| c["id"] == "shards").cloned().unwrap();
    assert_eq!((shards["status"].as_str(), shards["value"]["total"].as_u64()), (Some("ok"), Some(1)), "{f}");
}
