//! `CREATE BLEND` and `FROM blend(...)`: the rows of several queries of a
//! database in one list, by their calibrated heads, under the slate's
//! rules; each definition a version of a stored function, read by
//! `name@n`, kept across a reopen.

use chloedb::db::{Db, SqlResult};
use chloedb::query::Params;
use chloedb::schema::Schema;
use chloedb::storage::{DbOptions, Layout};
use chloedb::value::Value;

fn opts() -> DbOptions {
    DbOptions { sync: false, layout: Layout::SingleFile, segment_max_docs: 4, ..DbOptions::default() }
}

fn sql(db: &Db, s: &str, p: &Params) -> (Vec<String>, Vec<Vec<Value>>, Vec<String>) {
    match db.sql(s, p).unwrap_or_else(|e| panic!("{s}: {e}")) {
        SqlResult::Rows(r) => (r.columns, r.rows, r.stats.plan),
        SqlResult::Written(_) => (Vec::new(), Vec::new(), Vec::new()),
    }
}

fn run(db: &Db, s: &str) -> Vec<Vec<Value>> {
    sql(db, s, &Params::new()).1
}

fn fails(db: &Db, s: &str) -> String {
    db.sql(s, &Params::new()).err().unwrap_or_else(|| panic!("{s} ran")).to_string()
}

fn text(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        v => format!("{v:?}"),
    }
}

const SERP: &str = "CREATE BLEND serp AS
  SOURCES (
    stories  AS (SELECT id, title, author, score() AS relevance, points FROM stories ORDER BY bm25(title, :q) DESC LIMIT 20),
    comments AS (SELECT id, body AS title, author, score() AS relevance FROM comments ORDER BY bm25(body, :q) DESC LIMIT 20)
  )
  HEADS (relevance CALIBRATE tmm, points CALIBRATE minmax MISSING zero)
  UTILITY 1 * relevance + 0.1 * points
  SLOT 1 => source = 'stories'
  QUOTA (source = 'comments' AT LEAST 40% IN TOP 5)
  MAX 2 PER author
  WITH (k => 6, note => 'first')";

fn setup(path: &std::path::Path) -> Db {
    let seed = Schema::new("seed", vec![chloedb::schema::Field::new("id", chloedb::schema::FieldType::Keyword)], Some("id")).unwrap();
    let db = Db::create(path, seed, opts()).unwrap();
    run(&db, "CREATE TABLE stories (id KEYWORD PRIMARY KEY, title TEXT, author KEYWORD, points INT)");
    run(&db, "CREATE TABLE comments (id KEYWORD PRIMARY KEY, body TEXT, author KEYWORD)");
    for i in 0..8 {
        run(&db, &format!("INSERT INTO stories VALUES ('s{i}', 'rust async runtime part {i} {}', 'au{}', {})", "rust ".repeat(8 - i), i % 3, 10 * (8 - i)));
        run(&db, &format!("INSERT INTO comments VALUES ('c{i}', 'a comment on rust {} number {i}', 'cu{}')", "async ".repeat(8 - i), i % 4));
    }
    db
}

#[test]
fn blends_two_tables_by_its_rules() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("b.strata");
    let db = setup(&path);
    let made = run(&db, SERP);
    assert_eq!(made[0][..2], [Value::Str("serp".into()), Value::Int(1)]);
    assert!(fails(&db, SERP).contains("CREATE OR REPLACE BLEND"));

    let p = Params::new().set("q", Value::Str("rust async".into()));
    let (cols, rows, plan) = sql(&db, "SELECT slot, source, id, author, utility, blend_decision FROM blend('serp', q => :q)", &p);
    assert_eq!(cols, ["slot", "source", "id", "author", "utility", "blend_decision"]);
    assert_eq!(rows.len(), 6);
    // Slot 1 is a story's, and at least 2 of the top 5 are comments.
    assert_eq!(text(&rows[0][1]), "stories");
    assert!(text(&rows[0][5]).contains("slot 1 is kept"), "{:?}", rows[0]);
    let comments = rows[..5].iter().filter(|r| text(&r[1]) == "comments").count();
    assert!(comments >= 2, "{rows:?}");
    // At most 2 per author.
    let mut per = std::collections::HashMap::new();
    for r in &rows {
        *per.entry(text(&r[3])).or_insert(0) += 1;
    }
    assert!(per.values().all(|&n| n <= 2), "{rows:?}");
    assert!(plan.iter().any(|l| l.starts_with("blend serp@1: 2 sources")), "{plan:?}");
    assert!(plan.iter().any(|l| l.contains("QUOTA") || l.contains("quota")), "{plan:?}");

    // `*` is the slot, the source, every source's columns, the utility and why.
    let (cols, _, _) = sql(&db, "SELECT * FROM blend('serp', q => 'rust') LIMIT 3", &Params::new());
    assert_eq!(cols, ["slot", "source", "id", "title", "author", "relevance", "points", "utility", "blend_decision"]);
    let rows = run(&db, "SELECT id FROM blend('serp', q => 'rust') LIMIT 3");
    assert_eq!(rows.len(), 3);

    // A new version; the first stays readable.
    run(&db, &SERP.replace("CREATE BLEND", "CREATE OR REPLACE BLEND").replace("SLOT 1 => source = 'stories'", "SLOT 1 => source = 'comments'").replace("'first'", "'comments first'"));
    let now = run(&db, "SELECT source FROM blend('serp', q => 'rust') LIMIT 1");
    let was = run(&db, "SELECT source FROM blend('serp@1', q => 'rust') LIMIT 1");
    assert_eq!((text(&now[0][0]), text(&was[0][0])), ("comments".into(), "stories".into()));
    assert_eq!(db.blends().unwrap()[0].0.note.as_deref(), Some("comments first"));

    // Refused: a WHERE on the blend, a column it does not have, a blend
    // of no table, a source that is not a query.
    assert!(fails(&db, "SELECT * FROM blend('serp', q => 'x') WHERE slot = 1").contains("no join, WHERE"));
    assert!(fails(&db, "SELECT nope FROM blend('serp', q => 'x')").contains("no column nope"));
    assert!(fails(&db, "SELECT * FROM blend('none')").contains("no blend named none"));
    assert!(fails(&db, "SELECT * FROM blend('serp')").contains("unbound parameter :q"));
    assert!(fails(&db, "SELECT * FROM blend('serp@9', q => 'x')").contains("no version 9"));

    // Kept across a reopen; DROP BLEND leaves its versions.
    drop(db);
    let db = Db::open(&path, opts()).unwrap();
    assert_eq!(run(&db, "SELECT source FROM blend('serp', q => 'rust') LIMIT 1")[0][0], Value::Str("comments".into()));
    run(&db, "DROP BLEND serp");
    assert!(fails(&db, "SELECT * FROM blend('serp', q => 'x')").contains("no blend named serp"));
    assert_eq!(run(&db, "SELECT source FROM blend('serp@2', q => 'rust') LIMIT 1")[0][0], Value::Str("comments".into()));
    assert!(fails(&db, "DROP BLEND serp").contains("no blend named serp"));
    run(&db, "DROP BLEND IF EXISTS serp");
    // A function that is not a blend is not replaced by one.
    run(&db, "CREATE FUNCTION two() RETURNS INT AS '2'");
    assert!(fails(&db, "CREATE OR REPLACE BLEND two AS SOURCES (a AS (SELECT id FROM stories LIMIT 3))").contains("not a blend"));
}

/// A blend stacked under another as a node (`name AS BLEND (...)`): it
/// runs into its own k slots under its own rules, and its placed rows are
/// that source's rows above, with `via` naming the source under it and
/// its utility as their score; written back as it was read, and no more
/// than four blends deep.
#[test]
fn blends_stack_as_nodes() {
    let dir = tempfile::tempdir().unwrap();
    let db = setup(&dir.path().join("n.strata"));
    let page = "CREATE BLEND page AS
  SOURCES (
    web AS BLEND (
      SOURCES (
        stories AS (SELECT id, title, author, score() AS relevance FROM stories ORDER BY bm25(title, :q) DESC LIMIT 20),
        comments AS (SELECT id, body AS title, author, score() AS relevance FROM comments ORDER BY bm25(body, :q) DESC LIMIT 20)
      )
      HEADS (relevance CALIBRATE tmm)
      UTILITY 1 * relevance
      MAX 1 PER author
      WITH (k => 6)
    ),
    fresh AS (SELECT id, title, author, points FROM stories ORDER BY points DESC LIMIT 4)
  )
  QUOTA (source = 'web' AT LEAST 60%)
  WITH (k => 5)";
    run(&db, page);
    let (def, _) = db.blends().unwrap().into_iter().next().unwrap();
    assert_eq!(def.depth(), 2);
    assert_eq!(def.queries().len(), 3);
    // Read back as written.
    let again = chloedb::query::blend::parse_statement(&def.sql(true)).unwrap().unwrap();
    assert!(matches!(&again, chloedb::query::blend::Statement::Create { def: d, replace: true } if *d == def), "{}", def.sql(true));

    let p = Params::new().set("q", Value::Str("rust async".into()));
    let (cols, rows, _) = sql(&db, "SELECT slot, source, via, id, author FROM blend('page', q => :q)", &p);
    assert_eq!(cols, ["slot", "source", "via", "id", "author"]);
    assert_eq!(rows.len(), 5);
    let web: Vec<&Vec<Value>> = rows.iter().filter(|r| text(&r[1]) == "web").collect();
    assert!(web.len() >= 3, "{rows:?}");
    // The node's own rule held: one row per author among the rows it gave.
    let mut authors: Vec<String> = web.iter().map(|r| text(&r[4])).collect();
    authors.sort();
    authors.dedup();
    assert_eq!(authors.len(), web.len(), "{rows:?}");
    for r in &web {
        assert!(["web/stories", "web/comments"].contains(&text(&r[2]).as_str()), "{r:?}");
    }
    for r in rows.iter().filter(|r| text(&r[1]) == "fresh") {
        assert_eq!(text(&r[2]), "fresh", "{r:?}");
    }

    // Each node's receipts come with the blend's.
    let b = db.blend_run(&def, 1, &p, 5, chloedb::query::blend::Want::default()).unwrap();
    let j = b.receipts();
    assert_eq!(j["nodes"]["web"]["rows"].as_array().unwrap().len(), 6, "{}", j["nodes"]);
    assert_eq!(j["nodes"]["web"]["blend"], "web");

    // A source that reads blend(...) is told to stack it as a node; five
    // blends deep is refused.
    run(&db, "CREATE BLEND solo AS SOURCES (a AS (SELECT id, title FROM stories LIMIT 3))");
    assert!(fails(&db, "CREATE BLEND bad AS SOURCES (a AS (SELECT * FROM blend('solo')))").contains("AS BLEND"));
    let mut deep = "SOURCES (a AS (SELECT id FROM stories LIMIT 3))".to_string();
    for i in 0..4 {
        deep = format!("SOURCES (n{i} AS BLEND ({deep}))");
    }
    assert!(fails(&db, &format!("CREATE BLEND deep AS {deep}")).contains("stack at most 4 deep"));

    // A rule above reads `via`: at most one of the node's comments in the
    // top 5.
    run(&db, &page.replace("CREATE BLEND page", "CREATE OR REPLACE BLEND page").replace("QUOTA (source = 'web' AT LEAST 60%)", "QUOTA (source = 'web' AT LEAST 60%, via = 'web/comments' AT MOST 20%)"));
    let rows = sql(&db, "SELECT via FROM blend('page', q => :q)", &p).1;
    assert!(rows.iter().filter(|r| text(&r[0]) == "web/comments").count() <= 1, "{rows:?}");
    let (cols, _, _) = sql(&db, "SELECT * FROM blend('page', q => :q) LIMIT 2", &p);
    assert_eq!(cols[..3], ["slot", "source", "via"]);
}
