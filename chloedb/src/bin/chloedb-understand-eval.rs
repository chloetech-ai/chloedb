//! Query understanding (`bm25(.., understand => ..)`) measured on two
//! product searches:
//!
//! - Wayfair's **WANDS**: 42,994 products, 480 queries, 233,448 judgements
//!   (Exact 2, Partial 1, Irrelevant 0), each query with the product class
//!   it asks for. A product's `body` is its name and description; its
//!   class, category path, colour, style, material, shape and product type
//!   are `KEYWORD` fields, from its row and its features.
//! - Amazon's **ESCI** (Shopping Queries), its US test queries of the small
//!   version: 8,956 queries and the 164,900 products judged for them (Exact
//!   100, Substitute 10, Complement 1, Irrelevant 0: the paper's gains
//!   1, 0.1, 0.01, scaled). `body` is a product's title, bullet points and
//!   description; brand and colour are `KEYWORD` fields.
//!
//! ```text
//! git clone https://github.com/wayfair/WANDS && mv WANDS/dataset ~/wands
//! chloedb-understand-eval ~/wands --out results/text/understand_wands.json
//! # ESCI's parquet files, as TSV of the same shape (product.csv, query.csv, label.csv)
//! chloedb-understand-eval ~/esci/us --collection esci --out results/text/understand_esci.json
//! ```
//!
//! Each variant is the same `ORDER BY bm25(body, :q, ...)` with other
//! options. The queries split by id, even ones to choose settings on
//! (`dev`) and odd ones to report (`test`). Also measured on WANDS: how
//! often the classes a query's words name hold the class WANDS says it asks
//! for.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use chloedb::db::Db;
use chloedb::metrics::{mean, ndcg_at_k, paired_randomization, percentile};
use chloedb::query::Params;
use chloedb::schema::{Field, FieldType, Schema};
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
    /// Variants to run (comma-separated); all by default.
    #[arg(long)]
    variants: Option<String>,
    /// `wands` or `esci`.
    #[arg(long, default_value = "wands")]
    collection: String,
}

fn variants(esci: bool) -> Vec<(&'static str, String)> {
    // ESCI's brands are more than a table's own choice reads: listed.
    let fields = if esci { ", understand_fields => 'brand, color'" } else { "" };
    let mut v = vec![("plain", String::new())];
    v.push(("boost", format!(", understand => 'boost'{fields}")));
    for (name, o) in [("boost_weight_0.5", "understand_weight => 0.5"), ("boost_weight_1", "understand_weight => 1"), ("boost_share_0", "understand_share => 0"), ("boost_share_0.1", "understand_share => 0.1"), ("boost_share_0.3", "understand_share => 0.3")] {
        v.push((name, format!(", understand => 'boost'{fields}, {o}")));
    }
    v.push(("filter", format!(", understand => 'filter'{fields}")));
    if esci {
        v.push(("boost_brand", ", understand => 'boost', understand_fields => 'brand'".into()));
        v.push(("filter_brand", ", understand => 'filter', understand_fields => 'brand'".into()));
    } else {
        v.push(("boost_class", ", understand => 'boost', understand_fields => 'class'".into()));
        v.push(("filter_class", ", understand => 'filter', understand_fields => 'class'".into()));
    }
    v
}

fn split_values(v: &str) -> Vec<String> {
    v.split([',', '/', '&', ';']).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
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
    let only: Option<Vec<String>> = args.variants.as_ref().map(|v| v.split(',').map(|s| s.trim().to_string()).collect());
    let esci = args.collection == "esci";
    let s = |v: &str| if v.trim().is_empty() { Value::Null } else { Value::Str(v.trim().to_string()) };
    let list = |vs: Vec<String>| if vs.is_empty() { Value::Null } else { Value::List(vs.into_iter().map(Value::Str).collect()) };
    let mut rows = Vec::new();
    let mut r = tsv(&args.dir.join("product.csv"));
    let schema = if esci {
        for rec in r.records() {
            let rec = rec.unwrap();
            let g = |i: usize| rec.get(i).unwrap_or("");
            rows.push(vec![Value::Str(g(0).to_string()), Value::Str(format!("{} . {} . {}", g(1), g(3), g(2))), s(g(4)), list(split_values(g(5)))]);
        }
        Schema::new(
            "products",
            vec![Field::new("id", FieldType::Keyword), Field::new("body", FieldType::Text { positions: false }), Field::new("brand", FieldType::Keyword), Field::new("color", FieldType::Keyword).multi()],
            Some("id"),
        )
    } else {
        for rec in r.records() {
            let rec = rec.unwrap();
            let g = |i: usize| rec.get(i).unwrap_or("");
            let mut feats: HashMap<&str, &str> = HashMap::new();
            for kv in g(5).split('|') {
                if let Some((k, v)) = kv.split_once(':') {
                    feats.entry(k.trim()).or_insert(v.trim());
                }
            }
            let f = |k: &str| feats.get(k).copied().unwrap_or("");
            let category: Vec<String> = g(3).split('/').map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).collect();
            rows.push(vec![
                Value::Str(g(0).to_string()),
                Value::Str(format!("{} . {}", g(1), g(4))),
                list(g(2).split('|').map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).collect()),
                list(category),
                list(split_values(f("color"))),
                s(f("dsprimaryproductstyle")),
                s(f("primarymaterial")),
                s(f("shape")),
                s(f("producttype")),
                g(7).trim().parse::<f64>().map(Value::Float).unwrap_or(Value::Null),
            ]);
        }
        Schema::new(
            "products",
            vec![
                Field::new("id", FieldType::Keyword),
                Field::new("body", FieldType::Text { positions: false }),
                Field::new("class", FieldType::Keyword).multi(),
                Field::new("category", FieldType::Keyword).multi(),
                Field::new("color", FieldType::Keyword).multi(),
                Field::new("style", FieldType::Keyword),
                Field::new("material", FieldType::Keyword),
                Field::new("shape", FieldType::Keyword),
                Field::new("producttype", FieldType::Keyword),
                Field::new("rating", FieldType::Float),
            ],
            Some("id"),
        )
    }
    .unwrap();
    let tmp = std::env::temp_dir().join(format!("chloedb-understand-eval-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let db = Db::create(&tmp.join("products.strata"), schema, DbOptions::default()).unwrap();
    let n_products = rows.len();
    let t = Instant::now();
    db.load(rows).unwrap();
    db.optimize().unwrap();
    eprintln!("loaded {n_products} products in {:.1}s", t.elapsed().as_secs_f64());

    let mut queries: Vec<(u32, String, String)> = Vec::new();
    for rec in tsv(&args.dir.join("query.csv")).records() {
        let rec = rec.unwrap();
        queries.push((rec[0].parse().unwrap(), rec[1].to_string(), rec.get(2).unwrap_or("").to_string()));
    }
    let mut qrels: HashMap<u32, HashMap<String, u32>> = HashMap::new();
    for rec in tsv(&args.dir.join("label.csv")).records() {
        let rec = rec.unwrap();
        let g = match &rec[3] {
            "Exact" if esci => 100,
            "E" => 100,
            "S" => 10,
            "C" => 1,
            "Exact" => 2,
            "Partial" => 1,
            _ => 0,
        };
        qrels.entry(rec[1].parse().unwrap()).or_default().insert(rec[2].to_string(), g);
    }

    let mut results = serde_json::Map::new();
    let mut per_query: HashMap<&str, Vec<f64>> = HashMap::new();
    let mut vs = variants(esci);
    // Others by name: `filter:class+color`, `boost:style@0.25`.
    for o in only.iter().flatten() {
        if vs.iter().any(|(n, _)| n == o) {
            continue;
        }
        // `mode:fields@weight#share`, `*` for the table's own fields.
        if let Some((mode, rest)) = o.split_once(':') {
            let (rest, share) = rest.split_once('#').unwrap_or((rest, "0.1"));
            let (fields, w) = rest.split_once('@').unwrap_or((rest, "0.5"));
            let fields = if fields == "*" { String::new() } else { format!(", understand_fields => '{}'", fields.replace('+', ", ")) };
            vs.push((Box::leak(o.clone().into_boxed_str()), format!(", understand => '{mode}'{fields}, understand_weight => {w}, understand_share => {share}")));
        }
    }
    for (name, opts) in &vs {
        if only.as_ref().is_some_and(|o| !o.iter().any(|x| x == name)) {
            continue;
        }
        let sql = format!("SELECT id FROM products ORDER BY bm25(body, :q{opts}) DESC LIMIT 10");
        let (mut scores, mut lat) = (Vec::new(), Vec::new());
        let (mut understood, mut class_named, mut class_right) = (0usize, 0usize, 0usize);
        let mut examples = Vec::new();
        for (qid, text, qclass) in &queries {
            let p = Params::new().set("q", Value::Str(text.clone()));
            let t = Instant::now();
            let rs = db.query(&sql, &p).unwrap_or_else(|e| panic!("{name} {text}: {e}"));
            lat.push(t.elapsed().as_secs_f64() * 1000.0);
            let ranked: Vec<String> = rs.rows.iter().map(|r| r[0].as_str().unwrap().to_string()).collect();
            scores.push(ndcg_at_k(&ranked, qrels.get(qid).unwrap_or(&HashMap::new()), 10));
            if let Some(line) = rs.stats.plan.iter().find(|l| l.contains("understood")) {
                if !line.ends_with(": nothing") {
                    understood += 1;
                }
                if examples.len() < 12 && !line.ends_with(": nothing") {
                    examples.push(json!(line));
                }
            }
            // Which classes the words named, against the class the query asks for.
            if !esci && *name == "boost" {
                let u = db.understand("products", "body", text, &chloedb::query::understand::Options { fields: Some(vec!["class".into()]), ..Default::default() }).unwrap();
                let classes: Vec<&str> = u.spans.iter().flat_map(|s| s.matches.iter().map(|m| m.value.as_str())).collect();
                if !classes.is_empty() {
                    class_named += 1;
                    if classes.iter().any(|c| c.eq_ignore_ascii_case(qclass)) {
                        class_right += 1;
                    }
                }
            }
        }
        per_query.insert(name, scores.clone());
        let pick = |odd: bool| -> Vec<f64> { queries.iter().zip(&scores).filter(|((q, _, _), _)| (q % 2 == 1) == odd).map(|(_, s)| *s).collect() };
        let (dev, test) = (pick(false), pick(true));
        let mut v = json!({
            "sql": sql,
            "ndcg@10": mean(&scores),
            "ndcg@10_dev": mean(&dev),
            "ndcg@10_test": mean(&test),
            "p50_ms": percentile(&lat, 50.0),
            "p99_ms": percentile(&lat, 99.0),
            "queries_understood": understood,
            "examples": examples,
        });
        if let Some(plain) = per_query.get("plain") {
            if *name != "plain" {
                let pt: Vec<f64> = queries.iter().zip(plain).filter(|((q, _, _), _)| q % 2 == 1).map(|(_, s)| *s).collect();
                v["p_value_test_vs_plain"] = json!(paired_randomization(&test, &pt, 10_000, 7));
            }
        }
        if class_named > 0 {
            v["class_named"] = json!(class_named);
            v["class_named_right"] = json!(class_right);
        }
        eprintln!("{name:>18}: nDCG@10 {:.4} (dev {:.4}, test {:.4}), p50 {:.2} ms, understood {understood}", mean(&scores), mean(&dev), mean(&test), percentile(&lat, 50.0));
        results.insert(name.to_string(), v);
    }
    let out = json!({
        "collection": if esci { "ESCI (US, small version, test queries)" } else { "WANDS" },
        "products": n_products,
        "queries": queries.len(),
        "machine": machine(),
        "body": if esci { "product_title . product_bullet_point . product_description" } else { "product_name . product_description" },
        "variants": results,
    });
    drop(db);
    let _ = std::fs::remove_dir_all(&tmp);
    let text = serde_json::to_string_pretty(&out).unwrap();
    match &args.out {
        Some(p) => std::fs::write(p, text + "\n").unwrap(),
        None => println!("{text}"),
    }
}
