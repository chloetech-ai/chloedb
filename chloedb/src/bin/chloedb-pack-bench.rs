//! `chloedb-pack-bench`: what `PACK n TOKENS` gives against the usual way
//! to fill a model's context, the top rows of a ranking cut in the client.
//!
//! On BEIR collections (SciFact and NFCorpus by default), each query ranks
//! the documents by `bm25(body, :q)`, and the context is filled to a budget
//! of tokens three ways:
//!
//! - `top_k_trim`: the query's top 200 rows, whole, in order, while they
//!   fit; the first that does not is cut to what is left (its start), and
//!   the context ends there. This is what an application does with a top k.
//! - `pack`: `PACK n TOKENS`, a row at most a quarter of the budget, cut
//!   around the query's terms.
//! - `pack_dedupe`: the same with `DEDUPE 0.85`.
//!
//! For each, per budget: the share of a query's judged-relevant documents
//! the context holds some of (recall), the tokens of the context that are
//! of relevant documents (precision per token), the rows, the tokens spent
//! and the time per query. Tokens are [`count_tokens`]'s estimate for all
//! three.
//!
//! With `--token-sample FILE` it also writes a sample of texts with the
//! estimate of each (`{"source", "text", "estimate"}` per line), which
//! `chloedb/results/pack/token_error.py` compares with real tokenizers.
//!
//! ```sh
//! chloedb/tests/fixtures/beir/fetch.sh ~/beir
//! cargo run --release --bin chloedb-pack-bench -- --dir ~/beir
//! ```

use std::collections::HashSet;
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use chloedb::beir;
use chloedb::db::Db;
use chloedb::metrics::{mean, percentile};
use chloedb::query::pack::{count_tokens, excerpt};
use chloedb::query::Params;
use chloedb::storage::DbOptions;
use chloedb::{Field, FieldType, Schema, Value};
use clap::Parser;
use serde_json::{json, Value as Json};

#[derive(Parser)]
#[command(about = "PACK n TOKENS against a top k cut in the client, on BEIR")]
struct Args {
    /// Directory holding one BEIR folder per collection.
    #[arg(long)]
    dir: PathBuf,
    #[arg(long, value_delimiter = ',', default_value = "scifact,nfcorpus")]
    collections: Vec<String>,
    /// Budgets of tokens, comma-separated.
    #[arg(long, value_delimiter = ',', default_value = "500,1000,2000,4000")]
    budgets: Vec<usize>,
    #[arg(long, default_value = "results/pack/pack_vs_top_k.json")]
    out: PathBuf,
    /// Also write a sample of texts and their estimates here.
    #[arg(long)]
    token_sample: Option<PathBuf>,
    /// More texts for the sample: JSON lines with `text` (and `title`),
    /// the source named by the file (MIRACL's passages, for other scripts).
    #[arg(long, value_delimiter = ',')]
    token_extra: Vec<PathBuf>,
    /// Source code for the sample: the `.rs` files under this directory,
    /// in pieces of about 2,000 bytes.
    #[arg(long)]
    token_code: Option<PathBuf>,
    /// Directory for the databases (default: system temp).
    #[arg(long)]
    tmp: Option<PathBuf>,
}

fn machine() -> Json {
    let cpu = std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with("model name")).and_then(|l| l.split(':').nth(1)).map(|m| m.trim().to_string()));
    json!({ "cpu": cpu, "cpus": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1), "os": std::env::consts::OS, "arch": std::env::consts::ARCH })
}

/// One context: the documents it holds some of, with the tokens of each.
struct Context {
    docs: Vec<(String, usize)>,
    ms: f64,
}

/// The top rows, whole while they fit, then the first that does not cut
/// to what is left.
fn top_k_trim(db: &Db, p: &Params, budget: usize) -> Context {
    let t = Instant::now();
    let rs = db.query("SELECT id, body FROM docs ORDER BY bm25(body, :q, syntax => 'plain') DESC LIMIT 200", p).unwrap();
    let mut docs = Vec::new();
    let mut used = 0;
    for r in &rs.rows {
        let (id, body) = (r[0].as_str().unwrap().to_string(), r[1].as_str().unwrap_or(""));
        let n = count_tokens(body);
        if used + n <= budget {
            used += n;
            docs.push((id, n));
            continue;
        }
        let cut = excerpt(body, &[], budget - used);
        let n = count_tokens(&cut);
        if n > 0 {
            docs.push((id, n));
        }
        break;
    }
    Context { docs, ms: t.elapsed().as_secs_f64() * 1e3 }
}

fn packed(db: &Db, p: &Params, budget: usize, options: &str) -> Context {
    let t = Instant::now();
    let sql = format!("SELECT id, body FROM docs ORDER BY bm25(body, :q, syntax => 'plain') DESC PACK {budget} TOKENS {options}");
    let pk = db.pack(&sql, p).unwrap_or_else(|e| panic!("{sql}: {e}"));
    let docs = pk.receipt.rows.iter().filter(|d| d.taken).map(|d| (d.key.as_str().unwrap().to_string(), d.tokens)).collect();
    Context { docs, ms: t.elapsed().as_secs_f64() * 1e3 }
}

fn main() {
    let a = Args::parse();
    let tmp = a.tmp.clone().unwrap_or_else(std::env::temp_dir).join(format!("chloedb-pack-bench-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let mut sample = a.token_sample.as_ref().map(|f| std::io::BufWriter::new(std::fs::File::create(f).unwrap()));
    if let Some(w) = sample.as_mut() {
        for f in &a.token_extra {
            let source = f.file_stem().unwrap().to_string_lossy().to_string();
            for line in std::fs::read_to_string(f).unwrap().lines() {
                let j: Json = serde_json::from_str(line).unwrap();
                let (title, text) = (j["title"].as_str().unwrap_or(""), j["text"].as_str().unwrap_or(""));
                let body = if title.is_empty() { text.to_string() } else { format!("{title}\n{text}") };
                writeln!(w, "{}", json!({ "source": source, "text": body, "estimate": count_tokens(&body) })).unwrap();
            }
        }
        if let Some(dir) = &a.token_code {
            let mut files = Vec::new();
            let mut stack = vec![dir.clone()];
            while let Some(d) = stack.pop() {
                for e in std::fs::read_dir(&d).unwrap().flatten() {
                    let p = e.path();
                    match p.is_dir() {
                        true => stack.push(p),
                        false if p.extension().is_some_and(|x| x == "rs") => files.push(p),
                        false => {}
                    }
                }
            }
            files.sort();
            for f in files {
                let text = std::fs::read_to_string(&f).unwrap();
                let mut at = 0;
                while at < text.len() {
                    let mut to = (at + 2000).min(text.len());
                    while !text.is_char_boundary(to) {
                        to += 1;
                    }
                    let piece = &text[at..to];
                    writeln!(w, "{}", json!({ "source": "rust code", "text": piece, "estimate": count_tokens(piece) })).unwrap();
                    at = to;
                }
            }
        }
    }
    let mut report = serde_json::Map::new();
    for name in &a.collections {
        let coll = beir::load(&a.dir.join(name), "test").unwrap_or_else(|e| panic!("{e}"));
        let schema = Schema::new("docs", vec![Field::new("id", FieldType::Keyword), Field::new("body", FieldType::Text { positions: false })], Some("id")).unwrap();
        let db = Db::create(&tmp.join(format!("{name}.strata")), schema, DbOptions { sync: false, ..DbOptions::default() }).unwrap();
        db.load(coll.docs.iter().map(|(id, body)| vec![Value::Str(id.clone()), Value::Str(body.clone())])).unwrap();
        db.storage().flush().unwrap();
        if let Some(w) = sample.as_mut() {
            for (_, body) in coll.docs.iter().step_by((coll.docs.len() / 500).max(1)) {
                writeln!(w, "{}", json!({ "source": name, "text": body, "estimate": count_tokens(body) })).unwrap();
            }
        }
        let doc_tokens: Vec<f64> = coll.docs.iter().map(|(_, b)| count_tokens(b) as f64).collect();
        let queries: Vec<(&String, &String)> = coll.queries.iter().filter(|(q, _)| coll.qrels.get(q).is_some_and(|r| r.values().any(|g| *g > 0))).map(|(q, t)| (q, t)).collect();
        eprintln!("{name}: {} documents ({:.0} tokens on average), {} judged queries", coll.docs.len(), mean(&doc_tokens), queries.len());
        let mut by_budget = serde_json::Map::new();
        for &budget in &a.budgets {
            let mut variants = serde_json::Map::new();
            for variant in ["top_k_trim", "pack", "pack_dedupe"] {
                let (mut recall, mut precision, mut rows, mut used, mut ms) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
                for (qid, text) in &queries {
                    let p = Params::new().set("q", Value::Str((*text).clone()));
                    let c = match variant {
                        "top_k_trim" => top_k_trim(&db, &p, budget),
                        "pack" => packed(&db, &p, budget, ""),
                        _ => packed(&db, &p, budget, "DEDUPE 0.85"),
                    };
                    let relevant: HashSet<&str> = coll.qrels[*qid].iter().filter(|(_, g)| **g > 0).map(|(d, _)| d.as_str()).collect();
                    let spent: usize = c.docs.iter().map(|d| d.1).sum();
                    assert!(spent <= budget, "{variant} spent {spent} of {budget}");
                    let held = c.docs.iter().filter(|d| relevant.contains(d.0.as_str())).count();
                    let relevant_tokens: usize = c.docs.iter().filter(|d| relevant.contains(d.0.as_str())).map(|d| d.1).sum();
                    recall.push(held as f64 / relevant.len() as f64);
                    precision.push(relevant_tokens as f64 / budget as f64);
                    rows.push(c.docs.len() as f64);
                    used.push(spent as f64);
                    ms.push(c.ms);
                }
                let doc = json!({
                    "recall": mean(&recall),
                    "relevant_token_share": mean(&precision),
                    "rows": mean(&rows),
                    "tokens_used": mean(&used),
                    "ms_p50": percentile(&ms, 50.0),
                    "ms_p99": percentile(&ms, 99.0),
                });
                eprintln!(
                    "  {budget:>5} tokens {variant:<12} recall {:.3}, relevant tokens {:.3} of the budget, {:.1} rows, {:.0} tokens, p50 {:.2} ms",
                    doc["recall"].as_f64().unwrap(),
                    doc["relevant_token_share"].as_f64().unwrap(),
                    doc["rows"].as_f64().unwrap(),
                    doc["tokens_used"].as_f64().unwrap(),
                    doc["ms_p50"].as_f64().unwrap()
                );
                variants.insert(variant.to_string(), doc);
            }
            by_budget.insert(budget.to_string(), Json::Object(variants));
        }
        report.insert(name.clone(), json!({ "docs": coll.docs.len(), "doc_tokens_mean": mean(&doc_tokens), "queries": queries.len(), "budgets": by_budget }));
        drop(db);
    }
    let out = json!({ "machine": machine(), "collections": report });
    if let Some(dir) = a.out.parent() {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(&a.out, serde_json::to_string_pretty(&out).unwrap() + "\n").unwrap();
    eprintln!("wrote {}", a.out.display());
    if let Some(mut w) = sample {
        w.flush().unwrap();
    }
    let _ = std::fs::remove_dir_all(&tmp);
}
