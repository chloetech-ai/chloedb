//! Query understanding: expansion by pseudo-relevance feedback
//! (`bm25(.., expand => n)`, [`chloedb::query::prf`]) and "did you mean"
//! ([`Db::correct`]).

use chloedb::db::Db;
use chloedb::query::Params;
use chloedb::schema::{Field, FieldType, Schema};
use chloedb::storage::DbOptions;
use chloedb::value::Value;

fn db(dir: &tempfile::TempDir) -> Db {
    let schema = Schema::new(
        "papers",
        vec![Field::new("id", FieldType::Keyword), Field::new("body", FieldType::Text { positions: false }), Field::new("year", FieldType::Int)],
        Some("id"),
    )
    .unwrap();
    let db = Db::create(&dir.path().join("db.strata"), schema, DbOptions::default()).unwrap();
    let mut rows = Vec::new();
    // Vaccine papers say "immune" and "antibodies" often; one paper about
    // antibodies never says "vaccine".
    for i in 0..30 {
        rows.push(vec![Value::Str(format!("v{i}")), Value::Str(format!("mRNA vaccines raise antibodies and an immune response, trial {i}")), Value::Int(2015 + (i % 10) as i64)]);
    }
    rows.push(vec![Value::Str("ab".into()), Value::Str("antibodies of the immune system in mice".into()), Value::Int(2024)]);
    for i in 0..30 {
        rows.push(vec![Value::Str(format!("o{i}")), Value::Str(format!("graph neural networks for molecules, study {i}")), Value::Int(2020)]);
    }
    db.load(rows.into_iter()).unwrap();
    db
}

fn ids(db: &Db, sql: &str, q: &str) -> (Vec<String>, Vec<String>) {
    let r = db.query(sql, &Params::new().set("q", Value::Str(q.into()))).unwrap_or_else(|e| panic!("{sql}: {e}"));
    (r.rows.iter().map(|r| r[0].as_str().unwrap().to_string()).collect(), r.stats.plan.clone())
}

#[test]
fn expand_finds_rows_with_the_words_of_the_top_rows() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(&dir);
    // Plain BM25 for "vaccines" never reaches the antibody paper.
    let (plain, _) = ids(&db, "SELECT id FROM papers WHERE MATCH(body, 'vaccines OR antibodies OR immune') ORDER BY bm25(body, :q) DESC LIMIT 100", "vaccines");
    assert!(!plain.contains(&"ab".to_string()), "{plain:?}");
    let (expanded, plan) = ids(&db, "SELECT id FROM papers ORDER BY bm25(body, :q, expand => 5) DESC LIMIT 100", "vaccines");
    assert!(expanded.contains(&"ab".to_string()), "{expanded:?}");
    assert!(!expanded.iter().any(|i| i.starts_with('o')), "{expanded:?}");
    assert!(plan.iter().any(|l| l.starts_with("bm25(body) expanded from 5 rows of papers with") && l.contains("immun")), "{plan:?}");
    // The vaccine papers still rank first: the query keeps most of the weight.
    assert!(expanded[0].starts_with('v'), "{expanded:?}");
}

#[test]
fn expand_reads_its_rows_through_the_query_filter_and_takes_parameters() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(&dir);
    let p = Params::new().set("q", Value::Str("vaccines".into())).set("n", Value::Int(3));
    let r = db.query("SELECT id, year FROM papers WHERE year >= 2023 ORDER BY bm25(body, :q, expand => :n, expand_terms => 4, expand_weight => 0.5) DESC LIMIT 50", &p).unwrap();
    assert!(r.rows.iter().all(|row| row[1].as_i64().unwrap() >= 2023), "{:?}", r.rows);
    assert!(r.stats.plan.iter().any(|l| l.starts_with("bm25(body) expanded from 3 rows")), "{:?}", r.stats.plan);
    // expand => 0 is the query as written.
    let a = db.query("SELECT id FROM papers ORDER BY bm25(body, :q, expand => 0) DESC LIMIT 5", &p).unwrap();
    let b = db.query("SELECT id FROM papers ORDER BY bm25(body, :q) DESC LIMIT 5", &p).unwrap();
    assert_eq!(a.rows, b.rows);
}

#[test]
fn expand_refuses_what_it_cannot_expand() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(&dir);
    let p = Params::new().set("q", Value::Str("vaccines".into()));
    for (sql, says) in [
        ("SELECT id FROM papers ORDER BY bm25(body, :q, expand => 5, min => 2) DESC", "takes no min"),
        ("SELECT id FROM papers ORDER BY bm25(body, :q, expand => 500) DESC", "from 0 to 100"),
        ("SELECT id FROM papers ORDER BY bm25(body, :q, expand => 5, expand_weight => 2) DESC", "from 0 to 1"),
    ] {
        let e = db.query(sql, &p).unwrap_err().to_string();
        assert!(e.contains(says), "{sql}: {e}");
    }
}

#[test]
fn terms_syntax_ranks_analysed_weighted_terms() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(&dir);
    let r = db.query("SELECT id FROM papers ORDER BY bm25(body, 'antibodi^1', syntax => 'terms') DESC LIMIT 100", &Params::new()).unwrap();
    // "antibodi" is the analysed term: not analysed again.
    assert_eq!(r.rows.len(), 31);
    let e = db.query("SELECT id FROM papers ORDER BY bm25(body, 'x^-1', syntax => 'terms') DESC", &Params::new()).unwrap_err().to_string();
    assert!(e.contains("weight"), "{e}");
}

#[test]
fn correct_says_what_a_typed_query_meant() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(&dir);
    let c = db.correct("papers", "body", "mRNA vacines for nueral networks").unwrap();
    assert!(c.changed);
    assert_eq!(c.text, "mRNA vaccines for neural networks");
    let words: Vec<(&str, &str)> = c.words.iter().map(|w| (w.word.as_str(), w.to_word.as_str())).collect();
    assert_eq!(words, vec![("vacines", "vaccines"), ("nueral", "neural")]);
    // Words spelled right, and stopwords, stay.
    let ok = db.correct("papers", "body", "the immune response").unwrap();
    assert!(!ok.changed);
    assert_eq!(ok.text, "the immune response");
    assert!(db.correct("papers", "year", "x").is_err());
    assert!(db.correct("nope", "body", "x").is_err());

}
