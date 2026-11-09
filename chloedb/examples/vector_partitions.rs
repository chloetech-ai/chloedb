//! The engine comparison's "vector + filter" query (`WHERE rating >= 4
//! ORDER BY similarity(emb, :v) DESC LIMIT 10`) on a VECTOR field
//! partitioned by `rating` ([`chloedb::schema::VectorPartition`]), against
//! the plans Strata makes without partitions, by recall@10 against the
//! exact answers and time.
//!
//! The data file holds five arrays, each a little-endian u64 count and then
//! the values: the rows' vectors (n · dim f32), their ratings (n f32), their
//! topics (n u8, unused here), the queries' vectors (nq · dim f32) and the
//! queries' topics (nq u8, unused).
//!
//! `build` loads it into one segment as `engine-compare build` builds
//! Strata's (HNSW m 16, ef_construction 200, SQ8 codes, the vector not
//! stored), with the field partitioned by `rating` at 1, 2, 3 and 4 or,
//! with `--plain`, not partitioned. `truth` writes each query's exact top
//! 10 by dot product for each filter. `run` times every plan.
//!
//! ```text
//! cargo run --release --example vector_partitions -- build data.bin parts.strata [--plain] [threads]
//! cargo run --release --example vector_partitions -- truth data.bin truth.json
//! cargo run --release --example vector_partitions -- run parts.strata truth.json data.bin [plain.strata]
//! ```

use std::collections::BinaryHeap;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use serde_json::{json, Value as Json};
use chloedb::db::Db;
use chloedb::metrics::percentile;
use chloedb::query::exec::ExecOptions;
use chloedb::query::Params;
use chloedb::schema::VectorPartition;
use chloedb::segment::build::BuildOptions;
use chloedb::storage::DbOptions;
use chloedb::value::Value;
use chloedb::vector::kernels::dot_f32;
use chloedb::{Field, FieldType, Schema, VectorCodes, VectorIndexKind};

/// The filters: the benchmark's (20% of rows), and two narrower ones.
const FILTERS: [(&str, f32); 4] = [("none", f32::NEG_INFINITY), ("rating >= 4", 4.0), ("rating >= 4.5", 4.5), ("rating >= 4.9", 4.9)];

fn s<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn u64_at(f: &mut File, at: u64) -> Result<u64, String> {
    let mut b = [0u8; 8];
    f.seek(SeekFrom::Start(at)).map_err(s)?;
    f.read_exact(&mut b).map_err(s)?;
    Ok(u64::from_le_bytes(b))
}

fn f32s(f: &mut impl Read, n: usize) -> Result<Vec<f32>, String> {
    let mut b = vec![0u8; n * 4];
    f.read_exact(&mut b).map_err(s)?;
    Ok(b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
}

/// The data file's layout: row count, dimensions, and where each array's
/// values start.
struct Layout {
    n: usize,
    dim: usize,
    rows: u64,
    ratings: u64,
    queries: u64,
    nq: usize,
}

fn layout(path: &Path) -> Result<Layout, String> {
    let mut f = File::open(path).map_err(s)?;
    let nd = u64_at(&mut f, 0)?;
    let at = 8 + nd * 4;
    let n = u64_at(&mut f, at)?;
    let ratings = at + 8;
    let topics_at = ratings + n * 4;
    let nt = u64_at(&mut f, topics_at)?;
    let q_at = topics_at + 8 + nt;
    let nqd = u64_at(&mut f, q_at)?;
    let dim = (nd / n) as usize;
    Ok(Layout { n: n as usize, dim, rows: 8, ratings, queries: q_at + 8, nq: nqd as usize / dim })
}

fn ratings(path: &Path, l: &Layout) -> Result<Vec<f32>, String> {
    let mut f = File::open(path).map_err(s)?;
    f.seek(SeekFrom::Start(l.ratings)).map_err(s)?;
    f32s(&mut f, l.n)
}

fn queries(path: &Path, l: &Layout) -> Result<Vec<Vec<f32>>, String> {
    let mut f = File::open(path).map_err(s)?;
    f.seek(SeekFrom::Start(l.queries)).map_err(s)?;
    Ok(f32s(&mut f, l.nq * l.dim)?.chunks(l.dim).map(<[f32]>::to_vec).collect())
}

/// The rows' vectors, `chunk` at a time.
fn each_chunk(path: &Path, l: &Layout, chunk: usize, mut f: impl FnMut(usize, &[f32]) -> Result<(), String>) -> Result<(), String> {
    let mut r = BufReader::with_capacity(1 << 24, File::open(path).map_err(s)?);
    r.seek(SeekFrom::Start(l.rows)).map_err(s)?;
    let mut start = 0;
    while start < l.n {
        let m = chunk.min(l.n - start);
        let v = f32s(&mut r, m * l.dim)?;
        f(start, &v)?;
        start += m;
    }
    Ok(())
}

fn build(data: &Path, out: &Path, plain: bool, threads: usize) -> Result<(), String> {
    let l = layout(data)?;
    let ratings = ratings(data, &l)?;
    for p in [out.to_path_buf(), Path::new(&format!("{}-wal", out.display())).to_path_buf()] {
        if p.exists() {
            std::fs::remove_file(&p).map_err(s)?;
        }
    }
    let mut emb = Field::new("emb", FieldType::Vector { dim: l.dim, index: VectorIndexKind::Hnsw { m: 16, ef_construction: 200 } }).not_stored();
    emb.codes = VectorCodes::Sq8;
    if !plain {
        emb.partition = Some(VectorPartition { by: "rating".into(), bounds: vec![1.0, 2.0, 3.0, 4.0] });
    }
    let schema = Schema::new("docs", vec![Field::new("id", FieldType::Keyword), Field::new("rating", FieldType::Float), emb], Some("id")).map_err(s)?;
    let opts = DbOptions { build: BuildOptions::default().set("vector.threads", threads), segment_max_docs: l.n + 1, sync: false, ..DbOptions::default() };
    let db = Db::create(out, schema, opts).map_err(s)?;
    let t = Instant::now();
    let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<Value>>(4096);
    let path = data.to_path_buf();
    let reader = std::thread::spawn(move || {
        let l = layout(&path)?;
        each_chunk(&path, &l, 4096, |start, v| {
            for (i, row) in v.chunks(l.dim).enumerate() {
                let d = start + i;
                let doc = vec![Value::Str(format!("d{d}")), Value::Float(ratings[d] as f64), Value::Vector(Arc::from(row))];
                tx.send(doc).map_err(|_| "the load stopped".to_string())?;
            }
            Ok(())
        })
    });
    let loaded = db.load(rx.iter());
    reader.join().map_err(|_| "the reader panicked".to_string())??;
    loaded.map_err(s)?;
    db.storage().flush().map_err(s)?;
    let build_s = t.elapsed().as_secs_f64();
    let segments = db.snapshot().segments().len();
    let file_bytes = db.storage().file_stats().file_bytes;
    let stats = chloedb::inspect::table(&db.snapshot(), &chloedb::inspect::Options::default()).map_err(s)?;
    let emb_parts = stats.fields.iter().find(|f| f.name == "emb").map(|f| json!(f.sections)).unwrap_or(Json::Null);
    // The process's peak resident memory (Linux).
    let peak_kb = std::fs::read_to_string("/proc/self/status").ok().and_then(|st| st.lines().find(|l| l.starts_with("VmHWM:")).and_then(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok()));
    println!("{}", json!({"rows": l.n, "dim": l.dim, "plain": plain, "threads": threads, "build_s": build_s, "segments": segments, "file_bytes": file_bytes, "peak_rss_kb": peak_kb, "emb_sections": emb_parts}));
    Ok(())
}

/// Each query's exact top 10 for each filter, by dot product.
fn truth(data: &Path, out: &Path) -> Result<(), String> {
    let l = layout(data)?;
    let ratings = ratings(data, &l)?;
    let qs = queries(data, &l)?;
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    // Per query and filter, a min-heap of the best 10 (score, row).
    type Top = BinaryHeap<std::cmp::Reverse<(Ord32, u32)>>;
    let mut tops: Vec<Vec<Top>> = (0..qs.len()).map(|_| (0..FILTERS.len()).map(|_| BinaryHeap::new()).collect()).collect();
    let t = Instant::now();
    each_chunk(data, &l, 16384, |start, v| {
        let per = qs.len().div_ceil(threads);
        std::thread::scope(|sc| {
            for (qi, (qchunk, tchunk)) in qs.chunks(per).zip(tops.chunks_mut(per)).enumerate() {
                let ratings = &ratings;
                let _ = qi;
                sc.spawn(move || {
                    for (q, top) in qchunk.iter().zip(tchunk.iter_mut()) {
                        for (i, row) in v.chunks(l.dim).enumerate() {
                            let d = start + i;
                            let score = dot_f32(q, row);
                            for (fi, &(_, lo)) in FILTERS.iter().enumerate() {
                                if ratings[d] >= lo {
                                    let h = &mut top[fi];
                                    if h.len() < 10 {
                                        h.push(std::cmp::Reverse((Ord32(score), d as u32)));
                                    } else if score > h.peek().unwrap().0 .0 .0 {
                                        h.pop();
                                        h.push(std::cmp::Reverse((Ord32(score), d as u32)));
                                    }
                                }
                            }
                        }
                    }
                });
            }
        });
        Ok(())
    })?;
    let mut by_filter = serde_json::Map::new();
    for (fi, &(name, lo)) in FILTERS.iter().enumerate() {
        let share = ratings.iter().filter(|&&r| r >= lo).count() as f64 / l.n as f64;
        let ids: Vec<Vec<u32>> = tops
            .iter()
            .map(|t| {
                let mut v: Vec<(f32, u32)> = t[fi].iter().map(|r| (r.0 .0 .0, r.0 .1)).collect();
                v.sort_by(|a, b| b.0.total_cmp(&a.0));
                v.into_iter().map(|x| x.1).collect()
            })
            .collect();
        by_filter.insert(name.into(), json!({"share": share, "ids": ids}));
    }
    std::fs::write(out, serde_json::to_string(&json!({"rows": l.n, "queries": qs.len(), "filters": by_filter})).map_err(s)?).map_err(s)?;
    eprintln!("truth for {} queries in {:.0} s", qs.len(), t.elapsed().as_secs_f64());
    Ok(())
}

#[derive(Clone, Copy, PartialEq, PartialOrd)]
struct Ord32(f32);
impl Eq for Ord32 {}
#[allow(clippy::derive_ord_xor_partial_ord)]
impl Ord for Ord32 {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&o.0)
    }
}

/// One plan's pass over the queries: recall@10 and per-query times (µs),
/// and the plan of the first query.
fn pass(db: &Db, qs: &[Vec<f32>], truth: &[Vec<u32>], filter: &str, extra: &str, exec: &ExecOptions) -> Result<(f64, Vec<f64>, Vec<String>), String> {
    let w = if filter == "none" { String::new() } else { format!("WHERE {filter}") };
    let sql = format!("SELECT id FROM docs {w} ORDER BY similarity(emb, :v{extra}) DESC LIMIT 10");
    let mut recall = 0.0;
    let mut times = Vec::with_capacity(qs.len());
    let mut plan = Vec::new();
    for (i, q) in qs.iter().enumerate() {
        let p = Params::new().set("v", Value::Vector(q.clone().into()));
        let t = Instant::now();
        let r = db.query_with(&sql, &p, exec).map_err(|e| format!("{sql}: {e}"))?;
        times.push(t.elapsed().as_secs_f64() * 1e6);
        let got: Vec<u32> = r.rows.iter().map(|row| row[0].as_str().and_then(|x| x[1..].parse().ok()).unwrap_or(u32::MAX)).collect();
        let want = &truth[i];
        recall += want.iter().filter(|w| got.contains(w)).count() as f64 / want.len().max(1) as f64;
        if i == 0 {
            plan = r.stats.plan;
        }
    }
    Ok((recall / qs.len() as f64, times, plan))
}

fn run(db_path: &Path, truth_path: &Path, data: &Path, plain_path: Option<&Path>) -> Result<(), String> {
    let l = layout(data)?;
    let qs = queries(data, &l)?;
    let truth: Json = serde_json::from_str(&std::fs::read_to_string(truth_path).map_err(s)?).map_err(s)?;
    let db = Db::open(db_path, DbOptions { sync: false, ..DbOptions::default() }).map_err(s)?;
    let plain = plain_path.map(|p| Db::open(p, DbOptions { sync: false, ..DbOptions::default() })).transpose().map_err(s)?;
    let with = ExecOptions { threads: 1, ..ExecOptions::default() };
    let without = ExecOptions { vector_partitions: false, ..with.clone() };
    // (label, database, similarity options, exec options)
    let mut plans: Vec<(String, &Db, String, &ExecOptions)> = Vec::new();
    if let Some(p) = &plain {
        plans.push(("no partitions (today), auto".into(), p, String::new(), &with));
    }
    plans.push(("partitioned table, partitions off, auto".into(), &db, String::new(), &without));
    plans.push(("partitioned table, auto".into(), &db, String::new(), &with));
    for ef in [64, 128, 256, 512] {
        plans.push((format!("post-filter, ef {ef}"), &db, format!(", strategy => 'postfilter', ef => {ef}"), &with));
    }
    for ef in [32, 64, 128, 256] {
        plans.push((format!("partitions, ef {ef}"), &db, format!(", strategy => 'partition', ef => {ef}"), &with));
    }
    plans.push(("pre-filter scan".into(), &db, ", strategy => 'prefilter'".into(), &with));
    // Warm the pages: one pass of each database over every filter.
    for d in std::iter::once(&db).chain(plain.iter()) {
        for (f, _) in FILTERS {
            let ids: Vec<Vec<u32>> = serde_json::from_value(truth["filters"][f]["ids"].clone()).map_err(s)?;
            pass(d, &qs, &ids, f, "", &without)?;
        }
    }
    let mut out = Vec::new();
    for (f, _) in FILTERS {
        let ids: Vec<Vec<u32>> = serde_json::from_value(truth["filters"][f]["ids"].clone()).map_err(s)?;
        let share = truth["filters"][f]["share"].as_f64().unwrap_or(0.0);
        for (label, d, extra, exec) in &plans {
            if f == "none" && (label.starts_with("pre-filter") || label.starts_with("post-filter") || label.starts_with("partitions, ")) {
                continue;
            }
            // Best of two passes (container timings drift between runs).
            let (recall, t1, plan) = pass(d, &qs, &ids, f, extra, exec)?;
            let (_, t2, _) = pass(d, &qs, &ids, f, extra, exec)?;
            let mean = |t: &[f64]| t.iter().sum::<f64>() / t.len() as f64;
            let best = if mean(&t1) <= mean(&t2) { t1 } else { t2 };
            let row = json!({"filter": f, "share": share, "plan": label, "recall": recall, "mean_us": mean(&best), "p50_us": percentile(&best, 50.0), "p95_us": percentile(&best, 95.0), "first_plan": plan});
            println!("{row}");
            out.push(row);
        }
    }
    // The headline pair, alternated three times: the benchmark's query as
    // Strata plans it with and without partitions.
    let ids: Vec<Vec<u32>> = serde_json::from_value(truth["filters"]["rating >= 4"]["ids"].clone()).map_err(s)?;
    for round in 0..3 {
        for (label, exec) in [("auto, partitions off", &without), ("auto, partitions on", &with)] {
            let (recall, t, _) = pass(&db, &qs, &ids, "rating >= 4", "", exec)?;
            let row = json!({"ab_round": round, "plan": label, "recall": recall, "mean_us": t.iter().sum::<f64>() / t.len() as f64, "p50_us": percentile(&t, 50.0)});
            println!("{row}");
        }
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.first().map(String::as_str) {
        Some("build") if args.len() >= 3 => {
            let plain = args.iter().any(|a| a == "--plain");
            let threads = args[3..].iter().filter(|a| *a != "--plain").find_map(|a| a.parse().ok()).unwrap_or(4);
            build(Path::new(&args[1]), Path::new(&args[2]), plain, threads)
        }
        Some("truth") if args.len() == 3 => truth(Path::new(&args[1]), Path::new(&args[2])),
        Some("run") if args.len() >= 4 => run(Path::new(&args[1]), Path::new(&args[2]), Path::new(&args[3]), args.get(4).map(Path::new)),
        _ => Err("usage: vector_partitions build DATA DB [--plain] [THREADS] | truth DATA TRUTH | run DB TRUTH DATA [PLAIN_DB]".into()),
    };
    if let Err(e) = r {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
