//! What a coordinator started as "Operate a cluster" starts it (no
//! `--alerts`, no `--audit-log`) keeps across a restart, and what on-call
//! reads through it (findings of the persona study's second round, 17
//! November 2026): its alert rules, routes, silences and the latest
//! entries of its audit log, kept on its shards (OLU-201); `/metrics`
//! totals that match `/v1/health` on a copy, and the cluster's state in a
//! coordinator's `/metrics`, and on a server alone (OLU-202, OLU-3); the runbook's calls through
//! a coordinator, `POST /v1/admin/verify` comparing every shard and `GET
//! /v1/logs` saying where the copies' lines are (OLU-206); and the
//! checkpoints of every copy, listed (OLU-211). And of the third round
//! (18 November 2026): the Timeline's events, kept on the shards too
//! (OLU-301), and a coordinator started again on its first `--shards`
//! after a copy was replaced serving the cluster as it is (OLU-302).
//! Server processes, as an operator runs them on machines of their own.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use serde_json::{json, Value as Json};

const CLUSTER: &str = "kept-cluster-secret-1234";
const ADMIN: &str = "kept-admin-token-1234";
const SCHEMA: &str = r#"{"table": "acks", "key": "id", "fields": [{"name": "id", "type": "keyword"}, {"name": "n", "type": "int"}]}"#;
const WAIT: Duration = Duration::from_secs(60);

/// A port free now, from a block this test process starts at (by its
/// process id): a server started again binds the same ports.
fn port() -> u16 {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let base = (std::process::id() % 500) * 40 + 23;
    loop {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        assert!(n < 400, "this test process tried 400 ports and found none free");
        let p = (10_000 + (base + n) % 20_000) as u16;
        if std::net::TcpListener::bind(("127.0.0.1", p)).is_ok() {
            return p;
        }
    }
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_chloedb-server")
}

async fn http(base: &str, method: &str, path: &str, auth: &str, body: Option<Json>) -> (u16, Json) {
    let c = chloedb_server::http_client().timeout(Duration::from_secs(30)).build().unwrap();
    let mut r = match method {
        "GET" => c.get(format!("{base}{path}")),
        "PUT" => c.put(format!("{base}{path}")),
        _ => c.post(format!("{base}{path}")),
    };
    if !auth.is_empty() {
        r = r.bearer_auth(auth);
    }
    if let Some(b) = body {
        r = r.json(&b);
    }
    match r.send().await {
        Ok(r) => (r.status().as_u16(), r.json().await.unwrap_or(Json::Null)),
        Err(e) => (0, json!({"error": e.to_string()})),
    }
}

/// `/metrics` of `base`, as text.
async fn metrics(base: &str, auth: &str) -> String {
    let c = chloedb_server::http_client().timeout(Duration::from_secs(30)).build().unwrap();
    let mut r = c.get(format!("{base}/metrics"));
    if !auth.is_empty() {
        r = r.bearer_auth(auth);
    }
    let r = r.send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200);
    r.text().await.unwrap()
}

/// The value of the series `name` (with its labels as written, `name{a="b"}`).
fn value(text: &str, name: &str) -> Option<f64> {
    text.lines().find_map(|l| l.strip_prefix(name).and_then(|r| r.strip_prefix(' ')).and_then(|v| v.trim().parse().ok()))
}

/// One copy of a shard, in a folder of its own, with a backup folder.
struct Copy {
    name: String,
    disk: PathBuf,
    log: PathBuf,
    http: u16,
    grpc: u16,
    role: Vec<String>,
    child: Option<Child>,
}

impl Copy {
    fn new(dir: &Path, name: &str) -> Copy {
        let disk = dir.join(name);
        std::fs::create_dir_all(&disk).unwrap();
        Copy { name: name.into(), log: dir.join(format!("{name}.log")), disk, http: port(), grpc: port(), role: Vec::new(), child: None }
    }

    fn db(&self) -> PathBuf {
        self.disk.join("acks.chloedb")
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.grpc)
    }

    fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.http)
    }

    async fn start(&mut self) {
        let log = std::fs::OpenOptions::new().create(true).append(true).open(&self.log).unwrap();
        let mut cmd = Command::new(bin());
        cmd.arg("--db").arg(self.db()).args(["--listen", &format!("127.0.0.1:{}", self.http), "--grpc", &format!("127.0.0.1:{}", self.grpc)]);
        cmd.args(["--seq-no", "--log", "info", "--failover-after-ms", "1500", "--cluster-token", CLUSTER, "--start-apps", "off", "--node-name", &self.name]);
        cmd.arg("--backup-dir").arg(self.disk.join("backups")).args(&self.role);
        if !self.db().exists() {
            cmd.args(["--schema", SCHEMA]);
        }
        self.child = Some(cmd.stdout(Stdio::null()).stderr(log).spawn().unwrap());
        let t0 = Instant::now();
        while http(&self.base(), "GET", "/v1/health", CLUSTER, None).await.0 != 200 {
            assert!(t0.elapsed() < WAIT, "{} did not start: {}", self.name, std::fs::read_to_string(&self.log).unwrap_or_default());
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// What the coordinator keeps on this copy, as its file holds it.
    fn kept(&self) -> Json {
        let mut p = self.db().into_os_string();
        p.push(".coordinator.json");
        std::fs::read(&p).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Json::Null)
    }
}

impl Drop for Copy {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// One shard of a primary and a replica, and a coordinator started as the
/// guide starts it: no `--alerts`, `--audit-log` nor `--log-store`.
struct Cluster {
    dir: PathBuf,
    copies: Vec<Copy>,
    http: u16,
    coordinator: Option<Child>,
}

impl Cluster {
    async fn start(dir: &Path) -> Cluster {
        Cluster::start_with(dir, 2).await
    }

    /// A shard of `n` copies, the first its primary.
    async fn start_with(dir: &Path, n: usize) -> Cluster {
        let mut copies: Vec<Copy> = (0..n).map(|k| Copy::new(dir, &format!("s0{}", (b'a' + k as u8) as char))).collect();
        let replicas: Vec<String> = copies[1..].iter().map(Copy::url).collect();
        copies[0].role = vec!["--replicas".into(), replicas.join(",")];
        let primary = copies[0].url();
        for c in &mut copies[1..] {
            c.role = vec!["--replica".into(), "--replicas".into(), primary.clone()];
        }
        for c in &mut copies {
            c.start().await;
        }
        let mut c = Cluster { dir: dir.to_path_buf(), copies, http: port(), coordinator: None };
        c.start_coordinator().await;
        c
    }

    fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.http)
    }

    async fn start_coordinator(&mut self) {
        let list = self.copies.iter().map(Copy::url).collect::<Vec<_>>().join("|");
        self.start_coordinator_on(&list).await;
    }

    /// Started on `list` as `--shards`, as the guide starts it.
    async fn start_coordinator_on(&mut self, list: &str) {
        let log = std::fs::OpenOptions::new().create(true).append(true).open(self.dir.join("coordinator.log")).unwrap();
        let child = Command::new(bin())
            .args(["--shards", list, "--listen", &format!("127.0.0.1:{}", self.http), "--grpc", "off", "--log", "info", "--token", ADMIN, "--cluster-token", CLUSTER, "--start-apps", "off", "--node-name", "coord"])
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .unwrap();
        self.coordinator = Some(child);
        self.until("the coordinator serves", || async { self.call("GET", "/v1/cluster", None).await.0 == 200 }).await;
    }

    /// Killed, as a machine that loses its power: no last word to its shards.
    fn kill_coordinator(&mut self) {
        if let Some(mut c) = self.coordinator.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    async fn call(&self, method: &str, path: &str, body: Option<Json>) -> (u16, Json) {
        http(&self.base(), method, path, ADMIN, body).await
    }

    fn logs(&self) -> String {
        let mut s = std::fs::read_to_string(self.dir.join("coordinator.log")).unwrap_or_default();
        for c in &self.copies {
            s += &format!("\n--- {}\n{}", c.name, std::fs::read_to_string(&c.log).unwrap_or_default());
        }
        s
    }

    async fn until<F: std::future::Future<Output = bool>>(&self, what: &str, f: impl Fn() -> F) {
        let t0 = Instant::now();
        while !f().await {
            if t0.elapsed() > WAIT {
                panic!("waited {} s for: {what}\n{}", WAIT.as_secs(), self.logs());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

impl Drop for Cluster {
    fn drop(&mut self) {
        self.kill_coordinator();
    }
}

/// OLU-201: a coordinator without `--alerts` and `--audit-log` lost its
/// custom rules, its routes, its silences and its audit log at each
/// restart. It keeps them on every copy of its shards, and takes them back
/// before it serves: killed and started again, it has them all.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_coordinator_without_files_keeps_its_rules_routes_and_audit_on_its_shards() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = Cluster::start(dir.path()).await;
    let (s, a) = c.call("GET", "/v1/alerts", None).await;
    assert_eq!(s, 200, "{a}");
    assert_eq!(a["kept"]["memory_only"], false, "the rules are kept on the shards, not in memory alone: {}", a["kept"]);
    let mut rules: Vec<Json> = a["rules"].as_array().unwrap().iter().map(|r| {
        let mut r = r.clone();
        for k in ["status", "runbook_docs", "sql_run"] {
            r.as_object_mut().unwrap().remove(k);
        }
        r
    }).collect();
    rules.push(json!({"id": "lead-test", "name": "Lead test", "metric": "write_p99_ms", "op": ">", "threshold": 800, "severity": "warning"}));
    let (s, j) = c.call("PUT", "/v1/alerts/rules", Some(json!({"rules": rules}))).await;
    assert_eq!(s, 200, "{j}");
    let (s, j) = c.call("PUT", "/v1/alerts/routes", Some(json!({"routes": [{"name": "oncall-webhook", "url": "http://127.0.0.1:9/hook", "severities": ["critical"], "allow_internal": true}]}))).await;
    assert_eq!(s, 200, "{j}");
    let (s, j) = c.call("POST", "/v1/alerts/lead-test/silence", Some(json!({"minutes": 30, "reason": "drill"}))).await;
    assert_eq!(s, 200, "{j}");
    // Every copy holds them, and the audit entries of the three changes.
    c.until("every copy holds the rules, the route, the silence and three audit entries", || async {
        c.copies.iter().all(|k| {
            let j = k.kept();
            let rules = j["parts"]["rules"]["data"].to_string();
            let routes = j["parts"]["routes"]["data"].to_string();
            let states = j["parts"]["states"]["data"].to_string();
            rules.contains("lead-test") && routes.contains("oncall-webhook") && states.contains("drill") && j["audit"].as_array().map_or(0, Vec::len) >= 3
        })
    })
    .await;

    c.kill_coordinator();
    c.start_coordinator().await;
    let (_, a) = c.call("GET", "/v1/alerts", None).await;
    let ids: Vec<&str> = a["rules"].as_array().unwrap().iter().filter_map(|r| r["id"].as_str()).collect();
    assert!(ids.contains(&"lead-test"), "the custom rule after a restart: {ids:?}\n{}", c.logs());
    let routes: Vec<&str> = a["routes"].as_array().unwrap().iter().filter_map(|r| r["name"].as_str()).collect();
    assert_eq!(routes, vec!["oncall-webhook"], "{a}");
    let st = a["states"].as_array().unwrap().iter().find(|s| s["rule"] == "lead-test").cloned().unwrap_or_default();
    assert_eq!(st["silenced"]["reason"], "drill", "the silence after a restart: {st}");
    assert_eq!(a["kept"]["shards"]["started_with"]["took"], json!(["rules", "routes", "states"]), "{}", a["kept"]);
    let (s, au) = c.call("GET", "/v1/admin/audit?limit=100", None).await;
    assert_eq!(s, 200, "{au}");
    let routes: Vec<&str> = au["entries"].as_array().unwrap().iter().filter_map(|e| e["route"].as_str()).collect();
    for r in ["/v1/alerts/rules", "/v1/alerts/routes", "/silence"] {
        assert!(routes.iter().any(|x| x.ends_with(r)), "the audit entry of {r} after a restart: {routes:?}\n{au}");
    }
    assert_eq!(au["shards"]["keep"], 2000, "{au}");
    // The copies' own audit logs do not fill with the keeping.
    let (_, own) = http(&c.copies[0].base(), "GET", "/v1/admin/audit?limit=100", CLUSTER, None).await;
    assert!(!own.to_string().contains("alerts keep"), "{own}");
}

/// OLU-202, OLU-3, OLU-206 and OLU-211, through a coordinator as on-call
/// stands: a copy's `/metrics` totals match its `/v1/health`; the
/// coordinator's `/metrics` has each shard's copies, majority and primary;
/// `POST /v1/admin/verify` compares every shard; `GET /v1/logs` says where
/// the copies' lines are; and the checkpoints are listed, each shard's
/// newest and the names written on every shard.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn on_call_reads_the_cluster_and_its_checkpoints_through_the_coordinator() {
    let dir = tempfile::tempdir().unwrap();
    let c = Cluster::start(dir.path()).await;
    let ops: Vec<Json> = (0..40).map(|i| json!({"op": "upsert", "table": "acks", "doc": {"id": format!("r{i}"), "n": i}})).collect();
    let (s, j) = c.call("POST", "/v1/write", Some(json!({"ops": ops}))).await;
    assert_eq!(s, 200, "{j}");
    let (s, j) = c.call("POST", "/v1/sql", Some(json!({"sql": "CREATE TABLE notes (id KEYWORD PRIMARY KEY, body TEXT)"}))).await;
    assert_eq!(s, 200, "{j}");
    let (s, j) = c.call("POST", "/v1/sql", Some(json!({"sql": "INSERT INTO notes (id, body) VALUES ('a', 'one'), ('b', 'two'), ('c', 'three')"}))).await;
    assert_eq!(s, 200, "{j}");

    // OLU-202: on a copy, the totals are every table's, as /v1/health says.
    let p = &c.copies[0];
    c.until("the copy has every row", || async { http(&p.base(), "GET", "/v1/health", CLUSTER, None).await.1["rows"].as_u64() == Some(43) }).await;
    let (_, h) = http(&p.base(), "GET", "/v1/health", CLUSTER, None).await;
    let m = metrics(&p.base(), CLUSTER).await;
    assert_eq!(value(&m, "chloedb_docs"), h["rows"].as_f64(), "chloedb_docs against /v1/health's rows {h}\n{m}");
    for (t, n) in h["tables"].as_object().unwrap() {
        assert_eq!(value(&m, &format!("chloedb_table_docs{{table=\"{t}\"}}")), n.as_f64(), "{t} against /v1/health {h}\n{m}");
    }
    assert_eq!(value(&m, "chloedb_table_docs{table=\"acks\"}"), Some(40.0), "{m}");
    assert_eq!(value(&m, "chloedb_table_docs{table=\"notes\"}"), Some(3.0), "{m}");
    let segments: f64 = m.lines().filter(|l| l.starts_with("chloedb_table_segments{")).filter_map(|l| l.rsplit(' ').next()?.parse::<f64>().ok()).sum();
    assert_eq!(value(&m, "chloedb_segments"), Some(segments), "{m}");
    assert_eq!(value(&m, "chloedb_copy_info{shard=\"0\",node=\"s0a\",role=\"primary\"}"), Some(1.0), "{m}");

    // OLU-3: the coordinator's /metrics has the cluster's state.
    c.until("the coordinator's /metrics has the cluster", || async { value(&metrics(&c.base(), ADMIN).await, "chloedb_cluster_copies_up{shard=\"0\"}") == Some(2.0) }).await;
    let m = metrics(&c.base(), ADMIN).await;
    for (series, want) in [("chloedb_cluster_copies{shard=\"0\"}", 2.0), ("chloedb_cluster_copies_down{shard=\"0\"}", 0.0), ("chloedb_cluster_shard_majority{shard=\"0\"}", 1.0), ("chloedb_cluster_shard_primary{shard=\"0\"}", 1.0)] {
        assert_eq!(value(&m, series), Some(want), "{series}\n{m}");
    }
    assert!(m.contains("chloedb_cluster_copy_up{shard=\"0\",copy=\"s0a\""), "{m}");

    // OLU-206: the runbook's calls, through the coordinator.
    let (s, v) = c.call("POST", "/v1/admin/verify", Some(json!({"timeout_ms": 30000}))).await;
    assert_eq!(s, 200, "verify through the coordinator: {v}");
    assert_eq!((v["shards"][0]["shard"].as_u64(), v["differing"].as_u64()), (Some(0), Some(0)), "{v}");
    assert_eq!(v["replicas"][0]["url"], c.copies[1].url(), "{v}");
    assert_eq!(v["replicas"][0]["in_sync"], true, "{v}");
    let (s, l) = c.call("GET", "/v1/logs?q=s0a", None).await;
    assert_eq!(s, 200, "{l}");
    assert!(l["note"].as_str().is_some_and(|n| n.contains("POST /v1/logs/query")), "{l}");

    // OLU-211: the checkpoints, listed.
    let (s, j) = c.call("POST", "/v1/admin/checkpoint", Some(json!({"name": "nightly-1"}))).await;
    assert_eq!(s, 200, "{j}");
    let (s, j) = c.call("POST", "/v1/admin/copies/checkpoint", Some(json!({"url": c.copies[1].url(), "name": "replica-only"}))).await;
    assert_eq!(s, 200, "{j}");
    let (s, b) = c.call("GET", "/v1/admin/checkpoints", None).await;
    assert_eq!(s, 200, "{b}");
    assert_eq!(b["copies"].as_array().map(Vec::len), Some(2), "{b}");
    let names: Vec<&str> = b["complete"].as_array().unwrap().iter().filter_map(|c| c["name"].as_str()).collect();
    assert_eq!(names, vec!["replica-only", "nightly-1"], "every name on every shard, the newest first: {b}");
    assert_eq!((b["newest"][0]["shard"].as_u64(), b["newest"][0]["name"].as_str()), (Some(0), Some("replica-only")), "{b}");
    let on_primary = b["copies"].as_array().unwrap().iter().find(|k| k["url"] == c.copies[0].url()).unwrap();
    let cp = &on_primary["checkpoints"][0];
    assert_eq!((cp["name"].as_str(), cp["shard"].as_u64(), cp["slot_epoch"].as_u64().is_some()), (Some("nightly-1"), Some(0), true), "{on_primary}");
    assert!(cp["bytes"].as_u64().is_some_and(|n| n > 0), "{cp}");
    // On a copy itself, its own.
    let (s, own) = http(&c.copies[1].base(), "GET", "/v1/admin/checkpoints", CLUSTER, None).await;
    assert_eq!(s, 200, "{own}");
    let names: Vec<&str> = own["checkpoints"].as_array().unwrap().iter().filter_map(|c| c["name"].as_str()).collect();
    assert_eq!(names, vec!["replica-only"], "{own}");
}

/// OLU-202 on a server that is not in a cluster, made with no tables as
/// the guide's first page makes one: `chloedb_docs` counted its empty
/// placeholder table (0) while `/v1/health` counted the table made since.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_servers_metrics_count_every_table_as_its_health_does() {
    let dir = tempfile::tempdir().unwrap();
    let http_port = port();
    let base = format!("http://127.0.0.1:{http_port}");
    let log = std::fs::File::create(dir.path().join("server.log")).unwrap();
    let mut child = Command::new(bin())
        .arg("--db")
        .arg(dir.path().join("one.chloedb"))
        .args(["--listen", &format!("127.0.0.1:{http_port}"), "--grpc", "off", "--start-apps", "off", "--log-store", "off"])
        .stdout(Stdio::null())
        .stderr(log)
        .spawn()
        .unwrap();
    let t0 = Instant::now();
    while http(&base, "GET", "/v1/health", "", None).await.0 != 200 {
        assert!(t0.elapsed() < WAIT, "the server did not start: {}", std::fs::read_to_string(dir.path().join("server.log")).unwrap_or_default());
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    for sql in ["CREATE TABLE t (id KEYWORD PRIMARY KEY, body TEXT)", "INSERT INTO t (id, body) VALUES ('a', 'one'), ('b', 'two')"] {
        let (s, j) = http(&base, "POST", "/v1/sql", "", Some(json!({"sql": sql}))).await;
        assert_eq!(s, 200, "{sql}: {j}");
    }
    let (_, h) = http(&base, "GET", "/v1/health", "", None).await;
    let m = metrics(&base, "").await;
    let _ = child.kill();
    let _ = child.wait();
    assert_eq!(h["rows"], 2, "{h}");
    assert_eq!(value(&m, "chloedb_docs"), h["rows"].as_f64(), "chloedb_docs against /v1/health's rows {h}\n{m}");
    assert_eq!(value(&m, "chloedb_table_docs{table=\"t\"}"), Some(2.0), "{m}");
    assert_eq!(value(&m, "chloedb_segments"), value(&m, "chloedb_table_segments{table=\"t\"}"), "{m}");
    assert!(!m.contains("chloedb_copy_info"), "a server that is not a copy has no copy_info: {m}");
}

/// The events of `GET /v1/events` (the last 24 hours), and the answer.
async fn events(c: &Cluster) -> (Vec<Json>, Json) {
    let (s, j) = c.call("GET", "/v1/events?limit=5000", None).await;
    assert_eq!(s, 200, "{j}");
    (j["events"].as_array().cloned().unwrap_or_default(), j)
}

/// OLU-301: a coordinator without a log store (the guide's) lost its
/// events at each restart, so after a rolling upgrade, which restarts the
/// coordinator last, the Timeline said nothing happened. Its shards keep
/// them: a copy that went down and came back, and a note, are on the
/// Timeline of the coordinator started again, once each, and the answer
/// says from when its record runs.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_coordinator_without_a_log_store_keeps_its_events_on_its_shards() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = Cluster::start(dir.path()).await;
    let started = chloedb_server::telemetry::now_ms();
    let (s, j) = c.call("POST", "/v1/events/notes", Some(json!({"text": "drill: the replica goes down"}))).await;
    assert_eq!(s, 200, "{j}");
    // The replica goes down and comes back: two events of the cluster.
    let replica = c.copies[1].url();
    {
        let k = &mut c.copies[1];
        let mut child = k.child.take().unwrap();
        let _ = child.kill();
        let _ = child.wait();
    }
    c.until("the coordinator records the replica down", || async { events(&c).await.0.iter().any(|e| e["kind"] == "copy_down") }).await;
    c.copies[1].start().await;
    c.until("the coordinator records the replica back", || async { events(&c).await.0.iter().any(|e| e["kind"] == "copy_up") }).await;
    let (before, _) = events(&c).await;
    let kinds = |evs: &[Json]| evs.iter().filter_map(|e| e["kind"].as_str().map(String::from)).collect::<Vec<_>>();
    for k in ["note", "copy_down", "copy_up"] {
        assert!(kinds(&before).iter().any(|x| x == k), "{k} before the restart: {before:?}");
    }
    // Every copy holds them, the replica too once it is back.
    c.until("every copy holds the coordinator's events", || async {
        c.copies.iter().all(|k| {
            let held = k.kept();
            let evs = held["events"].as_array().cloned().unwrap_or_default();
            ["note", "copy_down", "copy_up"].iter().all(|kind| evs.iter().any(|e| e["kind"] == *kind && e["coordinator"] == "coord")) && held["events_from"].as_u64().is_some_and(|f| f > 0 && f <= started)
        })
    })
    .await;

    c.kill_coordinator();
    c.start_coordinator().await;
    let (after, j) = events(&c).await;
    assert!(j["source"].as_array().unwrap().contains(&json!("shards")), "{j}");
    for k in ["note", "copy_down", "copy_up"] {
        assert_eq!(after.iter().filter(|e| e["kind"] == k).count(), 1, "{k} once after the restart: {after:?}\n{}", c.logs());
    }
    let down = after.iter().find(|e| e["kind"] == "copy_down").unwrap();
    assert_eq!((down["where"]["shard"].as_u64(), down["detail"]["url"].as_str()), (Some(0), Some(replica.as_str())), "{down}");
    assert!(down.get("who").is_none() || down["who"].is_null(), "{down}");
    assert_eq!(after.iter().find(|e| e["kind"] == "note").unwrap()["title"], "drill: the replica goes down");
    // The record runs from before the first coordinator's note, not from
    // this coordinator's start.
    let record = j["record_from_ms"].as_u64().unwrap();
    assert!(record <= started && record < j["memory_from_ms"].as_u64().unwrap(), "{j}");
    // What it read at its start is said with the rest of what it kept.
    let (_, a) = c.call("GET", "/v1/alerts", None).await;
    assert!(a["kept"]["shards"]["started_with"]["events"].as_u64().is_some_and(|n| n >= 3), "{}", a["kept"]);
}

/// OLU-302: a copy killed for good (s0c) and replaced by a new one (s0d)
/// through a plan; the coordinator started again on the `--shards` it was
/// first given, which names s0c and not s0d. It served the list: s0c down,
/// s0d nowhere, "A copy is down" firing without end. It serves the cluster
/// map, as the guide says: s0d in, s0c out, every copy up.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_coordinator_started_on_its_first_list_after_a_copy_was_replaced_serves_the_cluster_as_it_is() {
    let dir = tempfile::tempdir().unwrap();
    let mut c = Cluster::start_with(dir.path(), 3).await;
    let first = c.copies.iter().map(Copy::url).collect::<Vec<_>>().join("|");
    let ops: Vec<Json> = (0..30).map(|i| json!({"op": "upsert", "table": "acks", "doc": {"id": format!("r{i}"), "n": i}})).collect();
    let (s, j) = c.call("POST", "/v1/write", Some(json!({"ops": ops}))).await;
    assert_eq!(s, 200, "{j}");

    // s0c is gone for good; s0d is a new, empty server.
    let gone = c.copies[2].url();
    {
        let mut child = c.copies[2].child.take().unwrap();
        let _ = child.kill();
        let _ = child.wait();
    }
    let mut d = Copy::new(dir.path(), "s0d");
    d.start().await;
    let (a, b) = (c.copies[0].url(), c.copies[1].url());
    let new = d.url();
    let (s, p) = http(&c.base(), "POST", "/v1/admin/plan", CLUSTER, Some(json!({"shards": [{"shard": 0, "copies": [a, b, new]}]}))).await;
    assert_eq!(s, 200, "{p}");
    let (s, j) = http(&c.base(), "POST", "/v1/admin/plan/apply", CLUSTER, Some(json!({"steps": p["steps"]}))).await;
    assert_eq!(s, 200, "{j}");
    c.until("the plan replaces s0c with s0d", || async {
        let (_, r) = c.call("GET", "/v1/admin/plan/run", None).await;
        let (_, cl) = c.call("GET", "/v1/cluster", None).await;
        let urls: Vec<&str> = cl["shards"][0]["replicas"].as_array().into_iter().flatten().filter_map(|x| x["url"].as_str()).collect();
        r["run"]["state"] == "done" && urls.contains(&new.as_str()) && !urls.contains(&gone.as_str())
    })
    .await;
    c.copies.push(d);

    // Started again on its first list, which names s0c and not s0d.
    c.kill_coordinator();
    c.start_coordinator_on(&first).await;
    let (_, cl) = c.call("GET", "/v1/cluster", None).await;
    let copies = cl["shards"][0]["replicas"].as_array().cloned().unwrap_or_default();
    let urls: Vec<&str> = copies.iter().filter_map(|x| x["url"].as_str()).collect();
    assert!(urls.contains(&new.as_str()) && !urls.contains(&gone.as_str()), "the cluster map's copies, not the list's: {urls:?}\n{cl}\n{}", c.logs());
    c.until("every copy answers", || async {
        let (_, cl) = c.call("GET", "/v1/cluster", None).await;
        cl["shards"][0]["replicas"].as_array().is_some_and(|v| v.len() == 3 && v.iter().all(|x| x["status"] != "down"))
    })
    .await;
    // No alert for the copy that left, and the coordinator says why it
    // left out a server it was given.
    tokio::time::sleep(Duration::from_secs(6)).await;
    let (_, al) = c.call("GET", "/v1/alerts", None).await;
    let firing: Vec<&Json> = al["states"].as_array().into_iter().flatten().filter(|x| x["state"] == "firing").collect();
    assert!(firing.iter().all(|x| !x.to_string().contains(gone.trim_start_matches("http://"))), "nothing fires for the copy that left: {firing:?}");
    let down = al["states"].as_array().into_iter().flatten().find(|x| x["rule"] == "copy-down").cloned().unwrap_or_default();
    assert!(down["state"] != "firing" && down["state"] != "pending", "A copy is down, for no copy: {down}");
    let log = std::fs::read_to_string(dir.path().join("coordinator.log")).unwrap_or_default();
    assert!(log.contains("do not answer and that the cluster map leaves out"), "{log}");
    // The copies keep the coordinator's state again: none fails.
    c.until("every copy keeps the coordinator's state", || async {
        let (_, al) = c.call("GET", "/v1/alerts", None).await;
        al["kept"]["shards"]["holding"] == 3 && al["kept"]["shards"]["failed"].as_array().is_some_and(|f| f.is_empty())
    })
    .await;
}
