//! Stored functions on a server: `CREATE FUNCTION` needs an admin key and
//! records its name; a query calls them as one database does, in every
//! read path; and `SELECT ... FROM _functions` reads their versions. Through
//! a coordinator, each version has one number on every shard, a query calls
//! the versions every shard holds, and a new shard gets them all.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use chloedb::db::Db;
use chloedb::query::Params;
use chloedb::storage::DbOptions;
use chloedb::value::Value;
use chloedb_server::auth::Access;
use chloedb_server::coord::Coordinator;
use chloedb_server::keys::{Principal, Scope};
use chloedb_server::replication::Acks;
use chloedb_server::service::{shard_of, After, AgentConfig, Backend, QueryReq, Service, SqlAnswer};
use chloedb_server::{demo, server, slots};
use tokio::net::TcpListener;

const N: usize = 300;

fn agent() -> AgentConfig {
    AgentConfig { api_key: None, model: chloedb_server::agent::DEFAULT_MODEL.into(), base_url: "http://unused".into() }
}

/// The demo products `keep` keeps, of the first `N`.
fn create_keeping(dir: &Path, name: &str, keep: impl Fn(usize) -> bool) -> Arc<Db> {
    let opts = DbOptions { publish_snapshots: true, segment_max_docs: 150, ship_log_bytes: 16 << 20, ..DbOptions::default() };
    let db = Db::create(&dir.join(name), demo::schema(N), opts).unwrap();
    db.set_embedder("emb", Arc::new(chloedb::embed::HashEmbedder { dim: demo::DIM })).unwrap();
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
    Principal { name: name.into(), key: Some(format!("{name}-id")), scope, root: false, tables: None, role: None, agent: false, on_behalf_of: None }
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

/// A shard's server: rows of `N` that `keep` keeps, numbered writes, its
/// slots kept beside the database, and at most 10,000 rows a query, as
/// chloedb-server's `--max-result-window` keeps by default.
async fn shard(dir: &Path, name: &str, keep: impl Fn(usize) -> bool) -> (Arc<Service>, String) {
    let db = create_keeping(dir, name, keep);
    db.storage().start_seq_no().unwrap();
    let schema = (*db.schema()).clone();
    let mut svc = Service::new(Backend::Local(db), schema, agent(), 16, Duration::from_secs(20));
    svc.max_rows = Some(10_000);
    svc.keep_slots_in(slots::file_of(&dir.join(name))).unwrap();
    let svc = Arc::new(svc);
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(server::serve_grpc(svc.clone(), l, Access::default()));
    (svc, url)
}

async fn coordinator(urls: &[String]) -> (Arc<Service>, Arc<Coordinator>) {
    let c = Arc::new(Coordinator::connect(urls.to_vec()).await.unwrap());
    let schema = c.schema.clone();
    (Arc::new(Service::new(Backend::Cluster(c.clone()), schema, agent(), 16, Duration::from_secs(10))), c)
}

/// Each version a service calls: (id, dropped).
fn versions(svc: &Arc<Service>) -> Vec<(String, bool)> {
    svc.functions().all().map(|f| (f.id(), f.dropped)).collect()
}

/// One shard with one copy: functions made through its coordinator are
/// numbered, called and listed. (The coordinator read a shard's
/// `_functions` with a LIMIT of a million rows, which a server refuses
/// past its `--max-result-window`, so once the shard held a function the
/// coordinator saw none, and the next CREATE FUNCTION waited for it.)
#[tokio::test(flavor = "multi_thread")]
async fn a_coordinator_of_one_shard_keeps_its_functions() {
    let dir = tempfile::tempdir().unwrap();
    let (one, url) = shard(dir.path(), "one.strata", |_| true).await;
    let (coord, _) = coordinator(&[url.clone()]).await;
    let admin = key_of("ops", Scope::Admin);
    sql_as(&coord, &admin, CHEAPER).await.unwrap();
    sql_as(&coord, &admin, KETTLES).await.unwrap();
    let both = [("cheaper@1".to_string(), false), ("kettles@1".to_string(), false)];
    assert_eq!(versions(&coord), both);
    assert_eq!(versions(&one), both);
    let called = "SELECT id FROM products WHERE MATCH(body, 'easy') ORDER BY cheaper(bm25(body, 'easy'), price) DESC LIMIT 5";
    let want = rows(&one, called).await;
    assert_eq!(want.len(), 5);
    assert_eq!(rows(&coord, called).await, want);
    assert_eq!(rows(&coord, "SELECT * FROM kettles(k => 3)").await, rows(&one, "SELECT * FROM kettles(k => 3)").await);
    // A coordinator started later finds them.
    let (later, _) = coordinator(&[url]).await;
    assert_eq!(versions(&later), both);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_coordinator_numbers_versions_alike_on_every_shard() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let whole = service(create(d, "whole.strata"));
    let key = |i: usize| Value::Str(format!("p{i:07}"));
    let mut shards = Vec::new();
    let mut urls = Vec::new();
    for s in 0..3 {
        let (svc, url) = shard(d, &format!("s{s}.strata"), |i| shard_of(&key(i), 3) == s).await;
        shards.push(svc);
        urls.push(url);
    }
    // One shard has numbered versions of its own: made and dropped on it
    // alone, before the cluster.
    let root = Principal::open();
    sql_as(&shards[1], &root, "CREATE FUNCTION label(p FLOAT) RETURNS KEYWORD AS CASE WHEN :p > 100 THEN 'dear' ELSE 'cheap' END").await.unwrap();
    sql_as(&shards[1], &root, "DROP FUNCTION label").await.unwrap();
    let (coord, c) = coordinator(&urls).await;
    let admin = key_of("ops", Scope::Admin);
    // Version 1 is on one shard only, so not called; its drop is served at once.
    assert_eq!(versions(&coord), [("label@2".to_string(), true)]);
    for svc in [&whole, &coord] {
        sql_as(svc, &admin, CHEAPER).await.unwrap();
        sql_as(svc, &admin, KETTLES).await.unwrap();
    }
    // `label` comes back as version 3 on every shard, after shard 1's.
    sql_as(&coord, &admin, "CREATE FUNCTION label(p FLOAT) RETURNS KEYWORD AS CASE WHEN :p > 50 THEN 'dear' ELSE 'cheap' END").await.unwrap();
    for s in &shards {
        let f = s.functions();
        assert_eq!(f.current("label").unwrap().version, 3, "{:?}", versions(s));
        assert_eq!(f.current("cheaper").unwrap().version, 1);
        assert_eq!(f.current("label").unwrap().hash, coord.functions().current("label").unwrap().hash);
    }
    assert_eq!(coord.functions().current("label").unwrap().version, 3);

    // Called through the coordinator as on one database.
    let called = "SELECT id FROM products WHERE MATCH(body, 'easy') ORDER BY cheaper(bm25(body, 'easy'), price) DESC LIMIT 8";
    let want = rows(&whole, called).await;
    assert_eq!(want.len(), 8);
    assert_eq!(rows(&coord, called).await, want);
    let top = "SELECT * FROM kettles(k => 4)";
    assert_eq!(rows(&coord, top).await, rows(&whole, top).await);
    let labels = rows(&coord, "SELECT id, label(price) FROM products WHERE MATCH(body, 'easy') ORDER BY id LIMIT 6").await;
    assert_eq!(labels.len(), 6);
    assert!(labels.iter().all(|r| ["dear", "cheap"].contains(&r[1].as_str().unwrap())), "{labels:?}");

    // A new version everywhere; the old one still answers by its number.
    let v2 = "CREATE OR REPLACE FUNCTION cheaper(s FLOAT, price FLOAT, w FLOAT DEFAULT 0.05) RETURNS FLOAT AS :s / (1 + :w * :price)";
    for svc in [&whole, &coord] {
        sql_as(svc, &admin, v2).await.unwrap();
    }
    assert_eq!(rows(&coord, called).await, rows(&whole, called).await);
    let pinned = called.replace("cheaper(", "\"cheaper@1\"(");
    assert_eq!(rows(&coord, &pinned).await, want);
    // The same definition again changes nothing.
    sql_as(&coord, &admin, v2).await.unwrap();
    assert!(shards.iter().all(|s| s.functions().max_version("cheaper") == 2));
    // A coordinator started later finds them.
    let (other, _) = coordinator(&urls).await;
    assert_eq!(rows(&other, called).await, rows(&whole, called).await);

    // A table function's body is checked against the cluster's tables.
    let e = sql_as(&coord, &admin, "CREATE FUNCTION nowhere(q TEXT) RETURNS TABLE AS SELECT id FROM missing WHERE MATCH(body, :q)").await.err().unwrap();
    assert!(e.contains("does not plan"), "{e}");
    // Dropped through the coordinator: from every shard.
    sql_as(&coord, &admin, "DROP FUNCTION kettles").await.unwrap();
    assert!(coord.query(QueryReq { sql: top.into(), ..Default::default() }).await.is_err());
    for s in &shards {
        assert!(s.functions().current("kettles").is_none());
    }
    sql_as(&coord, &admin, "DROP FUNCTION cheaper VERSION 1").await.unwrap();
    assert!(shards.iter().all(|s| s.functions().get("cheaper", 1).is_none()));

    // A new shard gets every version under its number.
    let (fresh, url) = shard(d, "s3.strata", |_| false).await;
    c.add_shard(vec![url], Acks::All).await.unwrap();
    let ids = |s: &Arc<Service>| s.functions().all().map(|f| (f.id(), f.hash.clone())).collect::<Vec<_>>();
    assert_eq!(ids(&fresh), ids(&shards[0]));
    assert_eq!(fresh.functions().current("label").unwrap().version, 3);
    assert!(fresh.functions().current("kettles").is_none());
}
