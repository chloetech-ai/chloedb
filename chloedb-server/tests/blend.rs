//! Blends on a server ([`chloedb::query::blend`]): `CREATE BLEND` and
//! `FROM blend(...)` through `/v1/sql` and `/v1/query`, the `/v1/blends`
//! routes (versions, a draft's preview with its receipts, a new version, a
//! drop), the admin a definition needs and the tables a limited key may
//! read; and through a coordinator over three shards, the one database's
//! slate.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use chloedb::db::Db;
use chloedb::query::Params;
use chloedb::storage::DbOptions;
use chloedb::value::Value;
use chloedb_server::auth::{Access, Secret};
use chloedb_server::coord::Coordinator;
use chloedb_server::keys::{Keys, Principal};
use chloedb_server::service::{After, AgentConfig, Backend, QueryReq, Service, SqlAnswer};
use chloedb_server::{demo, server};
use serde_json::{json, Value as Json};
use tokio::net::TcpListener;

fn agent() -> AgentConfig {
    AgentConfig { api_key: None, model: chloedb_server::agent::DEFAULT_MODEL.into(), base_url: "http://unused".into() }
}

fn create(dir: &Path, name: &str) -> Arc<Db> {
    let opts = DbOptions { segment_max_docs: 6, ..DbOptions::default() };
    Arc::new(Db::create(&dir.join(name), demo::schema(10), opts).unwrap())
}

const TABLES: [&str; 2] = [
    "CREATE TABLE stories (id KEYWORD PRIMARY KEY, title TEXT, author KEYWORD, points INT)",
    "CREATE TABLE comments (id KEYWORD PRIMARY KEY, body TEXT, author KEYWORD, votes INT)",
];

/// 30 stories and 30 comments, every author with several of each.
fn inserts() -> Vec<String> {
    let stories: Vec<String> = (0..30).map(|i| format!("('s{i:02}', 'rust async runtime story {i}', 'au{}', {})", i % 4, 300 - 7 * i)).collect();
    let comments: Vec<String> = (0..30).map(|i| format!("('c{i:02}', 'a comment on rust number {i}', 'cu{}', {})", i % 5, 90 - 2 * i)).collect();
    vec![
        format!("INSERT INTO stories (id, title, author, points) VALUES {}", stories.join(", ")),
        format!("INSERT INTO comments (id, body, author, votes) VALUES {}", comments.join(", ")),
    ]
}

/// Sources ordered by an integer, so that a coordinator merges the very
/// rows one database reads.
const SERP: &str = "CREATE BLEND serp AS
  SOURCES (
    stories  AS (SELECT id, title, author, points AS relevance FROM stories WHERE MATCH(title, :q) ORDER BY points DESC LIMIT 20),
    comments AS (SELECT id, body AS title, author, votes AS relevance FROM comments WHERE MATCH(body, :q) ORDER BY votes DESC LIMIT 20)
  )
  HEADS (relevance CALIBRATE rank)
  SLOT 1 => source = 'stories'
  QUOTA (source = 'comments' AT LEAST 30% IN TOP 10)
  MAX 3 PER author
  WITH (k => 10, note => 'stories first')";

async fn run(svc: &Arc<Service>, s: &str) -> SqlAnswer {
    svc.sql(&Principal::open(), s, &Params::new(), After::Any, 0).await.unwrap_or_else(|e| panic!("{s}: {}", e.message))
}

async fn serve_http(svc: Arc<Service>, access: Access) -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
    tokio::spawn(server::serve_http(svc, l, access));
    base
}

async fn serve_grpc(db: Arc<Db>) -> String {
    let schema = (*db.schema()).clone();
    let svc = Arc::new(Service::new(Backend::Local(db), schema, agent(), 16, Duration::from_secs(10)));
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(server::serve_grpc(svc, l, Access::default()));
    url
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

fn col(j: &Json, at: usize) -> Vec<String> {
    j["rows"].as_array().unwrap_or_else(|| panic!("{j}")).iter().map(|r| r[at].as_str().map_or_else(|| r[at].to_string(), str::to_string)).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_server_blends_through_sql_and_its_routes() {
    let dir = tempfile::tempdir().unwrap();
    let db = create(dir.path(), "a.strata");
    let schema = (*db.schema()).clone();
    let mut svc = Service::new(Backend::Local(db), schema, agent(), 16, Duration::from_secs(10));
    svc.keys = Arc::new(Keys::open(dir.path().join("a.strata.keys.json")).unwrap());
    let svc = Arc::new(svc);
    let base = serve_http(svc.clone(), Access { token: Some(Secret::new("root-token")), ..Default::default() }).await;
    let root = Some("root-token");
    for s in TABLES.iter().map(|s| s.to_string()).chain(inserts()) {
        let (st, j) = call(&base, "POST", "/v1/sql", root, Some(json!({"sql": s}))).await;
        assert_eq!(st, 200, "{j}");
    }
    let (st, j) = call(&base, "POST", "/v1/sql", root, Some(json!({"sql": SERP}))).await;
    assert_eq!(st, 200, "{j}");
    assert_eq!((j["rows"][0][0].as_str(), j["rows"][0][1].as_i64()), (Some("serp"), Some(1)), "{j}");

    // Read through the SQL route and the query route alike.
    let q = "SELECT slot, source, id, author, blend_decision FROM blend('serp', q => 'rust')";
    let (st, a) = call(&base, "POST", "/v1/sql", root, Some(json!({"sql": q}))).await;
    assert_eq!(st, 200, "{a}");
    let (st, b) = call(&base, "POST", "/v1/query", root, Some(json!({"sql": q}))).await;
    assert_eq!(st, 200, "{b}");
    assert_eq!(a["rows"], b["rows"]);
    assert_eq!(col(&a, 2).len(), 10);
    assert_eq!(col(&a, 1)[0], "stories");
    assert!(col(&a, 4)[0].contains("slot 1 is kept"), "{a}");
    assert!(col(&a, 1).iter().filter(|s| *s == "comments").count() >= 3, "{a}");
    let mut per = std::collections::HashMap::new();
    for au in col(&a, 3) {
        *per.entry(au).or_insert(0) += 1;
    }
    assert!(per.values().all(|&n| n <= 3), "{a}");
    // A parameter of the query is a source's parameter.
    let (st, p) = call(&base, "POST", "/v1/query", root, Some(json!({"sql": "SELECT id FROM blend('serp') LIMIT 4", "params": {"q": "rust"}}))).await;
    assert_eq!((st, col(&p, 0).len()), (200, 4), "{p}");

    // The listing and the versions.
    let (st, j) = call(&base, "GET", "/v1/blends", root, None).await;
    assert_eq!(st, 200, "{j}");
    let one = &j["blends"][0];
    assert_eq!((one["name"].as_str(), one["version"].as_i64(), one["note"].as_str(), one["k"].as_u64()), (Some("serp"), Some(1), Some("stories first"), Some(10)), "{j}");
    assert_eq!(one["sources"], json!(["stories", "comments"]));
    let def = one["def"].clone();

    // A draft previewed with its receipts, not stored: comments in slot 1.
    let mut draft = def.clone();
    draft["slots"] = json!([{"slot": 1, "when": {"column": "source", "value": "comments"}}]);
    let (st, r) = call(&base, "POST", "/v1/blends/preview", root, Some(json!({"def": draft, "params": {"q": "rust"}}))).await;
    assert_eq!(st, 200, "{r}");
    assert_eq!(r["rows"][0]["source"], "comments", "{r}");
    assert_eq!(r["rows"].as_array().unwrap().len(), 10);
    assert_eq!(r["version"], 0);
    assert!(r["rows"][0]["why"].as_str().unwrap().contains("slot 1 is kept"), "{r}");
    assert_eq!(r["sources"][0]["name"], "stories");
    assert!(r["sources"][0]["rows"].as_u64().unwrap() > 0, "{r}");
    assert!(!r["quotas"].as_array().unwrap().is_empty(), "{r}");
    assert!(!r["costs"].as_array().unwrap().is_empty(), "costs are asked for by default: {r}");
    assert!(r["sql"].as_str().unwrap().starts_with("CREATE OR REPLACE BLEND serp"), "{r}");
    let (st, j) = call(&base, "GET", "/v1/blends/serp", root, None).await;
    assert_eq!((st, j["versions"].as_array().map(Vec::len)), (200, Some(1)), "a preview stores nothing: {j}");
    // A stored version previewed by its name; a draft that does not hold.
    let (st, r) = call(&base, "POST", "/v1/blends/preview", root, Some(json!({"name": "serp", "params": {"q": "rust"}, "costs": false}))).await;
    assert_eq!((st, r["rows"][0]["source"].as_str(), r["version"].as_i64()), (200, Some("stories"), Some(1)), "{r}");
    assert!(r["costs"].as_array().is_none_or(Vec::is_empty), "{r}");
    let mut bad = def.clone();
    bad["quotas"] = json!([{"when": {"column": "source", "value": "comments"}, "min": 80.0, "max": 20.0}]);
    let (st, r) = call(&base, "POST", "/v1/blends/preview", root, Some(json!({"def": bad, "params": {"q": "rust"}}))).await;
    assert_eq!(st, 400, "{r}");

    // Saved as version 2; version 1 is still read as serp@1.
    let (st, j) = call(&base, "PUT", "/v1/blends/serp", root, Some(json!({"def": draft, "note": "comments first"}))).await;
    assert_eq!((st, j["version"].as_i64()), (200, Some(2)), "{j}");
    let (_, now) = call(&base, "POST", "/v1/query", root, Some(json!({"sql": "SELECT source FROM blend('serp', q => 'rust') LIMIT 1"}))).await;
    let (_, was) = call(&base, "POST", "/v1/query", root, Some(json!({"sql": "SELECT source FROM blend('serp@1', q => 'rust') LIMIT 1"}))).await;
    assert_eq!((col(&now, 0)[0].as_str(), col(&was, 0)[0].as_str()), ("comments", "stories"));
    let (_, j) = call(&base, "GET", "/v1/blends/serp", root, None).await;
    assert_eq!((j["current"].as_i64(), j["versions"][1]["note"].as_str()), (Some(2), Some("comments first")), "{j}");

    // A definition is an admin's; a read key previews and reads.
    let (st, made) = call(&base, "POST", "/v1/admin/keys", root, Some(json!({"name": "viewer", "scope": "read"}))).await;
    assert_eq!(st, 200, "{made}");
    let reader = made["secret"].as_str().unwrap().to_string();
    let read = Some(reader.as_str());
    assert_eq!(call(&base, "PUT", "/v1/blends/serp", read, Some(json!({"def": def}))).await.0, 403);
    assert_eq!(call(&base, "DELETE", "/v1/blends/serp", read, None).await.0, 403);
    let (st, j) = call(&base, "POST", "/v1/sql", read, Some(json!({"sql": SERP.replace("CREATE BLEND serp", "CREATE BLEND other")}))).await;
    assert_ne!(st, 200, "{j}");
    let (st, j) = call(&base, "POST", "/v1/blends/preview", read, Some(json!({"name": "serp", "params": {"q": "rust"}}))).await;
    assert_eq!(st, 200, "{j}");
    let (st, j) = call(&base, "POST", "/v1/query", read, Some(json!({"sql": q}))).await;
    assert_eq!(st, 200, "{j}");

    // A key limited to stories reads no blend of comments, by any route.
    let (st, made) = call(&base, "POST", "/v1/admin/keys", root, Some(json!({"name": "storyteller", "tables": {"stories": "read"}}))).await;
    assert_eq!(st, 200, "{made}");
    let limited = made["secret"].as_str().unwrap().to_string();
    let lim = Some(limited.as_str());
    let (st, j) = call(&base, "POST", "/v1/query", lim, Some(json!({"sql": "SELECT id FROM stories WHERE MATCH(title, 'rust') LIMIT 1"}))).await;
    assert_eq!(st, 200, "{j}");
    for (path, body) in [
        ("/v1/query", json!({"sql": q})),
        ("/v1/sql", json!({"sql": q})),
        ("/v1/blends/preview", json!({"name": "serp", "params": {"q": "rust"}})),
        ("/v1/blends/preview", json!({"def": def, "params": {"q": "rust"}})),
    ] {
        let (st, j) = call(&base, "POST", path, lim, Some(body.clone())).await;
        assert_eq!(st, 403, "{path} {body}: {j}");
    }

    // Dropped: the name reads nothing, its versions stay.
    let (st, j) = call(&base, "DELETE", "/v1/blends/serp", root, None).await;
    assert_eq!(st, 200, "{j}");
    let (st, j) = call(&base, "POST", "/v1/query", root, Some(json!({"sql": "SELECT id FROM blend('serp', q => 'rust')"}))).await;
    assert!(st >= 400 && j.to_string().contains("no blend named serp"), "{st} {j}");
    let (st, j) = call(&base, "POST", "/v1/query", root, Some(json!({"sql": "SELECT source FROM blend('serp@2', q => 'rust') LIMIT 1"}))).await;
    assert_eq!((st, col(&j, 0)[0].as_str()), (200, "comments"), "{j}");
    let (_, j) = call(&base, "GET", "/v1/blends", root, None).await;
    assert_eq!(j["blends"], json!([]), "{j}");
    let (_, j) = call(&base, "GET", "/v1/blends/serp", root, None).await;
    assert_eq!((j["current"].clone(), j["versions"][2]["dropped"].as_bool()), (Json::Null, Some(true)), "{j}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_coordinator_blends_as_one_database() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let urls = vec![serve_grpc(create(d, "s0.strata")).await, serve_grpc(create(d, "s1.strata")).await, serve_grpc(create(d, "s2.strata")).await];
    let c = Arc::new(Coordinator::connect(urls).await.unwrap());
    let schema = c.schema.clone();
    let svc = Arc::new(Service::new(Backend::Cluster(c), schema, agent(), 16, Duration::from_secs(10)));
    let one = create(d, "one.strata");
    for s in TABLES.iter().map(|s| s.to_string()).chain(inserts()).chain([SERP.to_string()]) {
        run(&svc, &s).await;
        one.sql(&s, &Params::new()).unwrap_or_else(|e| panic!("{s}: {e}"));
    }
    let p = Params::new().set("q", Value::Str("rust".into()));
    for q in [
        "SELECT slot, source, id, author, utility, blend_decision FROM blend('serp', q => 'rust')",
        "SELECT * FROM blend('serp') LIMIT 7",
        "SELECT id, author AS who FROM blend('serp@1', q => :q) LIMIT 12",
    ] {
        let want = one.query(q, &p).unwrap_or_else(|e| panic!("{q}: {e}"));
        assert!(!want.rows.is_empty(), "{q}");
        let got = svc.query(QueryReq { sql: q.into(), params: p.clone(), ..Default::default() }).await.unwrap_or_else(|e| panic!("{q}: {}", e.message)).rs;
        assert_eq!(got.columns, want.columns, "{q}");
        assert_eq!(got.rows, want.rows, "{q}");
    }
    // A new version through the coordinator is every shard's, and the
    // coordinator reads both.
    let v2 = SERP.replace("CREATE BLEND", "CREATE OR REPLACE BLEND").replace("SLOT 1 => source = 'stories'", "SLOT 1 => source = 'comments'");
    let SqlAnswer::Rows(a) = run(&svc, &v2).await else { panic!("rows") };
    assert_eq!(a.rs.rows[0][1], Value::Int(2));
    let q = |v: &str| QueryReq { sql: format!("SELECT source FROM blend('{v}', q => 'rust') LIMIT 1"), ..Default::default() };
    let now = svc.query(q("serp")).await.unwrap().rs.rows;
    let was = svc.query(q("serp@1")).await.unwrap().rs.rows;
    assert_eq!((now[0][0].clone(), was[0][0].clone()), (Value::Str("comments".into()), Value::Str("stories".into())));
    // DROP BLEND through the coordinator.
    run(&svc, "DROP BLEND serp").await;
    let e = svc.query(q("serp")).await.err().unwrap();
    assert!(e.message.contains("no blend named serp"), "{}", e.message);
}

/// A blend stacked under another as a node: its comments, one per author,
/// are a source of the blend above. Each query integer-ordered, so that a
/// coordinator merges the very rows one database reads.
const STACKED: &str = "CREATE BLEND stacked AS
  SOURCES (
    talk AS BLEND (
      SOURCES (
        comments AS (SELECT id, body AS title, author, votes AS relevance FROM comments WHERE MATCH(body, :q) ORDER BY votes DESC LIMIT 20)
      )
      HEADS (relevance CALIBRATE rank)
      MAX 1 PER author
      WITH (k => 5)
    ),
    stories AS (SELECT id, title, author, points AS relevance FROM stories WHERE MATCH(title, :q) ORDER BY points DESC LIMIT 20)
  )
  HEADS (relevance CALIBRATE rank)
  QUOTA (source = 'talk' AT LEAST 30% IN TOP 6)
  WITH (k => 8)";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn blends_stack_on_a_server_and_a_coordinator() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let urls = vec![serve_grpc(create(d, "s0.strata")).await, serve_grpc(create(d, "s1.strata")).await, serve_grpc(create(d, "s2.strata")).await];
    let c = Arc::new(Coordinator::connect(urls).await.unwrap());
    let schema = c.schema.clone();
    let coord = Arc::new(Service::new(Backend::Cluster(c), schema, agent(), 16, Duration::from_secs(10)));
    let db = create(d, "one.strata");
    let schema = (*db.schema()).clone();
    let mut one = Service::new(Backend::Local(db), schema, agent(), 16, Duration::from_secs(10));
    one.keys = Arc::new(Keys::open(d.join("one.strata.keys.json")).unwrap());
    let one = Arc::new(one);
    for s in TABLES.iter().map(|s| s.to_string()).chain(inserts()).chain([STACKED.to_string()]) {
        run(&coord, &s).await;
        run(&one, &s).await;
    }
    // The coordinator's slate is the one database's.
    let q = "SELECT slot, source, via, id, author FROM blend('stacked', q => 'rust')";
    let want = one.query(QueryReq { sql: q.into(), ..Default::default() }).await.unwrap().rs;
    let got = coord.query(QueryReq { sql: q.into(), ..Default::default() }).await.unwrap_or_else(|e| panic!("{}", e.message)).rs;
    assert_eq!(want.rows.len(), 8);
    assert_eq!((&got.columns, &got.rows), (&want.columns, &want.rows));
    let talk: Vec<&Vec<Value>> = want.rows.iter().filter(|r| r[1] == Value::Str("talk".into())).collect();
    assert!(talk.len() >= 2, "{:?}", want.rows);
    assert!(talk.iter().all(|r| r[2] == Value::Str("talk/comments".into())), "{:?}", want.rows);
    let mut authors: Vec<String> = talk.iter().map(|r| format!("{:?}", r[4])).collect();
    authors.sort();
    authors.dedup();
    assert_eq!(authors.len(), talk.len(), "the node's own MAX 1 PER author: {:?}", want.rows);

    // Over HTTP: a draft's preview has each node's receipts; a key limited
    // to stories may not preview a blend whose node reads comments.
    let base = serve_http(one.clone(), Access { token: Some(Secret::new("root-token")), ..Default::default() }).await;
    let root = Some("root-token");
    let (_, j) = call(&base, "GET", "/v1/blends/stacked", root, None).await;
    let def = j["versions"][0]["def"].clone();
    assert_eq!(def["sources"][0]["blend"]["k"], 5, "{j}");
    let (st, r) = call(&base, "POST", "/v1/blends/preview", root, Some(json!({"def": def, "params": {"q": "rust"}}))).await;
    assert_eq!(st, 200, "{r}");
    assert_eq!(r["nodes"]["talk"]["rows"].as_array().map(Vec::len), Some(5), "{r}");
    assert!(r["rows"].as_array().unwrap().iter().any(|x| x["via"] == "talk/comments"), "{r}");
    assert!(r["sql"].as_str().unwrap().contains("talk AS BLEND ("), "{r}");
    let (st, made) = call(&base, "POST", "/v1/admin/keys", root, Some(json!({"name": "storyteller", "tables": {"stories": "read"}}))).await;
    assert_eq!(st, 200, "{made}");
    let lim = made["secret"].as_str().map(str::to_string);
    for (path, body) in [("/v1/blends/preview", json!({"def": def, "params": {"q": "rust"}})), ("/v1/query", json!({"sql": q}))] {
        let (st, j) = call(&base, "POST", path, lim.as_deref(), Some(body)).await;
        assert_eq!(st, 403, "{path}: {j}");
    }
}
