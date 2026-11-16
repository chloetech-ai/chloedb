//! A server on a database made with no tables (`chloedb-server --db` on a
//! new path, and a StrataCluster with neither a schema nor demo): it lists
//! no tables, refuses writes that name none until the first table exists,
//! and then serves that table as a database created with it would, alone
//! and through a coordinator of two shards.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value as Json};
use chloedb::db::Db;
use chloedb::storage::DbOptions;
use chloedb_server::auth::Access;
use chloedb_server::coord::Coordinator;
use chloedb_server::replication::Acks;
use chloedb_server::service::{AgentConfig, Backend, Service};
use chloedb_server::{server, slots};
use tokio::net::TcpListener;

fn agent() -> AgentConfig {
    AgentConfig { api_key: None, model: chloedb_server::agent::DEFAULT_MODEL.into(), base_url: "http://unused".into() }
}

fn service(backend: Backend, schema: chloedb::Schema) -> Arc<Service> {
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
    let c = chloedb_server::http_client().build().unwrap();
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
    // ONB-11: the sizes of every table, kept 30 s, list none yet...
    let (s, j) = call(url, "GET", "/v1/index/tables", None).await;
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
    // ...and a table made since is listed at once, not after 30 s.
    let (s, j) = call(url, "GET", "/v1/index/tables", None).await;
    assert_eq!((s, listed(&j)), (200, vec!["snapshots".to_string()]), "{j}");
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
    let (s, j) = call(url, "POST", "/v1/write", Some(json!({"ops": [{"op": "upsert", "table": chloedb::schema::PLACEHOLDER_TABLE, "doc": {"id": "x"}}]}))).await;
    assert_eq!(s, 400, "{j}");
}

/// PLT-2: the apps a server starts (Sheets, ChloeCtx, the crawler) make
/// their tables before anyone else; a request that names no table goes to
/// the first table people made, never to an app's own state (Sheets'
/// workbooks, ChloeCtx's spaces, the app engine's settings), and to a
/// crawl's only while there is no other.
async fn the_apps_tables_are_not_the_first(url: &str) {
    for ddl in [
        "CREATE TABLE sheets_books (id KEYWORD PRIMARY KEY, name TEXT)",
        "CREATE TABLE sheets_cells (id KEYWORD PRIMARY KEY, v TEXT)",
        "CREATE TABLE ctx_spaces (id KEYWORD PRIMARY KEY, name TEXT)",
        "CREATE TABLE sa_settings (id KEYWORD PRIMARY KEY, name TEXT)",
    ] {
        let (s, j) = call(url, "POST", "/v1/sql", Some(json!({"sql": ddl}))).await;
        assert_eq!(s, 200, "{ddl}: {j}");
    }
    let mut names = Vec::new();
    for _ in 0..50 {
        names = listed(&call(url, "GET", "/v1/tables", None).await.1);
        if names.len() == 4 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(names.len(), 4, "{names:?}");
    let (s, j) = call(url, "POST", "/v1/write", Some(json!({"ops": [{"op": "insert", "doc": {"id": "x1", "name": "my note"}}]}))).await;
    assert!(s == 400 && j["error"].as_str().unwrap().contains("no tables yet"), "{s} {j}");
    let (s, j) = call(url, "GET", "/v1/schema", None).await;
    assert_eq!(s, 400, "{j}");
    // A crawl's table is the data a crawl was asked for: requests naming
    // none go to it while it is the only one (`chloedb-crawl --local`).
    let (s, j) = call(url, "POST", "/v1/sql", Some(json!({"sql": "CREATE TABLE crawl_example_com (id KEYWORD PRIMARY KEY, title TEXT)"}))).await;
    assert_eq!(s, 200, "{j}");
    let mut first = None;
    for _ in 0..50 {
        let (s, j) = call(url, "GET", "/v1/schema", None).await;
        if s == 200 {
            first = j["table"].as_str().map(String::from);
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(first.as_deref(), Some("crawl_example_com"));
    // The first table people made takes them from then on.
    let (s, j) = call(url, "POST", "/v1/sql", Some(json!({"sql": "CREATE TABLE notes (id KEYWORD PRIMARY KEY, name TEXT, n INT)"}))).await;
    assert_eq!(s, 200, "{j}");
    let mut first = None;
    for _ in 0..50 {
        first = call(url, "GET", "/v1/schema", None).await.1["table"].as_str().map(String::from);
        if first.as_deref() == Some("notes") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(first.as_deref(), Some("notes"));
    let (s, j) = call(url, "POST", "/v1/write", Some(json!({"ops": [{"op": "insert", "doc": {"id": "x1", "name": "my note", "n": 3}}]}))).await;
    assert_eq!(s, 200, "{j}");
    for (table, n) in [("notes", 1), ("sheets_books", 0), ("crawl_example_com", 0)] {
        let (s, j) = call(url, "POST", "/v1/query", Some(json!({"sql": format!("SELECT COUNT(*) FROM {table}")}))).await;
        assert_eq!((s, j["rows"][0][0].as_u64()), (200, Some(n)), "{table}: {j}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_server_gives_requests_naming_no_table_to_its_own_first_not_the_apps() {
    let dir = tempfile::tempdir().unwrap();
    let url = http(empty(dir.path(), "apps.strata")).await;
    the_apps_tables_are_not_the_first(&url).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_cluster_gives_requests_naming_no_table_to_its_own_first_not_the_apps() {
    let dir = tempfile::tempdir().unwrap();
    let mut shards = Vec::new();
    for i in 0..2 {
        shards.push(grpc(empty(dir.path(), &format!("a{i}.strata"))).await);
    }
    let c = Coordinator::connect(shards).await.unwrap();
    let schema = c.schema.clone();
    let url = http(service(Backend::Cluster(Arc::new(c)), schema)).await;
    the_apps_tables_are_not_the_first(&url).await;
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

/// A server of a cluster: a numbered database (`db`) that keeps its
/// cluster map beside it, as `chloedb-server --db` does.
async fn shard(dir: &std::path::Path, name: &str, db: Db) -> (Arc<Service>, String) {
    db.storage().start_seq_no().unwrap();
    let schema = (*db.schema()).clone();
    let mut svc = Service::new(Backend::Local(Arc::new(db)), schema, agent(), 16, Duration::from_secs(20));
    svc.keep_slots_in(slots::file_of(&dir.join(name))).unwrap();
    let svc = Arc::new(svc);
    let url = grpc(svc.clone()).await;
    (svc, url)
}

async fn moved(c: &Coordinator) {
    for _ in 0..1200 {
        let m = c.moves();
        if m["job"]["state"] != "running" {
            assert_eq!(m["job"]["state"], "done", "{m}");
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the moves did not end: {}", c.moves());
}

/// A shard that joins a cluster that began with no tables is a server made
/// with none, as the cluster's were: the coordinator creates the cluster's
/// tables on it, and slots then move to it. A server made with a table
/// (`--schema`), even one of the cluster's, is refused, saying so.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_shard_joins_a_cluster_that_began_with_no_tables() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let (_, s0) = shard(d, "s0.strata", Db::create_empty(&d.join("s0.strata"), DbOptions::default()).unwrap()).await;
    let c = Arc::new(Coordinator::connect(vec![s0]).await.unwrap());
    let url = http(service(Backend::Cluster(c.clone()), c.schema.clone())).await;
    let ddl = "CREATE TABLE snapshots (id KEYWORD PRIMARY KEY, item KEYWORD NOT NULL, price FLOAT)";
    let (s, j) = call(&url, "POST", "/v1/sql", Some(json!({"sql": ddl}))).await;
    assert_eq!(s, 200, "{j}");
    let ops: Vec<Json> = (0..200).map(|i| json!({"op": "upsert", "table": "snapshots", "doc": {"id": format!("s{i}"), "item": format!("i{}", i % 5), "price": i as f64}})).collect();
    let (s, j) = call(&url, "POST", "/v1/write", Some(json!({"ops": ops}))).await;
    assert_eq!(s, 200, "{j}");

    let typed = chloedb::Schema::from_json(r#"{"table": "snapshots", "key": "id", "fields": [{"name": "id", "type": "keyword"}, {"name": "item", "type": "keyword"}, {"name": "price", "type": "float"}]}"#).unwrap();
    let (_, with_schema) = shard(d, "typed.strata", Db::create(&d.join("typed.strata"), typed, DbOptions::default()).unwrap()).await;
    let e = c.add_shard(vec![with_schema], Acks::All).await.unwrap_err();
    assert!(e.status == 400 && e.message.contains("began with no tables") && e.message.contains("no --schema"), "{}", e.message);

    let (s1, u1) = shard(d, "s1.strata", Db::create_empty(&d.join("s1.strata"), DbOptions::default()).unwrap()).await;
    c.add_shard(vec![u1], Acks::All).await.unwrap();
    let names: Vec<String> = s1.catalog().iter().filter(|t| !t.is_placeholder()).map(|t| t.table.clone()).collect();
    assert_eq!(names, ["snapshots"]);
    c.rebalance(256).unwrap();
    moved(&c).await;
    let (_, j) = call(&url, "POST", "/v1/query", Some(json!({"sql": "SELECT COUNT(*), SUM(price) FROM snapshots"}))).await;
    assert_eq!(j["rows"], json!([[200, 19900.0]]), "{j}");
    let held = s1.query(chloedb_server::service::QueryReq { sql: "SELECT COUNT(*) FROM snapshots".into(), ..Default::default() }).await.unwrap();
    let n = chloedb_server::json::result(&held.rs, 0.0)["rows"][0][0].as_u64().unwrap();
    assert!(n > 0 && n < 200, "the new shard holds {n} of the rows");
}
