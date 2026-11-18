//! What round 3 of the persona study (18 November 2026) found in a
//! query's answer over HTTP, alone and through a coordinator of two
//! shards: the answer carried no column types (`"types"`), so a client
//! typed a column by its name and `ts AS d`, `MAX(ts)`, `i.ts` in a join
//! and `date_trunc('month', ts)` came back as text (SAM-301); a UNION of
//! an INT and a FLOAT answered both (SAM-308); and `{"embed": text}` or
//! `{"vector_of": key}` in `similarity(bge, :v)` took the table's first
//! vector field rather than `bge` (PRIYA-303).

use std::sync::Arc;
use std::time::Duration;

use chloedb::db::Db;
use chloedb::storage::DbOptions;
use chloedb_server::auth::Access;
use chloedb_server::coord::Coordinator;
use chloedb_server::service::{AgentConfig, Backend, Service};
use chloedb_server::server;
use serde_json::{json, Value as Json};
use tokio::net::TcpListener;

fn service(backend: Backend, schema: chloedb::Schema) -> Arc<Service> {
    let agent = AgentConfig { api_key: None, model: chloedb_server::agent::DEFAULT_MODEL.into(), base_url: "http://unused".into() };
    Arc::new(Service::new(backend, schema, agent, 16, Duration::from_secs(20)))
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

async fn call(url: &str, path: &str, body: Json) -> (u16, Json) {
    let c = chloedb_server::http_client().build().unwrap();
    let r = c.post(format!("{url}{path}")).json(&body).send().await.unwrap();
    let status = r.status().as_u16();
    (status, r.json().await.unwrap_or(Json::Null))
}

async fn sql(url: &str, s: &str) {
    let (status, j) = call(url, "/v1/sql", json!({"sql": s})).await;
    assert_eq!(status, 200, "{s}: {j}");
}

async fn answer(url: &str, s: &str) -> Json {
    let (status, j) = call(url, "/v1/query", json!({"sql": s})).await;
    assert_eq!(status, 200, "{s}: {j}");
    j
}

async fn types_follow_the_expression(url: &str) {
    sql(url, "CREATE TABLE invoice (id INT PRIMARY KEY, customer INT, ts TIMESTAMP, paid BOOL, total FLOAT)").await;
    sql(url, "CREATE TABLE customer (id INT PRIMARY KEY, name KEYWORD)").await;
    // A coordinator learns the tables from its shards.
    for _ in 0..50 {
        let (_, j) = call(url, "/v1/query", json!({"sql": "SELECT COUNT(*) FROM customer"})).await;
        if j.get("rows").is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    sql(url, "INSERT INTO invoice (id, customer, ts, paid, total) VALUES (1, 10, '2009-01-01T00:00:00Z', TRUE, 1.98), (2, 11, '2013-12-22T10:30:00Z', FALSE, 3.96), (3, 10, NULL, NULL, NULL)").await;
    sql(url, "INSERT INTO customer (id, name) VALUES (10, 'Ana'), (11, 'Sam')").await;
    for s in [
        "SELECT ts FROM invoice ORDER BY id LIMIT 1",
        "SELECT ts AS d FROM invoice ORDER BY id LIMIT 1",
        "SELECT MAX(ts) FROM invoice",
        "SELECT i.ts FROM invoice i JOIN customer c ON c.id = i.customer ORDER BY i.id LIMIT 1",
        "SELECT date_trunc('month', ts) AS m FROM invoice ORDER BY id LIMIT 1",
        "SELECT CAST(ts AS DATE) AS day FROM invoice ORDER BY id LIMIT 1",
        "SELECT ts + INTERVAL '1 day' AS next FROM invoice ORDER BY id LIMIT 1",
    ] {
        let j = answer(url, s).await;
        assert_eq!(j["types"], json!(["timestamp"]), "{s}: {j}");
    }
    let j = answer(url, "SELECT id, total, paid, ts FROM invoice ORDER BY id").await;
    assert_eq!(j["types"], json!(["int", "float", "bool", "timestamp"]), "{j}");
    assert_eq!(answer(url, "SELECT ts FROM invoice WHERE id = 3").await["types"], json!([null]), "only NULLs");
    // With objects, and on each page of a scroll.
    let (_, j) = call(url, "/v1/query", json!({"sql": "SELECT id, ts AS d FROM invoice ORDER BY id", "objects": true})).await;
    assert_eq!(j["types"], json!(["int", "timestamp"]), "{j}");
    let (_, j) = call(url, "/v1/query", json!({"sql": "SELECT id, ts AS d FROM invoice ORDER BY id", "page_size": 1})).await;
    assert_eq!(j["types"], json!(["int", "timestamp"]), "{j}");
    let (_, j) = call(url, "/v1/query", json!({"cursor": j["cursor"]})).await;
    assert_eq!(j["types"], json!(["int", "timestamp"]), "{j}");

    // SAM-308: the union's column is FLOAT.
    let j = answer(url, "SELECT 1 AS v UNION ALL SELECT 2.5").await;
    assert_eq!((&j["rows"], &j["types"]), (&json!([[1.0], [2.5]]), &json!(["float"])), "{j}");
    let j = answer(url, "SELECT id AS v FROM invoice WHERE id = 1 UNION ALL SELECT total FROM invoice WHERE id = 2").await;
    assert_eq!((&j["rows"], &j["types"]), (&json!([[1.0], [3.96]]), &json!(["float"])), "{j}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_server_types_each_column_by_its_expression() {
    let dir = tempfile::tempdir().unwrap();
    let url = http(empty(dir.path(), "one.chloedb")).await;
    types_follow_the_expression(&url).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_cluster_types_each_column_by_its_expression() {
    let dir = tempfile::tempdir().unwrap();
    let mut shards = Vec::new();
    for i in 0..2 {
        shards.push(grpc(empty(dir.path(), &format!("s{i}.chloedb"))).await);
    }
    let c = Coordinator::connect(shards).await.unwrap();
    let schema = c.schema.clone();
    let url = http(service(Backend::Cluster(Arc::new(c)), schema)).await;
    types_follow_the_expression(&url).await;
}

/// PRIYA-303: a parameter's vector is that of the field it is ranked
/// against.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_parameter_s_vector_is_the_field_it_is_used_with() {
    let dir = tempfile::tempdir().unwrap();
    let url = http(empty(dir.path(), "v.chloedb")).await;
    sql(&url, "CREATE TABLE other (id INT PRIMARY KEY)").await;
    sql(&url, "CREATE TABLE docs (id KEYWORD PRIMARY KEY, title TEXT, emb VECTOR(16) OPTIONS(embed = 'title', model = 'hash:16'), bge VECTOR(4))").await;
    sql(&url, "INSERT INTO docs (id, title, bge) VALUES ('a', 'vitamin d and covid', '[1,0,0,0]'), ('b', 'masks', '[0,1,0,0]')").await;
    let q = "SELECT id FROM docs ORDER BY similarity(bge, :v) DESC LIMIT 2";
    // vector_of reads bge, the field similarity names.
    let (s, j) = call(&url, "/v1/query", json!({"sql": q, "params": {"v": {"vector_of": "b"}}})).await;
    assert_eq!((s, &j["rows"]), (200, &json!([["b"], ["a"]])), "{j}");
    // embed: bge has no model, and the error says so (not a dimension mismatch).
    let (s, j) = call(&url, "/v1/query", json!({"sql": q, "params": {"v": {"embed": "vitamin d"}}})).await;
    let e = j["error"].as_str().unwrap_or_default();
    assert!(s == 400 && e.contains("parameter :v") && e.contains("docs.bge is embedded by no model") && e.contains("4 numbers"), "{s} {j}");
    // The field emb's model still embeds a text used with emb.
    let (s, j) = call(&url, "/v1/query", json!({"sql": "SELECT id FROM docs ORDER BY similarity(emb, :v) DESC LIMIT 2", "params": {"v": {"embed": "vitamin d"}}})).await;
    assert_eq!((s, j["rows"].as_array().map(Vec::len)), (200, Some(2)), "{j}");
    // Text for bge: the example names :v, the query's parameter.
    let (s, j) = call(&url, "/v1/query", json!({"sql": q, "params": {"v": "vitamin d"}})).await;
    let e = j["error"].as_str().unwrap_or_default();
    assert!(s == 400 && e.contains("{\"v\": {\"vector_of\"") && e.contains("embed('<model>', :v)"), "{s} {j}");
}
