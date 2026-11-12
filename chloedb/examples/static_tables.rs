//! Static (Model2Vec) models with their tables in other element types,
//! against the F32 table: the cosine of each text's vectors, and texts a
//! second on one core. For `research/cpu-encoders` (quantize_static.py
//! writes the int8 and f16 copies).
//!
//! ```text
//! cargo run --profile ci --example static_tables -- ~/beir/scifact/corpus.jsonl F32_DIR OTHER_DIR...
//! ```

use std::io::BufRead;
use std::path::Path;
use std::time::Instant;

use chloedb::embed::{Embedder, StaticEmbedder};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let f = std::fs::File::open(&args[0]).unwrap();
    let texts: Vec<String> = std::io::BufReader::new(f)
        .lines()
        .map(|l| {
            let v: serde_json::Value = serde_json::from_str(&l.unwrap()).unwrap();
            format!("{}\n{}", v["title"].as_str().unwrap_or(""), v["text"].as_str().unwrap_or(""))
        })
        .collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    let mut base: Option<Vec<Vec<f32>>> = None;
    for dir in &args[1..] {
        let e = StaticEmbedder::open(Path::new(dir)).unwrap_or_else(|e| panic!("{dir}: {e}"));
        let bytes = std::fs::metadata(Path::new(dir).join("model.safetensors")).unwrap().len();
        let t = Instant::now();
        let v = e.embed(&refs).unwrap();
        let rate = refs.len() as f64 / t.elapsed().as_secs_f64();
        let cos = base.as_ref().map(|b| {
            let c: Vec<f64> = b.iter().zip(&v).map(|(x, y)| x.iter().zip(y).map(|(a, b)| (a * b) as f64).sum()).collect();
            (c.iter().sum::<f64>() / c.len() as f64, c.iter().cloned().fold(1.0, f64::min))
        });
        println!("{dir}: {} MB, {rate:.0} texts/s on one core{}", bytes >> 20, cos.map_or(String::new(), |(m, lo)| format!(", cosine to the first: mean {m:.5}, min {lo:.5}")));
        base.get_or_insert(v);
    }
}
