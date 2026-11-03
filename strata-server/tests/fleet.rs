//! The coordinators of one cluster know each other (fleet): each finds the
//! others through the shards they share (`GET /v1/coordinators`); Running
//! now lists, and cancels, the queries of every coordinator; a Contain
//! rule set on one holds on all, stays across a restart and reaches a
//! coordinator that starts later; and fleet health counts every
//! coordinator's searches.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value as Json};
use strata::db::Db;
use strata::storage::DbOptions;
use strata_server::coord::Coordinator;
use strata_server::service::{AgentConfig, Backend, Service};
use strata_server::telemetry::{Caller, Meta};
use strata_server::{demo, peers, server};
use tokio::net::TcpListener;

const N: usize = 400;
/// A query that runs a while (see `tests/workload.rs`).
const JOIN: &str = "SELECT x.id, y.id FROM products x JOIN products y ON x.category = y.category AND x.price < y.price - 1000000";

fn agent() -> AgentConfig {
    AgentConfig { api_key: None, model: String::new(), base_url: String::new() }
}

/// A shard server of half the demo products, over gRPC.
async fn shard(dir: &Path, i: usize) -> String {
    let db = Db::create(&dir.join(format!("s{i}.strata")), demo::schema(N), DbOptions { publish_snapshots: true, ..DbOptions::default() }).unwrap();
    db.set_embedder("emb", Arc::new(strata::embed::HashEmbedder { dim: demo::DIM })).unwrap();
    db.load((0..N).filter(|k| k % 2 == i).map(|k| demo::product(k, 7))).unwrap();
    let db = Arc::new(db);
    let schema = (*db.schema()).clone();
    let svc = Arc::new(Service::new(Backend::Local(db), schema, agent(), 16, Duration::from_secs(10)));
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(server::serve_grpc(svc, l, Default::default()));
    url
}

/// A coordinator of `shards` named `name`, over HTTP, that keeps its
/// Contain rules in `contain`.
struct Coord {
    c: Arc<Coordinator>,
    svc: Arc<Service>,
    base: String,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
}

async fn coordinator(shards: &[String], name: &str, contain: &Path) -> Coord {
    let c = Arc::new(Coordinator::connect(shards.to_vec()).await.unwrap());
    let schema = c.schema.clone();
    let mut svc = Service::new(Backend::Cluster(c.clone()), schema, agent(), 16, Duration::from_secs(10));
    svc.place.name = Some(name.into());
    svc.contain = strata_server::contain::Contain::open(contain.to_path_buf()).unwrap();
    let svc = Arc::new(svc);
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    peers::start(&svc, base.clone());
    let task = tokio::spawn(server::serve_http(svc.clone(), l, Default::default()));
    Coord { c, svc, base, task }
}

impl Coord {
    /// Surveys the shards now (announcing itself) and reads what they heard.
    async fn look(&self) {
        let _ = self.c.snapshot_within(Duration::ZERO).await;
        peers::refresh(&self.svc).await;
    }

    fn names(&self) -> Vec<String> {
        peers::coordinators(&self.svc).into_iter().map(|p| p.name).collect()
    }
}

async fn call(method: reqwest::Method, base: &str, path: &str, body: Option<Json>) -> (u16, Json) {
    let mut rq = strata_server::http_client().build().unwrap().request(method, format!("{base}{path}")).header("x-strata-reason", "a runaway join");
    if let Some(b) = body {
        rq = rq.json(&b);
    }
    let r = rq.send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or(Json::Null))
}

async fn get(base: &str, path: &str) -> Json {
    let (s, j) = call(reqwest::Method::GET, base, path, None).await;
    assert_eq!(s, 200, "{path}: {j}");
    j
}

async fn until<F: std::future::Future<Output = bool>>(what: &str, f: impl Fn() -> F) {
    let t0 = Instant::now();
    while !f().await {
        assert!(t0.elapsed() < Duration::from_secs(40), "waited 40 s for: {what}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn encode(s: &str) -> String {
    peers::encode(s)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn coordinators_of_one_cluster_know_and_cover_each_other() {
    let dir = tempfile::tempdir().unwrap();
    let shards = vec![shard(dir.path(), 0).await, shard(dir.path(), 1).await];
    let a = coordinator(&shards, "coord-a", &dir.path().join("a.json")).await;
    let b = coordinator(&shards, "coord-b", &dir.path().join("b.json")).await;

    // ----- each finds the other through the shards
    until("each coordinator knows the other", || async {
        a.look().await;
        b.look().await;
        a.look().await;
        a.names().len() == 2 && b.names().len() == 2
    })
    .await;
    let j = get(&a.base, "/v1/coordinators").await;
    assert_eq!(j["server"], "coordinator");
    let cs = j["coordinators"].as_array().unwrap();
    assert_eq!(cs.len(), 2, "{j}");
    let me = cs.iter().find(|c| c["this"] == true).unwrap();
    assert_eq!((me["name"].as_str(), me["url"].as_str()), (Some("coord-a"), Some(a.base.as_str())));
    let other = cs.iter().find(|c| c["this"] == false).unwrap();
    assert_eq!((other["name"].as_str(), other["url"].as_str()), (Some("coord-b"), Some(b.base.as_str())));
    assert!(other["version"].is_string() && other["last_seen_ms"].as_u64().unwrap() > 0, "{other}");
    // The shell's brief cluster: which copies answer, without each copy's detail.
    let full = get(&a.base, "/v1/cluster").await;
    let brief = get(&a.base, "/v1/cluster?brief=1").await;
    assert!(full["shards"][0]["replicas"][0]["detail"].is_object(), "{full}");
    assert!(brief["shards"][0]["replicas"][0].get("detail").is_none() && brief["shards"][0]["replicas"][0]["status"] == full["shards"][0]["replicas"][0]["status"], "{brief}");
    assert!(brief.to_string().len() * 2 < full.to_string().len());
    // What a coordinator tells the others of its searches.
    let w = get(&a.base, "/v1/coordinators?window=1").await;
    assert!(w["window"]["window"]["queries"].is_number() && w["window"]["recent_s"] == 60, "{w}");

    // ----- Running now: every coordinator's queries, and a cancel on one stops them on all
    let of = |key: &str| Meta { caller: Some(Arc::new(Caller { key: Some(key.into()), ..Default::default() })), ..Default::default() };
    let on_b: Vec<_> = (0..2).map(|_| b.svc.live.start(JOIN, "sql", false, &of("reports-batch"))).collect();
    let on_a = a.svc.live.start(JOIN, "sql", false, &of("reports-batch"));
    let live = get(&a.base, "/v1/queries").await;
    let qs = live["queries"].as_array().unwrap();
    assert_eq!(qs.len(), 3, "{live}");
    assert_eq!(qs.iter().filter(|q| q["node"] == "coord-b" && q["coordinator_url"] == b.base.as_str()).count(), 2, "{live}");
    assert_eq!(live["nodes"].as_array().unwrap().iter().filter(|n| n["ok"] == true).count(), 2, "{live}");
    let g = get(&a.base, "/v1/queries?group=shape").await;
    let group = &g["groups"][0];
    assert_eq!(group["count"], 3, "{g}");
    let on: Vec<(&str, u64)> = group["coordinators"].as_array().unwrap().iter().map(|c| (c["name"].as_str().unwrap(), c["count"].as_u64().unwrap())).collect();
    assert_eq!(on, [("coord-a", 1), ("coord-b", 2)], "{g}");
    // What B alone runs, asked as a coordinator asks: B's only.
    assert_eq!(get(&b.base, "/v1/queries?scope=local").await["queries"].as_array().unwrap().len(), 2);
    let fp = strata_server::telemetry::fingerprint(JOIN);
    let (s, j) = call(reqwest::Method::DELETE, &a.base, &format!("/v1/queries?fingerprint={}&key=reports-batch", encode(&fp)), None).await;
    assert_eq!((s, j["cancelled"].as_u64(), j["here"].as_u64()), (200, Some(3), Some(1)), "{j}");
    for l in on_b.iter().chain([&on_a]) {
        tokio::time::timeout(Duration::from_secs(2), l.query.cancelled()).await.expect("cancelled on its coordinator");
    }
    // One query of another coordinator, by the id Running now gives it
    // (`<id>@<coordinator>`), and by its id and coordinator's URL; a query
    // of A with B's id is not touched.
    let one = b.svc.live.start(JOIN, "sql", false, &of("ops"));
    let mine = a.svc.live.start(JOIN, "sql", false, &of("ops"));
    let live = get(&a.base, "/v1/queries").await;
    let listed = live["queries"].as_array().unwrap().iter().find(|q| q["node"] == "coord-b" && q["key"] == "ops").cloned().unwrap_or_else(|| panic!("{live}"));
    assert_eq!(listed["id"], format!("{}@coord-b", one.query.id), "{listed}");
    let (s, j) = call(reqwest::Method::DELETE, &a.base, &format!("/v1/queries/{}", encode(listed["id"].as_str().unwrap())), None).await;
    assert_eq!(s, 200, "{j}");
    tokio::time::timeout(Duration::from_secs(2), one.query.cancelled()).await.expect("cancelled on B");
    assert!(tokio::time::timeout(Duration::from_millis(300), mine.query.cancelled()).await.is_err(), "A's own query runs on");
    drop(mine);
    let one = b.svc.live.start(JOIN, "sql", false, &of("ops"));
    let id = one.query.id;
    let (s, j) = call(reqwest::Method::DELETE, &a.base, &format!("/v1/queries/{id}?coordinator={}", encode(&b.base)), None).await;
    assert_eq!(s, 200, "{j}");
    tokio::time::timeout(Duration::from_secs(2), one.query.cancelled()).await.expect("cancelled on B");
    let (s, _) = call(reqwest::Method::DELETE, &a.base, &format!("/v1/queries/{id}?coordinator={}", encode("http://169.254.169.254")), None).await;
    assert_eq!(s, 404, "a coordinator it does not know: not called");
    drop((on_b, on_a, one));

    // ----- Contain: set on A, held on B with the same id
    let rule = json!({"match": {"fingerprint": fp, "key": "reports-batch"}, "action": "refuse", "reason": "a runaway join", "expires_s": 600});
    let (s, r) = call(reqwest::Method::POST, &a.base, "/v1/admin/contain", Some(rule)).await;
    assert_eq!(s, 200, "{r}");
    let id = r["id"].as_str().unwrap().to_string();
    let reached = r["coordinators"].as_array().unwrap();
    assert!(reached.len() == 2 && reached.iter().all(|c| c["ok"] == true), "{r}");
    assert!(b.svc.contain.list().iter().any(|x| x.id == id), "B holds A's rule");
    let all = get(&b.base, "/v1/admin/contain").await;
    let held = all.as_array().unwrap().iter().find(|x| x["id"] == id.as_str()).unwrap().clone();
    assert_eq!(held["on"], json!(["coord-b", "coord-a"]), "{held}");
    assert_eq!(held["missing"], json!([]));
    // B refuses the statement too.
    let (s, j) = call(reqwest::Method::GET, &b.base, "/v1/admin/contain?local=1", None).await;
    assert_eq!((s, j["rules"].as_array().map(Vec::len)), (200, Some(1)), "{j}");

    // ----- B restarts: the rule is still there, from its file
    b.task.abort();
    let b_url = b.base.clone();
    drop(b);
    let b2 = coordinator(&shards, "coord-b", &dir.path().join("b.json")).await;
    assert!(b2.svc.contain.list().iter().any(|x| x.id == id), "kept across the restart");
    assert_ne!(b2.base, b_url);

    // ----- C starts later and takes the rule up from the others
    let c = coordinator(&shards, "coord-c", &dir.path().join("c.json")).await;
    until("C knows A and takes up its rules", || async {
        a.look().await;
        c.look().await;
        strata_server::contain::sync(&c.svc).await;
        c.svc.contain.list().iter().any(|x| x.id == id)
    })
    .await;

    // ----- removed on A: gone on every coordinator, and not brought back
    until("A knows the restarted B and C", || async {
        b2.look().await;
        c.look().await;
        a.look().await;
        let urls: Vec<String> = peers::coordinators(&a.svc).into_iter().map(|p| p.url).collect();
        urls.contains(&b2.base) && urls.contains(&c.base)
    })
    .await;
    let (s, j) = call(reqwest::Method::DELETE, &a.base, &format!("/v1/admin/contain/{id}"), None).await;
    assert_eq!(s, 200, "{j}");
    assert!(!c.svc.contain.list().iter().any(|x| x.id == id), "removed on C");
    assert!(!b2.svc.contain.list().iter().any(|x| x.id == id), "removed on B");
    assert_eq!(get(&a.base, "/v1/admin/contain").await, json!([]), "on no coordinator");

    // ----- fleet health counts B's searches on A, and A's inspector B's reads (INS-3)
    let docs = |j: &Json| j["cluster"]["reads"]["docs"]["total"].as_u64().unwrap_or(0);
    let before = docs(&get(&a.base, "/v1/index?table=products").await);
    for _ in 0..10 {
        let (s, _) = call(reqwest::Method::POST, &b2.base, "/v1/query", Some(json!({"sql": "SELECT id, title FROM products LIMIT 5"}))).await;
        assert_eq!(s, 200);
    }
    b2.svc.series.add(b2.svc.sample());
    b2.svc.series.add(b2.svc.sample());
    until("A has B's searches", || async {
        let f = get(&a.base, "/v1/health/fleet").await;
        f["coordinators"].as_array().unwrap().iter().any(|c| c["url"] == b2.base.as_str() && c["qps"].as_f64().is_some_and(|q| q > 0.0))
    })
    .await;
    let ix = get(&a.base, "/v1/index?table=products").await;
    assert!(docs(&ix) >= before + 10, "B's reads counted on A: {} then {}", before, docs(&ix));
    assert!(ix["reads_from"].as_array().unwrap().iter().filter(|c| c["ok"] == true).count() >= 2, "{}", ix["reads_from"]);
    let f = get(&a.base, "/v1/health/fleet").await;
    let er = f["checks"].as_array().unwrap().iter().find(|c| c["id"] == "error_ratio").unwrap().clone();
    assert!(er["queries"].as_f64().unwrap() >= 10.0 && er["coordinators"].as_u64().unwrap() >= 2, "{er}");
}
