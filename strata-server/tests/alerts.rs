//! Alerts: rules over a window of points, no data, acknowledgements,
//! silences and notes, routes and their messages, backtests; and on a
//! coordinator, the copies' states of each_copy rules and the built-in
//! rules over the cluster's state.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value as Json};
use strata::db::Db;
use strata::storage::DbOptions;
use strata_server::coord::Coordinator;
use strata_server::service::{AgentConfig, Backend, Service};
use strata_server::{alerts, demo, server};
use tokio::net::TcpListener;

const N: usize = 300;

fn service(dir: &Path) -> Service {
    let db = Db::create(&dir.join("a.strata"), demo::schema(N), DbOptions { publish_snapshots: true, ..DbOptions::default() }).unwrap();
    db.set_embedder("emb", Arc::new(strata::embed::HashEmbedder { dim: demo::DIM })).unwrap();
    db.load((0..N).map(|i| demo::product(i, 7))).unwrap();
    let schema = (*db.schema()).clone();
    let agent = AgentConfig { api_key: None, model: String::new(), base_url: String::new() };
    Service::new(Backend::Local(Arc::new(db)), schema, agent, 16, Duration::from_secs(10))
}

async fn http(svc: Arc<Service>) -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
    tokio::spawn(server::serve_http(svc, l, Default::default()));
    url
}

async fn call(base: &str, method: &str, path: &str, body: Option<Json>) -> (u16, Json) {
    let c = strata_server::http_client().build().unwrap();
    let mut r = match method {
        "GET" => c.get(format!("{base}{path}")),
        "DELETE" => c.delete(format!("{base}{path}")),
        "PUT" => c.put(format!("{base}{path}")),
        _ => c.post(format!("{base}{path}")),
    };
    if let Some(b) = body {
        r = r.json(&b);
    }
    let r = r.send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or(Json::Null))
}

/// Points every 2 s ending at `t`, oldest first, each `f(i)`.
fn series(t: u64, n: usize, f: impl Fn(usize) -> Json) -> Vec<Arc<Json>> {
    (0..n)
        .map(|i| {
            let mut p = f(i);
            p["t"] = json!(t - (n - 1 - i) as u64 * 2000);
            Arc::new(p)
        })
        .collect()
}

fn state<'a>(a: &'a Json, rule: &str) -> &'a Json {
    a["states"].as_array().unwrap().iter().find(|s| s["rule"] == rule).unwrap_or_else(|| panic!("no state of {rule}: {a}"))
}

/// A noisy p99, every other point over the threshold: the rule that asks
/// every point to pass (the old rules) never fires; the mean over the
/// window does. Its acknowledgement, silence and notes are kept with it.
#[tokio::test(flavor = "multi_thread")]
async fn a_mean_over_its_window_fires_where_every_point_did_not() {
    let dir = tempfile::tempdir().unwrap();
    let svc = Arc::new(service(dir.path()));
    let base = http(svc.clone()).await;
    let rules = json!({"rules": [
        {"id": "slow-all", "name": "Slow, every point", "metric": "p99_ms", "op": ">", "threshold": 100.0, "eval": "all", "window_s": 60},
        {"id": "slow", "name": "Slow on average", "metric": "p99_ms", "op": ">", "threshold": 100.0, "eval": "mean", "window_s": 60, "severity": "critical"},
    ]});
    let (status, j) = call(&base, "PUT", "/v1/alerts/rules", Some(rules)).await;
    assert_eq!(status, 200, "{j}");
    let t = strata_server::telemetry::now_ms();
    let noisy = series(t, 31, |i| json!({"p99_ms": if i % 2 == 0 { 300.0 } else { 20.0 }}));
    let changes = svc.alerts.check(&noisy, t);
    assert_eq!(changes.iter().map(|e| (e.rule.as_str(), e.state)).collect::<Vec<_>>(), [("slow", "firing")]);
    alerts::deliver(&svc, changes);

    let (_, a) = call(&base, "GET", "/v1/alerts", None).await;
    assert_eq!(a["firing"], 1, "{a}");
    assert_eq!(a["states"][0]["rule"], "slow", "firing first: {a}");
    assert!(state(&a, "slow")["value"].as_f64().unwrap() > 150.0);
    assert_eq!(state(&a, "slow-all")["state"], "ok");

    let (status, s) = call(&base, "POST", "/v1/alerts/slow/ack", Some(json!({"note": "looking"}))).await;
    assert_eq!(status, 200, "{s}");
    assert_eq!((s["ack"]["by"].as_str(), s["ack"]["note"].as_str()), (Some("open"), Some("looking")), "{s}");
    let (status, s) = call(&base, "POST", "/v1/alerts/slow/silence", Some(json!({"minutes": 30, "reason": "deploying"}))).await;
    assert_eq!(status, 200, "{s}");
    assert_eq!(s["silenced"]["reason"], "deploying");
    assert_eq!(call(&base, "POST", "/v1/alerts/slow/silence", Some(json!({"minutes": 30}))).await.0, 400);
    let (status, s) = call(&base, "POST", "/v1/alerts/slow/notes", Some(json!({"text": "the merge of segment 12 is slow"}))).await;
    assert_eq!(status, 200, "{s}");
    assert_eq!(s["notes"].as_array().unwrap().len(), 2, "the ack's note and this one: {s}");
    let (status, s) = call(&base, "DELETE", "/v1/alerts/slow/silence", None).await;
    assert_eq!(status, 200, "{s}");
    assert!(s.get("silenced").is_none(), "{s}");
    assert_eq!(call(&base, "POST", "/v1/alerts/slow-all/ack", Some(json!({}))).await.0, 409, "nothing to acknowledge");
    assert_eq!(call(&base, "GET", "/v1/alerts/nothing", None).await.0, 404);

    let (_, one) = call(&base, "GET", "/v1/alerts/slow", None).await;
    let states: Vec<&str> = one["events"].as_array().unwrap().iter().map(|e| e["state"].as_str().unwrap()).collect();
    assert_eq!(states, ["unsilenced", "noted", "silenced", "acked", "firing"], "{one}");
    // Each is an event of the server's, and audited.
    let (_, ev) = call(&base, "GET", "/v1/events", None).await;
    let kinds: Vec<&str> = ev["events"].as_array().unwrap().iter().map(|e| e["kind"].as_str().unwrap()).collect();
    assert!(kinds.contains(&"alert_fired") && kinds.contains(&"alert_acked") && kinds.contains(&"alert_silenced"), "{ev}");
    let (_, audit) = call(&base, "GET", "/v1/admin/audit", None).await;
    let ack = audit["entries"].as_array().unwrap().iter().find(|e| e["route"] == "/v1/alerts/:rule/ack").expect("audited");
    assert_eq!((ack["object"].as_str(), ack["detail"].as_str()), (Some("alert slow"), Some("acknowledged alert slow")), "{ack}");
    assert!(ack["before"]["ack"].is_null() && ack["after"]["ack"]["by"] == "open", "{ack}");
}

/// A window without the metric is no data: a firing alert stays firing,
/// says since when it has no data, and resolves only on data that says
/// so. A window in which no query ended has no error ratio.
#[tokio::test(flavor = "multi_thread")]
async fn no_data_does_not_resolve_an_alert() {
    let dir = tempfile::tempdir().unwrap();
    let svc = Arc::new(service(dir.path()));
    let base = http(svc.clone()).await;
    let t = strata_server::telemetry::now_ms() - 600_000;
    let failing = series(t, 150, |_| json!({"queries": 100, "query_errors": 20, "error_ratio": 0.2}));
    let changes = svc.alerts.check(&failing, t);
    assert!(changes.iter().any(|e| e.rule == "errors" && e.state == "firing"), "{changes:?}");
    // Five minutes in which no query ended.
    let t2 = t + 302_000;
    let mut quiet = failing.clone();
    quiet.extend(series(t2, 150, |_| json!({"queries": 0, "query_errors": 0})));
    let changes = svc.alerts.check(&quiet, t2);
    assert!(changes.iter().all(|e| e.rule != "errors"), "no data resolves nothing: {changes:?}");
    let (_, a) = call(&base, "GET", "/v1/alerts", None).await;
    let s = state(&a, "errors");
    assert_eq!(s["state"], "firing", "{s}");
    assert!(s["no_data_since_ms"].as_u64().is_some(), "{s}");
    // Queries that succeed resolve it.
    let t3 = t2 + 300_000;
    quiet.extend(series(t3, 150, |_| json!({"queries": 100, "query_errors": 0, "error_ratio": 0.0})));
    let changes = svc.alerts.check(&quiet, t3);
    assert!(changes.iter().any(|e| e.rule == "errors" && e.state == "resolved"), "{changes:?}");
}

/// A route takes the alerts of its severities: the message names the
/// cluster, what fired, its value against its rule, the description, the
/// runbook and a link; resolving (an edit of the rule too) says so; each
/// delivery is counted, and a route can be tried.
#[tokio::test(flavor = "multi_thread")]
async fn a_route_is_told_what_fired_and_resolved() {
    let got: Arc<Mutex<Vec<Json>>> = Arc::default();
    let keep = got.clone();
    let hook = axum::Router::new().route(
        "/hook/secret",
        axum::routing::post(move |axum::Json(j): axum::Json<Json>| {
            let keep = keep.clone();
            async move {
                keep.lock().unwrap().push(j);
                "ok"
            }
        }),
    );
    let hl = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let hurl = format!("http://127.0.0.1:{}", hl.local_addr().unwrap().port());
    tokio::spawn(async move { axum::serve(hl, hook).await });

    let dir = tempfile::tempdir().unwrap();
    let mut svc = service(dir.path());
    svc.identity.name = Some("prod/search".into());
    svc.identity.public_url = Some("https://cc.example.com/".into());
    let svc = Arc::new(svc);
    let base = http(svc.clone()).await;
    let routes = json!({"routes": [{"name": "pager", "url": format!("{hurl}/hook/secret"), "severities": ["critical"]}, {"name": "chat", "url": format!("{hurl}/nowhere"), "severities": ["info"]}]});
    let (status, j) = call(&base, "PUT", "/v1/alerts/routes", Some(routes)).await;
    assert_eq!(status, 200, "{j}");
    assert_eq!(j["routes"][0]["url"], format!("{hurl}/..."), "a webhook's path is its secret: {j}");
    let rule = json!({"id": "slow", "name": "Slow search", "metric": "p99_ms", "op": ">", "threshold": 100.0, "eval": "mean", "window_s": 60,
        "severity": "critical", "description": "Searches are slow.", "runbook": "https://runbook.example.com/slow"});
    assert_eq!(call(&base, "PUT", "/v1/alerts/rules", Some(json!({"rules": [rule]}))).await.0, 200);

    let t = strata_server::telemetry::now_ms();
    let changes = svc.alerts.check(&series(t, 31, |_| json!({"p99_ms": 300.0})), t);
    alerts::deliver(&svc, changes);
    let wait = |n: usize| {
        let got = got.clone();
        async move {
            let t0 = Instant::now();
            while got.lock().unwrap().len() < n {
                assert!(t0.elapsed() < Duration::from_secs(10), "{n} messages");
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    };
    wait(1).await;
    let text = got.lock().unwrap()[0]["text"].as_str().unwrap().to_string();
    assert_eq!(text, "[strata FIRING critical] Slow search on prod/search: p99_ms = 300 (> 100, mean over 60 s). Searches are slow. Runbook: https://runbook.example.com/slow https://cc.example.com/#/alert/slow");
    assert_eq!(got.lock().unwrap()[0]["alert"]["rule"], "slow");

    // An edit of the rule resolves what it fired, and says who edited it.
    let mut r2 = json!({"id": "slow", "name": "Slow search", "metric": "p99_ms", "op": ">", "threshold": 200.0, "eval": "mean", "window_s": 60, "severity": "critical"});
    r2["runbook"] = json!("https://runbook.example.com/slow");
    let (status, j) = call(&base, "PUT", "/v1/alerts/rules", Some(json!({"rules": [r2]}))).await;
    assert_eq!(status, 200, "{j}");
    wait(2).await;
    let text = got.lock().unwrap()[1]["text"].as_str().unwrap().to_string();
    assert!(text.starts_with("[strata RESOLVED critical] Slow search on prod/search: p99_ms = 300 after "), "{text}");
    assert!(text.contains("(the rule was edited by open)"), "{text}");
    // The audit entry holds the rule before and after.
    let (_, audit) = call(&base, "GET", "/v1/admin/audit", None).await;
    let e = audit["entries"].as_array().unwrap().iter().find(|e| e["route"] == "/v1/alerts/rules").expect("audited");
    assert_eq!(e["before"]["slow"]["threshold"], 100.0, "{e}");
    assert_eq!(e["after"]["slow"]["threshold"], 200.0, "{e}");

    let (_, a) = call(&base, "GET", "/v1/alerts", None).await;
    let pager = a["routes"].as_array().unwrap().iter().find(|r| r["name"] == "pager").unwrap();
    assert_eq!(pager["sent"], 2, "{pager}");
    let (status, j) = call(&base, "POST", "/v1/alerts/routes/pager/test", Some(json!({}))).await;
    assert_eq!((status, j["ok"].as_bool()), (200, Some(true)), "{j}");
    let (status, j) = call(&base, "POST", "/v1/alerts/routes/chat/test", Some(json!({}))).await;
    assert_eq!((status, j["ok"].as_bool()), (200, Some(false)), "a 404 is a failed delivery: {j}");
    assert!(got.lock().unwrap()[2]["text"].as_str().unwrap().starts_with("[strata TEST]"));
}

/// A backtest replays a rule over the log store's minutes of load (7
/// days at most), and says when it would have fired.
#[tokio::test(flavor = "multi_thread")]
async fn a_backtest_replays_a_rule_over_the_load_kept() {
    let dir = tempfile::tempdir().unwrap();
    let mut svc = service(dir.path());
    let store = strata_server::logstore::LogStore::open(strata_server::logstore::LogOptions { capture: false, ..strata_server::logstore::LogOptions::new(dir.path().join("a.logs"), "n1".into()) }).unwrap();
    svc.log_store = Some(store.clone());
    let svc = Arc::new(svc);
    let base = http(svc.clone()).await;
    let now = strata_server::telemetry::now_ms() / 60_000 * 60_000;
    // Two days of minutes; an hour of slow ones a day ago.
    let loads: Vec<strata_server::logstore::Load> = (0..2 * 1440u64)
        .map(|m| {
            let ts_ms = now - (2 * 1440 - m) * 60_000;
            let slow = (1440..1500).contains(&m);
            strata_server::logstore::Load { ts_ms, role: "standalone".into(), qps: 10.0, cores: 4, p99_ms: Some(if slow { 900.0 } else { 20.0 }), ..Default::default() }
        })
        .collect();
    store.write_loads(&loads).unwrap();
    let rule = json!({"id": "p", "name": "p", "metric": "p99_ms", "op": ">", "threshold": 500.0, "eval": "mean", "window_s": 600});
    let (status, j) = call(&base, "POST", "/v1/alerts/backtest", Some(json!({"rule": rule, "from": now - 3 * 86_400_000, "to": now}))).await;
    assert_eq!(status, 200, "{j}");
    assert_eq!(j["source"], "load", "{j}");
    let fired = j["fired"].as_array().unwrap();
    assert_eq!(fired.len(), 1, "{j}");
    let (from, to) = (fired[0]["from_ms"].as_u64().unwrap(), fired[0]["to_ms"].as_u64().unwrap());
    let slow_from = now - (2 * 1440 - 1440) * 60_000;
    assert!(from >= slow_from && from < slow_from + 600_000, "{from} {slow_from}");
    assert!(to > slow_from + 3_600_000 - 60_000, "{to}");
    // A metric the load rows do not hold says so.
    let rule = json!({"id": "w", "name": "w", "metric": "write_queue", "op": ">", "threshold": 1.0});
    let (status, j) = call(&base, "POST", "/v1/alerts/backtest", Some(json!({"rule": rule, "from": now - 86_400_000, "to": now}))).await;
    assert_eq!(status, 400, "{j}");
}

// ---------------------------------------------------- a cluster's copies

/// A port free now, of a block of 20 this test process takes alone.
fn port() -> u16 {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let base = 21_000 + (std::process::id() % 400) * 20;
    loop {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        assert!(n < 20, "this test process took its 20 ports");
        let p = (base + n) as u16;
        if std::net::TcpListener::bind(("127.0.0.1", p)).is_ok() {
            return p;
        }
    }
}

/// A `strata-server` process of one database.
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
        let opts = DbOptions { publish_snapshots: true, ship_log_bytes: 16 << 20, ..DbOptions::default() };
        let db = Db::create(&path, demo::schema(N), opts).unwrap();
        if rows > 0 {
            db.set_embedder("emb", Arc::new(strata::embed::HashEmbedder { dim: demo::DIM })).unwrap();
            db.load((0..rows).map(|i| demo::product(i, 7))).unwrap();
        }
        db.storage().start_seq_no().unwrap();
        drop(db);
        Node { path, log: dir.join(format!("{name}.log")), http: port(), grpc: port(), child: None }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.grpc)
    }

    fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.http)
    }

    async fn start(&mut self) {
        let log = std::fs::OpenOptions::new().create(true).append(true).open(&self.log).unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_strata-server"))
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
        while strata_server::http_client().build().unwrap().get(format!("{}/v1/health", self.base())).send().await.is_err() {
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

    fn logs(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Checks the coordinator's cluster until `f` holds of its alerts.
async fn until(svc: &Arc<Service>, what: &str, nodes: &[&Node], f: impl Fn(&Json) -> bool) -> Json {
    let t0 = Instant::now();
    loop {
        alerts::check_cluster(svc).await;
        let a = svc.alerts.json();
        if f(&a) {
            return a;
        }
        if t0.elapsed() > Duration::from_secs(40) {
            let logs: Vec<String> = nodes.iter().map(|n| format!("--- {}\n{}", n.url(), n.logs())).collect();
            panic!("waited 40 s for: {what}\n{a}\n{}", logs.join("\n"));
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// Each copy evaluates an each_copy rule on its own points and says its
/// state in its detail; the coordinator's `/v1/alerts` gathers them, with
/// where it fires. A shard of two copies with failover that loses one has
/// no majority: the built-in rule fires at once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_coordinator_gathers_its_copies_and_sees_a_shard_lose_its_majority() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let mut a = Node::new(d, "a", N);
    let mut b = Node::new(d, "b", 0);
    a.start().await;
    b.start().await;
    let c = Arc::new(Coordinator::connect(vec![a.url()]).await.unwrap());
    let agent = AgentConfig { api_key: None, model: String::new(), base_url: String::new() };
    let schema = c.schema.clone();
    let svc = Arc::new(Service::new(Backend::Cluster(c.clone()), schema, agent, 16, Duration::from_secs(10)));
    svc.alerts.set_coordinator(true);
    c.add_replica(0, &b.url(), false).await.unwrap();

    // The same rule on the copies and the coordinator: many documents.
    let rule = json!({"id": "many-docs", "name": "Many documents", "metric": "docs", "op": ">", "threshold": 10.0, "eval": "max", "window_s": 10, "scope": "each_copy"});
    for n in [&a, &b] {
        let (status, j) = call(&n.base(), "PUT", "/v1/alerts/rules", Some(json!({"rules": [rule.clone()]}))).await;
        assert_eq!(status, 200, "{j}");
    }
    let rules: Vec<alerts::Rule> = vec![serde_json::from_value(rule).unwrap()];
    svc.alerts.set_rules(rules, "test").unwrap();

    let all = until(&svc, "both copies fire many-docs", &[&a, &b], |a| {
        a["states"].as_array().unwrap().iter().any(|s| s["rule"] == "many-docs" && s["state"] == "firing" && s["where"].as_array().is_some_and(|w| w.len() == 2))
    })
    .await;
    let s = state(&all, "many-docs");
    let shorts: Vec<&str> = s["where"].as_array().unwrap().iter().map(|w| w["short"].as_str().unwrap()).collect();
    assert_eq!(shorts, ["s0-0", "s0-1"], "{s}");
    assert_eq!(s["where"][0]["copy"], a.url().as_str());
    assert!(s["where"][0]["value"].as_f64().unwrap() >= N as f64, "{s}");
    assert_eq!(s["copies"]["firing"], 2, "{s}");
    assert_eq!(state(&all, "shard-no-majority")["state"], "ok", "{all}");
    assert!(all["rules"].as_array().unwrap().iter().any(|r| r["id"] == "shard-no-majority" && r["builtin"] == true), "{all}");

    // A copy lost: one of two is no majority.
    b.kill();
    let lost = until(&svc, "shard-no-majority fires", &[&a], |a| state(a, "shard-no-majority")["state"] == "firing").await;
    let s = state(&lost, "shard-no-majority");
    assert_eq!(s["severity"], "critical");
    assert_eq!(s["where"][0]["shard"], 0, "{s}");
    assert!(lost["events"].as_array().unwrap().iter().any(|e| e["rule"] == "shard-no-majority" && e["state"] == "firing"), "{lost}");
    // The copy down is pending (it fires after 30 s), and its rule's state
    // is the last it told.
    assert_eq!(state(&lost, "copy-down")["state"], "pending", "{lost}");
    let w = state(&lost, "many-docs")["where"].as_array().unwrap().clone();
    assert!(w.iter().any(|w| w["short"] == "s0-1" && w["unheard_since_ms"].as_u64().is_some()), "{w:?}");
}
