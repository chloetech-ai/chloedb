//! A model ChloeDB embeds with ([`chloedb::embed::open`]), on a BEIR
//! collection: documents a second (as `Db::load` embeds them: split
//! across the cores if the model asks), the latency of one query, and
//! nDCG@10 of exact search. For `research/cpu-encoders`.
//!
//! ```text
//! cargo run --profile ci --features onnx --example embed_beir -- MODEL_DIR ~/beir/scifact [DOCS]
//! ```

use std::path::Path;
use std::time::Instant;


fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let model = chloedb::embed::open(&args[0], Path::new("/")).unwrap_or_else(|e| panic!("{e}"));
    let c = chloedb::beir::load(Path::new(&args[1]), "test").unwrap();
    let limit = args.get(2).and_then(|n| n.parse().ok()).unwrap_or(c.docs.len());
    let texts: Vec<&str> = c.docs.iter().map(|(_, t)| t.as_str()).collect();
    let threads = if model.parallel() { std::thread::available_parallelism().map_or(1, |n| n.get()) } else { 1 };
    let t = Instant::now();
    let chunk = limit.div_ceil(threads);
    let docs: Vec<Vec<f32>> = std::thread::scope(|s| {
        let hs: Vec<_> = texts[..limit].chunks(chunk).map(|part| s.spawn(|| model.embed(part).unwrap())).collect();
        hs.into_iter().flat_map(|h| h.join().unwrap()).collect()
    });
    let dps = limit as f64 / t.elapsed().as_secs_f64();
    let mut lat = Vec::new();
    let queries: Vec<Vec<f32>> = c
        .queries
        .iter()
        .map(|(_, q)| {
            let t = Instant::now();
            let v = model.embed_query(q).unwrap();
            lat.push(t.elapsed().as_secs_f64() * 1e3);
            v
        })
        .collect();
    lat.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut ndcg = Vec::new();
    if limit == c.docs.len() {
        for ((qid, _), q) in c.queries.iter().zip(&queries) {
            let mut s: Vec<(f32, usize)> = docs.iter().enumerate().map(|(i, d)| (d.iter().zip(q).map(|(a, b)| a * b).sum(), i)).collect();
            s.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
            let ranked: Vec<String> = s.iter().map(|&(_, i)| c.docs[i].0.clone()).filter(|d| d != qid).take(100).collect();
            ndcg.push(chloedb::metrics::ndcg_at_k(&ranked, &c.qrels[qid], 10));
        }
    }
    println!(
        "{} on {}: {dps:.1} documents/s ({limit} documents, {threads} threads), query p50 {:.2} ms{}",
        model.name(),
        args[1],
        lat[lat.len() / 2],
        if ndcg.is_empty() { String::new() } else { format!(", nDCG@10 {:.4}", chloedb::metrics::mean(&ndcg)) }
    );
}
