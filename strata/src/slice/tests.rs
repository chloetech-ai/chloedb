use std::collections::HashSet;
use std::path::Path;

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use super::*;
use crate::db::Db;
use crate::schema::{Field, FieldType, Schema};
use crate::storage::DbOptions;
use crate::text::Bm25Params;
use crate::value::{Document, Value};

const WORDS: &[&str] = &[
    "pizza", "coffee", "shop", "bar", "deli", "bakery", "park", "museum", "gym", "salon", "thai", "sushi", "taco", "bagel", "bank", "pharmacy",
    "hardware", "books", "wine", "tea", "noodle", "burger", "bike", "dental", "laundry", "florist", "vegan", "ramen", "pub", "diner",
];

fn schema() -> Schema {
    Schema::new(
        "places",
        vec![
            Field::new("id", FieldType::Int),
            Field::new("body", FieldType::Text { positions: false }),
            Field::new("kind", FieldType::Keyword),
            Field::new("lat", FieldType::Float),
            Field::new("lon", FieldType::Float),
        ],
        Some("id"),
    )
    .unwrap()
}

/// A word by a Zipf-like law, so that some are common and some rare.
fn word(rng: &mut StdRng) -> &'static str {
    let r: f64 = rng.gen();
    WORDS[((WORDS.len() as f64).powf(r) as usize - 1).min(WORDS.len() - 1)]
}

/// `n` places in a 12 km square around (40.7, -74.0), denser at the
/// centre, each a few words long.
fn places(n: usize, seed: u64) -> Vec<Document> {
    let mut rng = StdRng::seed_from_u64(seed);
    (0..n)
        .map(|i| {
            let len = rng.gen_range(1..6);
            let body: Vec<&str> = (0..len).map(|_| word(&mut rng)).collect();
            let spread = if rng.gen_bool(0.5) { 0.02 } else { 0.06 };
            let lat = 40.7 + rng.gen_range(-spread..spread);
            let lon = -74.0 + rng.gen_range(-spread..spread) * 1.3;
            vec![Value::Int(i as i64), Value::Str(body.join(" ")), Value::Str(word(&mut rng).into()), Value::Float(lat), Value::Float(lon)]
        })
        .collect()
}

fn cloud(dir: &Path, docs: Vec<Document>) -> Db {
    let db = Db::create(&dir.join("cloud.strata"), schema(), DbOptions::default()).unwrap();
    // Two loads, so that the statistics and best scores span segments.
    let half = docs.len() / 2;
    let mut docs = docs;
    let rest = docs.split_off(half);
    db.load(docs).unwrap();
    db.load(rest).unwrap();
    db
}

fn opts() -> BuildOptions {
    BuildOptions { table: None, text: "body".into(), lat: "lat".into(), lon: "lon".into(), grid: Grid::new(40.7, -74.0, 1000.0).unwrap(), bm25: Bm25Params::default() }
}

fn fetch(dev: &mut Device, dir: &Path, lat: f64, lon: f64, reach: i32) {
    let p = dev.plan(lat, lon, reach);
    for t in p.drop {
        dev.drop_tile(t).unwrap();
    }
    for (t, _) in p.fetch {
        let bytes = std::fs::read(dir.join(t.path())).unwrap();
        dev.add_tile(t, &bytes).unwrap();
    }
}

/// The cloud's answer to a search: its keys and scores.
fn cloud_answer(db: &Db, m: &Manifest, s: &Search) -> (Vec<Value>, Vec<f64>) {
    let rs = db.query(&s.sql(m), &s.params(m)).unwrap();
    (rs.keys, rs.scores)
}

/// Keys of the rows scoring above the k-th: those the cloud and the device
/// must agree on exactly (rows tied with the k-th may differ).
fn above_kth(keys: &[Value], scores: &[f64]) -> HashSet<String> {
    let kth = *scores.last().unwrap_or(&f64::INFINITY);
    keys.iter().zip(scores).filter(|(_, &s)| s > kth).map(|(k, _)| format!("{k:?}")).collect()
}

#[test]
fn best_scores_bound_every_document() {
    let dir = tempfile::tempdir().unwrap();
    let db = cloud(dir.path(), places(3000, 1));
    let built = build(&db, &opts(), &dir.path().join("slice")).unwrap();
    let m = &built.manifest;
    assert_eq!(m.n_docs, 3000);
    assert_eq!(built.rows, 3000);
    assert_eq!(m.tiles.iter().map(|t| t.rows).sum::<u64>(), 3000);
    let words = m.word_map();
    assert!(words.len() >= 25, "{} words", words.len());
    for w in WORDS {
        let term = &query_terms(&m.schema, "body", w).unwrap()[0].0;
        if !words.contains_key(term.as_str()) {
            continue;
        }
        // The best score of the word alone, over the whole table.
        let rs = db.query("SELECT id FROM places ORDER BY bm25(body, :q, syntax => 'plain') DESC LIMIT 1", &Params::new().set("q", Value::Str(w.to_string()))).unwrap();
        let top = rs.scores[0] as f32;
        let best = words[term.as_str()].1;
        assert!(best >= top, "{w}: best {best} below the top score {top}");
        assert!(best <= top * 1.001, "{w}: best {best} far above the top score {top}");
    }
}

#[test]
fn manifest_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let db = cloud(dir.path(), places(500, 2));
    let built = build(&db, &opts(), &dir.path().join("slice")).unwrap();
    let text = std::fs::read_to_string(dir.path().join("slice/manifest.json")).unwrap();
    assert_eq!(text.len() as u64, built.manifest_bytes);
    assert_eq!(Manifest::from_json(&text).unwrap(), built.manifest);
    let mut later = built.manifest.clone();
    later.format = FORMAT + 1;
    assert!(Manifest::from_json(&later.to_json()).unwrap_err().to_string().contains("format"));
}

#[test]
fn proved_answers_are_the_clouds() {
    let dir = tempfile::tempdir().unwrap();
    let db = cloud(dir.path(), places(4000, 3));
    let out = dir.path().join("slice");
    let m = build(&db, &opts(), &out).unwrap().manifest;
    let mut dev = Device::create(&dir.path().join("device"), m.clone()).unwrap();
    let mut rng = StdRng::seed_from_u64(4);
    let (mut proved, mut searched) = (0, 0);
    let (mut lat, mut lon) = (40.7, -74.0);
    for step in 0..60 {
        // A walk across the city, the device fetching around it.
        lat += rng.gen_range(-0.004..0.004);
        lon += rng.gen_range(-0.005..0.005);
        fetch(&mut dev, &out, lat, lon, 1 + (step % 2));
        let held: u64 = dev.held().map(|(_, n)| n as u64).sum();
        let rs = dev.db().query("SELECT COUNT(*) FROM places", &Params::new()).unwrap();
        assert_eq!(rs.rows[0][0].as_i64().unwrap() as u64, held, "the local table holds exactly the tiles held");
        for _ in 0..5 {
            let q = (0..rng.gen_range(1..4)).map(|_| word(&mut rng)).collect::<Vec<_>>().join(" ");
            for (k, d0) in [(1, 1000.0), (3, 1000.0), (10, 1000.0), (10, 3000.0)] {
                let s = Search::new(&q, lat, lon, k, d0).unwrap();
                let a = dev.search(&s).unwrap();
                searched += 1;
                if !a.proved {
                    continue;
                }
                proved += 1;
                let (keys, scores) = cloud_answer(&db, &m, &s);
                assert_eq!(a.rows.scores, scores, "{q:?} k {k} d0 {d0} at ({lat}, {lon}): proved, so the scores are the cloud's");
                assert_eq!(above_kth(&a.rows.keys, &a.rows.scores), above_kth(&keys, &scores));
            }
        }
    }
    // The square is 3 or 5 km on a side with d0 1 km: most are proved.
    assert!(proved * 3 > searched, "proved {proved} of {searched}");
    assert!(proved < searched, "some searches must reach past the square");
}

#[test]
fn local_scores_are_the_clouds() {
    // Proved or not, every place held scores as in the cloud.
    let dir = tempfile::tempdir().unwrap();
    let db = cloud(dir.path(), places(2000, 5));
    let out = dir.path().join("slice");
    let m = build(&db, &opts(), &out).unwrap().manifest;
    let mut dev = Device::create(&dir.path().join("device"), m.clone()).unwrap();
    fetch(&mut dev, &out, 40.71, -74.01, 1);
    for q in ["pizza", "coffee shop", "thai noodle bar", "pizza pizza"] {
        let s = Search::new(q, 40.71, -74.01, 50, 1000.0).unwrap();
        let a = dev.search(&s).unwrap();
        for (key, score) in a.rows.keys.iter().zip(&a.rows.scores) {
            let one = s.sql(&m).replacen("FROM places", &format!("FROM places WHERE id = {}", key.as_i64().unwrap()), 1).replace("LIMIT 50", "LIMIT 1");
            let rs = db.query(&one, &s.params(&m)).unwrap();
            assert_eq!(rs.scores[0], *score, "{q}: row {key:?}");
        }
    }
}

#[test]
fn holding_everything_proves_everything() {
    let dir = tempfile::tempdir().unwrap();
    let db = cloud(dir.path(), places(1500, 6));
    let out = dir.path().join("slice");
    let m = build(&db, &opts(), &out).unwrap().manifest;
    let mut dev = Device::create(&dir.path().join("device"), m.clone()).unwrap();
    fetch(&mut dev, &out, 40.7, -74.0, 100);
    assert_eq!(dev.edge(40.7, -74.0), f64::INFINITY);
    for q in ["bagel", "wine bar", "nothing here"] {
        let s = Search::new(q, 40.69, -73.99, 10, 3000.0).unwrap();
        let a = dev.search(&s).unwrap();
        assert!(a.proved, "{q}");
        assert_eq!(a.rows.scores, cloud_answer(&db, &m, &s).1, "{q}");
    }
}

#[test]
fn plan_fetches_the_square_and_drops_far_tiles() {
    let dir = tempfile::tempdir().unwrap();
    let db = cloud(dir.path(), places(3000, 7));
    let out = dir.path().join("slice");
    let m = build(&db, &opts(), &out).unwrap().manifest;
    let mut dev = Device::create(&dir.path().join("device"), m.clone()).unwrap();
    let g = m.grid;
    // The middle of tile (0, 0).
    let (lat, lon) = (40.7 + 500.0 / KY, -74.0 + 500.0 / g.kx());
    let p = dev.plan(lat, lon, 1);
    assert!(!p.fetch.is_empty() && p.fetch.len() <= 9);
    assert!(p.drop.is_empty());
    // Nearest first: the user's own tile leads when it has rows.
    let u = g.tile(lat, lon);
    assert_eq!(u, Tile { x: 0, y: 0 });
    if m.tiles.iter().any(|t| t.tile() == u) {
        assert_eq!(p.fetch[0].0, u);
    }
    fetch(&mut dev, &out, lat, lon, 1);
    assert!(dev.plan(lat, lon, 1).fetch.is_empty());
    // 3 km east: the old square's west column is two rings out, dropped.
    let east = lon + 3000.0 / g.kx();
    let p = dev.plan(lat, east, 1);
    let v = g.tile(lat, east);
    assert!(p.drop.iter().all(|t| (t.x - v.x).abs().max((t.y - v.y).abs()) > 2));
    assert!(p.drop.iter().any(|t| t.x == u.x - 1));
    // The edge: inside a held tile, at least the distance to its border.
    let e = dev.edge(lat, lon);
    assert!(e > 1499.0 && e < 2501.0, "{e}");
}

#[test]
fn refuses_tables_it_cannot_carry() {
    let dir = tempfile::tempdir().unwrap();
    let s = Schema::new("p", vec![Field::new("id", FieldType::Int), Field::new("body", FieldType::Text { positions: false }), Field::new("lat", FieldType::Float), Field::new("lon", FieldType::Float)], None).unwrap();
    let db = Db::create(&dir.path().join("a.strata"), s, DbOptions::default()).unwrap();
    let e = build(&db, &opts(), &dir.path().join("out")).unwrap_err().to_string();
    assert!(e.contains("primary key"), "{e}");
    let s = Schema::new("p", vec![Field::new("id", FieldType::Int), Field::new("body", FieldType::Text { positions: false }), Field::new("lat", FieldType::Int), Field::new("lon", FieldType::Float)], Some("id")).unwrap();
    let db = Db::create(&dir.path().join("b.strata"), s, DbOptions::default()).unwrap();
    let e = build(&db, &opts(), &dir.path().join("out")).unwrap_err().to_string();
    assert!(e.contains("lat is not a single-valued FLOAT"), "{e}");
}

#[test]
fn grid_distances() {
    let g = Grid::new(40.0, -74.0, 1000.0).unwrap();
    let t = Tile { x: 0, y: 0 };
    assert_eq!(g.distance(500.0, 500.0, t), 0.0);
    assert_eq!(g.distance(-300.0, 500.0, t), 300.0);
    assert_eq!(g.distance(1300.0, 1400.0, t), 500.0);
    assert_eq!(g.tile(40.0, -74.0), t);
    let (x, y) = g.xy(40.0 + 1500.0 / KY, -74.0 - 10.0 / g.kx());
    assert!((x + 10.0).abs() < 1e-6 && (y - 1500.0).abs() < 1e-6);
    assert_eq!(g.tile(40.0 + 1500.0 / KY, -74.0 - 10.0 / g.kx()), Tile { x: -1, y: 1 });
    assert!(Grid::new(40.0, -74.0, 0.0).is_err());
}

#[test]
fn query_terms_count_repeats() {
    let t = query_terms(&schema(), "body", "Pizza pizza, coffee").unwrap();
    assert_eq!(t.iter().map(|(_, w)| *w).collect::<Vec<_>>(), vec![2.0, 1.0], "{t:?}");
}
