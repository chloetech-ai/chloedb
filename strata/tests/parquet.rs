//! Parquet files into Strata and back (`strata::parquet`, feature `parquet`).

#![cfg(feature = "parquet")]

use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use datafusion::arrow::array::{
    ArrayRef, BinaryArray, BooleanArray, Date32Array, Decimal128Array, DictionaryArray, FixedSizeListArray, Float64Array, Int64Array, ListArray, ListBuilder, RecordBatch, StringArray, StringBuilder, StructArray,
    Time64MicrosecondArray, TimestampMillisecondArray,
};
use datafusion::arrow::datatypes::{DataType, Field as AField, Float32Type, Float64Type, Int32Type, TimeUnit};
use ::parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use ::parquet::arrow::ArrowWriter;
use ::parquet::basic::Compression;
use ::parquet::file::properties::WriterProperties;
use serde_json::json;
use strata::db::Db;
use strata::parquet::{self as pq, ExportOptions, ImportOptions};
use strata::query::sql::Params;
use strata::storage::DbOptions;
use strata::{Field, FieldType, Schema, Value};

fn write(path: &Path, batch: &RecordBatch) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let props = WriterProperties::builder().set_compression(Compression::SNAPPY).build();
    let mut w = ArrowWriter::try_new(File::create(path).unwrap(), batch.schema(), Some(props)).unwrap();
    w.write(batch).unwrap();
    w.close().unwrap();
}

fn ids(db: &Db, sql: &str) -> Vec<Value> {
    db.query(sql, &Params::new()).unwrap_or_else(|e| panic!("{sql}: {e}")).rows.into_iter().map(|r| r[0].clone()).collect()
}

fn sorted(db: &Db, sql: &str) -> Vec<Value> {
    let mut v = ids(db, sql);
    v.sort_by_key(|x| format!("{x:?}"));
    v
}

fn field<'a>(s: &'a Schema, name: &str) -> &'a Field {
    s.field(name).unwrap_or_else(|| panic!("{} has no field {name}", s.table))
}

/// Three rows of what Spark, pandas and polars write: every kind of
/// column, ids from `first`.
fn events(first: i64) -> RecordBatch {
    let n = 3;
    let id: ArrayRef = Arc::new(Int64Array::from_iter_values(first..first + n));
    let title: ArrayRef = Arc::new(StringArray::from_iter_values((0..n).map(|i| format!("The quick brown fox jumps over the lazy dog, story number {}", first + i))));
    let kind: ArrayRef = Arc::new(["click", "view", "click"].into_iter().collect::<DictionaryArray<Int32Type>>());
    let city: ArrayRef = Arc::new(StringArray::from(vec![Some("Lisbon"), Some("Porto"), None]));
    let score: ArrayRef = Arc::new(Float64Array::from(vec![0.5, f64::NAN, 2.0]));
    let emb: ArrayRef = Arc::new(FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>((0..n).map(|i| Some(vec![Some(i as f32), Some(1.0), Some(0.0)])), 3));
    let mut tags = ListBuilder::new(StringBuilder::new());
    for i in 0..n {
        tags.values().append_value("new");
        if i % 2 == 0 {
            tags.values().append_value("sale");
        }
        tags.append(true);
    }
    let tags: ArrayRef = Arc::new(tags.finish());
    let at: ArrayRef = Arc::new(TimestampMillisecondArray::from(vec![1_767_225_600_000, 1_767_225_600_500, 1_767_312_000_000]).with_timezone("UTC"));
    let day: ArrayRef = Arc::new(Date32Array::from(vec![20454, 20455, 20456]));
    let price: ArrayRef = Arc::new(Decimal128Array::from(vec![1250, 200, 99_999]).with_precision_and_scale(10, 2).unwrap());
    let photo: ArrayRef = Arc::new(BinaryArray::from_vec(vec![b"\x00\xff", b"", b"png"]));
    let extra: ArrayRef = Arc::new(StructArray::from(vec![
        (Arc::new(AField::new("a", DataType::Int64, true)), Arc::new(Int64Array::from(vec![1, 2, 3])) as ArrayRef),
        (Arc::new(AField::new("b", DataType::Utf8, true)), Arc::new(StringArray::from(vec!["x", "y", "z"])) as ArrayRef),
    ]));
    let clock: ArrayRef = Arc::new(Time64MicrosecondArray::from(vec![1, 2, 3]));
    let pair: ArrayRef = Arc::new(ListArray::from_iter_primitive::<Float64Type, _, _>((0..n).map(|i| Some(vec![Some(i as f64), Some(-1.0)]))));
    let ok: ArrayRef = Arc::new(BooleanArray::from(vec![true, false, true]));
    RecordBatch::try_from_iter([
        ("id", id),
        ("title", title),
        ("kind", kind),
        ("City Name", city),
        ("score", score),
        ("emb", emb),
        ("tags", tags),
        ("at", at),
        ("day", day),
        ("price", price),
        ("photo", photo),
        ("extra", extra),
        ("clock", clock),
        ("pair", pair),
        ("ok", ok),
    ])
    .unwrap()
}

/// A directory as Spark writes one: part files under Hive partitions, a
/// `_SUCCESS` marker and hidden checksums.
fn spark_dir(dir: &Path) {
    write(&dir.join("year=2025/part-00000-1a2b.snappy.parquet"), &events(1));
    write(&dir.join("year=2026/part-00000-3c4d.snappy.parquet"), &events(4));
    std::fs::write(dir.join("_SUCCESS"), "").unwrap();
    std::fs::write(dir.join("year=2025/.part-00000-1a2b.snappy.parquet.crc"), "crc").unwrap();
}

#[test]
fn a_spark_directory_becomes_a_table() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("events");
    spark_dir(&src);
    assert!(pq::is_parquet(&src) && pq::is_parquet(&src.join("year=2025/part-00000-1a2b.snappy.parquet")));
    assert!(!pq::is_parquet(&src.join("_SUCCESS")));

    let plan = pq::plan(&src, None, &ImportOptions::default()).unwrap();
    let s = &plan.schema;
    assert_eq!(s.table, "events");
    assert_eq!((plan.files, plan.rows, plan.exists), (2, 6, false));
    assert_eq!(s.primary_key.map(|k| s.fields[k].name.as_str()), Some("id"));
    assert_eq!(field(s, "id").ty, FieldType::Int);
    assert!(matches!(field(s, "title").ty, FieldType::Text { .. }), "prose");
    assert_eq!(field(s, "kind").ty, FieldType::Keyword, "dictionary-encoded");
    assert_eq!(field(s, "City_Name").ty, FieldType::Keyword, "labels");
    assert_eq!(field(s, "score").ty, FieldType::Float);
    assert!(matches!(field(s, "emb").ty, FieldType::Vector { dim: 3, .. }));
    assert!(field(s, "tags").multi && field(s, "tags").ty == FieldType::Keyword);
    assert_eq!(field(s, "at").logical, Some(strata::schema::Logical::Timestamp));
    assert_eq!(field(s, "day").logical, Some(strata::schema::Logical::Timestamp));
    assert_eq!(field(s, "price").ty, FieldType::Float);
    assert_eq!(field(s, "extra").ty, FieldType::Json);
    assert!(matches!(field(s, "pair").ty, FieldType::Vector { dim: 2, .. }), "lists of one length");
    assert_eq!(field(s, "ok").logical, Some(strata::schema::Logical::Bool));
    assert_eq!(field(s, "year").ty, FieldType::Int, "a Hive partition of integers");
    assert!(s.field("photo").is_none() && s.field("clock").is_none());
    let skipped: Vec<&str> = plan.skipped.iter().map(|(c, _)| c.as_str()).collect();
    assert_eq!(skipped, ["photo", "clock"]);
    let j = plan.to_json();
    assert_eq!(j["columns"]["City Name"], "City_Name");
    assert_eq!(j["partitions"]["year"], "year");
    assert!(j["skipped"]["photo"].as_str().unwrap().contains("binary"));
    assert!(plan.notes.iter().any(|n| n.contains("id is the key")), "{:?}", plan.notes);
    assert!(plan.notes.iter().any(|n| n.starts_with("price: decimal(10, 2)")), "{:?}", plan.notes);

    let (db, done) = pq::import_new(&src, &dir.path().join("events.strata"), DbOptions::default(), &ImportOptions::default()).unwrap();
    assert_eq!((done.rows, done.files, done.created, done.bulk), (6, 2, true, true));
    assert!(done.notes.iter().any(|n| n == "score: 2 NaN or infinite values read as NULL"), "{:?}", done.notes);
    assert_eq!(sorted(&db, "SELECT id FROM events WHERE year = 2026"), [4, 5, 6].map(Value::Int));
    assert_eq!(ids(&db, "SELECT count(*) FROM events WHERE MATCH(title, 'foxes')"), [Value::Int(6)]);
    assert_eq!(sorted(&db, "SELECT id FROM events WHERE kind = 'view'"), [2, 5].map(Value::Int));
    assert_eq!(sorted(&db, "SELECT id FROM events WHERE tags = 'sale' AND year = 2025"), [1, 3].map(Value::Int));
    assert_eq!(ids(&db, "SELECT City_Name FROM events WHERE id = 1"), [Value::Str("Lisbon".into())]);
    assert_eq!(ids(&db, "SELECT score FROM events WHERE id = 2"), [Value::Null]);
    assert_eq!(ids(&db, "SELECT price FROM events WHERE id = 3"), [Value::Float(999.99)]);
    assert_eq!(ids(&db, "SELECT at FROM events WHERE id = 2"), [Value::Timestamp(strata::timestamp::parse("2026-01-01T00:00:00.5Z").unwrap())]);
    assert_eq!(ids(&db, "SELECT day FROM events WHERE id = 1"), [Value::Timestamp(strata::timestamp::parse("2026-01-01").unwrap())]);
    assert_eq!(ids(&db, "SELECT ok FROM events WHERE id = 2"), [Value::Bool(false)]);
    assert_eq!(ids(&db, "SELECT extra FROM events WHERE id = 6"), [Value::Str(r#"{"a":3,"b":"z"}"#.into())]);
    assert_eq!(ids(&db, "SELECT emb FROM events WHERE id = 3"), [Value::Vector(vec![2.0, 1.0, 0.0].into())]);
    let q = Params::new().set("q", Value::Vector(vec![2.0, 1.0, 0.0].into()));
    let near = db.query("SELECT id FROM events WHERE year = 2025 ORDER BY similarity(emb, :q) DESC LIMIT 1", &q).unwrap().rows;
    assert_eq!(near, [[Value::Int(3)]]);

    // Binary columns as base64, on request.
    let opts = ImportOptions { blobs_base64: true, table: Some("raw".into()), ..Default::default() };
    let plan = pq::plan(&src, Some(&db), &opts).unwrap();
    assert_eq!(plan.schema.table, "raw");
    assert_eq!(field(&plan.schema, "photo").ty, FieldType::Keyword);
    pq::import(&db, &src, &opts).unwrap();
    assert_eq!(ids(&db, "SELECT photo FROM raw WHERE id = 1"), [Value::Str("AP8=".into())]);
}

#[test]
fn into_an_existing_table_and_given_keys() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("events.parquet");
    write(&file, &events(1));
    let schema = Schema::new("events", vec![Field::new("id", FieldType::Int), Field::new("kind", FieldType::Keyword), Field::new("visits", FieldType::Int)], Some("id")).unwrap();
    let db = Db::create(&dir.path().join("crm.strata"), schema, DbOptions::default()).unwrap();
    let mut t = db.storage().begin().unwrap();
    t.insert(vec![Value::Int(1), "old".into(), Value::Int(7)]).unwrap();
    t.insert(vec![Value::Int(9), "other".into(), Value::Int(1)]).unwrap();
    t.commit().unwrap();
    let plan = pq::plan(&file, Some(&db), &ImportOptions::default()).unwrap();
    assert!(plan.exists);
    assert_eq!(plan.columns.iter().map(|(c, _)| c.as_str()).collect::<Vec<_>>(), ["id", "kind"]);
    assert!(plan.skipped.iter().any(|(c, why)| c == "title" && why.contains("no field title in table events")), "{:?}", plan.skipped);
    let done = pq::import(&db, &file, &ImportOptions::default()).unwrap();
    assert!(!done.created && !done.bulk);
    assert_eq!(done.rows, 3);
    // Upserted by key: 1 is replaced (its visits too: the row is the file's), 9 stays.
    assert_eq!(ids(&db, "SELECT kind FROM events WHERE id = 1"), [Value::Str("click".into())]);
    assert_eq!(ids(&db, "SELECT visits FROM events WHERE id = 1"), [Value::Null]);
    assert_eq!(sorted(&db, "SELECT id FROM events"), [1, 2, 3, 9].map(Value::Int));

    // Another key, or none.
    let by_title = ImportOptions { key: Some("title".into()), table: Some("stories".into()), ..Default::default() };
    let plan = pq::plan(&file, None, &by_title).unwrap();
    assert_eq!(plan.schema.primary_key.map(|k| plan.schema.fields[k].name.as_str()), Some("title"));
    assert_eq!(field(&plan.schema, "title").ty, FieldType::Keyword, "a key is a KEYWORD");
    let none = ImportOptions { key: Some(String::new()), ..Default::default() };
    assert_eq!(pq::plan(&file, None, &none).unwrap().schema.primary_key, None);
    let e = pq::plan(&file, None, &ImportOptions { key: Some("nope".into()), ..Default::default() }).unwrap_err().to_string();
    assert!(e.contains("key nope: no such column"), "{e}");
    let e = pq::plan(&file, None, &ImportOptions { key: Some("score".into()), ..Default::default() }).unwrap_err().to_string();
    assert!(e.contains("a key is integers or text"), "{e}");

    // A schema given: the plan's JSON, edited.
    let mut j = pq::plan(&file, None, &ImportOptions::default()).unwrap().to_json();
    j["table"] = json!("given");
    let fields = j["fields"].as_array_mut().unwrap();
    let title = fields.iter_mut().find(|f| f["name"] == "title").unwrap();
    title["type"] = json!("keyword");
    let given = Schema::from_json(&j.to_string()).unwrap();
    let (db2, done) = pq::import_new(&file, &dir.path().join("given.strata"), DbOptions::default(), &ImportOptions { schema: Some(given), ..Default::default() }).unwrap();
    assert_eq!(done.table, "given");
    assert_eq!(ids(&db2, "SELECT id FROM given WHERE title = 'The quick brown fox jumps over the lazy dog, story number 2'"), [Value::Int(2)]);
}

#[test]
fn a_value_that_does_not_fit_names_its_file_and_row() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("events");
    spark_dir(&src);
    // A table saying "City Name" is an INT, which "Lisbon" is not.
    let s = Schema::new("events", vec![Field::new("id", FieldType::Int), Field::new("City_Name", FieldType::Int)], Some("id")).unwrap();
    let e = pq::import_new(&src, &dir.path().join("x.strata"), DbOptions::default(), &ImportOptions { schema: Some(s), ..Default::default() }).err().unwrap().to_string();
    assert!(e.contains("year=2025/part-00000-1a2b.snappy.parquet: row 1") && e.contains("City_Name"), "{e}");
    // Not Parquet, and nothing to read.
    let text = dir.path().join("rows.csv");
    std::fs::write(&text, "a,b\n1,2\n").unwrap();
    assert!(!pq::is_parquet(&text));
    let e = pq::plan(&text, None, &ImportOptions::default()).unwrap_err().to_string();
    assert!(e.contains("not a Parquet file"), "{e}");
    let empty = dir.path().join("empty");
    std::fs::create_dir(&empty).unwrap();
    let e = pq::plan(&empty, None, &ImportOptions::default()).unwrap_err().to_string();
    assert!(e.contains("no Parquet files"), "{e}");
    // A key with NULLs in the sample is not guessed.
    let nulls = RecordBatch::try_from_iter([("id", Arc::new(StringArray::from(vec![Some("a"), None])) as ArrayRef)]).unwrap();
    write(&dir.path().join("nulls.parquet"), &nulls);
    assert_eq!(pq::plan(&dir.path().join("nulls.parquet"), None, &ImportOptions::default()).unwrap().schema.primary_key, None);
}

const COVER: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

/// Every kind of field, in two tables.
fn library(path: &Path) -> Db {
    let authors = vec![
        Field::new("id", FieldType::Int),
        Field::new("name", FieldType::Keyword).not_null(),
        Field::new("bio", FieldType::Text { positions: true }).analyzer(strata::schema::Analyzer::Simple),
        Field::boolean("active").default(json!(true)),
        Field::timestamp("joined").default(json!("now")),
    ];
    let authors = Schema::new("authors", authors, Some("id")).unwrap();
    let db = Db::create(path, authors, DbOptions::default()).unwrap();
    let books = Schema::new(
        "books",
        vec![
            Field::new("isbn", FieldType::Keyword),
            Field::new("title", FieldType::Text { positions: true }),
            Field::new("blurb", FieldType::Text { positions: false }),
            Field::new("body", FieldType::Text { positions: true }).generated(&["title", "blurb"]).not_stored(),
            Field::new("author", FieldType::Int).references("authors"),
            Field::new("price", FieldType::Float).default(json!(9.99)),
            Field::new("pages", FieldType::Int).not_null(),
            Field::new("tags", FieldType::Keyword).multi(),
            Field::new("years", FieldType::Int).multi(),
            Field::new("cites", FieldType::Edges),
            Field::new("terms", FieldType::Sparse),
            Field::new("emb", FieldType::Vector { dim: 3, index: strata::VectorIndexKind::Flat }),
            Field::new("views", FieldType::Vector { dim: 2, index: strata::VectorIndexKind::Flat }).multi(),
            Field::new("info", FieldType::Json),
            Field::timestamp("published"),
            Field::boolean("in_print"),
            Field::new("shelf", FieldType::Keyword).default(json!("new")),
            Field::media("cover", strata::schema::MediaKind::Image),
        ],
        Some("isbn"),
    )
    .unwrap();
    db.create_table(books).unwrap();
    db.sql(
        "INSERT INTO authors (id, name, bio, joined) VALUES
           (1, 'Le Guin', 'Wrote about wizards and anarchists', '2020-01-01T00:00:00Z'),
           (2, 'Calvino', NULL, '2021-06-15 08:30:00.125')",
        &Params::new(),
    )
    .unwrap();
    db.sql("INSERT INTO authors (id, name, active) VALUES (3, 'Borges', false)", &Params::new()).unwrap();
    let mut t = db.storage().begin().unwrap();
    let mut b = t.table("books").unwrap();
    let row = |isbn: &str, title: &str, author: i64, tags: &[&str], cites: &str, emb: [f32; 3], published: &str, in_print: bool| {
        vec![
            isbn.into(),
            title.into(),
            Value::Str(format!("A novel called {title}")),
            Value::Null,
            Value::Int(author),
            Value::Float(12.0),
            Value::Int(200),
            Value::List(tags.iter().map(|&s| Value::from(s)).collect()),
            Value::List(vec![Value::Int(1970), Value::Int(1972)]),
            Value::Str(cites.into()),
            Value::Str(r#"{"wizard": 1.5, "sea": 0.25}"#.into()),
            Value::Vector(emb.to_vec().into()),
            Value::List(vec![Value::Vector(vec![1.0, 0.0].into()), Value::Vector(vec![0.0, 1.0].into())]),
            Value::Str(r#"{"lang": "en", "awards": ["Hugo", "Nebula"]}"#.into()),
            Value::Timestamp(strata::timestamp::parse(published).unwrap()),
            Value::Bool(in_print),
            Value::Str("fiction".into()),
            Value::Str(strata::media::input_text(&json!({"uri": format!("https://example.com/{isbn}.png"), "sha256": COVER, "mime": "image/png", "bytes": 1234, "width": 600, "height": 900})).unwrap()),
        ]
    };
    b.insert(row("isbn-1", "A Wizard of Earthsea", 1, &["fantasy", "classic"], "[]", [1.0, 0.0, 0.0], "1968-11-01", true)).unwrap();
    b.insert(row("isbn-2", "Invisible Cities", 2, &["fable"], r#"["isbn-1"]"#, [0.0, 1.0, 0.0], "1972-01-01T12:00:00.5Z", false)).unwrap();
    t.commit().unwrap();
    db.sql("INSERT INTO books (isbn, title, author, pages) VALUES ('isbn-3', 'Ficciones', 3, 180)", &Params::new()).unwrap();
    db
}

fn rows_of(db: &Db, table: &str) -> Vec<Vec<Value>> {
    let snap = db.snapshot();
    let t = snap.table(table).unwrap();
    let schema = t.schema().clone();
    let mut docs: Vec<Vec<Value>> = db.documents(&t).collect::<strata::Result<_>>().unwrap();
    let k = schema.primary_key.unwrap();
    docs.sort_by_key(|d| format!("{:?}", d[k]));
    docs
}

#[test]
fn strata_to_parquet_and_back() {
    let dir = tempfile::tempdir().unwrap();
    let db = library(&dir.path().join("library.strata"));
    let out = dir.path().join("library");
    let done = pq::export(&db, &out, &ExportOptions::default()).unwrap();
    assert_eq!(done.tables.iter().map(|t| (t.table.as_str(), t.rows)).collect::<Vec<_>>(), [("authors", 3), ("books", 3)]);
    assert_eq!(done.tables[1].path, out.join("books.parquet"));
    assert!(done.tables[1].bytes > 0 && std::fs::metadata(&done.tables[1].path).unwrap().len() == done.tables[1].bytes);
    assert!(done.tables[1].notes.iter().any(|n| n.starts_with("cites: edges as JSON text")), "{:?}", done.tables[1].notes);
    assert_eq!(done.to_json()["tables"][1]["file"], "books.parquet");
    assert!(pq::export(&db, &out, &ExportOptions::default()).unwrap_err().to_string().contains("exists"));
    let e = pq::export(&db, &dir.path().join("all.parquet"), &ExportOptions::default()).unwrap_err().to_string();
    assert!(e.contains("one file, for one table"), "{e}");
    // A directory of tables is not one table.
    let e = pq::plan(&out, None, &ImportOptions::default()).err().unwrap().to_string();
    assert!(e.contains("different ChloeDB tables"), "{e}");

    // What any Parquet reader sees.
    let b = ParquetRecordBatchReaderBuilder::try_new(File::open(out.join("books.parquet")).unwrap()).unwrap();
    let kv = b.metadata().file_metadata().key_value_metadata().unwrap();
    assert!(kv.iter().any(|k| k.key == pq::META_KEY));
    let ty = |c: &str| b.schema().field_with_name(c).unwrap().data_type().clone();
    let list = |t: DataType| DataType::List(Arc::new(AField::new("item", t, true)));
    assert_eq!(ty("author"), DataType::Int64);
    assert_eq!(ty("price"), DataType::Float64);
    assert_eq!(ty("tags"), list(DataType::Utf8));
    assert_eq!(ty("years"), list(DataType::Int64));
    assert_eq!(ty("views"), list(DataType::FixedSizeList(Arc::new(AField::new("item", DataType::Float32, true)), 2)));
    for c in ["isbn", "title", "body", "cites", "terms", "info", "cover"] {
        assert_eq!(ty(c), DataType::Utf8, "{c}");
    }
    assert_eq!(b.schema().field_with_name("emb").unwrap().data_type(), &DataType::FixedSizeList(Arc::new(AField::new("item", DataType::Float32, true)), 3));
    assert_eq!(b.schema().field_with_name("published").unwrap().data_type(), &DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())));
    assert_eq!(b.schema().field_with_name("in_print").unwrap().data_type(), &DataType::Boolean);
    assert!(!b.schema().field_with_name("isbn").unwrap().is_nullable(), "the key");
    let rows: usize = b.build().unwrap().map(|r| r.unwrap().num_rows()).sum();
    assert_eq!(rows, 3);

    // And back: the same schemas and rows, a file at a time.
    let back_path = dir.path().join("back.strata");
    let (back, a) = pq::import_new(&out.join("authors.parquet"), &back_path, DbOptions::default(), &ImportOptions::default()).unwrap();
    assert!(a.notes.is_empty() && a.skipped.is_empty(), "{:?} {:?}", a.notes, a.skipped);
    let b = pq::import(&back, &out.join("books.parquet"), &ImportOptions::default()).unwrap();
    assert!(b.created && b.bulk && b.notes.is_empty() && b.skipped.is_empty(), "{b:?}");
    for (a, b) in db.catalog().iter().zip(back.catalog()) {
        assert_eq!(a.to_short_json(), b.to_short_json());
    }
    for t in ["authors", "books"] {
        assert_eq!(rows_of(&db, t), rows_of(&back, t), "{t}");
    }

    // One table, to one file, in any case.
    let one = dir.path().join("Books.parquet");
    let done = pq::export(&db, &one, &ExportOptions { tables: vec!["BOOKS".into()], ..Default::default() }).unwrap();
    assert_eq!(done.tables[0].path, one);
    assert_eq!(pq::plan(&one, None, &ImportOptions::default()).unwrap().schema.table, "books", "the table the file keeps");
    // An empty directory takes the files.
    let empty = dir.path().join("empty");
    std::fs::create_dir(&empty).unwrap();
    pq::export(&db, &empty, &ExportOptions { tables: vec!["authors".into()], meta: false }).unwrap();
    assert!(empty.join("authors.parquet").is_file());
    // Without the schema, the file is plain Parquet, read by its columns.
    let plan = pq::plan(&empty.join("authors.parquet"), None, &ImportOptions::default()).unwrap();
    assert!(plan.schema.field("bio").is_some_and(|f| matches!(f.ty, FieldType::Text { .. })));
    assert_eq!(field(&plan.schema, "joined").default, None, "defaults are the kept schema's");
}

#[test]
fn later_files_add_columns() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("logs");
    let a = RecordBatch::try_from_iter([("id", Arc::new(Int64Array::from(vec![1, 2])) as ArrayRef), ("msg", Arc::new(StringArray::from(vec!["start", "stop"])) as ArrayRef)]).unwrap();
    let b = RecordBatch::try_from_iter([
        ("id", Arc::new(Int64Array::from(vec![3])) as ArrayRef),
        ("level", Arc::new(StringArray::from(vec!["warn"])) as ArrayRef),
        ("msg", Arc::new(StringArray::from(vec!["again"])) as ArrayRef),
    ])
    .unwrap();
    write(&src.join("part-0.parquet"), &a);
    write(&src.join("part-1.parquet"), &b);
    let plan = pq::plan(&src, None, &ImportOptions::default()).unwrap();
    assert_eq!(plan.schema.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["id", "msg", "level"]);
    let (db, done) = pq::import_new(&src, &dir.path().join("logs.strata"), DbOptions::default(), &ImportOptions::default()).unwrap();
    assert_eq!(done.rows, 3);
    assert_eq!(sorted(&db, "SELECT level FROM logs"), [Value::Null, Value::Null, Value::Str("warn".into())]);
}
