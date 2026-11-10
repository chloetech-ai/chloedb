//! Cuts a query did not write are visible, and never cut a write: a ranked
//! query without LIMIT says it returned its default top 100 rows, a ranked
//! DELETE without LIMIT is refused rather than deleting 100 of its matches,
//! pages of a fused ranking (`LIMIT n OFFSET k`) are the pages of one
//! ranking, and `ppr` answers alike whatever the scale of its edge weights
//! and says when it stopped at `max_pushes`.

use std::collections::HashSet;
use std::sync::Arc;

use rand::{Rng, SeedableRng};
use chloedb::db::{Db, SqlResult};
use chloedb::query::exec::ResultSet;
use chloedb::query::Params;
use chloedb::storage::DbOptions;
use chloedb::value::Value;
use chloedb::{Field, FieldType, Schema, VectorIndexKind};

const DIM: usize = 16;

fn run(db: &Db, sql: &str, params: &Params) -> SqlResult {
    db.sql(sql, params).unwrap_or_else(|e| panic!("{sql}: {e}"))
}

fn query(db: &Db, sql: &str, params: &Params) -> ResultSet {
    db.query(sql, params).unwrap_or_else(|e| panic!("{sql}: {e}"))
}

fn count(db: &Db, sql: &str) -> i64 {
    query(db, sql, &Params::new()).rows[0][0].as_i64().unwrap()
}

fn written(r: SqlResult) -> u64 {
    match r {
        SqlResult::Written(n) => n,
        _ => panic!("expected a write"),
    }
}

/// 1,000 rows whose body every one holds `word`, in the memtable or
/// flushed into segments.
fn thousand(dir: &std::path::Path, name: &str, flush: bool) -> Db {
    let schema = Schema::new("t", vec![Field::new("id", FieldType::Int), Field::new("n", FieldType::Int), Field::new("body", FieldType::Text { positions: false })], Some("id")).unwrap();
    let db = Db::create(&dir.join(name), schema, DbOptions { sync: false, ..DbOptions::default() }).unwrap();
    db.load((0..1000).map(|i| vec![Value::Int(i), Value::Int(0), Value::Str(format!("row {i} word {}", "filler ".repeat(i as usize % 7)))])).unwrap();
    if flush {
        db.optimize().unwrap();
    }
    db
}

#[test]
fn a_ranked_query_without_limit_says_it_was_cut() {
    let dir = tempfile::tempdir().unwrap();
    for flush in [false, true] {
        let db = thousand(dir.path(), &format!("sel{flush}"), flush);
        let p = Params::new();
        let r = query(&db, "SELECT id FROM t ORDER BY bm25(body, 'word') DESC", &p);
        assert_eq!(r.rows.len(), 100, "the documented default");
        assert_eq!(r.stats.implicit_limit, Some(100));
        assert!(r.stats.truncated, "1,000 rows match: the answer is cut");
        assert!(r.stats.plan.iter().any(|l| l.contains("no LIMIT")), "{:?}", r.stats.plan);
        let json = serde_json::to_value(&r.stats).unwrap();
        assert_eq!((json["implicit_limit"].as_u64(), json["truncated"].as_bool()), (Some(100), Some(true)), "the stats the bindings read");

        // A LIMIT the query wrote is not a cut it did not ask for.
        let r = query(&db, "SELECT id FROM t ORDER BY bm25(body, 'word') DESC LIMIT 10", &p);
        assert_eq!((r.rows.len(), r.stats.implicit_limit, r.stats.truncated), (10, None, false));
        let json = serde_json::to_value(&r.stats).unwrap();
        assert!(json.get("truncated").is_none() && json.get("implicit_limit").is_none());

        // Fewer matches than the default: nothing is cut.
        let r = query(&db, "SELECT id FROM t WHERE id < 40 ORDER BY bm25(body, 'word') DESC", &p);
        assert_eq!((r.rows.len(), r.stats.implicit_limit, r.stats.truncated), (40, Some(100), false));
        // Exactly the default: nothing is cut either.
        let r = query(&db, "SELECT id FROM t WHERE id < 100 ORDER BY bm25(body, 'word') DESC", &p);
        assert_eq!((r.rows.len(), r.stats.truncated), (100, false));
        let r = query(&db, "SELECT id FROM t WHERE id < 101 ORDER BY bm25(body, 'word') DESC", &p);
        assert_eq!((r.rows.len(), r.stats.truncated), (100, true));
        // The rows themselves are those of the query with LIMIT 100.
        let a = query(&db, "SELECT id FROM t ORDER BY bm25(body, 'word') DESC", &p);
        let b = query(&db, "SELECT id FROM t ORDER BY bm25(body, 'word') DESC LIMIT 100", &p);
        assert_eq!(a.rows, b.rows);
        // An order by a column is not cut.
        let r = query(&db, "SELECT id FROM t ORDER BY id", &p);
        assert_eq!((r.rows.len(), r.stats.truncated), (1000, false));
    }
}

#[test]
fn a_ranked_delete_is_never_cut_by_the_read_default() {
    let dir = tempfile::tempdir().unwrap();
    for flush in [false, true] {
        let db = thousand(dir.path(), &format!("del{flush}"), flush);
        let p = Params::new();
        // Before: deleted 100 of the 1,000 rows that match.
        for sql in ["DELETE FROM t ORDER BY bm25(body, 'word') DESC", "DELETE FROM t WHERE MATCH(body, 'word') ORDER BY bm25(body, 'word') DESC"] {
            let e = db.sql(sql, &p).err().unwrap_or_else(|| panic!("{sql}: refused")).to_string();
            assert!(e.contains("needs a LIMIT") && e.contains("WHERE MATCH"), "{e}");
            assert_eq!(count(&db, "SELECT COUNT(*) FROM t"), 1000, "{sql}: nothing deleted");
        }
        // With a LIMIT, the top rows by the score.
        assert_eq!(written(run(&db, "DELETE FROM t ORDER BY bm25(body, 'word') DESC LIMIT 250", &p)), 250);
        assert_eq!(count(&db, "SELECT COUNT(*) FROM t"), 750);
        // An order by a column cuts nothing: every row WHERE keeps.
        let above = db_count_between(&db, 500);
        assert!(above > 100);
        assert_eq!(written(run(&db, "DELETE FROM t WHERE id >= 500 ORDER BY id", &p)), above);
        // Without ORDER BY, every row WHERE matches.
        assert_eq!(written(run(&db, "UPDATE t SET n = 3 WHERE MATCH(body, 'word')", &p)) as i64, count(&db, "SELECT COUNT(*) FROM t"));
        let left = count(&db, "SELECT COUNT(*) FROM t");
        assert!(left > 100);
        assert_eq!(written(run(&db, "DELETE FROM t WHERE MATCH(body, 'word')", &p)) as i64, left);
        assert_eq!(count(&db, "SELECT COUNT(*) FROM t"), 0);
    }
}

/// The rows left at or above `from`.
fn db_count_between(db: &Db, from: i64) -> u64 {
    count(db, &format!("SELECT COUNT(*) FROM t WHERE id >= {from}")) as u64
}

fn unit(rng: &mut rand::rngs::StdRng) -> Vec<f32> {
    let v: Vec<f32> = (0..DIM).map(|_| rng.gen_range(-1.0f32..1.0)).collect();
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    v.into_iter().map(|x| x / n).collect()
}

fn vec_value(v: &[f32]) -> Value {
    Value::Vector(Arc::from(v))
}

/// 2,000 rows that all match `word`, with bm25 scores that differ, a
/// rating and a vector (an HNSW index, as `VECTOR(16)` makes).
fn fused_db(dir: &std::path::Path) -> (Db, Params) {
    let mut rng = rand::rngs::StdRng::seed_from_u64(7);
    let schema = Schema::new(
        "t",
        vec![
            Field::new("id", FieldType::Int),
            Field::new("rating", FieldType::Float),
            Field::new("body", FieldType::Text { positions: false }),
            Field::new("emb", FieldType::Vector { dim: DIM, index: VectorIndexKind::Hnsw { m: 16, ef_construction: 200 } }),
        ],
        Some("id"),
    )
    .unwrap();
    let db = Db::create(&dir.join("f.strata"), schema, DbOptions { sync: false, ..DbOptions::default() }).unwrap();
    let words = ["alpha", "beta", "gamma", "delta", "omega"];
    let docs: Vec<Vec<Value>> = (0..2000)
        .map(|i| {
            let n = rng.gen_range(3..40);
            let mut body = vec!["word"; 1 + i % 5];
            body.extend((0..n).map(|_| words[rng.gen_range(0..words.len())]));
            vec![Value::Int(i as i64), Value::Float(rng.gen::<f64>() * 5.0), Value::Str(body.join(" ")), vec_value(&unit(&mut rng))]
        })
        .collect();
    db.load(docs).unwrap();
    db.optimize().unwrap();
    let q = unit(&mut rng);
    (db, Params::new().set("q", vec_value(&q)))
}

fn ids(r: &ResultSet) -> Vec<i64> {
    r.rows.iter().map(|r| r[0].as_i64().unwrap()).collect()
}

/// Pages of `page` rows from OFFSET 0 to `total`, one after another.
fn paged(db: &Db, p: &Params, order: &str, page: usize, total: usize) -> Vec<i64> {
    let mut out = Vec::new();
    for k in (0..total).step_by(page) {
        out.extend(ids(&query(db, &format!("SELECT id FROM t ORDER BY {order} DESC LIMIT {page} OFFSET {k}"), p)));
    }
    out
}

#[test]
fn pages_of_a_fused_ranking_are_pages_of_one_ranking() {
    let dir = tempfile::tempdir().unwrap();
    let (db, p) = fused_db(dir.path());
    let orders = [
        "rrf(bm25(body, 'word'), similarity(emb, :q))",
        "bm25(body, 'word') + 2 * similarity(emb, :q)",
        "minmax(bm25(body, 'word')) + minmax(similarity(emb, :q))",
        "similarity(emb, :q) + 0.1 * rating",
    ];
    for order in orders {
        // Before: 11 pages of 20 against LIMIT 220 showed between 3 and 85
        // rows twice. The default depth is 100: one query of LIMIT 100 and
        // the five pages of 20 that make it are the same rows in the same
        // order.
        let one = ids(&query(&db, &format!("SELECT id FROM t ORDER BY {order} DESC LIMIT 100"), &p));
        let pages = paged(&db, &p, order, 20, 100);
        assert_eq!(pages, one, "{order}: pages of 20 against LIMIT 100");
        // Paging on past the candidates: no row comes twice, and the page
        // that runs out says the candidates did.
        let deep = paged(&db, &p, order, 20, 400);
        assert_eq!(deep.len(), deep.iter().collect::<HashSet<_>>().len(), "{order}: a row on two pages");
        assert_eq!(deep[..100], one[..], "{order}");
        if deep.len() < 400 {
            let past = query(&db, &format!("SELECT id FROM t ORDER BY {order} DESC LIMIT 20 OFFSET {}", deep.len() / 20 * 20), &p);
            assert!(past.stats.truncated && past.stats.plan.iter().any(|l| l.contains("reaches past")), "{order}: {:?}", past.stats.plan);
        }
    }
    // With a depth given, any page size and LIMIT up to it agree.
    let order = "rrf(bm25(body, 'word'), similarity(emb, :q), depth => 300)";
    let one = ids(&query(&db, &format!("SELECT id FROM t ORDER BY {order} DESC LIMIT 220"), &p));
    assert_eq!(one.len(), 220);
    assert_eq!(paged(&db, &p, order, 20, 220), one, "11 pages of 20 against LIMIT 220");
    assert_eq!(paged(&db, &p, order, 55, 220), one, "4 pages of 55");
    // Ranked by one score, pages were exact and stay so.
    let order = "bm25(body, 'word')";
    let one = ids(&query(&db, &format!("SELECT id FROM t ORDER BY {order} DESC LIMIT 220"), &p));
    assert_eq!(paged(&db, &p, order, 20, 220), one);
}

/// The PageRank of `i0`'s neighbourhood with every edge weight `w`.
fn ppr_scores(dir: &std::path::Path, w: f64, opts: &str) -> (Vec<(String, f64)>, ResultSet) {
    let items = Schema::new("items", vec![Field::new("id", FieldType::Keyword), Field::new("title", FieldType::Text { positions: false })], Some("id")).unwrap();
    let db = Db::create(&dir.join(format!("g{w}{}.strata", opts.len())), items, DbOptions { sync: false, ..DbOptions::default() }).unwrap();
    db.load((0..40).map(|k| vec![Value::Str(format!("i{k}")), Value::Str(format!("item {k}"))])).unwrap();
    run(&db, "CREATE TABLE cooc (id KEYWORD PRIMARY KEY, src KEYWORD NOT NULL REFERENCES items, dst KEYWORD NOT NULL REFERENCES items, w FLOAT DEFAULT 0) OPTIONS(edges = 'src, dst, w')", &Params::new());
    let mut edges: Vec<(usize, usize)> = (1..16).map(|k| (0, k)).collect();
    edges.extend((1..16).map(|k| (k, k + 15)));
    edges.extend([(1, 2), (2, 3), (16, 17)]);
    db.load_into("cooc", edges.iter().map(|&(a, b)| vec![Value::Str(format!("i{a}>i{b}")), Value::Str(format!("i{a}")), Value::Str(format!("i{b}")), Value::Float(w)])).unwrap();
    let sql = format!("SELECT id, score() FROM items ORDER BY ppr(cooc, (SELECT id FROM items WHERE id = 'i0'){opts}) DESC LIMIT 8");
    let r = query(&db, &sql, &Params::new());
    (r.rows.iter().map(|r| (format!("{:?}", r[0]), r[1].as_f64().unwrap())).collect(), r)
}

#[test]
fn ppr_does_not_change_when_every_weight_is_scaled() {
    let dir = tempfile::tempdir().unwrap();
    // Before: weights of 1 gave 8 rows, weights of 1000 none.
    let (one, _) = ppr_scores(dir.path(), 1.0, "");
    assert_eq!(one.len(), 8);
    for w in [100.0, 1000.0, 0.01] {
        let (got, r) = ppr_scores(dir.path(), w, "");
        assert_eq!(got.len(), one.len(), "weights {w}: {:?}", r.stats.plan);
        for ((ka, a), (kb, b)) in one.iter().zip(&got) {
            assert!((a - b).abs() <= 1e-9 * a.abs().max(1.0), "weights {w}: {ka} {a} vs {kb} {b}");
        }
        let (ka, kb): (HashSet<_>, HashSet<_>) = (one.iter().map(|x| &x.0).collect(), got.iter().map(|x| &x.0).collect());
        assert_eq!(ka, kb, "weights {w}");
        assert!(!r.stats.max_pushes_reached);
    }
}

#[test]
fn ppr_says_it_stopped_at_max_pushes() {
    let dir = tempfile::tempdir().unwrap();
    let (_, r) = ppr_scores(dir.path(), 1.0, ", max_pushes => 2");
    assert!(r.stats.max_pushes_reached, "{:?}", r.stats.plan);
    assert!(r.stats.plan.iter().any(|l| l.contains("stopped at max_pushes")), "{:?}", r.stats.plan);
    let json = serde_json::to_value(&r.stats).unwrap();
    assert_eq!(json["max_pushes_reached"].as_bool(), Some(true));
    let (_, r) = ppr_scores(dir.path(), 1.0, "");
    assert!(!r.stats.max_pushes_reached && !r.stats.plan.iter().any(|l| l.contains("stopped at max_pushes")));
}
