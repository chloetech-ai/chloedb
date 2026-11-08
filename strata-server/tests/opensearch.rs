//! The OpenSearch-compatible API (`--opensearch-listen`): indexes,
//! `_bulk`, searches with the Query DSL and aggregations, scrolls,
//! documents, aliases and mappings, on one server and through a
//! coordinator over two shards, which answer the same; and its
//! credentials (a Bearer token or Basic authentication's password, and
//! the scope each request needs).

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value as Json};
use strata::db::Db;
use strata::storage::DbOptions;
use strata_server::auth::{Access, Secret};
use strata_server::coord::Coordinator;
use strata_server::keys::Keys;
use strata_server::service::{AgentConfig, Backend, Service};
use strata_server::{demo, server};
use tokio::net::TcpListener;

fn agent() -> AgentConfig {
    AgentConfig { api_key: None, model: strata_server::agent::DEFAULT_MODEL.into(), base_url: "http://unused".into() }
}

fn create(dir: &Path, name: &str) -> Arc<Db> {
    let db = Db::create(&dir.join(name), demo::schema(10), DbOptions { publish_snapshots: true, ..DbOptions::default() }).unwrap();
    db.set_embedder("emb", Arc::new(strata::embed::HashEmbedder { dim: demo::DIM })).unwrap();
    Arc::new(db)
}

fn local(dir: &Path) -> Service {
    let db = create(dir, "one.strata");
    let schema = (*db.schema()).clone();
    Service::new(Backend::Local(db), schema, agent(), 16, Duration::from_secs(10))
}

async fn shard(db: Arc<Db>) -> String {
    let schema = (*db.schema()).clone();
    let svc = Arc::new(Service::new(Backend::Local(db), schema, agent(), 16, Duration::from_secs(10)));
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(server::serve_grpc(svc, l, Access::default()));
    url
}

async fn cluster(dir: &Path) -> Service {
    let urls = vec![shard(create(dir, "s0.strata")).await, shard(create(dir, "s1.strata")).await];
    let c = Arc::new(Coordinator::connect(urls).await.unwrap());
    let schema = c.schema.clone();
    Service::new(Backend::Cluster(c), schema, agent(), 16, Duration::from_secs(10))
}

/// Serves `svc`'s OpenSearch API; its URL.
async fn serve(svc: Arc<Service>, access: Access) -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
    tokio::spawn(server::serve_opensearch(svc, l, access, strata_server::opensearch::VERSION_NUMBER));
    url
}

/// `method path` with `body` (JSON, or NDJSON text) and `auth` as the
/// Authorization header: the status and the answer.
async fn call(base: &str, method: &str, path: &str, body: Option<Json>, auth: Option<&str>) -> (u16, Json) {
    let c = strata_server::http_client().build().unwrap();
    let m = reqwest::Method::from_bytes(method.as_bytes()).unwrap();
    let mut r = c.request(m, format!("{base}{path}"));
    if let Some(a) = auth {
        r = r.header("authorization", a);
    }
    r = match body {
        Some(Json::String(nd)) => r.header("content-type", "application/x-ndjson").body(nd),
        Some(b) => r.json(&b),
        None => r,
    };
    let r = r.send().await.unwrap();
    let status = r.status().as_u16();
    let text = r.text().await.unwrap();
    (status, serde_json::from_str(&text).unwrap_or(Json::String(text)))
}

async fn ok(base: &str, method: &str, path: &str, body: Option<Json>) -> Json {
    let (s, j) = call(base, method, path, body, None).await;
    assert!(s == 200 || s == 201, "{method} {path}: {s} {j}");
    j
}

/// Document `i` of the test index.
fn book(i: usize) -> Json {
    let words = ["quick", "brown", "fox", "lazy", "dog", "search", "engine", "database"];
    let title = format!("{} {} {}", words[i % 8], words[(i / 8) % 8], words[(i * 3 + 1) % 8]);
    let (color, age, name) = (["red", "green", "blue"][i % 3], ["old", "new"][i % 2], ["ann", "bob", "cy", "dee"][i % 4]);
    let mut d = json!({
        "title": title,
        "tags": [color, age],
        "year": 1990 + (i % 30) as i64,
        "published": format!("{}-{:02}-15T00:00:00Z", 1990 + i % 30, 1 + i % 12),
        "author": {"name": name},
    });
    if i % 5 == 0 {
        d["award"] = json!(true);
    }
    d
}

const N: usize = 300;

/// A run of requests; what it found, to compare a cluster's with one
/// server's.
async fn scenario(base: &str) -> Vec<Json> {
    let mut seen = Vec::new();
    let info = ok(base, "GET", "/", None).await;
    assert_eq!(info["version"]["distribution"], "opensearch");
    ok(base, "PUT", "/books", Some(json!({"mappings": {"properties": {
        "title": {"type": "text", "fields": {"raw": {"type": "keyword"}}},
        "tags": {"type": "keyword"}, "year": {"type": "integer"}, "published": {"type": "date"},
        "author": {"properties": {"name": {"type": "keyword"}}}}}}))).await;
    let (s, j) = call(base, "PUT", "/books", Some(json!({})), None).await;
    assert_eq!((s, j["error"]["type"].as_str()), (400, Some("resource_already_exists_exception")));
    let mut nd = String::new();
    for i in 0..N {
        nd.push_str(&format!("{}\n{}\n", json!({"index": {"_index": "books", "_id": format!("b{i}")}}), book(i)));
    }
    let r = ok(base, "POST", "/_bulk?refresh=true", Some(Json::String(nd))).await;
    assert_eq!(r["errors"], false, "{r}");
    assert_eq!(r["items"].as_array().unwrap().len(), N);
    assert_eq!(ok(base, "GET", "/books/_count", None).await["count"], N);

    let total = |r: &Json| r["hits"]["total"]["value"].as_u64().unwrap();
    let ids = |r: &Json| r["hits"]["hits"].as_array().unwrap().iter().map(|h| h["_id"].as_str().unwrap().to_string()).collect::<BTreeSet<_>>();
    let count_of = |f: &dyn Fn(usize) -> bool| (0..N).filter(|&i| f(i)).count() as u64;
    let title = |i: usize| book(i)["title"].as_str().unwrap().to_string();

    // Full text, ranked: every title with the word.
    let r = ok(base, "POST", "/books/_search", Some(json!({"query": {"match": {"title": "fox"}}, "size": N}))).await;
    assert_eq!(total(&r), count_of(&|i| title(i).split(' ').any(|w| w == "fox")));
    let scores: Vec<f64> = r["hits"]["hits"].as_array().unwrap().iter().map(|h| h["_score"].as_f64().unwrap()).collect();
    assert!(scores.windows(2).all(|w| w[0] >= w[1]) && scores[0] > 0.0, "{scores:?}");
    seen.push(json!(ids(&r)));
    // A phrase being typed.
    let r = ok(base, "POST", "/books/_search", Some(json!({"query": {"match_phrase_prefix": {"title": "quick bro"}}, "size": N, "highlight": {"fields": {"title": {}}}}))).await;
    assert_eq!(total(&r), count_of(&|i| title(i).contains("quick bro")), "{r}");
    let hl = r["hits"]["hits"][0]["highlight"]["title"][0].as_str().unwrap();
    assert!(hl.contains("<em>quick</em> <em>brown</em>"), "{hl}");
    seen.push(json!(ids(&r)));
    // Filters and optional words: the rows without the words come after.
    let r = ok(base, "POST", "/books/_search", Some(json!({"query": {"bool": {"filter": [{"term": {"tags": "red"}}, {"range": {"year": {"gte": 2000}}}], "should": [{"match": {"title": "dog"}}]}}, "size": N}))).await;
    let want = count_of(&|i| i % 3 == 0 && 1990 + i % 30 >= 2000);
    assert_eq!((total(&r), r["hits"]["hits"].as_array().unwrap().len() as u64), (want, want));
    let first = &r["hits"]["hits"][0];
    assert!(first["_source"]["title"].as_str().unwrap().contains("dog") && first["_score"].as_f64().unwrap() > 0.0, "{first}");
    seen.push(json!(ids(&r)));
    // Sorted by a field, paged with search_after.
    let page = |after: Option<Json>| {
        let mut b = json!({"sort": [{"year": "desc"}, {"published": "asc"}], "size": 7, "query": {"term": {"author.name": "bob"}}});
        if let Some(a) = after {
            b["search_after"] = a;
        }
        b
    };
    let r = ok(base, "POST", "/books/_search", Some(page(None))).await;
    let years: Vec<i64> = r["hits"]["hits"].as_array().unwrap().iter().map(|h| h["sort"][0].as_i64().unwrap()).collect();
    assert!(years.windows(2).all(|w| w[0] >= w[1]) && years[0] == 2019, "{years:?}");
    let last = r["hits"]["hits"][6]["sort"].clone();
    let r2 = ok(base, "POST", "/books/_search", Some(page(Some(last)))).await;
    assert!(ids(&r).is_disjoint(&ids(&r2)) && total(&r2) == total(&r));
    // Aggregations.
    let r = ok(base, "POST", "/books/_search", Some(json!({"size": 0, "aggs": {
        "tags": {"terms": {"field": "tags", "size": 2}, "aggs": {"y": {"avg": {"field": "year"}}}},
        "decades": {"histogram": {"field": "year", "interval": 10}},
        "years": {"date_histogram": {"field": "published", "calendar_interval": "year"}},
        "names": {"composite": {"size": 3, "sources": [{"n": {"terms": {"field": "author.name"}}}]}},
        "awarded": {"filter": {"exists": {"field": "award"}}}}}))).await;
    let a = &r["aggregations"];
    assert_eq!(a["tags"]["buckets"].as_array().unwrap().len(), 2);
    assert_eq!(a["tags"]["sum_other_doc_count"].as_u64().unwrap() + a["tags"]["buckets"].as_array().unwrap().iter().map(|b| b["doc_count"].as_u64().unwrap()).sum::<u64>(), 2 * N as u64);
    assert_eq!(a["decades"]["buckets"].as_array().unwrap().len(), 3);
    assert_eq!(a["years"]["buckets"].as_array().unwrap().len(), 30);
    assert_eq!(a["names"]["after_key"], json!({"n": "cy"}));
    assert_eq!(a["awarded"]["doc_count"], count_of(&|i| i % 5 == 0));
    seen.push(a.clone());
    let r = ok(base, "POST", "/books/_search", Some(json!({"size": 0, "aggs": {"names": {"composite": {"size": 3, "after": {"n": "cy"}, "sources": [{"n": {"terms": {"field": "author.name"}}}]}}}}))).await;
    assert_eq!(r["aggregations"]["names"]["buckets"], json!([{"key": {"n": "dee"}, "doc_count": N / 4}]));
    // A scroll through every document, past pages of 70.
    let mut r = ok(base, "POST", "/books/_search?scroll=1m", Some(json!({"size": 70, "sort": ["_doc"], "_source": false}))).await;
    let mut all = BTreeSet::new();
    loop {
        let got = ids(&r);
        if got.is_empty() {
            break;
        }
        assert!(all.is_disjoint(&got));
        all.extend(got);
        let id = r["_scroll_id"].as_str().unwrap().to_string();
        r = ok(base, "POST", "/_search/scroll", Some(json!({"scroll": "1m", "scroll_id": id}))).await;
    }
    assert_eq!(all.len(), N);
    // Documents.
    let d = ok(base, "GET", "/books/_doc/b7", None).await;
    assert_eq!((d["found"].clone(), d["_version"].clone(), d["_source"].clone()), (json!(true), json!(1), book(7)));
    let (s, _) = call(base, "GET", "/books/_doc/nope", None, None).await;
    assert_eq!(s, 404);
    let u = ok(base, "POST", "/books/_update/b7?refresh=true", Some(json!({"doc": {"year": 2077, "extra": {"pages": 12}}}))).await;
    assert_eq!((u["result"].as_str(), u["_version"].as_i64()), (Some("updated"), Some(2)));
    let d = ok(base, "GET", "/books/_source/b7", None).await;
    assert_eq!((d["year"].clone(), d["extra"]["pages"].clone(), d["title"].clone()), (json!(2077), json!(12), json!(title(7))));
    let m = ok(base, "GET", "/books/_mapping", None).await;
    assert_eq!(m["books"]["mappings"]["properties"]["extra"]["properties"]["pages"]["type"], "long");
    let (s, j) = call(base, "PUT", "/books/_create/b7", Some(json!({"title": "x"})), None).await;
    assert_eq!((s, j["error"]["type"].as_str()), (409, Some("version_conflict_engine_exception")));
    ok(base, "DELETE", "/books/_doc/b8?refresh=true", None).await;
    let r = ok(base, "POST", "/books/_mget", Some(json!({"ids": ["b7", "b8"]}))).await;
    assert_eq!((r["docs"][0]["found"].clone(), r["docs"][1]["found"].clone()), (json!(true), json!(false)));
    let r = ok(base, "POST", "/books/_delete_by_query?refresh=true", Some(json!({"query": {"term": {"tags": "blue"}}}))).await;
    let blue = count_of(&|i| i % 3 == 2 && i != 8);
    assert_eq!(r["deleted"], blue);
    assert_eq!(ok(base, "GET", "/books/_count", None).await["count"], N as u64 - 1 - blue);
    // Aliases.
    ok(base, "POST", "/_aliases", Some(json!({"actions": [{"add": {"index": "books", "alias": "library"}}]}))).await;
    assert_eq!(ok(base, "GET", "/library/_count", None).await["count"], N as u64 - 1 - blue);
    assert_eq!(ok(base, "GET", "/_alias/library", None).await, json!({"books": {"aliases": {"library": {}}}}));
    // An index made by its first write, its mapping by its documents.
    let r = ok(base, "POST", "/logs/_doc?refresh=true", Some(json!({"msg": "disk full", "level": "warn", "at": "2026-11-08T10:00:00Z", "n": 3}))).await;
    assert_eq!(r["result"], "created");
    let m = ok(base, "GET", "/logs/_mapping", None).await;
    assert_eq!(m["logs"]["mappings"]["properties"]["msg"], json!({"type": "text", "fields": {"keyword": {"type": "keyword", "ignore_above": 256}}}));
    assert_eq!(m["logs"]["mappings"]["properties"]["at"]["type"], "date");
    let r = ok(base, "POST", "/logs/_search", Some(json!({"query": {"bool": {"must": {"match": {"msg": "disk"}}, "filter": {"term": {"msg.keyword": "disk full"}}}}}))).await;
    assert_eq!(total(&r), 1);
    let (s, j) = call(base, "GET", "/nope/_search", None, None).await;
    assert_eq!((s, j["error"]["type"].as_str()), (404, Some("index_not_found_exception")));
    let (s, j) = call(base, "POST", "/books/_search", Some(json!({"query": {"nosuch": {}}})), None).await;
    assert_eq!((s, j["error"]["type"].as_str()), (400, Some("parsing_exception")));
    ok(base, "DELETE", "/logs", None).await;
    let (s, _) = call(base, "HEAD", "/logs", None, None).await;
    assert_eq!(s, 404);
    seen
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_server_and_a_cluster_answer_opensearch_requests_alike() {
    let dir = tempfile::tempdir().unwrap();
    let one = serve(Arc::new(local(dir.path())), Access::default()).await;
    let many = serve(Arc::new(cluster(dir.path()).await), Access::default()).await;
    let a = scenario(&one).await;
    let b = scenario(&many).await;
    assert_eq!(a, b);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn requests_need_a_credential_of_their_scope() {
    let dir = tempfile::tempdir().unwrap();
    let mut svc = local(dir.path());
    svc.keys = Arc::new(Keys::open(dir.path().join("keys.json")).unwrap());
    let svc = Arc::new(svc);
    let reader = svc.keys.create("dashboards", strata_server::keys::Scope::Read, None, true, false).unwrap().1;
    let writer = svc.keys.create("ingest", strata_server::keys::Scope::Write, None, true, false).unwrap().1;
    let base = serve(svc, Access { token: Some(Secret::new("root-token")), ..Default::default() }).await;
    let basic = |user: &str, pass: &str| {
        use base64::Engine;
        format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}")))
    };
    let (s, j) = call(&base, "GET", "/", None, None).await;
    assert_eq!((s, j["error"]["type"].as_str()), (401, Some("security_exception")));
    assert_eq!(call(&base, "GET", "/", None, Some(&basic("admin", "wrong"))).await.0, 401);
    // The token, as Basic authentication's password (any user name).
    let root = basic("admin", "root-token");
    assert_eq!(call(&base, "PUT", "/docs", Some(json!({"mappings": {"properties": {"body": {"type": "text"}}}})), Some(&root)).await.0, 200);
    // A write key writes documents, and may not make an index.
    let w = basic("ingest", &writer);
    let (s, j) = call(&base, "POST", "/_bulk?refresh=true", Some(Json::String(format!("{}\n{}\n", json!({"index": {"_index": "docs", "_id": "1"}}), json!({"body": "hello world"})))), Some(&w)).await;
    assert_eq!((s, j["errors"].as_bool()), (200, Some(false)), "{j}");
    assert_eq!(call(&base, "PUT", "/other", Some(json!({})), Some(&w)).await.0, 403);
    let (s, j) = call(&base, "POST", "/_bulk", Some(Json::String(format!("{}\n{}\n", json!({"index": {"_index": "other"}}), json!({"a": 1})))), Some(&w)).await;
    assert_eq!((s, j["items"][0]["index"]["status"].as_u64()), (200, Some(403)), "{j}");
    // A read key searches, and may not write.
    let r = format!("Bearer {reader}");
    let (s, j) = call(&base, "POST", "/docs/_search", Some(json!({"query": {"match": {"body": "hello"}}})), Some(&r)).await;
    assert_eq!((s, j["hits"]["total"]["value"].as_u64()), (200, Some(1)));
    assert_eq!(call(&base, "PUT", "/docs/_doc/2", Some(json!({"body": "x"})), Some(&r)).await.0, 403);
    assert_eq!(call(&base, "DELETE", "/docs", None, Some(&w)).await.0, 403);
    assert_eq!(call(&base, "DELETE", "/docs", None, Some(&root)).await.0, 200);
}
