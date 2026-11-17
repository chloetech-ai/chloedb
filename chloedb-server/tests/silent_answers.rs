//! What the persona study of 17 November 2026 found a server answering
//! wrongly without an error, on a database made with no tables, alone and
//! through a coordinator of two shards: `/v1/write`'s top-level `table`
//! (ignored: the rows went to the first table) and fields it does not take,
//! `/v1/health`'s `docs` (it counted the placeholder, so 0), an upsert that
//! leaves fields out (it says nothing of the values it drops), and
//! weighted `rrf` and prefix terms in `bm25`, which shards rank for the
//! coordinator.

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

async fn call(url: &str, path: &str, body: Option<Json>) -> (u16, Json) {
    let c = chloedb_server::http_client().build().unwrap();
    let r = match body {
        None => c.get(format!("{url}{path}")),
        Some(b) => c.post(format!("{url}{path}")).json(&b),
    };
    let r = r.send().await.unwrap();
    let status = r.status().as_u16();
    (status, r.json().await.unwrap_or(Json::Null))
}

async fn write(url: &str, body: Json) -> (u16, Json) {
    call(url, "/v1/write", Some(body)).await
}

/// The first column of each row of `sql`'s answer.
async fn ids(url: &str, sql: &str) -> Vec<String> {
    let (s, j) = call(url, "/v1/query", Some(json!({"sql": sql}))).await;
    assert_eq!(s, 200, "{sql}: {j}");
    j["rows"].as_array().unwrap().iter().map(|r| r[0].as_str().map(String::from).unwrap_or_else(|| r[0].to_string())).collect()
}

async fn count(url: &str, table: &str) -> u64 {
    let (s, j) = call(url, "/v1/query", Some(json!({"sql": format!("SELECT COUNT(*) FROM {table}")}))).await;
    assert_eq!(s, 200, "{j}");
    j["rows"][0][0].as_u64().unwrap()
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v
}

async fn answers_what_it_was_asked(url: &str) {
    for ddl in ["CREATE TABLE p (id KEYWORD PRIMARY KEY, name TEXT, color TEXT, n INT)", "CREATE TABLE payroll (id INT PRIMARY KEY, name TEXT, salary INT)"] {
        let (s, j) = call(url, "/v1/sql", Some(json!({"sql": ddl}))).await;
        assert_eq!(s, 200, "{ddl}: {j}");
    }
    // A coordinator learns the tables from its shards.
    for _ in 0..50 {
        if call(url, "/v1/tables", None).await.1["tables"].as_array().is_some_and(|t| t.len() == 2) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // DEV-2: the health's docs are the default table's (p), not the
    // placeholder's.
    let rows = [("a", "sofa", "red", 1), ("b", "sofa sofas software", "blue", 2), ("c", "chair", "green", 3), ("d", "soft lamp", "blue red", 4)];
    let ops: Vec<Json> = rows.iter().map(|(id, name, color, n)| json!({"op": "upsert", "doc": {"id": id, "name": name, "color": color, "n": n}})).collect();
    let (s, j) = write(url, json!({"ops": ops})).await;
    assert_eq!(s, 200, "{j}");
    assert!(j.get("warnings").is_none(), "whole rows: {j}");
    let (s, j) = call(url, "/v1/health", None).await;
    assert_eq!((s, j["docs"].as_u64()), (200, Some(4)), "{j}");

    // LENA-6: the body's table is the table of the operations that name none.
    let (s, j) = write(url, json!({"table": "payroll", "ops": [{"op": "insert", "doc": {"id": 7, "name": "Ana", "salary": 100}}, {"op": "insert", "table": "p", "doc": {"id": "e", "name": "lamp", "n": 5}}]})).await;
    assert_eq!(s, 200, "{j}");
    assert_eq!((count(url, "payroll").await, count(url, "p").await), (1, 5));
    let (s, j) = write(url, json!({"table": "p", "ops": [{"op": "upsert", "doc": {"id": 77, "name": "meant for payroll", "salary": 1}}]})).await;
    assert_eq!(s, 400, "{j}");
    assert!(j["error"].as_str().unwrap().contains("no field salary in table p"), "{j}");
    let (s, j) = write(url, json!({"table": "nope", "ops": [{"op": "upsert", "doc": {"id": "x"}}]})).await;
    assert!(s >= 400 && j["error"].as_str().unwrap().contains("nope"), "{s} {j}");
    let (s, j) = write(url, json!({"tabel": "p", "ops": [{"op": "upsert", "doc": {"id": "x"}}]})).await;
    assert_eq!(s, 400, "{j}");
    let e = j["error"].as_str().unwrap();
    assert!(e.contains("no field \"tabel\"") && e.contains("table"), "{e}");
    assert_eq!((count(url, "payroll").await, count(url, "p").await), (1, 5), "nothing written by the refused");

    // MAYA-6: an upsert that replaces a row and leaves fields out says so.
    let (s, j) = write(url, json!({"ops": [{"op": "upsert", "table": "p", "doc": {"id": "c", "n": 9}}]})).await;
    assert_eq!((s, j["receipt"]["replaced"].as_u64()), (200, Some(1)), "{j}");
    let w = j["warnings"][0].as_str().unwrap_or_else(|| panic!("no warning: {j}"));
    assert!(w.contains("name, color of p") && w.contains("\"op\": \"set\"") && w.contains("UPDATE p SET name"), "{w}");
    assert_eq!(ids(url, "SELECT name FROM p WHERE id = 'c'").await, ["null"], "the row as the upsert wrote it");
    // A new key, or a whole row, replaces nothing left out.
    let (s, j) = write(url, json!({"ops": [{"op": "upsert", "table": "p", "doc": {"id": "f", "n": 1}}]})).await;
    assert!(s == 200 && j.get("warnings").is_none(), "{j}");
    let (s, j) = write(url, json!({"ops": [{"op": "upsert", "table": "p", "doc": {"id": "c", "name": "chair", "color": "green", "n": 3}}]})).await;
    assert!(s == 200 && j.get("warnings").is_none(), "{j}");

    // PRIYA-2: rrf's weights, ranked over shards as on one server.
    let q = |w: &str| format!("SELECT id FROM p WHERE id IN ('a', 'c') ORDER BY rrf(bm25(name, 'sofa'), bm25(color, 'green'){w}) DESC");
    assert_eq!(ids(url, &q(", weights => [0.1, 0.9]")).await, ["c", "a"]);
    assert_eq!(ids(url, &q(", weights => [0.9, 0.1]")).await, ["a", "c"]);
    let (s, j) = call(url, "/v1/query", Some(json!({"sql": "SELECT id FROM p ORDER BY bm25(name, 'sofa', bogus => 1) DESC"}))).await;
    assert!(s == 400 && j["error"].as_str().unwrap().contains("unknown option bogus for bm25"), "{s} {j}");

    // DEV-1: a prefix and a typo rank the rows MATCH keeps.
    for (word, kept) in [("sof*", vec!["a", "b", "d"]), ("sofaa~", vec!["a", "b"])] {
        assert_eq!(sorted(ids(url, &format!("SELECT id FROM p WHERE MATCH(name, '{word}')")).await), kept, "MATCH {word}");
        assert_eq!(sorted(ids(url, &format!("SELECT id FROM p ORDER BY bm25(name, '{word}') DESC")).await), kept, "bm25 {word}");
    }
    // ...and ORDER BY its alias.
    let r = ids(url, "SELECT id, bm25(name, 'sof*') AS score FROM p ORDER BY score DESC").await;
    assert_eq!(sorted(r), ["a", "b", "d"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_server_answers_what_it_was_asked() {
    let dir = tempfile::tempdir().unwrap();
    let url = http(empty(dir.path(), "one.strata")).await;
    answers_what_it_was_asked(&url).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_cluster_answers_what_it_was_asked() {
    let dir = tempfile::tempdir().unwrap();
    let mut shards = Vec::new();
    for i in 0..2 {
        shards.push(grpc(empty(dir.path(), &format!("s{i}.strata"))).await);
    }
    let c = Coordinator::connect(shards).await.unwrap();
    let schema = c.schema.clone();
    let url = http(service(Backend::Cluster(Arc::new(c)), schema)).await;
    answers_what_it_was_asked(&url).await;
}
