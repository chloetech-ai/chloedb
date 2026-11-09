//! A small prepared collection for the simulator's tests, in the files
//! `data::load` reads: documents with the attributes of
//! prepare_msmarco_compare.py (years 1990 to 2024, 64 categories by Zipf's
//! law, ratings to the hundredth), queries of a few words, and vectors.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use rand::Rng;
use serde_json::json;

use super::dist::{stream, Ranks};

pub const DIM: usize = 8;

const WORDS: &[&str] = &[
    "solar", "panels", "convert", "sunlight", "electricity", "battery", "storage", "grid", "wind", "turbine", "energy", "power", "plant", "nuclear", "coal",
    "carbon", "emissions", "climate", "weather", "ocean", "river", "mountain", "forest", "desert", "city", "village", "history", "ancient", "empire", "kingdom",
    "medicine", "doctor", "patient", "hospital", "vaccine", "virus", "bacteria", "protein", "genetics", "evolution", "planet", "galaxy", "telescope", "rocket",
    "satellite", "computer", "software", "network", "database", "search", "engine", "language", "grammar", "poetry", "novel", "author", "museum", "painting",
];

fn unit(rng: &mut impl Rng) -> Vec<f32> {
    let v: Vec<f32> = (0..DIM).map(|_| rng.gen_range(-1.0f32..1.0)).collect();
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
    v.into_iter().map(|x| x / n).collect()
}

fn sentence(rng: &mut impl Rng, words: usize) -> String {
    (0..words).map(|_| WORDS[rng.gen_range(0..WORDS.len())]).collect::<Vec<_>>().join(" ")
}

/// Writes a collection of `docs` documents and `queries` queries (every
/// tenth of them without text, as some BEIR queries are) under `dir`,
/// and `heldout` documents more for inserts.
pub fn write(dir: &Path, docs: usize, queries: usize, heldout: usize) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let mut rng = stream(99, "fixture");
    let cats = Ranks::new(64, 1.0);
    let mut d = std::io::BufWriter::new(std::fs::File::create(dir.join("docs.jsonl")).unwrap());
    let mut dv = std::io::BufWriter::new(std::fs::File::create(dir.join("doc_vecs.f32")).unwrap());
    let mut h = std::io::BufWriter::new(std::fs::File::create(dir.join("heldout.jsonl")).unwrap());
    let mut hv = std::io::BufWriter::new(std::fs::File::create(dir.join("heldout_vecs.f32")).unwrap());
    for i in 0..docs + heldout {
        let doc = json!({
            "id": format!("d{i}"), "title": sentence(&mut rng, 3), "text": sentence(&mut rng, 20),
            "year": rng.gen_range(1990..2025), "category": format!("c{:02}", cats.draw(&mut rng)),
            "rating": (rng.gen_range(0..=500) as f64) / 100.0, "topic": format!("t{:02}", rng.gen_range(0..32)),
        });
        let v: Vec<u8> = unit(&mut rng).iter().flat_map(|x| x.to_le_bytes()).collect();
        if i < docs {
            writeln!(d, "{doc}").unwrap();
            dv.write_all(&v).unwrap();
        } else {
            writeln!(h, "{doc}").unwrap();
            hv.write_all(&v).unwrap();
        }
    }
    if heldout == 0 {
        drop(h);
        std::fs::remove_file(dir.join("heldout.jsonl")).unwrap();
        std::fs::remove_file(dir.join("heldout_vecs.f32")).unwrap();
    }
    let mut q = std::io::BufWriter::new(std::fs::File::create(dir.join("queries.jsonl")).unwrap());
    let mut qv = std::io::BufWriter::new(std::fs::File::create(dir.join("query_vecs.f32")).unwrap());
    for i in 0..queries {
        let n = rng.gen_range(2..7);
        let text = if i % 10 == 9 { String::new() } else { sentence(&mut rng, n) };
        writeln!(q, "{}", json!({"id": format!("q{i}"), "text": text})).unwrap();
        qv.write_all(&unit(&mut rng).iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>()).unwrap();
    }
    std::fs::write(dir.join("qrels.tsv"), "query-id\tcorpus-id\tscore\n").unwrap();
    std::fs::write(dir.join("meta.json"), json!({"docs": docs, "queries": queries, "dim": DIM}).to_string()).unwrap();
    dir.to_path_buf()
}

/// A fresh directory under the system's temporary one.
pub fn tempdir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("engine-compare-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}
