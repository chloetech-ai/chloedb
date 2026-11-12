//! SPARSE fields (learned term weights): top-k by the dot product equal to
//! brute force over the quantised weights for every top-k algorithm, with
//! filters, over several segments and merged shards; in fusions and
//! products; SQL writes; and the errors.

use std::collections::HashMap;
use std::sync::Arc;

use rand::{Rng, SeedableRng};
use chloedb::cluster;
use chloedb::db::{Db, SqlResult};
use chloedb::query::exec::{ExecOptions, ResultSet};
use chloedb::query::stats::TermStats;
use chloedb::query::{bind, Params};
use chloedb::storage::DbOptions;
use chloedb::text::sparse::{quantise, to_value, SCALE};
use chloedb::{Field, FieldType, Schema, Value};

const N: usize = 3000;

fn schema() -> Schema {
    Schema::new(
        "docs",
        vec![
            Field::new("id", FieldType::Keyword),
            Field::new("year", FieldType::Int),
            Field::new("body", FieldType::Text { positions: false }),
            Field::new("splade", FieldType::Sparse),
        ],
        Some("id"),
    )
    .unwrap()
}

fn term(i: usize) -> String {
    format!("t{i}")
}

/// Documents with 10 to 60 terms each from a skewed vocabulary of 1,500,
/// weights up to 3.
fn docs(seed: u64) -> (Vec<Vec<Value>>, Vec<HashMap<String, f32>>) {
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let mut out = Vec::new();
    let mut weights = Vec::new();
    for i in 0..N {
        let mut w: HashMap<String, f32> = HashMap::new();
        for _ in 0..rng.gen_range(10..60) {
            let t = (rng.gen::<f64>().powi(3) * 1500.0) as usize;
            w.insert(term(t), rng.gen_range(0.0f32..3.0));
        }
        let pairs: Vec<(String, f32)> = w.iter().map(|(t, w)| (t.clone(), *w)).collect();
        let body = pairs.iter().take(5).map(|(t, _)| t.as_str()).collect::<Vec<_>>().join(" ");
        out.push(vec![Value::Str(format!("d{i:05}")), Value::Int(2000 + (i % 20) as i64), Value::Str(body), to_value(&pairs)]);
        weights.push(w);
    }
    (out, weights)
}

fn open(dir: &std::path::Path, name: &str, docs: &[Vec<Value>]) -> Db {
    let db = Db::create(&dir.join(name), schema(), DbOptions { segment_max_docs: 700, ..DbOptions::default() }).unwrap();
    db.load(docs.iter().cloned()).unwrap();
    db
}

fn query(rng: &mut impl Rng) -> Vec<(String, f32)> {
    (0..rng.gen_range(3..25)).map(|_| (term((rng.gen::<f64>().powi(2) * 1500.0) as usize), rng.gen_range(0.05f32..2.0))).collect()
}

fn json(q: &[(String, f32)]) -> Value {
    // Repeated query terms add up.
    let mut m: HashMap<&str, f32> = HashMap::new();
    for (t, w) in q {
        *m.entry(t).or_default() += w;
    }
    let pairs: Vec<(String, f32)> = m.into_iter().map(|(t, w)| (t.to_string(), w)).collect();
    to_value(&pairs)
}

/// Brute-force scores over the quantised weights, best first (ties by key).
fn brute(weights: &[HashMap<String, f32>], q: &[(String, f32)], keep: impl Fn(usize) -> bool) -> Vec<(String, f64)> {
    let mut m: HashMap<&str, f32> = HashMap::new();
    for (t, w) in q {
        *m.entry(t).or_default() += w;
    }
    let mut all: Vec<(String, f64)> = weights
        .iter()
        .enumerate()
        .filter(|(i, _)| keep(*i))
        .map(|(i, d)| {
            let s: f64 = m.iter().map(|(t, qw)| d.get(*t).map_or(0.0, |w| *qw as f64 * (quantise(*w) as f64 / SCALE as f64))).sum();
            (format!("d{i:05}"), s)
        })
        .filter(|(_, s)| *s > 0.0)
        .collect();
    all.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap().then_with(|| a.0.cmp(&b.0)));
    all
}

fn keys(r: &ResultSet) -> Vec<String> {
    r.keys.iter().map(|k| k.as_str().unwrap().to_string()).collect()
}

/// `got` holds the best `k` of `want` up to ties at the boundary, each
/// with its exact score.
fn check(got: &ResultSet, want: &[(String, f64)], k: usize, what: &str) {
    let want_score: HashMap<&str, f64> = want.iter().map(|(d, s)| (d.as_str(), *s)).collect();
    assert_eq!(got.keys.len(), k.min(want.len()), "{what}");
    for (key, s) in keys(got).iter().zip(&got.scores) {
        let w = want_score.get(key.as_str()).copied().unwrap_or(0.0);
        assert!((s - w).abs() < 1e-4 * w.max(1.0), "{what}: {key} scored {s}, brute force {w}");
    }
    if let (Some(last), Some(cut)) = (got.scores.last(), want.get(k.min(want.len()).saturating_sub(1))) {
        assert!((last - cut.1).abs() < 1e-4 * cut.1.max(1.0), "{what}: k-th score {last} vs {}", cut.1);
    }
}

#[test]
fn sparse_top_k_equals_brute_force() {
    let (d, w) = docs(1);
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path(), "s.strata", &d);
    let mut rng = rand::rngs::StdRng::seed_from_u64(2);
    for i in 0..30 {
        let q = query(&mut rng);
        let p = Params::new().set("q", json(&q));
        for algo in ["exhaustive", "maxscore", "bmw"] {
            let sql = format!("SELECT id, score() FROM docs ORDER BY similarity(splade, :q, algo => '{algo}') DESC LIMIT 10");
            let r = db.query(&sql, &p).unwrap();
            check(&r, &brute(&w, &q, |_| true), 10, &format!("query {i} {algo}"));
            assert!(r.stats.plan.iter().any(|l| l.contains("sparse")), "{:?}", r.stats.plan);
            let sql = format!("SELECT id, score() FROM docs WHERE year >= 2015 ORDER BY similarity(splade, :q, algo => '{algo}') DESC LIMIT 7");
            let r = db.query(&sql, &p).unwrap();
            check(&r, &brute(&w, &q, |j| 2000 + j % 20 >= 2015), 7, &format!("query {i} {algo} filtered"));
        }
    }
    // A query with no known term finds nothing.
    let r = db.query("SELECT id FROM docs ORDER BY similarity(splade, :q) DESC LIMIT 5", &Params::new().set("q", Value::Str(r#"{"zzz": 1}"#.into()))).unwrap();
    assert!(r.rows.is_empty());
}

/// The coordinator's two rounds over `shards`.
fn sharded(shards: &[&Db], sql: &str, params: &Params) -> ResultSet {
    let snaps: Vec<_> = shards.iter().map(|d| d.snapshot()).collect();
    let select = bind(sql, snaps[0].schema(), params).unwrap();
    let parts: Vec<TermStats> = shards.iter().zip(&snaps).map(|(d, s)| d.term_stats(s, &select).unwrap()).collect();
    let opts = ExecOptions { stats: Some(Arc::new(TermStats::sum(&parts))), shard: true, ..ExecOptions::default() };
    let answers = shards.iter().zip(&snaps).map(|(d, s)| d.execute(s, &select, &opts).unwrap()).collect();
    cluster::merge(&select, answers).unwrap()
}

#[test]
fn shards_fusions_and_products_with_sparse_scores() {
    let (d, _) = docs(3);
    let dir = tempfile::tempdir().unwrap();
    let whole = open(dir.path(), "whole.strata", &d);
    let a = open(dir.path(), "a.strata", &d[..N / 2]);
    let b = open(dir.path(), "b.strata", &d[N / 2..]);
    let mut rng = rand::rngs::StdRng::seed_from_u64(4);
    for _ in 0..5 {
        let q = query(&mut rng);
        let text = q.iter().take(3).map(|(t, _)| t.as_str()).collect::<Vec<_>>().join(" ");
        let p = Params::new().set("q", json(&q)).set("t", Value::Str(text));
        for sql in [
            "SELECT id, score() FROM docs ORDER BY similarity(splade, :q) DESC LIMIT 10",
            "SELECT id, score() FROM docs WHERE year < 2010 ORDER BY similarity(splade, :q) DESC LIMIT 10 OFFSET 3",
            "SELECT id, score() FROM docs ORDER BY rrf(bm25(body, :t), similarity(splade, :q)) DESC LIMIT 10",
            "SELECT id, score() FROM docs ORDER BY 0.5 * minmax(bm25(body, :t)) + 0.5 * minmax(similarity(splade, :q)) DESC LIMIT 10",
            "SELECT id, score() FROM docs ORDER BY similarity(splade, :q) * decay(year, 2019, 5) DESC LIMIT 10",
        ] {
            let want = whole.query(sql, &p).unwrap();
            let got = sharded(&[&a, &b], sql, &p);
            assert_eq!(keys(&want), keys(&got), "{sql}");
            for (x, y) in want.scores.iter().zip(&got.scores) {
                assert!((x - y).abs() < 1e-5 * x.abs().max(1.0), "{sql}: {x} vs {y}");
            }
            assert!(!want.rows.is_empty(), "{sql}");
        }
    }
}

#[test]
fn sparse_values_through_sql_writes_and_errors() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path(), "w.strata", &docs(5).0[..100]);
    let n = db.sql("INSERT INTO docs (id, year, body, splade) VALUES ('new', 2024, 'fresh', '{\"rare\": 2.5, \"t1\": 0.1}')", &Params::new()).unwrap();
    assert!(matches!(n, SqlResult::Written(1)), "{n:?}");
    let r = db.query("SELECT id, splade, score() FROM docs ORDER BY similarity(splade, :q) DESC LIMIT 1", &Params::new().set("q", Value::Str(r#"{"rare": 2}"#.into()))).unwrap();
    assert_eq!(keys(&r), vec!["new"]);
    assert!((r.scores[0] - 5.0).abs() < 1e-6, "{}", r.scores[0]);
    let back = chloedb::text::sparse::parse(&r.rows[0][1]).unwrap();
    assert_eq!(back, vec![("rare".to_string(), 2.5), ("t1".to_string(), 0.1)]);

    for (sql, msg) in [
        ("SELECT id FROM docs ORDER BY distance(splade, :q) ASC", "not distance"),
        ("SELECT id FROM docs ORDER BY similarity(splade, '[1, 2]') DESC", "JSON object"),
        ("SELECT id FROM docs ORDER BY similarity(splade, '{\"a\": -1}') DESC", "from 0"),
        ("SELECT id FROM docs ORDER BY similarity(year, '{\"a\": 1}') DESC", "VECTOR or SPARSE"),
        ("INSERT INTO docs (id, splade) VALUES ('bad', 'a:1')", "JSON object"),
    ] {
        let p = Params::new().set("q", Value::Str(r#"{"a": 1}"#.into()));
        let e = if sql.starts_with("INSERT") { db.sql(sql, &p).err() } else { db.query(sql, &p).err() };
        let e = e.unwrap_or_else(|| panic!("{sql} was accepted")).to_string();
        assert!(e.contains(msg), "{sql}: {e}");
    }
    let bad = vec![Value::Str("x".into()), Value::Int(1), Value::Null, Value::Str(r#"{"a": "b"}"#.into())];
    assert!(db.load([bad]).is_err());
}

/// A sparse score boosted by a row's values ranks by its exact top k, the
/// sparse index's bounds driving the expression (rank-safe boosts): the
/// same rows, bit for bit, as scoring every matching row.
#[test]
fn a_boosted_sparse_score_ranks_every_match() {
    let (d, _) = docs(5);
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path(), "b.strata", &d);
    let mut rng = rand::rngs::StdRng::seed_from_u64(6);
    let run = |sql: &str, params: &Params, opts: &ExecOptions| {
        let snap = db.snapshot();
        let select = bind(sql, snap.schema(), params).unwrap_or_else(|e| panic!("{sql}: {e}"));
        db.execute(&snap, &select, opts).unwrap_or_else(|e| panic!("{sql}: {e}"))
    };
    let reference = ExecOptions { rank_safe: false, ..ExecOptions::default() };
    for i in 0..60 {
        let params = Params::new().set("w", json(&query(&mut rng)));
        let s = format!("similarity(splade, :w{}, depth => 100000)", ["", ", algo => 'maxscore'", ", algo => 'bmw'"][i % 3]);
        let expr = [format!("{s} * (1 + (year - 2000) / 10)"), format!("{s} + 0.05 * (year - 2000)"), format!("ln(1 + {s}) * CASE WHEN year > 2010 THEN 2 ELSE 1 END")][i / 3 % 3].clone();
        let filter = ["", "WHERE year < 2015"][i % 2];
        let sql = format!("SELECT id, score() FROM docs {filter} ORDER BY {expr} DESC LIMIT {}", [1, 10, 100][i % 3]);
        let want = run(&sql, &params, &reference);
        let got = run(&sql, &params, &ExecOptions::default());
        assert_eq!(keys(&got), keys(&want), "{sql}");
        assert_eq!(got.scores.iter().map(|s| s.to_bits()).collect::<Vec<_>>(), want.scores.iter().map(|s| s.to_bits()).collect::<Vec<_>>(), "{sql}");
        let plan = got.stats.plan.join("\n");
        assert!(plan.contains("sparse") && plan.contains("ranks by the expression") && plan.contains("rank-safe"), "{sql}:\n{plan}");
    }
}

/// A sparse term holding `\0` was accepted, and the next flush panicked
/// building its dictionary: on the flusher thread, which then stopped for
/// good, and again at every reopen, which replays the row (audit S-1).
/// It is refused when written, by SQL, a transaction or a load, and
/// flushing goes on.
#[test]
fn a_sparse_term_with_a_zero_byte_is_refused_and_flushing_goes_on() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("z.strata");
    let opts = DbOptions { memtable_max_docs: 50, memtable_chunk_docs: 16, segment_max_docs: 700, sync: false, ..DbOptions::default() };
    let db = Db::create(&path, schema(), opts.clone()).unwrap();
    db.spawn_flusher();
    let bad = r#"{"a\u0000b": 1, "ok": 2}"#;
    let e = db.sql(&format!("INSERT INTO docs (id, year, splade) VALUES ('z', 1, '{bad}')"), &Params::new()).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(e.contains("\\0"), "{e:?}");
    let row = |id: &str, s: &str| vec![Value::Str(id.into()), Value::Int(1), Value::Null, Value::Str(s.into())];
    let stored = {
        let mut t = db.storage().begin().unwrap();
        match t.insert(row("z", bad)) {
            Err(_) => false,
            Ok(_) => t.commit().is_ok(),
        }
    };
    assert!(!stored, "a transaction stored the term");
    assert!(db.load([row("z", bad)]).is_err());
    // Enough rows to fill the memtable twice over: the flusher keeps up.
    for i in 0..200 {
        db.sql(&format!("INSERT INTO docs (id, year, splade) VALUES ('d{i}', 1, '{{\"ok\": 1, \"t{i}\": 0.5}}')"), &Params::new()).unwrap();
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while db.storage().file_stats().memtable_docs > 50 {
        assert!(std::time::Instant::now() < deadline, "the flusher did not catch up");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(db.storage().flush_error().is_none());
    db.storage().flush().unwrap();
    let q = Params::new().set("q", Value::Str(r#"{"ok": 1}"#.into()));
    let r = db.query("SELECT id FROM docs ORDER BY similarity(splade, :q) DESC LIMIT 500", &q).unwrap();
    assert_eq!(r.rows.len(), 200);
    drop(db);
    let db = Db::open(&path, opts).unwrap();
    assert_eq!(db.query("SELECT id FROM docs ORDER BY similarity(splade, :q) DESC LIMIT 500", &q).unwrap().rows.len(), 200);
}

/// A row written before terms were checked may hold a term with `\0`, in
/// a log no flush could empty (each one panicked). It is still a valid
/// row: the log replays whole, with the writes after it (a row replay did
/// not take would make the log damaged from there on), the flush that
/// follows leaves that term out of the index, and the row reads as it was
/// written.
#[test]
fn a_logged_row_with_a_zero_byte_term_replays_and_flushes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.strata");
    let wal = dir.path().join("old.strata-wal");
    let opts = DbOptions { memtable_max_docs: 1_000_000, sync: true, ..DbOptions::default() };
    let db = Db::create(&path, schema(), opts.clone()).unwrap();
    // The row as an earlier version logged it: written with a stand-in of
    // the escape's length, which is then changed in the log.
    let (stand_in, escape) = (r#""aQQQQQQb""#, r#""a\u0000b""#);
    db.sql(&format!("INSERT INTO docs (id, year, splade) VALUES ('old', 1, '{{{stand_in}: 1, \"ok\": 2}}')"), &Params::new()).unwrap();
    for i in 0..5 {
        db.sql(&format!("INSERT INTO docs (id, year, splade) VALUES ('d{i}', 1, '{{\"ok\": 1}}')"), &Params::new()).unwrap();
    }
    // The files as a crash now would leave them.
    let copy = dir.path().join("copy.strata");
    let copy_wal = dir.path().join("copy.strata-wal");
    std::fs::copy(&path, &copy).unwrap();
    let mut log = std::fs::read(&wal).unwrap();
    let at = log.windows(stand_in.len()).position(|w| w == stand_in.as_bytes()).expect("the row in the log");
    log[at..at + stand_in.len()].copy_from_slice(escape.as_bytes());
    // Each record's checksum, after the log's 16-byte header.
    let mut pos = 16;
    while pos + 8 <= log.len() {
        let len = u32::from_le_bytes(log[pos..pos + 4].try_into().unwrap()) as usize;
        let Some(end) = (pos + 8).checked_add(len).filter(|&e| e <= log.len()) else { break };
        let crc = crc32fast::hash(&log[pos + 8..end]);
        log[pos + 4..pos + 8].copy_from_slice(&crc.to_le_bytes());
        pos = end;
    }
    std::fs::write(&copy_wal, &log).unwrap();
    drop(db);

    let ids = |db: &Db| {
        let q = Params::new().set("q", Value::Str(r#"{"ok": 1}"#.into()));
        let mut ids = keys(&db.query("SELECT id FROM docs ORDER BY similarity(splade, :q) DESC LIMIT 100", &q).unwrap());
        ids.sort();
        ids
    };
    let want: Vec<String> = ["d0", "d1", "d2", "d3", "d4", "old"].iter().map(|s| s.to_string()).collect();
    let db = Db::open(&copy, opts.clone()).unwrap();
    assert!(db.storage().damaged().is_none(), "{:?}", db.storage().damaged());
    assert_eq!(ids(&db), want, "from the log");
    db.storage().flush().unwrap();
    assert_eq!(ids(&db), want, "from the segment");
    let r = db.query("SELECT splade FROM docs WHERE id = 'old'", &Params::new()).unwrap();
    let stored = chloedb::text::sparse::parse_stored(&r.rows[0][0]).unwrap();
    assert_eq!(stored, vec![("a\0b".to_string(), 1.0), ("ok".to_string(), 2.0)]);
    // The row can be written again without the term, or with another
    // column changed; the term itself is refused in a new value.
    db.sql("UPDATE docs SET year = 2 WHERE id = 'old'", &Params::new()).unwrap();
    assert!(db.sql(&format!("UPDATE docs SET splade = '{{{escape}: 1}}' WHERE id = 'old'"), &Params::new()).is_err());
    db.storage().flush().unwrap();
    db.storage().check().unwrap();
    drop(db);
    assert_eq!(ids(&Db::open(&copy, opts).unwrap()), want);
}
