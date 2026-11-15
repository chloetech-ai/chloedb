//! `chloedb-server cc`, the command center in a terminal, against a real
//! cluster: a shard of two server processes (a primary and its replica)
//! under a coordinator that serves HTTP in this process. Each screen is
//! drawn from what the routes answered, a handover is made from the
//! Cluster screen's keys, with the reason typed reaching the audit log, a
//! statement runs from the SQL screen, and the program refuses to start
//! without a terminal.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chloedb::db::Db;
use chloedb::storage::DbOptions;
use chloedb_server::cc::api::{self, Act, Api, Job, Shared, Who};
use chloedb_server::cc::app::{App, Effect, Screen, Seen};
use chloedb_server::cc::view;
use chloedb_server::coord::Coordinator;
use chloedb_server::keys::Audit;
use chloedb_server::service::{AgentConfig, Backend, Service};
use chloedb_server::{demo, server};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::Value as Json;
use tokio::net::TcpListener;
use tokio::sync::Notify;

const N: usize = 200;

/// A port free now, from a block of 20 this test process takes alone.
fn port() -> u16 {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let base = 30_000 + (std::process::id() % 100) * 20;
    loop {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        assert!(n < 20, "this test process took its 20 ports");
        let p = (base + n) as u16;
        if std::net::TcpListener::bind(("127.0.0.1", p)).is_ok() {
            return p;
        }
    }
}

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
    log: PathBuf,
    http: u16,
    grpc: u16,
    child: Child,
}

impl Node {
    async fn start(dir: &Path, name: &str, rows: usize) -> Node {
        let path = dir.join(format!("{name}.strata"));
        create(&path, rows);
        let (http, grpc) = (port(), port());
        let log = dir.join(format!("{name}.log"));
        let child = Command::new(env!("CARGO_BIN_EXE_chloedb-server"))
            .arg("--db")
            .arg(&path)
            .args(["--listen", &format!("127.0.0.1:{http}"), "--grpc", &format!("127.0.0.1:{grpc}")])
            .args(["--seq-no", "--log", "info", "--failover-after-ms", "600", "--log-store", "off", "--start-apps", "off", "--no-fetch-models"])
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(&log).unwrap())
            .spawn()
            .unwrap();
        let n = Node { log, http, grpc, child };
        let t0 = Instant::now();
        while n.role().await.is_none() {
            assert!(t0.elapsed() < Duration::from_secs(30), "{name} did not start: {}", n.logs());
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        n
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.grpc)
    }

    async fn replication(&self) -> Option<Json> {
        let r = chloedb_server::http_client().build().unwrap().get(format!("http://127.0.0.1:{}/v1/health", self.http)).send().await.ok()?;
        r.json::<Json>().await.ok().map(|h| h["replication"].clone())
    }

    async fn role(&self) -> Option<String> {
        self.replication().await.map(|r| r["role"].as_str().unwrap_or("?").to_string())
    }

    fn logs(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

async fn until<F: std::future::Future<Output = bool>>(what: &str, f: impl Fn() -> F) {
    let t0 = Instant::now();
    while !f().await {
        assert!(t0.elapsed() < Duration::from_secs(30), "waited 30 s for: {what}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

fn press(app: &mut App, shared: &Mutex<Shared>, k: KeyEvent) -> Effect {
    let s = api::lock(shared);
    let seen = Seen { data: &s.data, who: &s.who, base: &s.base, hub: s.hub.as_ref(), plan_running: view::plan_running(&s.data.cluster) };
    app.key(k, &seen)
}

/// Shows a screen: reads its routes once, then draws it.
async fn show(app: &mut App, api: &Api, shared: &Mutex<Shared>, screen: Screen) -> String {
    app.go(screen);
    api::lock(shared).want = api::Want { screen, ..api::Want::default() };
    api::read_round(api, shared).await;
    let s = api::lock(shared);
    assert!(s.errors.is_empty(), "{:?}", s.errors);
    view::render(app, &s, 120, 34)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_terminal_reads_a_cluster_and_hands_its_primary_over() {
    let dir = tempfile::tempdir().unwrap();
    let a = Node::start(dir.path(), "a", N).await;
    let b = Node::start(dir.path(), "b", 0).await;
    let coord = Arc::new(Coordinator::connect(vec![a.url()]).await.unwrap());
    coord.add_replica(0, &b.url(), false, false).await.unwrap();
    until("the replica is in sync and votes", || async { a.replication().await.is_some_and(|r| r["replicas"][0]["in_sync"] == true && r["replicas"][0]["member"] == true) }).await;
    let agent = AgentConfig { api_key: None, model: chloedb_server::agent::DEFAULT_MODEL.into(), base_url: "http://unused".into() };
    let mut svc = Service::new(Backend::Cluster(coord.clone()), coord.schema.clone(), agent, 16, Duration::from_secs(10));
    let audit = dir.path().join("audit.jsonl");
    svc.audit = Audit::new(Some(audit.clone())).unwrap();
    let svc = Arc::new(svc);
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
    tokio::spawn(server::serve_http(svc.clone(), l, Default::default()));

    let api = Api::new(&base, None, None).unwrap();
    let who = Who::of(&api.get("/v1/whoami").await.unwrap());
    assert!(who.can("admin"), "an open server's caller is its root: {who:?}");
    let shared = Arc::new(Mutex::new(Shared::new(&base, who)));
    let mut app = App::default();

    // Home and Cluster, from /v1/health/fleet and /v1/cluster.
    let home = show(&mut app, &api, &shared, Screen::Home).await;
    assert!(home.contains("Copies answering") && home.contains("2/2"), "{home}");
    assert!(home.contains(" SANDBOX "), "a server on this machine with no environment is a sandbox: {home}");
    let map = show(&mut app, &api, &shared, Screen::Cluster).await;
    assert!(map.contains("shard 0") && map.contains("◆ ●") && map.contains("2/2 serving"), "{map}");
    // A copy has the name the server's own texts give it (`127.0.0.1:<port>` here).
    let (ida, idb) = (chloedb_server::coord::copy_short(0, &a.url()), chloedb_server::coord::copy_short(0, &b.url()));
    assert!(map.contains(&format!("◆ {ida}")) && map.contains("primary") && map.contains(&format!("● {idb}")) && map.contains("replica"), "{map}");

    // Tables and storage, from /v1/index/tables (a coordinator asks each shard).
    let t = show(&mut app, &api, &shared, Screen::Tables).await;
    assert!(t.contains("products") && t.contains("200"), "{t}");
    assert!(t.contains("products by shard") && t.contains("shard 0"), "{t}");

    // A statement from the SQL screen, then the Queries screen shows it.
    app.go(Screen::Sql);
    app.sql = chloedb_server::cc::app::Input::of("SELECT id, title FROM products ORDER BY id LIMIT 3");
    let Effect::Run(job) = press(&mut app, &shared, KeyEvent::from(KeyCode::Enter)) else { panic!("a SELECT runs at once") };
    api::run_job(api.clone(), shared.clone(), Arc::new(Notify::new()), job).await;
    let sql = show(&mut app, &api, &shared, Screen::Sql).await;
    assert!(sql.contains("3 rows") && sql.contains("p0000000") && sql.contains("title"), "{sql}");
    let q = show(&mut app, &api, &shared, Screen::Queries).await;
    assert!(q.contains("SELECT id, title FROM products ORDER BY id LIMIT ?"), "{q}");

    // Alerts, Logs and the Timeline read without failing.
    let al = show(&mut app, &api, &shared, Screen::Alerts).await;
    assert!(al.contains("rules · worst first"), "{al}");
    show(&mut app, &api, &shared, Screen::Logs).await;
    show(&mut app, &api, &shared, Screen::Timeline).await;

    // The handover: the replica picked, p, a reason typed, Enter.
    show(&mut app, &api, &shared, Screen::Cluster).await;
    press(&mut app, &shared, KeyEvent::from(KeyCode::Down));
    assert_eq!(press(&mut app, &shared, key('p')), Effect::None);
    for c in "restart a for the kernel".chars() {
        press(&mut app, &shared, key(c));
    }
    let Effect::Run(job) = press(&mut app, &shared, KeyEvent::from(KeyCode::Enter)) else { panic!("the dialog hands it over") };
    assert_eq!(job, Job { act: Act::Promote { shard: 0, url: b.url(), id: idb.clone() }, reason: Some("restart a for the kernel".into()) });
    api::run_job(api.clone(), shared.clone(), Arc::new(Notify::new()), job).await;
    let note = api::lock(&shared).note.clone().unwrap();
    assert_eq!(note.tone, api::Tone::Good, "{}", note.text);
    assert!(note.text.starts_with(&format!("{idb} is shard 0's primary")), "{}", note.text);
    until("b is the primary and a follows it", || async { b.role().await.as_deref() == Some("primary") && a.role().await.as_deref() == Some("replica") }).await;
    let map = show(&mut app, &api, &shared, Screen::Cluster).await;
    assert!(map.contains(&format!("{idb} is shard 0's primary")), "the status bar says what happened: {map}");
    let audited = std::fs::read_to_string(&audit).unwrap();
    assert!(audited.lines().any(|l| l.contains("the primary of shard 0") && l.contains("restart a for the kernel")), "the reason is in the audit log: {audited}");
    assert!(audited.contains("chloedb-cc"), "the audit log names the app: {audited}");
}

/// Without a terminal it says what to use instead, and draws nothing.
#[test]
fn without_a_terminal_it_points_at_admin() {
    let out = Command::new(env!("CARGO_BIN_EXE_chloedb-server"))
        .args(["cc", "http://127.0.0.1:9"])
        .env("CHLOEDB_KEY", "sk_unused")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("cc draws in a terminal") && err.contains("chloedb-server admin"), "{err}");
    assert!(out.stdout.is_empty());
}
