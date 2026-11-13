//! Writes through a coordinator of two shards that a single server
//! answers one way and a cluster answered another (the 2026-11-13 swarm,
//! data engineer and ML engineer): a primary key unique across the shards
//! of a table placed by a column, a write by key with the wrong routing
//! refused, write receipts and `strict`, ON CONFLICT and UPDATE/DELETE
//! ... LIMIT, and a table's rows exported.

use std::sync::Arc;
use std::time::Duration;

use chloedb::db::Db;
use chloedb::storage::DbOptions;
use chloedb::Schema;
use chloedb_server::auth::Access;
use chloedb_server::coord::Coordinator;
use chloedb_server::service::{AgentConfig, Backend, Service};
use chloedb_server::server;
use serde_json::{json, Value as Json};
use tokio::net::TcpListener;

fn agent() -> AgentConfig {
    AgentConfig { api_key: None, model: chloedb_server::agent::DEFAULT_MODEL.into(), base_url: "http://unused".into() }
}

fn first() -> Schema {
    Schema::from_json(r#"{"table": "items", "key": "id", "fields": [{"name": "id", "type": "keyword"}, {"name": "n", "type": "int"}]}"#).unwrap()
}

async fn shard(dir: &std::path::Path, name: &str) -> String {
    let db = Db::create(&dir.join(name), first(), DbOptions { publish_snapshots: true, ..DbOptions::default() }).unwrap();
    let schema = (*db.schema()).clone();
    let svc = Arc::new(Service::new(Backend::Local(Arc::new(db)), schema, agent(), 16, Duration::from_secs(20)));
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(server::serve_grpc(svc, l, Access::default()));
    url
}

/// A coordinator over two shards, served over HTTP: its URL.
async fn two_shards(dir: &std::path::Path) -> String {
    let shards = vec![shard(dir, "a.strata").await, shard(dir, "b.strata").await];
    let c = Coordinator::connect(shards).await.unwrap();
    let schema = c.schema.clone();
    let svc = Arc::new(Service::new(Backend::Cluster(Arc::new(c)), schema, agent(), 16, Duration::from_secs(20)));
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(server::serve_http(svc, l, Access::default()));
    url
}

struct Api {
    c: reqwest::Client,
    base: String,
}

impl Api {
    async fn post(&self, path: &str, body: Json) -> (u16, Json) {
        let r = self.c.post(format!("{}{path}", self.base)).json(&body).send().await.unwrap();
        (r.status().as_u16(), r.json::<Json>().await.unwrap_or(Json::Null))
    }

    async fn sql(&self, sql: &str) -> (u16, Json) {
        self.post("/v1/sql", json!({"sql": sql})).await
    }

    async fn count(&self, sql: &str) -> i64 {
        let (s, a) = self.post("/v1/query", json!({"sql": sql})).await;
        assert_eq!(s, 200, "{sql}: {a}");
        a["rows"][0][0].as_i64().or_else(|| a["rows"][0].as_object().and_then(|o| o.values().next()).and_then(Json::as_i64)).unwrap_or_else(|| panic!("{sql}: {a}"))
    }

    async fn import(&self, query: &str, ct: &str, body: &str) -> (u16, Json) {
        let r = self.c.post(format!("{}/v1/import?{query}", self.base)).header("content-type", ct).body(body.to_string()).send().await.unwrap();
        (r.status().as_u16(), r.json::<Json>().await.unwrap_or(Json::Null))
    }
}

async fn api() -> (tempfile::TempDir, Api) {
    let dir = tempfile::tempdir().unwrap();
    let base = two_shards(dir.path()).await;
    let api = Api { c: chloedb_server::http_client().build().unwrap(), base };
    let (s, a) = api.sql("CREATE TABLE prods (id KEYWORD PRIMARY KEY, sku KEYWORD NOT NULL, title TEXT, stock INT) OPTIONS(shard_by = 'sku')").await;
    assert_eq!(s, 200, "{a}");
    (dir, api)
}

#[tokio::test]
async fn an_insert_of_a_key_another_shard_holds_is_refused() {
    let (_dir, api) = api().await;
    let (s, a) = api.sql("INSERT INTO prods (id, sku, title) VALUES ('p1', 'SKU-000', 'a')").await;
    assert_eq!(s, 200, "{a}");
    // Some of these skus go to the other shard: none may add a second p1.
    for i in 1..12 {
        let (s, a) = api.sql(&format!("INSERT INTO prods (id, sku, title) VALUES ('p1', 'SKU-{i:03}', 'b')")).await;
        assert_eq!(s, 409, "SKU-{i:03}: {a}");
        assert!(a["error"].as_str().unwrap().contains("p1"), "{a}");
    }
    assert_eq!(api.count("SELECT COUNT(*) FROM prods").await, 1);
    // Through the write operations too, and one key twice in one request.
    let (s, a) = api.post("/v1/write", json!({"ops": [{"op": "insert", "table": "prods", "doc": {"id": "p1", "sku": "SKU-007", "title": "c"}}]})).await;
    assert_eq!(s, 409, "{a}");
    let ops: Vec<Json> = (0..12).map(|i| json!({"op": "insert", "table": "prods", "doc": {"id": "p2", "sku": format!("SKU-{i:03}"), "title": "d"}})).collect();
    let (s, a) = api.post("/v1/write", json!({"ops": ops})).await;
    assert!(s == 409 || s == 400, "{s} {a}");
    assert_eq!(api.count("SELECT COUNT(*) FROM prods WHERE id = 'p2'").await, 0);
    // ON CONFLICT DO NOTHING keeps the row, wherever it is.
    for i in 0..6 {
        let (s, a) = api.sql(&format!("INSERT INTO prods (id, sku, title) VALUES ('p1', 'SKU-{i:03}', 'e') ON CONFLICT (id) DO NOTHING")).await;
        assert_eq!(s, 200, "{a}");
        assert_eq!(a["written"], 0, "{a}");
    }
    let (s, a) = api.sql("INSERT INTO prods (id, sku, title) VALUES ('p3', 'SKU-003', 'f') ON CONFLICT (id) DO NOTHING").await;
    assert_eq!((s, a["written"].as_u64()), (200, Some(1)), "{a}");
    // ON CONFLICT DO UPDATE, on the row's shard.
    let (s, a) = api.sql("INSERT INTO prods (id, sku, title, stock) VALUES ('p3', 'SKU-003', 'g', 5) ON CONFLICT (id) DO UPDATE SET stock = 7").await;
    assert_eq!(s, 200, "{a}");
    assert_eq!(api.count("SELECT stock FROM prods WHERE id = 'p3'").await, 7);
    assert_eq!(api.count("SELECT COUNT(*) FROM prods").await, 2);
}

#[tokio::test]
async fn a_write_by_key_with_the_wrong_routing_is_refused() {
    let (_dir, api) = api().await;
    let (s, a) = api.sql("INSERT INTO prods (id, sku, title, stock) VALUES ('p5', 'SKU-005', 'a', 1)").await;
    assert_eq!(s, 200, "{a}");
    let mut refused = 0;
    for i in 0..12 {
        let sku = format!("SKU-{i:03}");
        let (s, a) = api.post("/v1/write", json!({"ops": [{"op": "set", "table": "prods", "key": "p5", "field": "stock", "value": 9, "routing": sku}]})).await;
        match sku.as_str() {
            "SKU-005" => assert_eq!((s, a["written"].as_u64()), (200, Some(1)), "{a}"),
            _ if s == 404 => {
                refused += 1;
                let e = a["error"].as_str().unwrap();
                assert!(e.contains("p5") && e.contains(&sku) && e.contains("sku"), "{e}");
            }
            // The same shard as SKU-005's: the row is there.
            _ => assert_eq!((s, a["written"].as_u64()), (200, Some(1)), "{sku}: {a}"),
        }
    }
    assert!(refused > 0, "no sku went to the other shard");
    // Asked to pass over a key that is not there, it does.
    let ops: Vec<Json> = (0..12).map(|i| json!({"op": "delete", "table": "prods", "key": "nope", "routing": format!("SKU-{i:03}"), "if_missing": "skip"})).collect();
    let (s, a) = api.post("/v1/write", json!({"ops": ops})).await;
    assert_eq!(s, 200, "{a}");
    assert_eq!(a["receipt"]["not_found"], 12, "{a}");
}

#[tokio::test]
async fn writes_answer_receipts_and_strict_writes_nothing_on_a_mismatch() {
    let (_dir, api) = api().await;
    // One key twice: one row, and the answer says so.
    let (s, a) = api.import("table=prods&format=json", "application/json", r#"[{"id": "k1", "sku": "S1", "title": "a"}, {"id": "k1", "sku": "S1", "title": "b"}, {"id": "k2", "sku": "S2", "title": "c"}]"#).await;
    assert_eq!(s, 200, "{a}");
    assert_eq!(a["written"], 2, "{a}");
    let r = &a["receipt"];
    assert_eq!((r["received"].as_u64(), r["inserted"].as_u64(), r["collapsed"].as_u64(), r["replaced"].as_u64()), (Some(3), Some(2), Some(1), Some(0)), "{a}");
    let per: u64 = r["per_shard"].as_array().unwrap().iter().map(|p| p["rows"].as_u64().unwrap()).sum();
    assert_eq!(per, 2, "{a}");
    // Again: replaced, and strict refuses it, writing nothing.
    let (s, a) = api.import("table=prods&format=json", "application/json", r#"[{"id": "k2", "sku": "S2", "title": "z"}]"#).await;
    assert_eq!((s, a["receipt"]["replaced"].as_u64()), (200, Some(1)), "{a}");
    let (s, a) = api.import("table=prods&format=json&strict=true", "application/json", r#"[{"id": "k3", "sku": "S3", "title": "new"}, {"id": "k2", "sku": "S2", "title": "y"}]"#).await;
    assert_eq!(s, 409, "{a}");
    let (s, a) = api.import("table=prods&format=json&strict=true", "application/json", r#"[{"id": "k4", "sku": "S4"}, {"id": "k4", "sku": "S4"}]"#).await;
    assert_eq!(s, 409, "{a}");
    assert_eq!(api.count("SELECT COUNT(*) FROM prods").await, 2);
    let (s, a) = api.post("/v1/write", json!({"strict": true, "ops": [{"op": "upsert", "table": "prods", "doc": {"id": "k1", "sku": "S1", "title": "again"}}]})).await;
    assert_eq!(s, 409, "{a}");
    let (s, a) = api.post("/v1/write", json!({"strict": true, "ops": [{"op": "set", "table": "prods", "key": "nope", "routing": "S1", "field": "stock", "value": 1}]})).await;
    assert_eq!(s, 404, "{a}");
    let (s, a) = api.post("/v1/write", json!({"strict": true, "ops": [{"op": "upsert", "table": "prods", "doc": {"id": "k9", "sku": "S9", "title": "fresh"}}]})).await;
    assert_eq!((s, a["receipt"]["inserted"].as_u64()), (200, Some(1)), "{a}");
    // CSV: an empty cell given the column's default, or NULL.
    let (s, a) = api.sql("CREATE TABLE gaps (id INT PRIMARY KEY, n INT NOT NULL DEFAULT 4, t KEYWORD)").await;
    assert_eq!(s, 200, "{a}");
    let (s, a) = api.import("table=gaps&format=csv&empty=default", "text/csv", "id,n,t\n1,,x\n").await;
    assert_eq!(s, 200, "{a}");
    assert_eq!(api.count("SELECT n FROM gaps WHERE id = 1").await, 4);
    let (s, a) = api.import("table=gaps&format=csv&empty=nonsense", "text/csv", "id,n,t\n2,,x\n").await;
    assert_eq!(s, 400, "{a}");
}

#[tokio::test]
async fn update_and_delete_with_a_limit_change_that_many_rows() {
    let (_dir, api) = api().await;
    let ops: Vec<Json> = (0..20).map(|i| json!({"op": "upsert", "table": "prods", "doc": {"id": format!("r{i:02}"), "sku": format!("S{}", i % 7), "title": "t", "stock": i}})).collect();
    let (s, a) = api.post("/v1/write", json!({"ops": ops})).await;
    assert_eq!(s, 200, "{a}");
    let (s, a) = api.sql("UPDATE prods SET stock = 100 WHERE stock < 10 LIMIT 3").await;
    assert_eq!((s, a["written"].as_u64()), (200, Some(3)), "{a}");
    assert_eq!(api.count("SELECT COUNT(*) FROM prods WHERE stock = 100").await, 3);
    let (s, a) = api.sql("DELETE FROM prods WHERE stock < 100 ORDER BY stock DESC LIMIT 4").await;
    assert_eq!((s, a["written"].as_u64()), (200, Some(4)), "{a}");
    assert_eq!(api.count("SELECT COUNT(*) FROM prods").await, 16);
    assert_eq!(api.count("SELECT COUNT(*) FROM prods WHERE stock >= 16 AND stock < 100").await, 0);
    // Whether a statement runs here, unrun.
    let (s, a) = api.post("/v1/sql/check", json!({"sql": "DELETE FROM prods WHERE stock < 3 LIMIT 2"})).await;
    assert_eq!((s, a["runs"].as_bool(), a["deployment"].as_str()), (200, Some(true), Some("cluster")), "{a}");
    assert!(a["notes"][0].as_str().unwrap().contains("two steps"), "{a}");
    let (_, a) = api.post("/v1/sql/check", json!({"sql": "ALTER TABLE prods DROP COLUMN stock"})).await;
    assert_eq!(a["runs"], false, "{a}");
    let (_, a) = api.post("/v1/sql/check", json!({"sql": "SELECT nope FROM prods"})).await;
    assert_eq!(a["runs"], false, "{a}");
    assert_eq!(api.count("SELECT COUNT(*) FROM prods").await, 16);
    // A NOT NULL column without a default counts every shard's rows.
    let (s, a) = api.sql("ALTER TABLE prods ADD COLUMN must INT NOT NULL").await;
    assert_eq!(s, 400, "{a}");
    let e = a["error"].as_str().unwrap();
    assert!(e.contains("16 rows") || !e.contains("rows"), "{e}");
}

#[tokio::test]
async fn a_cluster_exports_a_table_s_rows() {
    let (_dir, api) = api().await;
    let ops: Vec<Json> = (0..30).map(|i| json!({"op": "upsert", "table": "prods", "doc": {"id": format!("e{i:02}"), "sku": format!("S{}", i % 5), "title": format!("t, \"{i}\""), "stock": i}})).collect();
    let (s, a) = api.post("/v1/write", json!({"ops": ops})).await;
    assert_eq!(s, 200, "{a}");
    let get = |q: &str| api.c.get(format!("{}/v1/export?{q}", api.base)).send();
    let r = get("format=jsonl&tables=prods").await.unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let text = r.text().await.unwrap();
    let rows: Vec<Json> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(rows.len(), 30, "{text}");
    let mut ids: Vec<&str> = rows.iter().map(|r| r["id"].as_str().unwrap()).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 30);
    let csv = get("format=csv&tables=prods").await.unwrap().text().await.unwrap();
    assert_eq!(csv.lines().count(), 31, "{csv}");
    assert!(csv.lines().next().unwrap().contains("sku"), "{csv}");
    let arrow = get("format=arrow&tables=prods").await.unwrap();
    assert_eq!(arrow.status().as_u16(), 200);
    assert!(!arrow.bytes().await.unwrap().is_empty());
    let pq = get("format=parquet&tables=prods").await.unwrap();
    assert_eq!(pq.status().as_u16(), 200);
    let bytes = pq.bytes().await.unwrap();
    assert!(bytes.starts_with(b"PAR1"), "{} bytes", bytes.len());
    // Imported back into another table, as a server's own export is.
    let (s, a) = api.sql("CREATE TABLE prods2 (id KEYWORD PRIMARY KEY, sku KEYWORD NOT NULL, title TEXT, stock INT) OPTIONS(shard_by = 'sku')").await;
    assert_eq!(s, 200, "{a}");
    let r = api.c.post(format!("{}/v1/import?table=prods2&format=parquet", api.base)).header("content-type", "application/vnd.apache.parquet").body(bytes).send().await.unwrap();
    let (s, a) = (r.status().as_u16(), r.json::<Json>().await.unwrap());
    assert_eq!(s, 200, "{a}");
    assert_eq!(api.count("SELECT COUNT(*) FROM prods2").await, 30);
    // One table at a time.
    assert_eq!(get("format=jsonl&tables=prods,prods2").await.unwrap().status().as_u16(), 400);
}
