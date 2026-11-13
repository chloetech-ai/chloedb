//! The answer cache ([`chloedb_server::answers`]): a query asked again on
//! the same version of the data is answered from it, its steps saying so;
//! a write, other parameters or a profiled query run it again; and a
//! coordinator's shards answer its repeated query from theirs.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value as Json};
use chloedb::db::Db;
use chloedb::storage::DbOptions;
use chloedb_server::answers::AnswerCache;
use chloedb_server::coord::Coordinator;
use chloedb_server::service::{shard_of, AgentConfig, Backend, Service};
use chloedb_server::{demo, server};
use tokio::net::TcpListener;

const N: usize = 2000;
const BM25: &str = "SELECT id FROM products WHERE price > :p ORDER BY bm25(body, 'laptop phone') DESC LIMIT 5";
const COUNT: &str = "SELECT COUNT(*) FROM products";

fn open(dir: &std::path::Path, name: &str, keep: impl Fn(usize) -> bool) -> Arc<Db> {
    let opts = DbOptions { publish_snapshots: true, segment_max_docs: 400, ..DbOptions::default() };
    let db = Db::create(&dir.join(name), demo::schema(N + 10), opts).unwrap();
    db.set_embedder("emb", Arc::new(chloedb::embed::HashEmbedder { dim: demo::DIM })).unwrap();
    db.load((0..N).filter(|&i| keep(i)).map(|i| demo::product(i, 7))).unwrap();
    Arc::new(db)
}

fn service(backend: Backend, schema: chloedb::Schema, cache: bool) -> Arc<Service> {
    let agent = AgentConfig { api_key: None, model: String::new(), base_url: String::new() };
    let mut svc = Service::new(backend, schema, agent, 16, Duration::from_secs(60));
    svc.answers = if cache { AnswerCache::new(8).map(Arc::new) } else { None };
    Arc::new(svc)
}

async fn listen() -> (TcpListener, String) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
    (l, url)
}

async fn query(base: &str, body: Json) -> Json {
    let r = chloedb_server::http_client().build().unwrap().post(format!("{base}/v1/query")).json(&body).send().await.unwrap();
    assert_eq!(r.status().as_u16(), 200);
    r.json().await.unwrap()
}

fn cached(j: &Json) -> usize {
    j["stats"]["steps"].as_array().into_iter().flatten().filter(|s| s["op"] == "answer cache").count()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_repeated_query_is_answered_from_the_cache_until_a_write() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path(), "one.strata", |_| true);
    let schema = (*db.schema()).clone();
    let svc = service(Backend::Local(db.clone()), schema, true);
    let (l, base) = listen().await;
    tokio::spawn(server::serve_http(svc.clone(), l, Default::default()));

    let ask = |p: f64| json!({"sql": BM25, "params": {"p": p}, "steps": true});
    let first = query(&base, ask(5.0)).await;
    assert_eq!(cached(&first), 0, "{first}");
    let again = query(&base, ask(5.0)).await;
    assert_eq!(cached(&again), 1, "{again}");
    assert_eq!((&again["rows"], &again["scores"]), (&first["rows"], &first["scores"]));
    // Other parameters are another query.
    assert_eq!(cached(&query(&base, ask(6.0)).await), 0);
    // A profiled query is measured, not looked up.
    assert_eq!(cached(&query(&base, json!({"sql": BM25, "params": {"p": 5.0}, "profile": true})).await), 0);

    let n = query(&base, json!({"sql": COUNT, "steps": true})).await;
    assert_eq!(cached(&query(&base, json!({"sql": COUNT, "steps": true})).await), 1);
    // A write moves the version on: the next answer is computed again,
    // and counts the new row.
    db.load([demo::product(N, 7)]).unwrap();
    let after = query(&base, json!({"sql": COUNT, "steps": true})).await;
    assert_eq!(cached(&after), 0, "{after}");
    assert_eq!(after["rows"][0][0].as_i64().unwrap(), n["rows"][0][0].as_i64().unwrap() + 1);
    let c = svc.answers.as_ref().unwrap();
    assert_eq!(c.hits.load(std::sync::atomic::Ordering::Relaxed), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_coordinator_s_repeated_query_is_answered_from_its_shards_caches() {
    let dir = tempfile::tempdir().unwrap();
    let shard = |s: usize| move |i: usize| shard_of(&chloedb::Value::Str(format!("p{i:07}")), 2) == s;
    let mut shards = Vec::new();
    for s in 0..2 {
        let db = open(dir.path(), &format!("s{s}.strata"), shard(s));
        let schema = (*db.schema()).clone();
        let (l, url) = listen().await;
        tokio::spawn(server::serve_grpc(service(Backend::Local(db), schema, true), l, Default::default()));
        shards.push(url);
    }
    let c = Coordinator::connect(shards).await.unwrap();
    let schema = c.schema.clone();
    let (l, base) = listen().await;
    tokio::spawn(server::serve_http(service(Backend::Cluster(Arc::new(c)), schema, false), l, Default::default()));

    let ask = json!({"sql": BM25, "params": {"p": 5.0}, "steps": true});
    let first = query(&base, ask.clone()).await;
    let mut again = query(&base, ask.clone()).await;
    // The coordinator's statistics round may keep the first answers'
    // statistics: by the third, both shards answer from their caches.
    if cached(&again) < 2 {
        again = query(&base, ask).await;
    }
    assert_eq!(cached(&again), 2, "{again}");
    assert_eq!((&again["rows"], &again["scores"]), (&first["rows"], &first["scores"]));
}
