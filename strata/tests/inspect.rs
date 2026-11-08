//! The index inspector's statistics of a table (`strata::inspect`): the
//! documents and bytes read from every segment, exact; value and document
//! sizes from a sample, near the truth; tokens from the length norms;
//! distinct terms from a sketch; and how shards' statistics add up.

use strata::db::Db;
use strata::inspect::{self, Options, TableStats};
use strata::query::Params;
use strata::schema::VectorIndexKind;
use strata::storage::DbOptions;
use strata::{Field, FieldType, Schema, Value};

fn schema() -> Schema {
    Schema::new(
        "docs",
        vec![
            Field::new("id", FieldType::Keyword),
            Field::new("body", FieldType::Text { positions: false }),
            Field::new("tags", FieldType::Keyword).multi(),
            Field::new("n", FieldType::Int),
            Field::timestamp("at"),
            Field::new("summary", FieldType::Text { positions: false }).stored_only(),
            Field::new("note", FieldType::Text { positions: false }).not_stored().generated(&["summary"]),
            Field::new("emb", FieldType::Vector { dim: 8, index: VectorIndexKind::Flat }),
        ],
        Some("id"),
    )
    .unwrap()
}

/// Row `i`: a body of `i % 50 + 1` words (of 300 distinct ones), tags on
/// every other row, a summary on every third (and the note generated from
/// it, not stored), a vector on every row.
fn row(i: usize) -> Vec<Value> {
    let words: Vec<String> = (0..i % 50 + 1).map(|w| format!("w{}", (i * 7 + w * 13) % 300)).collect();
    let tags = if i % 2 == 0 { Value::List(vec![Value::Str(format!("t{}", i % 40)), Value::Str("all".into())]) } else { Value::Null };
    let summary = if i % 3 == 0 { Value::Str(format!("note {i} of the row")) } else { Value::Null };
    let emb: Vec<f32> = (0..8).map(|d| ((i + d) % 5) as f32 / 5.0).collect();
    vec![Value::Str(format!("d{i:05}")), Value::Str(words.join(" ")), tags, Value::Int(i as i64), Value::Timestamp(1_700_000_000_000_000 + i as i64 * 1_000_000), summary, Value::Null, Value::Vector(emb.into())]
}

fn open(dir: &std::path::Path, n: usize, from: usize) -> Db {
    let opts = DbOptions { segment_max_docs: 700, memtable_max_docs: 1_000_000, memtable_chunk_docs: 64, sync: false, publish_snapshots: true, ..DbOptions::default() };
    let db = Db::create(&dir.join(format!("i{from}.strata")), schema(), opts).unwrap();
    db.load((from..from + n).map(row)).unwrap();
    db
}

fn field<'a>(s: &'a TableStats, name: &str) -> &'a inspect::FieldStats {
    s.fields.iter().find(|f| f.name == name).unwrap_or_else(|| panic!("no field {name}"))
}

#[test]
fn a_tables_statistics_are_exact_where_read_and_near_where_sampled() {
    let dir = tempfile::tempdir().unwrap();
    let n = 3000;
    let db = open(dir.path(), n, 0);
    // Deletes and writes after the load: on disk and in memory.
    for i in (0..300).step_by(3) {
        db.sql("DELETE FROM docs WHERE id = :k", &Params::new().set("k", Value::Str(format!("d{i:05}")))).unwrap();
    }
    let activity = db.storage().activity("docs").unwrap();
    assert!(activity.last_ms > 0 && activity.ops >= 3000 + 100, "{activity:?}");
    let live = n - 100;
    let snap = db.snapshot();
    let s = inspect::table(&snap, &Options { sample: 1000, ..Options::default() }).unwrap();

    assert_eq!(s.table, "docs");
    assert_eq!(s.primary_key.as_deref(), Some("id"));
    assert_eq!(s.docs.live, live as u64);
    assert_eq!(s.docs.deleted, 100);
    assert!(s.segments.len() >= 5, "{:?}", s.segments.len());
    // Every byte of every segment is a field's, the store's, another
    // section's or overhead.
    let other: u64 = s.bytes.other.values().sum();
    assert_eq!(s.bytes.fields + s.bytes.store + other + s.bytes.overhead, s.bytes.total);
    assert_eq!(s.bytes.total, s.segments.iter().map(|g| g.bytes).sum::<u64>());
    let index_sum: u64 = s.fields.iter().map(|f| f.index_bytes).sum();
    assert_eq!(index_sum, s.bytes.fields);
    assert!(s.bytes.other.contains_key("pk"), "{:?}", s.bytes.other);
    assert!(s.bytes.store_raw > s.bytes.store, "the store compresses: {:?}", s.bytes);

    // The body: every row has one; its tokens from the norms, exact for
    // these short lengths; its value size near the truth.
    let body = field(&s, "body");
    assert!(body.sections.contains_key("text.post") && body.sections.contains_key("text.dict"), "{:?}", body.sections);
    let tokens = body.tokens.as_ref().unwrap();
    assert!((tokens.n - live as f64).abs() < 0.5, "{tokens:?}");
    assert_eq!((tokens.min, tokens.max), (Some(1), Some(50)));
    let true_mean = (0..n).filter(|i| !(i % 3 == 0 && *i < 300)).map(|i| (i % 50 + 1) as f64).sum::<f64>() / live as f64;
    // Lengths of 24 and more are rounded down, by at most 12.5%.
    let mean = tokens.mean.unwrap();
    assert!(mean <= true_mean + 1e-9 && mean > true_mean * 0.95, "{mean} vs {true_mean}");
    assert!((body.present / live as f64 - 1.0).abs() < 0.01, "{}", body.present);
    let bytes_mean = (0..n).map(|i| row(i)[1].clone()).map(|v| if let Value::Str(s) = v { s.len() as f64 } else { 0.0 }).sum::<f64>() / n as f64;
    assert!((body.size.mean.unwrap() / bytes_mean - 1.0).abs() < 0.1, "{:?} vs {bytes_mean}", body.size.mean);
    let terms = body.terms.as_ref().unwrap();
    assert!((terms.estimate.unwrap() - 300.0).abs() < 10.0, "{:?}", terms.estimate);
    assert!(terms.low <= 300 && terms.high >= 300, "{terms:?}");

    // Tags on the even rows, two values each: t0, t2 .. t38 and "all", 21
    // distinct.
    let tags = field(&s, "tags");
    assert!((tags.present / live as f64 - 0.5).abs() < 0.06, "{}", tags.present);
    assert_eq!(tags.items.as_ref().unwrap().p50, Some(2.0));
    assert!((tags.terms.as_ref().unwrap().estimate.unwrap() - 21.0).abs() < 1.5, "{:?}", tags.terms.as_ref().map(|t| t.estimate));

    // The note is not stored: its fill comes from the norms, exactly.
    let note = field(&s, "note");
    assert_eq!(note.present_from, "norms");
    let notes = (0..n).filter(|i| i % 3 == 0 && *i >= 300).count();
    assert_eq!(note.present as usize, notes);
    assert_eq!(note.store_bytes, 0.0);

    // Numeric ranges over the rows written; the timestamp in microseconds.
    let num = field(&s, "n");
    assert_eq!((num.min, num.max), (Some(0.0), Some((n - 1) as f64)));
    let at = field(&s, "at");
    assert_eq!(at.kind, "timestamp");
    assert_eq!(at.max, Some(1_700_000_000_000_000.0 + (n - 1) as f64 * 1e6));
    assert_eq!(num.size.p50, Some(8.0));

    // The vector: every row, 32 bytes, in its own sections.
    let emb = field(&s, "emb");
    assert_eq!((emb.kind.as_str(), emb.dim), ("vector", Some(8)));
    assert!((emb.present / live as f64 - 1.0).abs() < 0.01, "{}", emb.present);
    assert_eq!(emb.size.p50, Some(32.0));
    assert!(emb.index_bytes > 0);

    // The stored fields' shares of the store add up to it.
    let shares: f64 = s.fields.iter().map(|f| f.store_bytes).sum();
    assert!(shares <= s.bytes.store as f64 * 1.0001 && shares > s.bytes.store as f64 * 0.7, "{shares} of {}", s.bytes.store);
    assert!(s.doc_bytes.p50.unwrap() > 20.0 && s.sample.docs >= 900 && s.sample.of == live as u64, "{:?} {:?}", s.doc_bytes.p50, s.sample);

    // The same seed reads the same sample.
    let again = inspect::table(&snap, &Options { sample: 1000, ..Options::default() }).unwrap();
    assert_eq!(again.doc_bytes, s.doc_bytes);
}

#[test]
fn shards_statistics_add_up_to_the_whole_tables() {
    let dir = tempfile::tempdir().unwrap();
    let whole = open(dir.path(), 2000, 0);
    let (a, b) = (open(dir.path(), 1200, 10_000), open(dir.path(), 800, 20_000));
    let all = Options { sample: 100_000, ..Options::default() };
    let (sa, sb) = (inspect::table(&a.snapshot(), &all).unwrap(), inspect::table(&b.snapshot(), &all).unwrap());
    let mut merged = sa.clone();
    merged.merge(&sb);
    merged.finish(false);
    assert_eq!(merged.docs.live, 2000);
    assert_eq!(merged.bytes.total, sa.bytes.total + sb.bytes.total);
    let body = field(&merged, "body");
    assert_eq!(body.index_bytes, field(&sa, "body").index_bytes + field(&sb, "body").index_bytes);
    // Both read every row: the merged distributions are the sum.
    assert_eq!(body.size.sampled, 2000);
    assert_eq!(body.tokens.as_ref().unwrap().n, 2000.0);
    // The sketches merge: the same 300 words in both.
    let terms = body.terms.as_ref().unwrap();
    assert!((terms.estimate.unwrap() - 300.0).abs() < 10.0, "{terms:?}");
    assert!(terms.sketch.is_none(), "finish(false) drops the sketches");
    assert_eq!(field(&merged, "n").min, Some(10_000.0));
    assert_eq!(field(&merged, "n").max, Some(20_799.0));
    let one = inspect::table(&whole.snapshot(), &all).unwrap();
    assert_eq!(one.docs.live, merged.docs.live);
    let mean = |s: &TableStats| field(s, "body").tokens.as_ref().unwrap().mean.unwrap();
    // Rows 0..2000 and 10000..11200 + 20000..20800 have the same lengths
    // pattern (i % 50).
    assert!((mean(&one) - mean(&merged)).abs() < 0.5, "{} vs {}", mean(&one), mean(&merged));
    // A round trip as JSON, as a coordinator gets it.
    let j = serde_json::to_string(&sa).unwrap();
    let back: TableStats = serde_json::from_str(&j).unwrap();
    assert_eq!(back.bytes.total, sa.bytes.total);
    assert_eq!(field(&back, "body").terms.as_ref().unwrap().sketch, field(&sa, "body").terms.as_ref().unwrap().sketch);
}
