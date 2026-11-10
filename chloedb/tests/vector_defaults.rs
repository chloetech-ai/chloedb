//! The default vector search is close to exact: on 20,000 random unit
//! vectors (the hard case for a graph index) it finds at least 95% of the
//! true top 10, choosing the exact scan where it costs no more than the
//! index; and `ORDER BY distance(..)` ranks by Euclidean distance, which
//! differs from the dot product on vectors that are not unit length.
//!
//! `cargo test --profile ci --test vector_defaults -- --ignored --nocapture`
//! prints recall and latency at 20,000 and 200,000 rows
//! (`CHLOEDB_VEC_ROWS` sets the sizes).

use std::sync::Arc;
use std::time::Instant;

use rand::{Rng, SeedableRng};
use chloedb::db::Db;
use chloedb::query::exec::ResultSet;
use chloedb::query::Params;
use chloedb::storage::DbOptions;
use chloedb::value::Value;
use chloedb::{Field, FieldType, Schema};

const DIM: usize = 64;

fn gaussian(rng: &mut impl Rng) -> f32 {
    let (u, v): (f64, f64) = (rng.gen_range(1e-12..1.0), rng.gen());
    ((-2.0 * u.ln()).sqrt() * (2.0 * std::f64::consts::PI * v).cos()) as f32
}

fn unit(rng: &mut impl Rng) -> Vec<f32> {
    let v: Vec<f32> = (0..DIM).map(|_| gaussian(rng)).collect();
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    v.into_iter().map(|x| x / n).collect()
}

/// `VECTOR(64)` with the default index (HNSW, M 16), `n` rows, one segment.
fn table(dir: &std::path::Path, n: usize, vecs: &[Vec<f32>]) -> Db {
    let schema = Schema::new("t", vec![Field::new("id", FieldType::Int), Field::new("emb", FieldType::Vector { dim: DIM, index: chloedb::schema::DEFAULT_HNSW })], Some("id")).unwrap();
    let opts = DbOptions { sync: false, segment_max_docs: n.max(1), ..DbOptions::default() };
    let db = Db::create(&dir.join(format!("v{n}.strata")), schema, opts).unwrap();
    db.load(vecs.iter().enumerate().map(|(i, v)| vec![Value::Int(i as i64), Value::Vector(Arc::from(v.as_slice()))])).unwrap();
    db.optimize().unwrap();
    db
}

fn query(db: &Db, sql: &str, p: &Params) -> ResultSet {
    db.query(sql, p).unwrap_or_else(|e| panic!("{sql}: {e}"))
}

fn ids(r: &ResultSet) -> Vec<i64> {
    r.rows.iter().map(|r| r[0].as_i64().unwrap()).collect()
}

/// The true top `k` by dot product.
fn truth(vecs: &[Vec<f32>], q: &[f32], k: usize) -> Vec<i64> {
    let mut all: Vec<(f32, i64)> = vecs.iter().enumerate().map(|(i, v)| (v.iter().zip(q).map(|(a, b)| a * b).sum(), i as i64)).collect();
    all.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    all.into_iter().take(k).map(|x| x.1).collect()
}

/// Mean and worst recall@10 of `opts` over `qs`, and the median latency
/// in milliseconds.
fn measure(db: &Db, vecs: &[Vec<f32>], qs: &[Vec<f32>], opts: &str) -> (f64, f64, f64) {
    let sql = format!("SELECT id FROM t ORDER BY similarity(emb, :q{opts}) DESC LIMIT 10");
    let (mut sum, mut worst, mut ms) = (0.0, 1.0f64, Vec::new());
    for q in qs {
        let p = Params::new().set("q", Value::Vector(Arc::from(q.as_slice())));
        let t = Instant::now();
        let got = ids(&query(db, &sql, &p));
        ms.push(t.elapsed().as_secs_f64() * 1e3);
        let want = truth(vecs, q, 10);
        let r = got.iter().filter(|g| want.contains(g)).count() as f64 / 10.0;
        sum += r;
        worst = worst.min(r);
    }
    ms.sort_by(f64::total_cmp);
    (sum / qs.len() as f64, worst, ms[ms.len() / 2])
}

#[test]
fn default_search_finds_95_percent_of_the_true_top_10() {
    // Before: 0.73 (worst query 0.3) with the index, where the exact scan
    // cost about the same.
    let dir = tempfile::tempdir().unwrap();
    let mut rng = rand::rngs::StdRng::seed_from_u64(1);
    let vecs: Vec<Vec<f32>> = (0..20_000).map(|_| unit(&mut rng)).collect();
    let qs: Vec<Vec<f32>> = (0..50).map(|_| unit(&mut rng)).collect();
    let db = table(dir.path(), vecs.len(), &vecs);
    let (recall, worst, _) = measure(&db, &vecs, &qs, "");
    assert!(recall >= 0.95, "default recall@10 {recall} (worst {worst})");
    // The index itself, forced, at the default breadth (ef 200; at 64 it
    // found 0.73).
    let (recall, worst, _) = measure(&db, &vecs, &qs, ", strategy => 'index'");
    assert!(recall >= 0.9, "index recall@10 {recall} (worst {worst})");
}

#[test]
#[ignore]
fn vector_defaults_report() {
    let sizes: Vec<usize> = std::env::var("CHLOEDB_VEC_ROWS").map(|s| s.split(',').map(|x| x.parse().unwrap()).collect()).unwrap_or(vec![20_000, 200_000]);
    for n in sizes {
        let dir = tempfile::tempdir().unwrap();
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        let vecs: Vec<Vec<f32>> = (0..n).map(|_| unit(&mut rng)).collect();
        let qs: Vec<Vec<f32>> = (0..200).map(|_| unit(&mut rng)).collect();
        let t = Instant::now();
        let db = table(dir.path(), n, &vecs);
        println!("{n} rows x {DIM} d: built in {:.1} s", t.elapsed().as_secs_f64());
        for opts in ["", ", strategy => 'flat'", ", strategy => 'index', ef => 64", ", strategy => 'index', ef => 128", ", strategy => 'index', ef => 200", ", strategy => 'index'", ", strategy => 'index', ef => 400"] {
            let _ = measure(&db, &vecs, &qs[..20], opts);
            let (r, w, ms) = measure(&db, &vecs, &qs, opts);
            println!("  {n} {:<36} recall@10 {r:.3} worst {w:.1} median {ms:.3} ms", if opts.is_empty() { "default" } else { opts });
        }
    }
}

#[test]
fn distance_ranks_by_euclidean_distance() {
    // Before: ORDER BY distance(..) ASC gave the rows and scores of
    // similarity(..) DESC, the largest dot product.
    let dir = tempfile::tempdir().unwrap();
    let mut rng = rand::rngs::StdRng::seed_from_u64(3);
    // Lengths from 0.1 to 10: long vectors win the dot product.
    let vecs: Vec<Vec<f32>> = (0..3000).map(|_| {
        let s = 10f32.powf(rng.gen_range(-1.0..1.0));
        unit(&mut rng).into_iter().map(|x| x * s).collect()
    }).collect();
    let db = table(dir.path(), vecs.len(), &vecs);
    for _ in 0..10 {
        let q = unit(&mut rng);
        let p = Params::new().set("q", Value::Vector(Arc::from(q.as_slice())));
        let mut by_l2: Vec<(f64, i64)> = vecs.iter().enumerate().map(|(i, v)| (v.iter().zip(&q).map(|(a, b)| ((a - b) as f64).powi(2)).sum::<f64>().sqrt(), i as i64)).collect();
        by_l2.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let r = query(&db, "SELECT id, score() FROM t ORDER BY distance(emb, :q) ASC LIMIT 5", &p);
        assert_eq!(ids(&r), by_l2[..5].iter().map(|x| x.1).collect::<Vec<_>>(), "{:?}", r.stats.plan);
        for (row, (d, _)) in r.rows.iter().zip(&by_l2) {
            let s = row[1].as_f64().unwrap();
            assert!((s + d).abs() < 1e-4, "score() is minus the distance: {s} vs {d}");
        }
        // The dot product ranks other rows first on these vectors.
        let sim = query(&db, "SELECT id FROM t ORDER BY similarity(emb, :q) DESC LIMIT 5", &p);
        assert_ne!(ids(&sim), ids(&r));
        // A WHERE applies.
        let r = query(&db, "SELECT id FROM t WHERE id < 1000 ORDER BY distance(emb, :q) ASC LIMIT 5", &p);
        let want: Vec<i64> = by_l2.iter().filter(|x| x.1 < 1000).take(5).map(|x| x.1).collect();
        assert_eq!(ids(&r), want);
    }
}
