//! Learned sparse retrieval run by ChloeDB's inverted index: documents'
//! term weights from a transformer (and from the static expansion) in
//! SPARSE fields, queries' weights from the model's IDF table, ranked by
//! `similarity(field, :q)` with the text index's top-k code, next to BM25
//! on the same documents. Reads what `research/cpu-encoders/sparse.py
//! --export DIR` writes.
//!
//! ```text
//! cargo run --profile ci --example sparse_eval -- DIR scifact nfcorpus arguana
//! ```

use std::collections::HashMap;
use std::io::BufRead;
use std::time::Instant;

use chloedb::db::Db;
use chloedb::query::Params;
use chloedb::storage::DbOptions;
use chloedb::{Field, FieldType, Schema, Value};

fn lines(path: &std::path::Path) -> Vec<serde_json::Value> {
    let f = std::fs::File::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    std::io::BufReader::new(f).lines().map(|l| serde_json::from_str(&l.unwrap()).unwrap()).collect()
}

fn qrels(beir: &std::path::Path, ds: &str) -> HashMap<String, HashMap<String, u32>> {
    let mut out: HashMap<String, HashMap<String, u32>> = HashMap::new();
    let text = std::fs::read_to_string(beir.join(ds).join("qrels/test.tsv")).unwrap();
    for l in text.lines().skip(1) {
        let p: Vec<&str> = l.split('\t').collect();
        out.entry(p[0].into()).or_default().insert(p[1].into(), p[2].parse().unwrap());
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = std::path::PathBuf::from(&args[0]);
    let beir = std::env::var_os("CHLOEDB_BEIR_DIR").map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from(std::env::var("HOME").unwrap()).join("beir"));
    for ds in &args[1..] {
        let docs = lines(&dir.join(format!("{ds}-docs.jsonl")));
        let queries = lines(&dir.join(format!("{ds}-queries.jsonl")));
        let judged = qrels(&beir, ds);
        let schema = Schema::new(
            "docs",
            vec![
                Field::new("id", FieldType::Keyword),
                Field::new("body", FieldType::Text { positions: false }),
                Field::new("sparse", FieldType::Sparse),
                Field::new("static", FieldType::Sparse),
            ],
            Some("id"),
        )
        .unwrap();
        let tmp = std::env::temp_dir().join(format!("chloedb-sparse-eval-{}-{ds}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let db = Db::create(&tmp.join("e.strata"), schema, DbOptions { sync: false, ..DbOptions::default() }).unwrap();
        let t0 = Instant::now();
        db.load(docs.iter().map(|d| {
            vec![Value::Str(d["id"].as_str().unwrap().into()), Value::Str(d["text"].as_str().unwrap().into()), Value::Str(d["sparse"].to_string()), Value::Str(d["static"].to_string())]
        }))
        .unwrap();
        let load_s = t0.elapsed().as_secs_f64();
        println!("{ds}: {} documents loaded and indexed in {load_s:.1} s, {} queries", docs.len(), queries.len());
        let runs = [
            ("bm25", "bm25(body, :t, syntax => 'plain')"),
            ("learned sparse (transformer docs, IDF queries)", "similarity(sparse, :q)"),
            ("static sparse expansion (lookup docs, IDF queries)", "similarity(static, :q)"),
            ("rrf(bm25, learned sparse)", "rrf(bm25(body, :t, syntax => 'plain'), similarity(sparse, :q))"),
        ];
        for (label, order) in runs {
            let sql = format!("SELECT id FROM docs ORDER BY {order} DESC LIMIT 101");
            let (mut ndcg, mut times) = (Vec::new(), Vec::new());
            for q in &queries {
                let qid = q["id"].as_str().unwrap();
                let p = Params::new().set("t", Value::Str(q["text"].as_str().unwrap().into())).set("q", Value::Str(q["sparse"].to_string()));
                let t = Instant::now();
                let r = db.query(&sql, &p).unwrap_or_else(|e| panic!("{sql}: {e}"));
                times.push(t.elapsed().as_secs_f64() * 1e3);
                let ranked: Vec<String> = r.rows.iter().filter_map(|row| row[0].as_str().map(String::from)).filter(|d| d != qid).take(100).collect();
                ndcg.push(chloedb::metrics::ndcg_at_k(&ranked, &judged[qid], 10));
            }
            times.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let p50 = times[times.len() / 2];
            let p95 = times[times.len() * 95 / 100];
            println!("  {label:<52} nDCG@10 {:.4}   p50 {p50:.2} ms  p95 {p95:.2} ms", chloedb::metrics::mean(&ndcg));
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
