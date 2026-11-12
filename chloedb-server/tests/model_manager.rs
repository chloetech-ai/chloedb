//! The command center's model manager (`ui/app/views/models.js`) over
//! HTTP: `/v1/models` says how each model makes its vectors and which
//! fields name it, `/v1/admin/models/convert` writes a static model's table
//! in int8 as a model of its own, and a field changes model as the page
//! changes it: a new field beside the old one, filled in by the backfill,
//! then the two renamed.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value as Json};
use chloedb::db::Db;
use chloedb::storage::DbOptions;
use chloedb::Schema;
use chloedb_server::server;
use chloedb_server::service::{AgentConfig, Backend, Service};
use tokio::net::TcpListener;

fn agent() -> AgentConfig {
    AgentConfig { api_key: None, model: chloedb_server::agent::DEFAULT_MODEL.into(), base_url: "http://unused".into() }
}

/// A Model2Vec model of nine words: animals in the first four dimensions,
/// vehicles in the last four.
fn tiny_static(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    let words = ["[UNK]", "cat", "dog", "fish", "bird", "car", "bus", "truck", "boat"];
    let vocab: serde_json::Map<String, Json> = words.iter().enumerate().map(|(i, w)| (w.to_string(), json!(i))).collect();
    let tok = json!({"version": "1.0", "truncation": null, "padding": null, "added_tokens": [], "normalizer": {"type": "Lowercase"}, "pre_tokenizer": {"type": "Whitespace"}, "post_processor": null, "decoder": null, "model": {"type": "WordLevel", "vocab": vocab, "unk_token": "[UNK]"}});
    std::fs::write(dir.join("tokenizer.json"), tok.to_string()).unwrap();
    std::fs::write(dir.join("config.json"), r#"{"model_type": "model2vec", "hidden_size": 8}"#).unwrap();
    let rows: Vec<f32> = (0..words.len() * 8)
        .map(|i| {
            let (w, d) = (i / 8, i % 8);
            let animal = (1..=4).contains(&w);
            let base = if (d < 4) == animal && w > 0 { 1.0 } else { 0.05 };
            base + ((w * 31 + d * 7) % 13) as f32 / 40.0
        })
        .collect();
    let data: Vec<u8> = rows.iter().flat_map(|x| x.to_le_bytes()).collect();
    let header = json!({"embeddings": {"dtype": "F32", "shape": [words.len(), 8], "data_offsets": [0, data.len()]}}).to_string();
    let mut out = (header.len() as u64).to_le_bytes().to_vec();
    out.extend(header.as_bytes());
    out.extend(data);
    std::fs::write(dir.join("model.safetensors"), out).unwrap();
}

fn client() -> reqwest::Client {
    chloedb_server::http_client().build().unwrap()
}

async fn get(base: &str, path: &str) -> Json {
    client().get(format!("{base}{path}")).send().await.unwrap().json().await.unwrap()
}

async fn post(base: &str, path: &str, body: Json) -> (u16, Json) {
    let r = client().post(format!("{base}{path}")).json(&body).send().await.unwrap();
    (r.status().as_u16(), r.json().await.unwrap_or(Json::Null))
}

async fn sql(base: &str, sql: &str) -> Json {
    let (status, j) = post(base, "/v1/sql", json!({ "sql": sql })).await;
    assert_eq!(status, 200, "{sql}: {j}");
    j
}

/// The keys of the rows nearest `text` by `field`.
async fn nearest(base: &str, field: &str, text: &str) -> Vec<String> {
    let (status, j) = post(base, "/v1/sql", json!({"sql": format!("SELECT id FROM docs ORDER BY similarity({field}, :q) DESC LIMIT 2"), "params": {"q": text}})).await;
    assert_eq!(status, 200, "{j}");
    j["rows"].as_array().unwrap().iter().map(|r| r[0].as_str().unwrap().to_string()).collect()
}

fn model<'a>(list: &'a Json, name: &str) -> Option<&'a Json> {
    list["models"].as_array().unwrap().iter().find(|m| m["name"] == name)
}

async fn until(what: &str, mut f: impl FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send>>) {
    for _ in 0..200 {
        if f().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("{what}: not within 10 s");
}

/// Whether every embedded field of `docs` has its model open and no
/// backfill running.
async fn settled(base: &str) -> bool {
    let t = get(base, "/v1/tables").await;
    let docs = t["tables"].as_array().unwrap().iter().find(|t| t["table"] == "docs").unwrap().clone();
    docs["embeddings"].as_array().unwrap().iter().all(|e| e["loaded"] == true && e["backfill"]["state"] != "running")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_field_changes_model_beside_the_old_one_then_switches_by_renaming() {
    let dir = tempfile::tempdir().unwrap();
    let models = dir.path().join("models");
    tiny_static(&models.join("tiny"));
    let schema = Schema::from_json(r#"{"table": "docs", "key": "id", "fields": [{"name": "id", "type": "keyword"}, {"name": "body", "type": "text"}, {"name": "emb", "type": "vector", "dim": 8, "index": "flat", "embed": {"from": "body", "model": "tiny"}}]}"#).unwrap();
    let db = Db::create(&dir.path().join("docs.strata"), schema, DbOptions { publish_snapshots: true, ..DbOptions::default() }).unwrap();
    let schema = (*db.schema()).clone();
    let mut svc = Service::new(Backend::Local(Arc::new(db)), schema, agent(), 16, Duration::from_secs(10));
    svc.open_models(&models);
    let svc = Arc::new(svc);
    tokio::spawn(svc.clone().keep_models());
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
    tokio::spawn(server::serve_http(svc.clone(), l, Default::default()));
    let docs = [("a", "cat dog"), ("b", "fish bird"), ("c", "car bus"), ("d", "truck boat")];
    svc.write_ops(docs.iter().map(|(id, body)| json!({"op": "upsert", "doc": {"id": id, "body": body}})).collect(), None).await.unwrap();
    assert!(nearest(&base, "emb", "dog").await.contains(&"a".to_string()));

    // The models: how each makes its vectors, and who names it.
    let list = get(&base, "/v1/models").await;
    let tiny = model(&list, "tiny").expect("the model directory is listed");
    assert_eq!((tiny["technique"].as_str(), tiny["precision"].as_str(), tiny["dim"].as_u64()), (Some("static"), Some("F32"), Some(8)), "{tiny}");
    assert_eq!(tiny["fields"], json!([{"table": "docs", "field": "emb"}]));
    assert!(tiny["bytes"].as_u64().unwrap() > 0);
    assert_eq!(model(&list, "potion-retrieval-32M").unwrap()["technique"], "static");
    assert_eq!(model(&list, "potion-retrieval-32M-int8").unwrap()["made_of"], "potion-retrieval-32M");
    assert_eq!(model(&list, "answerai-colbert-small-v1").unwrap()["technique"], "late-interaction");

    // Converting writes a model of its own, in the background.
    let (status, j) = post(&base, "/v1/admin/models/convert", json!({"model": "tiny", "to": "I8"})).await;
    assert_eq!((status, j["model"].as_str()), (200, Some("tiny-int8")), "{j}");
    until("the conversion", || {
        let base = base.clone();
        Box::pin(async move { model(&get(&base, "/v1/models").await, "tiny-int8").is_some_and(|m| m["job"]["state"] == "done" && m["ready"] == true) })
    })
    .await;
    let list = get(&base, "/v1/models").await;
    let int8 = model(&list, "tiny-int8").unwrap();
    assert_eq!((int8["precision"].as_str(), int8["made_of"].as_str(), int8["technique"].as_str()), (Some("I8"), Some("tiny"), Some("static")), "{int8}");
    assert!(int8["bytes"].as_u64() < tiny["bytes"].as_u64());
    // Names only, of models it can make.
    for (path, body, want) in [
        ("/v1/admin/models/convert", json!({"model": "tiny"}), 409),
        ("/v1/admin/models/convert", json!({"model": "tiny-int8"}), 400),
        ("/v1/admin/models/convert", json!({"model": "../models/tiny"}), 400),
        ("/v1/admin/models/convert", json!({"model": "/etc"}), 400),
        ("/v1/admin/models/convert", json!({"model": "tiny", "to": "I4"}), 400),
        ("/v1/admin/models/fetch", json!({"model": "someone/their-model"}), 400),
        ("/v1/admin/models/fetch", json!({"model": "tiny"}), 400),
    ] {
        let (status, j) = post(&base, path, body.clone()).await;
        assert_eq!(status, want, "{path} {body}: {j}");
    }

    // The change: a field beside the old one, filled in for the rows there.
    sql(&base, "ALTER TABLE docs ADD COLUMN emb_next VECTOR(8) OPTIONS(embed = 'body', model = 'tiny-int8', index = 'flat')").await;
    until("the new field's vectors", || {
        let base = base.clone();
        Box::pin(async move { settled(&base).await })
    })
    .await;
    assert_eq!(nearest(&base, "emb_next", "truck").await, nearest(&base, "emb", "truck").await, "int8 ranks as its original");
    assert!(nearest(&base, "emb_next", "bus").await.contains(&"c".to_string()));

    // The switch: two renames; queries of emb now use the int8 model.
    sql(&base, "ALTER TABLE docs RENAME COLUMN emb TO emb_prev, RENAME COLUMN emb_next TO emb").await;
    until("the models of the renamed fields", || {
        let base = base.clone();
        Box::pin(async move {
            let list = get(&base, "/v1/models").await;
            model(&list, "tiny-int8").is_some_and(|m| m["fields"] == json!([{"table": "docs", "field": "emb"}])) && model(&list, "tiny").is_some_and(|m| m["fields"] == json!([{"table": "docs", "field": "emb_prev"}]))
        })
    })
    .await;
    assert!(nearest(&base, "emb", "fish").await.contains(&"b".to_string()));
    // A row written now gets its vector from the model of the field renamed.
    svc.write_ops(vec![json!({"op": "upsert", "doc": {"id": "e", "body": "truck truck truck"}})], None).await.unwrap();
    assert_eq!(nearest(&base, "emb", "truck").await[0], "e");
    // And the old vectors go.
    sql(&base, "ALTER TABLE docs DROP COLUMN emb_prev").await;
    let list = get(&base, "/v1/models").await;
    assert_eq!(model(&list, "tiny").unwrap()["fields"], json!([]));
}
