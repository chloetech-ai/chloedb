//! Doc store round trips, the zero-copy contract of the forward readers,
//! and the forward sections built through `build_segment`.

mod forward_common;

use forward_common::build;
use rand::{rngs::StdRng, Rng, SeedableRng};
use chloedb::forward::{DocStore, KeywordColumn, NumericColumn, RangeIndex, StoreCache};
use chloedb::segment::build::{build_segment, BuildOptions};
use chloedb::segment::SegmentFile;
use chloedb::schema::Compression;
use chloedb::{data, Document, Field, FieldType, Schema, Value, VectorIndexKind};

fn schema() -> Schema {
    Schema::new(
        "t",
        vec![
            Field::new("id", FieldType::Keyword),
            Field::new("title", FieldType::Text { positions: false }).stored_only(),
            Field::new("n", FieldType::Int),
            Field::new("x", FieldType::Float).stored_only(),
            Field::new("hidden", FieldType::Text { positions: false }).stored_only().not_stored(),
            Field::new("v", FieldType::Vector { dim: 2, index: VectorIndexKind::Flat }).stored_only(),
        ],
        Some("id"),
    )
    .unwrap()
}

fn random_docs(rng: &mut StdRng, n: usize) -> Vec<Document> {
    (0..n)
        .map(|d| {
            let len = if rng.gen_bool(0.01) { rng.gen_range(0..60_000) } else { rng.gen_range(0..200) };
            let title: String = (0..len).map(|_| ['a', 'b', ' ', 'é', '\n', 'z'][rng.gen_range(0..6)]).collect();
            let int = Value::Int(rng.gen::<i64>() >> rng.gen_range(0..64));
            let float = Value::Float((rng.gen::<f64>() - 0.5) * 1e9);
            let mut opt = |v: Value| if rng.gen_bool(0.1) { Value::Null } else { v };
            vec![
                Value::Str(format!("doc-{d}")),
                opt(Value::Str(title)),
                opt(int),
                opt(float),
                Value::Str("not stored".into()),
                Value::Vector(vec![1.0, 2.0].into()),
            ]
        })
        .collect()
}

/// What the store returns: stored fields, `Null` for the rest.
fn expected(schema: &Schema, doc: &Document) -> Document {
    schema
        .fields
        .iter()
        .zip(doc)
        .map(|(f, v)| if f.stored && !matches!(f.ty, FieldType::Vector { .. }) { v.clone() } else { Value::Null })
        .collect()
}

#[test]
fn random_documents_round_trip() {
    for c in [Compression::Lz4, Compression::Zstd] {
        round_trip(schema().with_compression(c));
    }
}

fn round_trip(schema: Schema) {
    let mut rng = StdRng::seed_from_u64(1);
    for (n, block) in [(0usize, 16384usize), (1, 16384), (3000, 16384), (3000, 1), (3000, 1 << 20)] {
        let docs = random_docs(&mut rng, n);
        let seg = build(&schema, &docs, &BuildOptions::default().set("forward.store_block", block));
        let store = DocStore::open(&seg, &schema).unwrap();
        assert!(store.is_view());
        assert_eq!(store.len(), n);
        for (d, doc) in docs.iter().enumerate() {
            assert_eq!(store.get(d as u32).unwrap(), expected(&schema, doc), "doc {d}");
        }
        for _ in 0..200.min(n) {
            let d = rng.gen_range(0..n);
            let fields: Vec<usize> = (0..rng.gen_range(0..5)).map(|_| rng.gen_range(0..schema.fields.len())).collect();
            let want: Vec<Value> = fields.iter().map(|&f| expected(&schema, &docs[d])[f].clone()).collect();
            assert_eq!(store.get_fields(d as u32, &fields).unwrap(), want);
        }
        // Through a block cache: in docID order (each block decompressed
        // once), then in random order (the cache changes blocks).
        let all: Vec<usize> = (0..schema.fields.len()).collect();
        let (mut cache, mut whole) = (StoreCache::default(), StoreCache::default());
        for (d, doc) in docs.iter().enumerate() {
            assert_eq!(store.get_fields_cached(d as u32, &all, &mut cache).unwrap(), expected(&schema, doc), "doc {d}, cached");
            assert_eq!(store.get_cached(d as u32, &mut whole).unwrap(), expected(&schema, doc), "doc {d}, whole, cached");
        }
        for _ in 0..200.min(n) {
            let d = rng.gen_range(0..n);
            let fields: Vec<usize> = (0..rng.gen_range(0..5)).map(|_| rng.gen_range(0..schema.fields.len())).collect();
            let want: Vec<Value> = fields.iter().map(|&f| expected(&schema, &docs[d])[f].clone()).collect();
            assert_eq!(store.get_fields_cached(d as u32, &fields, &mut cache).unwrap(), want, "doc {d}, cached");
        }
        let (compressed, raw) = store.byte_sizes();
        assert!(n == 0 || compressed < raw, "{compressed} vs {raw}");
    }
}

#[test]
fn store_rejects_other_schema() {
    let schema = schema();
    let docs = random_docs(&mut StdRng::seed_from_u64(2), 10);
    let seg = build(&schema, &docs, &BuildOptions::default());
    let mut other = schema.clone();
    other.fields[2].ty = FieldType::Float;
    assert!(DocStore::open(&seg, &other).is_err());
}

#[test]
fn scifact_round_trip_and_zero_copy() {
    if !data::data_dir().join("scifact/meta.json").exists() {
        eprintln!("SciFact not prepared; skipping");
        return;
    }
    let c = data::load("scifact", None).unwrap();
    let schema = data::beir_schema(c.dim, VectorIndexKind::Flat);
    let seg = build(&schema, &c.docs, &BuildOptions::default());
    // The image again through a fresh buffer: every reader holds views.
    let seg = SegmentFile::from_vec(seg.image().to_vec()).unwrap();
    let store = DocStore::open(&seg, &schema).unwrap();
    for (d, doc) in c.docs.iter().enumerate() {
        assert_eq!(store.get(d as u32).unwrap(), expected(&schema, doc));
    }
    let (compressed, raw) = store.byte_sizes();
    eprintln!("scifact doc store: {raw} bytes raw, {compressed} compressed, {} blocks", store.n_blocks());
    // Sections are named by field id (`key_of`), not by the field's name.
    for f in ["year", "rating"] {
        let key = schema.key_of(f).unwrap();
        let col = NumericColumn::open(&seg, &key).unwrap();
        assert!(col.is_view());
        let idx = RangeIndex::open(&seg, &key).unwrap();
        assert!((0..idx.len()).all(|b| idx.bucket(b).is_view()));
        let fi = schema.field_index(f).unwrap();
        for (d, doc) in c.docs.iter().enumerate() {
            assert_eq!(col.get(d as u32), doc[fi]);
        }
    }
    for f in ["category", "topic", "id"] {
        let col = KeywordColumn::open(&seg, &schema.key_of(f).unwrap()).unwrap();
        assert!(col.is_view());
        let fi = schema.field_index(f).unwrap();
        for (d, doc) in c.docs.iter().enumerate() {
            assert_eq!(col.value_of_doc(d as u32), doc[fi].as_str());
        }
    }
}

#[test]
fn build_segment_writes_forward_sections() {
    let schema = Schema::new(
        "t",
        vec![
            Field::new("id", FieldType::Keyword),
            Field::new("year", FieldType::Int),
            Field::new("rating", FieldType::Float),
            Field::new("note", FieldType::Keyword).stored_only(),
        ],
        Some("id"),
    )
    .unwrap();
    let docs: Vec<Document> = (0..500)
        .map(|d| vec![Value::Str(format!("{d}")), Value::Int(1990 + d % 35), Value::Float((d % 500) as f64 / 100.0), Value::Str("n".into())])
        .collect();
    let seg = SegmentFile::from_vec(build_segment(&schema, &docs, &BuildOptions::default()).unwrap()).unwrap();
    // Sections are named by field id; label() names them by field.
    let names: Vec<String> = seg.section_sizes().into_iter().map(|(n, _)| chloedb::segment::label(&schema, &n)).collect();
    for s in ["col.year", "col.year.hist", "range.year", "col.rating", "kw.id.dict", "kw.id.codes", "kw.id.post", "store.index", "store.data"] {
        assert!(names.iter().any(|n| n == s), "missing {s} in {names:?}");
    }
    assert!(!names.iter().any(|n| n.starts_with("kw.note")));
    let want = (0..500).filter(|d| (10..20).contains(&(d % 35))).count();
    assert_eq!(NumericColumn::open(&seg, &schema.key_of("year").unwrap()).unwrap().scan_count(2000.0, 2009.0), want);
    assert_eq!(DocStore::open(&seg, &schema).unwrap().get(7).unwrap()[3], Value::Str("n".into()));
}

/// A segment of `n` random documents in small blocks, its image, and the
/// documents as the store returns them.
fn blocks_image(n: usize) -> (Schema, Vec<u8>, Vec<Document>) {
    blocks_image_of(n, Compression::Lz4)
}

fn blocks_image_of(n: usize, c: Compression) -> (Schema, Vec<u8>, Vec<Document>) {
    let schema = schema().with_compression(c);
    let docs = random_docs(&mut StdRng::seed_from_u64(5), n);
    let seg = build(&schema, &docs, &BuildOptions::default().set("forward.store_block", 2048));
    let want = docs.iter().map(|d| expected(&schema, d)).collect();
    (schema, seg.image().to_vec(), want)
}

/// Where the directory entry of section `name` starts in `image` (at its
/// `"name"`, which the entry's other fields follow).
fn entry_at(image: &[u8], name: &str) -> usize {
    let dir_len = u64::from_le_bytes(image[16..24].try_into().unwrap()) as usize;
    let dir = std::str::from_utf8(&image[24..24 + dir_len]).unwrap();
    24 + dir.find(&format!("\"name\":\"{name}\"")).unwrap_or_else(|| panic!("no section {name} in {dir}"))
}

#[test]
fn a_damaged_block_fails_its_checksum_and_the_others_read() {
    for c in [Compression::Lz4, Compression::Zstd] {
        damaged_block(c);
    }
}

fn damaged_block(c: Compression) {
    let (schema, mut image, want) = blocks_image_of(3000, c);
    let seg = SegmentFile::from_vec(image.clone()).unwrap();
    assert!(seg.has("store.crc"));
    let n_blocks = DocStore::open(&seg, &schema).unwrap().n_blocks();
    assert!(n_blocks > 20, "{n_blocks} blocks");
    let data = seg.section_range("store.data").unwrap();
    image[data.start + data.len() / 2] ^= 0x41;

    let seg = SegmentFile::from_vec(image.clone()).unwrap().checked_on_read();
    let store = DocStore::open(&seg, &schema).unwrap();
    let mut cache = StoreCache::default();
    let all: Vec<usize> = (0..schema.fields.len()).collect();
    let mut failed = Vec::new();
    for (d, doc) in want.iter().enumerate() {
        match store.get(d as u32) {
            Ok(got) => assert_eq!(&got, doc, "doc {d}"),
            Err(e) => {
                assert!(e.to_string().contains("fails its checksum"), "doc {d}: {e}");
                failed.push(d);
            }
        }
        // The cached path and the column scan read the same blocks.
        assert_eq!(store.get_fields_cached(d as u32, &all, &mut cache).is_err(), failed.last() == Some(&d), "doc {d}, cached");
    }
    // One block's documents, and only those.
    assert!(!failed.is_empty() && failed.len() < want.len() / 10, "{} of {} failed", failed.len(), want.len());
    assert!(failed.windows(2).all(|w| w[1] == w[0] + 1), "{failed:?}");
    assert!(store.field_values(1).unwrap_err().to_string().contains("fails its checksum"));
    // The whole section still fails its own checksum.
    assert!(seg.verify_sections().unwrap_err().to_string().contains("section store.data fails its checksum"));

    // Not checked (memtable images), nothing is checked, and nothing panics.
    let seg = SegmentFile::from_vec(image).unwrap();
    let store = DocStore::open(&seg, &schema).unwrap();
    for d in 0..want.len() {
        if let Err(e) = store.get(d as u32) {
            assert!(!e.to_string().contains("checksum"), "doc {d}: {e}");
        }
    }
}

#[test]
fn a_store_with_block_checksums_does_not_check_its_data_whole() {
    let (schema, mut image, want) = blocks_image(500);
    // A wrong crc32 for the whole of store.data, its blocks intact: one
    // digit changed, so the directory keeps its length.
    let at = entry_at(&image, "store.data");
    let crc = at + image[at..].windows(8).position(|w| w == b"\"crc32\":").unwrap() + "\"crc32\":".len();
    image[crc] = if image[crc] == b'1' { b'2' } else { b'1' };
    let seg = SegmentFile::from_vec(image).unwrap().checked_on_read();
    let store = DocStore::open(&seg, &schema).unwrap();
    for (d, doc) in want.iter().enumerate() {
        assert_eq!(&store.get(d as u32).unwrap(), doc, "doc {d}");
    }
    assert!(seg.verify_sections().unwrap_err().to_string().contains("section store.data fails its checksum"));
}

#[test]
fn a_store_without_block_checksums_is_checked_whole() {
    // As a segment written before store.crc: the section renamed, so the
    // reader does not find it (and passes it over).
    let (schema, mut image, want) = blocks_image(500);
    let at = entry_at(&image, "store.crc") + "\"name\":\"store.cr".len();
    image[at] = b'x';
    let seg = SegmentFile::from_vec(image.clone()).unwrap().checked_on_read();
    assert!(!seg.has("store.crc"));
    let store = DocStore::open(&seg, &schema).unwrap();
    for (d, doc) in want.iter().enumerate() {
        assert_eq!(&store.get(d as u32).unwrap(), doc, "doc {d}");
    }
    // A damaged block fails the whole section's checksum, as it did.
    let data = seg.section_range("store.data").unwrap();
    image[data.start + 10] ^= 0x41;
    let seg = SegmentFile::from_vec(image).unwrap().checked_on_read();
    let e = DocStore::open(&seg, &schema).unwrap_err().to_string();
    assert!(e.contains("section store.data fails its checksum"), "{e}");
}


/// The bytes of section `name` of a segment.
fn section(seg: &SegmentFile, name: &str) -> Vec<u8> {
    seg.require(name).unwrap().to_vec()
}

#[test]
fn zstd_blocks_say_so_and_lz4_ones_are_as_before() {
    let docs = random_docs(&mut StdRng::seed_from_u64(9), 2000);
    let lz4 = build(&schema(), &docs, &BuildOptions::default());
    let zstd = build(&schema().with_compression(Compression::Zstd), &docs, &BuildOptions::default());
    // LZ4: version 1, as every version wrote it; zstd: version 2, codec 1.
    let version = |seg: &SegmentFile| u64::from_le_bytes(section(seg, "store.index")[..8].try_into().unwrap());
    assert_eq!(version(&lz4), 1);
    assert_eq!(version(&zstd), 2);
    assert_eq!(u64::from_le_bytes(section(&zstd, "store.index")[8..16].try_into().unwrap()), 1);
    let (a, b) = (DocStore::open(&lz4, &schema()).unwrap(), DocStore::open(&zstd, &schema().with_compression(Compression::Zstd)).unwrap());
    // Larger blocks (32 KiB to LZ4's 16), and fewer bytes.
    assert!(b.n_blocks() < a.n_blocks(), "{} vs {} blocks", b.n_blocks(), a.n_blocks());
    assert!(b.byte_sizes().0 < a.byte_sizes().0, "{:?} vs {:?}", b.byte_sizes(), a.byte_sizes());
    for d in 0..docs.len() as u32 {
        assert_eq!(a.get(d).unwrap(), b.get(d).unwrap(), "doc {d}");
    }
    // A store reads by what its index says, whatever the schema reading it
    // says (a schema changed since keeps reading older segments).
    assert_eq!(DocStore::open(&zstd, &schema()).unwrap().get(5).unwrap(), b.get(5).unwrap());
}

#[test]
fn damaged_zstd_blocks_are_errors_not_panics() {
    // Not checked on read (as memtable images are not), so the damage
    // reaches the decompressor, which must refuse it or decode something:
    // never read out of bounds or panic. (Read from disk, a block's crc32
    // is checked first: `a_damaged_block_fails_its_checksum_and_the_others_read`.)
    let (schema, mut image, _) = blocks_image_of(3000, Compression::Zstd);
    let seg = SegmentFile::from_vec(image.clone()).unwrap();
    let data = seg.section_range("store.data").unwrap();
    let mut rng = StdRng::seed_from_u64(3);
    for _ in 0..200 {
        let at = rng.gen_range(data.clone());
        image[at] = rng.gen();
    }
    let seg = SegmentFile::from_vec(image).unwrap();
    let store = DocStore::open(&seg, &schema).unwrap();
    let failed = (0..3000u32).filter(|&d| store.get(d).is_err()).count();
    assert!(failed > 0);
    assert!(store.field_values(1).is_err());
}
