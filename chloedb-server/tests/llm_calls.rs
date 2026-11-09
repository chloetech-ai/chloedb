//! `llm()` on a server and through a coordinator ([`chloedb::query::llm`]):
//! a stored function that asks a language model, called through the
//! query and SQL paths; a coordinator asks about the merged rows only, on
//! itself, and answers as one database does, in its columns and in an
//! ORDER BY over a first ranking's top rows; and the server's metrics
//! count the requests. Against a mock of Anthropic's API on loopback,
//! named by an `endpoint.json` in the models directory.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener as StdListener;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value as J};
use chloedb::db::Db;
use chloedb::query::Params;
use chloedb::storage::DbOptions;
use chloedb::value::Value;
use chloedb_server::auth::Access;
use chloedb_server::coord::Coordinator;
use chloedb_server::keys::{Principal, Scope};
use chloedb_server::service::{shard_of, After, AgentConfig, Backend, QueryReq, ScrollAsk, Service, SqlAnswer};
use chloedb_server::{demo, server};
use tokio::net::TcpListener;

const N: usize = 300;

/// A model on loopback: for a number, the length of the text modulo 7;
/// else "ABOUT:" and the text's first three words, upper-cased. Counts
/// its requests.
struct Mock {
    url: String,
    seen: Arc<Mutex<Vec<J>>>,
}

impl Mock {
    fn start() -> Mock {
        let listener = StdListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let seen: Arc<Mutex<Vec<J>>> = Arc::default();
        let s = seen.clone();
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(conn) = conn else { continue };
                let s = s.clone();
                std::thread::spawn(move || {
                    let mut r = BufReader::new(conn.try_clone().unwrap());
                    let mut line = String::new();
                    r.read_line(&mut line).unwrap();
                    let mut len = 0usize;
                    loop {
                        let mut h = String::new();
                        r.read_line(&mut h).unwrap();
                        let h = h.trim_end().to_ascii_lowercase();
                        if h.is_empty() {
                            break;
                        }
                        if let Some(v) = h.strip_prefix("content-length:") {
                            len = v.trim().parse().unwrap();
                        }
                    }
                    let mut body = vec![0; len];
                    r.read_exact(&mut body).unwrap();
                    let body: J = serde_json::from_slice(&body).unwrap();
                    let user = body["messages"][0]["content"].as_str().unwrap().to_string();
                    let made = match body["system"].as_str().unwrap_or("").contains("Answer with a number") {
                        true => format!("{}", user.len() % 7),
                        false => format!("ABOUT: {}", user.split_whitespace().take(3).collect::<Vec<_>>().join(" ").to_uppercase()),
                    };
                    s.lock().unwrap().push(body);
                    let text = json!({"type": "message", "content": [{"type": "text", "text": made}]}).to_string();
                    let mut w = conn;
                    let _ = w.write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{text}", text.len()).as_bytes());
                });
            }
        });
        Mock { url, seen }
    }

    fn requests(&self) -> usize {
        self.seen.lock().unwrap().len()
    }

    /// A models directory whose model `mock-claude` is this server.
    fn models(&self, dir: &Path) -> std::path::PathBuf {
        let models = dir.join("models");
        std::fs::create_dir_all(models.join("mock-claude")).unwrap();
        std::fs::write(models.join("mock-claude").join("endpoint.json"), json!({"api": "anthropic", "base_url": self.url, "model": "claude-mock"}).to_string()).unwrap();
        models
    }
}

fn agent() -> AgentConfig {
    AgentConfig { api_key: None, model: chloedb_server::agent::DEFAULT_MODEL.into(), base_url: "http://unused".into() }
}

fn create_keeping(dir: &Path, name: &str, keep: impl Fn(usize) -> bool) -> Arc<Db> {
    let opts = DbOptions { publish_snapshots: true, segment_max_docs: 150, ship_log_bytes: 16 << 20, ..DbOptions::default() };
    let db = Db::create(&dir.join(name), demo::schema(N), opts).unwrap();
    db.set_embedder("emb", Arc::new(chloedb::embed::HashEmbedder { dim: demo::DIM })).unwrap();
    db.load((0..N).filter(|&i| keep(i)).map(|i| demo::product(i, 7))).unwrap();
    Arc::new(db)
}

fn service(db: Arc<Db>, models: &Path) -> Arc<Service> {
    let schema = (*db.schema()).clone();
    let mut svc = Service::new(Backend::Local(db), schema, agent(), 16, Duration::from_secs(10));
    svc.open_models(models);
    Arc::new(svc)
}

async fn shard(dir: &Path, name: &str, keep: impl Fn(usize) -> bool) -> String {
    let db = create_keeping(dir, name, keep);
    db.storage().start_seq_no().unwrap();
    let schema = (*db.schema()).clone();
    let mut svc = Service::new(Backend::Local(db), schema, agent(), 16, Duration::from_secs(20));
    svc.max_rows = Some(10_000);
    let svc = Arc::new(svc);
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(server::serve_grpc(svc, l, Access::default()));
    url
}

fn admin() -> Principal {
    Principal { name: "ops".into(), key: Some("ops-id".into()), scope: Scope::Admin, root: false }
}

async fn sql(svc: &Arc<Service>, s: &str) -> SqlAnswer {
    svc.sql(&admin(), s, &Params::new(), After::Any, 0).await.unwrap_or_else(|e| panic!("{s}: {}", e.message))
}

async fn query(svc: &Arc<Service>, s: &str) -> chloedb::query::exec::ResultSet {
    svc.query(QueryReq { sql: s.into(), ..Default::default() }).await.unwrap_or_else(|e| panic!("{s}: {}", e.message)).rs
}

#[tokio::test(flavor = "multi_thread")]
async fn a_server_asks_a_model_about_the_rows_it_returns() {
    let mock = Mock::start();
    let dir = tempfile::tempdir().unwrap();
    let svc = service(create_keeping(dir.path(), "one.strata", |_| true), &mock.models(dir.path()));
    sql(&svc, "CREATE FUNCTION about(t TEXT) RETURNS TEXT AS llm('mock-claude', 'Say what the product is about, one server.', :t)").await;
    let q = "SELECT id, about(title) FROM products WHERE MATCH(body, 'easy') ORDER BY bm25(body, 'easy') DESC LIMIT 4";
    let rs = query(&svc, q).await;
    assert_eq!(rs.columns, vec!["id", "about(title)"]);
    assert_eq!(rs.rows.len(), 4);
    assert!(rs.rows.iter().all(|r| matches!(&r[1], Value::Str(a) if a.starts_with("ABOUT: "))), "{:?}", rs.rows);
    assert_eq!(mock.requests(), 4);
    assert_eq!(mock.seen.lock().unwrap()[0]["model"], "claude-mock");
    // Through the SQL path, answered from memory.
    let SqlAnswer::Rows(a) = sql(&svc, q).await else { panic!("rows") };
    assert_eq!((a.rs.rows, mock.requests()), (rs.rows, 4));
    let metrics = svc.prometheus().await;
    assert!(metrics.lines().any(|l| l.starts_with("chloedb_llm_requests_total ") && l != "chloedb_llm_requests_total 0"), "{metrics}");
    // A scroll is refused, and so is a call where it would ask about rows
    // the query does not return.
    let e = svc.query(QueryReq { sql: q.into(), scroll: Some(ScrollAsk { page: 2, keep_alive: None, owner: "ops".into() }), ..Default::default() }).await.err().unwrap().message;
    assert!(e.contains("not scrolled"), "{e}");
    let e = svc.query(QueryReq { sql: "SELECT id FROM products WHERE about(title) = 'x' LIMIT 3".into(), ..Default::default() }).await.err().unwrap().message;
    assert!(e.contains("not part of WHERE"), "{e}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_coordinator_asks_about_the_merged_rows_only() {
    let mock = Mock::start();
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let models = mock.models(d);
    let key = |i: usize| Value::Str(format!("p{i:07}"));
    let mut urls = Vec::new();
    for s in 0..3 {
        urls.push(shard(d, &format!("s{s}.strata"), |i| shard_of(&key(i), 3) == s).await);
    }
    let c = Arc::new(Coordinator::connect(urls).await.unwrap());
    let schema = c.schema.clone();
    let mut coord = Service::new(Backend::Cluster(c), schema, agent(), 16, Duration::from_secs(10));
    coord.open_models(&models);
    let coord = Arc::new(coord);
    let whole = service(create_keeping(d, "whole.strata", |_| true), &models);
    let about = "CREATE FUNCTION about(t TEXT) RETURNS TEXT AS llm('mock-claude', 'Say what the product is about, a cluster.', :t)";
    sql(&coord, about).await;
    sql(&whole, about).await;

    // Four rows of three shards: four questions, asked on the coordinator.
    let q = "SELECT id, price, about(title) AS about FROM products WHERE MATCH(body, 'easy') ORDER BY bm25(body, 'easy') DESC LIMIT 4";
    let got = query(&coord, q).await;
    assert_eq!(got.rows.len(), 4);
    assert_eq!(mock.requests(), 4);
    // One database answers alike, from memory.
    assert_eq!(query(&whole, q).await.rows, got.rows);
    assert_eq!(mock.requests(), 4);

    // ORDER BY over the first ranking's top rows, ties in its order.
    let ranked = "WITH top AS (SELECT * FROM products WHERE MATCH(body, 'easy') ORDER BY bm25(body, 'easy') DESC LIMIT 12) SELECT id, score() FROM top ORDER BY llm('mock-claude', 'Grade it.', title, returns => 'number') DESC LIMIT 5";
    let got = query(&coord, ranked).await;
    assert_eq!(got.rows.len(), 5);
    assert_eq!(mock.requests(), 4 + 12);
    assert_eq!(query(&whole, ranked).await.rows, got.rows);
    let grades: Vec<f64> = got.rows.iter().map(|r| r[1].as_f64().unwrap()).collect();
    assert!(grades.windows(2).all(|w| w[0] >= w[1]), "{grades:?}");
}
