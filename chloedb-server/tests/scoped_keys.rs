//! Keys limited to tables ([`chloedb_server::keys::Grant`]): what such a
//! key reads, writes and creates over the HTTP API, what it is refused
//! (other tables, the server's own admin, routes that do not check
//! tables, gRPC and the OpenSearch listener), an app's key that the server
//! makes and keeps in memory, and an app told who called it with a token
//! of its own.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::http::HeaderMap;
use axum::Router;
use chloedb::db::Db;
use chloedb::storage::DbOptions;
use chloedb_server::apps::{parse_with, Apps};
use chloedb_server::auth::{Access, Secret};
use chloedb_server::keys::{Grant, Keys};
use chloedb_server::service::{AgentConfig, Backend, Service};
use chloedb_server::{demo, server};
use serde_json::{json, Value as Json};
use tokio::net::TcpListener;

fn service(dir: &std::path::Path) -> Service {
    let db = Db::create(&dir.join("a.strata"), demo::schema(10), DbOptions::default()).unwrap();
    let schema = (*db.schema()).clone();
    let agent = AgentConfig { api_key: None, model: String::new(), base_url: String::new() };
    let mut svc = Service::new(Backend::Local(Arc::new(db)), schema, agent, 16, Duration::from_secs(10));
    svc.keys = Arc::new(Keys::open(dir.join("a.strata.keys.json")).unwrap());
    svc
}

async fn serve(svc: Arc<Service>, access: Access) -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
    tokio::spawn(server::serve_http(svc, l, access));
    base
}

async fn call(base: &str, method: &str, path: &str, auth: Option<&str>, body: Option<Json>) -> (u16, Json) {
    let c = chloedb_server::http_client().build().unwrap();
    let mut r = c.request(method.parse().unwrap(), format!("{base}{path}"));
    if let Some(a) = auth {
        r = r.bearer_auth(a);
    }
    if let Some(b) = body {
        r = r.json(&b);
    }
    let r = r.send().await.unwrap();
    let status = r.status().as_u16();
    let text = r.text().await.unwrap();
    (status, serde_json::from_str(&text).unwrap_or(Json::String(text)))
}

#[tokio::test(flavor = "multi_thread")]
async fn a_key_limited_to_tables_touches_those_alone() {
    let dir = tempfile::tempdir().unwrap();
    let svc = Arc::new(service(dir.path()));
    let base = serve(svc.clone(), Access { token: Some(Secret::new("root-token")), ..Default::default() }).await;
    let root = Some("root-token");
    for t in ["crawl_docs", "secrets", "edges_secret"] {
        let (s, j) = call(&base, "POST", "/v1/sql", root, Some(json!({"sql": format!("CREATE TABLE {t} (id KEYWORD PRIMARY KEY, body TEXT)")}))).await;
        assert_eq!(s, 200, "{t}: {j}");
    }
    let (s, j) = call(&base, "POST", "/v1/write", root, Some(json!({"ops": [{"op": "upsert", "table": "secrets", "doc": {"id": "s1", "body": "the launch codes"}}]}))).await;
    assert_eq!(s, 200, "{j}");

    // Made by an admin; its scope is the most its tables give.
    let (s, made) = call(&base, "POST", "/v1/admin/keys", root, Some(json!({"name": "crawler", "tables": {"crawl_*": "admin"}}))).await;
    assert_eq!(s, 200, "{made}");
    assert_eq!(made["key"]["tables"], json!({"crawl_*": "admin"}));
    assert_eq!(made["key"]["scope"], "admin");
    let secret = made["secret"].as_str().unwrap().to_string();
    let key = Some(secret.as_str());
    // The keys file marks its digest, so that a server from before limits
    // never takes it.
    let file = std::fs::read_to_string(dir.path().join("a.strata.keys.json")).unwrap();
    assert!(file.contains("\"sha256\": \"t:"), "{file}");
    let bad = call(&base, "POST", "/v1/admin/keys", root, Some(json!({"name": "x", "tables": {"a.b": "read"}}))).await;
    assert_eq!(bad.0, 400, "{}", bad.1);

    // Who it is.
    let (s, me) = call(&base, "GET", "/v1/whoami", key, None).await;
    assert_eq!((s, &me["tables"]), (200, &json!({"crawl_*": "admin"})), "{me}");
    assert_eq!(me["auth"]["app_keys"], true);

    // Its own tables: created, written, read, dropped.
    let (s, j) = call(&base, "POST", "/v1/sql", key, Some(json!({"sql": "CREATE TABLE crawl_pages (id KEYWORD PRIMARY KEY, body TEXT)"}))).await;
    assert_eq!(s, 200, "{j}");
    let (s, j) = call(&base, "POST", "/v1/write", key, Some(json!({"ops": [{"op": "upsert", "table": "crawl_pages", "doc": {"id": "p1", "body": "hello"}}]}))).await;
    assert_eq!(s, 200, "{j}");
    let (s, j) = call(&base, "POST", "/v1/query", key, Some(json!({"sql": "SELECT id, body FROM crawl_pages"}))).await;
    assert_eq!((s, j["rows"].as_array().map(Vec::len)), (200, Some(1)), "{j}");
    let (s, j) = call(&base, "POST", "/v1/sql", key, Some(json!({"sql": "INSERT INTO crawl_docs (id, body) VALUES ('d1', 'x')"}))).await;
    assert_eq!(s, 200, "{j}");
    let (s, j) = call(&base, "GET", "/v1/kv/crawl_pages/p1", key, None).await;
    assert_eq!(s, 200, "{j}");
    // The listing shows its tables alone.
    let (s, j) = call(&base, "GET", "/v1/tables", key, None).await;
    assert_eq!(s, 200);
    let mut names: Vec<&str> = j["tables"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str().or(t["table"].as_str())).collect();
    names.sort();
    assert_eq!(names, ["crawl_docs", "crawl_pages"], "{j}");

    // Other tables, however they are named.
    for sql in [
        "SELECT * FROM secrets",
        "SELECT body FROM crawl_docs WHERE id IN (SELECT id FROM secrets)",
        "SELECT * FROM crawl_docs UNION ALL SELECT * FROM secrets",
        "WITH x AS (SELECT * FROM secrets) SELECT * FROM x",
        "SELECT * FROM \"SECRETS\"",
    ] {
        let (s, j) = call(&base, "POST", "/v1/query", key, Some(json!({"sql": sql}))).await;
        assert_eq!(s, 403, "{sql}: {j}");
        assert!(j.to_string().contains("secrets") || j.to_string().contains("SECRETS"), "{j}");
        let (s, _) = call(&base, "POST", "/v1/sql", key, Some(json!({"sql": sql}))).await;
        assert_eq!(s, 403, "{sql}");
        let (s, _) = call(&base, "POST", "/v1/explain", key, Some(json!({"sql": sql}))).await;
        assert_eq!(s, 403, "{sql}");
    }
    for sql in ["DROP TABLE secrets", "DELETE FROM secrets", "CREATE TABLE elsewhere (id KEYWORD PRIMARY KEY)", "UPDATE secrets SET body = 'x'"] {
        let (s, j) = call(&base, "POST", "/v1/sql", key, Some(json!({"sql": sql}))).await;
        assert_eq!(s, 403, "{sql}: {j}");
    }
    let writes = [
        json!({"ops": [{"op": "upsert", "table": "secrets", "doc": {"id": "s2", "body": "x"}}]}),
        json!({"ops": [{"op": "upsert", "table": "crawl_docs", "doc": {"id": "d2", "body": "x"}}, {"op": "delete", "table": "secrets", "key": "s1"}]}),
        // No table: the default table, which is not its.
        json!({"ops": [{"op": "upsert", "doc": {"id": "x"}}]}),
    ];
    for w in writes {
        let (s, j) = call(&base, "POST", "/v1/write", key, Some(w.clone())).await;
        assert_eq!(s, 403, "{w}: {j}");
    }
    assert_eq!(call(&base, "GET", "/v1/kv/secrets/s1", key, None).await.0, 403);
    assert_eq!(call(&base, "PUT", "/v1/kv/secrets/s1", key, Some(json!({"body": "x"}))).await.0, 403);
    assert_eq!(call(&base, "DELETE", "/v1/kv/secrets/s1", key, None).await.0, 403);
    assert_eq!(call(&base, "POST", "/v1/kv/get", key, Some(json!({"table": "secrets", "keys": ["s1"]}))).await.0, 403);
    assert_eq!(call(&base, "GET", "/v1/schema?table=secrets", key, None).await.0, 403);
    // The server's own admin, and routes that do not check tables.
    for (m, path, body) in [
        ("POST", "/v1/admin/keys", Some(json!({"name": "mine", "scope": "admin"}))),
        ("GET", "/v1/admin/keys", None),
        ("POST", "/v1/admin/vacuum", Some(json!({}))),
        ("GET", "/v1/export", None),
        ("GET", "/v1/feed?copy=true", None),
        ("GET", "/v1/queries/recent", None),
        ("POST", "/v1/plan", Some(json!({"plan": {"version": 1, "root": {"node": "search", "table": "secrets"}}}))),
        ("POST", "/v1/mcp", Some(json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))),
        ("GET", "/v1/functions", None),
        ("POST", "/v1/query", Some(json!({"sql": "SELECT * FROM crawl_docs", "log": {"table": "secrets_log"}}))),
    ] {
        let (s, j) = call(&base, m, path, key, body).await;
        assert_eq!(s, 403, "{m} {path}: {j}");
    }
    // Refusals are audited under its name.
    let (_, audit) = call(&base, "GET", "/v1/admin/audit?limit=200", root, None).await;
    assert!(audit.to_string().contains("\"who\":\"crawler\""), "{audit}");

    // Revoked, it opens nothing.
    let id = made["key"]["id"].as_str().unwrap();
    assert_eq!(call(&base, "DELETE", &format!("/v1/admin/keys/{id}"), root, None).await.0, 200);
    assert_eq!(call(&base, "POST", "/v1/query", key, Some(json!({"sql": "SELECT * FROM crawl_docs"}))).await.0, 401);
}

/// An app engine's key makes each app a key within its tables, over the
/// HTTP API: what the app's key reaches, the stored functions it may run
/// and define, and that it ends with the engine's key.
#[tokio::test(flavor = "multi_thread")]
async fn a_key_makes_its_apps_keys_and_they_end_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let svc = Arc::new(service(dir.path()));
    let base = serve(svc.clone(), Access { token: Some(Secret::new("root-token")), ..Default::default() }).await;
    let root = Some("root-token");
    for t in ["app_a1_tasks", "app_b2_tasks", "sa_people", "sa_numbers"] {
        let (s, j) = call(&base, "POST", "/v1/sql", root, Some(json!({"sql": format!("CREATE TABLE {t} (id KEYWORD PRIMARY KEY, body TEXT)")}))).await;
        assert_eq!(s, 200, "{t}: {j}");
    }
    let (s, j) = call(&base, "POST", "/v1/write", root, Some(json!({"ops": [{"op": "upsert", "table": "app_b2_tasks", "doc": {"id": "b", "body": "theirs"}}, {"op": "upsert", "table": "sa_numbers", "doc": {"id": "1"}}]}))).await;
    assert_eq!(s, 200, "{j}");
    let (_, who) = call(&base, "GET", "/v1/whoami", root, None).await;
    assert_eq!(who["auth"]["own_keys"], true);
    let (s, made) = call(&base, "POST", "/v1/admin/keys", root, Some(json!({"name": "apps-app", "tables": {"sa_*": "admin", "app_*": "admin"}}))).await;
    assert_eq!(s, 200, "{made}");
    let engine = made["secret"].as_str().unwrap().to_string();
    let e = Some(engine.as_str());

    // The engine makes app a1's key, within its own tables.
    let (s, a1) = call(&base, "POST", "/v1/keys/own", e, Some(json!({"name": "app-a1", "tables": {"app_a1_*": "write", "sa_numbers": "read"}}))).await;
    assert_eq!(s, 200, "{a1}");
    assert_eq!(a1["key"]["name"], "apps-app/app-a1");
    let app = a1["secret"].as_str().unwrap().to_string();
    let k = Some(app.as_str());
    let file = std::fs::read_to_string(dir.path().join("a.strata.keys.json")).unwrap();
    assert!(file.contains("\"sha256\": \"d:"), "{file}");
    for bad in [json!({"*": "read"}), json!({"crawl_x": "read"}), json!({"app_*": "admin", "other": "read"})] {
        let (s, j) = call(&base, "POST", "/v1/keys/own", e, Some(json!({"name": "x", "tables": bad}))).await;
        assert_eq!(s, 403, "{j}");
    }
    let (s, j) = call(&base, "POST", "/v1/keys/own", k, Some(json!({"name": "y", "tables": {"app_a1_x": "read"}}))).await;
    assert_eq!(s, 403, "a key so made makes none: {j}");
    let (s, j) = call(&base, "GET", "/v1/keys/own", e, None).await;
    assert_eq!((s, j["keys"].as_array().map(Vec::len)), (200, Some(1)), "{j}");

    // Its own rows, and nothing else.
    let (s, j) = call(&base, "POST", "/v1/write", k, Some(json!({"ops": [{"op": "upsert", "table": "app_a1_tasks", "doc": {"id": "a", "body": "mine"}}]}))).await;
    assert_eq!(s, 200, "{j}");
    let (s, j) = call(&base, "POST", "/v1/query", k, Some(json!({"sql": "SELECT body FROM app_a1_tasks"}))).await;
    assert_eq!((s, j["rows"].as_array().map(Vec::len)), (200, Some(1)), "{j}");
    for sql in ["SELECT * FROM app_b2_tasks", "SELECT * FROM sa_people", "SELECT * FROM app_a1_tasks WHERE id IN (SELECT id FROM app_b2_tasks)"] {
        let (s, j) = call(&base, "POST", "/v1/query", k, Some(json!({"sql": sql}))).await;
        assert_eq!(s, 403, "{sql}: {j}");
    }
    let (s, j) = call(&base, "POST", "/v1/write", k, Some(json!({"ops": [{"op": "upsert", "table": "app_b2_tasks", "doc": {"id": "x", "body": "no"}}]}))).await;
    assert_eq!(s, 403, "{j}");

    // The engine defines its functions with its key; the app runs a table
    // function whose body reads a table its key covers, and not one whose
    // body reads another app's.
    for f in ["CREATE FUNCTION nums(n INT DEFAULT 5) RETURNS TABLE AS SELECT id FROM sa_numbers LIMIT :n", "CREATE FUNCTION others() RETURNS TABLE AS SELECT id, body FROM app_b2_tasks LIMIT 10"] {
        let (s, j) = call(&base, "POST", "/v1/sql", e, Some(json!({"sql": f}))).await;
        assert_eq!(s, 200, "{f}: {j}");
        let (s, j) = call(&base, "POST", "/v1/sql", e, Some(json!({"sql": f}))).await;
        assert!(s == 200 || j.to_string().contains("exists"), "the same again: {j}");
    }
    let (s, j) = call(&base, "POST", "/v1/sql", e, Some(json!({"sql": "CREATE OR REPLACE FUNCTION nums(n INT DEFAULT 5) RETURNS TABLE AS SELECT id FROM sa_numbers LIMIT 1"}))).await;
    assert_eq!(s, 403, "a limited key replaces no function: {j}");
    let (s, j) = call(&base, "POST", "/v1/query", k, Some(json!({"sql": "SELECT * FROM nums(3)"}))).await;
    assert_eq!((s, j["rows"].as_array().map(Vec::len)), (200, Some(1)), "{j}");
    let (s, j) = call(&base, "POST", "/v1/query", k, Some(json!({"sql": "SELECT * FROM others()"}))).await;
    assert_eq!(s, 403, "{j}");

    // Revoking the engine's key ends the app's.
    let id = made["key"]["id"].as_str().unwrap();
    let (s, _) = call(&base, "DELETE", &format!("/v1/admin/keys/{id}"), root, None).await;
    assert_eq!(s, 200);
    let (s, _) = call(&base, "POST", "/v1/query", k, Some(json!({"sql": "SELECT body FROM app_a1_tasks"}))).await;
    assert_eq!(s, 401);
    // The token makes no key of its own here.
    let (s, _) = call(&base, "POST", "/v1/keys/own", root, Some(json!({"name": "z", "tables": {"a": "read"}}))).await;
    assert_eq!(s, 403);
}

/// The lines of `path`'s feed, read with `auth`, until one is transaction
/// `upto`.
async fn feed_lines(base: &str, path: &str, auth: &str, upto: u64) -> Vec<Json> {
    let c = chloedb_server::http_client().build().unwrap();
    let mut r = c.get(format!("{base}{path}")).bearer_auth(auth).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let (mut out, mut buf) = (Vec::new(), Vec::new());
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let chunk = tokio::time::timeout_at(deadline, r.chunk()).await.expect("the feed reaches the last transaction").unwrap().expect("the feed goes on");
        buf.extend_from_slice(&chunk);
        while let Some(i) = buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = buf.drain(..=i).collect();
            let j: Json = serde_json::from_slice(&line).unwrap();
            let done = j["seq"].as_u64() == Some(upto);
            out.push(j);
            if done {
                return out;
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_limited_key_follows_the_feed_of_its_tables_alone() {
    let dir = tempfile::tempdir().unwrap();
    let opts = DbOptions { publish_snapshots: true, ship_log_bytes: 16 << 20, ..DbOptions::default() };
    let db = Db::create(&dir.path().join("a.strata"), demo::schema(10), opts).unwrap();
    db.storage().start_seq_no().unwrap();
    let schema = (*db.schema()).clone();
    let agent = AgentConfig { api_key: None, model: String::new(), base_url: String::new() };
    let mut svc = Service::new(Backend::Local(Arc::new(db)), schema, agent, 16, Duration::from_secs(10));
    svc.keys = Arc::new(Keys::open(dir.path().join("a.strata.keys.json")).unwrap());
    let svc = Arc::new(svc);
    let base = serve(svc.clone(), Access { token: Some(Secret::new("root-token")), ..Default::default() }).await;
    let root = Some("root-token");
    for t in ["app_a1_tasks", "app_b2_tasks"] {
        let (s, j) = call(&base, "POST", "/v1/sql", root, Some(json!({"sql": format!("CREATE TABLE {t} (id KEYWORD PRIMARY KEY, body TEXT)")}))).await;
        assert_eq!(s, 200, "{t}: {j}");
    }
    // One transaction of both apps' rows, then one of the other's alone.
    let both = json!({"ops": [{"op": "upsert", "table": "app_a1_tasks", "doc": {"id": "a", "body": "mine"}}, {"op": "upsert", "table": "app_b2_tasks", "doc": {"id": "b", "body": "theirs"}}]});
    let (s, j) = call(&base, "POST", "/v1/write", root, Some(both)).await;
    assert_eq!(s, 200, "{j}");
    let (s, j) = call(&base, "POST", "/v1/write", root, Some(json!({"ops": [{"op": "delete", "table": "app_b2_tasks", "key": "b"}]}))).await;
    assert_eq!(s, 200, "{j}");
    let last = svc.local_db().unwrap().storage().numbering().unwrap().seq;
    let (s, made) = call(&base, "POST", "/v1/admin/keys", root, Some(json!({"name": "a1", "tables": {"app_a1_*": "read"}}))).await;
    assert_eq!(s, 200, "{made}");
    let key = made["secret"].as_str().unwrap().to_string();
    let (_, who) = call(&base, "GET", "/v1/whoami", Some(&key), None).await;
    assert_eq!(who["auth"]["limited_feed"], true);

    // The token reads every table's operations; the key, its own tables'.
    let all = feed_lines(&base, "/v1/feed?from=1", "root-token", last).await;
    let mine = feed_lines(&base, "/v1/feed?from=1", &key, last).await;
    let tables = |lines: &[Json]| -> Vec<String> { lines.iter().flat_map(|l| l["ops"].as_array().cloned().unwrap_or_default()).filter_map(|o| o["table"].as_str().map(String::from)).collect() };
    assert!(tables(&all).iter().any(|t| t == "app_b2_tasks"), "{all:?}");
    let seen = tables(&mine);
    assert!(seen.iter().any(|t| t == "app_a1_tasks"), "{mine:?}");
    assert!(seen.iter().all(|t| t == "app_a1_tasks"), "{mine:?}");
    assert!(!format!("{mine:?}").contains("theirs"), "{mine:?}");
    // Every transaction is still counted, with no operations when none is
    // of its tables.
    let seqs = |lines: &[Json]| -> Vec<u64> { lines.iter().filter_map(|l| l["seq"].as_u64()).collect() };
    assert_eq!(seqs(&mine), seqs(&all));
    assert_eq!(mine.last().unwrap()["ops"], json!([]), "{mine:?}");
    assert!(mine[0].get("feed").is_some(), "{mine:?}");
    // A copy is of every table: refused.
    let (s, j) = call(&base, "GET", "/v1/feed?copy=true", Some(&key), None).await;
    assert_eq!(s, 403, "{j}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_read_grant_reads_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let svc = Arc::new(service(dir.path()));
    let base = serve(svc.clone(), Access { token: Some(Secret::new("root-token")), ..Default::default() }).await;
    let root = Some("root-token");
    for t in ["sheets_books", "orders"] {
        assert_eq!(call(&base, "POST", "/v1/sql", root, Some(json!({"sql": format!("CREATE TABLE {t} (id KEYWORD PRIMARY KEY, body TEXT)")}))).await.0, 200);
    }
    let (_, made) = call(&base, "POST", "/v1/admin/keys", root, Some(json!({"name": "sheets", "tables": ["sheets_*=admin", "*=read"]}))).await;
    let key = made["secret"].as_str().map(String::from);
    let key = key.as_deref();
    assert!(key.is_some(), "{made}");
    assert_eq!(call(&base, "POST", "/v1/query", key, Some(json!({"sql": "SELECT * FROM orders"}))).await.0, 200);
    assert_eq!(call(&base, "POST", "/v1/write", key, Some(json!({"ops": [{"op": "upsert", "table": "sheets_books", "doc": {"id": "b"}}]}))).await.0, 200);
    let (s, j) = call(&base, "POST", "/v1/write", key, Some(json!({"ops": [{"op": "upsert", "table": "orders", "doc": {"id": "o"}}]}))).await;
    assert_eq!(s, 403, "{j}");
    assert!(j.to_string().contains("write"), "{j}");
    assert_eq!(call(&base, "POST", "/v1/sql", key, Some(json!({"sql": "DROP TABLE orders"}))).await.0, 403);
    // A scope given caps its grants.
    let (_, capped) = call(&base, "POST", "/v1/admin/keys", root, Some(json!({"name": "capped", "scope": "read", "tables": {"sheets_*": "admin"}}))).await;
    assert_eq!(capped["key"]["scope"], "read", "{capped}");
    let c = capped["secret"].as_str().map(String::from);
    assert_eq!(call(&base, "POST", "/v1/write", c.as_deref(), Some(json!({"ops": [{"op": "upsert", "table": "sheets_books", "doc": {"id": "c"}}]}))).await.0, 403);
}

/// The key a server makes for an app it starts: in memory, limited, and
/// no reason to close an open server.
#[tokio::test(flavor = "multi_thread")]
async fn an_apps_key_is_the_servers_alone() {
    let dir = tempfile::tempdir().unwrap();
    let svc = Arc::new(service(dir.path()));
    let base = serve(svc.clone(), Access::default()).await;
    assert_eq!(call(&base, "POST", "/v1/sql", None, Some(json!({"sql": "CREATE TABLE crawl_a (id KEYWORD PRIMARY KEY)"}))).await.0, 200);
    let secret = svc.keys.app_key("crawler", vec![Grant::parse("crawl_*=admin").unwrap()]).unwrap();
    let key = Some(secret.as_str());
    // The server stays open, but the app's key is the app's.
    assert_eq!(call(&base, "GET", "/v1/admin/keys", None, None).await.0, 200);
    let (s, me) = call(&base, "GET", "/v1/whoami", key, None).await;
    assert_eq!((s, me["name"].as_str()), (200, Some("app:crawler")), "{me}");
    assert_eq!(call(&base, "POST", "/v1/sql", key, Some(json!({"sql": "CREATE TABLE elsewhere (id KEYWORD PRIMARY KEY)"}))).await.0, 403);
    assert_eq!(call(&base, "POST", "/v1/write", key, Some(json!({"ops": [{"op": "upsert", "table": "crawl_a", "doc": {"id": "1"}}]}))).await.0, 200);
    // Listed, but kept nowhere and given to no other server.
    let (_, listed) = call(&base, "GET", "/v1/admin/keys", None, None).await;
    let app = listed["keys"].as_array().unwrap().iter().find(|k| k["name"] == "app:crawler").cloned().unwrap();
    assert_eq!((app["ephemeral"].as_bool(), app["owner"].as_str()), (Some(true), Some("app:crawler")), "{listed}");
    assert!(svc.keys.set().keys.is_empty() && !dir.path().join("a.strata.keys.json").exists());
    // The app's next start: a new key, the old one gone.
    let again = svc.keys.app_key("crawler", vec![Grant::parse("crawl_*=write").unwrap()]).unwrap();
    assert_eq!(call(&base, "GET", "/v1/whoami", key, None).await.1["name"], "open");
    assert_eq!(call(&base, "GET", "/v1/whoami", Some(&again), None).await.1["name"], "app:crawler");
    assert!(svc.keys.end_app_key("crawler"));
    assert_eq!(call(&base, "GET", "/v1/whoami", Some(&again), None).await.1["name"], "open");
}

/// An app's own token, from its variable or a file of the token folder,
/// and the caller as the server checked it; a caller's own headers of
/// those names are not passed on.
#[tokio::test(flavor = "multi_thread")]
async fn an_app_is_sent_its_own_token_and_who_called() {
    let seen: Arc<Mutex<Vec<HeaderMap>>> = Arc::default();
    let s = seen.clone();
    let app = Router::new().fallback(move |h: HeaderMap| {
        let s = s.clone();
        async move {
            s.lock().unwrap().push(h);
            axum::Json(json!({"ok": true}))
        }
    });
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let app_url = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });

    let dir = tempfile::tempdir().unwrap();
    let tokens = dir.path().join("tokens");
    std::fs::create_dir(&tokens).unwrap();
    let mut svc = service(dir.path());
    let list = parse_with(&[format!("crawler={app_url}"), format!("sheets={app_url}")], |v| (v == "CHLOEDB_APP_TOKEN_SHEETS").then(|| "sheets-own".to_string())).unwrap();
    svc.apps = Apps::new(list, Some("shared".into())).with_token_dir(Some(tokens.clone()));
    let base = serve(Arc::new(svc), Access { token: Some(Secret::new("root-token")), ..Default::default() }).await;
    let (_, made) = call(&base, "POST", "/v1/admin/keys", Some("root-token"), Some(json!({"name": "ana", "scope": "read"}))).await;
    let ana = made["secret"].as_str().unwrap().to_string();

    let c = chloedb_server::http_client().build().unwrap();
    let get = |path: &str| c.get(format!("{base}{path}")).bearer_auth(&ana).header("x-chloedb-principal", "root").header("x-chloedb-scope", "admin").send();
    assert_eq!(get("/v1/apps/sheets/x").await.unwrap().status(), 200);
    assert_eq!(get("/v1/apps/crawler/x").await.unwrap().status(), 200);
    std::fs::write(tokens.join("crawler"), "crawler-own\n").unwrap();
    assert_eq!(get("/v1/apps/crawler/y").await.unwrap().status(), 200);
    let seen = seen.lock().unwrap();
    let h = |i: usize, name: &str| seen[i].get_all(name).iter().map(|v| v.to_str().unwrap().to_string()).collect::<Vec<_>>();
    assert_eq!(h(0, "authorization"), ["Bearer sheets-own"]);
    assert_eq!(h(1, "authorization"), ["Bearer shared"]);
    assert_eq!(h(2, "authorization"), ["Bearer crawler-own"]);
    for i in 0..3 {
        assert_eq!(h(i, "x-chloedb-principal"), ["ana"]);
        assert_eq!(h(i, "x-chloedb-scope"), ["read"]);
        assert_eq!(h(i, "x-chloedb-key"), [made["key"]["id"].as_str().unwrap().to_string()]);
    }
}

/// What a request carries besides its statement or its rows: a stored
/// vector of another table as a parameter (audit N-D1), a stream's offset
/// (N-D3), and the proposals an admin applies (N-D2).
#[tokio::test(flavor = "multi_thread")]
async fn a_limited_key_reaches_no_other_table_through_params_marks_or_proposals() {
    let dir = tempfile::tempdir().unwrap();
    let svc = Arc::new(service(dir.path()));
    let base = serve(svc.clone(), Access { token: Some(Secret::new("root-token")), ..Default::default() }).await;
    let root = Some("root-token");
    for t in ["crawl_x", "secrets"] {
        let (s, j) = call(&base, "POST", "/v1/sql", root, Some(json!({"sql": format!("CREATE TABLE {t} (id KEYWORD PRIMARY KEY, emb VECTOR(2))")}))).await;
        assert_eq!(s, 200, "{t}: {j}");
    }
    let (s, j) = call(&base, "POST", "/v1/write", root, Some(json!({"ops": [{"op": "upsert", "table": "secrets", "doc": {"id": "s1", "emb": [0.25, 0.75]}}, {"op": "upsert", "table": "crawl_x", "doc": {"id": "c1", "emb": [1.0, 0.0]}}]}))).await;
    assert_eq!(s, 200, "{j}");
    let (_, made) = call(&base, "POST", "/v1/admin/keys", root, Some(json!({"name": "crawler", "tables": {"crawl_*": "admin"}}))).await;
    let secret = made["secret"].as_str().unwrap().to_string();
    let key = Some(secret.as_str());

    // A vector of another table, by name or by its field alone.
    let other = json!({"v": {"vector_of": "s1", "table": "secrets", "field": "emb"}});
    for (path, sql) in [("/v1/sql", "INSERT INTO crawl_x (id, emb) VALUES ('a', :v)"), ("/v1/query", "SELECT id FROM crawl_x ORDER BY similarity(emb, :v) DESC"), ("/v1/explain", "SELECT id FROM crawl_x ORDER BY similarity(emb, :v) DESC")] {
        let (s, j) = call(&base, "POST", path, key, Some(json!({"sql": sql, "params": other}))).await;
        assert_eq!(s, 403, "{path}: {j}");
        assert!(j.to_string().contains("secrets"), "{j}");
        let (s, j) = call(&base, "POST", path, key, Some(json!({"sql": sql, "params": {"v": {"vector_of": "s1", "field": "emb"}}}))).await;
        assert_eq!(s, 400, "{path}, no table: {j}");
    }
    assert_eq!(call(&base, "GET", "/v1/kv/crawl_x/a", key, None).await.0, 404, "nothing was written");
    // Its own table's vectors, as before; the root's, as before.
    let own = json!({"v": {"vector_of": "c1", "table": "crawl_x", "field": "emb"}});
    let (s, j) = call(&base, "POST", "/v1/query", key, Some(json!({"sql": "SELECT id FROM crawl_x ORDER BY similarity(emb, :v) DESC", "params": own}))).await;
    assert_eq!(s, 200, "{j}");
    let (s, j) = call(&base, "POST", "/v1/query", root, Some(json!({"sql": "SELECT id FROM crawl_x ORDER BY similarity(emb, :v) DESC", "params": other}))).await;
    assert_eq!(s, 200, "{j}");

    // A stream's offset, on a row of its own table or alone.
    for op in [
        json!({"op": "upsert", "table": "crawl_x", "doc": {"id": "m"}, "source": {"name": "orders-0", "offset": i64::MAX}}),
        json!({"op": "source", "table": "crawl_x", "source": {"name": "orders-0", "offset": i64::MAX}}),
    ] {
        let (s, j) = call(&base, "POST", "/v1/write", key, Some(json!({"ops": [op]}))).await;
        assert_eq!(s, 403, "{j}");
    }
    let (s, j) = call(&base, "POST", "/v1/write", root, Some(json!({"ops": [{"op": "upsert", "table": "secrets", "doc": {"id": "s2"}, "source": {"name": "orders-0", "offset": 1}}]}))).await;
    assert_eq!((s, j["skipped"].as_u64().unwrap_or(0)), (200, 0), "the stream goes on: {j}");

    // The proposals table: a grant by a pattern reads it, writes it never.
    let (s, j) = call(&base, "POST", "/v1/sql", root, Some(json!({"sql": chloedb::autopilot::proposals_sql(chloedb::autopilot::PROPOSALS).unwrap()}))).await;
    assert_eq!(s, 200, "{j}");
    let (_, all) = call(&base, "POST", "/v1/admin/keys", root, Some(json!({"name": "everything", "tables": {"*": "admin"}}))).await;
    let all = all["secret"].as_str().map(String::from);
    let row = json!({"ops": [{"op": "upsert", "table": "relevance_proposals", "doc": {"id": "p1", "target": "secrets", "profile": "p", "ts": "2026-11-13T00:00:00Z", "status": "open", "verdict": "propose", "statement": "DROP TABLE secrets"}}]});
    let (s, j) = call(&base, "POST", "/v1/write", all.as_deref(), Some(row.clone())).await;
    assert_eq!(s, 403, "{j}");
    assert_eq!(call(&base, "POST", "/v1/query", all.as_deref(), Some(json!({"sql": "SELECT id FROM relevance_proposals"}))).await.0, 200);
    let (_, named) = call(&base, "POST", "/v1/admin/keys", root, Some(json!({"name": "named", "tables": {"relevance_proposals": "write"}}))).await;
    let (s, j) = call(&base, "POST", "/v1/write", named["secret"].as_str(), Some(row)).await;
    assert_eq!(s, 200, "a grant that names it writes it: {j}");
}
