//! A recommender over HTTP: `CREATE RECOMMENDER` through `/v1/sql`, events
//! written by `/v1/write` feeding its counts, `recommend(...)` ranking a
//! table's rows through `/v1/query`; a coordinator refuses both, and runs
//! a `FULL JOIN` as one database does.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use chloedb::db::{Db, SqlResult};
use chloedb::query::Params;
use chloedb::storage::DbOptions;
use chloedb::value::Value;
use chloedb_server::auth::Access;
use chloedb_server::coord::Coordinator;
use chloedb_server::keys::{Grant, Principal, Scope};
use chloedb_server::service::{After, AgentConfig, Backend, QueryReq, Service};
use chloedb_server::{demo, server};
use serde_json::{json, Value as Json};
use tokio::net::TcpListener;

fn agent() -> AgentConfig {
    AgentConfig { api_key: None, model: chloedb_server::agent::DEFAULT_MODEL.into(), base_url: "http://unused".into() }
}

fn create(dir: &Path, name: &str) -> Arc<Db> {
    let db = Db::create(&dir.join(name), demo::schema(10), DbOptions { segment_max_docs: 50, publish_snapshots: true, ..DbOptions::default() }).unwrap();
    db.set_embedder("emb", Arc::new(chloedb::embed::HashEmbedder { dim: demo::DIM })).unwrap();
    Arc::new(db)
}

fn service(backend: Backend, schema: chloedb::Schema) -> Arc<Service> {
    Arc::new(Service::new(backend, schema, agent(), 16, Duration::from_secs(10)))
}

#[tokio::test(flavor = "multi_thread")]
async fn a_recommender_over_http() {
    let dir = tempfile::tempdir().unwrap();
    let db = create(dir.path(), "r.strata");
    let schema = (*db.schema()).clone();
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let h = format!("http://{}", l.local_addr().unwrap());
    let svc = service(Backend::Local(db), schema);
    tokio::spawn(server::serve_http(svc.clone(), l, Access::default()));
    let c = chloedb_server::http_client().build().unwrap();
    let post = |path: &str, body: Json| {
        let (c, url) = (c.clone(), format!("{h}{path}"));
        async move {
            let r = c.post(url).json(&body).send().await.unwrap();
            (r.status().as_u16(), r.json::<Json>().await.unwrap())
        }
    };
    for s in [
        "CREATE TABLE items (id KEYWORD PRIMARY KEY, title TEXT)",
        "INSERT INTO items VALUES ('tent', 'tent'), ('pegs', 'tent pegs'), ('stove', 'camp stove'), ('mug', 'enamel mug'), ('lamp', 'head lamp')",
        "CREATE TABLE events (id KEYWORD PRIMARY KEY, usr KEYWORD, item KEYWORD, ts TIMESTAMP)",
        "INSERT INTO events VALUES ('e1', 'u1', 'tent', '2026-11-01T10:00:00Z'), ('e2', 'u1', 'pegs', '2026-11-01T10:05:00Z')",
    ] {
        let (status, a) = post("/v1/sql", json!({"sql": s})).await;
        assert_eq!(status, 200, "{s}: {a}");
    }
    let (status, a) = post("/v1/sql", json!({"sql": "CREATE RECOMMENDER shop ON events(usr, item, ts) ITEMS items"})).await;
    assert_eq!(status, 200, "{a}");
    assert_eq!(a["rows"][0][2], 2, "{a}");
    // Events through /v1/write: two more users bought pegs after a tent, then a stove.
    let mut ops = Vec::new();
    for (u, base) in [("u2", 0), ("u3", 100)] {
        for (j, item) in ["tent", "pegs", "stove"].iter().enumerate() {
            ops.push(json!({"op": "insert", "table": "events", "doc": {"id": format!("{u}-{j}"), "usr": u, "item": item, "ts": chloedb::timestamp::format(1_793_000_000_000_000i64 + (base + j as i64) * 1_000_000)}}));
        }
    }
    let (status, a) = post("/v1/write", json!({"ops": ops})).await;
    assert_eq!(status, 200, "{a}");
    // u1 had a tent and pegs: a stove comes next.
    let (status, a) = post("/v1/query", json!({"sql": "SELECT id, title FROM items ORDER BY recommend('shop', :u) DESC LIMIT 2", "params": {"u": "u1"}})).await;
    assert_eq!(status, 200, "{a}");
    assert_eq!(a["rows"][0][0], "stove", "{a}");
    assert!(a["rows"].as_array().unwrap().iter().all(|r| r[0] != "tent" && r[0] != "pegs"), "{a}");
    let (status, a) = post("/v1/query", json!({"sql": "SELECT item, next FROM recommend('shop', 'u1', k => 1)"})).await;
    assert_eq!(status, 200, "{a}");
    assert_eq!(a["rows"], json!([["stove", 1]]), "{a}");
    // A key limited to tables reads recommendations only with read on the counts.
    let limited = |tables: &[&str]| Principal {
        name: "k".into(),
        key: Some("k-id".into()),
        scope: Scope::Read,
        root: false,
        tables: Some(tables.iter().map(|t| Grant::new(t, Scope::Read).unwrap()).collect::<Vec<_>>().into()),
        allow: None,
    };
    let q = "SELECT id FROM items ORDER BY recommend('shop', 'u1') DESC LIMIT 1";
    let e = svc.sql(&limited(&["items"]), q, &Params::new(), After::Any, 0).await.err().expect("refused").message;
    assert!(e.contains("shop_counts"), "{e}");
    assert!(svc.sql(&limited(&["items", "shop_*"]), q, &Params::new(), After::Any, 0).await.is_ok());
    let (status, a) = post("/v1/sql", json!({"sql": "DROP RECOMMENDER shop"})).await;
    assert_eq!(status, 200, "{a}");
    let (status, a) = post("/v1/query", json!({"sql": "SELECT * FROM recommend('shop', 'u1')"})).await;
    assert_eq!(status, 400, "{a}");
}

async fn shard(db: Arc<Db>) -> String {
    let schema = (*db.schema()).clone();
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(server::serve_grpc(service(Backend::Local(db), schema), l, Access::default()));
    url
}

fn local(db: &Db, sql: &str) -> Vec<Vec<Value>> {
    match db.sql(sql, &Params::new()).unwrap_or_else(|e| panic!("{sql}: {e}")) {
        SqlResult::Rows(r) => r.rows,
        SqlResult::Written(_) => Vec::new(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_coordinator_full_joins_and_refuses_recommenders() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let urls = vec![shard(create(d, "s0.strata")).await, shard(create(d, "s1.strata")).await];
    let c = Arc::new(Coordinator::connect(urls).await.unwrap());
    let schema = c.schema.clone();
    let svc = service(Backend::Cluster(c), schema);
    let one = create(d, "one.strata");
    let run = |s: String| {
        let svc = svc.clone();
        async move { svc.sql(&Principal::open(), &s, &Params::new(), After::Any, 0).await.map_err(|e| e.message) }
    };
    let mut setup = vec!["CREATE TABLE l (id INT PRIMARY KEY, k KEYWORD, x INT)".to_string(), "CREATE TABLE r (id INT PRIMARY KEY, k KEYWORD, y INT)".to_string()];
    let lv: Vec<String> = (0..40).map(|i| format!("({i}, {}, {i})", if i % 7 == 0 { "NULL".into() } else { format!("'k{}'", i % 13) })).collect();
    let rv: Vec<String> = (0..30).map(|i| format!("({i}, {}, {})", if i % 11 == 0 { "NULL".into() } else { format!("'k{}'", (i % 17) + 5) }, i * 2)).collect();
    setup.push(format!("INSERT INTO l VALUES {}", lv.join(", ")));
    setup.push(format!("INSERT INTO r VALUES {}", rv.join(", ")));
    for s in setup {
        run(s.clone()).await.unwrap_or_else(|e| panic!("{s}: {e}"));
        local(&one, &s);
    }
    for q in [
        "SELECT l.id, r.id FROM l FULL JOIN r ON l.k = r.k ORDER BY l.id NULLS LAST, r.id NULLS LAST",
        "SELECT l.id, r.id FROM l FULL OUTER JOIN r ON l.k = r.k AND r.y > 20 WHERE l.x IS NULL OR l.x < 30 ORDER BY l.id NULLS LAST, r.id NULLS LAST",
        "SELECT coalesce(l.k, r.k) AS key, count(*) FROM l FULL JOIN r ON l.k = r.k GROUP BY key ORDER BY key NULLS FIRST",
    ] {
        let got = svc.query(QueryReq { sql: q.into(), ..Default::default() }).await.unwrap_or_else(|e| panic!("{q}: {}", e.message)).rs.rows;
        assert_eq!(got, local(&one, q), "{q}");
        assert!(!got.is_empty(), "{q}");
    }
    run("CREATE TABLE events (id KEYWORD PRIMARY KEY, usr KEYWORD, item KEYWORD, ts INT)".into()).await.unwrap();
    let e = run("CREATE RECOMMENDER shop ON events(usr, item, ts)".into()).await.err().expect("refused");
    assert!(e.contains("does not run through a coordinator"), "{e}");
    let e = svc.query(QueryReq { sql: "SELECT id FROM l ORDER BY recommend('shop', 'u1') DESC".into(), ..Default::default() }).await.err().expect("refused").message;
    assert!(e.contains("does not run through a coordinator"), "{e}");
}
