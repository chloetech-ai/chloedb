//! The routes a key limited to tables uses for its own tables, beyond
//! statements and rows ([`chloedb_server::scoped`]): parameters that embed
//! text or read a row's vector, by the table the statement queries and
//! never another's; a table's schema with none named; `PACK`; plans;
//! logged queries; OTLP logs; Explain with hits; MCP's JSON-RPC refusal;
//! apps that check their callers and apps that do not; and the presets of
//! such keys, with the routes they give.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::http::HeaderMap;
use axum::Router;
use chloedb::db::Db;
use chloedb::storage::DbOptions;
use chloedb_server::apps::{parse_with, Apps};
use chloedb_server::auth::{Access, Secret};
use chloedb_server::keys::Keys;
use chloedb_server::service::{AgentConfig, Backend, Service};
use chloedb_server::{demo, server};
use serde_json::{json, Value as Json};
use tokio::net::TcpListener;

/// A server of the demo's `products` (whose `emb` is embedded by
/// `hash:64`), with a models directory, its keys in a file, and `apps`.
async fn start(dir: &std::path::Path, apps: Vec<String>) -> (String, Arc<Service>) {
    let db = Db::create(&dir.join("a.strata"), demo::schema(10), DbOptions::default()).unwrap();
    let schema = (*db.schema()).clone();
    let agent = AgentConfig { api_key: None, model: String::new(), base_url: String::new() };
    let mut svc = Service::new(Backend::Local(Arc::new(db)), schema, agent, 16, Duration::from_secs(10));
    svc.keys = Arc::new(Keys::open(dir.join("a.strata.keys.json")).unwrap());
    svc.open_models(dir);
    if !apps.is_empty() {
        svc.apps = Apps::new(parse_with(&apps, |_| None).unwrap(), Some("app-token".into()));
    }
    let svc = Arc::new(svc);
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
    tokio::spawn(server::serve_http(svc.clone(), l, Access { token: Some(Secret::new("root-token")), ..Default::default() }));
    (base, svc)
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

const ROOT: Option<&str> = Some("root-token");

async fn key(base: &str, body: Json) -> String {
    let (s, made) = call(base, "POST", "/v1/admin/keys", ROOT, Some(body)).await;
    assert_eq!(s, 200, "{made}");
    made["secret"].as_str().unwrap().to_string()
}

async fn ok(base: &str, sql: &str) {
    let (s, j) = call(base, "POST", "/v1/sql", ROOT, Some(json!({"sql": sql}))).await;
    assert_eq!(s, 200, "{sql}: {j}");
}

#[tokio::test(flavor = "multi_thread")]
async fn parameters_read_the_queried_table_and_no_other() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _svc) = start(dir.path(), Vec::new()).await;
    ok(&base, "CREATE TABLE swarm_x (id KEYWORD PRIMARY KEY, title TEXT, emb VECTOR(16) OPTIONS(embed = 'title', model = 'hash:16'))").await;
    ok(&base, "INSERT INTO swarm_x (id, title) VALUES ('a', 'red shoes'), ('b', 'blue kettle'), ('c', 'green lamp')").await;
    let k = key(&base, json!({"name": "swarm", "tables": {"swarm_*": "admin"}})).await;
    let k = Some(k.as_str());
    let sql = "SELECT id FROM swarm_x ORDER BY similarity(emb, :q) DESC LIMIT 3";

    // No table named: the queried table's field, for the key and the admin.
    for who in [k, ROOT] {
        for q in [json!({"embed": "red shoes", "field": "emb"}), json!({"embed": "red shoes"}), json!({"vector_of": "a"}), json!({"vector_of": "a", "field": "emb"})] {
            let (s, j) = call(&base, "POST", "/v1/query", who, Some(json!({"sql": sql, "params": {"q": q}}))).await;
            assert_eq!((s, &j["rows"][0][0]), (200, &json!("a")), "{q}: {j}");
        }
    }
    // Another table, named: refused for the key, before anything is read.
    for q in [json!({"embed": "tent", "table": "products"}), json!({"vector_of": "p0000001", "table": "products"}), json!({"embed": "tent", "table": "products", "field": "emb"})] {
        let (s, j) = call(&base, "POST", "/v1/query", k, Some(json!({"sql": sql, "params": {"q": q}}))).await;
        assert_eq!(s, 403, "{q}: {j}");
        assert!(j["error"].as_str().unwrap().contains("products"), "{j}");
        let (s, _) = call(&base, "POST", "/v1/sql", k, Some(json!({"sql": sql, "params": {"q": q}}))).await;
        assert_eq!(s, 403, "{q}");
        let (s, _) = call(&base, "POST", "/v1/explain", k, Some(json!({"sql": sql, "params": {"q": q}}))).await;
        assert_eq!(s, 403, "{q}");
    }
    // The admin may.
    let (s, j) = call(&base, "POST", "/v1/query", ROOT, Some(json!({"sql": "SELECT id FROM products ORDER BY similarity(emb, :q) DESC LIMIT 1", "params": {"q": {"embed": "tent"}}}))).await;
    assert_eq!(s, 200, "{j}");
    // Two tables with such a field: which one is not guessed.
    ok(&base, "CREATE TABLE swarm_y (id KEYWORD PRIMARY KEY, title TEXT, emb VECTOR(16) OPTIONS(embed = 'title', model = 'hash:16'))").await;
    let both = "SELECT id FROM swarm_x WHERE id IN (SELECT id FROM swarm_y) ORDER BY similarity(emb, :q) DESC";
    let (s, j) = call(&base, "POST", "/v1/query", k, Some(json!({"sql": both, "params": {"q": {"embed": "x"}}}))).await;
    assert_eq!(s, 400, "{j}");
    assert!(j["error"].as_str().unwrap().contains("add \"table\""), "{j}");

    // /v1/embed: the model of a table the key reads; /v1/models: the fields
    // of those alone.
    let (s, j) = call(&base, "POST", "/v1/embed", k, Some(json!({"text": "tent", "table": "products"}))).await;
    assert_eq!(s, 403, "{j}");
    let (s, j) = call(&base, "POST", "/v1/embed", k, Some(json!({"text": "tent"}))).await;
    assert_eq!((s, j["table"].as_str().map(|t| t.starts_with("swarm_"))), (200, Some(true)), "{j}");
    assert_eq!(j["vector"].as_array().map(Vec::len), Some(16), "{j}");
    let (s, j) = call(&base, "GET", "/v1/models", k, None).await;
    assert_eq!(s, 200, "{j}");
    assert!(!j.to_string().contains("\"products\""), "{j}");
    assert!(j.to_string().contains("swarm_x"), "{j}");
    let (_, j) = call(&base, "GET", "/v1/models", ROOT, None).await;
    assert!(j.to_string().contains("\"products\""), "{j}");

    // A rank profile that takes one text for BM25 and a vector.
    ok(&base, "CREATE RANK PROFILE shop ON swarm_x AS SELECT * FROM swarm_x ORDER BY bm25(title, :q) + similarity(emb, :q) DESC").await;
    let (s, j) = call(&base, "POST", "/v1/query", k, Some(json!({"sql": "SELECT id FROM swarm_x ORDER BY rank_profile('shop', q => :t) LIMIT 3", "params": {"t": "red shoes"}}))).await;
    assert_eq!((s, &j["rows"][0][0]), (200, &json!("a")), "{j}");
    let (s, j) = call(&base, "POST", "/v1/query", ROOT, Some(json!({"sql": "SELECT id FROM swarm_x ORDER BY rank_profile('shop', q => 'blue kettle') LIMIT 3"}))).await;
    assert_eq!((s, &j["rows"][0][0]), (200, &json!("b")), "{j}");
}

/// ONB-16: SHOW TABLES through /v1/sql answers the tables' names, without
/// ChloeDB's own, and for a key limited to tables those it reads alone.
#[tokio::test(flavor = "multi_thread")]
async fn show_tables_names_the_tables_a_key_reads() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _svc) = start(dir.path(), Vec::new()).await;
    ok(&base, "CREATE TABLE swarm_x (id KEYWORD PRIMARY KEY, title TEXT)").await;
    ok(&base, "CREATE FUNCTION bump(x INT) RETURNS INT AS :x + 1").await;
    let names = |j: &Json| j["rows"].as_array().unwrap().iter().map(|r| r[0].as_str().unwrap().to_string()).collect::<Vec<_>>();
    let (s, j) = call(&base, "POST", "/v1/sql", ROOT, Some(json!({"sql": "SHOW TABLES"}))).await;
    assert_eq!((s, &j["columns"]), (200, &json!(["name"])), "{j}");
    let mut all = names(&j);
    all.sort();
    assert_eq!(all, ["products", "swarm_x"], "{j}");
    let k = key(&base, json!({"name": "swarm", "tables": {"swarm_*": "read"}})).await;
    let (s, j) = call(&base, "POST", "/v1/sql", Some(&k), Some(json!({"sql": "show tables"}))).await;
    assert_eq!((s, names(&j)), (200, vec!["swarm_x".to_string()]), "{j}");
    // /v1/query takes queries alone, and says where SHOW TABLES goes.
    let (s, j) = call(&base, "POST", "/v1/query", ROOT, Some(json!({"sql": "SHOW TABLES"}))).await;
    assert_eq!(s, 400, "{j}");
    assert!(j["error"].as_str().unwrap().contains("/v1/sql"), "{j}");
}

/// LENA-1: a server with a token names no table to a caller without a
/// credential (its health says whether it is up, as a coordinator's does),
/// and a key limited to tables sees its own tables alone in its health,
/// in `/v1/cluster` and in an error for a table there is none of: no
/// other table's name, nor its rows. The token sees them all.
#[tokio::test(flavor = "multi_thread")]
async fn no_credential_and_a_limited_key_see_no_other_tables() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _svc) = start(dir.path(), Vec::new()).await;
    for t in ["payroll", "a_notes", "b_notes"] {
        ok(&base, &format!("CREATE TABLE {t} (id KEYWORD PRIMARY KEY, body TEXT)")).await;
    }
    ok(&base, "INSERT INTO payroll (id, body) VALUES ('p1', 'x'), ('p2', 'y')").await;
    ok(&base, "INSERT INTO a_notes (id, body) VALUES ('n1', 'x'), ('n2', 'y')").await;
    ok(&base, "INSERT INTO b_notes (id, body) VALUES ('m1', 'x')").await;
    let others = ["payroll", "b_notes", "products"];
    let names_none = |what: &str, j: &Json| {
        for t in others {
            assert!(!j.to_string().contains(t), "{what} names {t}: {j}");
        }
    };
    // Signed out, or with a wrong token: up or not, and nothing else.
    for auth in [None, Some("not-the-token")] {
        let (s, h) = call(&base, "GET", "/v1/health", auth, None).await;
        assert_eq!((s, &h), (200, &json!({"status": "ok"})), "{auth:?}");
    }
    let tenant = key(&base, json!({"name": "tenant-a", "tables": {"a_*": "admin"}})).await;
    let t = Some(tenant.as_str());
    let (s, h) = call(&base, "GET", "/v1/health", t, None).await;
    assert_eq!((s, &h["tables"], &h["rows"], &h["docs"], &h["status"]), (200, &json!({"a_notes": 2}), &json!(2), &json!(2), &json!("ok")), "{h}");
    names_none("health", &h);
    let (s, c) = call(&base, "GET", "/v1/cluster", t, None).await;
    assert_eq!((s, &c["node"]["health"]["tables"]), (200, &json!({"a_notes": 2})), "{c}");
    names_none("/v1/cluster", &c);
    // A table of its prefix there is none of: no listing of the others.
    for sql in ["INSERT INTO a_ghost (id) VALUES ('1')", "UPDATE a_ghost SET body = 'x' WHERE id = '1'", "DELETE FROM a_ghost WHERE id = '1'", "INSERT INTO a_note (id) VALUES ('1')"] {
        let (s, e) = call(&base, "POST", "/v1/sql", t, Some(json!({"sql": sql}))).await;
        assert_eq!(s, 400, "{sql}: {e}");
        let m = e["error"].as_str().unwrap();
        assert!(m.contains("no such table: a_") && m.contains("a_notes"), "{sql}: {m}");
        names_none(sql, &e);
    }
    let (_, e) = call(&base, "POST", "/v1/sql", t, Some(json!({"sql": "INSERT INTO a_note (id) VALUES ('1')"}))).await;
    assert!(e["error"].as_str().unwrap().contains("(did you mean a_notes?)"), "{e}");
    // The token's holder sees every table, and the whole listing.
    let (_, h) = call(&base, "GET", "/v1/health", ROOT, None).await;
    assert_eq!((&h["tables"]["payroll"], &h["tables"]["b_notes"]), (&json!(2), &json!(1)), "{h}");
    let (_, e) = call(&base, "POST", "/v1/sql", ROOT, Some(json!({"sql": "INSERT INTO a_ghost (id) VALUES ('1')"}))).await;
    assert!(e["error"].as_str().unwrap().contains("payroll"), "{e}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_limited_key_uses_its_own_tables_routes() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _svc) = start(dir.path(), Vec::new()).await;
    let k = key(&base, json!({"name": "ml", "tables": {"swarm_*": "admin"}})).await;
    let k = Some(k.as_str());

    // whoami: its grants, and that its scope is its tables'.
    let (s, me) = call(&base, "GET", "/v1/whoami", k, None).await;
    assert_eq!((s, &me["tables"], &me["limited"]), (200, &json!({"swarm_*": "admin"}), &json!(true)), "{me}");

    // /v1/schema, no table named: none it reads yet, then the first it does.
    let (s, j) = call(&base, "GET", "/v1/schema", k, None).await;
    assert_eq!(s, 400, "{j}");
    assert!(!j.to_string().contains("products"), "{j}");
    let (s, j) = call(&base, "POST", "/v1/sql", k, Some(json!({"sql": "CREATE TABLE swarm_items (id KEYWORD PRIMARY KEY, title TEXT, brand KEYWORD, year INT, price FLOAT)"}))).await;
    assert_eq!(s, 200, "{j}");
    let (s, j) = call(&base, "POST", "/v1/sql", k, Some(json!({"sql": "INSERT INTO swarm_items (id, title, brand, year, price) VALUES ('a', 'trail shoe', 'x', 2018, 10), ('b', 'road shoe', 'y', 2019, 20), ('c', 'tent', 'x', 2019, 30)"}))).await;
    assert_eq!(s, 200, "{j}");
    let (s, j) = call(&base, "GET", "/v1/schema", k, None).await;
    assert_eq!((s, j["table"].as_str().or(j["name"].as_str()).or(j["short"]["table"].as_str())), (200, Some("swarm_items")), "{j}");
    assert_eq!(call(&base, "GET", "/v1/schema?table=products", k, None).await.0, 403);
    assert_eq!(call(&base, "GET", "/v1/schema?table=nowhere", k, None).await.0, 403);

    // PACK n TOKENS, as the admin's.
    let pack = "SELECT id, title FROM swarm_items ORDER BY bm25(title, :t) DESC PACK 200 TOKENS";
    let (s, j) = call(&base, "POST", "/v1/query", k, Some(json!({"sql": pack, "params": {"t": "shoe"}}))).await;
    assert_eq!(s, 200, "{j}");
    let (s, _) = call(&base, "POST", "/v1/query", k, Some(json!({"sql": "SELECT id, title FROM products ORDER BY bm25(title, :t) DESC PACK 200 TOKENS", "params": {"t": "shoe"}}))).await;
    assert_eq!(s, 403);

    // A schema checked; a plan of its table, not of another.
    let (s, j) = call(&base, "POST", "/v1/schema/check", k, Some(json!({"table": "t", "key": "id", "fields": [{"name": "id", "type": "keyword"}]}))).await;
    assert_eq!(s, 200, "{j}");
    let (s, j) = call(&base, "POST", "/v1/plan", k, Some(json!({"plan": {"version": 1, "root": {"node": "project", "columns": [{"name": "id", "expr": {"field": 0}}], "input": {"node": "search", "table": "swarm_items"}}}}))).await;
    assert_eq!(s, 200, "{j}");
    let (s, j) = call(&base, "POST", "/v1/plan", k, Some(json!({"plan": {"version": 1, "root": {"node": "project", "columns": [{"name": "id", "expr": {"field": 0}}], "input": {"node": "search", "table": "products"}}}}))).await;
    assert_eq!(s, 403, "{j}");

    // A logged query into a table of its own, not another.
    let q = json!({"sql": "SELECT id FROM swarm_items ORDER BY bm25(title, :t) DESC LIMIT 2", "params": {"t": "shoe"}, "log": {"table": "swarm_rank_log"}});
    let (s, j) = call(&base, "POST", "/v1/query", k, Some(q)).await;
    assert_eq!(s, 200, "{j}");
    assert!(j["logged"]["rows"].as_u64().unwrap() > 0, "{j}");
    let q = json!({"sql": "SELECT id FROM swarm_items ORDER BY bm25(title, :t) DESC LIMIT 2", "params": {"t": "shoe"}, "log": {"table": "rank_log"}});
    assert_eq!(call(&base, "POST", "/v1/query", k, Some(q)).await.0, 403);
    let q = json!({"sql": "SELECT id FROM products ORDER BY bm25(title, :t) DESC LIMIT 2", "params": {"t": "shoe"}, "log": {"table": "swarm_rank_log"}});
    assert_eq!(call(&base, "POST", "/v1/query", k, Some(q)).await.0, 403);
    let (s, j) = call(&base, "POST", "/v1/rank_log/export", k, Some(json!({"table": "swarm_rank_log"}))).await;
    assert_eq!(s, 200, "{j}");
    assert_eq!(call(&base, "POST", "/v1/rank_log/export", k, Some(json!({"table": "rank_log"}))).await.0, 403);

    // OTLP logs into a table it writes.
    let logs = json!({"resourceLogs": [{"resource": {"attributes": []}, "scopeLogs": [{"logRecords": [{"timeUnixNano": "1700000000000000000", "severityText": "ERROR", "body": {"stringValue": "Gi0/1 down"}}]}]}]});
    let (s, j) = call(&base, "POST", "/v1/logs?table=swarm_otel", k, Some(logs.clone())).await;
    assert_eq!(s, 200, "{j}");
    let (s, j) = call(&base, "POST", "/v1/query", k, Some(json!({"sql": "SELECT COUNT(*) FROM swarm_otel"}))).await;
    assert_eq!((s, &j["rows"][0][0]), (200, &json!(1)), "{j}");
    assert_eq!(call(&base, "POST", "/v1/logs?table=otel_logs", k, Some(logs)).await.0, 403);

    // Explain of a query that orders groups, with hits: the plan, without them.
    let (s, j) = call(&base, "POST", "/v1/explain", k, Some(json!({"sql": "SELECT brand, COUNT(*) AS n FROM swarm_items GROUP BY brand ORDER BY n DESC", "hits": 5}))).await;
    assert_eq!(s, 200, "{j}");
    assert!(j.get("hits").is_none(), "{j}");

    // MCP: a JSON-RPC error saying why.
    let (s, j) = call(&base, "POST", "/v1/mcp", k, Some(json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))).await;
    assert_eq!(s, 403, "{j}");
    assert_eq!(j["jsonrpc"], "2.0", "{j}");
    assert!(j["error"]["message"].as_str().unwrap().contains("limited to tables"), "{j}");

    // Still refused: what shows other tables' work.
    for path in ["/v1/events", "/v1/health/fleet", "/metrics", "/v1/stats", "/v1/queries/recent"] {
        assert_eq!(call(&base, "GET", path, k, None).await.0, 403, "{path}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn presets_are_grants_and_routes() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _svc) = start(dir.path(), Vec::new()).await;
    let (s, made) = call(&base, "POST", "/v1/admin/keys", ROOT, Some(json!({"name": "netops", "preset": "oncall", "prefix": "netops_*"}))).await;
    assert_eq!(s, 200, "{made}");
    assert_eq!(made["key"]["tables"], json!({"netops_*": "write", "ctx_netops_*": "write", "ctx_incidents": "read"}), "{made}");
    assert_eq!((&made["key"]["preset"], &made["key"]["allow"]["routes"]), (&json!("oncall"), &json!(["fleet", "alerts"])), "{made}");
    // Kept as grants: the file holds them, whatever reads it.
    let file = std::fs::read_to_string(dir.path().join("a.strata.keys.json")).unwrap();
    assert!(file.contains("netops_*") && file.contains("\"fleet\""), "{file}");
    let k = made["secret"].as_str().unwrap().to_string();
    let k = Some(k.as_str());
    let (_, me) = call(&base, "GET", "/v1/whoami", k, None).await;
    assert_eq!(me["preset"], "oncall", "{me}");
    for path in ["/v1/events", "/v1/health/fleet", "/metrics", "/v1/alerts", "/v1/health"] {
        let (s, j) = call(&base, "GET", path, k, None).await;
        assert_eq!(s, 200, "{path}: {j}");
    }
    // No more than its grants: other tables, and the server's admin.
    assert_eq!(call(&base, "POST", "/v1/query", k, Some(json!({"sql": "SELECT * FROM products"}))).await.0, 403);
    assert_eq!(call(&base, "GET", "/v1/admin/keys", k, None).await.0, 403);
    assert_eq!(call(&base, "POST", "/v1/events/notes", k, Some(json!({"text": "x"}))).await.0, 403);

    let (s, made) = call(&base, "POST", "/v1/admin/keys", ROOT, Some(json!({"name": "ana", "preset": "analyst", "prefix": "ana_*"}))).await;
    assert_eq!(s, 200, "{made}");
    assert_eq!(made["key"]["tables"], json!({"*": "read", "ana_*": "admin"}), "{made}");
    for (body, why) in [
        (json!({"name": "a1", "preset": "wizard", "prefix": "x_*"}), "not a preset"),
        (json!({"name": "a2", "preset": "analyst"}), "prefix"),
        (json!({"name": "a3", "preset": "analyst", "prefix": "*"}), "prefix"),
        (json!({"name": "a4", "preset": "analyst", "prefix": "x_*", "tables": {"y": "read"}}), "leave them out"),
        (json!({"name": "a5", "scope": "admin", "routes": ["fleet"]}), "limited to tables"),
        (json!({"name": "a6", "tables": {"x_*": "read"}, "routes": ["everything"]}), "not a group"),
        // A misspelt field is refused, not left out: without its limits the
        // key would read every table.
        (json!({"name": "a7", "presett": "oncall", "prefix": "logs_*"}), "\"presett\" is not a field of a key"),
        (json!({"name": "a8", "tabels": {"logs_*": "write"}}), "\"tabels\" is not a field of a key"),
        (json!({"name": "a9", "prefix": "logs_*"}), "prefix is the tables of a preset"),
    ] {
        let (s, j) = call(&base, "POST", "/v1/admin/keys", ROOT, Some(body.clone())).await;
        assert_eq!(s, 400, "{body}: {j}");
        assert!(j["error"].as_str().unwrap().contains(why), "{body}: {j}");
    }
    let (_, listed) = call(&base, "GET", "/v1/admin/keys", ROOT, None).await;
    assert!(!listed.to_string().contains("\"a7\"") && !listed.to_string().contains("\"a8\""), "a refused key is not made: {listed}");
    // Given by hand, as a preset gives them.
    let (s, made) = call(&base, "POST", "/v1/admin/keys", ROOT, Some(json!({"name": "ops2", "tables": {"logs_*": "write"}, "routes": ["fleet"]}))).await;
    assert_eq!((s, &made["key"]["allow"]["routes"]), (200, &json!(["fleet"])), "{made}");
}

/// MARCO-301: the key the logs guide makes for the rotation (`oncall`)
/// reads an alert's page and the runbooks, and acknowledges, silences and
/// notes it, as itself in the audit log; it still changes no rule nor
/// route, and reads no admin route. A key limited to tables without the
/// `alerts` group does none of it, and one given it that does not write
/// reads the alert alone. `/v1/whoami` lists the routes each may call.
#[tokio::test(flavor = "multi_thread")]
async fn an_oncall_key_acknowledges_silences_and_notes_an_alert() {
    let dir = tempfile::tempdir().unwrap();
    let (base, svc) = start(dir.path(), Vec::new()).await;
    let rules = json!({"rules": [{"id": "payments-errors", "name": "Payments errors", "metric": "p99_ms", "op": ">", "threshold": 100.0, "eval": "mean", "window_s": 60, "severity": "critical"}]});
    let (s, j) = call(&base, "PUT", "/v1/alerts/rules", ROOT, Some(rules.clone())).await;
    assert_eq!(s, 200, "{j}");
    let t = chloedb_server::telemetry::now_ms();
    let points: Vec<Arc<Json>> = (0..31u64).map(|i| Arc::new(json!({"t": t - (30 - i) * 2000, "p99_ms": 300.0}))).collect();
    let fired = svc.alerts.check(&points, t);
    assert_eq!(fired.iter().map(|e| (e.rule.as_str(), e.state)).collect::<Vec<_>>(), [("payments-errors", "firing")]);
    chloedb_server::alerts::deliver(&svc, fired);

    let oncall = key(&base, json!({"name": "netops", "preset": "oncall", "prefix": "logs_*"})).await;
    let k = Some(oncall.as_str());
    let (s, j) = call(&base, "GET", "/v1/alerts/payments-errors", k, None).await;
    assert_eq!((s, j["rule"].as_str().or(j["state"]["rule"].as_str())), (200, Some("payments-errors")), "{j}");
    let (s, j) = call(&base, "GET", "/v1/runbooks?for=payments-errors", k, None).await;
    assert_eq!(s, 200, "{j}");
    let (s, j) = call(&base, "POST", "/v1/alerts/payments-errors/ack", k, Some(json!({"note": "on it"}))).await;
    assert_eq!((s, j["ack"]["by"].as_str()), (200, Some("netops")), "{j}");
    let (s, j) = call(&base, "POST", "/v1/alerts/payments-errors/silence", k, Some(json!({"minutes": 30, "reason": "rolling back"}))).await;
    assert_eq!((s, j["silenced"]["reason"].as_str()), (200, Some("rolling back")), "{j}");
    let (s, j) = call(&base, "POST", "/v1/alerts/payments-errors/notes", k, Some(json!({"text": "the deploy of 14:02"}))).await;
    assert_eq!(s, 200, "{j}");
    assert!(j["notes"].as_array().unwrap().iter().any(|n| n["by"] == "netops" && n["text"] == "the deploy of 14:02"), "{j}");
    let (s, j) = call(&base, "DELETE", "/v1/alerts/payments-errors/silence", k, None).await;
    assert_eq!(s, 200, "{j}");
    // Its own name in the audit log.
    let (_, audit) = call(&base, "GET", "/v1/admin/audit", ROOT, None).await;
    let ack = audit["entries"].as_array().unwrap().iter().find(|e| e["route"] == "/v1/alerts/:rule/ack").expect("audited");
    assert_eq!(ack["who"], "netops", "{ack}");

    // Still not the rules, the routes, a backtest, the server's own log, or
    // any admin route.
    for (method, path, body) in [
        ("PUT", "/v1/alerts/rules", Some(rules.clone())),
        ("GET", "/v1/alerts/routes", None),
        ("PUT", "/v1/alerts/routes", Some(json!({"routes": []}))),
        ("POST", "/v1/alerts/routes/x/test", Some(json!({}))),
        ("POST", "/v1/alerts/backtest", Some(json!({"rule": rules["rules"][0], "from": t - 60_000, "to": t}))),
        ("GET", "/v1/alerts/peer", None),
        ("GET", "/v1/logs", None),
        ("POST", "/v1/logs/query", Some(json!({"sql": "SELECT * FROM logs LIMIT 1"}))),
        ("GET", "/v1/admin/audit", None),
        ("GET", "/v1/admin/keys", None),
        ("POST", "/v1/admin/oncall", Some(json!({"alert": "payments-errors"}))),
        ("POST", "/v1/events/notes", Some(json!({"text": "x"}))),
    ] {
        let (s, j) = call(&base, method, path, k, body).await;
        assert_eq!(s, 403, "{method} {path}: {j}");
    }
    let (_, me) = call(&base, "GET", "/v1/whoami", k, None).await;
    let routes: Vec<&str> = me["may"]["routes"].as_array().unwrap_or_else(|| panic!("{me}")).iter().filter_map(Json::as_str).collect();
    for r in ["GET /v1/alerts", "GET /v1/alerts/:rule", "GET /v1/runbooks", "POST /v1/alerts/:rule/ack", "POST /v1/alerts/:rule/silence", "DELETE /v1/alerts/:rule/silence", "POST /v1/alerts/:rule/notes", "GET /v1/events", "POST /v1/sql"] {
        assert!(routes.contains(&r), "{r}: {routes:?}");
    }
    for r in ["PUT /v1/alerts/rules", "GET /v1/alerts/routes", "POST /v1/alerts/backtest", "GET /v1/logs", "GET /v1/admin/keys"] {
        assert!(!routes.contains(&r), "{r}: {routes:?}");
    }

    // A key limited to tables without the group: the list of what fires,
    // and nothing of one alert.
    let fleet = key(&base, json!({"name": "shipper", "tables": {"logs_*": "write"}, "routes": ["fleet"]})).await;
    for (method, path, body) in [("GET", "/v1/alerts/payments-errors", None), ("GET", "/v1/runbooks", None), ("POST", "/v1/alerts/payments-errors/notes", Some(json!({"text": "x"})))] {
        let (s, j) = call(&base, method, path, Some(&fleet), body).await;
        assert_eq!(s, 403, "{method} {path}: {j}");
        assert!(j["error"].as_str().unwrap().contains("not for a key limited to tables"), "{j}");
    }
    assert_eq!(call(&base, "GET", "/v1/alerts", Some(&fleet), None).await.0, 200);
    // Given the group without writing: it reads the alert, and changes nothing.
    let (s, made) = call(&base, "POST", "/v1/admin/keys", ROOT, Some(json!({"name": "viewer", "tables": {"logs_*": "read"}, "routes": ["alerts"]}))).await;
    assert_eq!((s, &made["key"]["allow"]["routes"]), (200, &json!(["alerts"])), "{made}");
    let viewer = Some(made["secret"].as_str().unwrap());
    assert_eq!(call(&base, "GET", "/v1/alerts/payments-errors", viewer, None).await.0, 200);
    let (s, j) = call(&base, "POST", "/v1/alerts/payments-errors/notes", viewer, Some(json!({"text": "x"}))).await;
    assert_eq!(s, 403, "{j}");
    assert!(j["error"].as_str().unwrap().contains("write scope"), "{j}");
    let (_, me) = call(&base, "GET", "/v1/whoami", viewer, None).await;
    let routes = me["may"]["routes"].to_string();
    assert!(routes.contains("\"GET /v1/alerts/:rule\"") && !routes.contains("POST /v1/alerts/:rule/notes"), "{routes}");
    // A key of every table has no such list: its scope says what it may.
    let (_, me) = call(&base, "GET", "/v1/whoami", ROOT, None).await;
    assert!(me["may"].get("routes").is_none(), "{me}");
}

/// The logs quickstart (docs/guide/logs.md): an `oncall` key imports JSON
/// lines into a log table without a key (inserted, as a log has none),
/// and reads them by the last minutes and per minute.
#[tokio::test(flavor = "multi_thread")]
async fn an_oncall_key_imports_and_searches_log_lines() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _svc) = start(dir.path(), Vec::new()).await;
    ok(&base, "CREATE TABLE logs_syslog (ts TIMESTAMP NOT NULL, severity KEYWORD, host KEYWORD OPTIONS(substring = true), body TEXT OPTIONS(substring = true), attributes JSON) OPTIONS(retain = '30 days')").await;
    let k = key(&base, json!({"name": "netops", "preset": "oncall", "prefix": "logs_*"})).await;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_micros() as i64;
    let at = |ago_s: i64| chloedb::timestamp::format(now - ago_s * 1_000_000);
    let mut lines = String::new();
    for i in 0..30 {
        let (sev, body) = if i % 3 == 0 { ("error", "%LINK-3-UPDOWN: Interface GigabitEthernet0/1, changed state to down") } else { ("info", "%SYS-5-CONFIG_I: Configured from console") };
        lines += &format!("{}\n", json!({"ts": at(i * 60 + 5), "severity": sev, "host": "core-rtr-02", "body": body, "attributes": {"iface": "Gi0/1"}}));
    }
    let c = chloedb_server::http_client().build().unwrap();
    // Twice: a table without a key keeps each line it is sent.
    for _ in 0..2 {
        let r = c.post(format!("{base}/v1/import?table=logs_syslog&format=jsonl")).bearer_auth(&k).header("content-type", "application/x-ndjson").body(lines.clone()).send().await.unwrap();
        assert_eq!(r.status(), 200, "{}", r.text().await.unwrap());
    }
    let rows = |sql: &str| {
        let (c, base, k, sql) = (c.clone(), base.clone(), k.clone(), sql.to_string());
        async move {
            let r = c.post(format!("{base}/v1/sql")).bearer_auth(&k).json(&json!({"sql": sql})).send().await.unwrap();
            let j: Json = r.json().await.unwrap();
            j["rows"].as_array().unwrap_or_else(|| panic!("{sql}: {j}")).clone()
        }
    };
    assert_eq!(rows("SELECT count(*) FROM logs_syslog").await, vec![json!([60])]);
    // The last 15 minutes' errors: the lines 5, 185, 365, 545 and 725 s ago, twice.
    assert_eq!(rows("SELECT count(*) FROM logs_syslog WHERE severity = 'error' AND body LIKE '%GigabitEthernet0/1,%' AND ts > now() - INTERVAL '15 minutes'").await, vec![json!([10])]);
    // Per minute: one bucket a line, 30 of them, a minute apart.
    let per = rows("SELECT time_bucket('1 minute', ts) AS m, count(*) AS n FROM logs_syslog GROUP BY m").await;
    assert_eq!(per.len(), 30, "{per:?}");
    assert!(per.iter().all(|r| r[1] == json!(2) && chloedb::timestamp::parse(r[0].as_str().unwrap()).unwrap() % 60_000_000 == 0),"{per:?}");
    let (s, j) = call(&base, "POST", "/v1/sql", Some(&k), Some(json!({"sql": "SELECT count(*) FROM products"}))).await;
    assert_eq!(s, 403, "{j}");
}

/// Apps: one that checks its callers takes a key limited to tables and a
/// write key's changes; one that does not takes neither, but lets a key
/// given it that reads every table read through it.
#[tokio::test(flavor = "multi_thread")]
async fn apps_by_whether_they_check_their_callers() {
    let mut urls = Vec::new();
    let seen: Arc<Mutex<Vec<HeaderMap>>> = Arc::default();
    for checks in [true, false] {
        let s = seen.clone();
        let app = Router::new()
            .route("/app.json", axum::routing::get(move || async move { axum::Json(json!({"name": "x", "title": "X", "checks_callers": checks})) }))
            .fallback(move |h: HeaderMap| {
                let s = s.clone();
                async move {
                    s.lock().unwrap().push(h);
                    axum::Json(json!({"ok": true}))
                }
            });
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        urls.push(format!("http://127.0.0.1:{}", l.local_addr().unwrap().port()));
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    }
    let dir = tempfile::tempdir().unwrap();
    let (base, _svc) = start(dir.path(), vec![format!("chloectx={}", urls[0]), format!("sheets={}", urls[1])]).await;
    let limited = key(&base, json!({"name": "ops", "tables": {"ctx_ops_*": "write"}})).await;
    let writer = key(&base, json!({"name": "writer", "scope": "write"})).await;
    let analyst = key(&base, json!({"name": "ana", "preset": "analyst", "prefix": "ana_*"})).await;

    // The listing, and whether each checks its callers.
    let (s, j) = call(&base, "GET", "/v1/apps", Some(&limited), None).await;
    assert_eq!(s, 200, "{j}");
    let checks: Vec<(String, bool)> = j["apps"].as_array().unwrap().iter().map(|a| (a["name"].as_str().unwrap().to_string(), a["checks_callers"].as_bool().unwrap())).collect();
    assert_eq!(checks, [("chloectx".to_string(), true), ("sheets".to_string(), false)], "{j}");

    let (s, j) = call(&base, "POST", "/v1/apps/chloectx/v1/spaces/ops_a/notes", Some(&limited), Some(json!({"kind": "fact", "text": "x"}))).await;
    assert_eq!(s, 200, "{j}");
    let (s, _) = call(&base, "POST", "/v1/apps/chloectx/v1/spaces/ops_a/notes", Some(&writer), Some(json!({"kind": "fact", "text": "x"}))).await;
    assert_eq!(s, 200);
    {
        let seen = seen.lock().unwrap();
        assert_eq!(seen[0].get("x-chloedb-tables").unwrap(), "ctx_ops_*=write");
        assert_eq!(seen[1].get("x-chloedb-scope").unwrap(), "write");
    }
    for (who, method) in [(&limited, "GET"), (&limited, "POST"), (&writer, "POST"), (&analyst, "POST")] {
        let (s, j) = call(&base, method, "/v1/apps/sheets/v1/books", Some(who), (method == "POST").then(|| json!({}))).await;
        assert_eq!(s, 403, "{method}: {j}");
    }
    assert_eq!(call(&base, "GET", "/v1/apps/sheets/v1/books", Some(&writer), None).await.0, 200);
    // The analyst was given Sheets, and reads every table: it reads through it.
    let (s, j) = call(&base, "GET", "/v1/apps/sheets/v1/books", Some(&analyst), None).await;
    assert_eq!(s, 200, "{j}");
    assert_eq!(call(&base, "POST", "/v1/apps/sheets/v1/books", ROOT, Some(json!({}))).await.0, 200);
}

/// An app that checks its callers runs a caller's statement with its own
/// token narrowed to the caller's tables (`x-chloedb-as-tables`): checked
/// as the caller's key would be, never wider than the token, and never a
/// way for a key limited to tables to change its own grants.
#[tokio::test(flavor = "multi_thread")]
async fn an_app_token_narrowed_to_a_callers_tables() {
    let dir = tempfile::tempdir().unwrap();
    let (base, _svc) = start(dir.path(), Vec::new()).await;
    ok(&base, "CREATE TABLE ana_x (id KEYWORD PRIMARY KEY, n INT)").await;
    let c = chloedb_server::http_client().build().unwrap();
    let as_ = |auth: &str, grants: &str, sql: &str| c.post(format!("{base}/v1/sql")).bearer_auth(auth).header("x-chloedb-as-tables", grants).json(&json!({"sql": sql})).send();
    let status = |r: reqwest::Response| r.status().as_u16();
    // Read on every table: reads, but neither writes nor makes a table.
    assert_eq!(status(as_("root-token", "*=read", "SELECT id FROM products LIMIT 1").await.unwrap()), 200);
    assert_eq!(status(as_("root-token", "*=read", "INSERT INTO ana_x (id, n) VALUES ('a', 1)").await.unwrap()), 403);
    assert_eq!(status(as_("root-token", "*=read", "CREATE TABLE ana_y (id KEYWORD PRIMARY KEY)").await.unwrap()), 403);
    // The analyst's: reads everything, admin of its own tables only.
    assert_eq!(status(as_("root-token", "*=read,ana_*=admin", "CREATE TABLE ana_y (id KEYWORD PRIMARY KEY)").await.unwrap()), 200);
    assert_eq!(status(as_("root-token", "*=read,ana_*=admin", "CREATE TABLE other_y (id KEYWORD PRIMARY KEY)").await.unwrap()), 403);
    // Its own routes only: what a key limited to tables may call.
    let r = c.get(format!("{base}/v1/admin/keys")).bearer_auth("root-token").header("x-chloedb-as-tables", "*=admin").send().await.unwrap();
    assert_eq!(r.status().as_u16(), 403);
    // Never wider than the credential: a read key narrowed to admin grants reads.
    let reader = key(&base, json!({"name": "reader", "scope": "read"})).await;
    assert_eq!(status(as_(&reader, "*=admin", "CREATE TABLE ana_z (id KEYWORD PRIMARY KEY)").await.unwrap()), 403);
    assert_eq!(status(as_(&reader, "*=admin", "SELECT id FROM products LIMIT 1").await.unwrap()), 200);
    // A key limited to tables cannot narrow itself into other grants: it
    // keeps, on each table, the lower of its scope and the caller's.
    let limited = key(&base, json!({"name": "lim", "tables": {"ana_*": "read"}})).await;
    assert_eq!(status(as_(&limited, "*=admin", "SELECT id FROM products LIMIT 1").await.unwrap()), 403);
    assert_eq!(status(as_(&limited, "*=admin", "SELECT id FROM ana_x LIMIT 1").await.unwrap()), 200);
    assert_eq!(status(as_(&limited, "*=admin", "INSERT INTO ana_x (id, n) VALUES ('b', 1)").await.unwrap()), 403);
    // A key like the one a server gives Sheets (DAT-5): the root's call
    // through it makes Sheets' tables and reads the rest; a writer's
    // writes them and makes none; a caller of other tables reads only
    // those.
    let sheets = key(&base, json!({"name": "sheets-like", "tables": {"ana_*": "admin", "*": "read"}})).await;
    assert_eq!(status(as_(&sheets, "*=admin", "CREATE TABLE ana_w (id KEYWORD PRIMARY KEY)").await.unwrap()), 200);
    assert_eq!(status(as_(&sheets, "*=admin", "SELECT id FROM products LIMIT 1").await.unwrap()), 200);
    assert_eq!(status(as_(&sheets, "*=admin", "CREATE TABLE other_w (id KEYWORD PRIMARY KEY)").await.unwrap()), 403);
    assert_eq!(status(as_(&sheets, "*=write", "INSERT INTO ana_x (id, n) VALUES ('c', 1)").await.unwrap()), 200);
    assert_eq!(status(as_(&sheets, "*=write", "CREATE TABLE ana_v (id KEYWORD PRIMARY KEY)").await.unwrap()), 403);
    assert_eq!(status(as_(&sheets, "*=write", "INSERT INTO products (id) VALUES ('p-new')").await.unwrap()), 403);
    assert_eq!(status(as_(&sheets, "ana_x=write", "SELECT id FROM products LIMIT 1").await.unwrap()), 403);
    assert_eq!(status(as_(&sheets, "ana_x=write", "SELECT id FROM ana_x LIMIT 1").await.unwrap()), 200);
    // A bad grant is refused, not ignored.
    assert_eq!(status(as_("root-token", "ana_x=sometimes", "SELECT 1").await.unwrap()), 400);
}
