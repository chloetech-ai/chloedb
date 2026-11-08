//! `hash(x [, seed])` through a coordinator: plans that hash (plan feature
//! "hash", proto `FUNC_HASH`) reach the shards, and a seeded sample, split
//! or shuffle over two shards holds the same rows, in the same order, as
//! over one database of the same rows.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use strata::db::{Db, SqlResult};
use strata::query::Params;
use strata::storage::DbOptions;
use strata::value::Value;
use strata_server::auth::Access;
use strata_server::coord::Coordinator;
use strata_server::keys::Principal;
use strata_server::service::{AgentConfig, After, Backend, QueryReq, Service};
use strata_server::{demo, server};
use tokio::net::TcpListener;

fn agent() -> AgentConfig {
    AgentConfig { api_key: None, model: strata_server::agent::DEFAULT_MODEL.into(), base_url: "http://unused".into() }
}

const TABLE: &str = "CREATE TABLE events (id KEYWORD PRIMARY KEY, who KEYWORD, n INT, at TIMESTAMP)";

fn create(dir: &Path, name: &str) -> Arc<Db> {
    let opts = DbOptions { segment_max_docs: 60, ..DbOptions::default() };
    let db = Db::create(&dir.join(name), demo::schema(10), opts).unwrap();
    db.set_embedder("emb", Arc::new(strata::embed::HashEmbedder { dim: demo::DIM })).unwrap();
    Arc::new(db)
}

async fn serve(db: Arc<Db>) -> String {
    let schema = (*db.schema()).clone();
    let svc = Arc::new(Service::new(Backend::Local(db), schema, agent(), 16, Duration::from_secs(10)));
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(server::serve_grpc(svc, l, Access::default()));
    url
}

async fn run(svc: &Arc<Service>, s: &str) {
    svc.sql(&Principal::open(), s, &Params::new(), After::Any, 0).await.unwrap_or_else(|e| panic!("{s}: {}", e.message));
}

async fn select(svc: &Arc<Service>, sql: &str) -> Vec<Vec<Value>> {
    svc.query(QueryReq { sql: sql.into(), ..Default::default() }).await.unwrap_or_else(|e| panic!("{sql}: {}", e.message)).rs.rows
}

fn local(db: &Db, sql: &str) -> Vec<Vec<Value>> {
    match db.sql(sql, &Params::new()).unwrap_or_else(|e| panic!("{sql}: {e}")) {
        SqlResult::Rows(r) => r.rows,
        SqlResult::Written(_) => Vec::new(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_split_over_shards_is_the_one_databases() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let urls = vec![serve(create(d, "s0.strata")).await, serve(create(d, "s1.strata")).await];
    let c = Arc::new(Coordinator::connect(urls).await.unwrap());
    let schema = c.schema.clone();
    let svc = Arc::new(Service::new(Backend::Cluster(c), schema, agent(), 16, Duration::from_secs(10)));
    let one = create(d, "one.strata");
    run(&svc, TABLE).await;
    local(&one, TABLE);
    for chunk in (0..900).collect::<Vec<_>>().chunks(90) {
        let values: Vec<String> = chunk
            .iter()
            .map(|i| {
                let n = if i % 11 == 0 { "NULL".to_string() } else { (i % 97).to_string() };
                format!("('e{i:04}', 'u{}', {n}, '{}')", i % 45, strata::timestamp::format(1_785_542_400_000_000 + *i as i64 * 3_600_000_000))
            })
            .collect();
        let insert = format!("INSERT INTO events (id, who, n, at) VALUES {}", values.join(", "));
        run(&svc, &insert).await;
        local(&one, &insert);
    }
    let sorted = |mut rows: Vec<Vec<Value>>| {
        rows.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
        rows
    };
    // Sets of rows: a sample, both sides of a split, by key and by user.
    for q in [
        "SELECT id FROM events WHERE hash(id, 7) % 10 < 8",
        "SELECT id FROM events WHERE NOT (hash(id, 7) % 10 < 8)",
        "SELECT id, who FROM events WHERE hash(who, 3) % 4294967296 < 2147483648 AND n > 10",
        "SELECT id, hash(id, 7), hash(n), hash(at, -1) FROM events WHERE who = 'u4'",
        "SELECT COUNT(*) FROM events WHERE hash(n, 2) % 2 = 0",
        "WITH q AS (SELECT id, who FROM events WHERE n > 5) SELECT id FROM q WHERE hash(who, 1) % 3 = 0",
    ] {
        let want = sorted(local(&one, q));
        assert!(!want.is_empty(), "{q}");
        assert_eq!(sorted(select(&svc, q).await), want, "{q}");
    }
    // Orders: a seeded shuffle is the same rows in the same order.
    for q in ["SELECT id FROM events ORDER BY hash(id, 5) LIMIT 25", "SELECT id, n FROM events WHERE n > 50 ORDER BY hash(who, 9), id LIMIT 40"] {
        let want = local(&one, q);
        assert!(want.len() >= 25, "{q}");
        assert_eq!(select(&svc, q).await, want, "{q}");
    }
}
