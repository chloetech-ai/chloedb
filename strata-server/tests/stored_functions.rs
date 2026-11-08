//! Stored functions on a server: `CREATE FUNCTION` needs an admin key and
//! records its name; a query calls them as one database does, in every
//! read path; and `SELECT ... FROM _functions` reads their versions.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use strata::db::Db;
use strata::query::Params;
use strata::storage::DbOptions;
use strata::value::Value;
use strata_server::keys::{Principal, Scope};
use strata_server::service::{After, AgentConfig, Backend, QueryReq, Service, SqlAnswer};
use strata_server::demo;

const N: usize = 300;

fn agent() -> AgentConfig {
    AgentConfig { api_key: None, model: strata_server::agent::DEFAULT_MODEL.into(), base_url: "http://unused".into() }
}

/// The demo products `keep` keeps, of the first `N`.
fn create_keeping(dir: &Path, name: &str, keep: impl Fn(usize) -> bool) -> Arc<Db> {
    let opts = DbOptions { publish_snapshots: true, segment_max_docs: 150, ship_log_bytes: 16 << 20, ..DbOptions::default() };
    let db = Db::create(&dir.join(name), demo::schema(N), opts).unwrap();
    db.set_embedder("emb", Arc::new(strata::embed::HashEmbedder { dim: demo::DIM })).unwrap();
    db.load((0..N).filter(|&i| keep(i)).map(|i| demo::product(i, 7))).unwrap();
    Arc::new(db)
}

fn create(dir: &Path, name: &str) -> Arc<Db> {
    create_keeping(dir, name, |_| true)
}

fn service(db: Arc<Db>) -> Arc<Service> {
    let schema = (*db.schema()).clone();
    Arc::new(Service::new(Backend::Local(db), schema, agent(), 16, Duration::from_secs(10)))
}

fn key_of(name: &str, scope: Scope) -> Principal {
    Principal { name: name.into(), key: Some(format!("{name}-id")), scope, root: false }
}

async fn sql_as(svc: &Arc<Service>, who: &Principal, s: &str) -> Result<SqlAnswer, String> {
    svc.sql(who, s, &Params::new(), After::Any, 0).await.map_err(|e| e.message)
}

async fn rows(svc: &Arc<Service>, s: &str) -> Vec<Vec<Value>> {
    svc.query(QueryReq { sql: s.into(), ..Default::default() }).await.unwrap_or_else(|e| panic!("{s}: {}", e.message)).rs.rows
}

const CHEAPER: &str = "CREATE FUNCTION cheaper(s FLOAT, price FLOAT, w FLOAT DEFAULT 0.01) RETURNS FLOAT AS :s / (1 + :w * :price)";
const KETTLES: &str = "CREATE FUNCTION kettles(t TEXT DEFAULT 'kettle', k INT DEFAULT 5) RETURNS TABLE AS SELECT id, price FROM products WHERE MATCH(body, :t) ORDER BY bm25(body, :t) DESC LIMIT :k";

#[tokio::test(flavor = "multi_thread")]
async fn a_server_keeps_and_calls_stored_functions() {
    let dir = tempfile::tempdir().unwrap();
    let svc = service(create(dir.path(), "one.strata"));
    let admin = key_of("ops", Scope::Admin);
    // A function is a definition of the database: a write key may not make one.
    let e = sql_as(&svc, &key_of("app", Scope::Write), CHEAPER).await.err().unwrap();
    assert!(e.contains("table, rank profile or function"), "{e}");
    sql_as(&svc, &admin, CHEAPER).await.unwrap();
    sql_as(&svc, &admin, KETTLES).await.unwrap();
    // Versions are numbered by the database, or by a coordinator.
    let e = sql_as(&svc, &admin, "CREATE OR REPLACE FUNCTION cheaper(s FLOAT) RETURNS FLOAT AT VERSION 9 AS :s").await.err().unwrap();
    assert!(e.contains("AT VERSION is for a coordinator"), "{e}");

    // Called as written out, through the query path and the SQL path.
    let called = "SELECT id FROM products WHERE MATCH(body, 'easy') ORDER BY cheaper(bm25(body, 'easy'), price) DESC LIMIT 5";
    let written = "SELECT id FROM products WHERE MATCH(body, 'easy') ORDER BY bm25(body, 'easy') / (1 + 0.01 * price) DESC LIMIT 5";
    let want = rows(&svc, written).await;
    assert_eq!(want.len(), 5);
    assert_eq!(rows(&svc, called).await, want);
    let SqlAnswer::Rows(a) = sql_as(&svc, &key_of("reader", Scope::Read), called).await.unwrap() else { panic!("rows") };
    assert_eq!(a.rs.rows, want, "a read key calls them");
    let top = rows(&svc, "SELECT * FROM kettles(k => 3)").await;
    assert_eq!(top, rows(&svc, "SELECT id, price FROM products WHERE MATCH(body, 'kettle') ORDER BY bm25(body, 'kettle') DESC LIMIT 3").await);

    // The versions, and who wrote them.
    sql_as(&svc, &admin, "CREATE OR REPLACE FUNCTION cheaper(s FLOAT, price FLOAT, w FLOAT DEFAULT 0.02) RETURNS FLOAT AS :s / (1 + :w * :price)").await.unwrap();
    let r = rows(&svc, "SELECT id, created_by, dropped FROM _functions ORDER BY id").await;
    let row = |id: &str| vec![Value::Str(id.into()), Value::Str("ops".into()), Value::Bool(false)];
    assert_eq!(r, [row("cheaper@1"), row("cheaper@2"), row("kettles@1")]);
    assert_eq!(rows(&svc, &called.replace("cheaper(", "\"cheaper@1\"(")).await, want, "a pinned version");
    sql_as(&svc, &admin, "DROP FUNCTION kettles").await.unwrap();
    let e = svc.query(QueryReq { sql: "SELECT * FROM kettles()".into(), ..Default::default() }).await.err().unwrap().message;
    assert!(e.contains("kettles"), "{e}");
    // Not a table people write.
    let e = sql_as(&svc, &admin, "INSERT INTO _functions (id) VALUES ('x@1')").await.err().unwrap();
    assert!(e.contains("_functions"), "{e}");
}
