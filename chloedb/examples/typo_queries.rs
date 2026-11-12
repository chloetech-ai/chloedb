//! Typo queries on a table of passages, text only, as the engine
//! comparison (`chloedb-server/src/bin/engine-compare`) asks them: each
//! query with two letters of its longest word swapped, ranked by
//! `bm25(body, :x, syntax => 'plain', fuzzy => 'auto')`.
//!
//! `build` loads `{"id", "title", "text"}` lines into one segment.
//! `quality` takes `id<TAB>query` lines (MS MARCO's dev queries) and
//! reports, against each query's top 10 by plain `bm25` as written, the
//! share the typed query finds by plain `bm25` and with `fuzzy => 'auto'`
//! (recovered), and the share `fuzzy => 'auto'` keeps of the query as
//! written (kept), with each fuzzy query's time. `walk` times the
//! dictionary walks alone, per word of one edit and of two.
//!
//! ```text
//! cargo run --release --example typo_queries -- build passages.jsonl docs.chloedb
//! cargo run --release --example typo_queries -- quality docs.chloedb queries.tsv [n]
//! cargo run --release --example typo_queries -- walk docs.chloedb queries.tsv [n]
//! ```

use std::io::BufRead;
use std::path::Path;
use std::time::Instant;

use serde_json::Value as Json;
use chloedb::db::Db;
use chloedb::metrics::percentile;
use chloedb::query::Params;
use chloedb::storage::DbOptions;
use chloedb::value::Value;
use chloedb::{Field, FieldType, Schema};

fn schema() -> Schema {
    Schema::new(
        "docs",
        vec![
            Field::new("id", FieldType::Keyword),
            Field::new("title", FieldType::Text { positions: false }).stored_only(),
            Field::new("text", FieldType::Text { positions: false }).stored_only(),
            Field::new("body", FieldType::Text { positions: true }).not_stored().generated(&["title", "text"]),
        ],
        Some("id"),
    )
    .unwrap()
}

fn build(docs: &str, out: &str) {
    let mut rows = Vec::new();
    for line in std::io::BufReader::new(std::fs::File::open(docs).unwrap()).lines() {
        let j: Json = serde_json::from_str(&line.unwrap()).unwrap();
        let s = |k: &str| Value::Str(j[k].as_str().unwrap_or("").to_string());
        rows.push(vec![s("id"), s("title"), s("text"), Value::Null]);
    }
    let opts = DbOptions { segment_max_docs: rows.len().max(1), ..DbOptions::default() };
    let t = Instant::now();
    let n = rows.len();
    Db::create(Path::new(out), schema(), opts).unwrap().load(rows).unwrap();
    eprintln!("{n} passages loaded in {:.1} s", t.elapsed().as_secs_f64());
}

fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(|w| w.to_lowercase()).collect()
}

/// The engine comparison's typo: the second and third letters of the
/// longest word of 5 letters or more swapped (none if they are the same).
fn typo_of(s: &str) -> Option<String> {
    let mut w = words(s);
    let (i, _) = w.iter().enumerate().filter(|(_, x)| x.len() >= 5 && x.is_ascii()).max_by_key(|(i, x)| (x.len(), std::cmp::Reverse(*i)))?;
    let mut b = w[i].clone().into_bytes();
    if b[1] == b[2] {
        return None;
    }
    b.swap(1, 2);
    w[i] = String::from_utf8(b).ok()?;
    Some(w.join(" "))
}

/// The first `n` queries that take a typo: each as written and typed.
fn queries(path: &str, n: usize) -> Vec<(String, String)> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter_map(|l| l.split_once('\t').map(|(_, q)| q.to_string()))
        .filter(|q| !q.trim().is_empty())
        .take(n)
        .filter_map(|q| typo_of(&q).map(|t| (words(&q).join(" "), t)))
        .collect()
}

fn quality(db: &str, path: &str, n: usize) {
    let db = Db::open(Path::new(db), DbOptions::default()).unwrap();
    let pairs = queries(path, n);
    let plain = "SELECT id FROM docs ORDER BY bm25(body, :x, syntax => 'plain') DESC LIMIT 10";
    let fuzzy = "SELECT id FROM docs ORDER BY bm25(body, :x, syntax => 'plain', fuzzy => 'auto') DESC LIMIT 10";
    let run = |sql: &str, x: &str| -> (Vec<String>, f64) {
        let mut p = Params::new();
        p.0.insert("x".into(), Value::Str(x.to_string()));
        let t = Instant::now();
        let r = db.query(sql, &p).unwrap();
        (r.keys.iter().map(|k| format!("{k:?}")).collect(), t.elapsed().as_secs_f64() * 1e3)
    };
    let share = |a: &[String], b: &[String]| if a.is_empty() { 1.0 } else { b.iter().filter(|x| a.contains(x)).count() as f64 / a.len() as f64 };
    let (mut typed_plain, mut typed_fuzzy, mut kept) = (0.0, 0.0, 0.0);
    let (mut ms_typed, mut ms_written) = (Vec::new(), Vec::new());
    for (written, typed) in &pairs {
        let (base, _) = run(plain, written);
        typed_plain += share(&base, &run(plain, typed).0);
        let (top, t) = run(fuzzy, typed);
        typed_fuzzy += share(&base, &top);
        ms_typed.push(t);
        let (top, t) = run(fuzzy, written);
        kept += share(&base, &top);
        ms_written.push(t);
    }
    let m = pairs.len() as f64;
    println!("{} queries with a typo", pairs.len());
    println!("recovered: plain {:.3}, fuzzy 'auto' {:.3}; kept {:.3}", typed_plain / m, typed_fuzzy / m, kept / m);
    println!("fuzzy 'auto', typed:   p50 {:.2} p90 {:.2} p99 {:.2} ms", percentile(&ms_typed, 50.0), percentile(&ms_typed, 90.0), percentile(&ms_typed, 99.0));
    println!("fuzzy 'auto', written: p50 {:.2} p90 {:.2} p99 {:.2} ms", percentile(&ms_written, 50.0), percentile(&ms_written, 90.0), percentile(&ms_written, 99.0));
}

fn walk(db: &str, path: &str, n: usize) {
    let db = Db::open(Path::new(db), DbOptions::default()).unwrap();
    let readers = db.readers(&db.snapshot()).unwrap();
    let body = 3;
    let (mut one, mut two, mut found) = (Vec::new(), Vec::new(), 0usize);
    for (_, typed) in queries(path, n) {
        for term in chloedb::text::analyze_query(&typed) {
            let e = chloedb::text::query::auto_edits(term.chars().count());
            if e == 0 {
                continue;
            }
            let t = Instant::now();
            for r in &readers {
                if let Some(tr) = r.text(body).unwrap() {
                    found += tr.fuzzy_terms(&term, e).len();
                }
            }
            let x = t.elapsed().as_secs_f64() * 1e3;
            if e == 1 {
                one.push(x)
            } else {
                two.push(x)
            }
        }
    }
    let words = (one.len() + two.len()).max(1);
    println!("{} segments, {} words, {:.1} terms found per word", readers.len(), words, found as f64 / words as f64);
    println!("1 edit  ({:4} words): p50 {:.2} p90 {:.2} ms", one.len(), percentile(&one, 50.0), percentile(&one, 90.0));
    println!("2 edits ({:4} words): p50 {:.2} p90 {:.2} ms", two.len(), percentile(&two, 50.0), percentile(&two, 90.0));
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let usage = "typo_queries build DOCS.jsonl OUT | quality DB QUERIES.tsv [n] | walk DB QUERIES.tsv [n]";
    let n = || a.get(4).map_or(300, |x| x.parse().expect(usage));
    match (a.get(1).map(String::as_str), a.get(2), a.get(3)) {
        (Some("build"), Some(docs), Some(out)) => build(docs, out),
        (Some("quality"), Some(db), Some(q)) => quality(db, q, n()),
        (Some("walk"), Some(db), Some(q)) => walk(db, q, n()),
        _ => panic!("{usage}"),
    }
}
