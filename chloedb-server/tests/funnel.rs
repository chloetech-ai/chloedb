//! A query's funnel over HTTP ([`chloedb_server::funnel`]): `POST
//! /v1/funnel` runs a search and says the time of each stage from its text
//! to its rows (its understood words, its terms, its plan, its match,
//! selection and ranking), the rows each stage kept and dropped, and what
//! its words became; `GET /v1/queries/funnel` lays the time of the queries
//! the server ran on the same stages, and none of the funnel's own runs is
//! among them. Through a coordinator over two shards, the rows' funnel is
//! one database's.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use chloedb::db::Db;
use chloedb::query::Params;
use chloedb::storage::DbOptions;
use chloedb_server::auth::Access;
use chloedb_server::coord::Coordinator;
use chloedb_server::funnel::{self, Ask};
use chloedb_server::keys::Principal;
use chloedb_server::service::{After, AgentConfig, Backend, Service};
use chloedb_server::server;
use serde_json::{json, Value as Json};
use tokio::net::TcpListener;

fn agent() -> AgentConfig {
    AgentConfig { api_key: None, model: chloedb_server::agent::DEFAULT_MODEL.into(), base_url: "http://unused".into() }
}

fn empty(dir: &Path, name: &str) -> Arc<Db> {
    Arc::new(Db::create_empty(&dir.join(name), DbOptions { sync: false, segment_max_docs: 8, ..DbOptions::default() }).unwrap())
}

fn local(db: Arc<Db>) -> Arc<Service> {
    let schema = (*db.schema()).clone();
    Arc::new(Service::new(Backend::Local(db), schema, agent(), 16, Duration::from_secs(20)))
}

/// Twenty items, as chloedb's own funnel test has them: five whose titles
/// hold "red" or "chair", one of them too dear and one of a brand the
/// searches leave out.
fn statements() -> Vec<String> {
    let mut rows = vec![
        "(1, 'red chair', 'acme', 50)".to_string(),
        "(2, 'red sofa chair', 'acme', 150)".to_string(),
        "(3, 'blue chair', 'bolt', 40)".to_string(),
        "(4, 'red table', 'acme', 30)".to_string(),
        "(5, 'green lamp', 'bolt', 20)".to_string(),
        "(6, 'red chair deluxe', 'zeta', 80)".to_string(),
    ];
    for i in 7..=20 {
        rows.push(format!("({i}, 'oak shelf {i}', 'bolt', 10)"));
    }
    vec!["CREATE TABLE items (id INT PRIMARY KEY, title TEXT, brand KEYWORD, price FLOAT)".to_string(), format!("INSERT INTO items (id, title, brand, price) VALUES {}", rows.join(", "))]
}

async fn run_all(svc: &Arc<Service>) {
    for s in statements() {
        svc.sql(&Principal::open(), &s, &Params::new(), After::Any, 0).await.unwrap_or_else(|e| panic!("{s}: {}", e.message));
    }
}

async fn serve_http(svc: Arc<Service>) -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
    tokio::spawn(server::serve_http(svc, l, Access::default()));
    base
}

async fn serve_grpc(db: Arc<Db>) -> String {
    let svc = local(db);
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(server::serve_grpc(svc, l, Access::default()));
    url
}

async fn call(base: &str, method: &str, path: &str, body: Option<Json>) -> (u16, Json) {
    let c = chloedb_server::http_client().build().unwrap();
    let mut r = c.request(method.parse().unwrap(), format!("{base}{path}"));
    if let Some(b) = body {
        r = r.json(&b);
    }
    let r = r.send().await.unwrap();
    let status = r.status().as_u16();
    let text = r.text().await.unwrap();
    (status, serde_json::from_str(&text).unwrap_or(Json::String(text)))
}

/// A stage's milliseconds in a funnel's `stages`.
fn stage_ms(j: &Json, stage: &str) -> f64 {
    j["stages"].as_array().unwrap().iter().find(|s| s["stage"] == stage).unwrap_or_else(|| panic!("no stage {stage}: {j}"))["ms"].as_f64().unwrap()
}

/// Each row's fate in a funnel's rows, by key.
fn fates(rows: &Json) -> Vec<(i64, String)> {
    let mut out: Vec<(i64, String)> = rows["rows"].as_array().unwrap().iter().map(|r| (r["key"].as_i64().unwrap(), r["fate"].as_str().unwrap().to_string())).collect();
    out.sort();
    out
}

const SEARCH: &str = "SELECT id FROM items WHERE price < 100 ORDER BY bm25(title, :t, fuzzy => 1) DESC LIMIT 1";
/// A search for "did you mean": a word in many rows, typed wrong.
const TYPO: &str = "SELECT id FROM items ORDER BY bm25(title, :t) DESC LIMIT 3";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_search_s_funnel_says_each_stage_s_time_and_rows() {
    let dir = tempfile::tempdir().unwrap();
    let svc = local(empty(dir.path(), "one.chloedb"));
    run_all(&svc).await;
    let base = serve_http(svc.clone()).await;

    let (st, j) = call(&base, "POST", "/v1/funnel", Some(json!({"sql": SEARCH, "params": {"t": "red chiar"}}))).await;
    assert_eq!(st, 200, "{j}");
    assert_eq!(j["kind"], "search", "{j}");
    // Every stage, its time summing to the whole.
    let stages = j["stages"].as_array().unwrap();
    assert_eq!(stages.len(), 13);
    let sum: f64 = stages.iter().map(|s| s["ms"].as_f64().unwrap()).sum();
    let took = j["took_ms"].as_f64().unwrap();
    assert!((sum - took).abs() < 0.05 + took * 0.01, "{sum} vs {took}");
    assert!(stage_ms(&j, "plan") > 0.0 && stage_ms(&j, "match") > 0.0, "{}", j["stages"]);
    assert!(j["steps"].as_array().is_some_and(|s| s.iter().any(|s| s["op"] == "terms")), "{}", j["steps"]);
    // The rows: the page, and what WHERE and the page took.
    let rows = &j["rows"];
    assert_eq!((rows["live"].as_u64(), rows["selected"].as_u64(), rows["shown"].as_u64()), (Some(20), Some(4), Some(1)), "{rows}");
    let f = fates(rows);
    assert!(f.contains(&(1, "shown".into())) && f.contains(&(2, "filtered".into())) && f.contains(&(3, "cut".into())), "{f:?}");
    let r2 = rows["rows"].as_array().unwrap().iter().find(|r| r["key"] == 2).unwrap();
    assert!(r2["why"].as_str().unwrap().contains("price < 100"), "{r2}");
    // The words: "chiar" in no title, "chair" its typo's term, and what
    // "did you mean" says.
    let search = &j["text"]["searches"][0];
    assert_eq!((search["field"].as_str(), search["text"].as_str()), (Some("title"), Some("red chiar")), "{}", j["text"]);
    let terms = search["terms"].as_array().unwrap();
    let term = |t: &str| terms.iter().find(|x| x["term"] == t).unwrap_or_else(|| panic!("no term {t}: {terms:?}"));
    assert_eq!((term("chiar")["kind"].as_str(), term("chiar")["df"].as_u64()), (Some("word"), Some(0)));
    assert_eq!((term("chair")["kind"].as_str(), term("chair")["of"].as_str(), term("chair")["df"].as_u64()), (Some("typo"), Some("chiar"), Some(4)));
    // "chair" is in 4 rows of 20: too few for "did you mean" to take
    // "chiar" for its typo (it wants a neighbour in 5).
    assert_eq!(search["corrected"]["changed"], false, "{search}");
    assert_eq!(j["answer"]["rows"], json!([[1]]));
    // "shelf" is in 14: "shelg" is its typo.
    let (st, s) = call(&base, "POST", "/v1/funnel", Some(json!({"sql": TYPO, "params": {"t": "oak shelg"}}))).await;
    assert_eq!(st, 200, "{s}");
    assert_eq!(s["text"]["searches"][0]["corrected"]["text"], "oak shelf", "{}", s["text"]);

    // Understood words: the brand the words name becomes a condition, a
    // stage of its own, and the query that ran says it.
    let understood = "SELECT id FROM items ORDER BY bm25(title, 'acme chair', understand => 'filter') DESC LIMIT 3";
    let (st, u) = call(&base, "POST", "/v1/funnel", Some(json!({"sql": understood}))).await;
    assert_eq!(st, 200, "{u}");
    assert!(u["text"]["understood"].as_array().is_some_and(|l| !l.is_empty()), "{}", u["text"]);
    assert!(stage_ms(&u, "understand") > 0.0, "{}", u["stages"]);
    assert_ne!(u["ran"], u["sql"]);
    assert!(fates(&u["rows"]).iter().all(|(k, f)| f != "shown" || [1, 2].contains(k)), "acme's chairs: {:?}", fates(&u["rows"]));

    // A query that is not a search: its time, and the rows its order cut.
    let (st, o) = call(&base, "POST", "/v1/funnel", Some(json!({"sql": "SELECT id FROM items WHERE brand = 'acme' ORDER BY price LIMIT 2"}))).await;
    assert_eq!(st, 200, "{o}");
    assert_eq!(o["kind"], "rows");
    assert_eq!(fates(&o["rows"]), vec![(1, "shown".into()), (2, "cut".into()), (4, "shown".into())]);
    let (st, bad) = call(&base, "POST", "/v1/funnel", Some(json!({"sql": "SELECT FROM"}))).await;
    assert_eq!(st, 400, "{bad}");

    // The server's queries on the stages: these, and none of the funnel's.
    for t in ["red chair", "blue", "oak shelf"] {
        let (st, a) = call(&base, "POST", "/v1/query", Some(json!({"sql": SEARCH, "params": {"t": t}}))).await;
        assert_eq!(st, 200, "{a}");
    }
    let (st, m) = call(&base, "GET", "/v1/queries/funnel", None).await;
    assert_eq!(st, 200, "{m}");
    assert_eq!(m["queries"], 3, "{m}");
    let stages = m["stages"].as_array().unwrap();
    let matched = stages.iter().find(|s| s["stage"] == "match").unwrap();
    assert_eq!(matched["ran"], 3, "{matched}");
    assert!(m["stage_rows"]["match"]["mean"].as_f64().is_some(), "{}", m["stage_rows"]);
    assert_eq!(m["statements"].as_array().unwrap().len(), 1, "{}", m["statements"]);
    assert_eq!(m["statements"][0]["queries"], 3);
    // Its own funnel of rows: what the matching found, down to the one row
    // each answered.
    let rows = &m["statements"][0]["rows"];
    assert!(rows["match"]["mean"].as_f64().is_some_and(|n| n >= 1.0), "{rows}");
    assert_eq!(rows["answered"]["mean"].as_f64(), Some(1.0), "{rows}");
    let slowest = m["slowest"].as_array().unwrap();
    assert_eq!(slowest.len(), 3);
    assert!(slowest.iter().all(|q| !q["sql"].as_str().unwrap().contains("funnel")), "{slowest:?}");
    // One of them, by its number.
    let id = slowest[0]["id"].as_u64().unwrap();
    let (st, one) = call(&base, "GET", &format!("/v1/queries/funnel?id={id}"), None).await;
    assert_eq!(st, 200, "{one}");
    assert_eq!(one["query"]["id"], id);
    assert!(stage_ms(&one, "match") > 0.0, "{}", one["stages"]);
    let (st, _) = call(&base, "GET", "/v1/queries/funnel?id=999999", None).await;
    assert_eq!(st, 404);
    // Filtered to a table no query read.
    let (_, none) = call(&base, "GET", "/v1/queries/funnel?table=nothing", None).await;
    assert_eq!(none["queries"], 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn through_a_coordinator_the_rows_funnel_is_one_database_s() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let urls = vec![serve_grpc(empty(d, "s0.chloedb")).await, serve_grpc(empty(d, "s1.chloedb")).await];
    let c = Arc::new(Coordinator::connect(urls).await.unwrap());
    let schema = c.schema.clone();
    let coord = Arc::new(Service::new(Backend::Cluster(c), schema, agent(), 16, Duration::from_secs(20)));
    run_all(&coord).await;
    let one = local(empty(d, "one.chloedb"));
    run_all(&one).await;
    for (sql, t) in [(SEARCH, Some("red chiar")), ("SELECT id FROM items WHERE price < 100 AND brand <> 'zeta' ORDER BY bm25(title, 'red chair') DESC LIMIT 1", None), ("SELECT id FROM items WHERE brand = 'acme' ORDER BY price LIMIT 2", None)] {
        let params = match t {
            Some(t) => Params::new().set("t", chloedb::value::Value::Str(t.into())),
            None => Params::new(),
        };
        let ask = || Ask { sql: sql.into(), params: params.clone(), timeout_ms: 0, budget: Default::default() };
        let want = funnel::one(&one, &Principal::open(), ask()).await.unwrap_or_else(|e| panic!("{sql}: {}", e.message));
        let got = funnel::one(&coord, &Principal::open(), ask()).await.unwrap_or_else(|e| panic!("{sql}: {}", e.message));
        for k in ["kind", "live", "matched", "selected", "shown", "ranked"] {
            assert_eq!(got["rows"][k], want["rows"][k], "{sql}: {k}: {}", got["rows"]);
        }
        assert_eq!(fates(&got["rows"]), fates(&want["rows"]), "{sql}");
        assert_eq!(got["answer"]["rows"], want["answer"]["rows"], "{sql}");
        // The coordinator's time: its calls to the shards, and the shards'
        // own steps on their stages.
        assert!(stage_ms(&got, "shards") > 0.0, "{sql}: {}", got["stages"]);
        if sql.contains("bm25") {
            assert!(stage_ms(&got, "match") > 0.0, "{sql}: {}", got["stages"]);
        }
    }
    // The words' terms through the coordinator: summed over the shards.
    let ask = Ask { sql: SEARCH.into(), params: Params::new().set("t", chloedb::value::Value::Str("red chiar".into())), timeout_ms: 0, budget: Default::default() };
    let got = funnel::one(&coord, &Principal::open(), ask).await.unwrap();
    let terms = got["text"]["searches"][0]["terms"].as_array().unwrap().clone();
    assert!(terms.iter().any(|t| t["term"] == "chair" && t["df"] == 4 && t["kind"] == "typo"), "{terms:?}");
    let ask = Ask { sql: TYPO.into(), params: Params::new().set("t", chloedb::value::Value::Str("oak shelg".into())), timeout_ms: 0, budget: Default::default() };
    let got = funnel::one(&coord, &Principal::open(), ask).await.unwrap();
    assert_eq!(got["text"]["searches"][0]["corrected"]["text"], "oak shelf", "{}", got["text"]);
}
