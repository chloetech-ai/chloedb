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
        ("GET", "/v1/feed", None),
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
