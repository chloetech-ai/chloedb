//! A database made with no tables ([`Db::create_empty`]): its first table
//! is the placeholder, the tables made after it are the database's, and
//! exports leave the placeholder out.

use chloedb::db::Db;
use chloedb::query::Params;
use chloedb::schema::PLACEHOLDER_TABLE;
use chloedb::storage::DbOptions;
use chloedb::Schema;

#[test]
fn a_database_with_no_tables_takes_its_first_one_later() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("e.strata");
    let db = Db::create_empty(&path, DbOptions::default()).unwrap();
    assert!(db.schema().is_placeholder());
    assert_eq!(db.catalog().len(), 1);
    assert!(db.default_table().is_none());

    db.sql("CREATE TABLE notes (id KEYWORD PRIMARY KEY, body TEXT)", &Params::new()).unwrap();
    assert_eq!(db.default_table().unwrap().table, "notes");
    db.sql("INSERT INTO notes (id, body) VALUES ('a', 'first note')", &Params::new()).unwrap();
    assert!(db.create_table(Schema::placeholder()).is_err(), "the placeholder is made once");
    drop(db);

    // Opened again, it is the same database: the placeholder first.
    let db = Db::open(&path, DbOptions::default()).unwrap();
    let names: Vec<String> = db.catalog().iter().map(|t| t.table.clone()).collect();
    assert_eq!(names, [PLACEHOLDER_TABLE, "notes"]);
    assert_eq!(db.default_table().unwrap().table, "notes");

    // An export of every table holds notes alone.
    #[cfg(feature = "sqlite")]
    {
        let e = chloedb::sqlite::export(&db, &dir.path().join("e.sqlite"), &chloedb::sqlite::ExportOptions::default()).unwrap();
        let tables: Vec<&str> = e.tables.iter().map(|t| t.table.as_str()).collect();
        assert_eq!(tables, ["notes"]);
    }
}
