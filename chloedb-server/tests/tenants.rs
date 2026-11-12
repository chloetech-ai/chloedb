//! Tenants (`--tenants`, experimental): databases kept in an object store,
//! served under `/v1/admin/tenants`. Two servers on one store answer for
//! the same tenants; what one commits the other reads with `latest`; of
//! two writing one tenant the store refuses the later (409), and the
//! write goes through when tried again; the routes are an admin's.

use std::sync::Arc;
use std::time::Duration;

use chloedb::db::Db;
use chloedb::storage::DbOptions;
use chloedb_server::auth::{Access, Secret};
use chloedb_server::keys::Keys;
use chloedb_server::service::{AgentConfig, Backend, Service};
use chloedb_server::tenants::Tenants;
use chloedb_server::{demo, server};
use serde_json::{json, Value as Json};
use tokio::net::TcpListener;

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

/// A server named `name` whose tenants are those of `store` (none when
/// `None`), and its address.
async fn serve(dir: &std::path::Path, name: &str, store: Option<&std::path::Path>) -> String {
    let db = Db::create(&dir.join(format!("{name}.strata")), demo::schema(10), DbOptions::default()).unwrap();
    let schema = (*db.schema()).clone();
    let agent = AgentConfig { api_key: None, model: String::new(), base_url: String::new() };
    let mut svc = Service::new(Backend::Local(Arc::new(db)), schema, agent, 16, Duration::from_secs(10));
    svc.keys = Arc::new(Keys::open(dir.join(format!("{name}.strata.keys.json"))).unwrap());
    svc.tenants = store.map(|s| Arc::new(Tenants::open(s.to_str().unwrap(), dir.join(format!("{name}-cache"))).unwrap()));
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
    tokio::spawn(server::serve_http(Arc::new(svc), l, Access { token: Some(Secret::new("root-token")), ..Default::default() }));
    base
}

#[tokio::test(flavor = "multi_thread")]
async fn two_servers_on_one_store_serve_the_same_tenants() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("store");
    let a = serve(dir.path(), "a", Some(&store)).await;
    let b = serve(dir.path(), "b", Some(&store)).await;
    let root = Some("root-token");
    let sql = |s: &str| Some(json!({"sql": s}));

    // An admin's routes.
    assert_eq!(call(&a, "GET", "/v1/admin/tenants/agent-7", None, None).await.0, 401);
    let (s, key) = call(&a, "POST", "/v1/admin/keys", root, Some(json!({"name": "writer", "scope": "write"}))).await;
    assert_eq!(s, 200, "{key}");
    assert_eq!(call(&a, "POST", "/v1/admin/tenants", key["secret"].as_str(), Some(json!({"id": "agent-7"}))).await.0, 403);

    let (s, j) = call(&a, "POST", "/v1/admin/tenants", root, Some(json!({"id": "agent-7", "sql": ["CREATE TABLE notes (id INT PRIMARY KEY, body TEXT)"]}))).await;
    assert_eq!(s, 200, "{j}");
    assert_eq!((&j["id"], &j["epoch"], &j["experimental"], &j["writer_here"]), (&json!("agent-7"), &json!(1), &json!(true), &json!(true)));
    assert_eq!(call(&a, "POST", "/v1/admin/tenants", root, Some(json!({"id": "agent-7"}))).await.0, 409);
    assert_eq!(call(&b, "POST", "/v1/admin/tenants", root, Some(json!({"id": "agent-7"}))).await.0, 409);
    assert_eq!(call(&a, "POST", "/v1/admin/tenants", root, Some(json!({"id": "../x"}))).await.0, 400);
    assert_eq!(call(&a, "GET", "/v1/admin/tenants/agent-8", root, None).await.0, 404);

    let (s, j) = call(&a, "POST", "/v1/admin/tenants/agent-7/sql", root, sql("INSERT INTO notes VALUES (1, 'the harbour at dawn')")).await;
    assert_eq!((s, &j["written"]), (200, &json!(1)), "{j}");

    // The other server, which has nothing of the tenant, answers for it.
    let (s, j) = call(&b, "POST", "/v1/admin/tenants/agent-7/sql", root, Some(json!({"sql": "SELECT id FROM notes WHERE MATCH(body, :q)", "params": {"q": "harbour"}}))).await;
    assert_eq!((s, &j["rows"]), (200, &json!([[1]])), "{j}");

    // It writes: the tenant is its to write from then on,
    let (s, j) = call(&b, "POST", "/v1/admin/tenants/agent-7/sql", root, sql("INSERT INTO notes VALUES (2, 'two')")).await;
    assert_eq!(s, 200, "{j}");
    assert_eq!(call(&b, "GET", "/v1/admin/tenants/agent-7", root, None).await.1["epoch"], 2);
    // and the first server, which has not heard, is refused, with nothing
    // written; trying again goes through.
    let third = sql("INSERT INTO notes VALUES (3, 'three')");
    let (s, j) = call(&a, "POST", "/v1/admin/tenants/agent-7/sql", root, third.clone()).await;
    assert_eq!(s, 409, "{j}");
    let (s, j) = call(&a, "POST", "/v1/admin/tenants/agent-7/sql", root, third).await;
    assert_eq!(s, 200, "{j}");

    // B reads its copy, or the store's latest when asked.
    let count = json!({"sql": "SELECT count(*) AS n FROM notes", "objects": true});
    assert_eq!(call(&b, "POST", "/v1/admin/tenants/agent-7/sql", root, Some(count.clone())).await.1["rows"], json!([{"n": 2}]));
    let latest = json!({"sql": "SELECT count(*) AS n FROM notes", "objects": true, "latest": true});
    assert_eq!(call(&b, "POST", "/v1/admin/tenants/agent-7/sql", root, Some(latest)).await.1["rows"], json!([{"n": 3}]));

    // A statement that does not parse is the caller's error.
    assert_eq!(call(&a, "POST", "/v1/admin/tenants/agent-7/sql", root, sql("SELEC 1")).await.0, 400);

    let (s, j) = call(&a, "POST", "/v1/admin/tenants/agent-7/base", root, None).await;
    assert_eq!(s, 200, "{j}");
    assert_eq!((&j["last_seq"], j["logs"].as_array().map(Vec::len)), (&j["base"]["seq"], Some(0)));

    // Deleted on one server, it is gone on both.
    assert_eq!(call(&a, "DELETE", "/v1/admin/tenants/agent-7", root, None).await.0, 200);
    assert_eq!(call(&a, "DELETE", "/v1/admin/tenants/agent-7", root, None).await.0, 404);
    assert_eq!(call(&b, "POST", "/v1/admin/tenants/agent-7/sql", root, Some(json!({"sql": "SELECT 1", "latest": true}))).await.0, 404);
    assert_eq!(call(&b, "GET", "/v1/admin/tenants/agent-7", root, None).await.0, 404);

    // A server started without --tenants has no such routes to answer.
    let plain = serve(dir.path(), "plain", None).await;
    let (s, j) = call(&plain, "GET", "/v1/admin/tenants/agent-7", root, None).await;
    assert_eq!(s, 404, "{j}");
    assert!(j["error"].as_str().unwrap_or_default().contains("--tenants"), "{j}");
}
