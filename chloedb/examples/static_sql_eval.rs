//! nDCG@10 of a model on a BEIR collection through SQL that embeds the
//! query text itself (`ORDER BY similarity(emb, :q) DESC` with `:q` the
//! query's text, [`chloedb::query::embed_sql`]), exact (flat) search, the
//! document itself left out of its own results as BEIR's evaluation does
//! (ArguAna's queries are documents of its corpus). For comparing a static
//! model with its int8 copy (`chloedb-server quantize-model`).
//!
//! ```text
//! chloedb/tests/fixtures/beir/fetch.sh ~/beir
//! cargo run --profile ci --example static_sql_eval -- ~/beir/scifact ~/models potion-retrieval-32M-int8
//! ```

use std::time::Instant;
use chloedb::db::{Db, SqlResult};
use chloedb::query::Params;
use chloedb::schema::{Field, FieldType, Schema};
use chloedb::storage::DbOptions;
use chloedb::value::Value;
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (beir, models, model) = (std::path::Path::new(&a[0]), std::path::Path::new(&a[1]), &a[2]);
    let c = chloedb::beir::load(beir, "test").unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let first = Schema::new("first", vec![Field::new("id", FieldType::Keyword)], Some("id")).unwrap();
    let db = Db::create(&tmp.path().join("e.strata"), first, DbOptions { sync: false, ..DbOptions::default() }).unwrap();
    db.open_embedders(models);
    db.sql(&format!("CREATE TABLE docs (id KEYWORD PRIMARY KEY, body TEXT, emb VECTOR OPTIONS(embed = 'body', model = '{model}', index = 'flat'))"), &Params::new()).unwrap();
    let t = Instant::now();
    for chunk in c.docs.chunks(2000) {
        db.load_into("docs", chunk.iter().map(|(id, b)| vec![Value::Str(id.clone()), Value::Str(b.clone()), Value::Null])).unwrap();
    }
    let load = t.elapsed().as_secs_f64();
    let mut sum = 0.0;
    let mut times = Vec::new();
    for (qid, q) in &c.queries {
        let t = Instant::now();
        let SqlResult::Rows(r) = db.sql("SELECT id FROM docs ORDER BY similarity(emb, :q) DESC LIMIT 11", &Params::new().set("q", Value::Str(q.clone()))).unwrap() else { panic!() };
        times.push(t.elapsed().as_secs_f64() * 1e3);
        let ranked: Vec<String> = r.rows.iter().map(|r| r[0].as_str().unwrap().to_string()).filter(|d| d != qid).take(10).collect();
        sum += chloedb::metrics::ndcg_at_k(&ranked, &c.qrels[qid], 10);
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!("{model} {}: nDCG@10 {:.4}, {} docs loaded in {load:.1}s, query p50 {:.2} ms ({} queries)", beir.display(), sum / c.queries.len() as f64, c.docs.len(), times[times.len() / 2], c.queries.len());
}
