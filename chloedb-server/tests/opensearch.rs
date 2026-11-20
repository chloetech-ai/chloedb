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
use chloedb::db::Db;
use chloedb::storage::DbOptions;
use chloedb_server::auth::{Access, Secret};
use chloedb_server::coord::Coordinator;
use chloedb_server::keys::Keys;
use chloedb_server::service::{AgentConfig, Backend, Service};
use chloedb_server::{demo, server};
use tokio::net::TcpListener;

fn agent() -> AgentConfig {
    AgentConfig { api_key: None, model: chloedb_server::agent::DEFAULT_MODEL.into(), base_url: "http://unused".into() }
}

fn create(dir: &Path, name: &str) -> Arc<Db> {
    let db = Db::create(&dir.join(name), demo::schema(10), DbOptions { publish_snapshots: true, ..DbOptions::default() }).unwrap();
    db.set_embedder("emb", Arc::new(chloedb::embed::HashEmbedder { dim: demo::DIM })).unwrap();
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
    tokio::spawn(server::serve_opensearch(svc, l, access, chloedb_server::opensearch::VERSION_NUMBER));
    url
}

/// `method path` with `body` (JSON, or NDJSON text) and `auth` as the
/// Authorization header: the status and the answer.
async fn call(base: &str, method: &str, path: &str, body: Option<Json>, auth: Option<&str>) -> (u16, Json) {
    let c = chloedb_server::http_client().build().unwrap();
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
    // A fixed_interval is date_bin's buckets, counted from the offset: the
    // same buckets and metrics as counted here, empty ones between.
    let r = ok(base, "POST", "/books/_search", Some(json!({"size": 0, "aggs": {
        "every": {"date_histogram": {"field": "published", "fixed_interval": "1000d", "offset": "+12h"}, "aggs": {"y": {"max": {"field": "year"}}}},
        "busy": {"date_histogram": {"field": "published", "fixed_interval": "90d", "min_doc_count": 1}}}}))).await;
    let day = 86_400_000i64;
    let ms = |i: usize| chloedb::timestamp::parse(book(i)["published"].as_str().unwrap()).unwrap() / 1000;
    let key = |t: i64, w: i64, off: i64| (t - off).div_euclid(w) * w + off;
    let mut want = std::collections::BTreeMap::<i64, (u64, i64)>::new();
    for i in 0..N {
        let e = want.entry(key(ms(i), 1000 * day, day / 2)).or_insert((0, i64::MIN));
        e.0 += 1;
        e.1 = e.1.max(1990 + (i % 30) as i64);
    }
    let every = r["aggregations"]["every"]["buckets"].as_array().unwrap();
    let (lo, hi) = (*want.keys().next().unwrap(), *want.keys().last().unwrap());
    assert_eq!(every.len() as i64, (hi - lo) / (1000 * day) + 1, "{every:?}");
    for b in every {
        let k = b["key"].as_i64().unwrap();
        let (n, y) = want.get(&k).copied().unwrap_or((0, 0));
        assert_eq!(b["doc_count"].as_u64(), Some(n), "{b}");
        if n > 0 {
            assert_eq!(b["y"]["value"].as_f64(), Some(y as f64), "{b}");
        }
    }
    let mut busy = std::collections::BTreeMap::<i64, u64>::new();
    for i in 0..N {
        *busy.entry(key(ms(i), 90 * day, 0)).or_default() += 1;
    }
    let got: Vec<(i64, u64)> = r["aggregations"]["busy"]["buckets"].as_array().unwrap().iter().map(|b| (b["key"].as_i64().unwrap(), b["doc_count"].as_u64().unwrap())).collect();
    assert_eq!(got, busy.into_iter().collect::<Vec<_>>());
    seen.push(r["aggregations"].clone());
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

/// Document `i` of the index of [`negations`]: each field missing from
/// some documents (and `tags` an empty list in others, `meta` an object
/// without `color`).
fn item(i: usize) -> Json {
    let mut d = json!({"title": format!("item {i}")});
    if i % 4 != 0 {
        d["brand"] = json!(["acme", "zeta", "omni"].get(i % 3));
    }
    if i % 5 != 0 {
        d["price"] = json!((i * 7 % 40) as f64 + 0.5);
    }
    match i % 6 {
        0 => {}
        1 => d["tags"] = json!([]),
        _ => d["tags"] = [json!(["x"]), json!(["x", "y"]), json!(["y"])][i % 3].clone(),
    }
    if i % 7 != 0 {
        d["seen"] = json!(format!("2026-0{}-01T00:00:00Z", 1 + i % 3));
    }
    match i % 3 {
        0 => {}
        1 => d["meta"] = json!({}),
        _ => d["meta"] = json!({"color": if i % 2 == 0 { "red" } else { "blue" }}),
    }
    d
}

const ITEMS: usize = 120;

/// `must_not`, a query string's `NOT` and `-`, and a `filters`
/// aggregation's other bucket keep the documents without the field, as
/// OpenSearch's do (a document matches a query or does not): each against
/// the documents counted here.
async fn negations(base: &str) {
    ok(base, "PUT", "/items", Some(json!({"mappings": {"properties": {
        "title": {"type": "text"}, "brand": {"type": "keyword"}, "price": {"type": "double"}, "tags": {"type": "keyword"},
        "seen": {"type": "date"}, "meta": {"properties": {"color": {"type": "keyword"}}}}}}))).await;
    let mut nd = String::new();
    for i in 0..ITEMS {
        nd.push_str(&format!("{}\n{}\n", json!({"index": {"_id": format!("i{i}")}}), item(i)));
    }
    assert_eq!(ok(base, "POST", "/items/_bulk?refresh=true", Some(Json::String(nd))).await["errors"], json!(false));

    let brand = |i: usize, b: &str| item(i)["brand"] == json!(b);
    let price = |i: usize| item(i)["price"].as_f64();
    let tag = |i: usize, t: &str| item(i)["tags"].as_array().is_some_and(|ts| ts.contains(&json!(t)));
    let seen = |i: usize, d: &str| item(i)["seen"] == json!(d);
    let color = |i: usize, c: &str| item(i)["meta"]["color"] == json!(c);
    let check = |what: &str, r: &Json, want: &dyn Fn(usize) -> bool| {
        let got: BTreeSet<String> = r["hits"]["hits"].as_array().unwrap().iter().map(|h| h["_id"].as_str().unwrap().to_string()).collect();
        let want: BTreeSet<String> = (0..ITEMS).filter(|&i| want(i)).map(|i| format!("i{i}")).collect();
        assert_eq!(r["hits"]["total"]["value"].as_u64(), Some(want.len() as u64), "{what}: total");
        assert_eq!(got, want, "{what}");
    };
    let search = |q: Json| async move { ok(base, "POST", "/items/_search", Some(json!({"query": q, "size": ITEMS, "track_total_hits": true}))).await };
    let not = |q: Json| json!({"bool": {"must_not": q}});

    // A condition on one field.
    let cases: Vec<(&str, Json, Box<dyn Fn(usize) -> bool>)> = vec![
        ("term", json!({"term": {"brand": "zeta"}}), Box::new(move |i| !brand(i, "zeta"))),
        ("terms", json!({"terms": {"brand": ["acme", "omni"]}}), Box::new(move |i| !brand(i, "acme") && !brand(i, "omni"))),
        ("range", json!({"range": {"price": {"lt": 10}}}), Box::new(move |i| !price(i).is_some_and(|p| p < 10.0))),
        ("two bounds", json!({"range": {"price": {"gte": 5, "lte": 30}}}), Box::new(move |i| !price(i).is_some_and(|p| (5.0..=30.0).contains(&p)))),
        ("a list", json!({"term": {"tags": "x"}}), Box::new(move |i| !tag(i, "x"))),
        ("prefix", json!({"prefix": {"brand": "ze"}}), Box::new(move |i| !brand(i, "zeta"))),
        ("date", json!({"term": {"seen": "2026-02-01T00:00:00Z"}}), Box::new(move |i| !seen(i, "2026-02-01T00:00:00Z"))),
        ("an object's field", json!({"term": {"meta.color": "red"}}), Box::new(move |i| !color(i, "red"))),
    ];
    for (what, q, want) in &cases {
        check(&format!("must_not {what}"), &search(not(q.clone())).await, want);
    }
    // Conditions on several fields, and a negation inside a negation.
    check("must_not of a should", &search(not(json!({"bool": {"should": [{"term": {"brand": "zeta"}}, {"range": {"price": {"gte": 30}}}]}}))).await, &|i| {
        !(brand(i, "zeta") || price(i).is_some_and(|p| p >= 30.0))
    });
    check("must_not of a must", &search(not(json!({"bool": {"must": [{"term": {"brand": "acme"}}, {"term": {"tags": "y"}}]}}))).await, &|i| !(brand(i, "acme") && tag(i, "y")));
    check("must_not of a must_not", &search(not(not(json!({"term": {"brand": "zeta"}})))).await, &|i| brand(i, "zeta"));
    check(
        "filter and must_not",
        &search(json!({"bool": {"filter": {"range": {"price": {"lt": 20}}}, "must_not": [{"term": {"tags": "y"}}, {"term": {"meta.color": "blue"}}]}})).await,
        &|i| price(i).is_some_and(|p| p < 20.0) && !tag(i, "y") && !color(i, "blue"),
    );
    // The positive conditions are as they were.
    check("term", &search(json!({"term": {"brand": "zeta"}})).await, &|i| brand(i, "zeta"));
    check("range", &search(json!({"range": {"price": {"lt": 10}}})).await, &|i| price(i).is_some_and(|p| p < 10.0));
    // A query string's NOT and -.
    for q in ["NOT brand:acme", "-brand:acme"] {
        check(q, &search(json!({"query_string": {"query": q, "default_field": "title"}})).await, &|i| !brand(i, "acme"));
    }
    check("item -brand:acme", &search(json!({"query_string": {"query": "item -brand:acme", "default_field": "title"}})).await, &|i| !brand(i, "acme"));
    // _count.
    let n = ok(base, "POST", "/items/_count", Some(json!({"query": not(json!({"term": {"brand": "zeta"}}))}))).await;
    assert_eq!(n["count"].as_u64(), Some((0..ITEMS).filter(|&i| !brand(i, "zeta")).count() as u64), "_count of a must_not");
    // A filters aggregation's other bucket: the documents neither filter
    // matches, those without a brand among them.
    let r = ok(base, "POST", "/items/_search", Some(json!({"size": 0, "aggs": {"by": {"filters": {
        "filters": {"acme": {"term": {"brand": "acme"}}, "cheap": {"range": {"price": {"lt": 10}}}}, "other_bucket_key": "rest"}}}}))).await;
    let buckets = &r["aggregations"]["by"]["buckets"];
    let count = |f: &dyn Fn(usize) -> bool| json!((0..ITEMS).filter(|&i| f(i)).count());
    assert_eq!(buckets["acme"]["doc_count"], count(&|i| brand(i, "acme")), "{buckets}");
    assert_eq!(buckets["cheap"]["doc_count"], count(&|i| price(i).is_some_and(|p| p < 10.0)), "{buckets}");
    assert_eq!(buckets["rest"]["doc_count"], count(&|i| !brand(i, "acme") && !price(i).is_some_and(|p| p < 10.0)), "{buckets}");
    ok(base, "DELETE", "/items", None).await;
}

/// A ticket of the shared index, of tenant `i % 3`.
fn ticket(i: usize) -> Json {
    let words = ["vpn", "password", "invoice", "printer", "login"];
    json!({"tenant_id": TENANTS[i % 3], "body": format!("{} {} issue", words[i % 5], words[(i / 5) % 5]), "n": i})
}

const TENANTS: [&str; 3] = ["acme", "globex", "initech"];
const TICKETS: usize = 60;

/// Filtered aliases, one per tenant, over one shared index (finding
/// KENJI-301): every read through one sees its tenant's documents alone,
/// a search, a count, an aggregation (`global` too), a get and `_mget`
/// by id, `_msearch`, `_cat/count` and a delete by query; two of them, or
/// one and the index, see what OpenSearch would. What `_alias` shows,
/// filters ChloeDB cannot apply refused when the alias is made, and an
/// index never deleted through an alias.
async fn tenant_aliases(base: &str) {
    ok(base, "PUT", "/tickets", Some(json!({"mappings": {"properties": {"tenant_id": {"type": "keyword"}, "body": {"type": "text"}, "n": {"type": "long"}}}}))).await;
    let mut nd = String::new();
    for i in 0..TICKETS {
        nd.push_str(&format!("{}\n{}\n", json!({"index": {"_id": format!("t{i}")}}), ticket(i)));
    }
    assert_eq!(ok(base, "POST", "/tickets/_bulk?refresh=true", Some(Json::String(nd))).await["errors"], json!(false));
    let of = |t: usize| (0..TICKETS).filter(|i| i % 3 == t).count() as u64;
    let a = ok(base, "POST", "/_aliases", Some(json!({"actions": [
        {"add": {"index": "tickets", "alias": "t-acme", "filter": {"term": {"tenant_id": "acme"}}}},
        {"add": {"index": "tickets", "alias": "t-globex", "filter": {"term": {"tenant_id": "globex"}}}},
        {"add": {"index": "tickets", "alias": "t-initech", "filter": {"term": {"tenant_id": "initech"}}, "routing": "3"}}]}))).await;
    assert_eq!(a, json!({"acknowledged": true}));
    let al = ok(base, "GET", "/_alias/t-acme", None).await;
    assert_eq!(al, json!({"tickets": {"aliases": {"t-acme": {"filter": {"term": {"tenant_id": "acme"}}}}}}));
    let al = ok(base, "GET", "/tickets/_alias", None).await;
    assert_eq!(al["tickets"]["aliases"]["t-initech"], json!({"filter": {"term": {"tenant_id": "initech"}}, "index_routing": "3", "search_routing": "3"}));
    assert_eq!(ok(base, "GET", "/tickets", None).await["tickets"]["aliases"]["t-globex"]["filter"], json!({"term": {"tenant_id": "globex"}}));

    let tenants_of = |r: &Json| r["hits"]["hits"].as_array().unwrap().iter().map(|h| h["_source"]["tenant_id"].as_str().unwrap().to_string()).collect::<BTreeSet<_>>();
    let set = |ts: &[&str]| ts.iter().map(|t| t.to_string()).collect::<BTreeSet<_>>();
    // A count and a search.
    assert_eq!(ok(base, "GET", "/t-acme/_count", None).await["count"], of(0));
    assert_eq!(ok(base, "POST", "/t-acme/_count", Some(json!({"query": {"match": {"body": "vpn"}}}))).await["count"], (0..TICKETS).filter(|i| i % 3 == 0 && ticket(*i)["body"].as_str().unwrap().contains("vpn")).count() as u64);
    let r = ok(base, "POST", "/t-acme/_search", Some(json!({"size": 100, "track_total_hits": true}))).await;
    assert_eq!((r["hits"]["total"]["value"].as_u64(), tenants_of(&r)), (Some(of(0)), set(&["acme"])), "{r}");
    let r = ok(base, "POST", "/t-globex/_search", Some(json!({"size": 100, "query": {"bool": {"must": {"match": {"body": "issue"}}, "filter": {"range": {"n": {"gte": 0}}}}}}))).await;
    assert_eq!((r["hits"]["total"]["value"].as_u64(), tenants_of(&r)), (Some(of(1)), set(&["globex"])), "{r}");
    let r = ok(base, "GET", "/t-acme/_search?q=body:vpn&size=100", None).await;
    assert_eq!(tenants_of(&r), set(&["acme"]), "{r}");
    // Aggregations, a global one too.
    let r = ok(base, "POST", "/t-acme/_search", Some(json!({"size": 0, "aggs": {"t": {"terms": {"field": "tenant_id"}}, "all": {"global": {}, "aggs": {"t": {"terms": {"field": "tenant_id"}}}}}}))).await;
    assert_eq!(r["aggregations"]["t"]["buckets"], json!([{"key": "acme", "doc_count": of(0)}]), "{r}");
    assert_eq!(r["aggregations"]["all"]["doc_count"], json!(of(0)), "{r}");
    assert_eq!(r["aggregations"]["all"]["t"]["buckets"], json!([{"key": "acme", "doc_count": of(0)}]), "{r}");
    // By id: another tenant's document is not found through the alias.
    let (s, d) = call(base, "GET", "/t-acme/_doc/t1", None, None).await;
    assert_eq!((s, d["found"].clone()), (404, json!(false)), "{d}");
    assert_eq!(ok(base, "GET", "/t-acme/_doc/t0", None).await["_source"], ticket(0));
    assert_eq!(call(base, "GET", "/t-acme/_source/t1", None, None).await.0, 404);
    assert_eq!(call(base, "HEAD", "/t-acme/_doc/t1", None, None).await.0, 404);
    let r = ok(base, "POST", "/t-acme/_mget", Some(json!({"ids": ["t0", "t1", "t3"]}))).await;
    let found: Vec<bool> = r["docs"].as_array().unwrap().iter().map(|d| d["found"].as_bool().unwrap()).collect();
    assert_eq!(found, vec![true, false, true], "{r}");
    let r = ok(base, "POST", "/_mget", Some(json!({"docs": [{"_index": "t-globex", "_id": "t0"}, {"_index": "tickets", "_id": "t0"}]}))).await;
    assert_eq!((r["docs"][0]["found"].clone(), r["docs"][1]["found"].clone()), (json!(false), json!(true)), "{r}");
    // _msearch, each line through its alias.
    let nd = format!("{}\n{}\n{}\n{}\n", json!({"index": "t-acme"}), json!({"size": 100}), json!({"index": "t-globex"}), json!({"size": 0, "aggs": {"t": {"terms": {"field": "tenant_id"}}}}));
    let r = ok(base, "POST", "/_msearch", Some(Json::String(nd))).await;
    assert_eq!(tenants_of(&r["responses"][0]), set(&["acme"]), "{r}");
    assert_eq!(r["responses"][1]["aggregations"]["t"]["buckets"], json!([{"key": "globex", "doc_count": of(1)}]), "{r}");
    // Two aliases: either tenant's; an alias and the index: every one.
    let r = ok(base, "POST", "/t-acme,t-globex/_search", Some(json!({"size": 100}))).await;
    assert_eq!((r["hits"]["total"]["value"].as_u64(), tenants_of(&r)), (Some(of(0) + of(1)), set(&["acme", "globex"])), "{r}");
    assert_eq!(ok(base, "GET", "/t-*/_count", None).await["count"], TICKETS as u64);
    assert_eq!(ok(base, "GET", "/t-acme,tickets/_count", None).await["count"], TICKETS as u64);
    let c = ok(base, "GET", "/_cat/count/t-globex?format=json", None).await;
    assert_eq!(c[0]["count"], json!(of(1).to_string()), "{c}");
    let c = ok(base, "GET", "/_cat/aliases?format=json", None).await;
    let acme = c.as_array().unwrap().iter().find(|r| r["alias"] == "t-acme").unwrap();
    assert_eq!((acme["index"].as_str(), acme["filter"].as_str()), (Some("tickets"), Some("*")), "{c}");
    // Filters are checked when the alias is made.
    let (s, e) = call(base, "POST", "/_aliases", Some(json!({"actions": [{"add": {"index": "tickets", "alias": "bad", "filter": {"nosuch": {}}}}]})), None).await;
    assert_eq!((s, e["error"]["type"].as_str()), (400, Some("illegal_argument_exception")), "{e}");
    let (s, e) = call(base, "POST", "/_aliases", Some(json!({"actions": [{"add": {"index": "tickets", "alias": "bad", "filtr": {"term": {"tenant_id": "acme"}}}}]})), None).await;
    assert_eq!(s, 400, "{e}");
    assert!(e["error"]["reason"].as_str().unwrap().contains("[add] unknown field [filtr]; did you mean [filter]?"), "{e}");
    let (s, e) = call(base, "POST", "/_aliases", Some(json!({"actions": [{"add": {"index": "tickets", "alias": "tickets"}}]})), None).await;
    assert_eq!((s, e["error"]["type"].as_str()), (400, Some("invalid_alias_name_exception")), "{e}");
    assert_eq!(call(base, "GET", "/_alias/bad", None, None).await.0, 404);
    // An index is not deleted through an alias.
    let (s, e) = call(base, "DELETE", "/t-acme", None, None).await;
    assert_eq!((s, e["error"]["type"].as_str()), (400, Some("illegal_argument_exception")), "{e}");
    assert_eq!(ok(base, "GET", "/tickets/_count", None).await["count"], TICKETS as u64);
    // A delete by query through an alias deletes its tenant's documents.
    let r = ok(base, "POST", "/t-initech/_delete_by_query?refresh=true", Some(json!({"query": {"match_all": {}}}))).await;
    assert_eq!(r["deleted"], json!(of(2)), "{r}");
    assert_eq!(ok(base, "GET", "/tickets/_count", None).await["count"], of(0) + of(1));
    // An index made with a filtered alias.
    ok(base, "PUT", "/notes", Some(json!({"mappings": {"properties": {"owner": {"type": "keyword"}}}, "aliases": {"notes-mine": {"filter": {"term": {"owner": "me"}}}}}))).await;
    ok(base, "POST", "/notes/_doc/1?refresh=true", Some(json!({"owner": "me", "text": "a"}))).await;
    ok(base, "POST", "/notes/_doc/2?refresh=true", Some(json!({"owner": "you", "text": "b"}))).await;
    assert_eq!(ok(base, "GET", "/notes-mine/_count", None).await["count"], 1);
    // Deleting an index takes its aliases with it.
    ok(base, "DELETE", "/notes", None).await;
    assert_eq!(call(base, "GET", "/_alias/notes-mine", None, None).await.0, 404);
    ok(base, "DELETE", "/tickets", None).await;
    assert_eq!(call(base, "GET", "/t-acme/_count", None, None).await.0, 404);
}

/// Keys a search does not read are refused, naming the key, as
/// OpenSearch refuses them, rather than left out (finding KENJI-302): at
/// the top of the body, inside `bool`, inside `match`, and the parts of
/// OpenSearch's search ChloeDB does not answer.
async fn unknown_keys(base: &str) {
    ok(base, "PUT", "/strict", Some(json!({"mappings": {"properties": {"tenant_id": {"type": "keyword"}, "body": {"type": "text"}}}}))).await;
    let mut nd = String::new();
    for i in 0..9 {
        nd.push_str(&format!("{}\n{}\n", json!({"index": {"_id": format!("s{i}")}}), ticket(i)));
    }
    assert_eq!(ok(base, "POST", "/strict/_bulk?refresh=true", Some(Json::String(nd))).await["errors"], json!(false));
    let refused = |body: Json, path: &'static str| async move {
        let (s, e) = call(base, "POST", path, Some(body.clone()), None).await;
        assert_eq!((s, e["error"]["type"].as_str()), (400, Some("parsing_exception")), "{body}: {e}");
        e["error"]["reason"].as_str().unwrap().to_string()
    };
    let why = refused(json!({"size": 1, "qeury": {"match": {"body": "vpn"}}}), "/strict/_search").await;
    assert!(why.contains("Unknown key for a START_OBJECT in [qeury]") && why.contains("did you mean [query]?"), "{why}");
    let why = refused(json!({"query": {"bool": {"must": {"match": {"body": "vpn"}}, "filtr": {"term": {"tenant_id": "acme"}}}}}), "/strict/_search").await;
    assert!(why.contains("[bool] query does not support [filtr]; did you mean [filter]?"), "{why}");
    let why = refused(json!({"query": {"match": {"body": {"query": "vpn", "operater": "and"}}}}), "/strict/_search").await;
    assert!(why.contains("[match] query does not support [operater]; did you mean [operator]?"), "{why}");
    let why = refused(json!({"query": {"match": {"body": "vpn", "tenant_id": "acme"}}}), "/strict/_search").await;
    assert!(why.contains("doesn't support multiple fields"), "{why}");
    let why = refused(json!({"query": {"term": {"tenant_id": {"value": "acme", "boots": 2}}}}), "/strict/_search").await;
    assert!(why.contains("[term] query does not support [boots]"), "{why}");
    for (key, body) in [
        ("rescore", json!({"query": {"match": {"body": "vpn"}}, "rescore": {"window_size": 10, "query": {"rescore_query": {"match_phrase": {"body": "vpn password"}}}}})),
        ("suggest", json!({"suggest": {"s": {"text": "pasword", "term": {"field": "body"}}}})),
        ("runtime_mappings", json!({"runtime_mappings": {"day": {"type": "keyword"}}, "aggs": {"d": {"terms": {"field": "day"}}}})),
        ("pit", json!({"size": 1, "pit": {"id": "abc", "keep_alive": "1m"}})),
        ("slice", json!({"slice": {"id": 0, "max": 2}})),
    ] {
        let why = refused(body, "/strict/_search").await;
        assert!(why.contains("ChloeDB"), "{key}: {why}");
    }
    // Through _msearch, _count and a scroll as well.
    let nd = format!("{}\n{}\n", json!({"index": "strict"}), json!({"qeury": {"match_all": {}}}));
    let r = ok(base, "POST", "/_msearch", Some(Json::String(nd))).await;
    assert_eq!((r["responses"][0]["status"].as_u64(), r["responses"][0]["error"]["type"].as_str()), (Some(400), Some("parsing_exception")), "{r}");
    refused(json!({"qeury": {"match_all": {}}}), "/strict/_count").await;
    refused(json!({"query": {"match_all": {}}, "max_docs": 1}), "/strict/_delete_by_query").await;
    // What is spelt right answers as before, keys that change no hit too.
    let r = ok(base, "POST", "/strict/_search", Some(json!({"query": {"bool": {"must": {"match": {"body": {"query": "vpn", "operator": "and"}}}, "filter": {"term": {"tenant_id": "acme"}}}}, "timeout": "1s", "explain": false}))).await;
    assert!(r["hits"]["hits"].as_array().unwrap().iter().all(|h| h["_source"]["tenant_id"] == "acme"), "{r}");
    ok(base, "DELETE", "/strict", None).await;
}

/// An alias moved from one index to another in one `_aliases` request,
/// while four clients search it: each search finds the one document of
/// the old index or of the new one, never no index or both (finding
/// KENJI-303). Then an alias over two indexes writes to its write index.
async fn alias_swaps(base: &str) {
    for (ix, id) in [("swap_a", "a"), ("swap_b", "b")] {
        ok(base, "PUT", &format!("/{ix}/_doc/{id}?refresh=true"), Some(json!({"from": ix}))).await;
    }
    ok(base, "POST", "/_aliases", Some(json!({"actions": [{"add": {"index": "swap_a", "alias": "live"}}]}))).await;
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut searchers = Vec::new();
    for _ in 0..4 {
        let (base, stop) = (base.to_string(), stop.clone());
        searchers.push(tokio::spawn(async move {
            let mut n = 0;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                let (s, r) = call(&base, "POST", "/live/_search", Some(json!({"query": {"match_all": {}}})), None).await;
                assert_eq!((s, r["hits"]["total"]["value"].as_u64()), (200, Some(1)), "a search in the middle of a swap: {r}");
                let ids: Vec<&str> = r["hits"]["hits"].as_array().unwrap().iter().map(|h| h["_id"].as_str().unwrap()).collect();
                assert!(ids == ["a"] || ids == ["b"], "{r}");
                n += 1;
            }
            n
        }));
    }
    let mut at = ("swap_a", "swap_b");
    for _ in 0..20 {
        ok(base, "POST", "/_aliases", Some(json!({"actions": [{"remove": {"index": at.0, "alias": "live"}}, {"add": {"index": at.1, "alias": "live"}}]}))).await;
        at = (at.1, at.0);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let mut searched = 0;
    for s in searchers {
        searched += s.await.unwrap();
    }
    assert!(searched >= 20, "{searched} searches");
    assert_eq!(ok(base, "GET", "/_alias/live", None).await, json!({"swap_a": {"aliases": {"live": {}}}}));
    // An alias over both, the write index named: writes go there.
    ok(base, "POST", "/_aliases", Some(json!({"actions": [{"add": {"index": "swap_a", "alias": "both"}}, {"add": {"index": "swap_b", "alias": "both", "is_write_index": true}}]}))).await;
    let (s, e) = call(base, "POST", "/_aliases", Some(json!({"actions": [{"add": {"index": "swap_a", "alias": "both", "is_write_index": true}}]})), None).await;
    assert_eq!((s, e["error"]["type"].as_str()), (400, Some("illegal_state_exception")), "{e}");
    let w = ok(base, "PUT", "/both/_doc/c?refresh=true", Some(json!({"from": "both"}))).await;
    assert_eq!(w["_index"], "swap_b", "{w}");
    assert_eq!(ok(base, "GET", "/both/_count", None).await["count"], 3);
    ok(base, "POST", "/_aliases", Some(json!({"actions": [{"add": {"index": "swap_b", "alias": "both", "is_write_index": false}}]}))).await;
    let (s, e) = call(base, "PUT", "/both/_doc/d", Some(json!({"from": "both"})), None).await;
    assert_eq!((s, e["error"]["type"].as_str()), (400, Some("illegal_argument_exception")), "{e}");
    for ix in ["swap_a", "swap_b"] {
        ok(base, "DELETE", &format!("/{ix}"), None).await;
    }
}

const TAGS: [&str; 6] = ["billing", "bug", "login", "refund", "vpn", "urgent"];

/// Document `i` of the facets index: an id, a day shared by many (none
/// for some), and a list of tags, a repeated one for some, none for others.
fn facet_doc(i: usize) -> Json {
    let n = (i * 7) % 5;
    let mut tags: Vec<&str> = (0..n).map(|k| TAGS[(i + k * 3) % TAGS.len()]).collect();
    if i % 11 == 0 {
        tags.push(TAGS[i % TAGS.len()]);
    }
    let mut d = json!({"id": format!("c{:03}", (i * 37) % 300), "tags": tags, "body": "refund please"});
    if i % 50 != 7 {
        d["created_at"] = json!(format!("2026-09-{:02}T04:31:49Z", 1 + i % 9));
    }
    d
}

/// A terms aggregation on a field of arrays counts each document once in
/// each tag it holds, as one on a single keyword does (finding KENJI-401),
/// and a sort by a date and then a keyword pages as one sort of every
/// match does, by from and size and by search_after (KENJI-404).
async fn facets_and_sorts(base: &str) {
    ok(base, "PUT", "/facets", Some(json!({"mappings": {"properties": {"id": {"type": "keyword"}, "tags": {"type": "keyword"}, "created_at": {"type": "date"}, "body": {"type": "text"}}}}))).await;
    let n = 300;
    let mut nd = String::new();
    for i in 0..n {
        nd.push_str(&format!("{}\n{}\n", json!({"index": {"_id": format!("f{i}")}}), facet_doc(i)));
    }
    assert_eq!(ok(base, "POST", "/facets/_bulk?refresh=true", Some(Json::String(nd))).await["errors"], json!(false));
    // Each document once per distinct tag.
    let mut counts: std::collections::BTreeMap<String, u64> = Default::default();
    for i in 0..n {
        let tags: BTreeSet<String> = facet_doc(i)["tags"].as_array().unwrap().iter().map(|t| t.as_str().unwrap().to_string()).collect();
        for t in tags {
            *counts.entry(t).or_default() += 1;
        }
    }
    let mut want: Vec<(String, u64)> = counts.into_iter().collect();
    want.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let total: u64 = want.iter().map(|w| w.1).sum();
    let r = ok(base, "POST", "/facets/_search", Some(json!({"size": 0, "aggs": {"t": {"terms": {"field": "tags", "size": 3}}}}))).await;
    let got: Vec<(String, u64)> = r["aggregations"]["t"]["buckets"].as_array().unwrap().iter().map(|b| (b["key"].as_str().unwrap().to_string(), b["doc_count"].as_u64().unwrap())).collect();
    assert_eq!(got, want[..3].to_vec(), "{r}");
    assert_eq!(r["aggregations"]["t"]["sum_other_doc_count"].as_u64(), Some(total - want[..3].iter().map(|w| w.1).sum::<u64>()), "{r}");

    let ids = |r: &Json| r["hits"]["hits"].as_array().unwrap().iter().map(|h| h["_id"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    for dir in ["desc", "asc"] {
        let sort = json!([{"created_at": {"order": dir}}, {"id": "asc"}]);
        let all = ids(&ok(base, "POST", "/facets/_search", Some(json!({"size": n, "sort": sort}))).await);
        assert_eq!(all.len(), n);
        // In the order asked: days, then ids within a day.
        let key = |id: &str| {
            let d = facet_doc(id[1..].parse().unwrap());
            (d["created_at"].as_str().map(String::from), d["id"].as_str().unwrap().to_string())
        };
        let dated: Vec<(String, String)> = all.iter().map(|id| key(id)).filter_map(|(at, id)| at.map(|at| (at, id))).collect();
        assert!(dated.windows(2).all(|w| if dir == "desc" { w[0].0 > w[1].0 || (w[0].0 == w[1].0 && w[0].1 < w[1].1) } else { w[0] < w[1] }), "{dir}: {dated:?}");
        for (from, size) in [(0, 10), (5, 7), (40, 40), (290, 20)] {
            let page = ids(&ok(base, "POST", "/facets/_search", Some(json!({"from": from, "size": size, "sort": sort, "query": {"match": {"body": "refund"}}}))).await);
            assert_eq!(page, all[from.min(n)..(from + size).min(n)].to_vec(), "{dir} from {from} size {size}");
        }
        // search_after over the dated documents, by their sort values.
        let mut paged = Vec::new();
        let mut after: Option<Json> = None;
        loop {
            let mut b = json!({"size": 13, "sort": sort, "query": {"exists": {"field": "created_at"}}});
            if let Some(a) = &after {
                b["search_after"] = a.clone();
            }
            let r = ok(base, "POST", "/facets/_search", Some(b)).await;
            let hits = r["hits"]["hits"].as_array().unwrap().clone();
            let Some(last) = hits.last() else { break };
            after = Some(last["sort"].clone());
            paged.extend(hits.iter().map(|h| h["_id"].as_str().unwrap().to_string()));
        }
        let want: Vec<String> = all.iter().filter(|id| key(id).0.is_some()).cloned().collect();
        assert_eq!(paged, want, "{dir}: search_after");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn array_facets_and_keyword_tie_breakers_answer_as_opensearch() {
    let dir = tempfile::tempdir().unwrap();
    facets_and_sorts(&serve(Arc::new(local(dir.path())), Access::default()).await).await;
    facets_and_sorts(&serve(Arc::new(cluster(dir.path()).await), Access::default()).await).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn filtered_aliases_keep_each_tenant_to_its_documents() {
    let dir = tempfile::tempdir().unwrap();
    tenant_aliases(&serve(Arc::new(local(dir.path())), Access::default()).await).await;
    tenant_aliases(&serve(Arc::new(cluster(dir.path()).await), Access::default()).await).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn search_bodies_with_unknown_keys_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    unknown_keys(&serve(Arc::new(local(dir.path())), Access::default()).await).await;
    unknown_keys(&serve(Arc::new(cluster(dir.path()).await), Access::default()).await).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_alias_swap_is_seen_whole() {
    let dir = tempfile::tempdir().unwrap();
    alias_swaps(&serve(Arc::new(local(dir.path())), Access::default()).await).await;
    alias_swaps(&serve(Arc::new(cluster(dir.path()).await), Access::default()).await).await;
}

/// Aliases kept by an older version, in each index's row of
/// `opensearch_indices`: they resolve as before, and the first change of
/// an alias keeps them, in the one row that keeps every alias since.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn aliases_kept_by_an_older_version_carry_over() {
    let dir = tempfile::tempdir().unwrap();
    let svc = Arc::new(local(dir.path()));
    let base = serve(svc.clone(), Access::default()).await;
    ok(&base, "PUT", "/old", None).await;
    ok(&base, "PUT", "/old/_doc/1?refresh=true", Some(json!({"k": "v"}))).await;
    // The index's row as 0.3.8 wrote it with an alias.
    let r = svc.query(chloedb_server::service::QueryReq { sql: "SELECT name, meta FROM opensearch_indices".into(), reads: true, ..Default::default() }).await.unwrap();
    let meta = r.rs.rows.iter().find(|row| chloedb_server::json::to_json(&row[0]) == json!("old")).map(|row| chloedb_server::json::to_json(&row[1])).unwrap();
    svc.write_request(vec![json!({"op": "upsert", "table": "opensearch_indices", "doc": {"name": "old", "aliases": ["before"], "meta": meta}})], None).await.unwrap();
    assert_eq!(ok(&base, "GET", "/before/_count", None).await["count"], 1);
    assert_eq!(ok(&base, "GET", "/_alias", None).await, json!({"old": {"aliases": {"before": {}}}}));
    ok(&base, "POST", "/_aliases", Some(json!({"actions": [{"add": {"index": "old", "alias": "after"}}]}))).await;
    assert_eq!(ok(&base, "GET", "/_alias", None).await, json!({"old": {"aliases": {"after": {}, "before": {}}}}));
    assert_eq!(ok(&base, "GET", "/before/_count", None).await["count"], 1);
    assert_eq!(ok(&base, "GET", "/after/_count", None).await["count"], 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn negations_keep_documents_without_the_field() {
    let dir = tempfile::tempdir().unwrap();
    negations(&serve(Arc::new(local(dir.path())), Access::default()).await).await;
    negations(&serve(Arc::new(cluster(dir.path()).await), Access::default()).await).await;
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
    let reader = svc.keys.create("dashboards", chloedb_server::keys::Scope::Read, None, true, false).unwrap().1;
    let writer = svc.keys.create("ingest", chloedb_server::keys::Scope::Write, None, true, false).unwrap().1;
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
