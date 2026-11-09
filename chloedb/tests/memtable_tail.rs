//! The memtable's last chunk, which every write changes, built in pieces
//! (`DbOptions::memtable_tail_docs`): the first query after a write
//! rebuilds only the last piece, and queries answer as with one piece.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use chloedb::db::Db;
use chloedb::query::Params;
use chloedb::segment::build::BuildOptions;
use chloedb::storage::DbOptions;
use chloedb::{Field, FieldType, Schema, Value, VectorIndexKind};

const DIM: usize = 8;
const WORDS: [&str; 8] = ["apple", "pear", "plum", "fig", "kiwi", "lime", "date", "yuzu"];

fn schema() -> Schema {
    Schema::new(
        "t",
        vec![
            Field::new("id", FieldType::Keyword),
            Field::new("n", FieldType::Int),
            Field::new("body", FieldType::Text { positions: true }),
            Field::new("emb", FieldType::Vector { dim: DIM, index: VectorIndexKind::Hnsw { m: 8, ef_construction: 40 } }),
        ],
        Some("id"),
    )
    .unwrap()
}

fn options(chunk: usize, tail: usize, publish: bool) -> DbOptions {
    DbOptions {
        mem_build: Some(BuildOptions::default().set("vector.index", "flat")),
        memtable_max_docs: 1_000_000,
        memtable_chunk_docs: chunk,
        memtable_tail_docs: tail,
        sync: false,
        publish_snapshots: publish,
        ..DbOptions::default()
    }
}

fn vector(rng: &mut StdRng) -> Vec<f32> {
    (0..DIM).map(|_| rng.gen_range(-1.0f32..1.0)).collect()
}

fn row(i: u64, n: i64, rng: &mut StdRng) -> Vec<Value> {
    let body: Vec<&str> = (0..rng.gen_range(1..6)).map(|_| WORDS[rng.gen_range(0..WORDS.len())]).collect();
    vec![Value::Str(format!("k{i:04}")), Value::Int(n), Value::Str(body.join(" ")), Value::Vector(vector(rng).into())]
}

fn insert(db: &Db, rows: impl IntoIterator<Item = Vec<Value>>) {
    let mut tx = db.storage().begin().unwrap();
    for r in rows {
        tx.upsert(r).unwrap();
    }
    tx.commit().unwrap();
}

/// Rows of `sql`, scores rounded to the sixth decimal.
fn rows(db: &Db, sql: &str, p: &Params) -> Vec<Vec<Value>> {
    let rs = db.query(sql, p).unwrap_or_else(|e| panic!("{sql}: {e}"));
    rs.rows
        .into_iter()
        .map(|r| r.into_iter().map(|v| if let Value::Float(f) = v { Value::Float((f * 1e6).round() / 1e6) } else { v }).collect())
        .collect()
}

/// A query after a write builds the last piece alone; a full chunk is one
/// segment.
#[test]
fn a_write_rebuilds_only_the_last_piece() {
    for publish in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::create(&dir.path().join("t.strata"), schema(), options(512, 64, publish)).unwrap();
        let mut rng = StdRng::seed_from_u64(1);
        for i in 0..300 {
            insert(&db, [row(i, i as i64, &mut rng)]);
        }
        let search = || db.query("SELECT id FROM t WHERE MATCH(body, 'plum') LIMIT 5", &Params::new()).unwrap();
        let b = chloedb::storage::tail_builds();
        search();
        assert_eq!(chloedb::storage::tail_builds() - b, 5, "pieces of 64 of 300 rows");
        assert_eq!(db.snapshot().segments().len(), 5);
        let b = chloedb::storage::tail_builds();
        search();
        assert_eq!(chloedb::storage::tail_builds() - b, 0, "nothing written since");
        for i in 300..340 {
            insert(&db, [row(i, i as i64, &mut rng)]);
            let b = chloedb::storage::tail_builds();
            search();
            assert_eq!(chloedb::storage::tail_builds() - b, 1, "a write after {i} rows");
        }
        // The chunk fills: its 512 rows are one segment, the next chunk's
        // first rows a piece.
        insert(&db, (340..520).map(|i| row(i, i as i64, &mut rng)));
        search();
        let segs = db.snapshot().segments().iter().map(|s| s.n_live).collect::<Vec<_>>();
        assert_eq!(segs, vec![512, 8], "publish {publish}");
    }
}

/// Inserts, replaces, deletes, values set in place and flushes, one or a
/// few rows a commit, on a database whose last chunk is built in pieces
/// and on one whose last chunk is one piece: every query answers the same.
#[test]
fn pieces_answer_what_one_piece_answers() {
    for seed in 0..6u64 {
        let dir = tempfile::tempdir().unwrap();
        let publish = seed % 2 == 1;
        let chunk = [256, 512, 192][seed as usize % 3];
        let pieces = Db::create(&dir.path().join("p.strata"), schema(), options(chunk, 64, publish)).unwrap();
        let whole = Db::create(&dir.path().join("w.strata"), schema(), options(chunk, 0, publish)).unwrap();
        let mut rng = StdRng::seed_from_u64(seed);
        let mut next = 0u64;
        for step in 0..700 {
            let dbs = [&pieces, &whole];
            match rng.gen_range(0..10) {
                0..=4 => {
                    let rs: Vec<Vec<Value>> = (0..rng.gen_range(1..4)).map(|_| {
                        next += 1;
                        row(next, step, &mut rng)
                    }).collect();
                    dbs.iter().for_each(|db| insert(db, rs.clone()));
                }
                5 => {
                    let r = row(rng.gen_range(0..=next), -step, &mut rng);
                    dbs.iter().for_each(|db| insert(db, [r.clone()]));
                }
                6 => {
                    let k = Params::new().set("k", Value::Str(format!("k{:04}", rng.gen_range(0..=next))));
                    dbs.iter().for_each(|db| {
                        db.sql("DELETE FROM t WHERE id = :k", &k).unwrap();
                    });
                }
                7 => {
                    let k = Params::new().set("k", Value::Str(format!("k{:04}", rng.gen_range(0..=next)))).set("n", Value::Int(1000 + step));
                    dbs.iter().for_each(|db| {
                        db.sql("UPDATE t SET n = :n WHERE id = :k", &k).unwrap();
                    });
                }
                8 if step % 97 == 0 => dbs.iter().for_each(|db| db.storage().flush().unwrap()),
                _ => {}
            }
            let w = WORDS[rng.gen_range(0..WORDS.len())];
            let p = Params::new()
                .set("w", Value::Str(w.into()))
                .set("q", Value::Vector(vector(&mut rng).into()))
                .set("k", Value::Str(format!("k{:04}", rng.gen_range(0..=next))))
                .set("lo", Value::Int(rng.gen_range(0..700)));
            for sql in [
                "SELECT id, n FROM t ORDER BY id",
                "SELECT id, score() FROM t ORDER BY bm25(body, :w) DESC, id LIMIT 7",
                "SELECT id FROM t WHERE MATCH(body, :w) AND n >= :lo ORDER BY n DESC, id LIMIT 5",
                "SELECT id FROM t ORDER BY similarity(emb, :q) DESC LIMIT 5",
                "SELECT * FROM t WHERE id = :k",
                "SELECT COUNT(*), SUM(n) FROM t WHERE MATCH(body, :w)",
            ] {
                assert_eq!(rows(&pieces, sql, &p), rows(&whole, sql, &p), "seed {seed} step {step}: {sql}");
            }
        }
    }
}
