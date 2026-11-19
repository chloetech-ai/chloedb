//! A query's funnel ([`chloedb::query::funnel`]): the rows each stage of a
//! search kept and dropped, and why, from probes that are queries of the
//! bound statement; and the step that says what a search's words became.

use chloedb::db::Db;
use chloedb::query::funnel::{Rows, Stage};
use chloedb::query::Params;
use chloedb::storage::DbOptions;
use chloedb::value::Value;
use serde_json::json;

/// Twenty items: five whose titles hold "red" or "chair", one of them too
/// dear and one of a brand the searches leave out, and fifteen of neither.
fn items(dir: &std::path::Path) -> Db {
    let db = Db::create_empty(&dir.join("t.chloedb"), DbOptions { sync: false, ..DbOptions::default() }).unwrap();
    let p = Params::new();
    db.sql("CREATE TABLE items (id INT PRIMARY KEY, title TEXT, brand KEYWORD, price FLOAT, v VECTOR(3) OPTIONS(index = 'flat'))", &p).unwrap();
    let mut rows = vec![
        "(1, 'red chair', 'acme', 50, '[1.0, 0.0, 0.0]')".to_string(),
        "(2, 'red sofa chair', 'acme', 150, '[0.9, 0.1, 0.0]')".to_string(),
        "(3, 'blue chair', 'bolt', 40, '[0.0, 1.0, 0.0]')".to_string(),
        "(4, 'red table', 'acme', 30, '[0.8, 0.2, 0.0]')".to_string(),
        "(5, 'green lamp', 'bolt', 20, '[0.0, 0.0, 1.0]')".to_string(),
        "(6, 'red chair deluxe', 'zeta', 80, '[0.95, 0.05, 0.0]')".to_string(),
    ];
    for i in 7..=20 {
        rows.push(format!("({i}, 'oak shelf {i}', 'bolt', 10, '[0.0, 0.1, 1.0]')"));
    }
    db.sql(&format!("INSERT INTO items (id, title, brand, price, v) VALUES {}", rows.join(", ")), &p).unwrap();
    db
}

fn fate(rows: &Rows, key: i64) -> (&'static str, Stage, String) {
    let r = rows.rows.iter().find(|r| r.key == json!(key)).unwrap_or_else(|| panic!("row {key} is not in the funnel: {rows:#?}"));
    (r.fate, r.stage, r.why.clone())
}

#[test]
fn a_hybrid_search_says_what_each_stage_kept_and_why_the_rest_left() {
    let dir = tempfile::tempdir().unwrap();
    let db = items(dir.path());
    let p = Params::new().set("q", Value::Vector(vec![1.0, 0.0, 0.0].into()));
    let sql = "SELECT id FROM items WHERE price < 100 AND brand <> 'zeta' ORDER BY rrf(bm25(title, 'red chair'), similarity(v, :q)) DESC LIMIT 2";
    let page: Vec<serde_json::Value> = db.query(sql, &p).unwrap().rows.into_iter().map(|r| json!(r[0].as_i64().unwrap())).collect();
    assert_eq!(page.len(), 2);
    assert_eq!(page[0], json!(1));

    let f = db.funnel(sql, &p).unwrap();
    assert_eq!(f.kind, "search");
    assert_eq!(f.live, Some(20));
    // Each input: BM25 counts the rows that hold a word; the nearest
    // vectors rank every row that has one.
    assert_eq!(f.inputs.len(), 2, "{f:#?}");
    assert_eq!((f.inputs[0].kind, f.inputs[0].matched), ("bm25", Some(5)));
    assert_eq!(f.inputs[0].text.as_deref(), Some("red chair"));
    assert_eq!(f.inputs[1].matched, None);
    // WHERE, a condition at a time, from every row a search by meaning reaches.
    assert_eq!(f.matched, Some(20));
    // `brand <> 'zeta'` is SQL's: a brand, and not that one.
    let texts: Vec<&str> = f.conditions.iter().map(|c| c.text.as_str()).collect();
    assert_eq!(texts, vec!["price < 100", "brand IS NOT NULL", "NOT (brand = 'zeta')"]);
    assert_eq!(f.conditions.iter().map(|c| c.left).collect::<Vec<_>>(), vec![Some(19), Some(19), Some(18)]);
    assert_eq!(f.selected, Some(18));
    assert_eq!(f.shown, 2);
    assert!(f.fused.as_deref().is_some_and(|x| x.contains("rrf")), "{:?}", f.fused);

    // Where each row left: the page as the query answers it, and the
    // other item that holds a word, ranked past it.
    let shown: Vec<serde_json::Value> = f.rows.iter().filter(|r| r.fate == "shown").map(|r| r.key.clone()).collect();
    assert_eq!(shown, page);
    let other = [3, 4].into_iter().find(|&k| json!(k) != page[1]).unwrap();
    let (fate_o, stage_o, why_o) = fate(&f, other);
    assert_eq!((fate_o, stage_o), ("cut", Stage::Rank), "{why_o}");
    assert!(why_o.contains("past the 2 rows the page reaches"), "{why_o}");
    let (fate2, stage2, why2) = fate(&f, 2);
    assert_eq!((fate2, stage2), ("filtered", Stage::Select));
    assert!(why2.contains("price"), "{why2}");
    let (fate6, _, why6) = fate(&f, 6);
    assert_eq!(fate6, "filtered");
    assert!(why6.contains("brand"), "{why6}");
    // The shown rows first, then by how far each got.
    assert_eq!(f.rows[0].key, page[0]);
    assert_eq!(f.rows[1].key, page[1]);
    let r2 = f.rows.iter().find(|r| r.key == json!(2)).unwrap();
    assert_eq!(r2.fails, vec![0]);
    assert_eq!(f.rows.iter().find(|r| r.key == json!(6)).unwrap().fails, vec![2]);
    assert!(r2.matched[0].is_some(), "the words matched it before WHERE dropped it: {r2:?}");
}

#[test]
fn one_ranking_cuts_at_the_page_and_says_where() {
    let dir = tempfile::tempdir().unwrap();
    let db = items(dir.path());
    let sql = "SELECT id FROM items WHERE price < 100 ORDER BY bm25(title, 'red chair') DESC LIMIT 1";
    let f = db.funnel(sql, &Params::new()).unwrap();
    assert_eq!(f.kind, "search");
    assert_eq!(f.inputs[0].matched, Some(5));
    // Row 2 holds both words but costs 150; rows 3, 4 and 6 hold one word
    // or rank below row 1.
    assert_eq!(f.selected, Some(4));
    assert_eq!(fate(&f, 1).0, "shown");
    assert_eq!(fate(&f, 2).0, "filtered");
    for k in [3, 4, 6] {
        let (fate, stage, why) = fate(&f, k);
        assert_eq!((fate, stage), ("cut", Stage::Rank), "row {k}: {why}");
    }
}

#[test]
fn rows_in_an_order_of_a_column_follow_the_same_funnel() {
    let dir = tempfile::tempdir().unwrap();
    let db = items(dir.path());
    let f = db.funnel("SELECT id FROM items WHERE brand = 'acme' ORDER BY price LIMIT 2", &Params::new()).unwrap();
    assert_eq!(f.kind, "rows");
    assert_eq!((f.live, f.selected, f.shown), (Some(20), Some(3), 2));
    assert_eq!(fate(&f, 4).0, "shown");
    assert_eq!(fate(&f, 1).0, "shown");
    assert_eq!(fate(&f, 2).0, "cut");
}

#[test]
fn a_query_of_groups_or_several_tables_has_no_rows_to_follow() {
    let dir = tempfile::tempdir().unwrap();
    let db = items(dir.path());
    let f = db.funnel("SELECT brand, COUNT(*) FROM items GROUP BY brand", &Params::new()).unwrap();
    assert_eq!(f.kind, "other");
    assert!(f.rows.is_empty());
    let e = db.funnel("SELECT a.id FROM items a JOIN items b ON a.id = b.id", &Params::new()).unwrap_err();
    assert!(e.to_string().contains("funnel"), "{e}");
}

#[test]
fn a_search_s_words_become_terms_in_a_step_of_their_own() {
    let dir = tempfile::tempdir().unwrap();
    let db = items(dir.path());
    let rs = db.query("SELECT id FROM items ORDER BY bm25(title, 'red chiar', fuzzy => 1) DESC LIMIT 3", &Params::new()).unwrap();
    let terms = rs.stats.steps.iter().find(|s| s.op == "terms").unwrap_or_else(|| panic!("no terms step: {:#?}", rs.stats.steps));
    assert!(terms.detail.starts_with("2 of the text"), "{terms:?}");
    assert!(terms.rows.is_some_and(|n| n >= 3), "chair added for chiar: {terms:?}");
    // On the funnel's stages, the expansion is its own.
    assert_eq!(Stage::of_step("terms"), Some(Stage::Expand));
}
