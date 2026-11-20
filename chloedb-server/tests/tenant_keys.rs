//! Keys bound to a tenant of one shared index on the OpenSearch API
//! (finding KENJI-402): a key made with `"tenant": {"index": "tickets",
//! "field": "tenant_id", "value": "acme"}` reads acme's documents alone, by
//! every read the API answers, writes acme's documents alone, changes no
//! index, mapping or alias, and is refused on the HTTP API, MCP and gRPC,
//! where its filter is not enforced. The ways across a tenant's reviewer
//! would try, each refused; then an admin checks that nothing crossed.
//! Every value here is made up.

use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use chloedb::db::Db;
use chloedb::storage::DbOptions;
use chloedb_server::auth::{Access, Secret};
use chloedb_server::keys::Keys;
use chloedb_server::service::{AgentConfig, Backend, Service};
use chloedb_server::{demo, pb, server};
use serde_json::{json, Value as Json};
use tokio::net::TcpListener;

const ROOT: &str = "root-token";

/// One server with its keys in a file: its HTTP API, its OpenSearch API
/// and its gRPC, all with the token `ROOT`.
async fn start(dir: &std::path::Path) -> (String, String, String) {
    let db = Db::create(&dir.join("t.strata"), demo::schema(10), DbOptions { publish_snapshots: true, ..DbOptions::default() }).unwrap();
    let schema = (*db.schema()).clone();
    let agent = AgentConfig { api_key: None, model: String::new(), base_url: String::new() };
    let mut svc = Service::new(Backend::Local(Arc::new(db)), schema, agent, 16, Duration::from_secs(10));
    svc.keys = Arc::new(Keys::open(dir.join("t.strata.keys.json")).unwrap());
    let svc = Arc::new(svc);
    let access = Access { token: Some(Secret::new(ROOT)), ..Default::default() };
    let bind = || async {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
        (l, url)
    };
    let (hl, http) = bind().await;
    tokio::spawn(server::serve_http(svc.clone(), hl, access.clone()));
    let (ol, os) = bind().await;
    tokio::spawn(server::serve_opensearch(svc.clone(), ol, access.clone(), chloedb_server::opensearch::VERSION_NUMBER));
    let (gl, grpc) = bind().await;
    tokio::spawn(server::serve_grpc(svc, gl, access));
    (http, os, grpc)
}

async fn call(base: &str, method: &str, path: &str, body: Option<Json>, auth: &str) -> (u16, Json) {
    let c = chloedb_server::http_client().build().unwrap();
    let mut r = c.request(reqwest::Method::from_bytes(method.as_bytes()).unwrap(), format!("{base}{path}")).bearer_auth(auth);
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

async fn root(base: &str, method: &str, path: &str, body: Option<Json>) -> Json {
    let (s, j) = call(base, method, path, body, ROOT).await;
    assert!(s == 200 || s == 201, "{method} {path}: {s} {j}");
    j
}

const TENANTS: [&str; 3] = ["acme", "globex", "initech"];
const TICKETS: usize = 45;

fn ticket(i: usize) -> Json {
    let words = ["printer", "vpn", "password", "invoice", "laptop"];
    json!({"tenant_id": (TENANTS[i % 3]), "body": format!("{} {} issue", words[i % 5], words[(i / 5) % 5]), "status": (["open", "closed"][i % 2]), "n": i})
}

/// The id of ticket `i`.
fn id(i: usize) -> String {
    format!("{}-{i:03}", &TENANTS[i % 3][..1])
}

fn of(t: usize) -> usize {
    (0..TICKETS).filter(|i| i % 3 == t).count()
}

fn hits(r: &Json) -> Vec<Json> {
    r["hits"]["hits"].as_array().cloned().unwrap_or_default()
}

/// Every hit is acme's.
fn acme_only(r: &Json, what: &str) {
    assert!(hits(r).iter().all(|h| h["_source"]["tenant_id"] == "acme"), "{what}: {r}");
}

/// Refused as OpenSearch's security plugin refuses: 403, security_exception.
fn refused(got: (u16, Json), what: &str) {
    assert_eq!((got.0, got.1["error"]["type"].as_str()), (403, Some("security_exception")), "{what}: {}", got.1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_key_bound_to_a_tenant_reaches_its_tenants_documents_alone() {
    let dir = tempfile::tempdir().unwrap();
    let (http, os, grpc) = start(dir.path()).await;

    // The shared index, another tenant's alias over it, and another index.
    root(&os, "PUT", "/tickets", Some(json!({"mappings": {"properties": {"tenant_id": {"type": "keyword"}, "body": {"type": "text"}, "status": {"type": "keyword"}, "n": {"type": "long"}}}}))).await;
    let mut nd = String::new();
    for i in 0..TICKETS {
        nd.push_str(&format!("{}\n{}\n", json!({"index": {"_id": id(i)}}), ticket(i)));
    }
    assert_eq!(root(&os, "POST", "/tickets/_bulk?refresh=true", Some(Json::String(nd))).await["errors"], json!(false));
    root(&os, "POST", "/_aliases", Some(json!({"actions": [{"add": {"index": "tickets", "alias": "t-globex", "filter": {"term": {"tenant_id": "globex"}}}}]}))).await;
    root(&os, "PUT", "/orders/_doc/o1?refresh=true", Some(json!({"tenant_id": "acme", "total": 10}))).await;
    root(&os, "PUT", "/notes", Some(json!({"mappings": {"properties": {"tenant_id": {"type": "text"}}}}))).await;

    // Keys: made by an admin, refused when they would be anything more.
    let key = |body: Json| {
        let http = http.clone();
        async move { call(&http, "POST", "/v1/admin/keys", Some(body), ROOT).await }
    };
    for bad in [
        json!({"name": "x", "scope": "admin", "tenant": {"index": "tickets", "field": "tenant_id", "value": "acme"}}),
        json!({"name": "x", "scope": "write", "tables": {"os_tickets": "write"}, "tenant": {"index": "tickets", "field": "tenant_id", "value": "acme"}}),
        json!({"name": "x", "scope": "read", "routes": ["fleet"], "tenant": {"index": "tickets", "field": "tenant_id", "value": "acme"}}),
        json!({"name": "x", "tenant": {"index": "tick*", "field": "tenant_id", "value": "acme"}}),
        json!({"name": "x", "tenant": {"index": "tickets", "field": "tenant_id"}}),
        json!({"name": "x", "tenant": {"index": "tickets", "field": "tenant_id", "value": "acme", "filter": {}}}),
        json!({"name": "x", "scope": "ops", "tenant": {"index": "tickets", "field": "tenant_id", "value": "acme"}}),
    ] {
        assert_eq!(key(bad.clone()).await.0, 400, "{bad}");
    }
    let (s, made) = key(json!({"name": "tenant-acme", "scope": "write", "tenant": {"index": "tickets", "field": "tenant_id", "value": "acme"}})).await;
    assert_eq!(s, 200, "{made}");
    assert_eq!(made["key"]["tenant"], json!({"indices": ["tickets"], "field": "tenant_id", "value": "acme"}), "{made}");
    assert_eq!((made["key"]["scope"].as_str(), made["key"]["limited"].as_bool()), (Some("write"), Some(true)), "{made}");
    let a = made["secret"].as_str().unwrap().to_string();
    let (_, made) = key(json!({"name": "tenant-globex", "scope": "read", "tenant": {"index": "tickets", "field": "tenant_id", "value": "globex"}})).await;
    let b = made["secret"].as_str().unwrap().to_string();
    let (_, made) = key(json!({"name": "tenant-notes", "scope": "read", "tenant": {"index": "notes", "field": "tenant_id", "value": "acme"}})).await;
    let c = made["secret"].as_str().unwrap().to_string();
    let a = a.as_str();
    let ra = |m: &'static str, p: &str, body: Option<Json>| {
        let (os, p) = (os.clone(), p.to_string());
        async move { call(&os, m, &p, body, a).await }
    };
    let (acme, globex) = (of(0), of(1));

    // --- Reads: acme's documents alone.
    // 1. Every document of the index.
    let (s, r) = ra("POST", "/tickets/_search", Some(json!({"size": 100}))).await;
    assert_eq!((s, r["hits"]["total"]["value"].as_u64()), (200, Some(acme as u64)), "{r}");
    acme_only(&r, "match_all");
    // 2. Another tenant's by its field.
    let (_, r) = ra("POST", "/tickets/_search", Some(json!({"query": {"term": {"tenant_id": "globex"}}}))).await;
    assert_eq!(r["hits"]["total"]["value"], 0, "{r}");
    // 3. In a should, beside its own.
    let (_, r) = ra("POST", "/tickets/_search", Some(json!({"size": 100, "query": {"bool": {"should": [{"term": {"tenant_id": "globex"}}, {"term": {"tenant_id": "acme"}}]}}}))).await;
    assert_eq!(r["hits"]["total"]["value"].as_u64(), Some(acme as u64), "{r}");
    acme_only(&r, "should");
    // 4. A must_not of its own.
    let (_, r) = ra("POST", "/tickets/_search", Some(json!({"query": {"bool": {"must_not": {"term": {"tenant_id": "acme"}}}}}))).await;
    assert_eq!(r["hits"]["total"]["value"], 0, "{r}");
    // 5. A query string, in the URL.
    let (_, r) = ra("GET", "/tickets/_search?q=tenant_id:globex", None).await;
    assert_eq!(r["hits"]["total"]["value"], 0, "{r}");
    // 6. Aggregations: terms, cardinality, global, a top_hits inside.
    let (_, r) = ra("POST", "/tickets/_search", Some(json!({"size": 0, "aggs": {
        "t": {"terms": {"field": "tenant_id", "min_doc_count": 0}, "aggs": {"top": {"top_hits": {"size": 50}}}},
        "all": {"global": {}, "aggs": {"t": {"terms": {"field": "tenant_id"}}}},
        "n": {"cardinality": {"field": "tenant_id"}}}}))).await;
    let buckets = r["aggregations"]["t"]["buckets"].as_array().unwrap();
    assert_eq!(buckets.iter().map(|b| b["key"].clone()).collect::<Vec<_>>(), vec![json!("acme")], "{r}");
    assert!(buckets[0]["top"]["hits"]["hits"].as_array().unwrap().iter().all(|h| h["_source"]["tenant_id"] == "acme"), "{r}");
    assert_eq!(r["aggregations"]["all"]["doc_count"].as_u64(), Some(acme as u64), "global: {r}");
    assert_eq!(r["aggregations"]["all"]["t"]["buckets"].as_array().unwrap().len(), 1, "{r}");
    assert_eq!(r["aggregations"]["n"]["value"], 1, "{r}");
    // 7. A post_filter of another's.
    let (_, r) = ra("POST", "/tickets/_search", Some(json!({"post_filter": {"term": {"tenant_id": "globex"}}}))).await;
    assert_eq!(r["hits"]["total"]["value"], 0, "{r}");
    // 8. Counts.
    assert_eq!(ra("GET", "/tickets/_count", None).await.1["count"].as_u64(), Some(acme as u64));
    assert_eq!(ra("POST", "/tickets/_count", Some(json!({"query": {"term": {"tenant_id": "globex"}}}))).await.1["count"], 0);
    // 9. By id: another's is not there, whichever way it is asked.
    assert_eq!(ra("GET", &format!("/tickets/_doc/{}", id(1)), None).await.0, 404);
    assert_eq!(ra("GET", &format!("/tickets/_source/{}", id(1)), None).await.0, 404);
    assert_eq!(ra("HEAD", &format!("/tickets/_doc/{}", id(1)), None).await.0, 404);
    assert_eq!(ra("GET", &format!("/tickets/_doc/{}", id(0)), None).await.0, 200);
    let (_, r) = ra("POST", "/tickets/_mget", Some(json!({"ids": [id(0), id(1)]}))).await;
    assert_eq!((r["docs"][0]["found"].as_bool(), r["docs"][1]["found"].as_bool()), (Some(true), Some(false)), "{r}");
    // 10. _mget naming another index, or another's alias.
    let (_, r) = ra("POST", "/_mget", Some(json!({"docs": [{"_index": "orders", "_id": "o1"}, {"_index": "t-globex", "_id": id(1)}]}))).await;
    assert!(r["docs"].as_array().unwrap().iter().all(|d| d["error"]["type"] == "security_exception" && d.get("_source").is_none()), "{r}");
    // 11. _msearch: its index bound, any other refused.
    let nd = format!("{}\n{}\n{}\n{}\n{}\n{}\n", json!({"index": "tickets"}), json!({"size": 100}), json!({"index": "orders"}), json!({}), json!({}), json!({}));
    let (_, r) = ra("POST", "/_msearch", Some(Json::String(nd))).await;
    let rs = r["responses"].as_array().unwrap();
    assert_eq!(rs[0]["hits"]["total"]["value"].as_u64(), Some(acme as u64), "{r}");
    acme_only(&rs[0], "msearch");
    assert_eq!((rs[1]["status"].as_u64(), rs[2]["status"].as_u64()), (Some(403), Some(403)), "{r}");
    // 12. A scroll pages acme's alone; a scroll id made up to name another
    // index is refused.
    let (_, r) = ra("POST", "/tickets/_search?scroll=1m", Some(json!({"size": 4, "sort": ["n"]}))).await;
    let mut seen = hits(&r).len();
    let mut sid = r["_scroll_id"].as_str().unwrap().to_string();
    loop {
        let (s, r) = ra("POST", "/_search/scroll", Some(json!({"scroll": "1m", "scroll_id": sid}))).await;
        assert_eq!(s, 200, "{r}");
        acme_only(&r, "scroll");
        if hits(&r).is_empty() {
            break;
        }
        seen += hits(&r).len();
        sid = r["_scroll_id"].as_str().unwrap().to_string();
    }
    assert_eq!(seen, acme);
    for other in ["orders", "*", "t-globex"] {
        let forged = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json!({"i": other, "b": {}, "q": {}}).to_string());
        refused(ra("POST", "/_search/scroll", Some(json!({"scroll_id": forged}))).await, other);
    }
    // 13. Other indexes, patterns, every index, several, another's alias.
    for p in ["/_search", "/tick*/_search", "/_all/_search", "/*/_search", "/tickets,orders/_search", "/orders/_search", "/t-globex/_search", "/-orders/_search", "/_count", "/orders/_count", "/t-globex/_count"] {
        refused(ra("POST", p, Some(json!({}))).await, p);
    }
    // 14. Listings and settings of the server, and an index's whole entry.
    for p in ["/_cat/indices", "/_cat/count/tickets", "/_cat/aliases", "/tickets/_stats", "/tickets", "/_alias", "/tickets/_alias", "/_mapping", "/tickets/_settings", "/_nodes", "/_cluster/state"] {
        refused(ra("GET", p, None).await, p);
    }
    assert_eq!(ra("GET", "/tickets/_mapping", None).await.0, 200);
    assert_eq!(ra("HEAD", "/tickets", None).await.0, 200);
    assert_eq!(ra("HEAD", "/orders", None).await.0, 403);
    assert_eq!(ra("GET", "/_cluster/health", None).await.1["active_primary_shards"], 1);
    // 15. An update by query counts acme's.
    assert_eq!(ra("POST", "/tickets/_update_by_query", Some(json!({"query": {"match_all": {}}}))).await.1["total"].as_u64(), Some(acme as u64));

    // --- Writes: acme's documents alone.
    // 16. Its own: written, and found.
    let (s, r) = ra("PUT", "/tickets/_doc/a-new?refresh=true", Some(json!({"tenant_id": "acme", "body": "new vpn issue", "status": "open", "n": 100}))).await;
    assert_eq!(s, 201, "{r}");
    assert_eq!(ra("GET", "/tickets/_doc/a-new", None).await.0, 200);
    // 17. A document of another tenant, of none, of both, in an object.
    for (doc, what) in [
        (json!({"tenant_id": "globex", "body": "x"}), "another's"),
        (json!({"body": "x"}), "no tenant"),
        (json!({"tenant_id": ["acme", "globex"], "body": "x"}), "two tenants"),
        (json!({"tenant_id": "ACME", "body": "x"}), "another case"),
    ] {
        refused(ra("PUT", "/tickets/_doc/evil-1", Some(doc)).await, what);
    }
    refused(ra("POST", "/tickets/_doc", Some(json!({"tenant_id": "globex"}))).await, "an id the server gives");
    // An object where the tenant's keyword is: refused, by the mapping or by the tenant.
    let (s, r) = ra("PUT", "/tickets/_doc/evil-1", Some(json!({"tenant_id": {"id": "acme"}, "body": "x"}))).await;
    assert!(s == 400 || s == 403, "an object: {s} {r}");
    // 18. Another's id: neither replaced, created, updated nor deleted.
    let g = id(1);
    refused(ra("PUT", &format!("/tickets/_doc/{g}"), Some(json!({"tenant_id": "acme", "body": "mine now"}))).await, "replace");
    refused(ra("PUT", &format!("/tickets/_create/{g}"), Some(json!({"tenant_id": "acme"}))).await, "create");
    assert_eq!(ra("POST", &format!("/tickets/_update/{g}"), Some(json!({"doc": {"status": "closed"}}))).await.0, 404);
    refused(ra("POST", &format!("/tickets/_update/{g}"), Some(json!({"doc": {"tenant_id": "acme"}, "doc_as_upsert": true}))).await, "upsert");
    assert_eq!(ra("POST", &format!("/tickets/_update/{g}?if_seq_no=0&if_primary_term=1"), Some(json!({"doc": {"status": "x"}}))).await.0, 409);
    assert_eq!(ra("DELETE", &format!("/tickets/_doc/{g}"), None).await.0, 404);
    // 19. Its own moved to another tenant.
    refused(ra("POST", &format!("/tickets/_update/{}", id(0)), Some(json!({"doc": {"tenant_id": "globex"}}))).await, "moved");
    // 20. A field the mapping does not have.
    let (s, r) = ra("PUT", "/tickets/_doc/a-field", Some(json!({"tenant_id": "acme", "sla_hours": 4}))).await;
    assert_eq!((s, r["error"]["type"].as_str()), (400, Some("strict_dynamic_mapping_exception")), "{r}");
    // 21. Another index, or one that does not exist (none is made), or
    // another's alias.
    for p in ["/orders/_doc/o2", "/brand-new/_doc/1", "/t-globex/_doc/evil-2"] {
        refused(ra("PUT", p, Some(json!({"tenant_id": "acme"}))).await, p);
    }
    // 22. _bulk: each item on its own.
    let nd = [
        (json!({"index": {"_index": "tickets", "_id": "a-bulk"}}), Some(json!({"tenant_id": "acme", "body": "bulk"}))),
        (json!({"index": {"_index": "tickets", "_id": "evil-3"}}), Some(json!({"tenant_id": "globex"}))),
        (json!({"index": {"_index": "orders", "_id": "o3"}}), Some(json!({"tenant_id": "acme"}))),
        (json!({"index": {"_index": "t-globex", "_id": "evil-4"}}), Some(json!({"tenant_id": "globex"}))),
        (json!({"delete": {"_index": "tickets", "_id": id(4)}}), None),
        (json!({"update": {"_index": "tickets", "_id": id(7)}}), Some(json!({"doc": {"tenant_id": "acme"}}))),
        (json!({"create": {"_index": "tickets", "_id": id(10)}}), Some(json!({"tenant_id": "acme"}))),
    ]
    .iter()
    .map(|(m, d)| format!("{m}\n{}", d.as_ref().map(|d| format!("{d}\n")).unwrap_or_default()))
    .collect::<String>();
    let (s, r) = ra("POST", "/_bulk?refresh=true", Some(Json::String(nd))).await;
    assert_eq!(s, 200, "{r}");
    let status: Vec<u64> = r["items"].as_array().unwrap().iter().map(|i| i.as_object().unwrap().values().next().unwrap()["status"].as_u64().unwrap()).collect();
    assert_eq!(status, vec![201, 403, 403, 403, 404, 404, 403], "{r}");
    // 23. A delete by query deletes acme's alone.
    let (_, r) = ra("POST", "/tickets/_delete_by_query", Some(json!({"query": {"ids": {"values": [id(1), id(2), id(3)]}}}))).await;
    assert_eq!(r["deleted"], 1, "{r}");
    // 24. Indexes, mappings and aliases are an admin's.
    refused(ra("PUT", "/tickets/_mapping", Some(json!({"properties": {"x": {"type": "keyword"}}}))).await, "mapping");
    refused(ra("POST", "/_aliases", Some(json!({"actions": [{"add": {"index": "tickets", "alias": "mine"}}]}))).await, "aliases");
    refused(ra("PUT", "/tickets/_alias/mine", None).await, "alias");
    refused(ra("DELETE", "/tickets", None).await, "delete index");
    refused(ra("PUT", "/acme-own", Some(json!({}))).await, "make index");
    refused(ra("PUT", "/tickets/_settings", Some(json!({"index": {"max_result_window": 5}}))).await, "settings");

    // --- The read key of another tenant: its own alone, and no write.
    let (_, r) = call(&os, "POST", "/tickets/_search", Some(json!({"size": 100})), &b).await;
    assert_eq!(r["hits"]["total"]["value"].as_u64(), Some(globex as u64), "{r}");
    assert!(hits(&r).iter().all(|h| h["_source"]["tenant_id"] == "globex"), "{r}");
    assert_eq!(call(&os, "PUT", "/tickets/_doc/g-new", Some(json!({"tenant_id": "globex"})), &b).await.0, 403);
    // A tenant field that is not a keyword: refused, not guessed at.
    refused(call(&os, "POST", "/notes/_search", Some(json!({})), &c).await, "text field");

    // --- Where the filter is not enforced, the key is refused.
    for (m, p, body) in [
        ("POST", "/v1/sql", Some(json!({"sql": "SELECT COUNT(*) FROM os_tickets"}))),
        ("POST", "/v1/query", Some(json!({"sql": "SELECT _id FROM os_tickets"}))),
        ("POST", "/v1/write", Some(json!({"ops": [{"op": "upsert", "table": "os_tickets", "doc": {"_id": "evil-5"}}]}))),
        ("GET", "/v1/tables", None),
        ("GET", "/v1/schema?table=os_tickets", None),
        ("GET", "/v1/export?table=os_tickets", None),
        ("GET", "/v1/feed", None),
        ("POST", "/v1/mcp", Some(json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))),
        ("POST", "/v1/keys/own", Some(json!({"name": "x", "tables": {"os_tickets": "read"}}))),
        ("POST", "/v1/admin/keys", Some(json!({"name": "y"}))),
    ] {
        let (s, j) = call(&http, m, p, body, a).await;
        assert_eq!(s, 403, "{m} {p}: {j}");
    }
    for p in ["/v1/alerts", "/v1/cluster", "/v1/models", "/v1/apps"] {
        assert_eq!(call(&http, "GET", p, None, a).await.0, 403, "{p}");
    }
    // Nor does an app's header narrow it into a key of no table.
    let r = chloedb_server::http_client().build().unwrap().get(format!("{http}/v1/alerts")).bearer_auth(a).header("x-chloedb-as-tables", "*=read").send().await.unwrap();
    assert_eq!(r.status().as_u16(), 403);
    let (s, me) = call(&http, "GET", "/v1/whoami", None, a).await;
    assert_eq!((s, me["tenant"]["value"].as_str()), (200, Some("acme")), "{me}");
    let ch = tonic::transport::Channel::from_shared(grpc).unwrap().connect().await.unwrap();
    let bearer = format!("Bearer {a}");
    let mut gc = pb::strata_client::StrataClient::with_interceptor(ch, move |mut req: tonic::Request<()>| {
        req.metadata_mut().insert("authorization", bearer.parse().unwrap());
        Ok(req)
    });
    let e = gc.schema(pb::Empty {}).await.unwrap_err();
    assert_eq!(e.code(), tonic::Code::PermissionDenied, "{e}");
    assert!(e.message().contains("bound to a tenant"), "{e}");

    // --- An admin checks: nothing crossed.
    let (_, r) = call(&os, "POST", "/tickets/_search", Some(json!({"size": 200})), ROOT).await;
    let docs = hits(&r);
    assert!(docs.iter().all(|h| !h["_id"].as_str().unwrap().starts_with("evil")), "{r}");
    let by = |t: &str| docs.iter().filter(|h| h["_source"]["tenant_id"] == t).count();
    // acme: its own, one written, one by _bulk, one deleted by query.
    assert_eq!((by("acme"), by("globex"), by("initech")), (acme + 1, globex, of(2)), "{r}");
    let g1 = root(&os, "GET", &format!("/tickets/_doc/{g}"), None).await;
    assert_eq!(g1["_source"], ticket(1), "{g1}");
    assert_eq!(root(&os, "GET", "/orders/_count", None).await["count"], 1);
    assert_eq!(call(&os, "HEAD", "/brand-new", None, ROOT).await.0, 404);
    assert!(root(&os, "GET", "/tickets/_mapping", None).await["tickets"]["mappings"]["properties"].get("sla_hours").is_none());
}

/// A key file a server from before tenants keys wrote back: the key lost
/// its tenant, and is a key of no table, which opens nothing; a key that
/// is limited to tables stays refused on the OpenSearch API, as it was.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_tenant_key_without_its_binding_opens_nothing_and_table_keys_stay_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (http, os, _) = start(dir.path()).await;
    root(&os, "PUT", "/tickets", Some(json!({"mappings": {"properties": {"tenant_id": {"type": "keyword"}}}}))).await;
    let (_, made) = call(&http, "POST", "/v1/admin/keys", Some(json!({"name": "tenant-acme", "tenant": {"index": "tickets", "field": "tenant_id", "value": "acme"}})), ROOT).await;
    let secret = made["secret"].as_str().unwrap().to_string();
    assert_eq!(call(&os, "GET", "/tickets/_count", None, &secret).await.0, 200);
    let (_, made) = call(&http, "POST", "/v1/admin/keys", Some(json!({"name": "app", "tables": {"os_tickets": "read"}})), ROOT).await;
    let table_key = made["secret"].as_str().unwrap().to_string();
    let (s, j) = call(&os, "GET", "/tickets/_count", None, &table_key).await;
    assert!(s == 403 && j["error"]["reason"].as_str().unwrap().contains("limited to tables"), "{j}");

    // The file as an older server keeps it: no tenant in the allowance.
    let file = dir.path().join("t.strata.keys.json");
    let mut set: Json = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let k = set["keys"].as_array_mut().unwrap().iter_mut().find(|k| k["name"] == "tenant-acme").unwrap();
    assert_eq!(k["tables"], json!([]), "{k}");
    assert!(k["sha256"].as_str().unwrap().starts_with("t:"), "{k}");
    k["allow"].as_object_mut().unwrap().remove("tenant");
    std::fs::write(dir.path().join("again.json"), set.to_string()).unwrap();
    let keys = Keys::open(dir.path().join("again.json")).unwrap();
    assert!(keys.resolve(None, None, Some(&format!("Bearer {secret}"))).is_none());
}
