//! An answer's column types follow the expression, not the column's name,
//! found by round 3 of the persona study (18 November 2026):
//!
//! - SAM-301: a client typed a column by looking its name up in the
//!   table, so `ts AS d`, `MAX(ts)`, `i.ts` in a join and `date_trunc`
//!   came back as text while `ts` was a timestamp. `ResultSet::types` gives
//!   each column's type from what it holds, as chloedb-server's answer
//!   and the Python and JavaScript packages now carry it.
//! - SAM-308: `SELECT 1 UNION ALL SELECT 2.5` answered an INT and a FLOAT
//!   in one column; the column is FLOAT, as in Postgres and DuckDB, on
//!   the operators and on DataFusion.
//! - PRIYA-301: the short form `"embed": ["title"], "model": "..."` (the
//!   Python docs' `add_column(..., embed=["title"], model=...)`) was
//!   "unknown option model".
//! - PRIYA-303: `{"embed": text}` and `{"vector_of": key}` used in
//!   `similarity(bge, :v)` took the table's first vector field;
//!   `chloedb::query::tables::param_fields` names the field each
//!   parameter is used with, and the error of a text for a field of no
//!   model names the query's own parameter.

use chloedb::db::Db;
use chloedb::query::Params;
use chloedb::storage::DbOptions;
use chloedb::value::Value;

fn db(dir: &std::path::Path) -> Db {
    let db = Db::create_empty(&dir.join("t.chloedb"), DbOptions { sync: false, ..DbOptions::default() }).unwrap();
    let p = Params::new();
    db.sql("CREATE TABLE invoice (id INT PRIMARY KEY, customer INT, ts TIMESTAMP, paid BOOL, total FLOAT, note TEXT)", &p).unwrap();
    db.sql("INSERT INTO invoice (id, customer, ts, paid, total, note) VALUES (1, 10, '2009-01-01T00:00:00Z', TRUE, 1.98, 'first'), (2, 11, '2013-12-22T10:30:00Z', FALSE, 3.96, NULL), (3, 10, NULL, NULL, NULL, NULL)", &p).unwrap();
    db.sql("CREATE TABLE customer (id INT PRIMARY KEY, name KEYWORD)", &p).unwrap();
    db.sql("INSERT INTO customer (id, name) VALUES (10, 'Ana'), (11, 'Sam')", &p).unwrap();
    db
}

fn types(db: &Db, sql: &str) -> Vec<Option<&'static str>> {
    db.query(sql, &Params::new()).unwrap_or_else(|e| panic!("{sql}: {e}")).types()
}

#[test]
fn a_column_s_type_follows_its_expression() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path());
    let ts = Some("timestamp");
    for sql in [
        "SELECT ts FROM invoice ORDER BY id LIMIT 1",
        "SELECT ts AS d FROM invoice ORDER BY id LIMIT 1",
        "SELECT MAX(ts) FROM invoice",
        "SELECT MIN(ts) AS first FROM invoice",
        "SELECT i.ts FROM invoice i JOIN customer c ON c.id = i.customer ORDER BY i.id LIMIT 1",
        "SELECT date_trunc('month', ts) AS m FROM invoice ORDER BY id LIMIT 1",
        "SELECT date_bin('1 day', ts) AS b FROM invoice ORDER BY id LIMIT 1",
        "SELECT ts + INTERVAL '1 day' AS next FROM invoice ORDER BY id LIMIT 1",
        "SELECT c.name, MAX(i.ts) AS last FROM invoice i JOIN customer c ON c.id = i.customer GROUP BY c.name ORDER BY c.name",
    ] {
        let t = types(&db, sql);
        assert_eq!(t.last().copied().flatten(), ts, "{sql}: {t:?}");
    }
    assert_eq!(types(&db, "SELECT id, total, paid, note, ts FROM invoice ORDER BY id"), [Some("int"), Some("float"), Some("bool"), Some("text"), ts]);
    assert_eq!(types(&db, "SELECT CAST(ts AS TEXT) AS ts FROM invoice ORDER BY id LIMIT 1"), [Some("text")], "a cast to text is text, whatever its name");
    assert_eq!(types(&db, "SELECT COUNT(*) AS n, SUM(total) AS s FROM invoice"), [Some("int"), Some("float")]);
    // A column of only NULLs has no type to give.
    assert_eq!(types(&db, "SELECT ts FROM invoice WHERE id = 3"), [None]);
    assert_eq!(types(&db, "SELECT id FROM invoice WHERE id > 99"), [None]);
}

#[test]
fn a_union_of_ints_and_floats_is_float() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(dir.path());
    let rows = |sql: &str, opts: &chloedb::query::exec::ExecOptions| db.query_with(sql, &Params::new(), opts).unwrap_or_else(|e| panic!("{sql}: {e}"));
    let mut engines = vec![chloedb::query::exec::Engine::Operators];
    #[cfg(feature = "datafusion")]
    engines.push(chloedb::query::exec::Engine::DataFusion);
    for engine in engines {
        let opts = chloedb::query::exec::ExecOptions { engine, ..db.exec.clone() };
        let r = rows("SELECT 1 AS v UNION ALL SELECT 2.5", &opts);
        assert_eq!(r.rows, [[Value::Float(1.0)], [Value::Float(2.5)]], "{engine:?}");
        assert_eq!(r.types(), [Some("float")]);
        // A NULL keeps its place; a column of INTs alone stays INT.
        let r = rows("SELECT id AS v FROM invoice WHERE id = 1 UNION ALL SELECT total FROM invoice WHERE id = 2 UNION ALL SELECT NULL", &opts);
        assert_eq!(r.rows, [[Value::Float(1.0)], [Value::Float(3.96)], [Value::Null]], "{engine:?}");
        let r = rows("SELECT 1 AS v UNION ALL SELECT 2", &opts);
        assert_eq!(r.rows, [[Value::Int(1)], [Value::Int(2)]], "{engine:?}");
        // UNION (distinct) takes 1 and 1.0 as one row; INTERSECT and
        // EXCEPT type their column by both sides, as UNION does.
        let r = rows("SELECT 1 AS v UNION SELECT 1.0 UNION SELECT 2.5", &opts);
        assert_eq!(r.rows, [[Value::Float(1.0)], [Value::Float(2.5)]], "{engine:?}");
        let r = rows("SELECT 1 AS v INTERSECT SELECT 1.0", &opts);
        assert_eq!(r.rows, [[Value::Float(1.0)]], "{engine:?}");
        let r = rows("SELECT 1 AS v EXCEPT SELECT 2.5", &opts);
        assert_eq!(r.rows, [[Value::Float(1.0)]], "{engine:?}");
        // A column of text and numbers is not made FLOAT.
        let r = rows("SELECT 'a' AS v UNION ALL SELECT 2.5", &opts);
        assert_eq!(r.types(), [None], "{engine:?}");
    }
}

#[test]
fn the_short_form_takes_embed_beside_model() {
    let short = |field: serde_json::Value| chloedb::Schema::from_json(&serde_json::json!({"table": "notes", "key": "id", "fields": [{"name": "id", "type": "keyword"}, {"name": "title", "type": "text"}, field]}).to_string());
    let s = short(serde_json::json!({"name": "emb", "type": "vector", "embed": ["title"], "model": "hash:16", "dim": 16})).unwrap();
    let e = s.field("emb").unwrap().embed.clone().unwrap();
    assert_eq!((e.from, e.model.as_str()), (vec!["title".to_string()], "hash:16"));
    let s = short(serde_json::json!({"name": "emb", "type": "vector", "embed": "title", "model": "potion-retrieval-32M"})).unwrap();
    assert!(matches!(s.field("emb").unwrap().ty, chloedb::FieldType::Vector { dim: 512, .. }), "the model's dimensions");
    let e = short(serde_json::json!({"name": "emb", "type": "vector", "embed": ["title"], "dim": 16})).unwrap_err().to_string();
    assert!(e.contains("embed needs a \"model\""), "{e}");
    let e = short(serde_json::json!({"name": "emb", "type": "vector", "model": "hash:16", "dim": 16})).unwrap_err().to_string();
    assert!(e.contains("give the fields it embeds"), "{e}");
}

#[test]
fn a_parameter_s_field_is_the_one_it_is_used_with() {
    use chloedb::query::tables::param_fields;
    let f = param_fields("SELECT id FROM docs ORDER BY similarity(bge, :v) DESC LIMIT 3");
    assert_eq!(f.get("v").map(String::as_str), Some("bge"));
    let f = param_fields("SELECT id FROM docs d WHERE id <> 'x' ORDER BY rrf(bm25(title, :q), similarity(d.emb, :v), maxsim(tok, :t)) DESC");
    assert_eq!((f.get("v").map(String::as_str), f.get("t").map(String::as_str), f.get("q")), (Some("emb"), Some("tok"), None));
    // dot takes them in either order; a parameter used with two fields
    // names neither.
    let f = param_fields("SELECT id, dot(:w, emb) AS d FROM docs ORDER BY similarity(bge, :v) + similarity(emb, :v) DESC");
    assert_eq!((f.get("w").map(String::as_str), f.get("v")), (Some("emb"), None));
    assert!(param_fields("not sql").is_empty());
}

#[test]
fn text_for_a_field_of_no_model_names_the_parameter() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::create_empty(&dir.path().join("v.chloedb"), DbOptions { sync: false, ..DbOptions::default() }).unwrap();
    let p = Params::new();
    db.sql("CREATE TABLE docs (id KEYWORD PRIMARY KEY, title TEXT, emb VECTOR(16) OPTIONS(embed = 'title', model = 'hash:16'), bge VECTOR(4))", &p).unwrap();
    // No model opens here: emb's vectors wait for embed_backfill.
    db.set_embed_later(true);
    db.sql("INSERT INTO docs (id, title, bge) VALUES ('a', 'vitamin d', '[1,0,0,0]')", &p).unwrap();
    let e = db.query("SELECT id FROM docs ORDER BY similarity(bge, :v) DESC LIMIT 3", &Params::new().set("v", Value::Str("vitamin d".into()))).unwrap_err().to_string();
    assert!(e.contains("\"params\": {\"v\": {\"vector_of\"") && e.contains("embed('<model>', :v)"), "{e}");
    let e = db.query("SELECT id FROM docs ORDER BY similarity(bge, :v) DESC LIMIT 3", &Params::new().set("v", Value::Vector(vec![0.0f32; 16].into()))).unwrap_err().to_string();
    assert!(e.contains("the vector has 16 dimensions, and bge has 4"), "{e}");
}
