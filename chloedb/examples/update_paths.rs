//! One-row writes by key on Part A's products table (A1's schema, made-up
//! rows), each way ChloeDB takes them: SQL `UPDATE ... WHERE id = :id`
//! (a price and a stock set, and a counter + 1), the same through
//! [`Db::write_ops`] (`Set`, `Add`), single-row inserts and deletes. Each
//! is timed one a transaction on one thread, as
//! `chloedb-py/bench/laptop/a1_app.py` does, so the time a statement
//! spends planning shows beside its commit's.
//!
//! ```text
//! update_paths DIR [ROWS] [--no-sync] [--seconds S]
//! ```
//!
//! `ONLY=counter` runs only the timings whose names hold `counter`.
use std::time::{Duration, Instant};

use rand::{Rng, SeedableRng};
use serde_json::json;
use chloedb::db::{Db, WriteOp};
use chloedb::query::Params;
use chloedb::storage::{DbOptions, Layout};
use chloedb::{Schema, Value};

fn summary(name: &str, lat: &mut [f64]) {
    lat.sort_by(f64::total_cmp);
    let q = |p: f64| lat[((lat.len() - 1) as f64 * p).round() as usize];
    let mean = lat.iter().sum::<f64>() / lat.len() as f64;
    println!("{name:26} {:7} txns, {:8.0} a second (mean), p50 {:7.3} ms, p99 {:7.3}", lat.len(), 1e3 / mean, q(0.5), q(0.99));
}

fn timed(name: &str, seconds: f64, mut f: impl FnMut(usize)) {
    if std::env::var("ONLY").is_ok_and(|o| !name.contains(o.as_str())) {
        return;
    }
    for i in 0..200 {
        f(i);
    }
    let (mut lat, t0) = (Vec::new(), Instant::now());
    let mut i = 200;
    while t0.elapsed() < Duration::from_secs_f64(seconds) {
        let t = Instant::now();
        f(i);
        lat.push(t.elapsed().as_secs_f64() * 1e3);
        i += 1;
    }
    summary(name, &mut lat);
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let dir = std::path::PathBuf::from(&a[1]);
    let rows: usize = a.get(2).filter(|x| !x.starts_with("--")).map_or(137_260, |x| x.parse().unwrap());
    let sync = !a.iter().any(|x| x == "--no-sync");
    let seconds: f64 = a.iter().position(|x| x == "--seconds").map_or(3.0, |i| a[i + 1].parse().unwrap());
    let schema = Schema::from_json(
        &json!({"table": "products", "key": "id", "fields": [
            {"name": "id", "type": "keyword"},
            {"name": "title", "type": "text", "indexed": false},
            {"name": "brand", "type": "keyword"},
            {"name": "platform", "type": "keyword"},
            {"name": "category", "type": "keyword"},
            {"name": "price", "type": "float"},
            {"name": "rating", "type": "float"},
            {"name": "rating_count", "type": "int"},
            {"name": "stock", "type": "int"},
            {"name": "year", "type": "int"},
            {"name": "rated", "type": "keyword"},
            {"name": "description", "type": "text", "indexed": false},
            {"name": "attrs", "type": "json"}]})
        .to_string(),
    )
    .unwrap();
    let opts = DbOptions { layout: Layout::SingleFile, memtable_max_docs: 10_000, sync, ..DbOptions::default() };
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = Db::create(&dir.join("u.strata"), schema.clone(), opts).unwrap();
    let mut rng = rand::rngs::StdRng::seed_from_u64(1);
    let mut next = 0;
    while next < rows {
        let docs: Vec<_> = (next..rows.min(next + 10_000))
            .map(|i| {
                let j = json!({"id": format!("p{i}"), "title": format!("product {i}"), "brand": format!("b{}", i % 500), "platform": "web", "category": format!("c{}", i % 40),
                    "price": rng.gen_range(5.0..80.0), "rating": 4.1, "rating_count": 0, "stock": rng.gen_range(0..200), "year": 2020, "rated": "yes",
                    "description": "a description of some words", "attrs": {"color": "red"}});
                chloedb::ingest::document(&schema, &j).unwrap()
            })
            .collect();
        next += docs.len();
        db.load_into("products", docs).unwrap();
    }
    db.optimize().unwrap();
    println!("{rows} rows loaded; sync {sync}");
    let keys: Vec<String> = (0..100_000).map(|_| format!("p{}", rng.gen_range(0..rows))).collect();
    let key = |i: usize| keys[i % keys.len()].clone();
    let price_sql = "UPDATE products SET price = :p, stock = :s WHERE id = :id";
    let count_sql = "UPDATE products SET rating_count = rating_count + 1 WHERE id = :id";
    for round in 0..2 {
        println!("round {round}");
        timed("sql update price", seconds, |i| {
            let p = Params::new().set("p", Value::Float(5.0 + (i % 70) as f64)).set("s", Value::Int((i % 200) as i64)).set("id", Value::Str(key(i)));
            db.sql(price_sql, &p).unwrap();
        });
        timed("prepare_write only", seconds, |i| {
            let p = Params::new().set("p", Value::Float(5.0 + (i % 70) as f64)).set("s", Value::Int((i % 200) as i64)).set("id", Value::Str(key(i)));
            db.prepare_write(price_sql, &p).unwrap().unwrap();
        });
        let prepared: Vec<_> = (0..1000).map(|i| {
            let p = Params::new().set("p", Value::Float(5.0 + (i % 70) as f64)).set("s", Value::Int((i % 200) as i64)).set("id", Value::Str(key(i)));
            db.prepare_write(price_sql, &p).unwrap().unwrap()
        }).collect();
        timed("write_prepared only", seconds, |i| {
            db.write_prepared(&prepared[i % prepared.len()]).unwrap();
        });
        timed("sql counter + 1", seconds, |_| {
            db.sql(count_sql, &Params::new().set("id", Value::Str("p7".into()))).unwrap();
        });
        timed("ops set price", seconds, |i| {
            let k = Value::Str(key(i));
            db.write_ops(vec![
                WriteOp::Set { table: "products".into(), key: k.clone(), field: "price".into(), value: Value::Float(5.0 + (i % 70) as f64) },
                WriteOp::Set { table: "products".into(), key: k, field: "stock".into(), value: Value::Int((i % 200) as i64) },
            ])
            .unwrap();
        });
        timed("ops add counter", seconds, |_| {
            db.write_ops(vec![WriteOp::Add { table: "products".into(), key: Value::Str("p7".into()), field: "rating_count".into(), delta: Value::Int(1) }]).unwrap();
        });
        let mut n = rows + round * 10_000_000;
        timed("sql insert", seconds, |_| {
            n += 1;
            let p = Params::new().set("id", Value::Str(format!("p{n}"))).set("t", Value::Str("new".into())).set("p", Value::Float(9.5));
            db.sql("INSERT INTO products (id, title, price) VALUES (:id, :t, :p)", &p).unwrap();
        });
        let mut d = rows + round * 10_000_000;
        timed("sql delete by id", seconds, |_| {
            d += 1;
            db.sql("DELETE FROM products WHERE id = :id", &Params::new().set("id", Value::Str(format!("p{d}")))).unwrap();
        });
    }
    let got = db.query("SELECT rating_count FROM products WHERE id = 'p7'", &Params::new()).unwrap();
    println!("p7 rating_count {:?}", got.rows);
}
