//! Blends logged and valued off-policy on a server
//! ([`chloedb::query::blend_eval`]): `FROM blend(...)` with `"log"` and
//! `"explore"` through `/v1/query`, its chances in the answer and the
//! log; clicks on the pages; `POST /v1/blends/:name/evaluate` and
//! `/tune` over the log. One database and a coordinator over three
//! shards draw the same pages, log the same rows and value the blends
//! alike.

use std::sync::Arc;
use std::time::Duration;

use chloedb::db::Db;
use chloedb::query::Params;
use chloedb::storage::DbOptions;
use chloedb_server::auth::Access;
use chloedb_server::coord::Coordinator;
use chloedb_server::keys::Principal;
use chloedb_server::service::{After, AgentConfig, Backend, Service};
use chloedb_server::{demo, server};
use rand::{Rng, SeedableRng};
use serde_json::{json, Value as Json};
use tokio::net::TcpListener;

fn agent() -> AgentConfig {
    AgentConfig { api_key: None, model: chloedb_server::agent::DEFAULT_MODEL.into(), base_url: "http://unused".into() }
}

fn local(dir: &std::path::Path, name: &str) -> Arc<Service> {
    let db = Arc::new(Db::create(&dir.join(name), demo::schema(10), DbOptions { segment_max_docs: 200, ..DbOptions::default() }).unwrap());
    let schema = (*db.schema()).clone();
    Arc::new(Service::new(Backend::Local(db), schema, agent(), 16, Duration::from_secs(20)))
}

async fn grpc(svc: Arc<Service>) -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(server::serve_grpc(svc, l, Access::default()));
    url
}

async fn http(svc: Arc<Service>) -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(server::serve_http(svc, l, Access::default()));
    url
}

async fn post(base: &str, path: &str, body: Json) -> (u16, Json) {
    let r = chloedb_server::http_client().build().unwrap().post(format!("{base}{path}")).json(&body).send().await.unwrap();
    let status = r.status().as_u16();
    let text = r.text().await.unwrap();
    (status, serde_json::from_str(&text).unwrap_or(Json::String(text)))
}

async fn run(svc: &Arc<Service>, s: &str) {
    svc.sql(&Principal::open(), s, &Params::new(), After::Any, 0).await.unwrap_or_else(|e| panic!("{s}: {}", e.message));
}

const GROUPS: usize = 16;
const PER: usize = 10;

/// Sources ordered by a whole number, so that a coordinator merges the
/// very rows one database reads.
fn blend(name: &str, engagement: f64) -> String {
    format!(
        "CREATE BLEND {name} AS
  SOURCES (
    news   AS (SELECT id, kind, author, rel AS relevance, pop AS engagement FROM docs WHERE grp = :g AND kind = 'news' ORDER BY rel DESC LIMIT 10),
    videos AS (SELECT id, kind, author, rel AS relevance, pop AS engagement FROM docs WHERE grp = :g AND kind = 'video' ORDER BY rel DESC LIMIT 10)
  )
  HEADS (relevance CALIBRATE minmax, engagement CALIBRATE minmax)
  UTILITY 1 * relevance + {engagement} * engagement
  QUOTA (kind = 'video' AT LEAST 25% IN TOP 6)
  MAX 3 PER author
  WITH (k => 6)"
    )
}

/// The docs, each group's relevances and engagements a permutation, and
/// each row's chance of a click once looked at.
fn world() -> (Vec<String>, std::collections::HashMap<String, f64>) {
    let mut rng = rand::rngs::StdRng::seed_from_u64(5);
    let mut rows = Vec::new();
    let mut appeal = std::collections::HashMap::new();
    for g in 0..GROUPS {
        let mut rel: Vec<usize> = (0..2 * PER).collect();
        let mut pop: Vec<usize> = (0..2 * PER).collect();
        for i in (1..rel.len()).rev() {
            rel.swap(i, rng.gen_range(0..=i));
            pop.swap(i, rng.gen_range(0..=i));
        }
        for (j, kind) in ["news", "video"].iter().enumerate() {
            for i in 0..PER {
                let (r, p) = (rel[j * PER + i], pop[j * PER + i]);
                let id = format!("g{g}-{kind}-{i}");
                appeal.insert(id.clone(), 0.05 + 0.5 * p as f64 / (2 * PER) as f64);
                rows.push(format!("('{id}', 'g{g}', '{kind}', 'a{}', {r}, {p})", rng.gen_range(0..5)));
            }
        }
    }
    (rows, appeal)
}

const T0: i64 = 1_700_000_000_000_000;

fn look(l: usize) -> f64 {
    1.0 / (l as f64).powf(0.8)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn blends_log_their_chances_and_are_valued_off_policy_on_one_database_and_three_shards() {
    let dir = tempfile::tempdir().unwrap();
    let whole = local(dir.path(), "whole.strata");
    let mut urls = Vec::new();
    for s in 0..3 {
        urls.push(grpc(local(dir.path(), &format!("s{s}.strata"))).await);
    }
    let c = Coordinator::connect(urls).await.unwrap();
    let schema = c.schema.clone();
    let coord = Arc::new(Service::new(Backend::Cluster(Arc::new(c)), schema, agent(), 16, Duration::from_secs(20)));
    let (docs, appeal) = world();
    for svc in [&whole, &coord] {
        run(svc, "CREATE TABLE docs (id KEYWORD PRIMARY KEY, grp KEYWORD, kind KEYWORD, author KEYWORD, rel INT, pop INT)").await;
        run(svc, "CREATE TABLE clicks (id KEYWORD PRIMARY KEY, usr KEYWORD, item KEYWORD, ts TIMESTAMP)").await;
        run(svc, &format!("INSERT INTO docs VALUES {}", docs.join(", "))).await;
        run(svc, &blend("feed", 0.3)).await;
        run(svc, &blend("engaging", 0.9)).await;
    }
    let (hw, hc) = (http(whole.clone()).await, http(coord.clone()).await);

    // Exploring searches, logged: the same page and chances from both.
    let n = 240;
    let explore = json!({"policy": "softmax", "tau": 0.4, "top": 5});
    let mut rng = rand::rngs::StdRng::seed_from_u64(9);
    let mut clicks = Vec::new();
    for i in 0..n {
        let g = rng.gen_range(0..GROUPS);
        let request = format!("r{i:04}");
        let ts = T0 + i as i64 * 10_000_000;
        let body = json!({"sql": "SELECT id, slot, source FROM blend('feed', g => :g)", "params": {"g": format!("g{g}")}, "log": {"request": request, "context": request, "ts": ts, "explore": explore}});
        let mut answers = Vec::new();
        for base in [&hw, &hc] {
            let (status, a) = post(base, "/v1/query", body.clone()).await;
            assert_eq!(status, 200, "{a}");
            assert_eq!(a["request"], json!(request));
            let p = a["propensities"].as_array().unwrap_or_else(|| panic!("an exploring blend answers each slot's chance: {a}"));
            assert_eq!(p.len(), a["rows"].as_array().unwrap().len());
            assert!(p.iter().all(|x| x.as_f64().is_some_and(|x| x > 0.0 && x <= 1.0)), "{a}");
            assert!(a["logged"]["rows"].as_u64().unwrap() >= 20, "every row of both sources is logged: {a}");
            answers.push((a["rows"].clone(), a["propensities"].clone()));
        }
        assert_eq!(answers[0], answers[1], "request {request}: three shards draw what one database draws");
        for (l, row) in answers[0].0.as_array().unwrap().iter().enumerate() {
            let key = row[0].as_str().unwrap();
            if rng.gen::<f64>() < look(l + 1) * appeal[key] {
                clicks.push(format!("('{request}-{l}', '{request}', '{key}', '{}')", chloedb::timestamp::format(ts + 1_000_000)));
            }
        }
    }
    assert!(clicks.len() > n / 4, "people clicked: {}", clicks.len());
    for svc in [&whole, &coord] {
        run(svc, &format!("INSERT INTO clicks VALUES {}", clicks.join(", "))).await;
    }

    // The log valued: the logging blend as it explored gets back every
    // chance (every weight 1), and the others are valued alike by both.
    let export = json!({"table": "rank_log", "events": {"table": "clicks", "user": "usr", "item": "item", "time": "ts"}, "window": "1 minute"});
    let targets = json!(["feed", "engaging@1", {"name": "feed", "label": "as logged", "explore": {"tau": 0.4, "top": 5}}]);
    let mut got = Vec::new();
    for base in [&hw, &hc] {
        let (status, a) = post(base, "/v1/blends/feed/evaluate", json!({"export": export, "targets": targets})).await;
        assert_eq!(status, 200, "{a}");
        assert_eq!(a["gathered"]["requests"].as_u64(), Some(n as u64), "{a}");
        let est = a["estimates"].as_array().unwrap();
        assert_eq!(est.iter().map(|e| e["label"].as_str().unwrap()).collect::<Vec<_>>(), ["feed@1", "engaging@1", "as logged"]);
        for s in est[2]["slots"].as_array().unwrap() {
            assert!((s["ess"].as_f64().unwrap() - n as f64).abs() < 1e-3, "the logged chances come back: {s}");
        }
        assert!((est[2]["snips"]["value"].as_f64().unwrap() - est[2]["logged"]["value"].as_f64().unwrap()).abs() < 1e-6, "{}", est[2]);
        for e in est {
            assert!(e["cdr"]["value"].as_f64().is_some_and(f64::is_finite), "{e}");
            assert!(["better", "worse", "same", "not proven"].contains(&e["verdict"].as_str().unwrap()), "{e}");
        }
        assert_eq!(est[1]["against"], json!("feed@1"), "{}", est[1]);
        got.push(est.iter().map(|e| (e["cdr"]["value"].as_f64().unwrap(), e["snips"]["value"].as_f64().unwrap())).collect::<Vec<_>>());
    }
    for (a, b) in got[0].iter().zip(&got[1]) {
        assert!((a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6, "one database and three shards value alike: {got:?}");
    }

    // Tuned on half the log, proven on the other half.
    let (status, t) = post(&hw, "/v1/blends/feed/tune", json!({"export": export, "rounds": 4, "seed": 3})).await;
    assert_eq!(status, 200, "{t}");
    assert_eq!(t["tried"].as_array().unwrap().len(), 5, "the start and four near it: {t}");
    assert!(t["best"]["sql"].as_str().is_some_and(|s| s.starts_with("CREATE")), "{t}");
    assert!(t["best"]["estimate"]["cdr"]["value"].as_f64().is_some(), "{t}");
    assert!(t["start"]["cdr"]["value"].as_f64().is_some(), "{t}");

    // What the routes refuse: no events, a blend that is not one, a
    // target that is not.
    let (status, e) = post(&hw, "/v1/blends/feed/evaluate", json!({"export": {"table": "rank_log"}})).await;
    assert_eq!(status, 400, "{e}");
    assert!(e["error"].as_str().unwrap().contains("events"), "{e}");
    let (status, e) = post(&hw, "/v1/blends/feed/evaluate", json!({"export": export, "targets": ["nope"]})).await;
    assert_eq!(status, 400, "{e}");
    let (status, e) = post(&hw, "/v1/blends/feed/evaluate", json!({"export": export, "targets": [3]})).await;
    assert_eq!(status, 400, "{e}");
    // A blend that logs nothing has nothing to value.
    let (status, a) = post(&hw, "/v1/blends/engaging/evaluate", json!({"export": export})).await;
    assert_eq!(status, 200, "{a}");
    assert_eq!((a["gathered"]["requests"].as_u64(), a["gathered"]["other"].as_u64()), (Some(0), Some(n as u64)), "{a}");
    // A blend's log does not interleave or take candidates.
    let (status, e) = post(&hw, "/v1/query", json!({"sql": "SELECT id FROM blend('feed', g => 'g1')", "log": {"candidates": 5}})).await;
    assert_eq!(status, 400, "{e}");
    let (status, e) = post(&hw, "/v1/query", json!({"sql": "SELECT id FROM blend('feed', g => 'g1')", "log": {"explore": {"policy": "shuffle", "top": 4}}})).await;
    assert_eq!(status, 400, "{e}");
    assert!(e["error"].as_str().unwrap().contains("softmax"), "{e}");
}
