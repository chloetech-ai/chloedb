//! Content understanding's built-in models ([`chloedb::content`]) measured:
//! `builtin:lang` on a CSV of `labels,text` rows, such as the test split of
//! `papluca/language-identification` (10,000 texts in 20 languages, from
//! reviews, news and sentences).
//!
//! ```text
//! curl -L -o test.csv https://huggingface.co/datasets/papluca/language-identification/resolve/main/test.csv
//! chloedb-content-eval test.csv --out results/text/content_lang.json
//! ```

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use clap::Parser;
use serde_json::json;

#[derive(Parser)]
struct Args {
    /// A CSV with columns `labels` (an ISO 639-1 code) and `text`.
    csv: PathBuf,
    #[arg(long)]
    out: Option<PathBuf>,
}

fn main() {
    let args = Args::parse();
    let mut r = csv::Reader::from_path(&args.csv).unwrap_or_else(|e| panic!("{}: {e}", args.csv.display()));
    let rows: Vec<(String, String)> = r.records().map(|x| x.unwrap()).map(|x| (x[0].to_string(), x[1].to_string())).collect();
    let mut per: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut confused: BTreeMap<String, usize> = BTreeMap::new();
    let t = Instant::now();
    let mut right = 0;
    for (want, text) in &rows {
        let got = chloedb::content::detect(text);
        let e = per.entry(want.clone()).or_default();
        e.0 += 1;
        if got == want {
            e.1 += 1;
            right += 1;
        } else {
            *confused.entry(format!("{want}->{got}")).or_default() += 1;
        }
    }
    let us = t.elapsed().as_secs_f64() * 1e6 / rows.len() as f64;
    let mut worst: Vec<(String, usize)> = confused.into_iter().collect();
    worst.sort_by(|a, b| b.1.cmp(&a.1));
    let cpu = std::fs::read_to_string("/proc/cpuinfo").ok().and_then(|s| s.lines().find(|l| l.starts_with("model name")).and_then(|l| l.split(':').nth(1)).map(|m| m.trim().to_string()));
    let out = json!({
        "data": args.csv.file_name().map(|f| f.to_string_lossy().to_string()),
        "texts": rows.len(),
        "accuracy": right as f64 / rows.len() as f64,
        "per_language": per.iter().map(|(k, (n, ok))| (k.clone(), json!({"texts": n, "accuracy": *ok as f64 / *n as f64}))).collect::<serde_json::Map<_, _>>(),
        "most_confused": worst.iter().take(15).map(|(k, n)| json!([k, n])).collect::<Vec<_>>(),
        "us_per_text": us,
        "machine": {"cpu": cpu, "cpus": std::thread::available_parallelism().map_or(0, |n| n.get())},
    });
    eprintln!("accuracy {:.4} over {} texts, {:.1} us a text; most confused {:?}", right as f64 / rows.len() as f64, rows.len(), us, &worst[..worst.len().min(10)]);
    let text = serde_json::to_string_pretty(&out).unwrap();
    match &args.out {
        Some(p) => std::fs::write(p, text + "\n").unwrap(),
        None => println!("{text}"),
    }
}
