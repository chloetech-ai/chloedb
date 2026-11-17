//! Questions that used to get a wrong answer without an error, found by the
//! persona study of 17 November 2026: options a ranking function does not
//! take (they were ignored), `rrf`'s `weights` (ignored too), a prefix or
//! a typo in `bm25`'s text (it ranked no rows where `MATCH` kept some),
//! `ORDER BY` a scored alias ("no such column"), and a write that names no
//! table in a database made with no tables (it went to the placeholder).

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

/// A database made with no tables, with table `p`: names that begin with
/// "sof" in three of its four rows.
fn shop(dir: &std::path::Path) -> Db {
    let db = Db::create_empty(&dir.join("p.strata"), DbOptions::default()).unwrap();
    let p = Params::new();
    db.sql("CREATE TABLE p (id KEYWORD PRIMARY KEY, name TEXT, color TEXT, n INT, emb VECTOR(2))", &p).unwrap();
    for (id, name, color, n) in [("a", "sofa", "red", 1), ("b", "sofa sofas software", "blue", 2), ("c", "chair", "green", 3), ("d", "soft lamp", "blue red", 4)] {
        db.sql(&format!("INSERT INTO p (id, name, color, n, emb) VALUES ('{id}', '{name}', '{color}', {n}, [1.0, {n}.0])"), &p).unwrap();
    }
    db
}

fn err(db: &Db, sql: &str) -> String {
    let mut p = Params::new();
    p.0.insert("v".into(), Value::Vector(vec![1.0f32, 0.0].into()));
    match db.query(sql, &p) {
        Ok(r) => panic!("{sql} was answered ({} rows), not refused", r.rows.len()),
        Err(e) => e.to_string(),
    }
}

#[test]
fn ranking_functions_refuse_options_they_do_not_take() {
    let dir = tempfile::tempdir().unwrap();
    let db = shop(dir.path());
    let e = err(&db, "SELECT id FROM p ORDER BY bm25(name, 'sofa', bogus => 7) DESC");
    assert!(e.contains("unknown option bogus for bm25") && e.contains("k1") && e.contains("syntax"), "{e}");
    let e = err(&db, "SELECT id FROM p ORDER BY bm25(name, 'sofa', k1 => 'x') DESC");
    assert!(e.contains("bm25's k1 is a number") && e.contains("'x'"), "{e}");
    let e = err(&db, "SELECT id FROM p ORDER BY bm25(name, 'sofa', b => 2) DESC");
    assert!(e.contains("bm25's b is a number from 0 to 1"), "{e}");
    let e = err(&db, "SELECT id FROM p ORDER BY bm25(name, 'sofa', algo => 'fast') DESC");
    assert!(e.contains("'maxscore'"), "{e}");
    let e = err(&db, "SELECT id FROM p ORDER BY similarity(emb, :v, ef => 'x') DESC");
    assert!(e.contains("similarity's ef is a whole number of at least 1"), "{e}");
    let e = err(&db, "SELECT id FROM p ORDER BY similarity(emb, :v, bogus => 1) DESC");
    assert!(e.contains("unknown option bogus for similarity") && e.contains("strategy"), "{e}");
    let e = err(&db, "SELECT id FROM p ORDER BY rrf(bm25(name, 'sofa'), similarity(emb, :v), kk => 60) DESC");
    assert!(e.contains("unknown option kk for rrf") && e.contains("weights"), "{e}");
    let e = err(&db, "SELECT id FROM p ORDER BY minmax(bm25(name, 'sofa'), k => 1) DESC");
    assert!(e.contains("minmax takes no options"), "{e}");

    // The options they take are taken, as before.
    let mut p = Params::new();
    p.0.insert("v".into(), Value::Vector(vec![1.0f32, 0.0].into()));
    let r = db.query("SELECT id FROM p ORDER BY bm25(name, 'sofa', k1 => 1.2, b => 0.75, algo => 'exhaustive') DESC", &p).unwrap();
    assert_eq!(sorted(ids(&r)), ["a", "b"]);
    let r = db.query("SELECT id FROM p ORDER BY rrf(bm25(name, 'sofa'), similarity(emb, :v, ef => 64), k => 60, depth => 10) DESC", &p).unwrap();
    assert_eq!(r.rows.len(), 4);
}

#[test]
fn rrf_weights_weigh_each_input() {
    let dir = tempfile::tempdir().unwrap();
    let db = shop(dir.path());
    let p = Params::new();
    // c is first by its color alone, a by its name alone.
    let q = |w: &str| format!("SELECT id FROM p WHERE id IN ('a', 'c') ORDER BY rrf(bm25(name, 'sofa'), bm25(color, 'green'){w}) DESC");
    let score = |r: &ResultSet, id: &str| r.scores[ids(r).iter().position(|x| x == id).unwrap()];

    let plain = db.query(&q(""), &p).unwrap();
    let even = db.query(&q(", weights => [1, 1]"), &p).unwrap();
    assert_eq!(ids(&plain), ids(&even));
    assert_eq!(plain.scores, even.scores, "weights of 1 are plain rrf");
    assert!((score(&plain, "a") - 1.0 / 61.0).abs() < 1e-9 && (score(&plain, "c") - 1.0 / 61.0).abs() < 1e-9);

    let color = db.query(&q(", weights => [0.1, 0.9]"), &p).unwrap();
    assert_eq!(ids(&color), ["c", "a"]);
    assert!((score(&color, "c") - 0.9 / 61.0).abs() < 1e-9, "{:?}", color.scores);
    assert!((score(&color, "a") - 0.1 / 61.0).abs() < 1e-9, "{:?}", color.scores);
    let name = db.query(&q(", weights => [0.9, 0.1], k => 10"), &p).unwrap();
    assert_eq!(ids(&name), ["a", "c"]);
    assert!((score(&name, "a") - 0.9 / 11.0).abs() < 1e-9, "k still counts: {:?}", name.scores);

    // As a parameter too.
    let mut w = Params::new();
    w.0.insert("w".into(), Value::Vector(vec![0.1f32, 0.9].into()));
    assert_eq!(ids(&db.query(&q(", weights => :w"), &w).unwrap()), ["c", "a"]);

    for (bad, says) in [("[1]", "2 rankings and 1 weight"), ("[0, 0]", "all 0"), ("[1, -1]", "at least 0"), ("'heavy'", "at least 0")] {
        let e = err(&db, &q(&format!(", weights => {bad}")));
        assert!(e.contains("weights") && e.contains(says), "{bad}: {e}");
    }
}

#[test]
fn order_by_a_scored_alias() {
    let dir = tempfile::tempdir().unwrap();
    let db = shop(dir.path());
    let p = Params::new();
    let by_call = db.query("SELECT id, bm25(name, 'sofa') AS score FROM p ORDER BY bm25(name, 'sofa') DESC", &p).unwrap();
    for sql in [
        "SELECT id, bm25(name, 'sofa') AS score FROM p ORDER BY score DESC",
        "SELECT id, bm25(name, 'sofa') AS Score FROM p ORDER BY score DESC LIMIT 10",
        "SELECT id, bm25(name, 'sofa') AS s FROM p WHERE n < 4 ORDER BY s DESC",
    ] {
        let r = db.query(sql, &p).unwrap();
        assert_eq!(ids(&r), ids(&by_call), "{sql}");
        assert_eq!(r.scores, by_call.scores, "{sql}");
    }
    // A key that is no alias is the table's column, as before.
    let r = db.query("SELECT id, n * 2 AS m FROM p ORDER BY n DESC", &p).unwrap();
    assert_eq!(ids(&r), ["d", "c", "b", "a"]);
}

#[test]
fn prefix_and_typo_terms_rank_the_rows_match_keeps() {
    let dir = tempfile::tempdir().unwrap();
    let db = shop(dir.path());
    let p = Params::new();
    for (word, kept) in [("sof*", vec!["a", "b", "d"]), ("sofaa~", vec!["a", "b"]), ("softwar~1", vec!["b"])] {
        let matched = db.query(&format!("SELECT id FROM p WHERE MATCH(name, '{word}')"), &p).unwrap();
        assert_eq!(sorted(ids(&matched)), kept, "MATCH {word}");
        let ranked = db.query(&format!("SELECT id FROM p ORDER BY bm25(name, '{word}') DESC"), &p).unwrap();
        assert_eq!(sorted(ids(&ranked)), kept, "bm25 {word} ranks the rows MATCH keeps");
        assert!(ranked.scores.iter().all(|s| *s > 0.0), "{word}: {:?}", ranked.scores);
    }
    // The row with more words that begin so ranks first, and a prefix
    // ranks beside other words.
    let r = db.query("SELECT id FROM p ORDER BY bm25(name, 'sof*') DESC", &p).unwrap();
    assert_eq!(ids(&r)[0], "b");
    let r = db.query("SELECT id FROM p ORDER BY bm25(name, 'chair sof*') DESC", &p).unwrap();
    assert_eq!(sorted(ids(&r)), ["a", "b", "c", "d"]);
    // Plain text is words as they are, as before.
    let r = db.query("SELECT id FROM p ORDER BY bm25(name, 'sof*', syntax => 'plain') DESC", &p).unwrap();
    assert!(r.rows.is_empty(), "{:?}", ids(&r));
    // A required word with a prefix, and a word left out.
    let r = db.query("SELECT id FROM p ORDER BY bm25(name, '+sof* -lamp') DESC", &p).unwrap();
    assert_eq!(sorted(ids(&r)), ["a", "b"]);
}

#[test]
fn a_prefix_of_many_words_ranks_by_the_most_common_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::create_empty(&dir.path().join("w.strata"), DbOptions::default()).unwrap();
    let p = Params::new();
    db.sql("CREATE TABLE w (id KEYWORD PRIMARY KEY, body TEXT)", &p).unwrap();
    let words: Vec<String> = (0..200).map(|i| format!("zed{}", char::from(b'a' + (i / 26) as u8).to_string() + &char::from(b'a' + (i % 26) as u8).to_string())).collect();
    let rows: Vec<String> = words.iter().enumerate().map(|(i, w)| format!("('{i}', '{w}')")).collect();
    db.sql(&format!("INSERT INTO w (id, body) VALUES {}", rows.join(", ")), &p).unwrap();
    db.sql("INSERT INTO w (id, body) VALUES ('x', 'zedaa zedaa zedab')", &p).unwrap();
    let r = db.query("SELECT id FROM w ORDER BY bm25(body, 'zed*') DESC LIMIT 5", &p).unwrap();
    assert_eq!(ids(&r)[0], "x");
    let note = r.stats.plan.iter().find(|l| l.starts_with("bm25 zed*")).unwrap_or_else(|| panic!("{:?}", r.stats.plan));
    // Of the 200 words, some are one stem ("zedas" is "zeda").
    let all: usize = note.rsplit("of the ").next().and_then(|t| t.split(' ').next()).and_then(|n| n.parse().ok()).unwrap_or(0);
    assert!(note.contains("the 128 terms in the most rows") && all > 128, "{note}");
}

#[test]
fn writes_that_name_no_table_go_to_the_default_table() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::create_empty(&dir.path().join("e.strata"), DbOptions::default()).unwrap();
    assert_eq!(db.count(), 0);
    let p = Params::new();
    db.sql("CREATE TABLE notes (id KEYWORD PRIMARY KEY, body TEXT)", &p).unwrap();
    let mut tx = db.storage().begin().unwrap();
    tx.insert(vec![Value::Str("a".into()), Value::Str("first note".into())]).unwrap();
    tx.upsert(vec![Value::Str("b".into()), Value::Str("second note".into())]).unwrap();
    tx.commit().unwrap();
    let r = db.query("SELECT id FROM notes ORDER BY id", &p).unwrap();
    assert_eq!(ids(&r), ["a", "b"]);
    assert_eq!(db.count(), 2, "the default table's rows");
    let placeholder = db.query("SELECT COUNT(*) FROM _placeholder", &p).unwrap();
    assert_eq!(placeholder.rows[0][0], Value::Int(0));

    // A row of the wrong shape names the table and its fields.
    let mut tx = db.storage().begin().unwrap();
    let e = tx.insert(vec![Value::Str("c".into())]).unwrap_err().to_string();
    assert!(e.contains("table notes has 2 fields (id, body)"), "{e}");
}
