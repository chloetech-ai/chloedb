//! The command center's feedback ([`chloedb_server::feedback`]): an item
//! sent with its screenshot and marks becomes a row of cc_feedback, which
//! a read key lists and reads (its picture too, served inert), a write key
//! triages (`if_status` refusing a second taker), an admin deletes; a key
//! limited to tables needs a grant on the table; the MCP tools answer the
//! screenshot as an image; and through a coordinator the same.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use chloedb::db::Db;
use chloedb::storage::DbOptions;
use chloedb_server::auth::{Access, Secret};
use chloedb_server::coord::Coordinator;
use chloedb_server::keys::Keys;
use chloedb_server::service::{AgentConfig, Backend, Service};
use chloedb_server::{demo, server};
use serde_json::{json, Value as Json};
use tokio::net::TcpListener;

fn agent() -> AgentConfig {
    AgentConfig { api_key: None, model: String::new(), base_url: String::new() }
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

/// A real PNG of 2 by 2 pixels.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x02, 0x08, 0x02, 0x00, 0x00, 0x00, 0xfd, 0xd4, 0x9a,
    0x73, 0x00, 0x00, 0x00, 0x16, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xc0, 0xf0, 0x9f, 0x81, 0x81, 0xe1, 0x3f, 0x03, 0x03, 0x00, 0x2a, 0xf4, 0x05, 0xfb, 0x0f, 0x6e, 0x34,
    0x4b, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

fn png_url() -> String {
    format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(PNG))
}

fn item() -> Json {
    json!({
        "kind": "bug", "note": "The Add shard button is too small on a phone.\nAnd 2 should say when it started.",
        "page": "cluster", "route": "#/cluster", "url": "http://example/#/cluster",
        "shot": png_url(), "thumb": png_url(), "shot_w": 2, "shot_h": 2,
        "marks": [
            {"n": 1, "tool": "box", "x": 10, "y": 20, "w": 30, "h": 40, "color": "#e5484d", "text": "too small", "target": {"selector": "button.btn", "text": "Add shard", "card": "Shards"}},
            {"n": 2, "tool": "arrow", "x": 0, "y": 0, "x2": 50, "y2": 60, "text": "when did this start"},
            {"n": 0, "tool": "redact", "x": 1, "y": 1, "w": 9, "h": 9, "target": {"text": "sk_live_should_not_be_kept"}}
        ],
        "context": {"viewport": {"w": 390, "h": 844, "dpr": 3}, "theme": "dark", "errors": [{"at": "2026-11-13T00:00:00Z", "msg": "TypeError: x is undefined"}]}
    })
}

async fn key(base: &str, root: &str, body: Json) -> String {
    let (s, made) = call(base, "POST", "/v1/admin/keys", Some(root), Some(body)).await;
    assert_eq!(s, 200, "{made}");
    made["secret"].as_str().unwrap().to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn feedback_is_sent_listed_triaged_and_read_by_agents() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::create(&dir.path().join("a.strata"), demo::schema(10), DbOptions::default()).unwrap();
    let schema = (*db.schema()).clone();
    let mut svc = Service::new(Backend::Local(Arc::new(db)), schema, agent(), 16, Duration::from_secs(10));
    svc.keys = Arc::new(Keys::open(dir.path().join("a.strata.keys.json")).unwrap());
    let base = serve(Arc::new(svc), Access { token: Some(Secret::new("root-token")), ..Default::default() }).await;
    let root = "root-token";
    let read = key(&base, root, json!({"name": "viewer", "scope": "read"})).await;
    let write = key(&base, root, json!({"name": "roberto", "scope": "write"})).await;
    let granted = key(&base, root, json!({"name": "triage-agent", "tables": {"cc_feedback": "write"}})).await;
    let other = key(&base, root, json!({"name": "crawler", "tables": {"crawl_*": "admin"}})).await;

    // Nothing yet: an empty list, saying the table is not made.
    let (s, j) = call(&base, "GET", "/v1/feedback", Some(&read), None).await;
    assert_eq!((s, &j["exists"], &j["feedback"]), (200, &json!(false), &json!([])), "{j}");
    assert_eq!(j["table"], "cc_feedback");

    // A read key may not send; a write key may, and the table is made for it.
    assert_eq!(call(&base, "POST", "/v1/feedback", Some(&read), Some(item())).await.0, 403);
    let (s, sent) = call(&base, "POST", "/v1/feedback", Some(&write), Some(item())).await;
    assert_eq!(s, 200, "{sent}");
    let id = sent["id"].as_str().unwrap().to_string();
    assert!(id.starts_with("fb-"), "{id}");

    // What is refused.
    for (body, why) in [
        (json!({"note": "  "}), "a note"),
        (json!({"note": "x", "shot": "data:image/svg+xml;base64,PHN2Zz4="}), "PNG, JPEG or WebP"),
        (json!({"note": "x", "shot": format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(PNG))}), "does not hold"),
        (json!({"note": "x", "kind": "rant"}), "kind is one of"),
        (json!({"note": "x", "marks": [{"tool": "laser"}]}), "tool"),
    ] {
        let (s, j) = call(&base, "POST", "/v1/feedback", Some(&write), Some(body.clone())).await;
        assert_eq!(s, 400, "{body}: {j}");
        assert!(j["error"].as_str().unwrap().contains(why), "{body}: {j}");
    }

    // Listed for a read key, newest first, without the pictures unless thumbnails are asked for.
    let (s, j) = call(&base, "GET", "/v1/feedback", Some(&read), None).await;
    assert_eq!(s, 200, "{j}");
    let f = &j["feedback"][0];
    assert_eq!((&f["id"], &f["status"], &f["kind"], &f["author"], &f["page"]), (&json!(id), &json!("new"), &json!("bug"), &json!("roberto"), &json!("cluster")), "{j}");
    assert_eq!(f["title"], "The Add shard button is too small on a phone.");
    assert!(f.get("shot").is_none() && f.get("thumb").is_none(), "{f}");
    assert_eq!(f["shot_bytes"], PNG.len());
    assert_eq!(j["counts"]["status"]["new"], 1);
    // The marks as text, the blacked-out one left out, and found by its words.
    assert_eq!(f["marks_text"], "1. box \"too small\" on \"Shards\" (button.btn: Add shard)\n2. arrow \"when did this start\"");
    let (_, j) = call(&base, "GET", "/v1/feedback?q=small&thumbs=1", Some(&read), None).await;
    assert_eq!(j["feedback"].as_array().unwrap().len(), 1, "{j}");
    assert_eq!(j["feedback"][0]["thumb"], png_url());
    let (_, j) = call(&base, "GET", "/v1/feedback?kind=idea", Some(&read), None).await;
    assert_eq!(j["feedback"], json!([]));
    assert_eq!(call(&base, "GET", "/v1/feedback?status=sleeping", Some(&read), None).await.0, 400);

    // One in full: its marks as JSON, nothing of what was blacked out.
    let (s, one) = call(&base, "GET", &format!("/v1/feedback/{id}"), Some(&read), None).await;
    assert_eq!(s, 200, "{one}");
    assert_eq!(one["marks"].as_array().unwrap().len(), 3);
    assert!(!one.to_string().contains("sk_live_should_not_be_kept"), "{one}");
    assert_eq!(one["context"]["viewport"]["dpr"], 3);
    assert_eq!(one["shot_url"], format!("/v1/feedback/{id}/shot"));
    assert!(one.get("shot").is_none());
    let (_, with) = call(&base, "GET", &format!("/v1/feedback/{id}?shot=1"), Some(&read), None).await;
    assert_eq!(with["shot"], png_url());
    assert_eq!(call(&base, "GET", "/v1/feedback/fb-nope", Some(&read), None).await.0, 404);
    assert_eq!(call(&base, "GET", "/v1/feedback/x'%20OR%201", Some(&read), None).await.0, 400);

    // The picture, served inert.
    let c = chloedb_server::http_client().build().unwrap();
    let r = c.get(format!("{base}/v1/feedback/{id}/shot")).bearer_auth(&read).send().await.unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["content-type"], "image/png");
    assert_eq!(r.headers()["x-content-type-options"], "nosniff");
    assert!(r.headers()["content-security-policy"].to_str().unwrap().contains("sandbox"));
    assert_eq!(r.bytes().await.unwrap().as_ref(), PNG);
    assert_eq!(c.get(format!("{base}/v1/feedback/{id}/thumb")).bearer_auth(&read).send().await.unwrap().status(), 200);
    assert_eq!(c.get(format!("{base}/v1/feedback/{id}/shot")).send().await.unwrap().status(), 401);

    // Triage: a read key may not; a write key sets priority and status.
    assert_eq!(call(&base, "PATCH", &format!("/v1/feedback/{id}"), Some(&read), Some(json!({"priority": 1}))).await.0, 403);
    let (s, t) = call(&base, "PATCH", &format!("/v1/feedback/{id}"), Some(&write), Some(json!({"priority": 1, "status": "planned"}))).await;
    assert_eq!(s, 200, "{t}");
    assert_eq!((&t["priority"], &t["status"], &t["updated_by"]), (&json!(1), &json!("planned"), &json!("roberto")), "{t}");
    for bad in [json!({"priority": 7}), json!({"status": "maybe"}), json!({"shot": "x"}), json!({})] {
        assert_eq!(call(&base, "PATCH", &format!("/v1/feedback/{id}"), Some(&write), Some(bad.clone())).await.0, 400, "{bad}");
    }

    // An agent's key limited to the table takes the item; a second taker is refused.
    let (s, j) = call(&base, "GET", "/v1/feedback?status=planned", Some(&granted), None).await;
    assert_eq!((s, j["feedback"].as_array().map(Vec::len)), (200, Some(1)), "{j}");
    let take = json!({"status": "doing", "assignee": "thread: fix the button", "if_status": "planned"});
    let (s, t) = call(&base, "PATCH", &format!("/v1/feedback/{id}"), Some(&granted), Some(take.clone())).await;
    assert_eq!((s, &t["status"], &t["assignee"]), (200, &json!("doing"), &json!("thread: fix the button")), "{t}");
    let (s, j) = call(&base, "PATCH", &format!("/v1/feedback/{id}"), Some(&write), Some(take)).await;
    assert_eq!(s, 409, "{j}");
    // And reads it with SQL, as any row.
    let (s, j) = call(&base, "POST", "/v1/query", Some(&granted), Some(json!({"sql": "SELECT id, status, assignee FROM cc_feedback WHERE status = 'doing'"}))).await;
    assert_eq!((s, j["rows"].as_array().map(Vec::len)), (200, Some(1)), "{j}");
    // A key limited to other tables sees none of it.
    for (m, p) in [("GET", "/v1/feedback".to_string()), ("GET", format!("/v1/feedback/{id}")), ("GET", format!("/v1/feedback/{id}/shot"))] {
        assert_eq!(call(&base, m, &p, Some(&other), None).await.0, 403, "{m} {p}");
    }
    assert_eq!(call(&base, "POST", "/v1/feedback", Some(&other), Some(item())).await.0, 403);

    // Over MCP: the list, and one item with its screenshot as an image.
    let rpc = |name: &str, args: Json| json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": args}});
    let (s, j) = call(&base, "POST", "/v1/mcp", Some(&read), Some(rpc("list_feedback", json!({"status": "all"})))).await;
    assert_eq!(s, 200, "{j}");
    assert_eq!(j["result"]["isError"], false, "{j}");
    assert!(j["result"]["content"][0]["text"].as_str().unwrap().contains(&id), "{j}");
    let (_, j) = call(&base, "POST", "/v1/mcp", Some(&read), Some(rpc("get_feedback", json!({"id": id})))).await;
    let content = j["result"]["content"].as_array().unwrap_or_else(|| panic!("{j}"));
    assert_eq!(content.len(), 2, "{j}");
    assert!(!content[0]["text"].as_str().unwrap().contains("base64"), "the picture is not in the text");
    assert_eq!((&content[1]["type"], &content[1]["mimeType"]), (&json!("image"), &json!("image/png")));
    assert_eq!(base64::engine::general_purpose::STANDARD.decode(content[1]["data"].as_str().unwrap()).unwrap(), PNG);
    let (_, j) = call(&base, "POST", "/v1/mcp", Some(&read), Some(rpc("update_feedback", json!({"id": id, "status": "done"})))).await;
    assert_eq!(j["result"]["isError"], true, "a read key does not triage: {j}");
    let (_, j) = call(&base, "POST", "/v1/mcp", Some(&write), Some(rpc("update_feedback", json!({"id": id, "status": "done", "resolution": "https://github.com/chloetech-ai/chloedb/pull/1"})))).await;
    assert_eq!(j["result"]["isError"], false, "{j}");
    let (_, j) = call(&base, "GET", "/v1/feedback", Some(&read), None).await;
    assert_eq!(j["feedback"], json!([]), "done is not open: {j}");
    let (_, j) = call(&base, "GET", "/v1/feedback?status=done", Some(&read), None).await;
    assert_eq!(j["feedback"][0]["resolution"], "https://github.com/chloetech-ai/chloedb/pull/1");

    // A note alone, without a picture.
    let (s, j) = call(&base, "POST", "/v1/feedback", Some(&write), Some(json!({"note": "Add a dark theme to the logs", "kind": "idea"}))).await;
    assert_eq!(s, 200, "{j}");
    let plain = j["id"].as_str().unwrap().to_string();
    let (_, one) = call(&base, "GET", &format!("/v1/feedback/{plain}"), Some(&read), None).await;
    assert_eq!(one["shot_url"], Json::Null, "{one}");
    assert_eq!(c.get(format!("{base}/v1/feedback/{plain}/shot")).bearer_auth(&read).send().await.unwrap().status(), 404);

    // Deleting is an admin's.
    assert_eq!(call(&base, "DELETE", &format!("/v1/feedback/{plain}"), Some(&write), None).await.0, 403);
    let (s, j) = call(&base, "DELETE", &format!("/v1/feedback/{plain}"), Some(root), None).await;
    assert_eq!((s, &j["deleted"]), (200, &json!(plain)), "{j}");
    assert_eq!(call(&base, "GET", &format!("/v1/feedback/{plain}"), Some(&read), None).await.0, 404);
}

async fn shard(dir: &std::path::Path, name: &str) -> String {
    let db = Arc::new(Db::create(&dir.join(name), demo::schema(10), DbOptions::default()).unwrap());
    db.set_embedder("emb", Arc::new(chloedb::embed::HashEmbedder { dim: demo::DIM })).unwrap();
    let schema = (*db.schema()).clone();
    let svc = Arc::new(Service::new(Backend::Local(db), schema, agent(), 16, Duration::from_secs(10)));
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(server::serve_grpc(svc, l, Access::default()));
    url
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn feedback_through_a_coordinator() {
    let dir = tempfile::tempdir().unwrap();
    let urls = vec![shard(dir.path(), "s0.strata").await, shard(dir.path(), "s1.strata").await];
    let c = Arc::new(Coordinator::connect(urls).await.unwrap());
    let schema = c.schema.clone();
    let svc = Arc::new(Service::new(Backend::Cluster(c), schema, agent(), 16, Duration::from_secs(10)));
    let base = serve(svc, Access::default()).await;
    let mut ids = Vec::new();
    for i in 0..6 {
        let mut b = item();
        b["note"] = json!(format!("item {i}: the shards card is cut off"));
        let (s, j) = call(&base, "POST", "/v1/feedback", None, Some(b)).await;
        assert_eq!(s, 200, "{j}");
        ids.push(j["id"].as_str().unwrap().to_string());
    }
    let (s, j) = call(&base, "GET", "/v1/feedback?status=all", None, None).await;
    assert_eq!(s, 200, "{j}");
    assert_eq!(j["feedback"].as_array().unwrap().len(), 6, "{j}");
    assert_eq!(j["counts"]["status"]["new"], 6, "{j}");
    let (s, t) = call(&base, "PATCH", &format!("/v1/feedback/{}", ids[3]), None, Some(json!({"priority": 0, "status": "triaged"}))).await;
    assert_eq!((s, &t["priority"], &t["status"]), (200, &json!(0), &json!("triaged")), "{t}");
    let (_, j) = call(&base, "GET", "/v1/feedback?status=triaged", None, None).await;
    assert_eq!(j["feedback"][0]["id"], json!(ids[3]), "{j}");
    let r = chloedb_server::http_client().build().unwrap().get(format!("{base}/v1/feedback/{}/shot", ids[5])).send().await.unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.bytes().await.unwrap().as_ref(), PNG);
    let (_, j) = call(&base, "GET", "/v1/feedback?q=shards", None, None).await;
    assert_eq!(j["feedback"].as_array().unwrap().len(), 6, "{j}");
}
