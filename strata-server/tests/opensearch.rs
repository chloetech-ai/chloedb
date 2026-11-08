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
    // Numbers: from their columns while no document holds an array of
    // one, from `_source` once one does (each value counts).
    ok(base, "PUT", "/nums", Some(json!({"mappings": {"properties": {"n": {"type": "long"}, "x": {"type": "double"}, "k": {"type": "keyword"}}}}))).await;
    for (i, d) in [json!({"n": 2, "x": 1.5, "k": "a"}), json!({"n": 4, "x": 2.5, "k": "b"}), json!({"n": 13, "k": "a"})].into_iter().enumerate() {
        ok(base, "PUT", &format!("/nums/_doc/{i}?refresh=true"), Some(d)).await;
    }
    let num_aggs = json!({"size": 0, "aggs": {"s": {"stats": {"field": "n"}}, "x": {"avg": {"field": "x"}}, "h": {"histogram": {"field": "n", "interval": 10}, "aggs": {"x": {"sum": {"field": "x"}}}}, "k": {"terms": {"field": "k"}, "aggs": {"n": {"max": {"field": "n"}}}}}});
    let a = ok(base, "POST", "/nums/_search", Some(num_aggs.clone())).await["aggregations"].clone();
    assert_eq!(a["s"], json!({"count": 3, "min": 2.0, "max": 13.0, "avg": 19.0 / 3.0, "sum": 19.0}));
    assert_eq!(a["x"]["value"], json!(2.0));
    assert_eq!(a["h"]["buckets"], json!([{"key": 0.0, "doc_count": 2, "x": {"value": 4.0}}, {"key": 10.0, "doc_count": 1, "x": {"value": 0.0}}]));
    assert_eq!(a["k"]["buckets"][0], json!({"key": "a", "doc_count": 2, "n": {"value": 13.0}}));
    seen.push(a);
    ok(base, "PUT", "/nums/_doc/3?refresh=true", Some(json!({"n": [1, 9], "x": [0.5, 7.5], "k": "b"}))).await;
    let (status, e) = call(base, "POST", "/nums/_search", Some(num_aggs.clone()), None).await;
    assert_eq!((status, e["error"]["type"].as_str()), (400, Some("illegal_argument_exception")), "{e}");
    let mut num_aggs = num_aggs;
    num_aggs["aggs"].as_object_mut().unwrap().remove("h");
    let a = ok(base, "POST", "/nums/_search", Some(num_aggs)).await["aggregations"].clone();
    assert_eq!(a["s"], json!({"count": 5, "min": 1.0, "max": 13.0, "avg": 29.0 / 5.0, "sum": 29.0}));
    assert_eq!(a["x"]["value"], json!(3.0));
    assert_eq!(a["k"]["buckets"][1], json!({"key": "b", "doc_count": 2, "n": {"value": 9.0}}));
    seen.push(a);
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

/// What Mastodon's searches need: n-gram fields (`edge_ngram`), words as
/// the standard tokenizer and `word_delimiter_graph` cut them, emoji,
/// `script_score` and `field_value_factor` over numbers, `gauss` over
/// dates, constant `should` boosts, and a search of two indexes with
/// `_index` filters and `collapse`. Returns what each search found.
async fn social(base: &str) -> Vec<Json> {
    let mut seen = Vec::new();
    let ids = |r: &Json| -> Vec<String> { r["hits"]["hits"].as_array().unwrap().iter().map(|h| h["_id"].as_str().unwrap().to_string()).collect() };
    let total = |r: &Json| r["hits"]["total"]["value"].as_u64().unwrap();
    let text = |analyzer: &str| json!({"type": "text", "analyzer": analyzer, "fields": {"edge_ngram": {"type": "text", "analyzer": "edge_ngram", "search_analyzer": analyzer}}});
    ok(base, "PUT", "/accounts", Some(json!({
        "settings": {"analysis": {
            "analyzer": {"verbatim": {"tokenizer": "standard", "filter": ["lowercase", "asciifolding"]}, "edge_ngram": {"tokenizer": "edge_ngram", "filter": ["lowercase", "asciifolding"]}},
            "tokenizer": {"edge_ngram": {"type": "edge_ngram", "min_gram": 1, "max_gram": 15}}}},
        "mappings": {"properties": {"id": {"type": "long"}, "followers_count": {"type": "long"}, "username": text("verbatim"), "display_name": text("verbatim")}}
    }))).await;
    let accounts = [(1, "gargron", "Eugen Rochko", 1000), (2, "garden_fan@mastodon.social", "Gardener 🌱", 10), (3, "alice", "Alice", 5), (4, "gary", "Gary 🌱", 0), (5, "rochko_fan", "Fan", 100)];
    let mut nd = String::new();
    for (id, u, d, f) in accounts {
        nd.push_str(&format!("{}\n{}\n", json!({"index": {"_id": id.to_string()}}), json!({"id": id, "username": u, "display_name": d, "followers_count": f})));
    }
    assert_eq!(ok(base, "POST", "/accounts/_bulk?refresh=true", Some(Json::String(nd))).await["errors"], json!(false));
    let autocomplete = |q: &str, boost: Option<i64>| {
        let core = json!({"dis_max": {"queries": [
            {"multi_match": {"query": q, "type": "most_fields", "fields": ["username", "username.*"]}},
            {"multi_match": {"query": q, "type": "most_fields", "fields": ["display_name", "display_name.*"]}}]}});
        let should: Vec<Json> = boost.map(|id| json!({"terms": {"id": [id], "boost": 100}})).into_iter().collect();
        json!({"query": {"bool": {"must": {"function_score": {"query": {"bool": {"must": [core]}}, "functions": [{"script_score": {"script": {"source": "Math.log10((Math.max(doc['followers_count'].value, 0) + 1))"}}}]}}, "should": should}}})
    };
    // `gar` starts gargron, garden_fan and gary; more followers rank
    // higher, and gary's none score 0.
    let r = ok(base, "POST", "/accounts/_search", Some(autocomplete("gar", None))).await;
    assert_eq!(ids(&r), ["1", "2", "4"]);
    assert_eq!(r["hits"]["hits"][2]["_score"], json!(0.0));
    // An account followed comes first.
    let r = ok(base, "POST", "/accounts/_search", Some(autocomplete("gar", Some(2)))).await;
    assert_eq!(ids(&r), ["2", "1", "4"]);
    assert!(r["hits"]["hits"][0]["_score"].as_f64().unwrap() > 100.0);
    seen.push(r);
    // The standard tokenizer keeps `mastodon.social` and `rochko_fan`
    // whole, and makes an emoji a word.
    let m = |f: &str, q: &str| json!({"query": {"match": {f: q}}});
    for (f, q, want) in [("username", "mastodon.social", 1), ("username", "mastodon", 0), ("username", "rochko", 0), ("username", "rochko_fan", 1), ("display_name", "rochko", 1), ("display_name", "🌱", 2)] {
        let r = ok(base, "POST", "/accounts/_search", Some(m(f, q))).await;
        assert_eq!(total(&r), want, "{f}: {q}");
        seen.push(json!(ids(&r)));
    }
    // Hashtags: words cut where case and digits change, and prefixes.
    ok(base, "PUT", "/tags", Some(json!({
        "settings": {"analysis": {
            "analyzer": {"content": {"tokenizer": "keyword", "filter": ["word_delimiter_graph", "lowercase", "asciifolding"]}, "edge_ngram": {"tokenizer": "edge_ngram", "filter": ["lowercase", "asciifolding"]}},
            "tokenizer": {"edge_ngram": {"type": "edge_ngram", "min_gram": 2, "max_gram": 15}}}},
        "mappings": {"properties": {"name": text("content"), "usage": {"type": "long"}, "last_status_at": {"type": "date"}}}
    }))).await;
    let tags = [("t1", "CatsOfMastodon", 50, "2026-11-07"), ("t2", "cats", 5, "2026-11-08"), ("t3", "Caturday", 500, "2026-09-01"), ("t4", "dogs2026", 1, "2026-11-08")];
    let mut nd = String::new();
    for (id, n, u, at) in tags {
        nd.push_str(&format!("{}\n{}\n", json!({"index": {"_id": id}}), json!({"name": n, "usage": u, "last_status_at": at})));
    }
    ok(base, "POST", "/tags/_bulk?refresh=true", Some(Json::String(nd))).await;
    let r = ok(base, "POST", "/tags/_search", Some(json!({"query": {"match": {"name": "cats"}}}))).await;
    assert_eq!(ids(&r).into_iter().collect::<BTreeSet<_>>(), ["t1", "t2"].map(String::from).into());
    let r = ok(base, "POST", "/tags/_search", Some(json!({"query": {"match": {"name": "2026"}}}))).await;
    assert_eq!(ids(&r), ["t4"]);
    let tag_search = json!({"query": {"function_score": {
        "query": {"multi_match": {"query": "cat", "fields": ["name.edge_ngram", "name"], "type": "most_fields", "operator": "and"}},
        "functions": [{"field_value_factor": {"field": "usage", "modifier": "log2p", "missing": 0}}, {"gauss": {"last_status_at": {"origin": "2026-11-08", "scale": "7d", "offset": "14d", "decay": 0.5}}}],
        "boost_mode": "multiply"}}});
    // Caturday is used most, but two months ago; CatsOfMastodon this week.
    let r = ok(base, "POST", "/tags/_search", Some(tag_search)).await;
    assert_eq!(ids(&r), ["t1", "t2", "t3"]);
    seen.push(r);
    // Statuses in two indexes, a status in both once.
    for (ix, private) in [("pub_statuses", false), ("statuses", true)] {
        let mut props = json!({"id": {"type": "long"}, "text": {"type": "text"}, "created_at": {"type": "date"}});
        if private {
            props["searchable_by"] = json!({"type": "long"});
        }
        ok(base, "PUT", &format!("/{ix}"), Some(json!({"mappings": {"properties": props}}))).await;
    }
    let mut pub_nd = String::new();
    let mut priv_nd = String::new();
    for i in 1..=30i64 {
        let doc = json!({"id": 1000 + i, "text": format!("coffee number {i}"), "created_at": "2026-11-01"});
        if i % 2 == 0 {
            pub_nd.push_str(&format!("{}\n{}\n", json!({"index": {"_id": (1000 + i).to_string()}}), doc));
        }
        if i % 3 == 0 || i % 2 == 1 {
            let mut d = doc.clone();
            d["searchable_by"] = json!(if i % 5 == 0 { vec![8] } else { vec![7, 9] });
            priv_nd.push_str(&format!("{}\n{}\n", json!({"index": {"_id": (1000 + i).to_string()}}), d));
        }
    }
    ok(base, "POST", "/pub_statuses/_bulk?refresh=true", Some(Json::String(pub_nd))).await;
    ok(base, "POST", "/statuses/_bulk?refresh=true", Some(Json::String(priv_nd))).await;
    let search = |from: usize| json!({
        "query": {"bool": {"must": [{"match": {"text": "coffee"}}], "filter": [{"bool": {"should": [{"term": {"_index": "pub_statuses"}}, {"bool": {"must": [{"term": {"_index": "statuses"}}, {"term": {"searchable_by": 7}}]}}], "minimum_should_match": 1}}]}},
        "collapse": {"field": "id"}, "sort": [{"id": {"order": "desc"}}], "size": 10, "from": from});
    // Public: the even ones; account 7's: the odd ones and the multiples of
    // 3 but those of 5.
    let want: Vec<String> = (1..=30i64).rev().filter(|i| i % 2 == 0 || ((i % 3 == 0 || i % 2 == 1) && i % 5 != 0)).map(|i| (1000 + i).to_string()).collect();
    let mut got = Vec::new();
    for from in [0, 10, 20] {
        let r = ok(base, "POST", "/pub_statuses,statuses/_search", Some(search(from))).await;
        got.extend(ids(&r));
        seen.push(r);
    }
    assert_eq!(got, want);
    let (s, _) = call(base, "POST", "/pub_statuses,statuses/_search", Some(json!({"aggs": {"n": {"max": {"field": "id"}}}})), None).await;
    assert_eq!(s, 400);
    for ix in ["accounts", "tags", "pub_statuses", "statuses"] {
        ok(base, "DELETE", &format!("/{ix}"), None).await;
    }
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
    let a = social(&one).await;
    let b = social(&many).await;
    assert_eq!(strip_took(a), strip_took(b));
}

/// Answers without their `took`.
fn strip_took(v: Vec<Json>) -> Vec<Json> {
    v.into_iter().map(|mut j| {
        if let Some(m) = j.as_object_mut() {
            m.remove("took");
        }
        j
    }).collect()
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
