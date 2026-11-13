//! A SPARSE field embedded by a learned sparse model (`embed`, `model`):
//! writes that leave it out get their text's term weights from the model
//! ([`Embedder::embed_sparse`]), an UPDATE of the text weighs it again, a
//! query's text gets its weights from the same model
//! ([`Db::embed_query_in`]), and `similarity(field, :q)` ranks by them,
//! alone or fused with BM25. Rows written while the model was not open
//! are filled in once it opens ([`Db::embed_backfill`]). A model of
//! vectors is refused for a SPARSE field, and a sparse model for a VECTOR
//! field. The model here is a stand-in that expands `car` to
//! `automobile`, as a learned sparse model expands a document's terms.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use chloedb::db::Db;
use chloedb::embed::{Embedder, HashEmbedder};
use chloedb::query::Params;
use chloedb::schema::{Field, FieldType, Schema};
use chloedb::storage::DbOptions;
use chloedb::value::Value;
use chloedb::VectorIndexKind;

/// Words weighted by their count; a document that says `car` also gets
/// `automobile` at half weight. A query's words weigh 1 and are not
/// expanded, as an inference-free model's are not.
struct Expanding(AtomicUsize);

impl Embedder for Expanding {
    fn name(&self) -> String {
        "expanding".into()
    }
    fn dim(&self) -> usize {
        0
    }
    fn embed(&self, _texts: &[&str]) -> chloedb::Result<Vec<Vec<f32>>> {
        Err(chloedb::Error::Schema("term weights only".into()))
    }
    fn parallel(&self) -> bool {
        false
    }
    fn sparse(&self) -> bool {
        true
    }
    fn embed_sparse(&self, texts: &[&str]) -> chloedb::Result<Vec<Vec<(String, f32)>>> {
        self.0.fetch_add(texts.len(), Ordering::Relaxed);
        Ok(texts
            .iter()
            .map(|t| {
                let mut w: Vec<(String, f32)> = Vec::new();
                for word in t.split_whitespace().map(str::to_lowercase) {
                    match w.iter_mut().find(|(x, _)| *x == word) {
                        Some((_, n)) => *n += 1.0,
                        None => w.push((word.clone(), 1.0)),
                    }
                    if word == "car" && !w.iter().any(|(x, _)| x == "automobile") {
                        w.push(("automobile".into(), 0.5));
                    }
                }
                w
            })
            .collect())
    }
    fn embed_query_sparse(&self, text: &str) -> chloedb::Result<Vec<(String, f32)>> {
        Ok(text.split_whitespace().map(|w| (w.to_lowercase(), 1.0)).collect())
    }
}

/// id, body, sp (body's term weights).
fn schema() -> Schema {
    Schema::new(
        "docs",
        vec![Field::new("id", FieldType::Keyword), Field::new("body", FieldType::Text { positions: false }), Field::new("sp", FieldType::Sparse).embedded("expanding", &["body"])],
        Some("id"),
    )
    .unwrap()
}

fn doc(id: &str, body: &str) -> Vec<Value> {
    vec![Value::Str(id.into()), Value::Str(body.into()), Value::Null]
}

fn terms(db: &Db, id: &str) -> Option<serde_json::Value> {
    let r = db.query("SELECT sp FROM docs WHERE id = :k", &Params::new().set("k", Value::Str(id.into()))).unwrap();
    r.rows.first().and_then(|row| row[0].as_str().map(|s| serde_json::from_str(s).unwrap()))
}

fn top(db: &Db, order: &str, text: &str) -> Vec<String> {
    let p = Params::new().set("q", db.embed_query_in("docs", "sp", text).unwrap()).set("t", Value::Str(text.into()));
    let r = db.query(&format!("SELECT id FROM docs ORDER BY {order} DESC LIMIT 3"), &p).unwrap();
    r.rows.iter().map(|row| row[0].as_str().unwrap().to_string()).collect()
}

#[test]
fn writes_get_their_terms_from_the_model_and_queries_rank_by_them() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::create(&dir.path().join("s.strata"), schema(), DbOptions::default()).unwrap();
    let m = Arc::new(Expanding(AtomicUsize::new(0)));
    db.set_embedder("sp", m.clone()).unwrap();
    db.load([doc("a", "a red car for sale"), doc("b", "the train to town"), doc("c", "red red apples")]).unwrap();
    assert_eq!(m.0.load(Ordering::Relaxed), 3);
    let a = terms(&db, "a").unwrap();
    assert_eq!(a["car"], 1.0);
    assert_eq!(a["automobile"], 0.5);
    assert_eq!(terms(&db, "c").unwrap()["red"], 2.0);

    // The expansion finds a document BM25 cannot.
    assert_eq!(top(&db, "similarity(sp, :q)", "automobile"), ["a"]);
    assert!(top(&db, "bm25(body, :t, syntax => 'plain')", "automobile").is_empty());
    let mut fused = top(&db, "rrf(bm25(body, :t, syntax => 'plain'), similarity(sp, :q))", "red automobile");
    fused.sort();
    assert_eq!(fused, ["a", "c"]);
    assert_eq!(db.embed_query_in("docs", "sp", "Red car").unwrap(), Value::Str(r#"{"car":1.0,"red":1.0}"#.into()));

    // SQL writes are weighed too, and a text changed is weighed again.
    db.sql("INSERT INTO docs (id, body) VALUES ('d', 'an old car')", &Params::new()).unwrap();
    assert_eq!(top(&db, "similarity(sp, :q)", "old automobile")[0], "d");
    db.sql("UPDATE docs SET body = 'a car and a train' WHERE id = 'b'", &Params::new()).unwrap();
    assert_eq!(terms(&db, "b").unwrap()["automobile"], 0.5);
    // Terms a writer gives are kept.
    db.load([vec![Value::Str("e".into()), Value::Str("nothing here".into()), Value::Str(r#"{"zebra": 3}"#.into())]]).unwrap();
    assert_eq!(terms(&db, "e").unwrap(), serde_json::json!({"zebra": 3}));

    // The database says it needs a version of ChloeDB that knows such fields.
    let features = chloedb::storage::manifest::read(&dir.path().join("s.strata")).unwrap().features();
    assert!(features.contains(&"sparse_embed".to_string()), "{features:?}");

    // Text where the terms go says how to give it.
    let e = db.query("SELECT id FROM docs ORDER BY similarity(sp, 'red car') DESC", &Params::new()).unwrap_err().to_string();
    assert!(e.contains("expanding's term weights of body") && e.contains("\"embed\""), "{e}");
}

#[test]
fn rows_written_before_the_model_opens_get_their_terms_from_a_backfill() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::create(&dir.path().join("s.strata"), schema(), DbOptions::default()).unwrap();
    db.load((0..50).map(|i| doc(&format!("d{i:02}"), &format!("car number {i}")))).unwrap();
    db.storage().flush().unwrap();
    db.load([doc("x", ""), doc("y", "a bus")]).unwrap();
    assert!(terms(&db, "d00").is_none());
    assert_eq!(db.missing_vectors("docs", "sp").unwrap().len(), 51, "the row without text needs none");
    db.set_embedder("sp", Arc::new(Expanding(AtomicUsize::new(0)))).unwrap();
    assert_eq!(db.embed_backfill("docs", "sp", 16).unwrap(), 51);
    assert!(db.missing_vectors("docs", "sp").unwrap().is_empty());
    assert_eq!(terms(&db, "d07").unwrap()["7"], 1.0);
    assert_eq!(top(&db, "similarity(sp, :q)", "bus"), ["y"]);
}

#[test]
fn a_model_of_vectors_and_a_sparse_model_each_go_to_their_own_field() {
    let dir = tempfile::tempdir().unwrap();
    let mut fields = schema().fields;
    fields.push(Field::new("emb", FieldType::Vector { dim: 8, index: VectorIndexKind::Flat }).embedded("hash:8", &["body"]));
    let s = Schema::new("docs", fields, Some("id")).unwrap();
    let db = Db::create(&dir.path().join("s.strata"), s, DbOptions::default()).unwrap();
    let e = db.set_embedder("sp", Arc::new(HashEmbedder { dim: 8 })).unwrap_err().to_string();
    assert!(e.contains("needs a learned sparse model"), "{e}");
    let e = db.set_embedder("emb", Arc::new(Expanding(AtomicUsize::new(0)))).unwrap_err().to_string();
    assert!(e.contains("is a learned sparse model"), "{e}");
    db.set_embedder("sp", Arc::new(Expanding(AtomicUsize::new(0)))).unwrap();
    db.set_embedder("emb", Arc::new(HashEmbedder { dim: 8 })).unwrap();
    db.load([vec![Value::Str("a".into()), Value::Str("a car".into()), Value::Null, Value::Null]]).unwrap();
    assert!(terms(&db, "a").is_some());
    let r = db.query("SELECT emb FROM docs WHERE id = 'a'", &Params::new()).unwrap();
    assert_eq!(r.rows[0][0].as_vector().map(|v| v.len()), Some(8));
}

#[test]
fn the_schema_keeps_a_sparse_fields_model() {
    let s = schema();
    let back = Schema::from_json_value(&s.to_short_json()).unwrap();
    assert_eq!(back.field("sp").unwrap().embed, s.field("sp").unwrap().embed);
    // A SPARSE field weighs text, one set of terms a row.
    let media = Schema::new(
        "t",
        vec![Field::new("id", FieldType::Keyword), Field::media("img", chloedb::media::MediaKind::Image), Field::new("sp", FieldType::Sparse).embedded("m", &["img"])],
        Some("id"),
    );
    assert!(media.unwrap_err().to_string().contains("weighs the terms of text"));
}

/// A learned sparse model from a models directory, by name, as a table
/// names it: `CHLOEDB_SPARSE_MODEL` is its directory (one
/// `research/cpu-encoders/sparse_onnx_export.py` writes); skipped without.
#[cfg(feature = "onnx")]
#[test]
fn a_learned_sparse_model_opens_from_the_models_directory() {
    let Some(model) = std::env::var_os("CHLOEDB_SPARSE_MODEL").map(std::path::PathBuf::from) else {
        eprintln!("skipped: CHLOEDB_SPARSE_MODEL is not set");
        return;
    };
    let (models, name) = (model.parent().unwrap(), model.file_name().unwrap().to_str().unwrap());
    let s = Schema::new(
        "docs",
        vec![Field::new("id", FieldType::Keyword), Field::new("body", FieldType::Text { positions: false }), Field::new("sp", FieldType::Sparse).embedded(name, &["body"])],
        Some("id"),
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let db = Db::create(&dir.path().join("s.strata"), s, DbOptions::default()).unwrap();
    assert!(db.open_embedders(models).is_empty());
    assert!(db.embedder("sp").unwrap().sparse());
    db.load([doc("a", "Aspirin lowers the risk of a heart attack."), doc("b", "The train leaves the station at noon."), doc("c", "Protein folding in neurons.")]).unwrap();
    // The document side expands: a word the text does not say has weight.
    let a = terms(&db, "a").unwrap();
    assert!(a["heart"].as_f64().unwrap() > 0.0 && a.as_object().unwrap().len() > 8, "{a}");
    assert_eq!(top(&db, "similarity(sp, :q)", "cardiac drug")[0], "a");
    assert_eq!(top(&db, "similarity(sp, :q)", "railway timetable")[0], "b");
}

/// A database without an embedded SPARSE field declares nothing new: a
/// plain SPARSE field, and embedded VECTOR fields, need what they needed
/// before, so versions from before `sparse_embed` open it.
#[test]
fn only_an_embedded_sparse_field_needs_the_new_feature() {
    let dir = tempfile::tempdir().unwrap();
    let s = Schema::new(
        "docs",
        vec![
            Field::new("id", FieldType::Keyword),
            Field::new("body", FieldType::Text { positions: false }),
            Field::new("sp", FieldType::Sparse),
            Field::new("emb", FieldType::Vector { dim: 8, index: VectorIndexKind::Flat }).embedded("hash:8", &["body"]),
        ],
        Some("id"),
    )
    .unwrap();
    let db = Db::create(&dir.path().join("p.strata"), s, DbOptions::default()).unwrap();
    db.set_embedder("emb", Arc::new(HashEmbedder { dim: 8 })).unwrap();
    db.load([vec![Value::Str("a".into()), Value::Str("a car".into()), Value::Str(r#"{"car": 1}"#.into()), Value::Null]]).unwrap();
    db.storage().flush().unwrap();
    let features = chloedb::storage::manifest::read(&dir.path().join("p.strata")).unwrap().features();
    assert!(!features.iter().any(|f| f == "sparse_embed"), "{features:?}");
}
