//! What the command center's workload views read: points that tell the
//! server's failures from the client's, statements over a window, the
//! identity of each query, running queries by shape, and Contain rules.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value as Json};
use strata::db::Db;
use strata::storage::DbOptions;
use strata_server::service::{AgentConfig, Backend, Service};
use strata_server::{demo, server};
use tokio::net::TcpListener;

const N: usize = 4000;

/// Every product meets every product of its category, and no pair passes
/// the rest of the condition: a query that runs a while.
const JOIN: &str = "SELECT x.id, y.id FROM products x JOIN products y ON x.category = y.category AND x.price < y.price - 1000000";

fn open(dir: &std::path::Path, name: &str) -> Arc<Db> {
    let db = Db::create(&dir.join(name), demo::schema(N), DbOptions { publish_snapshots: true, ..DbOptions::default() }).unwrap();
    db.set_embedder("emb", Arc::new(strata::embed::HashEmbedder { dim: demo::DIM })).unwrap();
    db.load((0..N).map(|i| demo::product(i, 7))).unwrap();
    Arc::new(db)
}

fn service(db: Arc<Db>) -> Service {
    let agent = AgentConfig { api_key: None, model: String::new(), base_url: String::new() };
    let schema = (*db.schema()).clone();
    Service::new(Backend::Local(db), schema, agent, 16, Duration::from_secs(60))
}

async fn http(svc: Arc<Service>) -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
    tokio::spawn(server::serve_http(svc, l, Default::default()));
    url
}

async fn get(base: &str, path: &str) -> Json {
    let r = strata_server::http_client().build().unwrap().get(format!("{base}{path}")).send().await.unwrap();
    assert_eq!(r.status(), 200, "{path}");
    r.json().await.unwrap()
}

async fn query(base: &str, body: Json) -> (u16, Json) {
    let r = strata_server::http_client().build().unwrap().post(format!("{base}/v1/query")).json(&body).send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or(Json::Null))
}

/// The latest point, after a new sample.
fn point(svc: &Service) -> Json {
    (*svc.series.add(svc.sample()).expect("a point")).clone()
}

/// A point of an interval in which no query ended has no error ratio (no
/// data, not none failing); a client's bad SQL and its own deadline are
/// counted apart from the server's failures; no quantile exceeds the
/// slowest query.
#[tokio::test(flavor = "multi_thread")]
async fn points_tell_server_failures_from_the_client_s() {
    let dir = tempfile::tempdir().unwrap();
    let svc = Arc::new(service(open(dir.path(), "one.strata")));
    let base = http(svc.clone()).await;
    svc.series.add(svc.sample());

    let quiet = point(&svc);
    assert_eq!(quiet["queries"], 0, "{quiet}");
    assert!(quiet.get("error_ratio").is_none(), "no query ended, so no ratio: {quiet}");
    assert!(quiet.get("availability").is_none(), "{quiet}");
    assert!(quiet["p99_ms"].is_null(), "{quiet}");

    for _ in 0..3 {
        let (status, body) = query(&base, json!({"sql": "SELECT id FROM products WHERE MATCH(body, 'laptop') LIMIT 5"})).await;
        assert_eq!(status, 200, "{body}");
    }
    let ok = point(&svc);
    assert_eq!((ok["queries"].as_u64(), ok["error_ratio"].as_f64(), ok["availability"].as_f64()), (Some(3), Some(0.0), Some(1.0)), "{ok}");
    let (p99, max) = (ok["p99_ms"].as_f64().unwrap(), ok["max_ms"].as_f64().unwrap());
    assert!(p99 <= max, "p99 {p99} above the slowest query {max}");

    // Bad SQL is the client's error; a 1 ms deadline the client asked
    // for is its own deadline. Neither is a failure of the server.
    let (status, _) = query(&base, json!({"sql": "SELECT id FROM products WHERE name = 'unterminated"})).await;
    assert_eq!(status, 400);
    let (status, body) = query(&base, json!({"sql": JOIN, "timeout_ms": 1})).await;
    assert_eq!(status, 504, "{body}");
    let p = point(&svc);
    assert_eq!((p["queries"].as_u64(), p["query_errors"].as_u64(), p["client_errors"].as_u64(), p["client_deadlines"].as_u64()), (Some(2), Some(0), Some(1), Some(1)), "{p}");
    assert_eq!(p["error_ratio"], 0.0, "{p}");
    assert_eq!(p["availability"], 1.0, "{p}");

    let recent = get(&base, "/v1/queries/recent?limit=2").await;
    let outcomes: Vec<&str> = recent["queries"].as_array().unwrap().iter().map(|q| q["outcome"].as_str().unwrap()).collect();
    assert_eq!(outcomes, ["client_deadline", "client_error"], "{recent}");
    assert_eq!(recent["queries"][0]["deadline_ms"], 1, "{recent}");

    // The series answers the same: the quiet point without a ratio.
    let series = get(&base, "/v1/timeseries").await;
    let points = series["points"].as_array().unwrap();
    assert!(points.iter().any(|p| p["queries"] == 0 && p.get("error_ratio").is_none()), "{series}");
    let metrics = strata_server::http_client().build().unwrap().get(format!("{base}/metrics")).send().await.unwrap().text().await.unwrap();
    assert!(metrics.contains("outcome=\"client_deadline\"} 1"), "{metrics}");
    assert!(metrics.contains("outcome=\"client_error\"} 1"), "{metrics}");
}

async fn call(method: reqwest::Method, base: &str, path: &str, key: &str, headers: &[(&str, &str)], body: Option<Json>) -> (u16, Json) {
    let mut r = strata_server::http_client().build().unwrap().request(method, format!("{base}{path}")).bearer_auth(key);
    for (k, v) in headers {
        r = r.header(*k, *v);
    }
    if let Some(b) = body {
        r = r.json(&b);
    }
    let r = r.send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or(Json::Null))
}

/// A server with an admin key ("ops"), and a read key ("reports-batch").
async fn keyed(svc: &Arc<Service>) -> (String, String, String) {
    let base = http(svc.clone()).await;
    let (status, ops) = call(reqwest::Method::POST, &base, "/v1/admin/keys", "", &[], Some(json!({"name": "ops", "scope": "admin"}))).await;
    assert_eq!(status, 200, "{ops}");
    let ops = ops["secret"].as_str().unwrap().to_string();
    let (status, rb) = call(reqwest::Method::POST, &base, "/v1/admin/keys", &ops, &[], Some(json!({"name": "reports-batch", "scope": "read"}))).await;
    assert_eq!(status, 200, "{rb}");
    (base, ops, rb["secret"].as_str().unwrap().to_string())
}

/// Each query is kept with who sent it (the key's name and id, the
/// client's address and app), its parameters and its deadline, running
/// and finished, and so is it in the log store; the command center's own
/// are tagged.
#[tokio::test(flavor = "multi_thread")]
async fn queries_are_kept_with_who_sent_them() {
    let dir = tempfile::tempdir().unwrap();
    let mut svc = service(open(dir.path(), "one.strata"));
    let store = strata_server::logstore::LogStore::open(strata_server::logstore::LogOptions { capture: false, ..strata_server::logstore::LogOptions::new(dir.path().join("one.logs"), "n1".into()) }).unwrap();
    let keep = store.clone();
    let _ = svc.query_log.keep.set(Box::new(move |q, slow| keep.keep_query(q, slow)));
    svc.log_store = Some(store.clone());
    let svc = Arc::new(svc);
    let (base, ops, rb) = keyed(&svc).await;

    let sql = "SELECT id FROM products WHERE MATCH(body, :q) AND price < :p LIMIT 5";
    let agent = [("user-agent", "nightly-reports/2.3 (linux)")];
    let (status, body) = call(reqwest::Method::POST, &base, "/v1/query", &rb, &agent, Some(json!({"sql": sql, "params": {"q": "laptop", "p": 500}, "timeout_ms": 2000}))).await;
    assert_eq!(status, 200, "{body}");
    let ui = [("x-strata-client", "command-center")];
    let (status, _) = call(reqwest::Method::POST, &base, "/v1/query", &ops, &ui, Some(json!({"sql": "SELECT COUNT(*) FROM products"}))).await;
    assert_eq!(status, 200);

    let (_, recent) = call(reqwest::Method::GET, &base, "/v1/queries/recent?limit=2", &ops, &[], None).await;
    let (mine, theirs) = (&recent["queries"][1], &recent["queries"][0]);
    assert_eq!(mine["key"], "reports-batch", "{recent}");
    assert!(mine["key_id"].as_str().is_some_and(|k| !k.is_empty()), "{mine}");
    assert_eq!(mine["app"], "nightly-reports/2.3", "{mine}");
    assert_eq!(mine["addr"], "127.0.0.1", "{mine}");
    assert_eq!(mine["params"], json!({"p": 500, "q": "laptop"}), "{mine}");
    assert_eq!(mine["deadline_ms"], 2000);
    assert!(mine.get("client").is_none(), "{mine}");
    assert_eq!((theirs["key"].as_str(), theirs["client"].as_str()), (Some("ops"), Some("command-center")), "{theirs}");

    // A running query says who sent it too.
    let b = base.clone();
    let rb2 = rb.clone();
    let run = tokio::spawn(async move { call(reqwest::Method::POST, &b, "/v1/query", &rb2, &[("x-strata-app", "billing")], Some(json!({"sql": JOIN, "timeout_ms": 60_000}))).await });
    let id = loop {
        let (_, live) = call(reqwest::Method::GET, &base, "/v1/queries", &ops, &[], None).await;
        if let Some(q) = live["queries"].as_array().unwrap().iter().find(|q| q["sql"] == JOIN) {
            assert_eq!((q["key"].as_str(), q["app"].as_str()), (Some("reports-batch"), Some("billing")), "{q}");
            assert!(q["fingerprint"].as_str().is_some(), "{q}");
            break q["id"].as_u64().unwrap();
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    };
    let (status, _) = call(reqwest::Method::DELETE, &base, &format!("/v1/queries/{id}"), &ops, &[], None).await;
    assert_eq!(status, 200);
    assert_eq!(run.await.unwrap().0, 499);

    // The log store keeps them with their columns.
    store.flush();
    let (rs, _) = store.query("SELECT key_name, app, client, params, deadline_ms FROM queries WHERE key_name = 'reports-batch' ORDER BY ts LIMIT 5", &strata::query::Params::new(), false, Duration::from_secs(5)).await.unwrap();
    assert_eq!(rs.rows.len(), 2, "{:?}", rs.rows);
    assert_eq!(rs.rows[0][1], strata::Value::Str("nightly-reports/2.3".into()));
    let params: Json = serde_json::from_str(rs.rows[0][3].as_str().unwrap()).unwrap();
    assert_eq!(params["q"], "laptop");
    assert_eq!(rs.rows[0][4], strata::Value::Int(2000));
    let (rs, _) = store.query("SELECT COUNT(*) FROM queries WHERE client = 'command-center'", &strata::query::Params::new(), false, Duration::from_secs(5)).await.unwrap();
    assert_eq!(rs.rows[0][0], strata::Value::Int(1));
}
