//! The Experiments app against a real chloedb-server: a site of its own
//! tables added through the API, a fast and a served experiment run in the
//! background to their reports, a look at the arms, a cut by two
//! attributes, the token, and a delete that drops the served run's log.
//!
//! It needs the chloedb-server binary, as tests/served.rs does.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use chloedb_experiments::app::{self, Opts};
use chloedb_experiments::server::Server;

fn server_bin() -> Option<PathBuf> {
    if let Some(b) = std::env::var_os("CHLOEDB_SERVER_BIN") {
        return Some(PathBuf::from(b));
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../chloedb-server/target");
    ["ci", "release"].iter().map(|p| root.join(p).join("chloedb-server")).find(|p| p.exists())
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

const WORDS: [&str; 16] = ["tent", "lamp", "boot", "cord", "mug", "kettle", "chair", "rope", "map", "stove", "pack", "knife", "torch", "flask", "sock", "hat"];

struct Client {
    url: String,
    http: reqwest::Client,
}

impl Client {
    async fn call(&self, method: reqwest::Method, path: &str, body: Option<Value>, token: Option<&str>) -> (u16, Value) {
        let mut r = self.http.request(method, format!("{}{path}", self.url));
        if let Some(t) = token {
            r = r.bearer_auth(t);
        }
        if let Some(b) = body {
            r = r.json(&b);
        }
        let resp = r.send().await.unwrap();
        let status = resp.status().as_u16();
        (status, resp.json().await.unwrap_or(Value::Null))
    }
    async fn get(&self, path: &str) -> Value {
        let (s, v) = self.call(reqwest::Method::GET, path, None, Some("t")).await;
        assert_eq!(s, 200, "GET {path}: {v}");
        v
    }
    async fn post(&self, path: &str, body: Value) -> Value {
        let (s, v) = self.call(reqwest::Method::POST, path, Some(body), Some("t")).await;
        assert_eq!(s, 200, "POST {path}: {v}");
        v
    }
    async fn done(&self, id: &str) -> Value {
        let t0 = Instant::now();
        loop {
            let r = self.get(&format!("/v1/experiments/{id}")).await;
            match r["status"].as_str() {
                Some("done") => return r,
                Some("failed") => panic!("experiment {id} failed: {}", r["error"]),
                _ => {}
            }
            assert!(t0.elapsed() < Duration::from_secs(300), "experiment {id} did not end: {r}");
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_app_runs_experiments_on_a_site_of_its_own_tables() {
    let Some(bin) = server_bin() else {
        assert!(std::env::var_os("CHLOEDB_EXPERIMENTS_NEED_SERVER").is_none(), "no chloedb-server binary (CHLOEDB_SERVER_BIN)");
        eprintln!("skipped: no chloedb-server binary (build chloedb-server, or set CHLOEDB_SERVER_BIN)");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let port = free_port();
    let child = Command::new(&bin)
        .args(["--db", &dir.path().join("x.strata").to_string_lossy(), "--listen", &format!("127.0.0.1:{port}"), "--grpc", "off"])
        .env("CHLOEDB_START_APPS", "off")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("chloedb-server");
    let _guard = Running(child);
    let server = Server::new(&format!("http://127.0.0.1:{port}"), None);
    let t0 = Instant::now();
    while server.health().await.is_err() {
        assert!(t0.elapsed() < Duration::from_secs(60), "chloedb-server did not start");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // A shop's site as three tables: rows, queries with their traffic, and
    // grades (by how many of a query's words a row's title holds).
    server.sql("CREATE TABLE shop (id KEYWORD PRIMARY KEY, title TEXT, body TEXT)", json!({})).await.unwrap();
    server.sql("CREATE TABLE shop_queries (id KEYWORD PRIMARY KEY, text TEXT, weight FLOAT)", json!({})).await.unwrap();
    server.sql("CREATE TABLE shop_grades (id KEYWORD PRIMARY KEY, query KEYWORD, doc KEYWORD, grade INT)", json!({})).await.unwrap();
    let rows: Vec<(String, String, String)> = (0..600).map(|i| (format!("d{i}"), format!("{} {}", WORDS[i % 16], WORDS[(i * 7 + 3) % 16]), format!("{} {} {}", WORDS[(i * 5 + 1) % 13], WORDS[(i * 3 + 2) % 16], WORDS[i % 16]))).collect();
    server.write(rows.iter().map(|(id, t, b)| json!({"op": "upsert", "table": "shop", "doc": {"id": id, "title": t, "body": b}})).collect()).await.unwrap();
    let queries: Vec<(String, String)> = (0..30).map(|q| (format!("q{q}"), format!("{} {}", WORDS[q % 16], WORDS[(q * 3 + 5) % 16]))).collect();
    server.write(queries.iter().enumerate().map(|(q, (id, text))| json!({"op": "upsert", "table": "shop_queries", "doc": {"id": id, "text": text, "weight": 1.0 / (q + 1) as f64}})).collect()).await.unwrap();
    let mut grades = Vec::new();
    for (qid, text) in &queries {
        for (id, title, _) in &rows {
            let g = text.split(' ').filter(|w| title.split(' ').any(|t| t == *w)).count();
            if g > 0 {
                grades.push(json!({"op": "upsert", "table": "shop_grades", "doc": {"id": format!("{qid}/{id}"), "query": qid, "doc": id, "grade": g}}));
            }
        }
    }
    server.write(grades).await.unwrap();

    // The app, on a port of its own, with a token.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(app::serve(server.clone(), listener, Opts { token: Some("t".into()), beir: Vec::new() }));
    let c = Client { url, http: reqwest::Client::new() };
    let t0 = Instant::now();
    while c.http.get(format!("{}/healthz", c.url)).send().await.is_err() {
        assert!(t0.elapsed() < Duration::from_secs(10));
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    // Its tables are made in the background.
    let t0 = Instant::now();
    while c.call(reqwest::Method::GET, "/v1/sites", None, Some("t")).await.0 != 200 {
        assert!(t0.elapsed() < Duration::from_secs(30), "the app's tables");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // The token is needed everywhere but /healthz; the manifest names the view.
    assert_eq!(c.call(reqwest::Method::GET, "/v1/experiments", None, None).await.0, 401);
    assert_eq!(c.call(reqwest::Method::GET, "/v1/experiments", None, Some("wrong")).await.0, 401);
    let m = c.get("/app.json").await;
    assert_eq!(m["name"], "experiments");
    assert_eq!(m["view"], "ui/view.js");
    let view = c.http.get(format!("{}/ui/view.js", c.url)).bearer_auth("t").send().await.unwrap();
    assert!(view.text().await.unwrap().contains("export default"));
    let meta = c.get("/v1/meta").await;
    assert_eq!(meta["defaults"]["model"], "dbn");
    assert_eq!(meta["metrics"].as_array().unwrap().len(), 8);

    let title = "bm25(title, :t, syntax => 'plain')";
    let body = "bm25(body, :t, syntax => 'plain')";
    let site = json!({"name": "shop", "table": "shop", "queries": "shop_queries", "judgments": "shop_grades", "about": "a test shop", "rankings": [["Body", body], ["Title", title]]});
    let added = c.post("/v1/sites", site).await;
    assert_eq!(added["queries"], 30);
    let (s, v) = c.call(reqwest::Method::POST, "/v1/sites", Some(json!({"name": "x; DROP", "table": "shop", "queries": "shop_queries", "judgments": "shop_grades"})), Some("t")).await;
    assert_eq!(s, 400, "{v}");
    let sites = c.get("/v1/sites").await;
    assert_eq!(sites["sites"][0]["name"], "shop");
    let one = c.get("/v1/sites/shop").await;
    assert!(one["judged"].as_u64().unwrap() > 100, "{one}");

    // What each arm shows for the busiest query, row by row with grades.
    let pv = c.post("/v1/preview", json!({"site": "shop", "ranks": [body, title], "limit": 5, "queries": 2})).await;
    let q = &pv["queries"][0];
    assert_eq!(q["id"], "q0");
    assert_eq!(q["lists"].as_array().unwrap().len(), 2);
    assert!(q["lists"][1]["rows"][0]["grade"].as_u64().unwrap() >= 1, "title ranks a judged row first: {q}");

    // A spec the app refuses: one arm.
    let (s, v) = c.call(reqwest::Method::POST, "/v1/experiments", Some(json!({"name": "x", "site": "shop", "arms": [{"name": "A", "rank": body}]})), Some("t")).await;
    assert_eq!(s, 400, "{v}");

    // Fast: title against body, which the grades favour.
    let spec = json!({"name": "title vs body", "site": "shop", "arms": [{"name": "Body", "rank": body}, {"name": "Title", "rank": title}], "users": 3000, "impressions": 60000, "explore_share": 0.15});
    let id = c.post("/v1/experiments", spec).await["id"].as_str().unwrap().to_string();
    let r = c.done(&id).await;
    let rep = &r["report"];
    assert_eq!(rep["srm"]["ok"], true, "{}", rep["srm"]);
    let success = rep["metrics"].as_array().unwrap().iter().find(|m| m["key"] == "success").unwrap();
    let cmp = &success["comparisons"][0];
    assert!(cmp["true_delta"].as_f64().unwrap() > 0.0, "the title ranking is truly better here: {cmp}");
    assert!(cmp["p"].as_f64().unwrap() < 0.05, "and the experiment sees it: {cmp}");
    assert!(cmp["cuped"]["variance_reduction"].as_f64().unwrap() > 0.0, "{cmp}");
    assert_eq!(cmp["series"].as_array().unwrap().len(), 20);
    assert_eq!(rep["verdicts"][0]["call"], "ship", "{}", rep["verdicts"]);
    assert_eq!(rep["cohorts"]["device"]["metrics"]["success"][0]["cells"].as_array().unwrap().len(), 3);
    let wi = &rep["whatif"];
    assert!(wi["searches"].as_u64().unwrap() > 5000, "{wi}");
    assert_eq!(wi["arms"].as_array().unwrap().len(), 2);
    assert_eq!(r["summary"]["verdicts"][0]["call"], "ship");

    // The list, and a cut by two attributes of the run kept in memory.
    let list = c.get("/v1/experiments").await;
    assert_eq!(list["experiments"][0]["id"], id.as_str());
    let cut = c.post(&format!("/v1/experiments/{id}/cut"), json!({"by": ["age", "device"]})).await;
    let cells = cut["metrics"]["success"][0]["cells"].as_array().unwrap();
    assert!(cells.len() > 10 && cells[0]["cell"].as_str().unwrap().contains(" · "), "{cut}");
    let (s, _) = c.call(reqwest::Method::POST, &format!("/v1/experiments/{id}/cut"), Some(json!({"by": ["shoe size"]})), Some("t")).await;
    assert_eq!(s, 400);

    // Served: each search a logged query, the exploring share's with its
    // policy, and each arm's searches timed one by one.
    let spec = json!({"name": "served", "site": "shop", "arms": [{"name": "Body", "rank": body}, {"name": "Title", "rank": title}], "users": 500, "impressions": 3000, "mode": "served", "pre": 0.2, "explore_share": 0.2});
    let sid = c.post("/v1/experiments", spec).await["id"].as_str().unwrap().to_string();
    let r = c.done(&sid).await;
    let rep = &r["report"];
    let n: u64 = rep["latency"].as_array().unwrap().iter().map(|l| l["n"].as_u64().unwrap()).sum();
    assert!((2000..3000).contains(&n), "the arms' searches, timed one by one: {n}");
    let logged = server.query(&format!("SELECT COUNT(*) FROM exp_log_{sid}"), json!({}), None).await.unwrap()["rows"][0][0].as_u64().unwrap();
    assert!(logged >= 3000, "every search is in the rank log, a row per row shown: {logged}");
    let explored = server.query(&format!("SELECT COUNT(*) FROM exp_log_{sid} WHERE variant = 'explore'"), json!({}), None).await.unwrap()["rows"][0][0].as_u64().unwrap();
    assert!(explored > 0, "the exploring share is logged as its variant");

    // Delete: the row, and the served run's log and events.
    let (s, _) = c.call(reqwest::Method::DELETE, &format!("/v1/experiments/{sid}"), None, Some("t")).await;
    assert_eq!(s, 200);
    let (s, _) = c.call(reqwest::Method::GET, &format!("/v1/experiments/{sid}"), None, Some("t")).await;
    assert_eq!(s, 404);
    assert!(server.query(&format!("SELECT COUNT(*) FROM exp_log_{sid}"), json!({}), None).await.is_err(), "its log is dropped");
}
