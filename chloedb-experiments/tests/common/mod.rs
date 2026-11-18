//! What the app's tests share: a chloedb-server of their own, a shop's
//! site as three tables, and a client of the app.
#![allow(dead_code)]

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use chloedb_experiments::server::Server;

pub fn server_bin() -> Option<PathBuf> {
    if let Some(b) = std::env::var_os("CHLOEDB_SERVER_BIN") {
        return Some(PathBuf::from(b));
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../chloedb-server/target");
    ["ci", "release"].iter().map(|p| root.join(p).join("chloedb-server")).find(|p| p.exists())
}

pub fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

pub struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub const WORDS: [&str; 16] = ["tent", "lamp", "boot", "cord", "mug", "kettle", "chair", "rope", "map", "stove", "pack", "knife", "torch", "flask", "sock", "hat"];

pub struct Client {
    pub url: String,
    pub http: reqwest::Client,
}

impl Client {
    pub async fn call(&self, method: reqwest::Method, path: &str, body: Option<Value>, token: Option<&str>) -> (u16, Value) {
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
    pub async fn get(&self, path: &str) -> Value {
        let (s, v) = self.call(reqwest::Method::GET, path, None, Some("t")).await;
        assert_eq!(s, 200, "GET {path}: {v}");
        v
    }
    pub async fn post(&self, path: &str, body: Value) -> Value {
        let (s, v) = self.call(reqwest::Method::POST, path, Some(body), Some("t")).await;
        assert_eq!(s, 200, "POST {path}: {v}");
        v
    }
    pub async fn done(&self, id: &str) -> Value {
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

/// A chloedb-server of its own, with the shop's tables: rows, queries
/// with their traffic, and grades (by how many of a query's words a row's
/// title holds). None when there is no chloedb-server binary.
pub async fn shop_server() -> Option<(Server, Running, tempfile::TempDir)> {
    let Some(bin) = server_bin() else {
        assert!(std::env::var_os("CHLOEDB_EXPERIMENTS_NEED_SERVER").is_none(), "no chloedb-server binary (CHLOEDB_SERVER_BIN)");
        eprintln!("skipped: no chloedb-server binary (build chloedb-server, or set CHLOEDB_SERVER_BIN)");
        return None;
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
    let guard = Running(child);
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

    Some((server, guard, dir))
}

/// The app's site of the shop's tables.
pub fn shop_site() -> Value {
    json!({"name": "shop", "table": "shop", "queries": "shop_queries", "judgments": "shop_grades", "about": "a test shop", "rankings": [["Body", BODY], ["Title", TITLE]]})
}

pub const TITLE: &str = "bm25(title, :t, syntax => 'plain')";
pub const BODY: &str = "bm25(body, :t, syntax => 'plain')";

/// Waits for the app at `c` to have made its tables.
pub async fn ready(c: &Client) {
    let t0 = Instant::now();
    while c.http.get(format!("{}/healthz", c.url)).send().await.is_err() {
        assert!(t0.elapsed() < Duration::from_secs(10));
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let t0 = Instant::now();
    while c.call(reqwest::Method::GET, "/v1/sites", None, Some("t")).await.0 != 200 {
        assert!(t0.elapsed() < Duration::from_secs(30), "the app's tables");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
