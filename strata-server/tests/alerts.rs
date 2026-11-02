//! Alerts: rules over a window of points, no data, acknowledgements,
//! silences and notes, routes and their messages, backtests; and on a
//! coordinator, the copies' states of each_copy rules and the built-in
//! rules over the cluster's state.

#![cfg(unix)]

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value as Json};
use strata::db::Db;
use strata::storage::DbOptions;
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

