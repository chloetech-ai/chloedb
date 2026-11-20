//! `INSERT ... SELECT` and `CREATE TABLE ... AS SELECT` on one server and
//! through a coordinator (SAM-8): the same tables and rows as one
//! database's, a write that fails leaves nothing, and `/v1/sql/check`
//! says what a coordinator does.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use chloedb::db::{Db, SqlResult};
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

fn create(dir: &Path, name: &str) -> Arc<Db> {
    let db = Db::create(&dir.join(name), demo::schema(10), DbOptions::default()).unwrap();
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

async fn run(svc: &Arc<Service>, s: &str) -> u64 {
    match svc.sql(&Principal::open(), s, &Params::new(), After::Any, 0).await.unwrap_or_else(|e| panic!("{s}: {}", e.message)) {
        SqlAnswer::Written { w, .. } => w.rows,
        SqlAnswer::Rows(_) => 0,
    }
}

async fn fails(svc: &Arc<Service>, s: &str) -> String {
    match svc.sql(&Principal::open(), s, &Params::new(), After::Any, 0).await {
        Ok(_) => panic!("{s}: expected an error"),
        Err(e) => e.message,
    }
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

const TICKETS: &str = "CREATE TABLE tickets (id INT PRIMARY KEY, customer KEYWORD, amount FLOAT, notes TEXT)";

/// Made-up tickets: every seventh without an amount.
fn ticket_rows() -> Vec<String> {
    (0..300).map(|i| {
        let amount = if i % 7 == 0 { "NULL".to_string() } else { format!("{}.5", i % 40) };
        let notes = if i % 5 == 0 { "asked for a refund" } else { "the parcel came late" };
        format!("INSERT INTO tickets (id, customer, amount, notes) VALUES ({i}, 'c{}', {amount}, '{notes}')", i % 9)
    }).collect()
}

/// The statements both run, and the queries whose answers must agree.
const MODELS: [&str; 4] = [
    "CREATE TABLE refunds (PRIMARY KEY (id)) AS SELECT id, customer, amount, notes FROM tickets WHERE MATCH(notes, 'refund')",
    "CREATE TABLE by_customer AS SELECT customer, COUNT(*) AS n, SUM(amount) AS total FROM tickets GROUP BY customer",
    "INSERT INTO refunds (id, customer, amount, notes) SELECT id + 1000, customer, amount, notes FROM tickets WHERE id < 20",
    "INSERT INTO refunds (id, customer, amount, notes) SELECT id, customer, amount * 2, notes FROM tickets WHERE id < 50 ON CONFLICT (id) DO UPDATE SET amount = excluded.amount",
];
const CHECKS: [&str; 5] = [
    "SELECT COUNT(*) FROM refunds",
    "SELECT COUNT(*) FROM refunds WHERE amount IS NULL",
    "SELECT SUM(amount) FROM refunds",
    "SELECT customer, n, total FROM by_customer ORDER BY customer",
    "SELECT id FROM refunds WHERE MATCH(notes, 'refunds') ORDER BY id LIMIT 5",
];

async fn agrees_with_one_database(svc: &Arc<Service>, dir: &Path) {
    let one = create(dir, "one.strata");
    run(svc, TICKETS).await;
    local(&one, TICKETS);
    for r in ticket_rows() {
        run(svc, &r).await;
        local(&one, &r);
    }
    for m in MODELS {
        let n = run(svc, m).await;
        let want = match one.sql(m, &Params::new()).unwrap() {
            SqlResult::Written(n) => n,
            SqlResult::Rows(_) => panic!("{m}: a write"),
        };
        assert_eq!(n, want, "{m}");
    }
    for q in CHECKS {
        let want = local(&one, q);
        assert!(!want.is_empty(), "{q}");
        assert_eq!(select(svc, q).await, want, "{q}");
    }
    let types = |s: &chloedb::Schema| s.fields.iter().map(|f| (f.name.clone(), f.type_name())).collect::<Vec<_>>();
    for t in ["refunds", "by_customer"] {
        let a = svc.catalog().into_iter().find(|s| s.table == t).unwrap_or_else(|| panic!("no {t}"));
        let b = one.catalog().into_iter().find(|s| s.table == t).unwrap();
        assert_eq!(types(&a), types(&b), "{t}");
        assert_eq!(a.primary_key, b.primary_key, "{t}");
    }
    // A key two rows share: the table is not left behind.
    let e = fails(svc, "CREATE TABLE dup (PRIMARY KEY (customer)) AS SELECT customer, id FROM tickets").await;
    assert!(e.contains("c0") || e.to_lowercase().contains("key"), "{e}");
    assert!(!svc.catalog().iter().any(|s| s.table == "dup"));
    assert!(fails(svc, "INSERT INTO nosuch SELECT id FROM tickets").await.contains("no such table: nosuch"));
    assert!(fails(svc, "CREATE TABLE refunds AS SELECT id FROM tickets").await.contains("already exists"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn on_one_server_the_rows_are_one_databases() {
    let dir = tempfile::tempdir().unwrap();
    let db = create(dir.path(), "s.strata");
    let schema = (*db.schema()).clone();
    let svc = Arc::new(Service::new(Backend::Local(db), schema, agent(), 16, Duration::from_secs(10)));
    agrees_with_one_database(&svc, dir.path()).await;
    let check = svc.sql_check("INSERT INTO refunds (id) SELECT id + 5000 FROM tickets", &Params::new(), 0, Default::default()).await.unwrap();
    assert_eq!(check["runs"], true, "{check}");
    assert_eq!(check["kind"], "write", "{check}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn through_a_coordinator_the_rows_are_one_databases() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let urls = vec![serve(create(d, "s0.strata")).await, serve(create(d, "s1.strata")).await];
    let c = Arc::new(Coordinator::connect(urls).await.unwrap());
    let schema = c.schema.clone();
    let svc = Arc::new(Service::new(Backend::Cluster(c), schema, agent(), 16, Duration::from_secs(10)));
    agrees_with_one_database(&svc, d).await;
    let check = svc.sql_check("CREATE TABLE more AS SELECT id FROM tickets", &Params::new(), 0, Default::default()).await.unwrap();
    assert_eq!(check["runs"], true, "{check}");
    assert!(check["notes"][0].as_str().unwrap().contains("not one transaction"), "{check}");
}
