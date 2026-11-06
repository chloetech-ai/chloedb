//! What a table holds and what its parts weigh ([`table`]): its documents,
//! the bytes on disk of each field's index and of the doc store, the
//! size of each field's values and of each document, how many documents
//! have a value of each field, the distinct terms of each text and keyword
//! field, and the range of each numeric one. For the command center's
//! index inspector (`GET /v1/index`), and for anyone with the file.
//!
//! What is read from every segment is exact: document counts, section
//! bytes, the tokens of each text field's documents (from their one-byte
//! length norms, which round long lengths), numeric ranges (over every row
//! written, deleted rows included, until a merge drops them); a length norm
//! is exact below 24 terms and rounded down, by at most 12.5%, from there. What needs a
//! document's values is read from a uniform sample of the live documents
//! ([`Options::sample`] of them), each standing for as many as its segment
//! has per sampled one: value sizes, fill, document sizes and each field's
//! share of the doc store. Distinct terms are counted with a HyperLogLog
//! sketch over the segments' dictionaries ([`Hll`], within about 2%), which
//! sketches of other shards merge with.
//!
//! The parts merge ([`Dist::merge`], [`Hll::merge`]), so a coordinator
//! adds up what its shards answer.

use std::collections::BTreeMap;
use std::time::Instant;

use rand::{seq::index, SeedableRng};
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::forward::{DocStore, KeywordColumn, NumericColumn, StoreCache};
use crate::schema::{FieldType, Logical, Schema};
use crate::segment::DocId;
use crate::storage::{SegmentView, Snapshot};
use crate::text::reader::TextFieldReader;
use crate::value::Value;
use crate::vector::VectorReader;

/// Documents read for value sizes, by default.
pub const SAMPLE: usize = 2000;
/// The most documents a sample reads.
pub const MAX_SAMPLE: usize = 100_000;
/// Dictionary entries a field's distinct-term sketch reads at most; past
/// this, only the bounds are given.
pub const TERMS_BUDGET: u64 = 8_000_000;
/// Length norms read per segment at most for a text field's tokens; a
/// larger segment's are read at a stride.
pub const NORMS_BUDGET: usize = 2_000_000;

/// What [`table`] reads.
#[derive(Clone, Debug)]
pub struct Options {
    /// Live documents to read the values of (at most [`MAX_SAMPLE`]).
    pub sample: usize,
    /// Seed of the sample: the same seed on the same version reads the
    /// same documents.
    pub seed: u64,
    /// Sketch each text and keyword field's distinct terms.
    pub terms: bool,
}

impl Default for Options {
    fn default() -> Options {
        Options { sample: SAMPLE, seed: 0x5eed, terms: true }
    }
}

/// A table's statistics ([`table`]).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TableStats {
    pub table: String,
    /// The primary key's field.
    pub primary_key: Option<String>,
    /// The version read.
    pub txn: u64,
    /// Its sequence number, in a database that numbers its writes.
    pub seq_no: Option<u64>,
    pub docs: DocCounts,
    pub bytes: ByteCounts,
    /// Each segment, on disk then in memory.
    pub segments: Vec<SegmentStat>,
    pub fields: Vec<FieldStats>,
    /// The bytes of a document's stored values, as the doc store encodes
    /// them before compression (sampled).
    pub doc_bytes: Dist,
    pub sample: SampleInfo,
    /// How long the statistics took.
    pub took_ms: f64,
}

impl TableStats {
    /// Adds another shard's statistics of the same table: counts and bytes
    /// add up, distributions and sketches merge, fields by name. The
    /// version read becomes the newest of the two.
    pub fn merge(&mut self, o: &TableStats) {
        if self.table.is_empty() {
            *self = o.clone();
            return;
        }
        self.txn = self.txn.max(o.txn);
        self.seq_no = self.seq_no.max(o.seq_no);
        self.docs.live += o.docs.live;
        self.docs.deleted += o.docs.deleted;
        self.docs.in_memory += o.docs.in_memory;
        let (a, b) = (&mut self.bytes, &o.bytes);
        a.total += b.total;
        a.on_disk += b.on_disk;
        a.in_memory += b.in_memory;
        a.fields += b.fields;
        a.store += b.store;
        a.store_raw += b.store_raw;
        a.overhead += b.overhead;
        for (k, v) in &b.other {
            *a.other.entry(k.clone()).or_default() += v;
        }
        self.segments.extend(o.segments.iter().cloned());
        for of in &o.fields {
            let Some(f) = self.fields.iter_mut().find(|f| f.name == of.name) else {
                self.fields.push(of.clone());
                continue;
            };
            f.index_bytes += of.index_bytes;
            for (k, v) in &of.sections {
                *f.sections.entry(k.clone()).or_default() += v;
            }
            f.store_bytes += of.store_bytes;
            f.present += of.present;
            f.size.merge(&of.size);
            merge_opt(&mut f.items, &of.items, Dist::merge);
            merge_opt(&mut f.tokens, &of.tokens, Dist::merge);
            merge_opt(&mut f.terms, &of.terms, Terms::merge);
            f.min = min_opt(f.min, of.min, f64::min);
            f.max = min_opt(f.max, of.max, f64::max);
        }
        self.doc_bytes.merge(&o.doc_bytes);
        self.sample.docs += o.sample.docs;
        self.sample.of += o.sample.of;
        self.took_ms = self.took_ms.max(o.took_ms);
    }

    /// Fills each distribution's mean and percentiles; drops the
    /// distinct-term sketches unless `sketches`.
    pub fn finish(&mut self, sketches: bool) {
        self.doc_bytes.finish();
        for f in &mut self.fields {
            f.size.finish();
            for d in [&mut f.items, &mut f.tokens].into_iter().flatten() {
                d.finish();
            }
            if let (Some(t), false) = (&mut f.terms, sketches) {
                t.sketch = None;
            }
        }
    }
}

fn merge_opt<T: Clone>(a: &mut Option<T>, b: &Option<T>, f: impl FnOnce(&mut T, &T)) {
    match (a.as_mut(), b) {
        (Some(x), Some(y)) => f(x, y),
        (None, Some(y)) => *a = Some(y.clone()),
        _ => {}
    }
}

fn min_opt(a: Option<f64>, b: Option<f64>, f: impl Fn(f64, f64) -> f64) -> Option<f64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(f(x, y)),
        (x, y) => x.or(y),
    }
}

/// Documents of a table.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DocCounts {
    /// Live documents.
    pub live: u64,
    /// Deleted documents still in segments (until a merge drops them).
    pub deleted: u64,
    /// Live documents in memory, not yet in an on-disk segment.
    pub in_memory: u64,
}

/// Bytes of a table's segments.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ByteCounts {
    /// Every segment's image, on disk and in memory.
    pub total: u64,
    pub on_disk: u64,
    pub in_memory: u64,
    /// The sections of fields' indexes (the sum of [`FieldStats::index_bytes`]).
    pub fields: u64,
    /// The doc store (`store.*`), compressed as it is on disk.
    pub store: u64,
    /// The doc store's blocks before compression.
    pub store_raw: u64,
    /// The other sections, by name: the primary key index (`pk`), the
    /// field ids (`fields`), the rows' sequence numbers, ...
    pub other: BTreeMap<String, u64>,
    /// Segment headers, directories and alignment.
    pub overhead: u64,
}

/// One segment.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SegmentStat {
    pub id: u64,
    pub docs: u64,
    pub live: u64,
    pub bytes: u64,
    pub in_memory: bool,
}

/// What [`table`] read for its sampled statistics.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SampleInfo {
    /// Documents read.
    pub docs: u64,
    /// Live documents they were drawn from.
    pub of: u64,
}

/// One field.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FieldStats {
    pub name: String,
    /// The field's id in its table.
    pub id: u32,
    /// `text`, `keyword`, `int`, `float`, `bool`, `timestamp`, `vector`,
    /// `edges`, `sparse` or `json`.
    pub kind: String,
    /// A vector field's dimension.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dim: Option<usize>,
    pub stored: bool,
    pub indexed: bool,
    pub multi: bool,
    /// The table's primary key.
    pub key: bool,
    /// Bytes of the field's index sections, in every segment.
    pub index_bytes: u64,
    /// Those bytes by part (`text.post`, `kw.dict`, `col`, `vec.ivf`, ...).
    pub sections: BTreeMap<String, u64>,
    /// The field's share of the doc store on disk: the compressed store's
    /// bytes in proportion to the field's raw bytes in the sampled rows.
    pub store_bytes: f64,
    /// Documents with a value (estimated from the sample, or counted from
    /// the length norms of a text field that is not stored).
    pub present: f64,
    /// Where `present` comes from: `sample` or `norms`.
    pub present_from: String,
    /// The bytes of a document's value: a string's UTF-8 bytes, 8 for a
    /// number, 1 for a bool, 4 per dimension for a vector, the sum of a
    /// list's (sampled; documents without a value are not counted).
    pub size: Dist,
    /// Values per document of a multi-valued field (sampled).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub items: Option<Dist>,
    /// Analysed terms per document of a text field, from the length
    /// norms of every live document (lengths of 24 and more rounded down,
    /// by at most 12.5%).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<Dist>,
    /// Distinct terms or values of a text or keyword field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terms: Option<Terms>,
    /// Smallest and largest value of a numeric field (microseconds since
    /// 1970 for a timestamp), over the rows written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
}

/// A field's distinct terms or values: at least the most any one segment
/// has, at most the sum over segments, and the sketch's estimate when the
/// dictionaries fit [`TERMS_BUDGET`].
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Terms {
    pub low: u64,
    pub high: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimate: Option<f64>,
    /// The sketch, for a coordinator to merge with other shards'.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sketch: Option<Hll>,
}

impl Terms {
    /// Adds another shard's terms: the sketches merge, the bounds become
    /// the larger low and the summed high.
    pub fn merge(&mut self, o: &Terms) {
        self.low = self.low.max(o.low);
        self.high += o.high;
        self.sketch = match (self.sketch.take(), &o.sketch) {
            (Some(mut a), Some(b)) => {
                a.merge(b);
                Some(a)
            }
            _ => None,
        };
        self.estimate = self.sketch.as_ref().map(|s| s.estimate().clamp(self.low as f64, self.high as f64));
    }
}

// ---------------------------------------------------------- distributions

/// A distribution of whole numbers (sizes, counts) as a log-linear
/// histogram: one bucket per value below 16, then four per power of two,
/// so each bucket spans at most a quarter of its lower bound. Each value
/// counts with a weight (the documents a sampled one stands for).
/// Distributions merge by adding their buckets.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Dist {
    /// The weight of the values: the documents they stand for.
    pub n: f64,
    /// Values counted.
    pub sampled: u64,
    /// Weighted sum of the values.
    pub sum: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<u64>,
    /// `[lo, hi, weight]`: the weight of the values in `lo..hi`, by `lo`.
    pub buckets: Vec<(u64, u64, f64)>,
    /// Filled by [`Dist::finish`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mean: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p50: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p90: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p99: Option<f64>,
}

/// The bucket of `v`: its bounds `lo..hi`.
pub fn bucket(v: u64) -> (u64, u64) {
    if v < 16 {
        return (v, v + 1);
    }
    let e = 63 - v.leading_zeros();
    let shift = e - 2;
    let lo = (v >> shift) << shift;
    (lo, lo.saturating_add(1 << shift))
}

impl Dist {
    /// Counts `v` with weight `w`.
    pub fn add(&mut self, v: u64, w: f64) {
        self.n += w;
        self.sampled += 1;
        self.sum += v as f64 * w;
        self.min = Some(self.min.map_or(v, |m| m.min(v)));
        self.max = Some(self.max.map_or(v, |m| m.max(v)));
        let (lo, hi) = bucket(v);
        match self.buckets.binary_search_by_key(&lo, |b| b.0) {
            Ok(i) => self.buckets[i].2 += w,
            Err(i) => self.buckets.insert(i, (lo, hi, w)),
        }
    }

    /// Adds `o`'s values.
    pub fn merge(&mut self, o: &Dist) {
        self.n += o.n;
        self.sampled += o.sampled;
        self.sum += o.sum;
        self.min = match (self.min, o.min) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        self.max = match (self.max, o.max) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        for &(lo, hi, w) in &o.buckets {
            match self.buckets.binary_search_by_key(&lo, |b| b.0) {
                Ok(i) => self.buckets[i].2 += w,
                Err(i) => self.buckets.insert(i, (lo, hi, w)),
            }
        }
    }

    /// Scales every weight by `f` (a sample standing for `f` times more).
    pub fn scale(&mut self, f: f64) {
        self.n *= f;
        self.sum *= f;
        for b in &mut self.buckets {
            b.2 *= f;
        }
    }

    /// The value below which `q` of the weight lies, interpolated within
    /// its bucket.
    pub fn quantile(&self, q: f64) -> Option<f64> {
        if self.n <= 0.0 {
            return None;
        }
        let target = q.clamp(0.0, 1.0) * self.n;
        let mut seen = 0.0;
        for &(lo, hi, w) in &self.buckets {
            if seen + w >= target && w > 0.0 {
                let v = lo as f64 + (hi - lo) as f64 * ((target - seen) / w);
                // Within a bucket of one value, that value.
                let v = if hi - lo == 1 { lo as f64 } else { v };
                return Some(v.clamp(self.min.unwrap_or(lo) as f64, self.max.unwrap_or(hi) as f64));
            }
            seen += w;
        }
        self.max.map(|m| m as f64)
    }

    /// Fills the mean and the percentiles.
    pub fn finish(&mut self) {
        self.mean = (self.n > 0.0).then(|| self.sum / self.n);
        self.p50 = self.quantile(0.5);
        self.p90 = self.quantile(0.9);
        self.p99 = self.quantile(0.99);
    }

    fn finished(mut self) -> Dist {
        self.finish();
        self
    }
}

// --------------------------------------------------------------- sketches

/// A HyperLogLog sketch of distinct byte strings: 4096 one-byte registers,
/// a standard error of about 1.6%. Its hash is fixed ([`hash64`]), so
/// sketches made by any build merge.
#[derive(Clone, Debug, PartialEq)]
pub struct Hll {
    regs: Vec<u8>,
}

const HLL_P: u32 = 12;

impl Default for Hll {
    fn default() -> Hll {
        Hll { regs: vec![0; 1 << HLL_P] }
    }
}

/// FNV-1a of `b`, mixed by SplitMix64's finalizer: a hash that never
/// changes between builds or machines.
pub fn hash64(b: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &x in b {
        h ^= x as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    mix64(h)
}

/// SplitMix64's finalizer.
pub fn mix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    z ^ (z >> 31)
}

impl Hll {
    pub fn add(&mut self, b: &[u8]) {
        self.add_hash(hash64(b));
    }

    pub fn add_hash(&mut self, h: u64) {
        let i = (h >> (64 - HLL_P)) as usize;
        let rank = ((h << HLL_P) | (1 << (HLL_P - 1))).leading_zeros() as u8 + 1;
        if self.regs[i] < rank {
            self.regs[i] = rank;
        }
    }

    pub fn merge(&mut self, o: &Hll) {
        for (a, &b) in self.regs.iter_mut().zip(&o.regs) {
            *a = (*a).max(b);
        }
    }

    /// The estimated count of distinct strings added.
    pub fn estimate(&self) -> f64 {
        let m = self.regs.len() as f64;
        let alpha = 0.7213 / (1.0 + 1.079 / m);
        let sum: f64 = self.regs.iter().map(|&r| 2f64.powi(-(r as i32))).sum();
        let e = alpha * m * m / sum;
        let zeros = self.regs.iter().filter(|&&r| r == 0).count();
        if e <= 2.5 * m && zeros > 0 {
            m * (m / zeros as f64).ln()
        } else {
            e
        }
    }
}

impl Serialize for Hll {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        use base64::Engine;
        s.serialize_str(&base64::engine::general_purpose::STANDARD_NO_PAD.encode(&self.regs))
    }
}

impl<'de> Deserialize<'de> for Hll {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Hll, D::Error> {
        use base64::Engine;
        let s = String::deserialize(d)?;
        let regs = base64::engine::general_purpose::STANDARD_NO_PAD.decode(s.trim_end_matches('=')).map_err(serde::de::Error::custom)?;
        if regs.len() != 1 << HLL_P {
            return Err(serde::de::Error::custom(format!("a sketch has {} registers, not {}", 1 << HLL_P, regs.len())));
        }
        Ok(Hll { regs })
    }
}

// ------------------------------------------------------------------ table

/// The kind of field `ty` is, as [`FieldStats::kind`] names it.
pub fn kind_name(ty: &FieldType, logical: Option<Logical>) -> &'static str {
    match (ty, logical) {
        (FieldType::Int, Some(Logical::Bool)) => "bool",
        (FieldType::Int, Some(Logical::Timestamp)) => "timestamp",
        (FieldType::Text { .. }, _) => "text",
        (FieldType::Keyword, _) => "keyword",
        (FieldType::Int, _) => "int",
        (FieldType::Float, _) => "float",
        (FieldType::Vector { .. }, _) => "vector",
        (FieldType::Edges, _) => "edges",
        (FieldType::Sparse, _) => "sparse",
        (FieldType::Json, _) => "json",
    }
}

/// The bytes of value `v` ([`FieldStats::size`]) and, for a list, its
/// items; `None` for no value.
pub fn value_bytes(v: &Value) -> Option<(u64, Option<u64>)> {
    Some(match v {
        Value::Null => return None,
        Value::Int(_) | Value::Float(_) | Value::Timestamp(_) => (8, None),
        Value::Bool(_) => (1, None),
        Value::Str(s) => (s.len() as u64, None),
        Value::Vector(x) => (x.len() as u64 * 4, None),
        Value::List(xs) => (xs.iter().filter_map(|x| value_bytes(x).map(|b| b.0)).sum(), Some(xs.len() as u64)),
    })
}

/// The field a section belongs to, as its position in `schema`, and the
/// section's part (`text.post`, `kw.dict`, `col.hist`, `vec.ivf`): the
/// name's component and its last part. `None` for a section of no field
/// (the doc store, the primary key index, ...).
fn section_field(schema: &Schema, name: &str) -> Option<(usize, String)> {
    let mut parts = name.split('.');
    let comp = parts.next()?;
    let key = parts.next()?;
    let pos = match key.strip_prefix('f').and_then(|id| id.parse::<u32>().ok()).filter(|_| !schema.is_legacy()) {
        Some(id) => schema.position_of(id)?,
        None => schema.fields.iter().position(|f| f.name == key)?,
    };
    let part = match parts.last() {
        Some(last) => format!("{comp}.{last}"),
        None => comp.to_string(),
    };
    Some((pos, part))
}

/// The statistics of the table `snap` looks at ([`TableStats`]).
pub fn table(snap: &Snapshot, opts: &Options) -> Result<TableStats> {
    let t0 = Instant::now();
    let schema = snap.schema();
    let segs = snap.segments();
    let mut out = TableStats {
        table: schema.table.clone(),
        primary_key: schema.primary_key.map(|k| schema.fields[k].name.clone()),
        txn: snap.txn(),
        seq_no: snap.seq_no(),
        ..Default::default()
    };
    let mut fields: Vec<FieldStats> = schema
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| FieldStats {
            name: f.name.clone(),
            id: f.id,
            kind: kind_name(&f.ty, f.logical).into(),
            dim: match f.ty {
                FieldType::Vector { dim, .. } => Some(dim),
                _ => None,
            },
            stored: f.stored,
            indexed: f.indexed,
            multi: f.multi,
            key: schema.primary_key == Some(i),
            present_from: "sample".into(),
            ..Default::default()
        })
        .collect();

    // Documents and bytes, from every segment.
    for s in segs {
        let docs = s.file.n_docs() as u64;
        let bytes = s.file.image().len() as u64;
        out.docs.live += s.n_live as u64;
        out.docs.deleted += docs - s.n_live as u64;
        out.bytes.total += bytes;
        if s.in_memory {
            out.docs.in_memory += s.n_live as u64;
            out.bytes.in_memory += bytes;
        } else {
            out.bytes.on_disk += bytes;
        }
        out.segments.push(SegmentStat { id: s.id, docs, live: s.n_live as u64, bytes, in_memory: s.in_memory });
        let mut in_sections = 0;
        for (name, len) in s.file.section_sizes().into_iter().chain(s.file.overlay_sizes()) {
            let len = len as u64;
            in_sections += len;
            match section_field(schema, &name) {
                Some((i, part)) => {
                    fields[i].index_bytes += len;
                    *fields[i].sections.entry(part).or_default() += len;
                    out.bytes.fields += len;
                }
                None if name.starts_with("store.") => out.bytes.store += len,
                None => *out.bytes.other.entry(name).or_default() += len,
            }
        }
        out.bytes.overhead += bytes.saturating_sub(in_sections);
        if s.file.has("store.index") {
            if let Ok(store) = DocStore::open(&s.file, schema) {
                out.bytes.store_raw += store.byte_sizes().1 as u64;
            }
        }
    }

    // Each text field's tokens per document, from every live document's
    // length norm; and the documents with any, for one not stored.
    for (i, f) in schema.fields.iter().enumerate() {
        if !matches!(f.ty, FieldType::Text { .. }) || !f.indexed {
            continue;
        }
        let key = f.key();
        let mut tokens = Dist::default();
        let mut with = 0.0;
        for s in segs {
            if !s.file.has(&format!("text.{key}.norms")) {
                continue;
            }
            let r = TextFieldReader::open(&s.file, &key)?;
            let n = s.file.n_docs();
            let step = n.div_ceil(NORMS_BUDGET).max(1);
            let mut seg = Dist::default();
            let mut d = 0;
            while d < n {
                let doc = d as DocId;
                if s.live.as_ref().is_none_or(|l| l.contains(doc)) {
                    let len = r.doc_len(doc) as u64;
                    if len > 0 {
                        seg.add(len, step as f64);
                        with += step as f64;
                    }
                }
                d += step;
            }
            tokens.merge(&seg);
        }
        fields[i].tokens = Some(tokens.finished());
        if !f.stored {
            fields[i].present = with;
            fields[i].present_from = "norms".into();
        }
    }

    // Distinct terms and numeric ranges, from every segment's sections.
    for (i, f) in schema.fields.iter().enumerate() {
        let key = f.key();
        match f.ty {
            FieldType::Text { .. } | FieldType::Sparse | FieldType::Keyword => {
                let text = !matches!(f.ty, FieldType::Keyword);
                let dict = if text { format!("text.{key}.dict") } else { format!("kw.{key}.dict") };
                let mut terms = Terms::default();
                let mut readers = Vec::new();
                for s in segs.iter().filter(|s| s.file.has(&dict)) {
                    let n = if text {
                        let r = TextFieldReader::open(&s.file, &key)?;
                        let n = r.n_terms() as u64;
                        readers.push(Either::Text(r));
                        n
                    } else {
                        let r = KeywordColumn::open(&s.file, &key)?;
                        let n = r.n_values() as u64;
                        readers.push(Either::Keyword(r));
                        n
                    };
                    terms.low = terms.low.max(n);
                    terms.high += n;
                }
                if readers.is_empty() {
                    continue;
                }
                if opts.terms && terms.high <= TERMS_BUDGET {
                    let mut h = Hll::default();
                    for r in &readers {
                        match r {
                            Either::Text(r) => r.for_each_term(|t, _| h.add(t)),
                            Either::Keyword(r) => (0..r.n_values() as u32).for_each(|c| h.add(r.value(c).as_bytes())),
                        }
                    }
                    terms.estimate = Some(h.estimate().clamp(terms.low as f64, terms.high as f64));
                    terms.sketch = Some(h);
                }
                fields[i].terms = Some(terms);
            }
            FieldType::Int | FieldType::Float if !f.multi => {
                for s in segs.iter().filter(|s| s.file.has(&format!("col.{key}"))) {
                    let c = NumericColumn::open(&s.file, &key)?;
                    if let (Some(lo), Some(hi)) = (c.min(), c.max()) {
                        let fs = &mut fields[i];
                        fs.min = Some(fs.min.map_or(lo, |m| m.min(lo)));
                        fs.max = Some(fs.max.map_or(hi, |m| m.max(hi)));
                    }
                }
            }
            _ => {}
        }
    }

    // The sample: values, fill, sizes.
    sample(snap, opts, &mut out, &mut fields)?;
    for f in &mut fields {
        f.size.finish();
        if let Some(d) = &mut f.items {
            d.finish();
        }
    }
    out.doc_bytes.finish();
    out.fields = fields;
    out.took_ms = t0.elapsed().as_secs_f64() * 1e3;
    Ok(out)
}

enum Either {
    Text(TextFieldReader),
    Keyword(KeywordColumn),
}

/// Reads a uniform sample of the live documents into `out.doc_bytes` and
/// each field's size, items, fill and doc store share.
fn sample(snap: &Snapshot, opts: &Options, out: &mut TableStats, fields: &mut [FieldStats]) -> Result<()> {
    let schema = snap.schema();
    let segs = snap.segments();
    let live: u64 = segs.iter().map(|s| s.n_live as u64).sum();
    let want = opts.sample.min(MAX_SAMPLE) as u64;
    out.sample.of = live;
    if live == 0 || want == 0 {
        return Ok(());
    }
    let stored: Vec<usize> = schema.fields.iter().enumerate().filter(|(_, f)| f.stored && !matches!(f.ty, FieldType::Vector { .. })).map(|(i, _)| i).collect();
    let vectors: Vec<usize> = schema.fields.iter().enumerate().filter(|(_, f)| matches!(f.ty, FieldType::Vector { .. })).map(|(i, _)| i).collect();
    let mut rng = rand::rngs::StdRng::seed_from_u64(opts.seed);
    // Raw bytes of each field and of whole rows in the sample, weighted,
    // for each field's share of the store.
    let mut field_raw = vec![0.0f64; fields.len()];
    let mut row_raw = 0.0f64;
    let mut present = vec![0.0f64; fields.len()];
    for s in segs {
        if s.n_live == 0 {
            continue;
        }
        let n = s.file.n_docs();
        // This segment's share of the sample, drawn from all its documents
        // in proportion, the deleted ones then left out.
        let share = ((want as f64 * s.n_live as f64 / live as f64).round() as usize).max(1);
        let draw = if share as u64 >= s.n_live as u64 { n } else { (share as f64 * n as f64 / s.n_live as f64).ceil() as usize }.min(n);
        let mut docs: Vec<DocId> = if draw >= n { (0..n as DocId).collect() } else { index::sample(&mut rng, n, draw).into_iter().map(|d| d as DocId).collect() };
        docs.sort_unstable();
        docs.retain(|&d| is_live(s, d));
        if docs.is_empty() {
            continue;
        }
        let w = s.n_live as f64 / docs.len() as f64;
        let store = if !stored.is_empty() && s.file.has("store.index") { Some(DocStore::open(&s.file, schema)?) } else { None };
        let vecs: Vec<(usize, Option<VectorReader>)> = vectors.iter().map(|&i| (i, VectorReader::open(&s.file, &schema.fields[i].key()).ok())).collect();
        let mut cache = StoreCache::default();
        for &d in &docs {
            out.sample.docs += 1;
            if let Some(store) = &store {
                let values = store.get_fields_cached(d, &stored, &mut cache)?;
                let len = store.row_len_cached(d, &mut cache)? as u64;
                out.doc_bytes.add(len, w);
                row_raw += len as f64 * w;
                for (&i, v) in stored.iter().zip(&values) {
                    if let Some((bytes, items)) = value_bytes(v) {
                        present[i] += w;
                        field_raw[i] += bytes as f64 * w;
                        fields[i].size.add(bytes, w);
                        if let Some(k) = items.filter(|_| fields[i].multi) {
                            fields[i].items.get_or_insert_with(Dist::default).add(k, w);
                        }
                    }
                }
            }
            for (i, r) in &vecs {
                let Some(r) = r else { continue };
                if r.has_vector(d) {
                    present[*i] += w;
                    if !fields[*i].multi {
                        fields[*i].size.add(fields[*i].dim.unwrap_or(0) as u64 * 4, w);
                    }
                }
            }
        }
    }
    // A value's size is counted as written (an integer as 8 bytes), which
    // can be more than its encoding: the shares never add up to more than
    // the store.
    let whole = row_raw.max(field_raw.iter().sum());
    for (i, f) in fields.iter_mut().enumerate() {
        if f.present_from == "sample" {
            f.present = present[i];
        }
        if whole > 0.0 {
            f.store_bytes = out.bytes.store as f64 * field_raw[i] / whole;
        }
    }
    Ok(())
}

fn is_live(s: &SegmentView, d: DocId) -> bool {
    s.live.as_ref().is_none_or(|l| l.contains(d))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_cover_every_value_once() {
        let mut prev = 0;
        for v in 0..100_000u64 {
            let (lo, hi) = bucket(v);
            assert!(lo <= v && v < hi, "{v} in {lo}..{hi}");
            assert!(lo >= prev);
            prev = lo;
            if v >= 16 {
                assert!((hi - lo) * 4 <= lo, "{v}: {lo}..{hi} is wider than a quarter of {lo}");
            }
        }
        // The last bucket ends at the largest value rather than past it.
        assert_eq!(bucket(u64::MAX), (7 << 61, u64::MAX));
    }

    #[test]
    fn quantiles_of_a_uniform_run() {
        let mut d = Dist::default();
        for v in 1..=1000u64 {
            d.add(v, 1.0);
        }
        d.finish();
        let p50 = d.p50.unwrap();
        assert!((p50 - 500.0).abs() < 40.0, "{p50}");
        assert!((d.p99.unwrap() - 990.0).abs() < 40.0, "{:?}", d.p99);
        assert_eq!(d.mean, Some(500.5));
        assert_eq!((d.min, d.max), (Some(1), Some(1000)));
    }

    #[test]
    fn dists_merge_as_if_added_together() {
        let (mut a, mut b, mut both) = (Dist::default(), Dist::default(), Dist::default());
        for v in 0..500u64 {
            a.add(v * 3, 2.0);
            both.add(v * 3, 2.0);
        }
        for v in 0..700u64 {
            b.add(v * 7 + 1, 1.0);
            both.add(v * 7 + 1, 1.0);
        }
        a.merge(&b);
        assert_eq!(a, both);
        let j = serde_json::to_string(&a).unwrap();
        let back: Dist = serde_json::from_str(&j).unwrap();
        assert_eq!(back, a);
    }

    #[test]
    fn hll_counts_within_a_few_percent_and_merges() {
        let (mut a, mut b) = (Hll::default(), Hll::default());
        for i in 0..60_000u32 {
            a.add(format!("term{i}").as_bytes());
        }
        for i in 40_000..100_000u32 {
            b.add(format!("term{i}").as_bytes());
        }
        let ea = a.estimate();
        assert!((ea / 60_000.0 - 1.0).abs() < 0.05, "{ea}");
        a.merge(&b);
        let e = a.estimate();
        assert!((e / 100_000.0 - 1.0).abs() < 0.05, "{e}");
        let mut small = Hll::default();
        for i in 0..100u32 {
            small.add(&i.to_le_bytes());
        }
        assert!((small.estimate() - 100.0).abs() < 3.0, "{}", small.estimate());
        let j = serde_json::to_string(&a).unwrap();
        assert_eq!(serde_json::from_str::<Hll>(&j).unwrap(), a);
    }
}
