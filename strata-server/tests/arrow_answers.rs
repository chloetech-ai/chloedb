//! `POST /v1/query` with `"format": "arrow"`: the rows as an Arrow IPC
//! stream, the same rows and values as the JSON answer, each column of
//! one Arrow type (vectors as lists of Float32, times in UTC), from a
//! server and from a coordinator of two shards.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value as Json};
use strata::db::Db;
use strata::storage::DbOptions;
use strata_server::auth::Access;
use strata_server::coord::Coordinator;
use strata_server::service::{AgentConfig, Backend, Service};
use strata_server::{json as js, server};
use tokio::net::TcpListener;

fn agent() -> AgentConfig {
    AgentConfig { api_key: None, model: strata_server::agent::DEFAULT_MODEL.into(), base_url: "http://unused".into() }
}

fn empty(dir: &std::path::Path, name: &str) -> Arc<Service> {
    let db = Db::create_empty(&dir.join(name), DbOptions::default()).unwrap();
    let schema = (*db.schema()).clone();
    Arc::new(Service::new(Backend::Local(Arc::new(db)), schema, agent(), 16, Duration::from_secs(20)))
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
    let r = strata_server::http_client().build().unwrap().post(format!("{url}{path}")).json(&body).send().await.unwrap();
    let status = r.status().as_u16();
    (status, r.json().await.unwrap_or(Json::Null))
}

/// The answer to `sql` as Arrow, read back: its content type, whether
/// it has a took_ms header, its columns' names and Arrow types, and its
/// rows as the JSON answer writes them.
async fn arrow(url: &str, sql: &str) -> (String, bool, Vec<String>, Vec<String>, Json) {
    let r = strata_server::http_client().build().unwrap().post(format!("{url}/v1/query")).json(&json!({"sql": sql, "format": "arrow"})).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200, "{sql}");
    let ct = r.headers().get("content-type").unwrap().to_str().unwrap().to_string();
    let took = r.headers().get("x-strata-took-ms").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<f64>().ok()).is_some();
    let batch = strata::query::arrow::batch_of_ipc(&r.bytes().await.unwrap()).unwrap();
    let names = batch.schema().fields().iter().map(|f| f.name().clone()).collect();
    let types = batch.schema().fields().iter().map(|f| f.data_type().to_string()).collect();
    let cols: Vec<Vec<strata::Value>> = batch.columns().iter().map(|a| strata::query::arrow::values_of(a).unwrap()).collect();
    let rows = (0..batch.num_rows()).map(|r| json!(cols.iter().map(|c| js::to_json(&c[r])).collect::<Vec<_>>())).collect();
    (ct, took, names, types, rows)
}

async fn arrow_matches_json(url: &str) {
    let (s, j) = call(url, "/v1/sql", json!({"sql": "CREATE TABLE docs (id KEYWORD PRIMARY KEY, title TEXT, g KEYWORD, n INT, x FLOAT, ts TIMESTAMP, tags KEYWORD[], emb VECTOR(3))"})).await;
    assert_eq!(s, 200, "{j}");
    for _ in 0..50 {
        if call(url, "/v1/query", json!({"sql": "SELECT COUNT(*) FROM docs"})).await.0 == 200 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let words = ["river", "stone", "dragon", "letter"];
    let ops: Vec<Json> = (0..300)
        .map(|i| {
            json!({"op": "upsert", "doc": {
                "id": format!("d{i:03}"), "title": format!("{} {}", words[i % 4], words[(i / 4) % 4]), "g": format!("g{}", i % 5),
                "n": i, "x": (i * 37 % 100) as f64 / 10.0, "ts": format!("2026-01-{:02}T{:02}:00:00Z", 1 + i % 28, i % 24),
                "tags": [format!("t{}", i % 3), format!("u{}", i % 2)], "emb": [(i % 7) as f64, 1.0, (i % 3) as f64 / 2.0],
            }})
        })
        .collect();
    let (s, j) = call(url, "/v1/write", json!({"ops": ops})).await;
    assert_eq!(s, 200, "{j}");

    let queries = [
        "SELECT id, n, x, ts, tags, emb FROM docs WHERE n >= 100 ORDER BY n LIMIT 50",
        "SELECT id, title, score() FROM docs ORDER BY bm25(title, 'river dragon') DESC LIMIT 10",
        "SELECT g, COUNT(*) AS c, MAX(ts) AS last FROM docs GROUP BY g ORDER BY g",
        "WITH q AS (SELECT id, n FROM docs WHERE n < 20) SELECT COUNT(*) AS c FROM q",
        "SELECT id FROM docs WHERE n < 0",
    ];
    for sql in queries {
        let (s, want) = call(url, "/v1/query", json!({"sql": sql})).await;
        assert_eq!(s, 200, "{sql}: {want}");
        let (ct, took, names, _, rows) = arrow(url, sql).await;
        assert_eq!(ct, "application/vnd.apache.arrow.stream");
        assert!(took, "{sql}: x-strata-took-ms");
        assert_eq!(json!(names), want["columns"], "{sql}");
        assert_eq!(rows, want["rows"], "{sql}");
    }
    let (_, _, _, types, _) = arrow(url, queries[0]).await;
    assert_eq!(types[..4], ["Utf8", "Int64", "Float64", "Timestamp(µs, \"UTC\")"], "{types:?}");
    assert!(types[4].starts_with("List(") && types[4].contains("Utf8"), "{types:?}");
    assert!(types[5].starts_with("List(") && types[5].contains("Float32"), "{types:?}");

    let (s, j) = call(url, "/v1/query", json!({"sql": queries[0], "format": "csv"})).await;
    assert_eq!(s, 400, "{j}");
    assert!(j["error"].as_str().unwrap().contains("\"arrow\""), "{j}");
    let (s, j) = call(url, "/v1/query", json!({"sql": queries[0], "format": "arrow", "log": true})).await;
    assert_eq!(s, 400, "{j}");
    let (s, j) = call(url, "/v1/query", json!({"sql": queries[0], "format": "json"})).await;
    assert_eq!((s, j["rows"].as_array().map(Vec::len)), (200, Some(50)), "{j}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_server_answers_arrow() {
    let dir = tempfile::tempdir().unwrap();
    arrow_matches_json(&http(empty(dir.path(), "one.strata")).await).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_coordinator_answers_arrow() {
    let dir = tempfile::tempdir().unwrap();
    let mut shards = Vec::new();
    for i in 0..2 {
        shards.push(grpc(empty(dir.path(), &format!("s{i}.strata"))).await);
    }
    let c = Coordinator::connect(shards).await.unwrap();
    let schema = c.schema.clone();
    let svc = Arc::new(Service::new(Backend::Cluster(Arc::new(c)), schema, agent(), 16, Duration::from_secs(20)));
    arrow_matches_json(&http(svc).await).await;
}
