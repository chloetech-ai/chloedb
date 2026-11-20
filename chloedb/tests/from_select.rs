//! `INSERT ... SELECT` and `CREATE TABLE ... AS SELECT` (SAM-8, SAM-10):
//! the columns' types a made table takes, NULLs kept, a key, the INSERT's
//! columns and conflicts, what is refused and in what words; and the join
//! of a TEXT column, whose refusal says why and what to do (SAM-402).

use chloedb::db::{Db, SqlResult};
use chloedb::query::Params;
use chloedb::storage::DbOptions;
use chloedb::value::Value;

fn run(db: &Db, sql: &str) -> SqlResult {
    db.sql(sql, &Params::new()).unwrap_or_else(|e| panic!("{sql}: {e}"))
}

fn written(db: &Db, sql: &str) -> u64 {
    match run(db, sql) {
        SqlResult::Written(n) => n,
        SqlResult::Rows(_) => panic!("{sql}: expected a write"),
    }
}

fn rows(db: &Db, sql: &str) -> Vec<Vec<Value>> {
    match run(db, sql) {
        SqlResult::Rows(r) => r.rows,
        SqlResult::Written(_) => panic!("{sql}: expected rows"),
    }
}

fn fails(db: &Db, sql: &str) -> String {
    match db.sql(sql, &Params::new()) {
        Ok(_) => panic!("{sql}: expected an error"),
        Err(e) => e.to_string(),
    }
}

fn s(v: &str) -> Value {
    Value::Str(v.into())
}

/// The type of each column of `table`, as CREATE TABLE writes it.
fn types(db: &Db, table: &str) -> Vec<(String, String)> {
    let schema = db.catalog().into_iter().find(|t| t.table == table).unwrap_or_else(|| panic!("no table {table}"));
    schema.fields.iter().map(|f| (f.name.clone(), f.type_name().to_string())).collect()
}

/// Made-up support tickets: a key, a customer code, an amount (NULL for
/// one), a count, a flag, a time and the ticket's text.
fn tickets(db: &Db) {
    run(db, "CREATE TABLE tickets (id INT PRIMARY KEY, customer KEYWORD, amount FLOAT, items INT, urgent BOOL, opened TIMESTAMP, notes TEXT OPTIONS(analyzer = 'english'))");
    run(
        db,
        "INSERT INTO tickets (id, customer, amount, items, urgent, opened, notes) VALUES \
         (1, 'c1', 12.5, 1, true, '2026-01-03T10:00:00Z', 'asked for a refund of the second order'), \
         (2, 'c2', NULL, NULL, false, '2026-01-04T11:00:00Z', 'the parcel came late'), \
         (3, 'c1', 40.0, 3, false, '2026-02-01T09:30:00Z', 'a chargeback from the bank'), \
         (4, 'c3', 7.25, 2, NULL, NULL, NULL)",
    );
}

#[test]
fn create_table_as_takes_the_types_of_the_columns_it_reads_and_keeps_nulls() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::create_empty(&dir.path().join("db"), DbOptions { sync: false, ..DbOptions::default() }).unwrap();
    tickets(&db);
    assert_eq!(written(&db, "CREATE TABLE copy AS SELECT * FROM tickets"), 4);
    assert_eq!(
        types(&db, "copy"),
        [("id", "INT"), ("customer", "KEYWORD"), ("amount", "FLOAT"), ("items", "INT"), ("urgent", "BOOL"), ("opened", "TIMESTAMP"), ("notes", "TEXT")].map(|(a, b)| (a.to_string(), b.to_string()))
    );
    // No key unless it says one, as in PostgreSQL.
    assert_eq!(db.catalog().into_iter().find(|t| t.table == "copy").unwrap().primary_key, None);
    // The same rows, NULLs as NULLs.
    assert_eq!(rows(&db, "SELECT id, amount, items, urgent, opened, notes FROM copy ORDER BY id"), rows(&db, "SELECT id, amount, items, urgent, opened, notes FROM tickets ORDER BY id"));
    assert_eq!(rows(&db, "SELECT COUNT(*) FROM copy WHERE amount IS NULL"), vec![vec![Value::Int(1)]]);
    // A TEXT column keeps its analyzer: it is searched as the source is.
    assert_eq!(rows(&db, "SELECT id FROM copy WHERE MATCH(notes, 'refunds') ORDER BY id"), vec![vec![Value::Int(1)]]);
    // A KEYWORD column joins and groups.
    assert_eq!(rows(&db, "SELECT c.customer, COUNT(*) AS n FROM copy c JOIN tickets t ON c.id = t.id GROUP BY c.customer ORDER BY c.customer"), vec![vec![s("c1"), Value::Int(2)], vec![s("c2"), Value::Int(1)], vec![s("c3"), Value::Int(1)]]);
}

#[test]
fn create_table_as_types_expressions_by_their_values_casts_and_operands() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::create_empty(&dir.path().join("db"), DbOptions { sync: false, ..DbOptions::default() }).unwrap();
    tickets(&db);
    run(
        &db,
        "CREATE TABLE by_customer (PRIMARY KEY (customer)) AS \
         SELECT customer, COUNT(*) AS n, SUM(amount) AS total, AVG(items) AS mean_items, MAX(opened) AS last_opened, \
                upper(customer) AS code, CAST(customer AS TEXT) AS words, CAST(NULL AS INT) AS nothing, NULL AS unknown \
         FROM tickets GROUP BY customer",
    );
    assert_eq!(
        types(&db, "by_customer"),
        [("customer", "KEYWORD"), ("n", "INT"), ("total", "FLOAT"), ("mean_items", "FLOAT"), ("last_opened", "TIMESTAMP"), ("code", "KEYWORD"), ("words", "TEXT"), ("nothing", "INT"), ("unknown", "KEYWORD")]
            .map(|(a, b)| (a.to_string(), b.to_string()))
    );
    let key = db.catalog().into_iter().find(|t| t.table == "by_customer").unwrap();
    assert_eq!(key.primary_key.map(|k| key.fields[k].name.clone()).as_deref(), Some("customer"));
    assert_eq!(rows(&db, "SELECT customer, n, total, nothing FROM by_customer ORDER BY customer"), vec![
        vec![s("c1"), Value::Int(2), Value::Float(52.5), Value::Null],
        vec![s("c2"), Value::Int(1), Value::Null, Value::Null],
        vec![s("c3"), Value::Int(1), Value::Float(7.25), Value::Null],
    ]);
    // With no rows, the types come from the expressions.
    run(&db, "CREATE TABLE none AS SELECT id, amount * 2 AS doubled, items + 1 AS more, amount > 10 AS big, customer || '!' AS shout FROM tickets WHERE id > 100");
    assert_eq!(types(&db, "none"), [("id", "INT"), ("doubled", "FLOAT"), ("more", "INT"), ("big", "BOOL"), ("shout", "KEYWORD")].map(|(a, b)| (a.to_string(), b.to_string())));
    assert_eq!(rows(&db, "SELECT COUNT(*) FROM none"), vec![vec![Value::Int(0)]]);
}

#[test]
fn create_table_as_takes_the_columns_it_defines_and_drops_the_table_when_its_rows_fail() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::create_empty(&dir.path().join("db"), DbOptions { sync: false, ..DbOptions::default() }).unwrap();
    tickets(&db);
    // Defined columns take their definitions; a defined column the SELECT
    // does not give is added, NULL.
    run(&db, "CREATE TABLE t2 (id INT PRIMARY KEY, customer TEXT, note KEYWORD) AS SELECT id, customer, amount FROM tickets");
    assert_eq!(types(&db, "t2"), [("id", "INT"), ("customer", "TEXT"), ("amount", "FLOAT"), ("note", "KEYWORD")].map(|(a, b)| (a.to_string(), b.to_string())));
    assert_eq!(rows(&db, "SELECT COUNT(*) FROM t2 WHERE note IS NULL"), vec![vec![Value::Int(4)]]);
    // A key two rows share: refused, and no table is left.
    let e = fails(&db, "CREATE TABLE dup (PRIMARY KEY (customer)) AS SELECT customer, id FROM tickets");
    assert!(e.contains("c1"), "{e}");
    assert!(!db.catalog().iter().any(|t| t.table == "dup"));
    // A table that is there: refused before its SELECT runs, or nothing with IF NOT EXISTS.
    assert!(fails(&db, "CREATE TABLE t2 AS SELECT id FROM tickets").contains("t2 already exists"));
    assert_eq!(written(&db, "CREATE TABLE IF NOT EXISTS t2 AS SELECT id FROM nosuch"), 0);
    // Names: a qualifier is dropped; an expression needs one; two alike are refused.
    run(&db, "CREATE TABLE j AS SELECT a.id, b.notes FROM tickets a JOIN tickets b ON a.id = b.id");
    assert_eq!(types(&db, "j"), [("id", "INT"), ("notes", "TEXT")].map(|(a, b)| (a.to_string(), b.to_string())));
    let e = fails(&db, "CREATE TABLE x AS SELECT COUNT(*) FROM tickets");
    assert!(e.contains("needs a name of its own") && e.contains("AS name"), "{e}");
    let e = fails(&db, "CREATE TABLE x AS SELECT a.id, b.id FROM tickets a JOIN tickets b ON a.id = b.id");
    assert!(e.contains("two columns named id"), "{e}");
    // A WITH, and a UNION, which names its columns by its first SELECT.
    run(&db, "CREATE TABLE w AS WITH big AS (SELECT id, amount FROM tickets WHERE amount > 10) SELECT id, amount FROM big");
    assert_eq!(rows(&db, "SELECT id FROM w ORDER BY id"), vec![vec![Value::Int(1)], vec![Value::Int(3)]]);
    run(&db, "CREATE TABLE u AS SELECT id, customer FROM tickets WHERE id = 1 UNION ALL SELECT id, customer FROM tickets WHERE id = 4");
    assert_eq!(types(&db, "u"), [("id", "INT"), ("customer", "KEYWORD")].map(|(a, b)| (a.to_string(), b.to_string())));
    assert_eq!(rows(&db, "SELECT COUNT(*) FROM u"), vec![vec![Value::Int(2)]]);
}

#[test]
fn insert_select_writes_every_row_as_values_would() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::create_empty(&dir.path().join("db"), DbOptions { sync: false, ..DbOptions::default() }).unwrap();
    tickets(&db);
    run(&db, "CREATE TABLE facts (id INT PRIMARY KEY, customer KEYWORD, amount FLOAT, items INT, source KEYWORD DEFAULT 'tickets')");
    // Named columns; the others take their defaults; NULL stays NULL.
    assert_eq!(written(&db, "INSERT INTO facts (id, customer, amount, items) SELECT id, customer, amount, items FROM tickets WHERE id <= 2"), 2);
    assert_eq!(rows(&db, "SELECT id, amount, items, source FROM facts ORDER BY id"), vec![
        vec![Value::Int(1), Value::Float(12.5), Value::Int(1), s("tickets")],
        vec![Value::Int(2), Value::Null, Value::Null, s("tickets")],
    ]);
    // A key that is taken fails the whole statement: nothing is written.
    let e = fails(&db, "INSERT INTO facts (id, customer) SELECT id, customer FROM tickets");
    assert!(e.to_lowercase().contains("key"), "{e}");
    assert_eq!(rows(&db, "SELECT COUNT(*) FROM facts"), vec![vec![Value::Int(2)]]);
    // ON CONFLICT DO NOTHING keeps the rows there; DO UPDATE changes them.
    run(&db, "INSERT INTO facts (id, customer, amount) SELECT id, customer, amount FROM tickets ON CONFLICT (id) DO NOTHING");
    assert_eq!(rows(&db, "SELECT COUNT(*) FROM facts"), vec![vec![Value::Int(4)]]);
    run(&db, "INSERT INTO facts (id, customer, amount) SELECT id, customer, amount * 2 FROM tickets WHERE id = 1 ON CONFLICT (id) DO UPDATE SET amount = excluded.amount");
    assert_eq!(rows(&db, "SELECT amount FROM facts WHERE id = 1"), vec![vec![Value::Float(25.0)]]);
    // Without a column list, the table's columns in order; with parameters.
    run(&db, "CREATE TABLE ids (id INT PRIMARY KEY, customer KEYWORD)");
    let n = match db.sql("INSERT INTO ids SELECT id, customer FROM tickets WHERE customer = :c", &Params::new().set("c", s("c1"))).unwrap() {
        SqlResult::Written(n) => n,
        SqlResult::Rows(_) => panic!("a write"),
    };
    assert_eq!(n, 2);
    // A number goes into a KEYWORD as its text; a whole FLOAT into an INT.
    run(&db, "INSERT INTO ids (id, customer) SELECT id + 10, items FROM tickets WHERE id = 3");
    assert_eq!(rows(&db, "SELECT customer FROM ids WHERE id = 13"), vec![vec![s("3")]]);
    // Rows that do not fit: refused with the row and the column.
    let e = fails(&db, "INSERT INTO ids (id, customer) SELECT amount, customer FROM tickets WHERE id = 4");
    assert!(e.contains("row 1 of the SELECT") && e.contains("id"), "{e}");
    let e = fails(&db, "INSERT INTO ids (id) SELECT id, customer FROM tickets");
    assert!(e.contains("2 columns in its SELECT for 1 named columns"), "{e}");
    assert!(fails(&db, "INSERT INTO nosuch SELECT id FROM tickets").contains("no such table: nosuch"));
    // Many rows, in one statement.
    run(&db, "CREATE TABLE many (id INT PRIMARY KEY, v INT)");
    let values: Vec<String> = (0..3000).map(|i| format!("({i}, {})", i % 7)).collect();
    run(&db, &format!("INSERT INTO many (id, v) VALUES {}", values.join(", ")));
    run(&db, "CREATE TABLE many2 (id INT PRIMARY KEY, v INT)");
    assert_eq!(written(&db, "INSERT INTO many2 SELECT id, v FROM many"), 3000);
    assert_eq!(rows(&db, "SELECT SUM(v) FROM many2"), rows(&db, "SELECT SUM(v) FROM many"));
}

#[test]
fn a_prepared_insert_select_says_where_it_runs() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::create_empty(&dir.path().join("db"), DbOptions { sync: false, ..DbOptions::default() }).unwrap();
    tickets(&db);
    let e = db.prepare_write("INSERT INTO tickets (id) SELECT id + 10 FROM tickets", &Params::new()).err().expect("refused").to_string();
    assert!(e.contains("INSERT ... SELECT runs as a statement of its own"), "{e}");
}

#[test]
fn a_join_on_a_text_column_says_why_and_what_to_do() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::create_empty(&dir.path().join("db"), DbOptions { sync: false, ..DbOptions::default() }).unwrap();
    // As a Postgres follower makes a table whose borough is varchar.
    run(&db, "CREATE TABLE zones (id INT PRIMARY KEY, borough TEXT, zone TEXT)");
    run(&db, "INSERT INTO zones (id, borough, zone) VALUES (1, 'Queens', 'Astoria'), (2, 'Queens', 'Jamaica'), (3, 'Bronx', 'Fordham')");
    let e = fails(&db, "SELECT a.borough, COUNT(*) AS n FROM zones a JOIN zones b ON a.borough = b.borough GROUP BY a.borough");
    assert!(e.contains("borough, a TEXT field of zones") && e.contains("KEYWORD") && e.contains("postgres.md"), "{e}");
    // The copy the message suggests joins.
    run(&db, "CREATE TABLE zones_k AS SELECT *, CAST(borough AS KEYWORD) AS borough_k FROM zones");
    assert_eq!(
        rows(&db, "SELECT a.borough_k, COUNT(*) AS n FROM zones_k a JOIN zones_k b ON a.borough_k = b.borough_k GROUP BY a.borough_k ORDER BY a.borough_k"),
        vec![vec![s("Bronx"), Value::Int(1)], vec![s("Queens"), Value::Int(4)]]
    );
}

#[test]
fn a_select_cut_at_its_default_limit_is_not_written() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::create_empty(&dir.path().join("db"), DbOptions { sync: false, ..DbOptions::default() }).unwrap();
    run(&db, "CREATE TABLE notes (id INT PRIMARY KEY, body TEXT)");
    let values: Vec<String> = (0..150).map(|i| format!("({i}, 'refund number {i}')")).collect();
    run(&db, &format!("INSERT INTO notes (id, body) VALUES {}", values.join(", ")));
    // Ranked without a LIMIT, the SELECT returns 100 of the 150.
    let e = fails(&db, "CREATE TABLE best AS SELECT id FROM notes ORDER BY bm25(body, 'refund') DESC");
    assert!(e.contains("without a LIMIT") && e.contains("100"), "{e}");
    assert!(!db.catalog().iter().any(|t| t.table == "best"));
    // With the match in WHERE, every row.
    assert_eq!(written(&db, "CREATE TABLE all_refunds AS SELECT id FROM notes WHERE MATCH(body, 'refund')"), 150);
}
