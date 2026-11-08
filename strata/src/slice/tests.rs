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
    BuildOptions { table: None, key: BuildKey::Geo { text: "body".into(), lat: "lat".into(), lon: "lon".into(), grid: Grid::new(40.7, -74.0, 1000.0).unwrap(), bm25: Bm25Params::default() } }
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
fn cloud_answer(db: &Db, m: &Manifest, s: &dyn SliceQuery) -> (Vec<Value>, Vec<f64>) {
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
    assert_eq!(m.geo().unwrap().n_docs, 3000);
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
    let g = m.geo().unwrap().grid;
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

#[test]
fn search_or_ask_asks_the_cloud_when_the_device_cannot_answer() {
    let dir = tempfile::tempdir().unwrap();
    let db = cloud(dir.path(), places(3000, 7));
    let out = dir.path().join("slice");
    let m = build(&db, &opts(), &out).unwrap().manifest;
    let mut dev = Device::create(&dir.path().join("device"), m.clone()).unwrap();
    fetch(&mut dev, &out, 40.7, -74.0, 2);
    let asked = std::cell::Cell::new(0);
    let ask = |sql: &str, p: &Params| {
        asked.set(asked.get() + 1);
        db.query(sql, p)
    };
    let mut whys = HashSet::new();
    for q in ["pizza", "coffee shop", "ramen pub diner", "nothing here at all"] {
        for (k, d0) in [(1, 1000.0), (10, 1000.0), (50, 3000.0)] {
            let s = Search::new(q, 40.7, -74.0, k, d0).unwrap();
            let before = asked.get();
            let o = dev.search_or_ask(&s, ask).unwrap();
            let a = &o.answer;
            whys.insert(a.why);
            assert_eq!(a.on_device, asked.get() == before, "{q} k {k}: asked the cloud exactly when not on the device");
            assert_eq!(a.on_device, matches!(a.why, Why::Proved | Why::NoMatch), "{q} k {k}: {:?}", a.why);
            match a.why {
                Why::Short => assert!(a.rows.scores.len() < k),
                Why::Bound => assert!(a.rows.scores[k - 1] < a.bound),
                Why::NoMatch => assert!(a.text_bound == 0.0 && o.rows.rows.is_empty()),
                _ => {}
            }
            // Either way the answer is the cloud's.
            assert_eq!(o.rows.scores, cloud_answer(&db, &m, &s).1, "{q} k {k} d0 {d0}");
        }
    }
    assert!(whys.contains(&Why::Proved) && whys.contains(&Why::NoMatch), "{whys:?}");
    assert!(whys.contains(&Why::Short) || whys.contains(&Why::Bound), "{whys:?}");

    // A stale device asks the cloud for everything, proved or not.
    dev.set_stale(true);
    let s = Search::new("pizza", 40.7, -74.0, 1, 1000.0).unwrap();
    let before = asked.get();
    let o = dev.search_or_ask(&s, ask).unwrap();
    assert!(o.answer.proved && o.answer.stale && !o.answer.on_device);
    assert_eq!((o.answer.why, asked.get()), (Why::Stale, before + 1));
    dev.set_stale(false);
    assert!(dev.search(&s).unwrap().on_device);
}

#[test]
fn an_answer_carries_the_request_that_asks_the_cloud() {
    let dir = tempfile::tempdir().unwrap();
    let db = cloud(dir.path(), places(800, 8));
    let out = dir.path().join("slice");
    let m = build(&db, &opts(), &out).unwrap().manifest;
    let mut dev = Device::create(&dir.path().join("device"), m.clone()).unwrap();
    fetch(&mut dev, &out, 40.7, -74.0, 1);
    let s = Search::new("pizza bar", 40.7, -74.0, 5, 1000.0).unwrap();
    let a = dev.search(&s).unwrap();
    let j = a.to_json(&s, &m);
    assert_eq!(j["scores"].as_array().unwrap().len(), a.rows.scores.len());
    assert_eq!(j["why"], serde_json::json!(a.why.as_str()));
    let body: serde_json::Value = serde_json::from_str(j["ask"].as_str().unwrap()).unwrap();
    assert_eq!(body["sql"], serde_json::json!(s.sql(&m)));
    // A whole-number float stays a float, so the cloud scores with it as
    // the device does.
    assert!(body["params"]["d0sq"].is_f64(), "{}", body["params"]);
    assert_eq!(body["params"]["q"], serde_json::json!("pizza bar"));
}

#[test]
fn a_new_slice_replaces_the_devices_old_database() {
    let dir = tempfile::tempdir().unwrap();
    let db = cloud(dir.path(), places(600, 9));
    let out = dir.path().join("slice");
    let m = build(&db, &opts(), &out).unwrap().manifest;
    let dev_dir = dir.path().join("device");
    std::fs::create_dir_all(dev_dir.join("keep-me")).unwrap();
    let mut dev = Device::create(&dev_dir, m.clone()).unwrap();
    fetch(&mut dev, &out, 40.7, -74.0, 1);
    drop(dev);
    db.write_ops(vec![crate::db::WriteOp::Delete { table: "places".into(), key: Value::Int(0) }]).unwrap();
    let m2 = build(&db, &opts(), &out).unwrap().manifest;
    assert!(m2.version > m.version && m2.built_ms >= m.built_ms);
    // Twice: a device made again of the same version starts empty too.
    for _ in 0..2 {
        let dev = Device::create(&dev_dir, m2.clone()).unwrap();
        assert_eq!(dev.held().count(), 0);
        let mut names: Vec<String> = std::fs::read_dir(&dev_dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names, vec!["keep-me".to_string(), format!("places-{}", m2.version)]);
    }
}

// ---------------------------------------------------------------- vectors

const DIM: usize = 16;

fn vector_schema() -> Schema {
    Schema::new(
        "tracks",
        vec![Field::new("id", FieldType::Int), Field::new("genre", FieldType::Keyword), Field::new("emb", FieldType::Vector { dim: DIM, index: crate::schema::VectorIndexKind::Flat })],
        Some("id"),
    )
    .unwrap()
}

/// `n` unit vectors around 24 topics, some tight and some looser, as track
/// embeddings are.
fn tracks(n: usize, seed: u64) -> Vec<Document> {
    let mut rng = StdRng::seed_from_u64(seed);
    let unit = |v: Vec<f32>| {
        let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        v.into_iter().map(|x| x / n).collect::<Vec<f32>>()
    };
    let topics: Vec<Vec<f32>> = (0..24).map(|_| unit((0..DIM).map(|_| rng.gen_range(-1.0..1.0)).collect())).collect();
    (0..n)
        .map(|i| {
            let t = rng.gen_range(0..topics.len());
            let spread = if t % 3 == 0 { 0.3 } else { 0.12 };
            let v = unit(topics[t].iter().map(|x| x + rng.gen_range(-spread..spread)).collect());
            vec![Value::Int(i as i64), Value::Str(format!("g{t}")), Value::Vector(v.into())]
        })
        .collect()
}

fn vector_cloud(dir: &Path, docs: Vec<Document>) -> Db {
    let db = Db::create(&dir.join("tracks.strata"), vector_schema(), DbOptions::default()).unwrap();
    let mut docs = docs;
    let rest = docs.split_off(docs.len() / 2);
    db.load(docs).unwrap();
    db.load(rest).unwrap();
    db
}

fn vector_opts(clusters: usize) -> BuildOptions {
    BuildOptions { table: None, key: BuildKey::Vector { field: "emb".into(), clusters, seed: 7, codes: true } }
}

fn fetch_vector(dev: &mut Device, dir: &Path, taste: &[f32], rows: u64) {
    let p = dev.plan_vector(taste, rows).unwrap();
    for t in p.drop {
        dev.drop_tile(t).unwrap();
    }
    for (t, _) in p.fetch {
        dev.add_tile(t, &std::fs::read(dir.join(t.path())).unwrap()).unwrap();
    }
    // What it holds fits in half as many rows again (or is one cluster).
    let held: usize = dev.held().map(|(_, n)| n).sum();
    assert!(held as u64 <= rows * 3 / 2 || dev.held().count() == 1, "{held} rows held for a budget of {rows}");
}

fn near(rng: &mut StdRng, v: &[f32], spread: f32) -> Vec<f32> {
    v.iter().map(|x| x + rng.gen_range(-spread..spread)).collect()
}

#[test]
fn cluster_bounds_hold_every_row() {
    let dir = tempfile::tempdir().unwrap();
    let docs = tracks(3000, 11);
    let db = vector_cloud(dir.path(), docs.clone());
    let out = dir.path().join("slice");
    let built = build(&db, &vector_opts(40), &out).unwrap();
    let m = &built.manifest;
    assert_eq!((built.rows, m.kind(), m.tiles.len()), (3000, "vector", 40));
    assert_eq!(m.tiles.iter().map(|t| t.rows).sum::<u64>(), 3000);
    assert_eq!(Manifest::from_json(&m.to_json()).unwrap(), *m);
    let v = m.vectors().unwrap();
    // Every row of a cluster scores at most its bound, for any query.
    let mut rng = StdRng::seed_from_u64(12);
    let queries: Vec<Vec<f32>> = (0..20).map(|_| (0..DIM).map(|_| rng.gen_range(-1.0..1.0)).collect()).collect();
    let mut dev = Device::create(&dir.path().join("device"), m.clone()).unwrap();
    for t in &m.tiles {
        dev.add_tile(t.tile(), &std::fs::read(out.join(t.tile().path())).unwrap()).unwrap();
        let rows = dev.db().query("SELECT emb FROM tracks", &Params::new()).unwrap();
        for q in &queries {
            let qn = q.iter().map(|&x| x as f64 * x as f64).sum::<f64>().sqrt();
            let b = v.bound(t.x as usize, q, qn);
            for r in &rows.rows {
                let x = r[0].as_vector().unwrap();
                let s = crate::vector::kernels::dot_f32(x, q) as f64;
                assert!(s <= b, "cluster {}: a row scores {s} above its bound {b}", t.x);
            }
        }
        dev.drop_tile(t.tile()).unwrap();
    }
    // The radii are tight: some row sits at each.
    assert!(v.radii.iter().all(|&r| r > 0.0 && r < 2.0), "{:?}", v.radii);
}

fn codes_of(out: &Path) -> Vec<u8> {
    use std::io::Read;
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(&std::fs::read(out.join(CODES_FILE)).unwrap()[..]).read_to_end(&mut bytes).unwrap();
    bytes
}

#[test]
fn code_bounds_hold_every_row_and_are_tighter() {
    let dir = tempfile::tempdir().unwrap();
    let db = vector_cloud(dir.path(), tracks(3000, 21));
    let out = dir.path().join("slice");
    let m = build(&db, &vector_opts(40), &out).unwrap().manifest;
    let v = m.vectors().unwrap();
    let c = v.codes.as_ref().expect("a vector slice has codes by default");
    assert_eq!(std::fs::metadata(out.join(CODES_FILE)).unwrap().len(), c.bytes);
    let codes = codes_of(&out);
    assert_eq!(codes.len(), 3000 * DIM);
    let mut rng = StdRng::seed_from_u64(22);
    let queries: Vec<Vec<f32>> = (0..20).map(|_| (0..DIM).map(|_| rng.gen_range(-1.0..1.0)).collect()).collect();
    let mut dev = Device::create(&dir.path().join("device"), m.clone()).unwrap();
    let (mut tighter, mut start) = (0, 0);
    for t in &m.tiles {
        let end = start + t.rows as usize * DIM;
        dev.add_tile(t.tile(), &std::fs::read(out.join(t.tile().path())).unwrap()).unwrap();
        let rows = dev.db().query("SELECT emb FROM tracks", &Params::new()).unwrap();
        assert_eq!(rows.rows.len() as u64, t.rows);
        for q in &queries {
            let qn = q.iter().map(|&x| x as f64 * x as f64).sum::<f64>().sqrt();
            let by_codes = c.query(q).best(&codes[start..end]) + v.margin(t.x as usize, qn);
            for r in &rows.rows {
                let s = crate::vector::kernels::dot_f32(r[0].as_vector().unwrap(), q) as f64;
                assert!(s <= by_codes, "cluster {}: a row scores {s} above its codes' bound {by_codes}", t.x);
            }
            tighter += (by_codes < v.bound(t.x as usize, q, qn)) as usize;
        }
        dev.drop_tile(t.tile()).unwrap();
        start = end;
    }
    assert!(tighter * 10 >= queries.len() * m.tiles.len() * 9, "the codes' bound was tighter {tighter} times of {}", queries.len() * m.tiles.len());
    // Without codes: no file, and a device refuses to add any.
    let plain = dir.path().join("plain");
    let mut o = vector_opts(40);
    if let BuildKey::Vector { codes, .. } = &mut o.key {
        *codes = false;
    }
    let m2 = build(&db, &o, &plain).unwrap().manifest;
    assert!(m2.vectors().unwrap().codes.is_none() && !plain.join(CODES_FILE).exists());
    let mut d2 = Device::create(&dir.path().join("d2"), m2).unwrap();
    assert!(d2.add_codes(&std::fs::read(out.join(CODES_FILE)).unwrap()).unwrap_err().to_string().contains("without codes"));
    // Codes of another slice (other row counts) are refused.
    let m3 = build(&db, &vector_opts(30), &dir.path().join("s3")).unwrap().manifest;
    let mut d3 = Device::create(&dir.path().join("d3"), m3).unwrap();
    let mut short = codes.clone();
    short.truncate(codes.len() - DIM);
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut gz, &short).unwrap();
    assert!(d3.add_codes(&gz.finish().unwrap()).unwrap_err().to_string().contains("bytes for"));
}

#[test]
fn with_codes_a_device_proves_more_and_names_what_would_prove_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let docs = tracks(5000, 23);
    let db = vector_cloud(dir.path(), docs.clone());
    let out = dir.path().join("slice");
    let m = build(&db, &vector_opts(60), &out).unwrap().manifest;
    let mut plain = Device::create(&dir.path().join("a"), m.clone()).unwrap();
    let mut coded = Device::create(&dir.path().join("b"), m.clone()).unwrap();
    coded.add_codes(&std::fs::read(out.join(CODES_FILE)).unwrap()).unwrap();
    assert!(coded.has_codes() && !plain.has_codes());
    let mut rng = StdRng::seed_from_u64(24);
    let (mut plain_proved, mut coded_proved, mut refined) = (0, 0, 0);
    for step in 0..30 {
        let taste = docs[rng.gen_range(0..docs.len())][2].as_vector().unwrap().to_vec();
        fetch_vector(&mut plain, &out, &taste, 400);
        fetch_vector(&mut coded, &out, &taste, 400);
        for i in 0..4 {
            let k = [1, 5, 10, 30][rng.gen_range(0..4)];
            // Near the taste, or (one in four) near another track, which
            // the clusters held may not cover.
            let to = if i == 3 { docs[rng.gen_range(0..docs.len())][2].as_vector().unwrap().to_vec() } else { taste.clone() };
            let s = VectorSearch::new(near(&mut rng, &to, 0.1), k).unwrap();
            let (a, b) = (plain.search_vector(&s).unwrap(), coded.search_vector(&s).unwrap());
            assert_eq!(a.rows.scores, b.rows.scores, "the same clusters held");
            assert!(b.bound <= a.bound, "codes only tighten the bound");
            plain_proved += a.proved as usize;
            coded_proved += b.proved as usize;
            let (keys, scores) = cloud_answer(&db, &m, &s);
            if b.proved {
                assert!(b.need.is_empty());
                assert_eq!(b.rows.scores, scores, "step {step}");
                assert_eq!(above_kth(&b.rows.keys, &b.rows.scores), above_kth(&keys, &scores), "step {step}");
            } else if b.why == Why::Bound {
                // Fetching the clusters it named proves the answer.
                assert!(!b.need.is_empty());
                for (t, bytes) in &b.need {
                    assert_eq!(*bytes, std::fs::metadata(out.join(t.path())).unwrap().len());
                    coded.add_tile(*t, &std::fs::read(out.join(t.path())).unwrap()).unwrap();
                }
                let c = coded.search_vector(&s).unwrap();
                assert!(c.proved, "step {step}: holding what it named, the device proves it");
                assert_eq!(c.rows.scores, scores, "step {step}");
                refined += 1;
                for (t, _) in &b.need {
                    coded.drop_tile(*t).unwrap();
                }
            }
        }
    }
    assert!(coded_proved > plain_proved && refined > 0, "proved {plain_proved} by centres, {coded_proved} with codes; {refined} refined");
}

#[test]
fn proved_vector_answers_are_the_clouds() {
    let dir = tempfile::tempdir().unwrap();
    let docs = tracks(5000, 13);
    let db = vector_cloud(dir.path(), docs.clone());
    let out = dir.path().join("slice");
    let m = build(&db, &vector_opts(60), &out).unwrap().manifest;
    let mut dev = Device::create(&dir.path().join("device"), m.clone()).unwrap();
    let mut rng = StdRng::seed_from_u64(14);
    let (mut proved, mut asked) = (0, 0);
    for step in 0..60 {
        // A user's taste: a track's vector; their searches near it.
        let taste = docs[rng.gen_range(0..docs.len())][2].as_vector().unwrap().to_vec();
        fetch_vector(&mut dev, &out, &taste, [250, 500, 1000][step % 3]);
        for _ in 0..5 {
            let q = near(&mut rng, &taste, 0.05);
            let k = [1, 5, 10, 30][rng.gen_range(0..4)];
            let s = VectorSearch::new(q, k).unwrap();
            let a = dev.search_vector(&s).unwrap();
            let (keys, scores) = cloud_answer(&db, &m, &s);
            if a.proved {
                proved += 1;
                assert_eq!(a.why, Why::Proved);
                assert_eq!(a.rows.scores, scores, "step {step}: a proved answer's scores are the cloud's");
                assert_eq!(above_kth(&a.rows.keys, &a.rows.scores), above_kth(&keys, &scores), "step {step}");
            } else {
                asked += 1;
                assert!(matches!(a.why, Why::Bound | Why::Short), "{:?}", a.why);
                // What the device lacks could beat its k-th: the bound is above it.
                assert!(a.rows.scores.len() < k || a.rows.scores[k - 1] < a.bound);
            }
        }
    }
    assert!(proved >= 60 && asked >= 10, "proved {proved}, asked {asked}");
}

#[test]
fn a_device_holding_every_cluster_proves_every_vector_search() {
    let dir = tempfile::tempdir().unwrap();
    let db = vector_cloud(dir.path(), tracks(800, 15));
    let out = dir.path().join("slice");
    let m = build(&db, &vector_opts(0), &out).unwrap().manifest;
    assert_eq!(m.tiles.len(), 28, "about the square root of 800 rows");
    let mut dev = Device::create(&dir.path().join("device"), m.clone()).unwrap();
    fetch_vector(&mut dev, &out, &[0.1; DIM], 1000);
    assert_eq!(dev.held().count(), m.tiles.len());
    let s = VectorSearch::new(vec![0.3; DIM], 10).unwrap();
    let a = dev.search_vector(&s).unwrap();
    assert!(a.proved && a.on_device && a.bound == f64::NEG_INFINITY);
    assert_eq!(a.rows.scores, cloud_answer(&db, &m, &s).1);
    // The JSON an app reads: no bound, and the request that asks the cloud.
    let j = a.to_json(&s, &m);
    assert_eq!((j["bound"].is_null(), j["why"].as_str()), (true, Some("proved")));
    let ask: Json = serde_json::from_str(j["ask"].as_str().unwrap()).unwrap();
    assert!(ask["sql"].as_str().unwrap().contains("strategy => 'flat', rerank => 800"), "{ask}");
    assert_eq!(ask["params"]["v"].as_array().unwrap().len(), DIM);
}

#[test]
fn a_vector_slice_and_a_geo_slice_refuse_each_others_searches() {
    let dir = tempfile::tempdir().unwrap();
    let vdb = vector_cloud(dir.path(), tracks(300, 16));
    let vm = build(&vdb, &vector_opts(5), &dir.path().join("v")).unwrap().manifest;
    let gdb = cloud(dir.path(), places(300, 17));
    let gm = build(&gdb, &opts(), &dir.path().join("g")).unwrap().manifest;
    let vdev = Device::create(&dir.path().join("vd"), vm).unwrap();
    let gdev = Device::create(&dir.path().join("gd"), gm).unwrap();
    let err = vdev.search(&Search::new("pizza", 40.7, -74.0, 5, 1000.0).unwrap()).unwrap_err().to_string();
    assert!(err.contains("search it with a vector"), "{err}");
    let err = gdev.search_vector(&VectorSearch::new(vec![0.1; DIM], 5).unwrap()).unwrap_err().to_string();
    assert!(err.contains("search it with text"), "{err}");
    assert!(vdev.plan(40.7, -74.0, 2).fetch.is_empty());
    assert!(vdev.plan_vector(&[0.1; 3], 2).unwrap_err().to_string().contains("3 numbers"));
    // A vector slice needs a vector in every row, and a field of vectors.
    let err = build(&gdb, &BuildOptions { table: None, key: BuildKey::Vector { field: "body".into(), clusters: 4, seed: 1, codes: true } }, &dir.path().join("x")).unwrap_err().to_string();
    assert!(err.contains("not a VECTOR field"), "{err}");
}

#[test]
fn vector_search_or_ask_asks_only_when_it_must() {
    let dir = tempfile::tempdir().unwrap();
    let docs = tracks(2000, 18);
    let db = vector_cloud(dir.path(), docs.clone());
    let out = dir.path().join("slice");
    let m = build(&db, &vector_opts(30), &out).unwrap().manifest;
    let mut dev = Device::create(&dir.path().join("device"), m.clone()).unwrap();
    let taste = docs[0][2].as_vector().unwrap().to_vec();
    fetch_vector(&mut dev, &out, &taste, 270);
    let mut asked = 0;
    for k in [1, 1000] {
        let s = VectorSearch::new(taste.clone(), k).unwrap();
        let o = dev
            .search_or_ask(&s, |sql, p| {
                asked += 1;
                db.query(sql, p)
            })
            .unwrap();
        assert_eq!(o.rows.scores, cloud_answer(&db, &m, &s).1, "k {k}");
        assert_eq!(o.answer.on_device, k == 1);
    }
    assert_eq!(asked, 1, "only k = 1000 went to the cloud");
}
