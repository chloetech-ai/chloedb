//! A vector slice's device folder stays bounded as the user's taste moves
//! (NADIA-402, NADIA-304): tiles dropped leave room the file gives back.

use chloedb::db::Db;
use chloedb::schema::{Field, FieldType, Schema, VectorIndexKind};
use chloedb::slice::{build, BuildKey, BuildOptions, Device};
use chloedb::storage::DbOptions;
use chloedb::value::Value;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

const DIM: usize = 16;

fn unit(v: Vec<f32>) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    v.into_iter().map(|x| x / n).collect()
}

fn folder_bytes(p: &std::path::Path) -> u64 {
    std::fs::read_dir(p)
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            if e.file_type().unwrap().is_dir() {
                folder_bytes(&e.path())
            } else {
                e.metadata().unwrap().len()
            }
        })
        .sum()
}

#[test]
fn the_device_folder_does_not_grow_with_every_taste() {
    let dir = tempfile::tempdir().unwrap();
    let schema = Schema::new("tracks", vec![Field::new("id", FieldType::Int), Field::new("emb", FieldType::Vector { dim: DIM, index: VectorIndexKind::Flat })], Some("id")).unwrap();
    let db = Db::create(&dir.path().join("tracks.strata"), schema, DbOptions::default()).unwrap();
    let mut rng = StdRng::seed_from_u64(3);
    let topics: Vec<Vec<f32>> = (0..24).map(|_| unit((0..DIM).map(|_| rng.gen_range(-1.0..1.0)).collect())).collect();
    let docs: Vec<Vec<Value>> = (0..6000)
        .map(|i| {
            let t = &topics[rng.gen_range(0..topics.len())];
            vec![Value::Int(i), Value::Vector(unit(t.iter().map(|x| x + rng.gen_range(-0.2..0.2)).collect()).into())]
        })
        .collect();
    db.load(docs).unwrap();
    let out = dir.path().join("slice");
    let built = build(&db, &BuildOptions { table: None, key: BuildKey::Vector { field: "emb".into(), clusters: 60, seed: 7, codes: true } }, &out).unwrap();
    let m = built.manifest;
    let all_tiles: u64 = m.tiles.iter().map(|t| t.bytes).sum();
    let mut dev = Device::create(&dir.path().join("device"), m).unwrap();
    let rows = 1000u64;
    let mut sizes = Vec::new();
    for step in 0..60 {
        let taste: Vec<f32> = unit((0..DIM).map(|_| rng.gen_range(-1.0..1.0)).collect());
        let p = dev.plan_vector(&taste, rows).unwrap();
        for t in p.drop {
            dev.drop_tile(t).unwrap();
        }
        for (t, _) in p.fetch {
            dev.add_tile(t, &std::fs::read(out.join(t.path())).unwrap()).unwrap();
        }
        if step % 10 == 9 {
            sizes.push(folder_bytes(&dir.path().join("device")));
        }
    }
    let held: usize = dev.held().map(|(_, n)| n).sum();
    eprintln!("held {held} rows; folder after each 10 tastes {sizes:?}; the slice's tiles {all_tiles}");
    let last = *sizes.last().unwrap();
    // Several times what it holds at most, never what it fetched in all.
    assert!(last < all_tiles * 2, "the folder is {last} bytes after 60 tastes; the slice's tiles are {all_tiles}");
    assert!(sizes[5] <= sizes[1] * 3 / 2 + (1 << 20), "{sizes:?}");
}
