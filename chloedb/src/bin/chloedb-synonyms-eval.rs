//! Mined synonyms ([`chloedb::query::mine`]) measured on Wayfair's WANDS:
//! 42,994 products and 480 queries with graded judgments.
//!
//! ```text
//! git clone https://github.com/wayfair/WANDS && mv WANDS/dataset ~/wands
//! chloedb-synonyms-eval ~/wands --out results/text/synonyms_wands.json
//! ```
//!
//! The words of the products' names are mined by their vectors
//! (potion-retrieval-32M's, through the table's vector field), at each
//! cosine threshold asked for; each list becomes the table's synonym list
//! (`CREATE OR REPLACE SYNONYMS ON products`), and nDCG@10 of
//! `bm25(body, :q, synonyms => 1)` is measured against plain
//! `bm25(body, :q)`, the queries split by id (even ones `dev`, odd ones
//! `test`) so that a threshold chosen on one half is judged on the other.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use chloedb::db::Db;
use chloedb::metrics::{mean, ndcg_at_k, paired_randomization};
use chloedb::query::mine::{Options, Request, Source};
use chloedb::query::Params;
use chloedb::schema::{Field, FieldType, Schema, VectorIndexKind};
use chloedb::storage::DbOptions;
use chloedb::value::Value;
use clap::Parser;
use serde_json::{json, Value as Json};

#[derive(Parser)]
struct Args {
    /// The WANDS dataset folder (product.csv, query.csv, label.csv).
    dir: PathBuf,
    /// Where to write the results (JSON).
    #[arg(long)]
    out: Option<PathBuf>,
    /// The embedding model.
    #[arg(long, default_value = "potion-retrieval-32M")]
    model: String,
    /// Cosine thresholds to mine at (comma-separated).
    #[arg(long, default_value = "0.4,0.5,0.6,0.7,0.8")]
    min_cos: String,
    /// Words embedded at most.
    #[arg(long, default_value_t = 3000)]
    max_words: usize,
    /// Rules kept for one word at most.
    #[arg(long, default_value_t = 3)]
    per_word: usize,
}

fn machine() -> Json {
    let cpu = std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with("model name")).and_then(|l| l.split(':').nth(1)).map(|m| m.trim().to_string()));
    json!({ "cpu": cpu, "cpus": std::thread::available_parallelism().map_or(0, |n| n.get()), "os": std::env::consts::OS, "arch": std::env::consts::ARCH })
}

fn tsv(path: &std::path::Path) -> csv::Reader<std::fs::File> {
    csv::ReaderBuilder::new().delimiter(b'\t').quoting(false).flexible(true).from_path(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn main() {
    let args = Args::parse();
    let models = chloedb::embed::models_dir();
    let embedder = chloedb::embed::open(&args.model, &models).unwrap_or_else(|e| panic!("{e}"));
    let dim = embedder.dim();
    let mut rows = Vec::new();
    for rec in tsv(&args.dir.join("product.csv")).records() {
        let rec = rec.unwrap();
        let g = |i: usize| rec.get(i).unwrap_or("");
        rows.push(vec![Value::Str(g(0).to_string()), Value::Str(g(1).to_string()), Value::Str(format!("{} . {}", g(1), g(4))), Value::Null]);
    }
    let schema = Schema::new(
        "products",
        vec![
            Field::new("id", FieldType::Keyword),
            Field::new("name", FieldType::Text { positions: false }),
            Field::new("body", FieldType::Text { positions: false }),
            Field::new("emb", FieldType::Vector { dim, index: VectorIndexKind::Flat }).embedded(&args.model, &["name"]),
        ],
        Some("id"),
    )
    .unwrap();
    let tmp = std::env::temp_dir().join(format!("chloedb-synonyms-eval-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let db = Db::create(&tmp.join("products.strata"), schema, DbOptions { sync: false, ..DbOptions::default() }).unwrap();
    db.set_embedder("emb", embedder.clone()).unwrap();
    let n_products = rows.len();
    let t = Instant::now();
    db.load(rows).unwrap();
    db.optimize().unwrap();
    eprintln!("loaded and embedded {n_products} products in {:.1}s", t.elapsed().as_secs_f64());

    let mut queries: Vec<(u32, String)> = Vec::new();
    for rec in tsv(&args.dir.join("query.csv")).records() {
        let rec = rec.unwrap();
        queries.push((rec[0].parse().unwrap(), rec[1].to_string()));
    }
    let mut qrels: HashMap<u32, HashMap<String, u32>> = HashMap::new();
    for rec in tsv(&args.dir.join("label.csv")).records() {
        let rec = rec.unwrap();
        let g = match &rec[3] {
            "Exact" => 2,
            "Partial" => 1,
            _ => 0,
        };
        qrels.entry(rec[1].parse().unwrap()).or_default().insert(rec[2].to_string(), g);
    }
    let judged: Vec<&(u32, String)> = queries.iter().filter(|q| qrels.get(&q.0).is_some_and(|j| j.values().any(|&g| g > 0))).collect();
    let ndcg_of = |sql: &str| -> (Vec<f64>, f64) {
        let t = Instant::now();
        let xs = judged
            .iter()
            .map(|q| {
                let r = db.query(sql, &Params::new().set("q", Value::Str(q.1.clone()))).unwrap_or_else(|e| panic!("{sql}: {e}"));
                let ranked: Vec<String> = r.rows.iter().filter_map(|row| row[0].as_str().map(String::from)).collect();
                ndcg_at_k(&ranked, &qrels[&q.0], 10)
            })
            .collect();
        (xs, t.elapsed().as_secs_f64() * 1e3 / judged.len() as f64)
    };
    let split = |xs: &[f64]| -> (f64, f64, f64) {
        let (mut dev, mut test) = (Vec::new(), Vec::new());
        for (q, x) in judged.iter().zip(xs) {
            if q.0 % 2 == 0 { dev.push(*x) } else { test.push(*x) }
        }
        (mean(xs), mean(&dev), mean(&test))
    };
    let (plain, plain_ms) = ndcg_of("SELECT id FROM products WHERE MATCH(body, :q) ORDER BY bm25(body, :q) DESC LIMIT 10");
    let (pa, pd, pt) = split(&plain);
    println!("{:>22}: nDCG@10 {pa:.4} (dev {pd:.4}, test {pt:.4}), {plain_ms:.2} ms a query", "bm25");
    let mut results = vec![json!({ "run": "bm25", "ndcg@10": pa, "dev": pd, "test": pt, "ms_per_query": plain_ms })];
    for c in args.min_cos.split(',').map(str::trim).filter(|c| !c.is_empty()) {
        let min_cos: f32 = c.parse().unwrap_or_else(|e| panic!("{c}: {e}"));
        let req = Request {
            table: "products".into(),
            field: "name".into(),
            source: Source::Vectors { vector: "emb".into() },
            options: Options { min_cos, max_words: args.max_words, per_word: args.per_word, ..Options::default() },
        };
        let m = db.mine_synonyms(&req).unwrap();
        if m.proposals.is_empty() {
            println!("{:>22}: no rules from {} words", format!("mined, cosine >= {c}"), m.words);
            results.push(json!({ "run": format!("mined_cos_{c}"), "min_cos": min_cos, "rules": 0, "words": m.words }));
            continue;
        }
        db.sql(&format!("CREATE OR REPLACE SYNONYMS ON products AS '{}'", m.rules.replace('\'', "''")), &Params::new()).unwrap();
        let (xs, ms) = ndcg_of("SELECT id FROM products WHERE MATCH(body, :q, synonyms => 1) ORDER BY bm25(body, :q, synonyms => 1) DESC LIMIT 10");
        let (a, d, te) = split(&xs);
        let p = paired_randomization(&plain, &xs, 10_000, 7);
        let (better, worse) = (plain.iter().zip(&xs).filter(|(p, x)| x > p).count(), plain.iter().zip(&xs).filter(|(p, x)| x < p).count());
        let sample: Vec<&str> = m.proposals.iter().map(|p| p.rule.as_str()).take(20).collect();
        println!(
            "{:>22}: {} rules from {} words in {:.1}s; nDCG@10 {a:.4} (dev {d:.4}, test {te:.4}), p = {p:.3} against bm25, {better} queries better and {worse} worse, {ms:.2} ms a query",
            format!("mined, cosine >= {c}"),
            m.proposals.len(),
            m.words,
            m.took_ms / 1e3
        );
        println!("{:>22}  closest: {}", "", sample.join(" | "));
        results.push(json!({
            "run": format!("mined_cos_{c}"), "min_cos": min_cos, "rules": m.proposals.len(), "words": m.words, "mined_s": m.took_ms / 1e3,
            "ndcg@10": a, "dev": d, "test": te, "p_vs_bm25": p, "queries_better": better, "queries_worse": worse, "ms_per_query": ms,
            "closest_rules": sample,
        }));
    }
    let _ = std::fs::remove_dir_all(&tmp);
    if let Some(out) = &args.out {
        let doc = json!({
            "collection": "WANDS",
            "products": n_products,
            "judged_queries": judged.len(),
            "model": args.model,
            "max_words": args.max_words,
            "per_word": args.per_word,
            "machine": machine(),
            "results": results,
        });
        std::fs::write(out, serde_json::to_string_pretty(&doc).unwrap()).unwrap();
        eprintln!("wrote {}", out.display());
    }
}
