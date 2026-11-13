//! Classifiers (`CREATE CLASSIFIER`, [`chloedb::query::classify`])
//! measured on Wayfair's WANDS: 42,994 products, each with a product
//! class, and 480 queries, each with the class it asks for.
//!
//! ```text
//! git clone https://github.com/wayfair/WANDS && mv WANDS/dataset ~/wands
//! chloedb-classify-eval ~/wands --out results/text/classify_wands.json
//! ```
//!
//! A product's `emb` is potion-retrieval-32M's vector of its name. Each
//! method trains on the products with a class (a fifth held out, by a hash
//! of the vector) and is measured three ways:
//!
//! - **products**: how often the class of a held-out product is the one
//!   the classifier gives it;
//! - **queries**: how often the class a query asks for is the one the
//!   classifier gives the query's text (the queries split by id, even ones
//!   `dev`, odd ones `test`), and how often it is among the best three;
//! - **search**: nDCG@10 of `bm25(body, :q)` and of the same ranking with
//!   a boost for the products of the query's class,
//!   `bm25(body, :q) * (1 + w * (class = topic(:v)))`, `:v` the query's
//!   vector, so the call is computed before planning.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use chloedb::db::{Db, SqlResult};
use chloedb::metrics::{mean, ndcg_at_k, paired_randomization, percentile};
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
    /// Methods to train (comma-separated).
    #[arg(long, default_value = "centroid,logistic")]
    methods: String,
    /// Boost weights to measure (comma-separated).
    #[arg(long, default_value = "0.1,0.25,0.5")]
    weights: String,
    /// More options of CREATE CLASSIFIER, as written in OPTIONS (...).
    #[arg(long, default_value = "")]
    options: String,
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
    let s = |v: &str| if v.trim().is_empty() { Value::Null } else { Value::Str(v.trim().to_string()) };
    let mut rows = Vec::new();
    for rec in tsv(&args.dir.join("product.csv")).records() {
        let rec = rec.unwrap();
        let g = |i: usize| rec.get(i).unwrap_or("");
        let classes: Vec<String> = g(2).split('|').map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).collect();
        rows.push(vec![
            Value::Str(g(0).to_string()),
            Value::Str(g(1).to_string()),
            Value::Str(format!("{} . {}", g(1), g(4))),
            if classes.is_empty() { Value::Null } else { Value::List(classes.iter().cloned().map(Value::Str).collect()) },
            classes.first().map_or(Value::Null, |c| s(c)),
            Value::Null,
        ]);
    }
    let schema = Schema::new(
        "products",
        vec![
            Field::new("id", FieldType::Keyword),
            Field::new("name", FieldType::Text { positions: false }),
            Field::new("body", FieldType::Text { positions: false }),
            Field::new("class", FieldType::Keyword).multi(),
            Field::new("label", FieldType::Keyword),
            Field::new("emb", FieldType::Vector { dim, index: VectorIndexKind::Flat }).embedded(&args.model, &["name"]),
        ],
        Some("id"),
    )
    .unwrap();
    let tmp = std::env::temp_dir().join(format!("chloedb-classify-eval-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let db = Db::create(&tmp.join("products.strata"), schema, DbOptions { sync: false, ..DbOptions::default() }).unwrap();
    db.set_embedder("emb", embedder.clone()).unwrap();
    let n_products = rows.len();
    let t = Instant::now();
    db.load(rows).unwrap();
    db.optimize().unwrap();
    let load_s = t.elapsed().as_secs_f64();
    eprintln!("loaded and embedded {n_products} products in {load_s:.1}s");

    let mut queries: Vec<(u32, String, String)> = Vec::new();
    for rec in tsv(&args.dir.join("query.csv")).records() {
        let rec = rec.unwrap();
        queries.push((rec[0].parse().unwrap(), rec[1].to_string(), rec.get(2).unwrap_or("").trim().to_string()));
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
    let judged: Vec<&(u32, String, String)> = queries.iter().filter(|q| qrels.get(&q.0).is_some_and(|j| j.values().any(|&g| g > 0))).collect();
    let qvec: HashMap<u32, Value> = judged.iter().map(|q| (q.0, db.embed_query_in("products", "emb", &q.1).unwrap())).collect();
    let ndcg_of = |sql: &str, extra: &dyn Fn(Params) -> Params| -> Vec<f64> {
        judged
            .iter()
            .map(|q| {
                let p = extra(Params::new().set("q", Value::Str(q.1.clone())).set("v", qvec[&q.0].clone()));
                let r = db.query(sql, &p).unwrap_or_else(|e| panic!("{sql}: {e}"));
                let ranked: Vec<String> = r.rows.iter().filter_map(|row| row[0].as_str().map(String::from)).collect();
                ndcg_at_k(&ranked, &qrels[&q.0], 10)
            })
            .collect()
    };
    let split = |xs: &[f64]| -> (f64, f64, f64) {
        let (mut dev, mut test) = (Vec::new(), Vec::new());
        for (q, x) in judged.iter().zip(xs) {
            if q.0 % 2 == 0 { dev.push(*x) } else { test.push(*x) }
        }
        (mean(xs), mean(&dev), mean(&test))
    };
    let plain = ndcg_of("SELECT id FROM products WHERE MATCH(body, :q) ORDER BY bm25(body, :q) DESC LIMIT 10", &|p| p);
    let (pa, pd, pt) = split(&plain);
    println!("{:>28}: nDCG@10 {pa:.4} (dev {pd:.4}, test {pt:.4})", "bm25");
    let mut results = serde_json::Map::new();
    results.insert("bm25".into(), json!({ "ndcg@10": pa, "dev": pd, "test": pt }));

    for method in args.methods.split(',').map(str::trim).filter(|m| !m.is_empty()) {
        let name = format!("topic_{method}");
        let extra = if args.options.is_empty() { String::new() } else { format!(", {}", args.options) };
        let sql = format!("CREATE OR REPLACE CLASSIFIER {name} ON products(emb) LABELS label OPTIONS (method = '{method}'{extra})");
        let t = Instant::now();
        let SqlResult::Rows(report) = db.sql(&sql, &Params::new()).unwrap_or_else(|e| panic!("{sql}: {e}")) else { panic!("a report") };
        let train_s = t.elapsed().as_secs_f64();
        let col = |c: &str| report.columns.iter().position(|x| x == c).map(|i| report.rows[0][i].clone()).unwrap_or(Value::Null);
        let f = |v: Value| v.as_f64().unwrap_or(f64::NAN);
        let labels = f(col("labels")) as usize;
        let holdout = f(col("holdout_accuracy"));
        println!("{name:>28}: {labels} labels, trained in {train_s:.1}s, held-out products {:.2}% right, train {:.2}%", holdout * 100.0, f(col("train_accuracy")) * 100.0);
        // Queries.
        let (mut top1, mut top3, mut known, mut lat) = (Vec::new(), Vec::new(), 0usize, Vec::new());
        for q in &queries {
            let t = Instant::now();
            let c = db.classify(&name, &q.1, 3).unwrap();
            lat.push(t.elapsed().as_secs_f64() * 1e3);
            if c.top.len() == 3 && db.query("SELECT COUNT(*) FROM products WHERE label = :c", &Params::new().set("c", Value::Str(q.2.clone()))).ok().and_then(|r| r.rows.first().and_then(|r| r[0].as_i64())).unwrap_or(0) > 0 {
                known += 1;
            }
            top1.push((c.label.as_deref() == Some(q.2.as_str())) as u8 as f64);
            top3.push(c.top.iter().any(|(l, _)| *l == q.2) as u8 as f64);
        }
        let by = |xs: &[f64]| -> (f64, f64) {
            let (mut dev, mut test) = (Vec::new(), Vec::new());
            for (q, x) in queries.iter().zip(xs) {
                if q.0 % 2 == 0 { dev.push(*x) } else { test.push(*x) }
            }
            (mean(&dev), mean(&test))
        };
        let ((d1, t1), (_, t3)) = (by(&top1), by(&top3));
        lat.sort_by(f64::total_cmp);
        println!(
            "{:>28}  queries: top-1 {:.1}% (dev {:.1}%, test {:.1}%), top-3 {:.1}% (test {:.1}%), {known} of {} asking for a class products have; p50 {:.3} ms a text",
            "",
            mean(&top1) * 100.0,
            d1 * 100.0,
            t1 * 100.0,
            mean(&top3) * 100.0,
            t3 * 100.0,
            queries.len(),
            percentile(&lat, 50.0)
        );
        // Rows: a column of the rows a query returns.
        let t = Instant::now();
        let r = db.query(&format!("SELECT id, {name}(emb) FROM products LIMIT 10000"), &Params::new()).unwrap();
        let rows_ms = t.elapsed().as_secs_f64() * 1e3;
        let agree = r.rows.iter().filter(|row| row[1] != Value::Null).count();
        println!("{:>28}  rows: {} classified in {rows_ms:.0} ms ({agree} with a label)", "", r.rows.len());
        let mut boosts = serde_json::Map::new();
        for w in args.weights.split(',').map(str::trim).filter(|w| !w.is_empty()) {
            let sql = format!("SELECT id FROM products WHERE MATCH(body, :q) ORDER BY bm25(body, :q) * (1 + {w} * CASE WHEN class = {name}(:v) THEN 1 ELSE 0 END) DESC LIMIT 10");
            let t = Instant::now();
            let xs = ndcg_of(&sql, &|p| p);
            let per_q = t.elapsed().as_secs_f64() * 1e3 / judged.len() as f64;
            let (a, d, te) = split(&xs);
            let p = paired_randomization(&plain, &xs, 10_000, 7);
            println!("{:>28}  boost {w}: nDCG@10 {a:.4} (dev {d:.4}, test {te:.4}), p = {p:.3} against bm25, {per_q:.2} ms a query", "");
            boosts.insert(w.to_string(), json!({ "ndcg@10": a, "dev": d, "test": te, "p_vs_bm25": p, "ms_per_query": per_q }));
        }
        results.insert(
            name.clone(),
            json!({
                "method": method,
                "labels": labels,
                "train_s": train_s,
                "holdout_products_accuracy": holdout,
                "train_accuracy": f(col("train_accuracy")),
                "queries_top1": mean(&top1), "queries_top1_dev": d1, "queries_top1_test": t1,
                "queries_top3": mean(&top3), "queries_top3_test": t3,
                "queries_with_a_class_products_have": known,
                "classify_text_p50_ms": percentile(&lat, 50.0),
                "classify_10000_rows_ms": rows_ms,
                "boost": boosts,
            }),
        );
    }
    let _ = std::fs::remove_dir_all(&tmp);
    if let Some(out) = &args.out {
        let doc = json!({
            "collection": "WANDS",
            "products": n_products,
            "queries": queries.len(),
            "judged_queries": judged.len(),
            "model": args.model,
            "embedded_s": load_s,
            "machine": machine(),
            "results": results,
        });
        std::fs::write(out, serde_json::to_string_pretty(&doc).unwrap()).unwrap();
        eprintln!("wrote {}", out.display());
    }
}
