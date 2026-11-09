//! Apps (`--app name=url`): the server lists them with their manifests,
//! and forwards `/v1/apps/<name>/...` to them with its own app token in
//! place of the caller's key, a read key for `GET` and an admin key for
//! the rest.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Bytes;
use axum::http::{HeaderMap, Method, Uri};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use serde_json::{json, Value as Json};
use chloedb::db::Db;
use chloedb::storage::DbOptions;
use chloedb_server::apps::{parse, Apps};
use chloedb_server::auth::{Access, Secret};
use chloedb_server::keys::Keys;
use chloedb_server::service::{AgentConfig, Backend, Service};
use chloedb_server::{demo, server};
use tokio::net::TcpListener;

/// What the app was asked: method, path and query, its bearer token, body.
type Seen = Arc<Mutex<Vec<(String, String, String, String)>>>;

async fn fake_app() -> (String, Seen) {
    let seen: Seen = Arc::default();
    let s = seen.clone();
    let app = Router::new()
        .route(
            "/app.json",
            get(|| async { axum::Json(json!({"name": "crawler", "title": "Crawler", "description": "a test app", "icon_svg": "<circle cx=\"12\" cy=\"12\" r=\"9\"/>", "view": "ui/view.js", "version": "9.9"})) }),
        )
        .fallback(move |method: Method, uri: Uri, headers: HeaderMap, body: Bytes| {
            let s = s.clone();
            async move {
                let auth = headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
                s.lock().unwrap().push((method.to_string(), uri.to_string(), auth, String::from_utf8_lossy(&body).into_owned()));
                if uri.path() == "/ui/view.js" {
                    return ([("content-type", "text/javascript")], "export default { render() {} };").into_response();
                }
                ([("content-type", "application/json")], json!({"ok": true, "path": uri.path()}).to_string()).into_response()
            }
        });
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (url, seen)
}

async fn call(base: &str, method: &str, path: &str, auth: Option<&str>, body: Option<Json>) -> (u16, String) {
    let c = chloedb_server::http_client().build().unwrap();
    let mut r = c.request(method.parse().unwrap(), format!("{base}{path}"));
    if let Some(a) = auth {
        r = r.bearer_auth(a);
    }
    if let Some(b) = body {
        r = r.json(&b);
    }
    let r = r.send().await.unwrap();
    (r.status().as_u16(), r.text().await.unwrap())
}

#[tokio::test(flavor = "multi_thread")]
async fn apps_are_listed_and_forwarded_with_the_servers_token() {
    let (app_url, seen) = fake_app().await;
    let dir = tempfile::tempdir().unwrap();
    let db = Db::create(&dir.path().join("a.strata"), demo::schema(10), DbOptions::default()).unwrap();
    let schema = (*db.schema()).clone();
    let agent = AgentConfig { api_key: None, model: String::new(), base_url: String::new() };
    let mut svc = Service::new(Backend::Local(Arc::new(db)), schema, agent, 16, Duration::from_secs(10));
    svc.keys = Arc::new(Keys::open(dir.path().join("a.strata.keys.json")).unwrap());
    // One app that answers, one that does not.
    svc.apps = Apps::new(parse(&[format!("crawler={app_url}/"), "gone=http://127.0.0.1:9".into()]).unwrap(), Some("app-secret".into()));
    let svc = Arc::new(svc);
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
    tokio::spawn(server::serve_http(svc, l, Access { token: Some(Secret::new("root-token")), ..Default::default() }));
    let root = Some("root-token");
    let (s, j) = call(&base, "POST", "/v1/admin/keys", root, Some(json!({"name": "viewer", "scope": "read"}))).await;
    assert_eq!(s, 200, "{j}");
    let reader: Json = serde_json::from_str(&j).unwrap();
    let reader = reader["secret"].as_str().unwrap().to_string();
    let read = Some(reader.as_str());

    // The listing: each app's manifest, or why it does not answer.
    assert_eq!(call(&base, "GET", "/v1/apps", None, None).await.0, 401);
    let (s, j) = call(&base, "GET", "/v1/apps", read, None).await;
    assert_eq!(s, 200, "{j}");
    let j: Json = serde_json::from_str(&j).unwrap();
    let apps = j["apps"].as_array().unwrap();
    assert_eq!(apps.len(), 2);
    assert_eq!(apps[0]["name"], "crawler");
    assert_eq!(apps[0]["up"], true);
    assert_eq!(apps[0]["title"], "Crawler");
    assert_eq!(apps[0]["view"], "/v1/apps/crawler/ui/view.js");
    assert_eq!(apps[1]["name"], "gone");
    assert_eq!(apps[1]["up"], false);
    assert!(apps[1]["error"].as_str().unwrap().contains("does not answer"), "{}", apps[1]);

    // A read key reads through it; the app sees the server's token, never the key.
    let (s, body) = call(&base, "GET", "/v1/apps/crawler/ui/view.js", read, None).await;
    assert_eq!((s, body.as_str()), (200, "export default { render() {} };"));
    let (s, body) = call(&base, "GET", "/v1/apps/crawler/v1/crawls/c1?x=1", read, None).await;
    assert_eq!(s, 200, "{body}");
    {
        let seen = seen.lock().unwrap();
        assert_eq!(seen[0], ("GET".into(), "/ui/view.js".into(), "Bearer app-secret".into(), String::new()));
        assert_eq!(seen[1], ("GET".into(), "/v1/crawls/c1?x=1".into(), "Bearer app-secret".into(), String::new()));
        assert!(seen.iter().all(|(.., auth, _)| !auth.contains(&reader) && !auth.contains("root-token")));
    }

    // Changes need an admin key.
    let start = json!({"seeds": ["https://example.com/"]});
    assert_eq!(call(&base, "POST", "/v1/apps/crawler/v1/crawls", read, Some(start.clone())).await.0, 403);
    assert_eq!(call(&base, "DELETE", "/v1/apps/crawler/v1/crawls/c1", read, None).await.0, 403);
    let (s, body) = call(&base, "POST", "/v1/apps/crawler/v1/crawls", root, Some(start.clone())).await;
    assert_eq!(s, 200, "{body}");
    assert_eq!(serde_json::from_str::<Json>(&body).unwrap()["path"], "/v1/crawls");
    {
        let seen = seen.lock().unwrap();
        let last = seen.last().unwrap();
        assert_eq!((last.0.as_str(), last.1.as_str(), last.2.as_str()), ("POST", "/v1/crawls", "Bearer app-secret"));
        assert_eq!(serde_json::from_str::<Json>(&last.3).unwrap(), start);
    }

    // No such app; an app that does not answer.
    let (s, body) = call(&base, "GET", "/v1/apps/nope/x", read, None).await;
    assert_eq!(s, 404, "{body}");
    assert!(body.contains("crawler, gone"), "{body}");
    let (s, body) = call(&base, "GET", "/v1/apps/gone/x", read, None).await;
    assert_eq!(s, 502, "{body}");
}
