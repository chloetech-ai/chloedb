//! `PACK n TOKENS` through a server and a coordinator: the slices of
//! candidates and the rows followed are queries of their own, each merged
//! from the shards, and the pack is the one database's.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use chloedb::db::Db;
use chloedb::query::Params;
use chloedb::storage::DbOptions;
use chloedb::value::Value;
use chloedb_server::auth::Access;
use chloedb_server::coord::Coordinator;
use chloedb_server::keys::Principal;
use chloedb_server::service::{AgentConfig, After, Backend, QueryReq, Service, SqlAnswer};
use chloedb_server::{demo, server};
use tokio::net::TcpListener;

fn agent() -> AgentConfig {
    AgentConfig { api_key: None, model: chloedb_server::agent::DEFAULT_MODEL.into(), base_url: "http://unused".into() }
}

const TABLE: &str = "CREATE TABLE memory (id KEYWORD PRIMARY KEY, rank INT, text TEXT, links EDGES)";

fn create(dir: &Path, name: &str) -> Arc<Db> {
    let opts = DbOptions { segment_max_docs: 60, ..DbOptions::default() };
    let db = Db::create(&dir.join(name), demo::schema(10), opts).unwrap();
    db.set_embedder("emb", Arc::new(chloedb::embed::HashEmbedder { dim: demo::DIM })).unwrap();
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn packs_through_a_coordinator_as_one_database() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let urls = vec![serve(create(d, "s0.strata")).await, serve(create(d, "s1.strata")).await];
    let c = Arc::new(Coordinator::connect(urls).await.unwrap());
    let schema = c.schema.clone();
    let svc = Arc::new(Service::new(Backend::Cluster(c), schema, agent(), 16, Duration::from_secs(10)));
    let one = create(d, "one.strata");
    run(&svc, TABLE).await;
    one.sql(TABLE, &Params::new()).unwrap();
    // 300 notes on the retry policy, each linking to the next, and every
    // tenth repeating the one before it.
    let values: Vec<String> = (0..300)
        .map(|i| {
            let text = match i % 10 {
                9 => format!("note {} retry policy backoff capped at {} seconds", i - 1, (i - 1) % 7),
                _ => format!("note {i} retry policy backoff capped at {} seconds", i % 7),
            };
            format!("('m{i:03}', {}, '{text}', '[\"m{:03}\"]')", 1000 - i, (i + 1) % 300)
        })
        .collect();
    let insert = format!("INSERT INTO memory (id, rank, text, links) VALUES {}", values.join(", "));
    run(&svc, &insert).await;
    one.sql(&insert, &Params::new()).unwrap();
    for q in [
        "SELECT id, text FROM memory WHERE MATCH(text, 'retry') ORDER BY rank DESC PACK 200 TOKENS DEDUPE 0.9 WITH RECEIPT",
        "SELECT id, text FROM memory WHERE rank % 2 = 0 ORDER BY rank DESC PACK 120 TOKENS FOLLOW links 1 HOP WITH RECEIPT",
        "SELECT id, text FROM memory WHERE rank % 3 = 0 ORDER BY rank DESC PACK 150 TOKENS FOLLOW links 2 HOPS WITH RECEIPT",
        "SELECT id, text FROM memory ORDER BY rank DESC PACK 100000 TOKENS LIMIT 70 OFFSET 3",
    ] {
        let want = one.query(q, &Params::new()).unwrap_or_else(|e| panic!("{q}: {e}"));
        assert!(!want.rows.is_empty(), "{q}");
        let got = svc.query(QueryReq { sql: q.into(), ..Default::default() }).await.unwrap_or_else(|e| panic!("{q}: {}", e.message)).rs;
        assert_eq!(got.columns, want.columns, "{q}");
        assert_eq!(got.rows, want.rows, "{q}");
    }
    // Through SQL, as a query.
    let q = "SELECT id, text FROM memory ORDER BY rank DESC PACK 50 TOKENS";
    let SqlAnswer::Rows(a) = svc.sql(&Principal::open(), q, &Params::new(), After::Any, 0).await.unwrap_or_else(|e| panic!("{}", e.message)) else { panic!("rows") };
    assert_eq!(a.rs.rows[0][0], Value::Str("m000".into()));
    assert!(a.rs.columns.ends_with(&["pack_tokens".to_string(), "pack_decision".to_string()]));
}

/// On a server, a pack is admitted once: a Contain rule that lets one
/// query of a key run at a time refused its second read while its first
/// held the turn (audit P3). Its reads hold one snapshot, a lease released
/// after the pack unless the client asks to keep it (P4); `LIMIT 0`
/// answers the pack's columns (N-B9).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_server_admits_a_pack_once_and_reads_one_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let db = create(dir.path(), "local.strata");
    let schema = (*db.schema()).clone();
    let svc = Arc::new(Service::new(Backend::Local(db), schema, agent(), 16, Duration::from_secs(10)));
    run(&svc, TABLE).await;
    let values: Vec<String> = (0..400).map(|i| format!("('m{i:03}', {}, 'note {i} retry policy backoff capped at {} seconds', '[]')", 1000 - i, i % 7)).collect();
    run(&svc, &format!("INSERT INTO memory (id, rank, text, links) VALUES {}", values.join(", "))).await;
    svc.contain.add(&serde_json::json!({"match": {"key": "agent"}, "action": "limit", "concurrency": 1, "expires_s": 60, "reason": "one at a time"}), "test").unwrap();
    let caller = Arc::new(chloedb_server::telemetry::Caller { key: Some("agent".into()), ..Default::default() });
    // About 2,000 tokens of notes: more than the first read's 64 rows.
    let sql = "SELECT id, text FROM memory ORDER BY rank DESC PACK 2000 TOKENS";
    let a = chloedb_server::telemetry::calling(caller.clone(), svc.query(QueryReq { sql: sql.into(), timeout_ms: 3000, ..Default::default() })).await.unwrap_or_else(|e| panic!("{}: {}", e.status, e.message));
    let line = a.rs.stats.plan.iter().find(|l| l.starts_with("pack: ")).unwrap();
    assert!(!line.ends_with("1 query"), "{line}");
    assert!(a.rs.rows.len() > 64, "{line}");
    assert_eq!((a.lease, svc.sweep_leases()), (0, 0), "the pack's lease is released");
    // Kept for the client with hold.
    let a = chloedb_server::telemetry::calling(caller, svc.query(QueryReq { sql: sql.into(), hold: true, ..Default::default() })).await.unwrap();
    assert!(a.lease != 0);
    assert_eq!(svc.sweep_leases(), 1);
    svc.release(&[a.lease]);
    let a = svc.query(QueryReq { sql: "SELECT id, text FROM memory ORDER BY rank DESC PACK 100 TOKENS LIMIT 0".into(), ..Default::default() }).await.unwrap_or_else(|e| panic!("{}", e.message));
    assert_eq!(a.rs.columns, vec!["id", "text", "pack_tokens", "pack_decision"]);
    assert!(a.rs.rows.is_empty());
    // The budget a parameter.
    let a = svc.query(QueryReq { sql: "SELECT id, text FROM memory ORDER BY rank DESC PACK :n TOKENS".into(), params: Params::new().set("n", Value::Int(60)), ..Default::default() }).await.unwrap();
    let spent: i64 = a.rs.rows.iter().map(|r| match r[2] {
        Value::Int(n) => n,
        _ => panic!("{r:?}"),
    }).sum();
    assert!(spent > 0 && spent <= 60, "{spent}");
}
