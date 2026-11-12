//! One coordinator at a time changes the cluster map on Kubernetes, and
//! the map it leaves in doubt outlives it (finding E-2): the Lease of the
//! map ([`chloedb_server::operator::lease`]) against a stand-in for the API
//! server that keeps one object and checks its `resourceVersion` as the
//! API server does, and coordinators that take it.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::State;
use axum::http::{Method, StatusCode};
use axum::response::IntoResponse;
use chloedb::db::Db;
use chloedb::storage::DbOptions;
use chloedb_server::auth::Access;
use chloedb_server::coord::Coordinator;
use chloedb_server::operator::kube::{Kube, Options};
use chloedb_server::operator::lease::{MapLease, TERM};
use chloedb_server::replication::Acks;
use chloedb_server::service::{AgentConfig, Backend, Service};
use chloedb_server::{demo, server, slots};
use serde_json::{json, Value as Json};
use tokio::net::TcpListener;

/// The Lease the stand-in keeps, its version, the calls it was made, and
/// whether it refuses every call as forbidden.
#[derive(Default)]
struct Api {
    lease: Option<Json>,
    version: u64,
    calls: Vec<String>,
    forbidden: bool,
}

async fn api(State(a): State<Arc<Mutex<Api>>>, method: Method, body: axum::body::Bytes) -> axum::response::Response {
    let mut a = a.lock().unwrap();
    a.calls.push(method.to_string());
    if a.forbidden {
        return (StatusCode::FORBIDDEN, axum::Json(json!({"message": "leases is forbidden"}))).into_response();
    }
    let body: Json = serde_json::from_slice(&body).unwrap_or(Json::Null);
    let at = |a: &Api| body["metadata"]["resourceVersion"].as_str() == Some(a.version.to_string().as_str());
    match method {
        Method::GET => match &a.lease {
            Some(l) => axum::Json(l.clone()).into_response(),
            None => (StatusCode::NOT_FOUND, axum::Json(json!({"message": "not found"}))).into_response(),
        },
        Method::POST if a.lease.is_some() => (StatusCode::CONFLICT, axum::Json(json!({"message": "already exists"}))).into_response(),
        Method::PUT if a.lease.is_none() || !at(&a) => (StatusCode::CONFLICT, axum::Json(json!({"message": "the object has been modified"}))).into_response(),
        Method::POST | Method::PUT => {
            a.version += 1;
            let mut l = body.clone();
            l["metadata"]["resourceVersion"] = json!(a.version.to_string());
            a.lease = Some(l.clone());
            axum::Json(l).into_response()
        }
        _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
    }
}

async fn api_server() -> (Arc<Mutex<Api>>, Kube) {
    let a = Arc::new(Mutex::new(Api::default()));
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let app = axum::Router::new().fallback(api).with_state(a.clone());
    tokio::spawn(async move { axum::serve(l, app).await });
    let kube = Kube::new(&Options { api: Some(url), token: Some("t".into()), ..Options::default() }).unwrap();
    (a, kube)
}

fn holder(a: &Arc<Mutex<Api>>) -> (String, String) {
    let a = a.lock().unwrap();
    let l = a.lease.as_ref().unwrap();
    (l["spec"]["holderIdentity"].as_str().unwrap().to_string(), l["metadata"]["annotations"]["strata.chloetech.ai/map-in-doubt"].as_str().unwrap().to_string())
}

#[tokio::test]
async fn one_coordinator_holds_the_maps_lease_and_the_next_takes_its_map_in_doubt() {
    let (a, kube) = api_server().await;
    let lease = |me: &str| MapLease::new(kube.clone(), "shop", "orders", me);
    let (one, two) = (lease("coord-a"), lease("coord-b"));
    let map = slots::SlotMap::even(2, 5);
    let t0 = 1_700_000_000_000u64;

    // The first takes it (made then); the second waits for it.
    assert_eq!(one.try_take(t0).await.unwrap(), Ok(None));
    let by = two.try_take(t0).await.unwrap().unwrap_err();
    assert_eq!(by.holder, "coord-a");
    assert_eq!(by.left_ms, TERM.as_millis() as u64);
    // Its clock runs 100 ms a call, so its wait ends after a few tries.
    let tick = std::sync::atomic::AtomicU64::new(t0);
    let e = two.take(Duration::from_millis(300), &|| tick.fetch_add(100, std::sync::atomic::Ordering::Relaxed)).await.unwrap_err();
    assert!(e.status == 503 && e.message.contains("coordinator coord-a is changing the cluster map"), "{}", e.message);

    // It keeps the map it is about to give, then stops before it ends
    // its change: the second takes the Lease once its term is over, with
    // that map in doubt.
    one.keep(Some(&map), t0 + 1_000).await.unwrap();
    assert_eq!(holder(&a), ("coord-a".into(), map.to_json().to_string()));
    assert!(two.try_take(t0 + 1_000 + TERM.as_millis() as u64 - 1).await.unwrap().is_err());
    let later = t0 + 2_000 + TERM.as_millis() as u64;
    assert_eq!(two.try_take(later).await.unwrap(), Ok(Some(map.clone())));
    // The first, which did not see that, keeps it no more, and gives no map.
    let e = one.keep(Some(&map), later).await.unwrap_err();
    assert!(e.status == 503 && e.message.contains("another coordinator took it"), "{}", e.message);
    // Started again in its pod (its name), it waits for the second.
    let again = lease("coord-a");
    assert!(again.try_take(later).await.unwrap().is_err());
    // The second's change ends with no map in doubt: free for any.
    two.release(None, later + 10).await.unwrap();
    assert_eq!(holder(&a), (String::new(), String::new()));
    assert_eq!(again.try_take(later + 20).await.unwrap(), Ok(None));
    again.release(Some(&map), later + 30).await.unwrap();
    assert_eq!(lease("coord-c").try_take(later + 40).await.unwrap(), Ok(Some(map)));

    // An account that may not use Leases (a Role from an older operator):
    // the map is changed without one.
    a.lock().unwrap().forbidden = true;
    let old = lease("coord-d");
    assert_eq!(old.try_take(later).await.unwrap(), Ok(None));
    old.keep(None, later).await.unwrap();
    old.release(None, later).await.unwrap();
}

fn agent() -> AgentConfig {
    AgentConfig { api_key: None, model: chloedb_server::agent::DEFAULT_MODEL.into(), base_url: "http://unused".into() }
}

/// A shard server of the demo products `keep` keeps, at `dir/name`.
async fn serve(dir: &Path, name: &str, keep: impl Fn(usize) -> bool) -> String {
    let opts = DbOptions { publish_snapshots: true, ship_log_bytes: 16 << 20, ..DbOptions::default() };
    let path = dir.join(name);
    let db = Db::create(&path, demo::schema(50), opts).unwrap();
    db.set_embedder("emb", Arc::new(chloedb::embed::HashEmbedder { dim: demo::DIM })).unwrap();
    let docs: Vec<_> = (0..50).filter(|&i| keep(i)).map(|i| demo::product(i, 7)).collect();
    if !docs.is_empty() {
        db.load(docs).unwrap();
    }
    db.storage().start_seq_no().unwrap();
    let schema = (*db.schema()).clone();
    let mut svc = Service::new(Backend::Local(Arc::new(db)), schema, agent(), 4, Duration::from_secs(10));
    svc.keep_slots_in(slots::file_of(&path)).unwrap();
    let svc = Arc::new(svc);
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(server::serve_grpc(svc, l, Access::default()));
    url
}

/// The fork of finding E-2, as the Lease ends it: a coordinator gave a
/// map and stopped before any server answered (a restart, an upgrade); the
/// operator's next change reaches another coordinator, which never saw
/// that map. It finds it in the Lease, gives it again first, and makes its
/// own change from it, one epoch on, rather than a second map of the same
/// epoch.
#[tokio::test(flavor = "multi_thread")]
async fn a_change_on_another_coordinator_gives_the_map_left_in_doubt_first() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let (a, kube) = api_server().await;
    let s0 = serve(d, "s0.strata", |_| true).await;
    let s1 = serve(d, "s1.strata", |_| false).await;
    let s2 = serve(d, "s2.strata", |_| false).await;

    let first = Coordinator::connect(vec![s0.clone()]).await.unwrap();
    first.lease_map_changes(MapLease::new(kube.clone(), "shop", "orders", "coord-a"));
    first.add_shard(vec![s1.clone()], Acks::All).await.unwrap();
    let made = first.map().unwrap();
    // It took the Lease, kept the map before it gave it, and let the Lease
    // go with nothing in doubt.
    assert_eq!(holder(&a), (String::new(), String::new()));
    let calls = a.lock().unwrap().calls.clone();
    assert!(calls.len() >= 4 && calls[..2] == ["GET", "POST"] && calls[2..].iter().all(|c| c == "PUT"), "{calls:?}");

    // The map it gave next, and stopped: in the Lease, held by it still.
    let gone = made.set_servers(0, vec![s0.clone()]).unwrap();
    assert_eq!(gone.epoch, made.epoch + 1);
    {
        let mut api = a.lock().unwrap();
        let l = api.lease.as_mut().unwrap();
        l["spec"]["holderIdentity"] = json!("coord-a");
        l["metadata"]["annotations"]["strata.chloetech.ai/map-in-doubt"] = json!(gone.to_json().to_string());
        l["metadata"]["annotations"]["strata.chloetech.ai/kept-ms"] = json!("0");
    }

    // Another coordinator, which never saw it, adds a shard.
    let second = Coordinator::connect(vec![s0.clone(), s1.clone()]).await.unwrap();
    second.lease_map_changes(MapLease::new(kube.clone(), "shop", "orders", "coord-b"));
    let j = second.add_shard(vec![s2.clone()], Acks::All).await.unwrap();
    let now = second.map().unwrap();
    assert_eq!(now.epoch, gone.epoch + 1, "its change is made from the map in doubt: {j}");
    assert_eq!(now.replicas(), [vec![s0.clone()], vec![s1.clone()], vec![s2.clone()]]);
    assert_eq!(holder(&a), (String::new(), String::new()));
}
