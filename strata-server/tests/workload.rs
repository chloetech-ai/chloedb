//! What the command center's workload views read: points that tell the
//! server's failures from the client's, statements over a window, the
//! identity of each query, running queries by shape, and Contain rules.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value as Json};
use strata::db::Db;
use strata::storage::DbOptions;
use strata_server::service::{AgentConfig, Backend, Service};
use strata_server::{demo, server};
use tokio::net::TcpListener;

const N: usize = 4000;

/// Every product meets every product of its category, and no pair passes
/// the rest of the condition: a query that runs a while.
const JOIN: &str = "SELECT x.id, y.id FROM products x JOIN products y ON x.category = y.category AND x.price < y.price - 1000000";

fn open(dir: &std::path::Path, name: &str) -> Arc<Db> {
    let db = Db::create(&dir.join(name), demo::schema(N), DbOptions { publish_snapshots: true, ..DbOptions::default() }).unwrap();
    db.set_embedder("emb", Arc::new(strata::embed::HashEmbedder { dim: demo::DIM })).unwrap();
    db.load((0..N).map(|i| demo::product(i, 7))).unwrap();
    Arc::new(db)
}

fn service(db: Arc<Db>) -> Service {
    let agent = AgentConfig { api_key: None, model: String::new(), base_url: String::new() };
    let schema = (*db.schema()).clone();
    Service::new(Backend::Local(db), schema, agent, 16, Duration::from_secs(60))
}

async fn http(svc: Arc<Service>) -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
    tokio::spawn(server::serve_http(svc, l, Default::default()));
    url
}

async fn get(base: &str, path: &str) -> Json {
    let r = strata_server::http_client().build().unwrap().get(format!("{base}{path}")).send().await.unwrap();
    assert_eq!(r.status(), 200, "{path}");
    r.json().await.unwrap()
}

async fn query(base: &str, body: Json) -> (u16, Json) {
    let r = strata_server::http_client().build().unwrap().post(format!("{base}/v1/query")).json(&body).send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or(Json::Null))
}

/// The latest point, after a new sample.
fn point(svc: &Service) -> Json {
    (*svc.series.add(svc.sample()).expect("a point")).clone()
}

/// A point of an interval in which no query ended has no error ratio (no
/// data, not none failing); a client's bad SQL and its own deadline are
/// counted apart from the server's failures; no quantile exceeds the
/// slowest query.
#[tokio::test(flavor = "multi_thread")]
async fn points_tell_server_failures_from_the_client_s() {
    let dir = tempfile::tempdir().unwrap();
    let svc = Arc::new(service(open(dir.path(), "one.strata")));
    let base = http(svc.clone()).await;
    svc.series.add(svc.sample());

    let quiet = point(&svc);
    assert_eq!(quiet["queries"], 0, "{quiet}");
    assert!(quiet.get("error_ratio").is_none(), "no query ended, so no ratio: {quiet}");
    assert!(quiet.get("availability").is_none(), "{quiet}");
    assert!(quiet["p99_ms"].is_null(), "{quiet}");

    for _ in 0..3 {
        let (status, body) = query(&base, json!({"sql": "SELECT id FROM products WHERE MATCH(body, 'laptop') LIMIT 5"})).await;
        assert_eq!(status, 200, "{body}");
    }
    let ok = point(&svc);
    assert_eq!((ok["queries"].as_u64(), ok["error_ratio"].as_f64(), ok["availability"].as_f64()), (Some(3), Some(0.0), Some(1.0)), "{ok}");
    let (p99, max) = (ok["p99_ms"].as_f64().unwrap(), ok["max_ms"].as_f64().unwrap());
    assert!(p99 <= max, "p99 {p99} above the slowest query {max}");

    // Bad SQL is the client's error; a 1 ms deadline the client asked
    // for is its own deadline. Neither is a failure of the server.
    let (status, _) = query(&base, json!({"sql": "SELECT id FROM products WHERE name = 'unterminated"})).await;
    assert_eq!(status, 400);
    let (status, body) = query(&base, json!({"sql": JOIN, "timeout_ms": 1})).await;
    assert_eq!(status, 504, "{body}");
    let p = point(&svc);
    assert_eq!((p["queries"].as_u64(), p["query_errors"].as_u64(), p["client_errors"].as_u64(), p["client_deadlines"].as_u64()), (Some(2), Some(0), Some(1), Some(1)), "{p}");
    assert_eq!(p["error_ratio"], 0.0, "{p}");
    assert_eq!(p["availability"], 1.0, "{p}");

    let recent = get(&base, "/v1/queries/recent?limit=2").await;
    let outcomes: Vec<&str> = recent["queries"].as_array().unwrap().iter().map(|q| q["outcome"].as_str().unwrap()).collect();
    assert_eq!(outcomes, ["client_deadline", "client_error"], "{recent}");
    assert_eq!(recent["queries"][0]["deadline_ms"], 1, "{recent}");

    // The series answers the same: the quiet point without a ratio.
    let series = get(&base, "/v1/timeseries").await;
    let points = series["points"].as_array().unwrap();
    assert!(points.iter().any(|p| p["queries"] == 0 && p.get("error_ratio").is_none()), "{series}");
    let metrics = strata_server::http_client().build().unwrap().get(format!("{base}/metrics")).send().await.unwrap().text().await.unwrap();
    assert!(metrics.contains("outcome=\"client_deadline\"} 1"), "{metrics}");
    assert!(metrics.contains("outcome=\"client_error\"} 1"), "{metrics}");
}

async fn call(method: reqwest::Method, base: &str, path: &str, key: &str, headers: &[(&str, &str)], body: Option<Json>) -> (u16, Json) {
    let mut r = strata_server::http_client().build().unwrap().request(method, format!("{base}{path}")).bearer_auth(key);
    for (k, v) in headers {
        r = r.header(*k, *v);
    }
    if let Some(b) = body {
        r = r.json(&b);
    }
    let r = r.send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or(Json::Null))
}

/// A server with an admin key ("ops"), and a read key ("reports-batch").
async fn keyed(svc: &Arc<Service>) -> (String, String, String) {
    let base = http(svc.clone()).await;
    let (status, ops) = call(reqwest::Method::POST, &base, "/v1/admin/keys", "", &[], Some(json!({"name": "ops", "scope": "admin"}))).await;
    assert_eq!(status, 200, "{ops}");
    let ops = ops["secret"].as_str().unwrap().to_string();
    let (status, rb) = call(reqwest::Method::POST, &base, "/v1/admin/keys", &ops, &[], Some(json!({"name": "reports-batch", "scope": "read"}))).await;
    assert_eq!(status, 200, "{rb}");
    (base, ops, rb["secret"].as_str().unwrap().to_string())
}

/// Each query is kept with who sent it (the key's name and id, the
/// client's address and app), its parameters and its deadline, running
/// and finished, and so is it in the log store; the command center's own
/// are tagged.
#[tokio::test(flavor = "multi_thread")]
async fn queries_are_kept_with_who_sent_them() {
    let dir = tempfile::tempdir().unwrap();
    let mut svc = service(open(dir.path(), "one.strata"));
    let store = strata_server::logstore::LogStore::open(strata_server::logstore::LogOptions { capture: false, ..strata_server::logstore::LogOptions::new(dir.path().join("one.logs"), "n1".into()) }).unwrap();
    let keep = store.clone();
    let _ = svc.query_log.keep.set(Box::new(move |q, slow| keep.keep_query(q, slow)));
    svc.log_store = Some(store.clone());
    let svc = Arc::new(svc);
    let (base, ops, rb) = keyed(&svc).await;

    let sql = "SELECT id FROM products WHERE MATCH(body, :q) AND price < :p LIMIT 5";
    let agent = [("user-agent", "nightly-reports/2.3 (linux)")];
    let (status, body) = call(reqwest::Method::POST, &base, "/v1/query", &rb, &agent, Some(json!({"sql": sql, "params": {"q": "laptop", "p": 500}, "timeout_ms": 2000}))).await;
    assert_eq!(status, 200, "{body}");
    let ui = [("x-strata-client", "command-center")];
    let (status, _) = call(reqwest::Method::POST, &base, "/v1/query", &ops, &ui, Some(json!({"sql": "SELECT COUNT(*) FROM products"}))).await;
    assert_eq!(status, 200);

    let (_, recent) = call(reqwest::Method::GET, &base, "/v1/queries/recent?limit=2", &ops, &[], None).await;
    let (mine, theirs) = (&recent["queries"][1], &recent["queries"][0]);
    assert_eq!(mine["key"], "reports-batch", "{recent}");
    assert!(mine["key_id"].as_str().is_some_and(|k| !k.is_empty()), "{mine}");
    assert_eq!(mine["app"], "nightly-reports", "{mine}");
    assert_eq!(mine["addr"], "127.0.0.1", "{mine}");
    assert_eq!(mine["params"], json!({"p": 500, "q": "laptop"}), "{mine}");
    assert_eq!(mine["deadline_ms"], 2000);
    assert!(mine.get("client").is_none(), "{mine}");
    assert_eq!((theirs["key"].as_str(), theirs["client"].as_str()), (Some("ops"), Some("command-center")), "{theirs}");

    // A running query says who sent it too.
    let b = base.clone();
    let rb2 = rb.clone();
    let run = tokio::spawn(async move { call(reqwest::Method::POST, &b, "/v1/query", &rb2, &[("x-strata-app", "billing")], Some(json!({"sql": JOIN, "timeout_ms": 60_000}))).await });
    let id = loop {
        let (_, live) = call(reqwest::Method::GET, &base, "/v1/queries", &ops, &[], None).await;
        if let Some(q) = live["queries"].as_array().unwrap().iter().find(|q| q["sql"] == JOIN) {
            assert_eq!((q["key"].as_str(), q["app"].as_str()), (Some("reports-batch"), Some("billing")), "{q}");
            assert!(q["fingerprint"].as_str().is_some(), "{q}");
            break q["id"].as_u64().unwrap();
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    };
    let (status, _) = call(reqwest::Method::DELETE, &base, &format!("/v1/queries/{id}"), &ops, &[], None).await;
    assert_eq!(status, 200);
    assert_eq!(run.await.unwrap().0, 499);

    // The log store keeps them with their columns.
    store.flush();
    let (rs, _) = store.query("SELECT key_name, app, client, params, deadline_ms FROM queries WHERE key_name = 'reports-batch' ORDER BY ts LIMIT 5", &strata::query::Params::new(), false, Duration::from_secs(5)).await.unwrap();
    assert_eq!(rs.rows.len(), 2, "{:?}", rs.rows);
    assert_eq!(rs.rows[0][1], strata::Value::Str("nightly-reports".into()));
    let params: Json = serde_json::from_str(rs.rows[0][3].as_str().unwrap()).unwrap();
    assert_eq!(params["q"], "laptop");
    assert_eq!(rs.rows[0][4], strata::Value::Int(2000));
    let (rs, _) = store.query("SELECT COUNT(*) FROM queries WHERE client = 'command-center'", &strata::query::Params::new(), false, Duration::from_secs(5)).await.unwrap();
    assert_eq!(rs.rows[0][0], strata::Value::Int(1));
}

/// Statements are counted a minute at a time: a window's statements come
/// with the window before (`prev`), their keys, and whether they are new;
/// one statement's page has its minutes and slowest calls with who sent
/// them.
#[tokio::test(flavor = "multi_thread")]
async fn statements_compare_with_the_window_before() {
    let dir = tempfile::tempdir().unwrap();
    let svc = Arc::new(service(open(dir.path(), "one.strata")));
    let (base, ops, rb) = keyed(&svc).await;
    let now = strata_server::telemetry::now_ms() / 60_000 * 60_000;
    let sql = "SELECT id FROM products WHERE MATCH(body, 'laptop') LIMIT 5";
    let fp = strata_server::telemetry::fingerprint(sql);
    // The same statement 20 minutes ago, as the server counted it then.
    let then = strata_server::telemetry::Finished { id: 999, ts_ms: now - 20 * 60_000, sql: sql.into(), fingerprint: fp.clone(), kind: "bm25", outcome: "ok", ms: 3.0, rows: 5, ..Default::default() };
    svc.query_log.statements.add(&Arc::new(then));
    for q in ["laptop", "phone"] {
        let (status, body) = call(reqwest::Method::POST, &base, "/v1/query", &rb, &[], Some(json!({"sql": sql.replace("laptop", q)}))).await;
        assert_eq!(status, 200, "{body}");
    }
    let ui = [("x-strata-client", "command-center")];
    let (status, _) = call(reqwest::Method::POST, &base, "/v1/query", &ops, &ui, Some(json!({"sql": "SELECT COUNT(*) FROM products"}))).await;
    assert_eq!(status, 200);

    let (from, to) = (now - 15 * 60_000, strata_server::telemetry::now_ms() + 1);
    let (status, j) = call(reqwest::Method::GET, &base, &format!("/v1/queries/stats?from={from}&to={to}"), &ops, &[], None).await;
    assert_eq!(status, 200, "{j}");
    assert_eq!(j["prev"]["to"], from, "{j}");
    let list = j["statements"].as_array().unwrap();
    let s = list.iter().find(|s| s["fingerprint"] == fp.as_str()).expect("the statement");
    assert_eq!(s["calls"], 2, "{s}");
    assert_eq!(s["outcomes"], json!({"ok": 2}));
    assert_eq!(s["prev"]["calls"], 1, "{s}");
    assert_eq!(s["first_seen_ms"], now - 20 * 60_000);
    assert_eq!(s["keys"][0], json!({"key": "reports-batch", "calls": 2, "total_ms": s["total_ms"], "errors": 0}));
    assert!(s["p99_ms"].as_f64().unwrap() <= s["max_ms"].as_f64().unwrap());
    let count = list.iter().find(|s| s["sql"] == "SELECT COUNT(*) FROM products").unwrap();
    assert!(count["prev"].is_null(), "{count}");
    // The command center's own statement says so, and its sample whose.
    assert_eq!((count["command_center"].as_bool(), count["sample"]["client"].as_str()), (Some(true), Some("command-center")), "{count}");
    assert_eq!(s["command_center"], false);
    assert_eq!(s["sample"]["key"], "reports-batch", "{s}");

    // Its page, by id or by fingerprint.
    for which in [s["id"].as_str().unwrap().to_string(), fp.clone()] {
        let path = format!("/v1/queries/stats/{}?from={}&to={to}", urlencoding(&which), now - 30 * 60_000);
        let (status, d) = call(reqwest::Method::GET, &base, &path, &ops, &[], None).await;
        assert_eq!(status, 200, "{d}");
        assert_eq!(d["statement"]["calls"], 3, "{d}");
        assert!(d["statement"]["share"].as_f64().unwrap() < 1.0, "of the window's time over every statement: {d}");
        assert!(d["trend"].as_array().unwrap().len() >= 2, "{d}");
        assert!(d["slowest"].as_array().unwrap().iter().any(|c| c["id"] == 999 && c["ms"] == 3.0), "{d}");
        assert!(d["latest"].as_array().unwrap().iter().any(|c| c["key"] == "reports-batch"), "{d}");
    }
    let (status, _) = call(reqwest::Method::GET, &base, "/v1/queries/stats/0000000000000000", &ops, &[], None).await;
    assert_eq!(status, 404);

    // A reset starts again, and is audited.
    let (status, _) = call(reqwest::Method::POST, &base, "/v1/queries/stats/reset", &ops, &[], Some(json!({}))).await;
    assert_eq!(status, 200);
    let (_, j) = call(reqwest::Method::GET, &base, "/v1/queries/stats", &ops, &[], None).await;
    assert_eq!(j["count"], 0, "{j}");
    let (_, audit) = call(reqwest::Method::GET, &base, "/v1/admin/audit", &ops, &[], None).await;
    assert!(audit.to_string().contains("reset this server's statement statistics"), "{audit}");
}

fn urlencoding(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// Running queries come grouped by statement shape and key, and are
/// cancelled together by shape and key, audited with the reason given. A
/// Contain rule refuses a statement from a key with 429 naming the rule
/// and its reason, until it ends; a limit lets some run at once and
/// refuses the rest once their deadline passes waiting.
#[tokio::test(flavor = "multi_thread")]
async fn running_queries_are_grouped_cancelled_and_contained() {
    use reqwest::Method;
    use strata_server::telemetry::{Caller, Meta};
    let dir = tempfile::tempdir().unwrap();
    let svc = Arc::new(service(open(dir.path(), "one.strata")));
    let (base, ops, rb) = keyed(&svc).await;
    // Queries running, as the server lists them: three of one key, one of
    // another.
    let of = |key: &str| Meta { caller: Some(Arc::new(Caller { key: Some(key.into()), ..Default::default() })), ..Default::default() };
    let held: Vec<_> = (0..3).map(|_| svc.live.start(JOIN, "sql", false, &of("reports-batch"))).collect();
    let other = svc.live.start(JOIN, "sql", false, &of("ops"));
    let fp = strata_server::telemetry::fingerprint(JOIN);
    let join = urlencoding(&fp);
    let (status, j) = call(Method::GET, &base, "/v1/queries?group=shape", &ops, &[], None).await;
    assert_eq!(status, 200, "{j}");
    let groups = j["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 2, "{j}");
    let g = groups.iter().find(|g| g["key"] == "reports-batch").unwrap();
    assert_eq!((g["fingerprint"].as_str(), g["count"].as_u64(), g["ids"].as_array().unwrap().len()), (Some(fp.as_str()), Some(3), 3), "{g}");
    assert_eq!(call(Method::GET, &base, "/v1/queries?group=nothing", &ops, &[], None).await.0, 400);
    assert_eq!(call(Method::DELETE, &base, "/v1/queries", &ops, &[], None).await.0, 400, "a statement must be named");

    // One key's, then the rest.
    let why = [("x-strata-reason", "a runaway join")];
    let (status, j) = call(Method::DELETE, &base, &format!("/v1/queries?fingerprint={join}&key=reports-batch"), &ops, &why, None).await;
    assert_eq!((status, j["cancelled"].as_u64()), (200, Some(3)), "{j}");
    for l in &held {
        tokio::time::timeout(Duration::from_secs(1), l.query.cancelled()).await.expect("cancelled");
    }
    let (_, live) = call(Method::GET, &base, "/v1/queries", &ops, &[], None).await;
    assert!(live["queries"].as_array().unwrap().iter().any(|q| q["key"] == "ops" && q["cancelled"] == false), "{live}");
    let (_, j) = call(Method::DELETE, &base, &format!("/v1/queries?fingerprint={join}"), &ops, &why, None).await;
    assert_eq!(j["cancelled"], 1, "{j}");
    tokio::time::timeout(Duration::from_secs(1), other.query.cancelled()).await.expect("cancelled");
    let (_, audit) = call(Method::GET, &base, "/v1/admin/audit", &ops, &[], None).await;
    let e = audit["entries"].as_array().unwrap().iter().find(|e| e["route"] == "/v1/queries" && e["method"] == "DELETE" && e["detail"].as_str().is_some_and(|d| d.starts_with("cancelled 3"))).expect("audited");
    assert_eq!(e["reason"], "a runaway join", "{e}");
    assert_eq!(e["before"]["queries"].as_array().unwrap().len(), 3, "{e}");
    drop((held, other));

    // Refused for a second: this statement from reports-batch.
    let sql = "SELECT id FROM products WHERE MATCH(body, 'laptop') LIMIT 5";
    let sfp = strata_server::telemetry::fingerprint(sql);
    let rule = json!({"match": {"fingerprint": sfp, "key": "reports-batch"}, "action": "refuse", "expires_s": 1, "reason": "it floods the shards"});
    let (status, r) = call(Method::POST, &base, "/v1/admin/contain", &ops, &[], Some(rule)).await;
    assert_eq!(status, 200, "{r}");
    let id = r["id"].as_str().unwrap().to_string();
    let (status, j) = call(Method::POST, &base, "/v1/query", &rb, &[], Some(json!({"sql": sql}))).await;
    assert_eq!(status, 429, "{j}");
    let message = j["error"].as_str().unwrap();
    assert!(message.contains(&format!("contain rule {id}")) && message.contains("it floods the shards"), "{message}");
    assert_eq!(call(Method::POST, &base, "/v1/query", &ops, &[], Some(json!({"sql": sql}))).await.0, 200, "another key's runs");
    let (_, rules) = call(Method::GET, &base, "/v1/admin/contain", &ops, &[], None).await;
    assert_eq!((rules[0]["id"].as_str(), rules[0]["hits"].as_u64(), rules[0]["refused"].as_u64(), rules[0]["by"].as_str()), (Some(id.as_str()), Some(1), Some(1), Some("ops")), "{rules}");
    let (_, recent) = call(Method::GET, &base, "/v1/queries/recent?limit=5", &ops, &[], None).await;
    assert!(recent["queries"].as_array().unwrap().iter().any(|q| q["outcome"] == "refused" && q["key"] == "reports-batch"), "{recent}");
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(call(Method::POST, &base, "/v1/query", &rb, &[], Some(json!({"sql": sql}))).await.0, 200, "the rule ended");
    let (_, rules) = call(Method::GET, &base, "/v1/admin/contain", &ops, &[], None).await;
    assert_eq!(rules, json!([]));
    strata_server::contain::sweep(&svc);
    let (_, ev) = call(Method::GET, &base, "/v1/events?kind=contain", &ops, &[], None).await;
    let titles: Vec<&str> = ev["events"].as_array().unwrap().iter().map(|e| e["title"].as_str().unwrap()).collect();
    assert!(titles.iter().any(|t| t.starts_with(&format!("Contain rule {id} ended"))) && titles.iter().any(|t| t.starts_with(&format!("Contain rule {id} refuses"))), "{titles:?}");
    // Its end is audited, by the server, with the rule before; the
    // Timeline says it once (the contain event, not a change too).
    let (_, audit) = call(Method::GET, &base, "/v1/admin/audit?kind=change", &ops, &[], None).await;
    let object = format!("contain rule {id}");
    let ended = audit["entries"].as_array().unwrap().iter().find(|e| e["object"] == object.as_str() && e["reason"] == "expired").unwrap_or_else(|| panic!("the end audited: {audit}"));
    assert_eq!((ended["who"].as_str(), ended["actor"].as_str(), ended["before"]["id"].as_str(), ended["before"]["by"].as_str()), (Some("system"), Some("server"), Some(id.as_str()), Some("ops")), "{ended}");
    assert!(ended["detail"].as_str().is_some_and(|d| d.starts_with(&format!("contain rule {id} ended"))), "{ended}");
    let (_, changes) = call(Method::GET, &base, "/v1/events?kind=change", &ops, &[], None).await;
    assert!(!changes["events"].as_array().unwrap().iter().any(|e| e["detail"]["object"] == object.as_str()), "said once: {changes}");

    // A limit of one at a time: while one holds its turn, the next waits
    // for it, and is refused when its deadline passes.
    let rule = json!({"match": {"fingerprint": sfp}, "action": "limit", "concurrency": 1, "expires_s": 60, "reason": "one at a time"});
    let (status, r) = call(Method::POST, &base, "/v1/admin/contain", &ops, &[], Some(rule)).await;
    assert_eq!(status, 200, "{r}");
    let turn = svc.contain.admit(&sfp, None, None, Duration::from_secs(1)).await.unwrap();
    let (status, j) = call(Method::POST, &base, "/v1/query", &rb, &[], Some(json!({"sql": sql, "timeout_ms": 300}))).await;
    assert_eq!(status, 429, "{j}");
    assert!(j["error"].as_str().unwrap().contains("one at a time"), "{j}");
    drop(turn);
    assert_eq!(call(Method::POST, &base, "/v1/query", &rb, &[], Some(json!({"sql": sql, "timeout_ms": 300}))).await.0, 200, "its turn came");
    let lid = r["id"].as_str().unwrap();
    assert_eq!(call(Method::DELETE, &base, &format!("/v1/admin/contain/{lid}"), &ops, &[], None).await.0, 200);
    assert_eq!(call(Method::DELETE, &base, &format!("/v1/admin/contain/{lid}"), &ops, &[], None).await.0, 404);
    let (_, audit) = call(Method::GET, &base, "/v1/admin/audit", &ops, &[], None).await;
    assert!(audit["entries"].as_array().unwrap().iter().any(|e| e["object"] == format!("contain rule {lid}") && e["before"]["action"] == "limit"), "{audit}");
}
