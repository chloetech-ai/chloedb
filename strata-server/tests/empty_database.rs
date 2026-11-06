//! A server on a database made with no tables (`strata-server --db` on a
//! new path, and a StrataCluster with neither a schema nor demo): it lists
//! no tables, refuses writes that name none until the first table exists,
//! and then serves that table as a database created with it would, alone
//! and through a coordinator of two shards.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value as Json};
use strata::db::Db;
use strata::storage::DbOptions;
use strata_server::auth::Access;
use strata_server::coord::Coordinator;
use strata_server::service::{AgentConfig, Backend, Service};
use strata_server::server;
use tokio::net::TcpListener;

fn agent() -> AgentConfig {
    AgentConfig { api_key: None, model: strata_server::agent::DEFAULT_MODEL.into(), base_url: "http://unused".into() }
}

fn service(backend: Backend, schema: strata::Schema) -> Arc<Service> {
    Arc::new(Service::new(backend, schema, agent(), 16, Duration::from_secs(20)))
}

fn empty(dir: &std::path::Path, name: &str) -> Arc<Service> {
    let db = Db::create_empty(&dir.join(name), DbOptions::default()).unwrap();
    let schema = (*db.schema()).clone();
    service(Backend::Local(Arc::new(db)), schema)
}

async fn http(svc: Arc<Service>) -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(server::serve_http(svc, l, Access::default()));
    url
}

async fn grpc(svc: Arc<Service>) -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(server::serve_grpc(svc, l, Access::default()));
    url
}

/// A call's status and JSON answer.
async fn call(url: &str, method: &str, path: &str, body: Option<Json>) -> (u16, Json) {
    let c = strata_server::http_client().build().unwrap();
    let r = match method {
        "GET" => c.get(format!("{url}{path}")),
        _ => c.post(format!("{url}{path}")).json(&body.unwrap_or(Json::Null)),
    };
    let r = r.send().await.unwrap();
    let status = r.status().as_u16();
    (status, r.json().await.unwrap_or(Json::Null))
}

fn listed(j: &Json) -> Vec<String> {
    j["tables"].as_array().unwrap().iter().map(|t| t["table"].as_str().unwrap().to_string()).collect()
}

/// What every server and coordinator of a database with no tables does,
/// asked at `url`.
async fn starts_empty_then_serves_its_first_table(url: &str) {
    let (s, j) = call(url, "GET", "/v1/tables", None).await;
    assert_eq!((s, listed(&j)), (200, Vec::<String>::new()), "{j}");
    let (s, j) = call(url, "POST", "/v1/write", Some(json!({"ops": [{"op": "upsert", "doc": {"id": "x"}}]}))).await;
    assert_eq!(s, 400, "{j}");
    assert!(j["error"].as_str().unwrap().contains("no tables yet"), "{j}");
    let (s, j) = call(url, "GET", "/v1/schema", None).await;
    assert_eq!(s, 400, "{j}");

    let (s, j) = call(url, "POST", "/v1/sql", Some(json!({"sql": "CREATE TABLE snapshots (id KEYWORD PRIMARY KEY, item KEYWORD NOT NULL, price FLOAT) OPTIONS(shard_by = 'item', cluster_by = 'item')"}))).await;
    assert_eq!(s, 200, "{j}");
    // A coordinator learns the table from its shards.
    let mut names = Vec::new();
    for _ in 0..50 {
        names = listed(&call(url, "GET", "/v1/tables", None).await.1);
        if !names.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(names, ["snapshots"]);
    let (s, j) = call(url, "GET", "/v1/schema", None).await;
    assert_eq!((s, j["table"].as_str()), (200, Some("snapshots")), "{j}");

    // Writes that name no table go to it, as to a database's first table.
    let ops: Vec<Json> = (0..40).map(|i| json!({"op": "upsert", "doc": {"id": format!("s{i}"), "item": format!("i{}", i % 5), "price": i as f64}})).collect();
    let (s, j) = call(url, "POST", "/v1/write", Some(json!({"ops": ops}))).await;
    assert_eq!(s, 200, "{j}");
    let (s, j) = call(url, "POST", "/v1/query", Some(json!({"sql": "SELECT item, COUNT(*) AS n FROM snapshots GROUP BY item ORDER BY item"}))).await;
    assert_eq!(s, 200, "{j}");
    assert_eq!(j["rows"], json!([["i0", 8], ["i1", 8], ["i2", 8], ["i3", 8], ["i4", 8]]), "{j}");
    // The placeholder takes no writes even by name.
    let (s, j) = call(url, "POST", "/v1/write", Some(json!({"ops": [{"op": "upsert", "table": strata::schema::PLACEHOLDER_TABLE, "doc": {"id": "x"}}]}))).await;
    assert_eq!(s, 400, "{j}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_server_starts_with_no_tables() {
    let dir = tempfile::tempdir().unwrap();
    let url = http(empty(dir.path(), "one.strata")).await;
    starts_empty_then_serves_its_first_table(&url).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_cluster_starts_with_no_tables() {
    let dir = tempfile::tempdir().unwrap();
    let mut shards = Vec::new();
    for i in 0..2 {
        shards.push(grpc(empty(dir.path(), &format!("s{i}.strata"))).await);
    }
    let c = Coordinator::connect(shards).await.unwrap();
    let schema = c.schema.clone();
    assert!(schema.is_placeholder());
    let url = http(service(Backend::Cluster(Arc::new(c)), schema)).await;
    starts_empty_then_serves_its_first_table(&url).await;
}
