//! Learned sparse retrieval end to end in ChloeDB: a [`SparseEncoder`]
//! writes the documents' term weights into a SPARSE field (documents a
//! second), each query is encoded (an IDF lookup, no model) and run as SQL,
//! `similarity(sparse, :q)` alone and fused with BM25 by `rrf`, timed
//! together: p50 and p95 of a query, and queries a second on one thread
//! and on every core. With a dense model directory as well (`--dense
//! DIR`, an [`OnnxEmbedder`] or any model `embed::open` opens), the same
//! for its vectors, `similarity(dense, :v)` with the query embedded in the
//! timing. For `research/cpu-encoders`.
//!
//! ```text
//! cargo run --profile ci --features onnx --example sparse_beir -- SPARSE_DIR ~/beir/scifact [--dense DIR]
//! ```
//!
//! [`SparseEncoder`]: chloedb::embed::SparseEncoder
//! [`OnnxEmbedder`]: chloedb::embed::OnnxEmbedder

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use chloedb::db::Db;
use chloedb::embed::{Embedder, SparseEncoder};
use chloedb::query::Params;
use chloedb::storage::DbOptions;
use chloedb::{Field, FieldType, Schema, Value, VectorIndexKind};

struct Run {
    label: &'static str,
    sql: String,
    params: Box<dyn Fn(&str) -> Params + Sync>,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let enc = Arc::new(SparseEncoder::open(Path::new(&args[0])).unwrap_or_else(|e| panic!("{e}")));
    let dir = Path::new(&args[1]);
    let dense: Option<Arc<dyn Embedder>> = args.iter().position(|a| a == "--dense").map(|i| chloedb::embed::open(&args[i + 1], Path::new("/")).unwrap_or_else(|e| panic!("{e}")));
    let c = chloedb::beir::load(dir, "test").unwrap();
    let texts: Vec<&str> = c.docs.iter().map(|(_, t)| t.as_str()).collect();

    let t = Instant::now();
    let sparse = enc.encode_documents(&texts).unwrap();
    let sparse_s = t.elapsed().as_secs_f64();
    let terms = sparse.iter().map(Vec::len).sum::<usize>() as f64 / sparse.len() as f64;
    println!("{} on {}: {:.1} documents/s, {terms:.0} terms a document ({} documents)", enc.name(), dir.display(), texts.len() as f64 / sparse_s, texts.len());
    let vectors = dense.as_ref().map(|m| {
        let t = Instant::now();
        let v = m.embed(&texts).unwrap();
        println!("{} on {}: {:.1} documents/s", m.name(), dir.display(), texts.len() as f64 / t.elapsed().as_secs_f64());
        v
    });

    let mut fields = vec![Field::new("id", FieldType::Keyword), Field::new("body", FieldType::Text { positions: false }), Field::new("sparse", FieldType::Sparse)];
    if let Some(m) = &dense {
        fields.push(Field::new("dense", FieldType::Vector { dim: m.dim(), index: VectorIndexKind::Flat }));
    }
    let schema = Schema::new("docs", fields, Some("id")).unwrap();
    let tmp = std::env::temp_dir().join(format!("chloedb-sparse-beir-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let db = Db::create(&tmp.join("e.strata"), schema, DbOptions { sync: false, ..DbOptions::default() }).unwrap();
    db.load(c.docs.iter().enumerate().map(|(i, (id, text))| {
        let mut row = vec![Value::Str(id.clone()), Value::Str(text.clone()), Value::Str(SparseEncoder::to_json(&sparse[i]))];
        if let Some(v) = &vectors {
            row.push(Value::Vector(v[i].clone().into()));
        }
        row
    }))
    .unwrap();

    let bm25 = "bm25(body, :t, syntax => 'plain')";
    let e1 = enc.clone();
    let e2 = enc.clone();
    let mut runs = vec![
        Run { label: "bm25", sql: bm25.into(), params: Box::new(|q| Params::new().set("t", Value::Str(q.into()))) },
        Run { label: "learned sparse", sql: "similarity(sparse, :q)".into(), params: Box::new(move |q| Params::new().set("q", Value::Str(SparseEncoder::to_json(&e1.encode_query(q).unwrap())))) },
        Run {
            label: "rrf(bm25, learned sparse)",
            sql: format!("rrf({bm25}, similarity(sparse, :q))"),
            params: Box::new(move |q| Params::new().set("t", Value::Str(q.into())).set("q", Value::Str(SparseEncoder::to_json(&e2.encode_query(q).unwrap())))),
        },
    ];
    if let Some(m) = dense.clone() {
        runs.push(Run { label: "dense", sql: "similarity(dense, :v)".into(), params: Box::new(move |q| Params::new().set("v", Value::Vector(m.embed_query(q).unwrap().into()))) });
    }

    let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
    for run in &runs {
        let sql = format!("SELECT id FROM docs ORDER BY {} DESC LIMIT 101", run.sql);
        let (mut ndcg, mut times) = (Vec::new(), Vec::new());
        let t0 = Instant::now();
        for (qid, q) in &c.queries {
            let t = Instant::now();
            let r = db.query(&sql, &(run.params)(q)).unwrap_or_else(|e| panic!("{sql}: {e}"));
            times.push(t.elapsed().as_secs_f64() * 1e3);
            let ranked: Vec<String> = r.rows.iter().filter_map(|row| row[0].as_str().map(String::from)).filter(|d| d != qid).take(100).collect();
            ndcg.push(chloedb::metrics::ndcg_at_k(&ranked, &c.qrels[qid], 10));
        }
        let qps1 = c.queries.len() as f64 / t0.elapsed().as_secs_f64();
        // every core at once, each thread a share of the queries
        let t0 = Instant::now();
        std::thread::scope(|s| {
            for part in c.queries.chunks(c.queries.len().div_ceil(threads)) {
                let (db, sql, run) = (&db, &sql, run);
                s.spawn(move || {
                    for (_, q) in part {
                        db.query(sql, &(run.params)(q)).unwrap();
                    }
                });
            }
        });
        let qps = c.queries.len() as f64 / t0.elapsed().as_secs_f64();
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "  {:<28} nDCG@10 {:.4}   p50 {:.2} ms  p95 {:.2} ms   {qps1:.0} queries/s on 1 thread, {qps:.0} on {threads}",
            run.label,
            chloedb::metrics::mean(&ndcg),
            times[times.len() / 2],
            times[times.len() * 95 / 100]
        );
    }
    let _ = std::fs::remove_dir_all(&tmp);
}
