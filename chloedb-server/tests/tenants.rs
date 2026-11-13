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
use chloedb_server::tenants::{Limits, Tenants};
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
    svc.tenants = store.map(|s| Arc::new(Tenants::open(s.to_str().unwrap(), dir.join(format!("{name}-cache")), Limits::default()).unwrap()));
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

    // Either server lists the store's tenants, and which it has open.
    assert_eq!(call(&a, "POST", "/v1/admin/tenants", root, Some(json!({"id": "agent-9"}))).await.0, 200);
    let (s, j) = call(&b, "GET", "/v1/admin/tenants", root, None).await;
    assert_eq!((s, &j["tenants"], &j["open"]), (200, &json!(["agent-7", "agent-9"]), &json!(["agent-7"])), "{j}");

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

/// Tenants are closed when idle, and beyond the most a server keeps
/// open; a closed one answers as before, from the copy the cache kept,
/// and one that was written gets a base before it is closed.
#[test]
fn idle_tenants_are_closed_and_open_again_when_asked_for() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("store");
    let limits = Limits { idle: Duration::from_secs(3600), open_max: 2 };
    let t = Tenants::open(store.to_str().unwrap(), dir.path().join("cache"), limits).unwrap();
    let sql = |id: &str, s: &str| t.sql(id, s, &chloedb::query::Params::new(), false).unwrap();
    for id in ["a", "b", "c"] {
        t.create(id, &["CREATE TABLE notes (id INT PRIMARY KEY, body TEXT)".to_string()], None).unwrap();
        sql(id, "INSERT INTO notes VALUES (1, 'one')");
    }
    assert_eq!(t.open_ids(), ["a", "b", "c"]);
    // None is idle, but three are one too many: the one unused longest goes,
    assert_eq!(t.close_idle(), 1);
    assert_eq!(t.open_ids(), ["b", "c"]);
    // with a base that holds its row, so that opening it reads no log.
    let a = t.describe("a").unwrap();
    assert_eq!((&a["base"]["seq"], &a["last_seq"], a["logs"].as_array().map(Vec::len), &a["tail"]["transactions"]), (&a["last_seq"], &json!(2), Some(0), &json!(0)), "{a}");
    assert_eq!(sql("a", "SELECT body FROM notes WHERE id = 1")["rows"], json!([["one"]]));
    assert_eq!(t.open_ids(), ["a", "b", "c"]);
    assert_eq!(t.close_idle(), 1);
    assert_eq!(t.open_ids(), ["a", "c"], "b was used before c, and a just now");

    // With no time allowed, every tenant is idle.
    drop(t);
    let t = Tenants::open(store.to_str().unwrap(), dir.path().join("cache"), Limits { idle: Duration::ZERO, open_max: 100 }).unwrap();
    let sql = |id: &str, s: &str| t.sql(id, s, &chloedb::query::Params::new(), false).unwrap();
    sql("b", "INSERT INTO notes VALUES (2, 'two')");
    sql("c", "SELECT 1");
    assert_eq!((t.open_ids(), t.close_idle(), t.open_ids()), (vec!["b".to_string(), "c".to_string()], 2, Vec::new()));
    assert_eq!(sql("b", "SELECT count(*) FROM notes")["rows"], json!([[2]]));
    assert_eq!(t.list().unwrap()["tenants"], json!(["a", "b", "c"]));
}

/// A blackboard (a tenant made from the template) is worked by agents
/// through its tools, over HTTP and as an MCP server, on either server.
#[tokio::test(flavor = "multi_thread")]
async fn a_blackboard_is_worked_through_http_and_mcp() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("store");
    let a = serve(dir.path(), "a", Some(&store)).await;
    let b = serve(dir.path(), "b", Some(&store)).await;
    let root = Some("root-token");

    assert_eq!(call(&a, "POST", "/v1/admin/tenants", root, Some(json!({"id": "job", "template": "whiteboard"}))).await.0, 400);
    let (s, j) = call(&a, "POST", "/v1/admin/tenants", root, Some(json!({"id": "job", "template": "blackboard"}))).await;
    assert_eq!(s, 200, "{j}");
    assert_eq!(call(&a, "POST", "/v1/admin/tenants", root, Some(json!({"id": "plain"}))).await.0, 200);

    let tool = |base: &str, name: &str, body: Json| {
        let (base, path) = (base.to_string(), format!("/v1/admin/tenants/job/board/{name}"));
        async move { call(&base, "POST", &path, Some("root-token"), Some(body)).await }
    };
    let (s, j) = tool(&a, "add_tasks", json!({"agent": "lead", "tasks": [{"title": "port the parser", "spec": "two passes"}, {"id": "lexer", "title": "port the lexer"}]})).await;
    assert_eq!((s, &j["added"]), (200, &json!(["t1", "lexer"])), "{j}");

    // A claim, and the same claim sent again with its key.
    let claim = json!({"agent": "ann", "n": 1, "lease_s": 60, "key": "c1"});
    let (s, j) = tool(&a, "claim", claim.clone()).await;
    assert_eq!((s, &j["tasks"][0]["id"], &j["tasks"][0]["owner"], &j["tasks"][0]["state"]), (200, &json!("t1"), &json!("ann"), &json!("claimed")), "{j}");
    assert!(j["tasks"][0]["lease_until"].as_str().is_some_and(|t| t.ends_with('Z')), "{j}");
    assert_eq!(tool(&a, "claim", claim).await.1, j);

    // What the board refuses is a conflict; what it cannot read, the caller's error.
    let (s, j) = tool(&a, "complete", json!({"agent": "bob", "task": "t1", "result": "mine"})).await;
    assert_eq!(s, 409, "{j}");
    assert!(j["error"].as_str().unwrap_or_default().contains("claimed by ann"), "{j}");
    assert_eq!(tool(&a, "erase", json!({"agent": "bob"})).await.0, 404);
    assert_eq!(tool(&a, "claim", json!({"agent": "bob", "count": 2})).await.0, 400);
    assert_eq!(tool(&a, "claim", json!({"n": 2})).await.0, 400);
    assert_eq!(tool(&a, "complete", json!({"agent": "bob", "task": "t1"})).await.0, 400);
    assert_eq!(call(&a, "POST", "/v1/admin/tenants/plain/board/status", root, Some(json!({"agent": "bob"}))).await.0, 400);
    assert_eq!(call(&a, "POST", "/v1/admin/tenants/nothing/board/status", root, Some(json!({"agent": "bob"}))).await.0, 404);
    assert_eq!(call(&a, "POST", "/v1/admin/tenants/job/board/status", None, Some(json!({"agent": "bob"}))).await.0, 401);

    // The same tools as an MCP server, for the agent its URL names.
    let mcp = |base: &str, agent: &str, msg: Json| {
        let (base, path) = (base.to_string(), format!("/v1/admin/tenants/job/mcp?agent={agent}"));
        async move { call(&base, "POST", &path, Some("root-token"), Some(msg)).await }
    };
    let (s, j) = mcp(&a, "bob", json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}})).await;
    assert_eq!((s, &j["result"]["protocolVersion"], &j["result"]["serverInfo"]["name"]), (200, &json!("2025-06-18"), &json!("chloedb-blackboard")), "{j}");
    assert!(j["result"]["instructions"].as_str().unwrap_or_default().contains("\"bob\""), "{j}");
    assert_eq!(mcp(&a, "bob", json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).await.0, 202);
    let (_, j) = mcp(&a, "bob", json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).await;
    let names: Vec<&str> = j["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["claim", "heartbeat", "complete", "fail", "post", "search", "timeline", "status", "get_task", "add_tasks"]);
    assert_eq!(call(&a, "POST", "/v1/admin/tenants/job/mcp", root, Some(json!({"jsonrpc": "2.0", "id": 3, "method": "ping"}))).await.0, 400);
    assert_eq!(call(&a, "POST", "/v1/admin/tenants/nothing/mcp?agent=bob", root, Some(json!({"jsonrpc": "2.0", "id": 3, "method": "ping"}))).await.0, 404);

    // A tool's answer is JSON in text; a refusal is an answer marked as an error.
    let call_tool = |base: &str, agent: &str, name: &str, args: Json| {
        let sent = mcp(base, agent, json!({"jsonrpc": "2.0", "id": 9, "method": "tools/call", "params": {"name": name, "arguments": args}}));
        async move {
            let (s, j) = sent.await;
            assert_eq!(s, 200, "{j}");
            j
        }
    };
    let said = |j: &Json| -> Json { serde_json::from_str(j["result"]["content"][0]["text"].as_str().unwrap_or("null")).unwrap_or(Json::Null) };
    let j = call_tool(&a, "bob", "claim", json!({"n": 5})).await;
    assert_eq!((&j["result"]["isError"], &said(&j)["tasks"][0]["id"], said(&j)["tasks"].as_array().map(Vec::len)), (&json!(false), &json!("lexer"), Some(1)), "{j}");
    let j = call_tool(&a, "bob", "complete", json!({"task": "lexer", "result": "ported", "key": "d1"})).await;
    assert_eq!(said(&j)["state"], "done", "{j}");
    let j = call_tool(&a, "bob", "complete", json!({"task": "t1", "result": "not mine"})).await;
    assert_eq!(j["result"]["isError"], true, "{j}");
    assert!(j["result"]["content"][0]["text"].as_str().unwrap_or_default().contains("claimed by ann"), "{j}");
    assert_eq!(call_tool(&a, "bob", "claim", json!({"count": 5})).await["error"]["code"], -32602);
    assert_eq!(call_tool(&a, "bob", "erase", json!({})).await["error"]["code"], -32602);

    let first = said(&call_tool(&a, "bob", "post", json!({"body": "The lexer keeps a one token lookahead for the parser.", "task": "lexer"})).await);
    assert_eq!((&first["id"], &first["dup_of"]), (&json!("f1"), &Json::Null), "{first}");
    let again = said(&call_tool(&a, "ann", "post", json!({"body": "the lexer keeps a one-token lookahead, for the parser"})).await);
    assert_eq!(again["dup_of"], "f1", "{again}");
    let found = said(&call_tool(&a, "ann", "search", json!({"q": "lookahead"})).await);
    assert_eq!((found["findings"].as_array().map(Vec::len), &found["findings"][0]["id"], &found["findings"][0]["duplicates"]), (Some(1), &json!("f1"), &json!(1)), "{found}");

    // The other server answers for the same board: it reads the store
    // when asked, and a call of its own takes the tenant, after which
    // the first server's next call is tried again there, not refused.
    let (s, j) = tool(&b, "status", json!({"agent": "carol", "latest": true})).await;
    assert_eq!((s, &j["tasks"]["claimed"], &j["tasks"]["done"], &j["findings"], &j["duplicates"]), (200, &json!(1), &json!(1), &json!(2), &json!(1)), "{j}");
    let (s, j) = tool(&b, "add_tasks", json!({"agent": "carol", "tasks": [{"id": "lexer", "title": "again"}, {"title": "port the printer"}]})).await;
    assert_eq!((s, &j["added"], &j["existing"]), (200, &json!(["t2"]), &json!(["lexer"])), "{j}");
    let (s, j) = tool(&a, "complete", json!({"agent": "ann", "task": "t1", "result": "parser ported"})).await;
    assert_eq!((s, &j["state"]), (200, &json!("done")), "{j}");
    let (s, j) = tool(&a, "timeline", json!({"agent": "ann", "since": 2})).await;
    let told: Vec<String> = j["events"].as_array().unwrap().iter().map(|e| format!("{} {} {}", e["agent"].as_str().unwrap(), e["kind"].as_str().unwrap(), e["about"].as_str().unwrap())).collect();
    assert_eq!((s, told), (200, ["ann claim t1", "bob claim lexer", "bob done lexer", "bob finding f1", "ann finding f2", "carol task t2", "ann done t1"].map(String::from).to_vec()), "{j}");
    let (_, j) = tool(&a, "get_task", json!({"agent": "ann", "id": "t2"})).await;
    assert_eq!((&j["state"], &j["title"]), (&json!("open"), &json!("port the printer")), "{j}");
}
