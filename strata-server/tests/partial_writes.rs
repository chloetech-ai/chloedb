//! A write through a coordinator that fails on one shard after another
//! took its part answers an idempotency key (the client's, or one the
//! coordinator gave it). The same request sent again with that key writes
//! the rest, and the shard that took its part does not apply it again:
//! for write operations and for SQL statements alike.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value as Json};
use strata::db::Db;
use strata::storage::DbOptions;
use strata::Schema;
use strata_server::auth::Access;
use strata_server::coord::Coordinator;
use strata_server::service::{AgentConfig, Backend, Service};
use strata_server::server;
use tokio::net::{TcpListener, TcpSocket};
use tokio::task::JoinHandle;

fn agent() -> AgentConfig {
    AgentConfig { api_key: None, model: strata_server::agent::DEFAULT_MODEL.into(), base_url: "http://unused".into() }
}

fn counters() -> Schema {
    Schema::from_json(r#"{"table": "counters", "key": "id", "fields": [{"name": "id", "type": "keyword"}, {"name": "n", "type": "int"}]}"#).unwrap()
}

fn shard(dir: &std::path::Path, name: &str) -> Arc<Service> {
    let db = Db::create(&dir.join(name), counters(), DbOptions::default()).unwrap();
    let schema = (*db.schema()).clone();
    Arc::new(Service::new(Backend::Local(Arc::new(db)), schema, agent(), 16, Duration::from_secs(10)))
}

/// Serves `svc` over gRPC at `addr` (port 0: any), until the handle is
/// aborted; the address can be served again after.
async fn grpc_at(svc: Arc<Service>, addr: SocketAddr) -> (SocketAddr, JoinHandle<()>) {
    let socket = TcpSocket::new_v4().unwrap();
    socket.set_reuseaddr(true).unwrap();
    socket.bind(addr).unwrap();
    let l: TcpListener = socket.listen(64).unwrap();
    let at = l.local_addr().unwrap();
    let h = tokio::spawn(async move {
        let _ = server::serve_grpc(svc, l, Access::default()).await;
    });
    (at, h)
}

async fn http(svc: Arc<Service>) -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(server::serve_http(svc, l, Access::default()));
    url
}

async fn post(url: &str, path: &str, body: Json) -> (u16, Json) {
    let r = strata_server::http_client().build().unwrap().post(format!("{url}{path}")).json(&body).send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap())
}

/// The rows and the sum of their counts, through the coordinator.
async fn totals(url: &str) -> (i64, i64) {
    let (_, a) = post(url, "/v1/query", json!({"sql": "SELECT COUNT(*), SUM(n) FROM counters"})).await;
    (a["rows"][0][0].as_i64().unwrap(), a["rows"][0][1].as_i64().unwrap_or(0))
}

/// A failure partway: the shard that took its part and the one that did
/// not, and the key to send it again with.
fn failed_partway(status: u16, a: &Json) -> String {
    assert!(status >= 400 && a["partial"] == true, "{status} {a}");
    let shards = a["shards"].as_array().unwrap();
    assert!(shards[0]["rows"].as_u64().unwrap() > 0 && shards[1]["error"].is_string(), "{a}");
    let key = a["idempotency_key"].as_str().unwrap_or_else(|| panic!("no key: {a}")).to_string();
    assert!(a["error"].as_str().unwrap().contains(&format!("idempotency_key \"{key}\"")), "{a}");
    key
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_write_that_failed_partway_is_sent_again_with_its_key() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (shard(dir.path(), "a.strata"), shard(dir.path(), "b.strata"));
    let (at_a, _) = grpc_at(a, "127.0.0.1:0".parse().unwrap()).await;
    let (at_b, served_b) = grpc_at(b.clone(), "127.0.0.1:0".parse().unwrap()).await;
    let c = Arc::new(Coordinator::connect(vec![format!("http://{at_a}"), format!("http://{at_b}")]).await.unwrap());
    let url = http(Arc::new(Service::new(Backend::Cluster(c.clone()), c.schema.clone(), agent(), 16, Duration::from_secs(10)))).await;

    // Counters on both shards, each event adding 1.
    let ops: Vec<Json> = (0..40).map(|i| json!({"op": "add", "key": format!("k{i}"), "field": "n", "value": 1, "doc": {"id": format!("k{i}")}})).collect();
    let (status, w) = post(&url, "/v1/write", json!({"ops": ops})).await;
    assert_eq!(status, 200, "{w}");
    assert_eq!(totals(&url).await, (40, 40));

    // The second shard stops: the first takes its part, the error names a
    // key the coordinator made.
    served_b.abort();
    let _ = served_b.await;
    let (status, failed) = post(&url, "/v1/write", json!({"ops": ops})).await;
    let key = failed_partway(status, &failed);
    assert!(key.starts_with("auto-"), "{key}");
    let (_, back) = grpc_at(b.clone(), at_b).await;
    // Sent again with the key: the second shard's part alone is applied.
    let mut sent = (0, Json::Null);
    for _ in 0..50 {
        sent = post(&url, "/v1/write", json!({"ops": ops, "idempotency_key": key})).await;
        if sent.0 == 200 {
            break;
        }
        // The coordinator reconnects to the shard.
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(sent.0, 200, "{}", sent.1);
    assert_eq!(totals(&url).await, (40, 80), "every counter added to once more");
    // And again: nothing more.
    let (status, w) = post(&url, "/v1/write", json!({"ops": ops, "idempotency_key": key})).await;
    assert_eq!(status, 200, "{w}");
    assert_eq!(totals(&url).await, (40, 80));

    // A statement on every shard, with the client's own key.
    back.abort();
    let _ = back.await;
    let update = json!({"sql": "UPDATE counters SET n = n + 1 WHERE n > 0", "idempotency_key": "bump-1"});
    let (status, failed) = post(&url, "/v1/sql", update.clone()).await;
    assert_eq!(failed_partway(status, &failed), "bump-1");
    let _ = grpc_at(b.clone(), at_b).await;
    let mut sent = (0, Json::Null);
    for _ in 0..50 {
        sent = post(&url, "/v1/sql", update.clone()).await;
        if sent.0 == 200 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(sent.0, 200, "{}", sent.1);
    assert_eq!(totals(&url).await, (40, 120), "every row updated once");
    let (status, w) = post(&url, "/v1/sql", update.clone()).await;
    assert_eq!(status, 200, "{w}");
    assert_eq!(totals(&url).await, (40, 120));
    // The key with another statement is refused.
    let (status, w) = post(&url, "/v1/sql", json!({"sql": "UPDATE counters SET n = n + 2 WHERE n > 0", "idempotency_key": "bump-1"})).await;
    assert_eq!(status, 422, "{w}");

    // On one server, a statement's key answers a retry its first answer.
    let one = shard(dir.path(), "one.strata");
    let one = http(one).await;
    let insert = json!({"sql": "INSERT INTO counters (id, n) VALUES ('x', 1)", "idempotency_key": "ins-1"});
    assert_eq!(post(&one, "/v1/sql", insert.clone()).await.0, 200);
    let (status, w) = post(&one, "/v1/sql", insert).await;
    assert_eq!((status, w["written"].as_u64()), (200, Some(1)), "{w}");
}
