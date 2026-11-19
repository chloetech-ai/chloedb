//! The search box a beginner builds from the docs, found by the persona
//! study's fourth round (19 November 2026).
//!
//! MAYA-401: the typo search the docs teach, `WHERE MATCH(f, :q, fuzzy =>
//! 'auto') ORDER BY bm25(f, :q)`, returned fewer rows than its own WHERE
//! matched when the word typed is itself a word of the index: bm25's
//! `'auto'` corrects a typo and leaves a word it knows as it is, so the
//! rows MATCH found by a neighbour ("lasagna" for "lasagne") scored
//! nothing and dropped out, and a misspelling found more than either
//! spelling. A bm25 ranking the text a fuzzy MATCH filters by now reads the
//! words as MATCH does.
//!
//! MAYA-402 (and MAYA-14): a visitor's text with a stray quote or bracket
//! was a parse error, and MATCH had no plain-text mode; the parser's errors
//! named its tokens (`unexpected RParen`). `MATCH(f, :q, syntax =>
//! 'plain')` reads the text as words alone, and the errors say what is
//! wrong in words.

use chloedb::db::Db;
use chloedb::query::exec::ResultSet;
use chloedb::query::Params;
use chloedb::storage::DbOptions;
use chloedb::value::Value;

fn ids(r: &ResultSet) -> Vec<String> {
    r.keys.iter().map(|k| k.as_str().unwrap().to_string()).collect()
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v
}

fn table(dir: &std::path::Path, rows: &[(&str, &str)]) -> Db {
    let db = Db::create_empty(&dir.join("t.strata"), DbOptions::default()).unwrap();
    let p = Params::new();
    db.sql("CREATE TABLE t (id KEYWORD PRIMARY KEY, name TEXT)", &p).unwrap();
    for (id, text) in rows {
        db.sql(&format!("INSERT INTO t (id, name) VALUES ('{id}', '{text}')"), &p).unwrap();
    }
    db
}

fn run(db: &Db, sql: &str, q: &str) -> chloedb::error::Result<ResultSet> {
    let mut p = Params::new();
    p.0.insert("q".into(), Value::Str(q.to_string()));
    db.query(sql, &p)
}

fn count(db: &Db, sql: &str, q: &str) -> i64 {
    run(db, sql, q).unwrap().rows[0][0].as_i64().unwrap()
}

/// Two names spelt "colour", five "color".
fn colours(dir: &std::path::Path) -> Db {
    table(
        dir,
        &[
            ("u1", "colour chart"),
            ("u2", "colour wheel"),
            ("o1", "color chart"),
            ("o2", "color wheel"),
            ("o3", "color pencils"),
            ("o4", "color markers"),
            ("o5", "color paper"),
        ],
    )
}

#[test]
fn a_ranked_typo_search_returns_every_row_its_where_matched() {
    let dir = tempfile::tempdir().unwrap();
    let db = colours(dir.path());
    for fuzzy in ["'auto'", "1", "2"] {
        let counted = format!("SELECT COUNT(*) FROM t WHERE MATCH(name, :q, fuzzy => {fuzzy})");
        let ranked = format!("SELECT id FROM t WHERE MATCH(name, :q, fuzzy => {fuzzy}) ORDER BY bm25(name, :q) DESC LIMIT 30");
        let both = format!("SELECT id FROM t WHERE MATCH(name, :q, fuzzy => {fuzzy}) ORDER BY bm25(name, :q, fuzzy => {fuzzy}) DESC LIMIT 30");
        for q in ["colour", "color", "colur"] {
            let n = count(&db, &counted, q);
            assert_eq!(n, 7, "{q} {fuzzy}: MATCH counts every spelling");
            let r = run(&db, &ranked, q).unwrap();
            assert_eq!(ids(&r).len() as i64, n, "{q} {fuzzy}: the ranked query returns what its WHERE matched; plan {:?}", r.stats.plan);
            let r = run(&db, &both, q).unwrap();
            assert_eq!(ids(&r).len() as i64, n, "{q} {fuzzy}, fuzzy on both: plan {:?}", r.stats.plan);
        }
    }
    // The spelling typed ranks first.
    let ranked = "SELECT id FROM t WHERE MATCH(name, :q, fuzzy => 'auto') ORDER BY bm25(name, :q) DESC LIMIT 30";
    let r = ids(&run(&db, ranked, "colour").unwrap());
    assert_eq!(sorted(r[..2].to_vec()), ["u1", "u2"], "{r:?}");
    let r = ids(&run(&db, ranked, "color").unwrap());
    assert_eq!(sorted(r[..5].to_vec()), ["o1", "o2", "o3", "o4", "o5"], "{r:?}");
    // Two words: a row has both, each as typed or near it.
    let n = count(&db, "SELECT COUNT(*) FROM t WHERE MATCH(name, :q, fuzzy => 'auto')", "colour chart");
    assert_eq!(n, 2);
    let r = ids(&run(&db, "SELECT id FROM t WHERE MATCH(name, :q, fuzzy => 'auto') ORDER BY bm25(name, :q) DESC LIMIT 30", "colour chart").unwrap());
    assert_eq!(r, ["u1", "o1"]);
}

/// A bm25 alone still corrects a typo rather than widening a word it knows
/// (`auto_typos_go_by_how_common_the_words_are` in ranking_fixes.rs): only
/// a MATCH beside it, of the same text, widens it.
#[test]
fn a_ranking_alone_still_corrects_rather_than_widens() {
    let dir = tempfile::tempdir().unwrap();
    let db = colours(dir.path());
    let alone = "SELECT id FROM t ORDER BY bm25(name, :q, fuzzy => 'auto') DESC LIMIT 30";
    assert_eq!(sorted(ids(&run(&db, alone, "color").unwrap())), ["o1", "o2", "o3", "o4", "o5"]);
    // Another text in the MATCH: the bm25 is not the MATCH's.
    let other = "SELECT id FROM t WHERE MATCH(name, 'chart', fuzzy => 'auto') ORDER BY bm25(name, :q, fuzzy => 'auto') DESC LIMIT 30";
    assert_eq!(ids(&run(&db, other, "color").unwrap()), ["o1"]);
}

/// What a visitor types: each returns rows or none, never an error.
const VISITOR: [&str; 12] = ["\"soup", "soup (easy", "soup)", "soup -", "-", "soup*", "(", ")", "\"", "soup OR", "AND soup", "soup^2 ~ )@2"];

#[test]
fn plain_syntax_reads_a_search_box_as_words() {
    let dir = tempfile::tempdir().unwrap();
    let db = table(dir.path(), &[("s1", "easy tomato soup"), ("s2", "lentil soup"), ("s3", "soupy noodles"), ("n1", "-easy- (salad)")]);
    for q in VISITOR {
        for opts in ["syntax => 'plain'", "syntax => 'plain', fuzzy => 'auto'", "fuzzy => 'auto', syntax => 'plain', min => 1"] {
            let sql = format!("SELECT id FROM t WHERE MATCH(name, :q, {opts}) ORDER BY bm25(name, :q) DESC LIMIT 10");
            if let Err(e) = run(&db, &sql, q) {
                panic!("{q:?} with {opts}: {e}");
            }
            let sql = format!("SELECT COUNT(*) FROM t WHERE MATCH(name, :q, {opts})");
            run(&db, &sql, q).unwrap_or_else(|e| panic!("{q:?} with {opts}: {e}"));
        }
    }
    let plain = |q: &str| sorted(ids(&run(&db, "SELECT id FROM t WHERE MATCH(name, :q, syntax => 'plain')", q).unwrap()));
    assert_eq!(plain("\"soup"), ["s1", "s2"]);
    assert_eq!(plain("soup (easy"), ["s1"]);
    assert_eq!(plain("soup)"), ["s1", "s2"]);
    assert_eq!(plain("soup -"), ["s1", "s2"]);
    // A minus is no NOT, nor a star a prefix, nor OR an operator.
    assert_eq!(plain("soup -easy"), ["s1"]);
    assert_eq!(plain("soup*"), ["s1", "s2"]);
    assert_eq!(plain("lentil OR tomato"), Vec::<String>::new());
    assert_eq!(plain("-"), Vec::<String>::new());
    assert_eq!(plain("(salad)"), ["n1"]);
    // With typos, and ranked by the bm25 beside it, which reads the text
    // as plain too.
    let ranked = |q: &str| ids(&run(&db, "SELECT id FROM t WHERE MATCH(name, :q, syntax => 'plain', fuzzy => 'auto') ORDER BY bm25(name, :q) DESC LIMIT 10", q).unwrap());
    assert_eq!(sorted(ranked("lentl soup)")), ["s2"]);
    assert_eq!(sorted(ranked("-easy")), ["n1", "s1"]);
    // The query syntax is the default, and says what else there is.
    assert_eq!(sorted(ids(&run(&db, "SELECT id FROM t WHERE MATCH(name, :q, syntax => 'query')", "soup -easy").unwrap())), ["s2"]);
    let e = run(&db, "SELECT id FROM t WHERE MATCH(name, :q, syntax => 'words')", "soup").unwrap_err().to_string();
    assert!(e.contains("'query'") && e.contains("'plain'"), "{e}");
}

#[test]
fn a_bad_match_query_says_what_is_wrong_in_words() {
    let dir = tempfile::tempdir().unwrap();
    let db = table(dir.path(), &[("s1", "easy tomato soup")]);
    let err = |q: &str| run(&db, "SELECT id FROM t WHERE MATCH(name, :q)", q).unwrap_err().to_string();
    for (q, says) in [
        ("\"soup", "a \" without its closing \""),
        ("soup (easy", "a ( without its )"),
        ("soup)", "a ) without its ("),
        ("soup -", "ends after -"),
        ("-", "ends after -"),
        ("soup OR", "ends after OR"),
        ("(soup OR)", "OR"),
    ] {
        let e = err(q);
        assert!(e.contains(says), "{q:?}: {e}");
        // No token names.
        for name in ["RParen", "LParen", "Tok", "Phrase(", "Word("] {
            assert!(!e.contains(name), "{q:?}: {e}");
        }
        // And the way out for a search box's text.
        assert!(e.contains("syntax => 'plain'"), "{q:?}: {e}");
    }
}

/// MAYA-405 (and MAYA-304): sqlite3's `?` and Postgres's `$1` are values
/// by position; ChloeDB's parameters are named, and the error says how.
#[test]
fn a_placeholder_by_position_says_to_name_it() {
    let dir = tempfile::tempdir().unwrap();
    let db = table(dir.path(), &[("s1", "easy tomato soup")]);
    for sql in ["SELECT id FROM t WHERE MATCH(name, ?)", "SELECT id FROM t WHERE id = $1", "SELECT id FROM t WHERE id = ?1"] {
        let e = db.query(sql, &Params::new()).unwrap_err().to_string();
        assert!(e.contains("by position") && e.contains(":q") && e.contains("q=..."), "{sql}: {e}");
    }
    let mut p = Params::new();
    p.0.insert("1".into(), Value::Str("s1".into()));
    // A value given for $1 by its number still binds it.
    assert_eq!(ids(&db.query("SELECT id FROM t WHERE id = $1", &p).unwrap()), ["s1"]);
}
