//! The command center's Simulator (src/traffic): a test run through a
//! coordinator of two shards loads its table, measures each kind alone,
//! sends the mix at rising rates, and leaves a report with a grade, the
//! model fitted to it, advice, and predictions of other shapes; its table
//! is dropped afterwards. A run on a single server predicts clusters too.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value as Json};
use strata::db::Db;
use strata::storage::DbOptions;
use strata_server::auth::Access;
use strata_server::coord::Coordinator;
use strata_server::service::{AgentConfig, Backend, Service};
use strata_server::traffic::run::{self, Config};
use strata_server::traffic::data::TABLE;
use strata_server::{demo, server};
use tokio::net::TcpListener;

fn agent() -> AgentConfig {
    AgentConfig { api_key: None, model: strata_server::agent::DEFAULT_MODEL.into(), base_url: "http://unused".into() }
}

fn create(dir: &Path, name: &str) -> Arc<Db> {
    Arc::new(Db::create(&dir.join(name), demo::schema(10), DbOptions::default()).unwrap())
}

fn local(db: Arc<Db>) -> Arc<Service> {
    let schema = (*db.schema()).clone();
    Arc::new(Service::new(Backend::Local(db), schema, agent(), 16, Duration::from_secs(10)))
}

async fn serve(db: Arc<Db>) -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(server::serve_grpc(local(db), l, Access::default()));
    url
}

/// A small run: a couple of thousand documents, short steps, a target.
fn small() -> Config {
    serde_json::from_value(json!({
        "profile": "search-app", "docs": 2000, "dim": 16, "alone_s": 0.5, "step_s": 1.0, "max_steps": 4,
        "start_qps": 20.0, "factor": 2.0, "target_qps": 30.0, "label": "small",
    }))
    .unwrap()
}

/// Waits for the run to end; answers its report.
async fn finished(svc: &Arc<Service>) -> Json {
    let t0 = Instant::now();
    loop {
        let v = run::view(svc);
        let state = v["run"]["state"].as_str().unwrap_or("").to_string();
        if state != "running" {
            assert_eq!(state, "done", "the run ended {state}: {}", v["run"]);
            let id = v["reports"][0]["id"].as_str().unwrap().to_string();
            return run::report(svc, &id).unwrap();
        }
        assert!(t0.elapsed() < Duration::from_secs(240), "the run took too long: {}", v["run"]);
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

fn check_report(r: &Json) {
    assert_eq!(r["state"], "done");
    assert!(r["steps"].as_array().is_some_and(|s| !s.is_empty()), "{}", r["steps"]);
    let kinds = r["kinds"].as_array().unwrap();
    assert!(kinds.len() >= 8, "{kinds:?}");
    assert!(kinds.iter().all(|k| k["requests"].as_u64().unwrap() > 0 && k["errors"] == 0), "{kinds:?}");
    let g = &r["grade"];
    assert!(g["score"].as_i64().is_some_and(|s| (0..=100).contains(&s)));
    assert!(g["label"].is_string() && g["headline"].is_string());
    let checks = g["checks"].as_array().unwrap();
    for id in ["capacity", "latency", "bottleneck", "errors", "cost"] {
        assert!(checks.iter().any(|c| c["id"] == id), "no {id} check: {checks:?}");
    }
    let advice = r["advice"].as_array().unwrap();
    for id in ["cheapest", "copy_more", "machine_up"] {
        assert!(advice.iter().any(|a| a["id"] == id), "no {id} advice: {advice:?}");
    }
    let cap = r["model"]["base"]["prediction"]["capacity_qps"].as_f64().unwrap();
    assert!(cap > 0.0 && cap.is_finite(), "{}", r["model"]["base"]);
    assert!(r["model"]["base"]["cost_month"].as_f64().unwrap() > 0.0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_test_run_through_a_coordinator_grades_and_predicts() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let urls = vec![serve(create(d, "s0.strata")).await, serve(create(d, "s1.strata")).await];
    let c = Arc::new(Coordinator::connect(urls).await.unwrap());
    let schema = c.schema.clone();
    let svc = Arc::new(Service::new(Backend::Cluster(c), schema, agent(), 16, Duration::from_secs(10)));

    let v = run::view(&svc);
    assert_eq!(v["coordinator"], true);
    assert!(v["presets"].as_array().unwrap().len() >= 3);
    assert!(v["kinds"].as_array().unwrap().len() >= 14);

    // Settings that do not hold are refused before anything runs.
    let bad: Config = serde_json::from_value(json!({"mix": {"no such kind": 1.0}})).unwrap();
    assert_eq!(run::start(&svc, "test", bad).unwrap_err().status, 400);

    run::start(&svc, "test", small()).unwrap();
    // One run at a time.
    assert_eq!(run::start(&svc, "test", small()).unwrap_err().status, 409);
    let r = finished(&svc).await;
    check_report(&r);
    assert_eq!(r["by"], "test");
    assert_eq!(r["cluster"]["shards"], 2);
    // Every search asked both shards; a key lookup one.
    let calls = |k: &str| r["kinds"].as_array().unwrap().iter().find(|x| x["kind"] == k).unwrap()["calls"].as_f64().unwrap();
    assert!((calls("bm25") - 2.0).abs() < 0.01, "{}", calls("bm25"));
    assert!((calls("key lookup") - 1.0).abs() < 0.01, "{}", calls("key lookup"));
    // The test table is gone.
    assert!(!svc.catalog_now().await.iter().any(|t| t.table == TABLE));

    // Other shapes, by the model: a copy more holds no less, at any shards.
    let p = run::predict(&svc, &json!({"shards": [1, 2, 4], "copies": [1, 2], "qps": 30.0})).unwrap();
    let cells = p["cells"].as_array().unwrap();
    assert_eq!(cells.len(), 6);
    let cap = |s: u64, r: u64| cells.iter().find(|c| c["shards"] == s && c["copies"] == r).unwrap()["capacity_qps"].as_f64().unwrap();
    for s in [1, 2, 4] {
        assert!(cap(s, 2) >= cap(s, 1) * 0.999, "{s} shards: {} copies 2 vs {} copies 1", cap(s, 2), cap(s, 1));
    }
    assert!(cells.iter().all(|c| c["cost_month"].as_f64().unwrap() > 0.0));
    let one = run::predict(&svc, &json!({"shape": {"shards": 3, "copies": 2}, "machine": "c-16"})).unwrap();
    assert_eq!(one["machine"]["name"], "c-16");
    assert!(one["detail"]["curve"].as_array().is_some_and(|c| !c.is_empty()));
    assert!(run::predict(&svc, &json!({"run": "nope"})).is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_test_run_on_one_server_predicts_clusters() {
    let dir = tempfile::tempdir().unwrap();
    let svc = local(create(dir.path(), "one.strata"));
    let mut c = small();
    c.keep = true;
    run::start(&svc, "test", c).unwrap();
    let r = finished(&svc).await;
    check_report(&r);
    assert_eq!(r["cluster"]["local"], true);
    // Kept, then dropped by hand.
    assert!(svc.catalog_now().await.iter().any(|t| t.table == TABLE));
    assert_eq!(run::drop_table(&svc).await.unwrap()["dropped"], true);
    assert!(!svc.catalog_now().await.iter().any(|t| t.table == TABLE));
    // A cluster of it, by the model.
    let p = run::predict(&svc, &json!({"shards": [2], "copies": [2]})).unwrap();
    assert!(p["cells"][0]["capacity_qps"].as_f64().unwrap() > 0.0);
    assert_eq!(p["cells"][0]["coordinators"], 1);
}
