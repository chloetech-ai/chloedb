//! A follower's changes of tables sent again: a `CREATE TABLE` (or an
//! `ALTER TABLE`) the follower's cluster applied, but whose answer was
//! lost (a 503 from a proxy, a coordinator that stopped as it answered),
//! is tried again by the follow process. The second try finds the table
//! made ("already exists"); the follower takes the table as made when it
//! is as the source has it, rather than stopping.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::http::{Method, StatusCode, Uri};
use axum::response::IntoResponse;
use serde_json::{json, Value as Json};
use chloedb::db::Db;
use chloedb::query::Params;
use chloedb::storage::DbOptions;
use chloedb_server::auth::Access;
use chloedb_server::coord::Coordinator;
use chloedb_server::follow::{self, Options};
use chloedb_server::keys::Principal;
use chloedb_server::service::{After, AgentConfig, Backend, QueryReq, Service};
use chloedb_server::{demo, replication, server, slots};
use tokio::net::TcpListener;

fn agent() -> AgentConfig {
    AgentConfig { api_key: None, model: chloedb_server::agent::DEFAULT_MODEL.into(), base_url: "http://unused".into() }
}

/// A shard over gRPC, its writes numbered, holding `rows` demo products.
async fn shard(dir: &Path, name: &str, rows: usize) -> String {
    let opts = DbOptions { publish_snapshots: true, ship_log_bytes: 16 << 20, ..DbOptions::default() };
    let path = dir.join(name);
    let db = Db::create(&path, demo::schema(rows.max(1)), opts).unwrap();
    db.set_embedder("emb", Arc::new(chloedb::embed::HashEmbedder { dim: demo::DIM })).unwrap();
    if rows > 0 {
        db.load((0..rows).map(|i| demo::product(i, 7))).unwrap();
    }
    db.storage().start_seq_no().unwrap();
    let db = Arc::new(db);
    let schema = (*db.schema()).clone();
    let mut svc = Service::new(Backend::Local(db), schema, agent(), 16, Duration::from_secs(10));
    svc.keep_slots_in(slots::file_of(&path)).unwrap();
    svc.keep_replication_in(replication::file_of(&path));
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(server::serve_grpc(Arc::new(svc), l, Access::default()));
    url
}

/// A coordinator over one shard, served over HTTP.
async fn coordinator(shard: String) -> (Arc<Service>, String) {
    let c = Arc::new(Coordinator::connect(vec![shard]).await.unwrap());
    let schema = c.schema.clone();
    let svc = Arc::new(Service::new(Backend::Cluster(c), schema, agent(), 16, Duration::from_secs(10)));
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(server::serve_http(svc.clone(), l, Access::default()));
    (svc, url)
}

async fn sql(svc: &Arc<Service>, s: &str) {
    if let Err(e) = svc.sql(&Principal::open(), s, &Params::new(), After::Any, 0).await {
        panic!("{s}: {}", e.message);
    }
}

/// The count `q` answers on `svc` (-1 while it fails: a table not made yet).
async fn count(svc: &Arc<Service>, q: &str) -> i64 {
    match svc.query(QueryReq { sql: q.into(), ..Default::default() }).await {
        Ok(a) => chloedb_server::json::result(&a.rs, 0.0)["rows"][0][0].as_i64().unwrap_or(-1),
        Err(_) => -1,
    }
}

/// Waits for `done`, failing if the follow process `f` ends meanwhile.
async fn until(what: &str, f: &tokio::task::JoinHandle<Result<(), chloedb_server::ApiError>>, done: impl Fn() -> futures::future::BoxFuture<'static, bool>) {
    let t0 = Instant::now();
    while !done().await {
        assert!(!f.is_finished(), "{what}: the follow process ended");
        assert!(t0.elapsed() < Duration::from_secs(60), "{what}: not within 60 s");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// A proxy to `target` that passes every request on, but answers 503 to
/// the first `/v1/sql` whose statement starts with one of `lost` once the
/// target took it: an answer lost after the change was made.
async fn lossy(target: String, lost: &'static [&'static str]) -> (String, Arc<AtomicUsize>) {
    let dropped = Arc::new(AtomicUsize::new(0));
    let seen = Arc::new(std::sync::Mutex::new(Vec::<&'static str>::new()));
    let client = chloedb_server::http_client().build().unwrap();
    let d = dropped.clone();
    let handler = move |method: Method, uri: Uri, headers: axum::http::HeaderMap, body: Bytes| {
        let (client, target, d, seen) = (client.clone(), target.clone(), d.clone(), seen.clone());
        async move {
            let url = format!("{target}{}", uri.path_and_query().map_or("/", |p| p.as_str()));
            let mut r = client.request(method, url).body(body.clone());
            for (k, v) in headers.iter().filter(|(k, _)| *k != "host" && *k != "content-length") {
                r = r.header(k, v);
            }
            let answer = r.send().await.unwrap();
            let status = answer.status();
            let text = answer.bytes().await.unwrap();
            if uri.path() == "/v1/sql" && status.is_success() {
                let stmt = serde_json::from_slice::<Json>(&body).ok().and_then(|j| j["sql"].as_str().map(str::to_ascii_uppercase)).unwrap_or_default();
                let mut seen = seen.lock().unwrap();
                if let Some(kind) = lost.iter().find(|k| stmt.starts_with(**k) && !seen.contains(k)) {
                    seen.push(kind);
                    d.fetch_add(1, Ordering::Relaxed);
                    return (StatusCode::SERVICE_UNAVAILABLE, axum::Json(json!({"error": "the answer was lost on the way"}))).into_response();
                }
            }
            (StatusCode::from_u16(status.as_u16()).unwrap(), [("content-type", "application/json")], text).into_response()
        }
    };
    let app = axum::Router::new().fallback(handler);
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await });
    (url, dropped)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_change_of_tables_applied_and_sent_again_does_not_stop_the_follower() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let (svc_a, url_a) = coordinator(shard(d, "s0.strata", 20).await).await;
    let (svc_b, url_b) = coordinator(shard(d, "t0.strata", 0).await).await;
    // The source makes a table, and the follower's first answer to
    // making it, and to changing it later, is lost.
    sql(&svc_a, "CREATE TABLE events (id INT PRIMARY KEY, n INT)").await;
    sql(&svc_a, "INSERT INTO events (id, n) VALUES (1, 1), (2, 2)").await;
    let (proxy, dropped) = lossy(url_b.clone(), &["CREATE TABLE", "ALTER TABLE"]).await;
    let o = Options { source: url_a, source_key: None, source_ca: None, target: proxy, target_token: None, target_ca: None, name: "east".into(), batch: 40, copy: false, reverse: false, once: None };
    let f = tokio::spawn(follow::run(o));
    until("the table copied", &f, || {
        let b = svc_b.clone();
        Box::pin(async move { b.catalog().iter().any(|t| t.table == "events") && count(&b, "SELECT COUNT(*) FROM events").await == 2 })
    })
    .await;
    // Changed on the source as it is followed: the change, sent again, too.
    sql(&svc_a, "ALTER TABLE events ADD COLUMN note TEXT").await;
    sql(&svc_a, "INSERT INTO events (id, n, note) VALUES (3, 3, 'after')").await;
    until("the change followed", &f, || {
        let b = svc_b.clone();
        Box::pin(async move { b.catalog().iter().any(|t| t.table == "events" && t.field_index("note").is_some()) && count(&b, "SELECT COUNT(*) FROM events WHERE note = 'after'").await == 1 })
    })
    .await;
    assert_eq!(dropped.load(Ordering::Relaxed), 2, "both answers were lost on the way");
    f.abort();
}
