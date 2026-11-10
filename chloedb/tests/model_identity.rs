//! A model is more than its name: a table records the fingerprint of the
//! weights its vectors came from and refuses a model of the same name whose
//! weights differ, tables from before fingerprints keep opening by name,
//! and a write that needs a vector its field's model cannot make (the model
//! is not open) fails unless the caller asked to embed later.

use std::sync::Arc;

use chloedb::db::Db;
use chloedb::embed::HashEmbedder;
use chloedb::query::Params;
use chloedb::schema::{Field, FieldType, Schema};
use chloedb::storage::DbOptions;
use chloedb::value::Value;
use chloedb::VectorIndexKind;

fn schema(model: &str) -> Schema {
    Schema::new(
        "docs",
        vec![
            Field::new("id", FieldType::Keyword),
            Field::new("title", FieldType::Text { positions: false }),
            Field::new("emb", FieldType::Vector { dim: 8, index: VectorIndexKind::Flat }).not_stored().embedded(model, &["title"]),
        ],
        Some("id"),
    )
    .unwrap()
}

fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{}", chloedb::model_pins::sha256_hex(bytes))
}

fn fingerprint_of(db: &Db) -> Option<String> {
    db.schema().field("emb").unwrap().embed.as_ref().unwrap().fingerprint.clone()
}

#[test]
fn a_table_records_its_models_weights_and_refuses_others_of_the_same_name() {
    let dir = tempfile::tempdir().unwrap();
    let models = dir.path().join("models");
    std::fs::create_dir_all(models.join("mine")).unwrap();
    let weights = models.join("mine").join("model.safetensors");
    std::fs::write(&weights, b"weights one").unwrap();

    let path = dir.path().join("f.strata");
    let db = Db::create_in(&path, schema("mine"), DbOptions::default(), &models).unwrap();
    assert_eq!(fingerprint_of(&db), Some(sha256(b"weights one")));
    // The same weights pass the check (they are no model, so they fail to
    // open after it).
    let failed = db.open_embedders(&models);
    assert_eq!(failed.len(), 1);
    assert!(!failed[0].1.to_string().contains("is not the one"), "{}", failed[0].1);
    drop(db);

    // Recorded in the file: a reopened database still knows it.
    let db = Db::open(&path, DbOptions::default()).unwrap();
    assert_eq!(fingerprint_of(&db), Some(sha256(b"weights one")));
    // Before: other weights under the same name opened, and wrote vectors
    // of another space beside the rows'.
    std::fs::write(&weights, b"weights two, changed upstream").unwrap();
    let failed = db.open_embedders(&models);
    assert_eq!(failed.len(), 1);
    let e = failed[0].1.to_string();
    assert!(e.contains("is not the one field emb's vectors came from") && e.contains(&sha256(b"weights one")), "{e}");
    assert!(db.embedder("emb").is_none());
    drop(db);

    // A table without a fingerprint (from before them, or whose model was
    // not there) opens its model by name, as before.
    let other = dir.path().join("models-elsewhere");
    let db = Db::create_in(&dir.path().join("g.strata"), schema("mine"), DbOptions::default(), &other).unwrap();
    assert_eq!(fingerprint_of(&db), None);
    let failed = db.open_embedders(&models);
    assert!(!failed[0].1.to_string().contains("is not the one"));
    // Hash models have none.
    let db = Db::create_in(&dir.path().join("h.strata"), schema("hash:8"), DbOptions::default(), &models).unwrap();
    assert_eq!(fingerprint_of(&db), None);
}

#[test]
fn a_write_without_the_fields_model_fails_unless_asked_to_embed_later() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::create(&dir.path().join("w.strata"), schema("hash:8"), DbOptions::default()).unwrap();
    let p = Params::new();
    // Before: the row was stored without a vector, and no error said so.
    let e = db.sql("INSERT INTO docs (id, title) VALUES ('a', 'red mug')", &p).unwrap_err().to_string();
    assert!(e.contains("not open") && e.contains("set_embed_later"), "{e}");
    let e = db.load([vec![Value::Str("a".into()), Value::Str("red mug".into()), Value::Null]]).unwrap_err().to_string();
    assert!(e.contains("not open"), "{e}");
    assert_eq!(db.query("SELECT COUNT(*) FROM docs", &p).unwrap().rows[0][0], Value::Int(0));
    // A row that brings its vector, or has no text, needs no model.
    let v = Params::new().set("v", Value::Vector(Arc::from(vec![1.0f32, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0])));
    db.sql("INSERT INTO docs (id, title, emb) VALUES ('b', 'blue mug', :v)", &v).unwrap();
    db.sql("INSERT INTO docs (id) VALUES ('c')", &p).unwrap();
    // On purpose: stored without a vector, and filled in later.
    db.set_embed_later(true);
    db.sql("INSERT INTO docs (id, title) VALUES ('a', 'red mug')", &p).unwrap();
    let missing = db.missing_vectors("docs", "emb").unwrap();
    assert!(missing.contains(&Value::Str("a".into())) && !missing.contains(&Value::Str("b".into())), "{missing:?}");
    db.set_embed_later(false);
    db.set_embedder("emb", Arc::new(HashEmbedder { dim: 8 })).unwrap();
    db.sql("INSERT INTO docs (id, title) VALUES ('d', 'green mug')", &p).unwrap();
    assert!(db.embed_backfill("docs", "emb", 16).unwrap() >= 1);
    assert!(!db.missing_vectors("docs", "emb").unwrap().contains(&Value::Str("a".into())));
}
