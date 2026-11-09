//! Searching segments on several threads returns exactly what one thread
//! returns.

use chloedb::data;
use chloedb::db::Db;
use chloedb::query::exec::ExecOptions;
use chloedb::query::Params;
use chloedb::storage::DbOptions;
use chloedb::value::Value;
use chloedb::VectorIndexKind;

#[test]
fn threads_do_not_change_answers() {
    let docs = data::synthetic(4000, 16, 11);
    let dir = tempfile::tempdir().unwrap();
    let opts = DbOptions { segment_max_docs: 300, ..DbOptions::default() };
    let db = Db::create(&dir.path().join("p.strata"), data::beir_schema(16, VectorIndexKind::Hnsw { m: 12, ef_construction: 60 }), opts).unwrap();
    db.load(docs.iter().cloned()).unwrap();
    // Deletes give segments live filters.
    let mut t = db.storage().begin().unwrap();
    for i in (0..4000).step_by(7) {
        t.delete(&Value::Str(format!("d{i:07}"))).unwrap();
    }
    t.commit().unwrap();
    assert!(db.snapshot().segments().len() >= 10);
    let title = docs[9][data::col::TITLE].as_str().unwrap().to_string();
    let params = Params::new().set("q", docs[9][data::col::EMB].clone()).set("t", Value::Str(title));
    let queries = [
        "SELECT id, score() FROM docs ORDER BY bm25(body, :t) DESC LIMIT 20",
        "SELECT id, score() FROM docs WHERE year BETWEEN 2000 AND 2010 ORDER BY bm25(body, :t) DESC LIMIT 20",
        "SELECT id, score() FROM docs ORDER BY similarity(emb, :q) DESC LIMIT 20",
        "SELECT id, score() FROM docs WHERE category IN ('c00', 'c05') ORDER BY similarity(emb, :q) DESC LIMIT 20",
        "SELECT id, score() FROM docs WHERE rating < 1 ORDER BY similarity(emb, :q, strategy => 'postfilter') DESC LIMIT 20",
        "SELECT id, score() FROM docs ORDER BY 0.4 * minmax(bm25(body, :t)) + 0.6 * minmax(similarity(emb, :q)) DESC LIMIT 20",
        "SELECT id, score() FROM docs ORDER BY rrf(bm25(body, :t), similarity(emb, :q)) DESC LIMIT 20",
    ];
    for sql in queries {
        let one = db.query_with(sql, &params, &ExecOptions::default()).unwrap();
        for threads in [2, 4, 8] {
            // fan_out_rows 0: these segments are far below the size that
            // fans out by default, and the point is to compare threads.
            let many = db.query_with(sql, &params, &ExecOptions { threads, fan_out_rows: 0, ..ExecOptions::default() }).unwrap();
            assert_eq!(one.rows, many.rows, "{sql} on {threads} threads");
            assert!(many.stats.plan.iter().any(|l| l.contains("threads")), "{sql}: {:?}", many.stats.plan);
        }
    }
}

/// Segments searched one after another start at the `k`-th score the
/// earlier ones found ("above" in the plan): the same rows and scores as
/// one segment holding them all, for plain BM25, with filters, with a key
/// tiebreak (every row tied with the `k`-th is kept) and for rank-safe
/// boosts, whose values can be minus infinity (a NULL rating).
#[test]
fn a_floor_from_earlier_segments_changes_no_answer() {
    let mut docs = data::synthetic(4000, 16, 5);
    for d in docs.iter_mut().step_by(9) {
        d[data::col::RATING] = Value::Null;
    }
    let dir = tempfile::tempdir().unwrap();
    let open = |name: &str, seg: usize| {
        let opts = DbOptions { segment_max_docs: seg, ..DbOptions::default() };
        let db = Db::create(&dir.path().join(name), data::beir_schema(16, VectorIndexKind::Flat), opts).unwrap();
        db.load(docs.iter().cloned()).unwrap();
        let mut t = db.storage().begin().unwrap();
        for i in (0..4000).step_by(11) {
            t.delete(&Value::Str(format!("d{i:07}"))).unwrap();
        }
        t.commit().unwrap();
        db
    };
    let (many, one) = (open("many.strata", 250), open("one.strata", 100_000));
    assert!(many.snapshot().segments().len() >= 15);
    assert_eq!(one.snapshot().segments().len(), 1);
    let shapes = [
        "SELECT id, score() FROM docs ORDER BY bm25(body, :t{algo}) DESC LIMIT {k}",
        "SELECT id, score() FROM docs WHERE year >= 2005 ORDER BY bm25(body, :t{algo}) DESC LIMIT {k}",
        "SELECT id, score() FROM docs ORDER BY bm25(body, :t{algo}) DESC, id LIMIT {k}",
        "SELECT id, score() FROM docs ORDER BY bm25(body, :t{algo}) * (1 + 0.2 * rating) DESC, id LIMIT {k}",
        "SELECT id, score() FROM docs WHERE category IN ('c00', 'c01', 'c02') ORDER BY bm25(body, :t{algo}) * decay(year, 2015, 3) DESC, id LIMIT {k}",
    ];
    let (mut above, mut runs) = (0, 0);
    for (qi, i) in (0..4000).step_by(97).enumerate() {
        let words: Vec<&str> = docs[i][data::col::TITLE].as_str().unwrap().split(' ').take(1 + qi % 4).collect();
        let params = Params::new().set("t", Value::Str(words.join(" ")));
        for shape in shapes {
            let algo = ["", ", algo => 'maxscore'", ", algo => 'bmw'", ", algo => 'exhaustive'"][qi % 4];
            let k = [1, 10, 40, 1000][qi / 4 % 4];
            let sql = shape.replace("{algo}", algo).replace("{k}", &k.to_string());
            let want = one.query(&sql, &params).unwrap();
            let got = many.query_with(&sql, &params, &ExecOptions { threads: 1, ..ExecOptions::default() }).unwrap();
            let scores = |r: &chloedb::query::exec::ResultSet| r.scores.iter().map(|s| s.to_bits()).collect::<Vec<_>>();
            assert_eq!(scores(&got), scores(&want), "{sql} {params:?}: scores");
            if sql.contains(", id") {
                assert_eq!(got.rows, want.rows, "{sql} {params:?}");
            }
            above += got.stats.plan.iter().filter(|l| l.contains(", above ")).count();
            runs += 1;
        }
    }
    assert!(above > runs * 5, "{above} segments of {runs} queries started above a floor");
}

#[test]
fn large_segments_build_ivf_instead_of_hnsw() {
    use chloedb::segment::build::BuildOptions;
    let docs = data::synthetic(900, 16, 2);
    let dir = tempfile::tempdir().unwrap();
    let opts = DbOptions { segment_max_docs: 600, build: BuildOptions::default().set("vector.hnsw.max_docs", 400), ..DbOptions::default() };
    let db = Db::create(&dir.path().join("i.strata"), data::beir_schema(16, VectorIndexKind::Hnsw { m: 12, ef_construction: 60 }), opts).unwrap();
    db.load(docs).unwrap();
    let snap = db.snapshot();
    let kinds: Vec<String> = db.readers(&snap).unwrap().iter().map(|r| r.vector(data::col::EMB).unwrap().unwrap().index_kind().to_string()).collect();
    assert_eq!(kinds, vec!["ivf", "hnsw"], "600 vectors exceed the bound, 300 do not");
}
