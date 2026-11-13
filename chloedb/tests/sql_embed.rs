//! Text where a query wants a vector ([`chloedb::query::embed_sql`]):
//! `similarity(emb, 'words')` by the field's own model, `embed('model',
//! text)` by a model named, and a static model with its table in int8
//! ranking as its F32 original does.

use chloedb::db::{Db, SqlResult};
use chloedb::embed::{Embedder, HashEmbedder};
use chloedb::query::Params;
use chloedb::schema::{Field, FieldType, Schema};
use chloedb::storage::DbOptions;
use chloedb::value::Value;

fn placeholder() -> Schema {
    Schema::new("first", vec![Field::new("id", FieldType::Keyword)], Some("id")).unwrap()
}

fn run(db: &Db, sql: &str, p: &Params) -> SqlResult {
    db.sql(sql, p).unwrap_or_else(|e| panic!("{sql}: {e}"))
}

fn ids(db: &Db, sql: &str, p: &Params) -> Vec<String> {
    match run(db, sql, p) {
        SqlResult::Rows(r) => r.rows.into_iter().map(|r| r[0].as_str().unwrap_or_default().to_string()).collect(),
        SqlResult::Written(_) => panic!("{sql}: expected rows"),
    }
}

fn fails(db: &Db, sql: &str, p: &Params) -> String {
    match db.sql(sql, p) {
        Ok(_) => panic!("{sql}: expected an error"),
        Err(e) => e.to_string(),
    }
}

fn notes(dir: &std::path::Path, model: &str, dim: usize) -> Db {
    let models = dir.join("models");
    std::fs::create_dir_all(&models).unwrap();
    let db = Db::create(&dir.join("e.strata"), placeholder(), DbOptions { sync: false, ..DbOptions::default() }).unwrap();
    assert!(db.open_embedders(&models).is_empty());
    run(&db, &format!("CREATE TABLE notes (id KEYWORD PRIMARY KEY, body TEXT, year INT, emb VECTOR({dim}) OPTIONS(embed = 'body', model = '{model}', index = 'flat'), raw VECTOR({dim}) OPTIONS(index = 'flat'))"), &Params::new());
    let rows = [("n1", "graph walks over citations", 2020), ("n2", "vector search with binary codes", 2021), ("n3", "random walks on large graphs", 2022), ("n4", "codes for vector quantization", 2023)];
    for (id, body, year) in rows {
        let raw = db.embed_query_in("notes", "emb", body).unwrap();
        run(&db, "INSERT INTO notes (id, body, year, raw) VALUES (:id, :body, :year, :raw)", &Params::new().set("id", Value::Str(id.into())).set("body", Value::Str(body.into())).set("year", Value::Int(year)).set("raw", raw));
    }
    db
}

#[test]
fn similarity_to_text_embeds_it_with_the_fields_model() {
    let dir = tempfile::tempdir().unwrap();
    let db = notes(dir.path(), "hash:64", 64);
    let none = Params::new();
    // The text in the query, and the vector the caller would have sent.
    let by_text = ids(&db, "SELECT id FROM notes ORDER BY similarity(emb, 'vector codes') DESC LIMIT 2", &none);
    let v = Value::Vector(HashEmbedder { dim: 64 }.embed_query("vector codes").unwrap().into());
    let by_vector = ids(&db, "SELECT id FROM notes ORDER BY similarity(emb, :v) DESC LIMIT 2", &Params::new().set("v", v));
    assert_eq!(by_text, by_vector);
    assert!(by_text.contains(&"n2".to_string()) && by_text.contains(&"n4".to_string()), "{by_text:?}");
    // A parameter of text, the same text in WHERE and ORDER BY, an alias,
    // embed(text) written out, distance, and fused with BM25.
    let q = Params::new().set("q", Value::Str("graph walks".into()));
    assert_eq!(ids(&db, "SELECT id FROM notes n WHERE year >= 2021 ORDER BY similarity(n.emb, :q) DESC LIMIT 1", &q), ["n3"]);
    assert_eq!(ids(&db, "SELECT id FROM notes ORDER BY similarity(emb, embed(:q)) DESC LIMIT 1", &q), ["n1"]);
    assert_eq!(ids(&db, "SELECT id FROM notes ORDER BY distance(emb, 'graph walks') ASC LIMIT 1", &none), ["n1"]);
    let fused = ids(&db, "SELECT id FROM notes ORDER BY rrf(bm25(body, :q), similarity(emb, :q)) DESC LIMIT 2", &q);
    assert_eq!(fused.len(), 2);
    // A vector written as text is a vector still.
    let json = serde_json::to_string(&HashEmbedder { dim: 64 }.embed_query("vector codes").unwrap()).unwrap();
    assert_eq!(ids(&db, "SELECT id FROM notes ORDER BY similarity(emb, :v) DESC LIMIT 2", &Params::new().set("v", Value::Str(json))), by_vector);
    // EXPLAIN shows the plan the query runs.
    match run(&db, "EXPLAIN SELECT id FROM notes ORDER BY similarity(emb, 'vector codes') DESC LIMIT 2", &none) {
        SqlResult::Rows(r) => assert!(!r.rows.is_empty()),
        SqlResult::Written(_) => unreachable!(),
    }
    // A field of no model takes a vector, or text by a model named.
    let e = fails(&db, "SELECT id FROM notes ORDER BY similarity(raw, 'vector codes') DESC LIMIT 2", &none);
    assert!(e.contains("is text"), "{e}");
    assert_eq!(ids(&db, "SELECT id FROM notes ORDER BY similarity(raw, embed('hash:64', 'vector codes')) DESC LIMIT 2", &none), by_vector);
    let e = fails(&db, "SELECT id FROM notes ORDER BY similarity(raw, embed('vector codes')) DESC LIMIT 2", &none);
    assert!(e.contains("embed('model', ..)"), "{e}");
    // A column is embedded at write time, not once a row in a query.
    let e = fails(&db, "SELECT id FROM notes ORDER BY similarity(raw, embed('hash:64', body)) DESC LIMIT 2", &none);
    assert!(e.contains("embed = 'column'"), "{e}");
    // A model that is not there says how to get it.
    let e = fails(&db, "SELECT id FROM notes ORDER BY similarity(raw, embed('potion-base-8M', 'x')) DESC LIMIT 2", &none);
    assert!(e.contains("fetch-model potion-base-8M"), "{e}");
    // A path is not a model's name.
    let e = fails(&db, "SELECT id FROM notes ORDER BY similarity(raw, embed('/etc', 'x')) DESC LIMIT 2", &none);
    assert!(e.contains("named by its directory"), "{e}");
}

#[test]
fn embed_gives_a_vector_in_a_projection() {
    let dir = tempfile::tempdir().unwrap();
    let db = notes(dir.path(), "hash:16", 16);
    let want = Value::Vector(HashEmbedder { dim: 16 }.embed_query("binary codes").unwrap().into());
    match run(&db, "SELECT id, embed('hash:16', 'binary codes') AS v FROM notes WHERE id = 'n1'", &Params::new()) {
        SqlResult::Rows(r) => assert_eq!(r.rows[0][1], want),
        SqlResult::Written(_) => unreachable!(),
    }
}

/// A static model of a few words, F32, as Model2Vec writes one.
fn tiny_static(dir: &std::path::Path) {
    std::fs::create_dir_all(dir).unwrap();
    let words = ["[UNK]", "cat", "dog", "fish", "bird", "car", "bus", "truck", "boat"];
    let vocab: serde_json::Map<String, serde_json::Value> = words.iter().enumerate().map(|(i, w)| (w.to_string(), serde_json::json!(i))).collect();
    let tok = serde_json::json!({"version": "1.0", "truncation": null, "padding": null, "added_tokens": [], "normalizer": {"type": "Lowercase"}, "pre_tokenizer": {"type": "Whitespace"}, "post_processor": null, "decoder": null, "model": {"type": "WordLevel", "vocab": vocab, "unk_token": "[UNK]"}});
    std::fs::write(dir.join("tokenizer.json"), tok.to_string()).unwrap();
    std::fs::write(dir.join("config.json"), r#"{"model_type": "model2vec", "hidden_size": 8}"#).unwrap();
    // animals in the first dimensions, vehicles in the last, and a little of each
    let rows: Vec<f32> = (0..words.len() * 8).map(|i| {
        let (w, d) = (i / 8, i % 8);
        let animal = (1..=4).contains(&w);
        let base = if (d < 4) == animal && w > 0 { 1.0 } else { 0.05 };
        base + ((w * 31 + d * 7) % 13) as f32 / 40.0
    }).collect();
    let data: Vec<u8> = rows.iter().flat_map(|x| x.to_le_bytes()).collect();
    let header = serde_json::json!({"embeddings": {"dtype": "F32", "shape": [words.len(), 8], "data_offsets": [0, data.len()]}}).to_string();
    let mut out = (header.len() as u64).to_le_bytes().to_vec();
    out.extend(header.as_bytes());
    out.extend(data);
    std::fs::write(dir.join("model.safetensors"), out).unwrap();
}

#[test]
fn an_int8_static_model_ranks_as_its_original() {
    let dir = tempfile::tempdir().unwrap();
    let models = dir.path().join("models");
    tiny_static(&models.join("tiny"));
    let q = chloedb::embed::quantize_static(&models.join("tiny"), &models.join("tiny-int8"), "I8").unwrap();
    assert!(q.bytes_after * 2 < q.bytes_before, "{q:?}");
    let mut ranked = Vec::new();
    for model in ["tiny", "tiny-int8"] {
        let sub = dir.path().join(model);
        std::fs::create_dir_all(&sub).unwrap();
        let db = Db::create(&sub.join("e.strata"), placeholder(), DbOptions { sync: false, ..DbOptions::default() }).unwrap();
        db.open_embedders(&models);
        run(&db, &format!("CREATE TABLE pets (id KEYWORD PRIMARY KEY, body TEXT, emb VECTOR(8) OPTIONS(embed = 'body', model = '{model}', index = 'flat'))"), &Params::new());
        for (id, body) in [("a", "cat and dog"), ("b", "car bus truck"), ("c", "fish bird"), ("d", "boat and car"), ("e", "dog dog bird")] {
            run(&db, "INSERT INTO pets (id, body) VALUES (:id, :body)", &Params::new().set("id", Value::Str(id.into())).set("body", Value::Str(body.into())));
        }
        let pets = ids(&db, "SELECT id FROM pets ORDER BY similarity(emb, 'a dog and a cat') DESC LIMIT 5", &Params::new());
        assert_eq!(pets.len(), 5);
        assert!(pets[..3].iter().all(|p| ["a", "c", "e"].contains(&p.as_str())), "{model}: {pets:?}");
        ranked.push(pets);
        // embed() names the model by its name under the models directory.
        let named = ids(&db, &format!("SELECT id FROM pets ORDER BY similarity(emb, embed('{model}', 'truck')) DESC LIMIT 2"), &Params::new());
        assert!(named.iter().all(|p| ["b", "d"].contains(&p.as_str())), "{model}: {named:?}");
    }
    assert_eq!(ranked[0], ranked[1]);
}

#[test]
fn embed_of_no_text_and_hash_out_of_range_are_errors_not_panics() {
    let dir = tempfile::tempdir().unwrap();
    let db = notes(dir.path(), "hash:64", 64);
    let none = Params::new();
    // embed(*) and a named argument: one argument, no expression.
    for sql in ["SELECT id FROM notes ORDER BY similarity(emb, embed(*)) DESC LIMIT 2", "SELECT id FROM notes ORDER BY similarity(emb, embed(q => 'codes')) DESC LIMIT 2", "SELECT id FROM notes ORDER BY similarity(raw, embed(*)) DESC LIMIT 2"] {
        let e = fails(&db, sql, &none);
        assert!(e.contains("embed(..) takes text"), "{sql}: {e}");
    }
    // hash:N with N of 0, past the vector limit, or past usize.
    for n in ["0", "65537", "99999999999", "999999999999999999999999"] {
        let sql = format!("SELECT embed('hash:{n}', 'x') AS v FROM notes LIMIT 1");
        let e = fails(&db, &sql, &none);
        assert!(e.contains("hash:N") || e.contains("hash:<dimensions>"), "{sql}: {e}");
    }
    assert!(chloedb::embed::open("hash:0", dir.path()).is_err());
    assert!(chloedb::embed::open("hash:65537", dir.path()).is_err());
    assert!(chloedb::embed::open(&format!("hash:{}", chloedb::schema::MAX_VECTOR_DIM), dir.path()).is_ok());
    // The largest is still served.
    run(&db, "SELECT embed('hash:65536', 'x') AS v FROM notes LIMIT 1", &none);
    // A new field has the same limit.
    let e = fails(&db, "CREATE TABLE big (id KEYWORD PRIMARY KEY, v VECTOR(65537))", &none);
    assert!(e.contains("from 1 to 65536"), "{e}");
}
