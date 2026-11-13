//! `CREATE RECOMMENDER`: counts made of the events written before it, kept
//! up as events are written (in the transaction that writes them, after a
//! reopen too), and `recommend(...)` ranking items by them, by the
//! similarity of the user's recent items and by popularity, leaving out
//! what the user had.

use chloedb::db::{Db, SqlResult};
use chloedb::query::Params;
use chloedb::schema::Schema;
use chloedb::storage::{DbOptions, Layout};
use chloedb::value::Value;

fn opts() -> DbOptions {
    DbOptions { sync: false, layout: Layout::SingleFile, segment_max_docs: 4, ..DbOptions::default() }
}

fn sql(db: &Db, s: &str) -> Vec<Vec<Value>> {
    match db.sql(s, &Params::new()).unwrap_or_else(|e| panic!("{s}: {e}")) {
        SqlResult::Rows(r) => r.rows,
        SqlResult::Written(_) => Vec::new(),
    }
}

fn fails(db: &Db, s: &str) -> String {
    db.sql(s, &Params::new()).err().unwrap_or_else(|| panic!("{s} ran")).to_string()
}

fn col(rows: &[Vec<Value>]) -> Vec<String> {
    rows.iter().map(|r| r[0].as_str().unwrap_or_default().to_string()).collect()
}

fn event(db: &Db, id: usize, user: &str, item: &str, ts: i64) {
    sql(db, &format!("INSERT INTO events VALUES ('e{id}', '{user}', '{item}', {ts})"));
}

fn setup(path: &std::path::Path) -> Db {
    let seed = Schema::new("seed", vec![chloedb::schema::Field::new("id", chloedb::schema::FieldType::Keyword)], Some("id")).unwrap();
    let db = Db::create(path, seed, opts()).unwrap();
    sql(&db, "CREATE TABLE items (id KEYWORD PRIMARY KEY, title TEXT, emb VECTOR(3))");
    for (id, v) in [("a", "1.0,0.0,0.0"), ("b", "0.0,1.0,0.0"), ("c", "0.0,0.0,1.0"), ("d", "0.7,0.7,0.0"), ("e", "0.0,0.2,0.98"), ("f", "-1.0,0.0,0.0")] {
        sql(&db, &format!("INSERT INTO items VALUES ('{id}', 'item {id}', ARRAY[{v}])"));
    }
    sql(&db, "CREATE TABLE events (id KEYWORD PRIMARY KEY, usr KEYWORD, item KEYWORD, ts INT)");
    // Three users had a then b; two then c, one then d.
    let mut n = 0;
    for (u, items) in [("u1", ["a", "b", "c"]), ("u2", ["a", "b", "d"]), ("u3", ["a", "b", "c"])] {
        for (t, i) in items.iter().enumerate() {
            n += 1;
            event(&db, n, u, i, t as i64 * 10);
        }
    }
    db
}

#[test]
fn counts_events_written_before_and_after() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("r.strata");
    let db = setup(&path);
    let made = sql(&db, "CREATE RECOMMENDER shop ON events(usr, item, ts) ITEMS items(emb) OPTIONS (window = 2, history = 10)");
    assert_eq!(made[0][..4], [Value::Str("shop".into()), Value::Str("shop_counts".into()), Value::Int(9), Value::Int(3)]);
    assert!(fails(&db, "CREATE RECOMMENDER shop ON events(usr, item, ts)").contains("CREATE OR REPLACE RECOMMENDER"));
    assert!(fails(&db, "CREATE RECOMMENDER x ON events(usr, nope, ts)").contains("no column nope"));

    // The counts from before.
    let next = sql(&db, "SELECT dst, next FROM shop_counts WHERE kind = 'pair' AND src = 'b' AND next > 0 ORDER BY next DESC");
    assert_eq!(next, vec![vec![Value::Str("c".into()), Value::Float(2.0)], vec![Value::Str("d".into()), Value::Float(1.0)]]);

    // A new user's first event: what came after a, b first.
    event(&db, 10, "u4", "a", 5);
    let recs = sql(&db, "SELECT item, next, co FROM recommend('shop', 'u4', k => 3, similar => 0)");
    assert_eq!(col(&recs), ["b", "c", "d"]);
    assert_eq!(recs[0][1], Value::Int(1));
    // And after b: c (twice) before d (once), and neither a nor b, which u4 had.
    event(&db, 11, "u4", "b", 6);
    let ranked = sql(&db, "SELECT id FROM items ORDER BY recommend('shop', 'u4', similar => 0) DESC LIMIT 3");
    assert_eq!(col(&ranked)[..2], ["c", "d"]);
    assert!(!col(&ranked).iter().any(|i| i == "a" || i == "b"));
    // As a score among others, with a filter, and with the plan line.
    let p = Params(std::collections::HashMap::from([("u".to_string(), Value::Str("u4".into()))]));
    let rs = db.query("SELECT id FROM items WHERE id <> 'c' ORDER BY recommend('shop', :u) DESC LIMIT 2", &p).unwrap();
    assert_eq!(col(&rs.rows), ["d", "e"]);
    assert!(rs.stats.plan.iter().any(|l| l.starts_with("recommend shop for 'u4'")), "{:?}", rs.stats.plan);

    // Similar items: e is nearest c by its vector, and nobody had it.
    sql(&db, "INSERT INTO events VALUES ('e12', 'u5', 'c', 1)");
    let recs = sql(&db, "SELECT item, similar FROM recommend('shop', 'u5', next => 0, co => 0, popular => 0)");
    assert_eq!(col(&recs)[0], "e");

    // A row replacing an event of its key is not a new event.
    let n_before = sql(&db, "SELECT n FROM shop_counts WHERE kind = 'item' AND src = 'c'");
    sql(&db, "INSERT OR REPLACE INTO events VALUES ('e12', 'u5', 'c', 1)");
    assert_eq!(sql(&db, "SELECT n FROM shop_counts WHERE kind = 'item' AND src = 'c'"), n_before);
    // Several events in one statement, out of order: each user's in time order.
    sql(&db, "INSERT INTO events VALUES ('e13', 'u6', 'd', 30), ('e14', 'u6', 'a', 10), ('e15', 'u6', 'f', 20)");
    let next = |a: &str, b: &str| sql(&db, &format!("SELECT next FROM shop_counts WHERE kind = 'pair' AND src = '{a}' AND dst = '{b}'"));
    assert_eq!(next("a", "f"), vec![vec![Value::Float(1.0)]]);
    assert_eq!(next("f", "d"), vec![vec![Value::Float(1.0)]]);

    // A user with no events gets the most popular items.
    let recs = sql(&db, "SELECT item, popular FROM recommend('shop', 'nobody', k => 2)");
    assert_eq!(col(&recs), ["a", "b"]);

    // Errors that say what to do.
    assert!(fails(&db, "SELECT id FROM items ORDER BY recommend('nope', 'u1') DESC").contains("CREATE RECOMMENDER nope"));
    assert!(fails(&db, "SELECT id FROM items ORDER BY recommend('shop', 'u1', depth => 3) DESC").contains("unknown option depth"));

    // After a reopen, writes still feed the counts.
    drop(db);
    let db = Db::open(&path, opts()).unwrap();
    event(&db, 20, "u7", "e", 1);
    event(&db, 21, "u7", "f", 2);
    let got = sql(&db, "SELECT next FROM shop_counts WHERE kind = 'pair' AND src = 'e' AND dst = 'f'");
    assert_eq!(got, vec![vec![Value::Float(1.0)]]);

    // Dropped: the function and the counts go, events are written as before.
    sql(&db, "DROP RECOMMENDER shop");
    assert!(db.storage().table("shop_counts").is_none());
    event(&db, 22, "u7", "a", 3);
    assert!(fails(&db, "SELECT * FROM recommend('shop', 'u7')").contains("no recommender named shop"));
    sql(&db, "DROP RECOMMENDER IF EXISTS shop");
}

#[test]
fn integer_items_and_a_thousand_users() {
    let dir = tempfile::tempdir().unwrap();
    let seed = Schema::new("seed", vec![chloedb::schema::Field::new("id", chloedb::schema::FieldType::Keyword)], Some("id")).unwrap();
    let db = Db::create(&dir.path().join("i.strata"), seed, DbOptions { segment_max_docs: 5000, ..opts() }).unwrap();
    sql(&db, "CREATE TABLE ev (id INT PRIMARY KEY, u INT, item INT, at TIMESTAMP)");
    sql(&db, "CREATE TABLE products (id INT PRIMARY KEY, name KEYWORD)");
    let values: Vec<String> = (0..50).map(|i| format!("({i}, 'p{i}')")).collect();
    sql(&db, &format!("INSERT INTO products VALUES {}", values.join(", ")));
    sql(&db, "CREATE RECOMMENDER buys ON ev(u, item, at) ITEMS products");
    // No events yet: nothing to rank, and on its own the ranking answers the items it lists.
    assert!(sql(&db, "SELECT id FROM products ORDER BY recommend('buys', 1) DESC LIMIT 2").is_empty());
    assert!(sql(&db, "SELECT * FROM recommend('buys', 1)").is_empty());
    // Each user walks items i, i+1, i+2: what follows i is i+1.
    let mut id = 0;
    for chunk in (0..1000).collect::<Vec<_>>().chunks(100) {
        let mut rows = Vec::new();
        for u in chunk {
            for s in 0..3 {
                id += 1;
                rows.push(format!("({id}, {u}, {}, '{}')", (u % 40) + s, chloedb::timestamp::format(1_700_000_000_000_000i64 + s * 1_000_000)));
            }
        }
        sql(&db, &format!("INSERT INTO ev VALUES {}", rows.join(", ")));
    }
    sql(&db, &format!("INSERT INTO ev VALUES ({}, 5000, 7, '2026-11-13T00:00:00Z')", id + 1));
    let recs = sql(&db, "SELECT item FROM recommend('buys', 5000, k => 2)");
    assert_eq!(recs[0][0], Value::Int(8));
    let ranked = sql(&db, "SELECT id, name FROM products ORDER BY recommend('buys', 5000) DESC LIMIT 2");
    assert_eq!(ranked[0], vec![Value::Int(8), Value::Str("p8".into())]);
}
