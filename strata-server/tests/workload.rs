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
