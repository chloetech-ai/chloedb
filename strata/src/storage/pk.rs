//! Primary-key index: section `pk` of every segment of a table with a
//! primary key.
//!
//! ```text
//! kind: u64 | n: u64 | dir_bits: u64
//! dir:     [u32; 2^dir_bits + 1]  first entry whose hash has each top-bit prefix
//! entries: [u32; 2n]              (fingerprint, docID) pairs, sorted by hash prefix, fingerprint, docID
//! kind 3, KEYWORD keys in the key field's keyword column:
//!          field [u8]             the field's section key: its column is `kw.<field>.*`
//! kind 4, KEYWORD keys here:
//!          offsets [u64; n + 1], bytes [u8]   the key of each document, in docID order
//! kind 5, INT:
//!          keys [i64; n]
//! ```
//!
//! Key hashes are uniform, so a radix directory over their top bits (about
//! four entries per slot) turns a lookup into one directory read and a short
//! scan: a cold lookup touches a constant number of pages (directory, entry,
//! stored key) where a binary search over the entries touches about
//! `log2(n) - 9`. An entry keeps the low 32 bits of its key's hash (its
//! fingerprint), so that with the directory's prefix it holds `32 +
//! dir_bits` bits of the hash: about one probe in a billion of a missing
//! key matches one, and matches are confirmed by comparing the key.
//! (fingerprint, docID) pairs are interleaved so that a probe reads one
//! cache line.
//!
//! A KEYWORD key field that is indexed, as one is unless its options say
//! `indexed = false`, has a keyword column ([`KeywordColumn`]), whose
//! sorted dictionary holds each key once and whose code column gives each
//! document's: the index reads the keys there (kind 3) rather than keeping
//! a copy of its own, which took as many bytes again as the keys (8 more a
//! key for its offset). A key field without a column keeps its keys here
//! (kinds 4 and 5).
//!
//! Databases written before field ids keep the layout the versions before
//! wrote, so that those versions still open them: kinds 1 (KEYWORD, then as
//! kind 4) and 2 (INT, then as kind 5), whose entries are `[u64; 2n]` of
//! (hash, docID). Since the integer hash is a bijection, an INT key with an
//! equal 64-bit hash needs no comparison there.
//!
//! A segment of more than [`BLOOM_MIN_KEYS`] keys also gets a Bloom filter
//! of its key hashes, in memory only: built from the section's entries on
//! its first lookup (nothing on disk changes), it answers most lookups of
//! a key the segment lacks from one cache line of memory, without reading
//! the directory or the entries, which may be pages not in memory. A read
//! by key probes every segment newest first until one holds the key, so
//! most probes are misses.

use crate::postings::bytes::{ByteReader, ByteWriter, OwnedBytes, Slice};

use crate::error::{corrupt, Error, Result};
use crate::forward::KeywordColumn;
use crate::schema::{FieldType, Schema};
use crate::segment::{DocId, SegmentFile};
use crate::value::{Document, Value};

/// Section name of the primary-key index.
pub const SECTION: &str = "pk";

/// The layouts of the versions before fingerprints, kept for databases
/// written before field ids.
const KIND_STR_WIDE: u64 = 1;
const KIND_INT_WIDE: u64 = 2;
const KIND_COLUMN: u64 = 3;
const KIND_STR: u64 = 4;
const KIND_INT: u64 = 5;

/// A primary-key value, borrowed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyRef<'a> {
    Int(i64),
    Str(&'a [u8]),
}

impl<'a> KeyRef<'a> {
    /// The key of a value, if it can be one (KEYWORD or INT).
    pub fn of(v: &'a Value) -> Option<KeyRef<'a>> {
        match v {
            Value::Int(x) => Some(KeyRef::Int(*x)),
            Value::Str(s) => Some(KeyRef::Str(s.as_bytes())),
            _ => None,
        }
    }
}

/// The 64-bit hash of a key. It is part of the file format: changing it
/// invalidates every stored `pk` section.
pub fn key_hash(k: KeyRef) -> u64 {
    match k {
        KeyRef::Int(v) => hash_int(v),
        KeyRef::Str(s) => hash_bytes(s),
    }
}

/// Hash function type, overridable in tests to force collisions.
pub(crate) type HashFn = fn(KeyRef) -> u64;

const P0: u64 = 0xa076_1d64_78bd_642f;
const P1: u64 = 0xe703_7ed1_a0b4_28db;
const P2: u64 = 0x8ebc_6af0_9c88_c6e3;

#[inline]
fn mum(a: u64, b: u64) -> u64 {
    let r = (a as u128) * (b as u128);
    (r as u64) ^ ((r >> 64) as u64)
}

/// Multiply-fold hash over 16-byte blocks (in the style of wyhash).
fn hash_bytes(s: &[u8]) -> u64 {
    let mut h = P0 ^ (s.len() as u64).wrapping_mul(P2);
    let mut blocks = s.chunks_exact(16);
    for c in &mut blocks {
        let a = u64::from_le_bytes(c[..8].try_into().unwrap());
        let b = u64::from_le_bytes(c[8..].try_into().unwrap());
        h = mum(a ^ P1, b ^ h);
    }
    let rest = blocks.remainder();
    let mut tail = [0u8; 16];
    tail[..rest.len()].copy_from_slice(rest);
    let a = u64::from_le_bytes(tail[..8].try_into().unwrap());
    let b = u64::from_le_bytes(tail[8..].try_into().unwrap());
    h = mum(a ^ P1, b ^ h);
    mum(h ^ P2, (s.len() as u64) ^ P1)
}

/// The splitmix64 finaliser: a bijection, so distinct integers never collide.
fn hash_int(v: i64) -> u64 {
    let mut z = (v as u64).wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Directory size: about four entries per slot, at most 2^24 slots.
fn dir_bits(n: usize) -> u32 {
    if n < 8 {
        0
    } else {
        (n.ilog2() - 2).min(24)
    }
}

#[inline]
fn bucket(h: u64, bits: u32) -> usize {
    h.checked_shr(64 - bits).unwrap_or(0) as usize
}

/// How a segment's `pk` section holds its keys.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PkLayout {
    /// As the versions before fingerprints wrote it (kinds 1 and 2), for a
    /// table of a database written before field ids.
    Wide,
    /// Keys in the section (kinds 4 and 5).
    Own,
    /// KEYWORD keys read from the key field's keyword column (kind 3),
    /// named by the field's section key ([`crate::schema::Field::key`]).
    Column(String),
}

impl PkLayout {
    /// The layout of `schema`'s segments' key index (of a table with a
    /// primary key).
    pub fn of(schema: &Schema) -> PkLayout {
        let Some(pk) = schema.primary_key else { return PkLayout::Own };
        let f = &schema.fields[pk];
        if schema.is_legacy() {
            PkLayout::Wide
        } else if f.ty == FieldType::Keyword && f.indexed && f.media.is_none() {
            PkLayout::Column(f.key())
        } else {
            PkLayout::Own
        }
    }
}

/// Writes the primary-key index section of a segment (nothing if the table
/// has no primary key).
pub fn build_pk_index(schema: &Schema, docs: &[Document], out: &mut crate::segment::SegmentWriter) -> Result<()> {
    let Some(pk) = schema.primary_key else { return Ok(()) };
    let mut keys = KeyList::new(&schema.fields[pk].ty)?;
    for d in docs {
        keys.push(&d[pk])?;
    }
    out.put(SECTION, keys.build(key_hash, &PkLayout::of(schema)));
    Ok(())
}

/// [`build_pk_index`] from the documents' keys alone.
pub fn build_pk_index_of(schema: &Schema, keys: &KeyList, out: &mut crate::segment::SegmentWriter) -> Result<()> {
    if schema.primary_key.is_some() {
        out.put(SECTION, keys.build(key_hash, &PkLayout::of(schema)));
    }
    Ok(())
}

/// Primary keys in docID order, packed as the section stores them: what a
/// segment build keeps of its documents' keys, for the key index and EDGES
/// fields, while the documents arrive in batches.
pub struct KeyList {
    int: bool,
    ints: Vec<i64>,
    /// KEYWORD keys: `offsets[d]..offsets[d + 1]` of `bytes`.
    offsets: Vec<u64>,
    bytes: Vec<u8>,
}

impl KeyList {
    /// No keys yet, of a KEYWORD or INT primary key.
    pub fn new(ty: &FieldType) -> Result<KeyList> {
        match ty {
            FieldType::Keyword | FieldType::Int => Ok(KeyList::of(*ty == FieldType::Int)),
            _ => Err(Error::Schema("primary key must be KEYWORD or INT".into())),
        }
    }

    /// No keys yet, INT keys if `int`, else KEYWORD.
    pub fn of(int: bool) -> KeyList {
        KeyList { int, ints: Vec::new(), offsets: if int { Vec::new() } else { vec![0] }, bytes: Vec::new() }
    }

    /// Adds the next document's key.
    pub fn push(&mut self, v: &Value) -> Result<()> {
        match (self.int, v) {
            (false, Value::Str(s)) => {
                self.bytes.extend_from_slice(s.as_bytes());
                self.offsets.push(self.bytes.len() as u64);
            }
            (true, Value::Int(x)) => self.ints.push(*x),
            _ => return Err(Error::Schema(format!("bad primary key value {v:?}"))),
        }
        Ok(())
    }

    /// The keys added.
    pub fn len(&self) -> usize {
        if self.int { self.ints.len() } else { self.offsets.len() - 1 }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Document `d`'s key.
    pub fn get(&self, d: usize) -> KeyRef<'_> {
        if self.int {
            KeyRef::Int(self.ints[d])
        } else {
            KeyRef::Str(&self.bytes[self.offsets[d] as usize..self.offsets[d + 1] as usize])
        }
    }

    /// The `pk` section of these keys, laid out as `layout` says.
    fn build(&self, hash: HashFn, layout: &PkLayout) -> Vec<u8> {
        let n = self.len();
        let bits = dir_bits(n);
        let wide = *layout == PkLayout::Wide;
        let mut pairs: Vec<(u64, u32)> = (0..n).map(|d| (hash(self.get(d)), d as u32)).collect();
        if wide {
            pairs.sort_unstable();
        } else {
            pairs.sort_unstable_by_key(|&(h, d)| (bucket(h, bits), h as u32, d));
        }
        let mut dir = vec![0u32; (1usize << bits) + 1];
        let mut i = 0;
        for (b, slot) in dir.iter_mut().enumerate() {
            while i < n && bucket(pairs[i].0, bits) < b {
                i += 1;
            }
            *slot = i as u32;
        }
        let kind = match (layout, self.int) {
            (PkLayout::Wide, false) => KIND_STR_WIDE,
            (PkLayout::Wide, true) => KIND_INT_WIDE,
            (PkLayout::Column(_), false) => KIND_COLUMN,
            (_, false) => KIND_STR,
            (_, true) => KIND_INT,
        };
        let mut w = ByteWriter::new();
        w.u64(kind);
        w.u64(n as u64);
        w.u64(bits as u64);
        w.array(&dir);
        if wide {
            w.array(&pairs.iter().flat_map(|&(h, d)| [h, d as u64]).collect::<Vec<u64>>());
        } else {
            w.array(&pairs.iter().flat_map(|&(h, d)| [h as u32, d]).collect::<Vec<u32>>());
        }
        match (kind, layout) {
            (KIND_COLUMN, PkLayout::Column(field)) => w.array(field.as_bytes()),
            (KIND_STR_WIDE | KIND_STR, _) => {
                w.array(&self.offsets);
                w.array(&self.bytes);
            }
            _ => w.array(&self.ints),
        }
        w.finish()
    }
}

/// Segments with more keys than this get a Bloom filter ([`Bloom`]);
/// smaller ones, such as the memtable's, are probed in their directory.
pub const BLOOM_MIN_KEYS: usize = 4096;

/// Bits of filter per key: with eight bits set per key, about one probe in
/// a hundred of a missing key passes.
const BLOOM_BITS_PER_KEY: usize = 12;

/// The salts of a split-block Bloom filter (Parquet's): each sets one bit
/// in its 32-bit word of a key's block.
const SALT: [u32; 8] = [0x47b6_137b, 0x4497_4d91, 0x8824_ad5b, 0xa2b7_289d, 0x7054_95c7, 0x2df1_424b, 0x9efc_4947, 0x5c6b_fb31];

/// A split-block Bloom filter of key hashes: 256-bit blocks, a key's block
/// chosen by its hash and eight of the block's bits, one in each word,
/// set for it; a probe reads one block.
pub struct Bloom {
    blocks: Box<[[u32; 8]]>,
}

impl Bloom {
    /// The filter of these hashes, sized for `n` of them.
    pub fn of(hashes: impl Iterator<Item = u64>, n: usize) -> Bloom {
        let mut bloom = Bloom::sized(n);
        hashes.for_each(|h| bloom.add(h));
        bloom
    }

    /// An empty filter sized for `n` hashes.
    fn sized(n: usize) -> Bloom {
        let nb = (n * BLOOM_BITS_PER_KEY / 256 + 1).next_power_of_two();
        Bloom { blocks: vec![[0u32; 8]; nb].into_boxed_slice() }
    }

    #[inline]
    fn add(&mut self, h: u64) {
        let (b, m) = Bloom::at(h, self.blocks.len());
        for (w, m) in self.blocks[b].iter_mut().zip(m) {
            *w |= m;
        }
    }

    #[inline]
    fn at(h: u64, nb: usize) -> (usize, [u32; 8]) {
        // The low bits pick the block (the directory buckets by the top
        // ones); the hash multiplied, its high half salted, the bits.
        // A narrow index's hashes keep only their top and bottom bits,
        // which the multiply spreads over the high half.
        let b = h as usize & (nb - 1);
        let x = (h.wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 32) as u32;
        (b, SALT.map(|s| 1u32 << (x.wrapping_mul(s) >> 27)))
    }

    /// False when no hash given had `h`; true for those, and rarely for
    /// others.
    #[inline]
    pub fn may_contain(&self, h: u64) -> bool {
        let (b, m) = Bloom::at(h, self.blocks.len());
        self.blocks[b].iter().zip(m).all(|(w, m)| w & m == m)
    }

    /// Bytes of memory it holds.
    pub fn bytes(&self) -> usize {
        self.blocks.len() * 32
    }
}

impl std::fmt::Debug for Bloom {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Bloom({} blocks)", self.blocks.len())
    }
}

/// Where an index's keys are.
#[derive(Clone, Debug)]
enum Keys {
    /// In the key field's keyword column (kind 3).
    Column(Box<KeywordColumn>),
    /// KEYWORD keys in the section, in docID order (kinds 1 and 4).
    Bytes { offsets: Slice<u64>, bytes: Slice<u8> },
    /// INT keys in the section, in docID order (kinds 2 and 5).
    Ints(Slice<i64>),
}

/// An index's entries: interleaved (hash, docID) of the wide layout, or
/// (fingerprint, docID).
#[derive(Clone, Debug)]
enum Entries {
    Wide(Slice<u64>),
    Narrow(Slice<u32>),
}

/// Zero-copy reader of a `pk` section.
#[derive(Clone, Debug)]
pub struct PkIndex {
    n: usize,
    bits: u32,
    dir: Slice<u32>,
    entries: Entries,
    keys: Keys,
    hash: HashFn,
    /// `hash` is [`key_hash`], whose integer hash is a bijection, so an
    /// integer key with an equal 64-bit hash needs no comparison.
    bijective_ints: bool,
    /// The filter of the keys' hashes, built on the first lookup of an
    /// index of more than [`BLOOM_MIN_KEYS`] keys, shared by the copies
    /// of the index that snapshots hold.
    bloom: std::sync::Arc<std::sync::OnceLock<Bloom>>,
}

/// The field whose keyword column a `pk` section (`bytes`) reads its keys
/// from (kind 3), if it does: the column's sections are then as much a
/// part of the key index as the section is (a cold segment's cache keeps
/// them, [`crate::storage::cold`]).
pub fn key_column(bytes: OwnedBytes) -> Result<Option<String>> {
    let mut r = ByteReader::new(bytes);
    let kind = r.u64().map_err(corrupt)?;
    if kind != KIND_COLUMN {
        return Ok(None);
    }
    let _n = r.u64().map_err(corrupt)?;
    let _bits = r.u64().map_err(corrupt)?;
    let _dir: Slice<u32> = r.array().map_err(corrupt)?;
    let _entries: Slice<u32> = r.array().map_err(corrupt)?;
    let field: Slice<u8> = r.array().map_err(corrupt)?;
    Ok(Some(std::str::from_utf8(&field).map_err(|_| corrupt("bad pk key field"))?.to_string()))
}

impl PkIndex {
    /// The index of a segment, or `None` if it has no `pk` section.
    pub fn open(seg: &SegmentFile) -> Result<Option<PkIndex>> {
        let Some(bytes) = seg.section(SECTION)? else { return Ok(None) };
        let mut idx = PkIndex::read(bytes, key_hash, Some(seg))?;
        idx.bijective_ints = true;
        Ok(Some(idx))
    }

    /// The index in `bytes`, of segment `seg`, whose keyword column a kind
    /// 3 index reads its keys from.
    pub(crate) fn read(bytes: OwnedBytes, hash: HashFn, seg: Option<&SegmentFile>) -> Result<PkIndex> {
        let mut r = ByteReader::new(bytes);
        let kind = r.u64().map_err(corrupt)?;
        let n = r.u64().map_err(corrupt)? as usize;
        let bits = r.u64().map_err(corrupt)? as u32;
        if !(KIND_STR_WIDE..=KIND_INT).contains(&kind) || bits > 24 {
            return Err(corrupt("bad pk section header"));
        }
        let dir: Slice<u32> = r.array().map_err(corrupt)?;
        let wide = kind <= KIND_INT_WIDE;
        let (entries, n_entries) = if wide {
            let e: Slice<u64> = r.array().map_err(corrupt)?;
            let len = e.len();
            (Entries::Wide(e), len)
        } else {
            let e: Slice<u32> = r.array().map_err(corrupt)?;
            let len = e.len();
            (Entries::Narrow(e), len)
        };
        if dir.len() != (1 << bits) + 1 || n_entries != 2 * n || dir[dir.len() - 1] as usize != n {
            return Err(corrupt("pk section sizes disagree"));
        }
        let keys = match kind {
            KIND_COLUMN => {
                let field: Slice<u8> = r.array().map_err(corrupt)?;
                let field = std::str::from_utf8(&field).map_err(|_| corrupt("bad pk key field"))?;
                let seg = seg.ok_or_else(|| corrupt("a pk index of a keyword column without its segment"))?;
                let col = KeywordColumn::open(seg, field)?;
                if col.len() != n || col.is_multi() {
                    return Err(corrupt("pk key column disagrees"));
                }
                Keys::Column(Box::new(col))
            }
            KIND_STR_WIDE | KIND_STR => {
                let offsets: Slice<u64> = r.array().map_err(corrupt)?;
                let bytes: Slice<u8> = r.array().map_err(corrupt)?;
                if offsets.len() != n + 1 || offsets[n] as usize != bytes.len() {
                    return Err(corrupt("pk key offsets disagree"));
                }
                Keys::Bytes { offsets, bytes }
            }
            _ => {
                let ints: Slice<i64> = r.array().map_err(corrupt)?;
                if ints.len() != n {
                    return Err(corrupt("pk key array size"));
                }
                Keys::Ints(ints)
            }
        };
        Ok(PkIndex { n, bits, dir, entries, keys, hash, bijective_ints: false, bloom: Default::default() })
    }

    /// Number of documents (keys) in the segment.
    pub fn len(&self) -> usize {
        self.n
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// Whether the keys are KEYWORD values.
    fn str_keys(&self) -> bool {
        !matches!(self.keys, Keys::Ints(_))
    }

    /// The document holding `key`. A segment holds each key at most once,
    /// except an in-memory memtable segment, which keeps deleted versions
    /// (use [`lookup_all`](Self::lookup_all) with the live filter there).
    pub fn lookup(&self, key: &Value) -> Option<DocId> {
        self.lookup_all(key).next()
    }

    /// Every document holding `key`, in increasing docID order.
    pub fn lookup_all<'a>(&'a self, key: &'a Value) -> impl Iterator<Item = DocId> + 'a {
        self.lookup_key(KeyRef::of(key))
    }

    /// Every document holding the borrowed key `k`, in increasing docID order.
    pub fn lookup_key<'a>(&'a self, k: Option<KeyRef<'a>>) -> impl Iterator<Item = DocId> + 'a {
        let h = k.map_or(0, |k| (self.hash)(k));
        self.lookup_hashed(k, h)
    }

    /// Like [`lookup_key`](Self::lookup_key) with the key's hash already
    /// computed (`h` must be this index's hash of `k`), so that a lookup over
    /// many segments hashes once.
    pub fn lookup_hashed<'a>(&'a self, k: Option<KeyRef<'a>>, h: u64) -> impl Iterator<Item = DocId> + 'a {
        let k = k.filter(|k| matches!(k, KeyRef::Str(_)) == self.str_keys());
        let range = match k {
            Some(_) if self.bloom().is_some_and(|b| !b.may_contain(self.bloom_hash(h))) => 0..0,
            Some(_) => {
                let b = bucket(h, self.bits);
                self.dir[b] as usize..self.dir[b + 1] as usize
            }
            None => 0..0,
        };
        // The wide layout's integer hash is a bijection: an equal hash is
        // an equal key. A fingerprint is a part of the hash.
        let (probe, verify) = match self.entries {
            Entries::Wide(_) => (h, self.str_keys() || !self.bijective_ints),
            Entries::Narrow(_) => (h as u32 as u64, true),
        };
        range
            .map(move |i| self.entry(i))
            .skip_while(move |&(eh, _)| eh < probe)
            .take_while(move |&(eh, _)| eh == probe)
            .filter(move |&(_, d)| !verify || Some(self.key_ref(d)) == k)
            .map(|(_, d)| d)
    }

    /// Entry `i`: its hash (wide) or fingerprint, and its document.
    #[inline]
    fn entry(&self, i: usize) -> (u64, DocId) {
        match &self.entries {
            Entries::Wide(e) => (e[2 * i], e[2 * i + 1] as DocId),
            Entries::Narrow(e) => (e[2 * i] as u64, e[2 * i + 1]),
        }
    }

    /// What the Bloom filter holds of hash `h`: all of it (wide), or the
    /// bits a narrow entry keeps with its directory slot.
    #[inline]
    fn bloom_hash(&self, h: u64) -> u64 {
        match self.entries {
            Entries::Wide(_) => h,
            Entries::Narrow(_) => (h & !(u64::MAX >> self.bits)) | (h & u32::MAX as u64),
        }
    }

    /// The Bloom filter of the keys, built now if this is its first use;
    /// `None` for an index of at most [`BLOOM_MIN_KEYS`] keys.
    pub fn bloom(&self) -> Option<&Bloom> {
        (self.n > BLOOM_MIN_KEYS).then(|| {
            self.bloom.get_or_init(|| match &self.entries {
                Entries::Wide(e) => Bloom::of((0..self.n).map(|i| e[2 * i]), self.n),
                Entries::Narrow(e) => {
                    // An entry's hash is its slot's prefix and its
                    // fingerprint.
                    let (dir, e): (&[u32], &[u32]) = (&self.dir, e);
                    let mut bloom = Bloom::sized(self.n);
                    for (b, w) in dir.windows(2).enumerate() {
                        let prefix = (b as u64).checked_shl(64 - self.bits).unwrap_or(0);
                        for pair in e[2 * w[0] as usize..2 * w[1] as usize].chunks_exact(2) {
                            bloom.add(prefix | pair[0] as u64);
                        }
                    }
                    bloom
                }
            })
        })
    }

    /// The key of document `doc`.
    pub fn key_ref(&self, doc: DocId) -> KeyRef<'_> {
        let d = doc as usize;
        match &self.keys {
            Keys::Column(c) => KeyRef::Str(if d < self.n { c.value_bytes_of_doc(doc).unwrap_or_default() } else { &[] }),
            Keys::Bytes { offsets, bytes } => KeyRef::Str(&bytes[offsets[d] as usize..offsets[d + 1] as usize]),
            Keys::Ints(ints) => KeyRef::Int(ints[d]),
        }
    }

    /// The key of document `doc` as a value.
    pub fn key(&self, doc: DocId) -> Value {
        match self.key_ref(doc) {
            KeyRef::Int(v) => Value::Int(v),
            KeyRef::Str(b) => Value::Str(String::from_utf8_lossy(b).into_owned()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Field;
    use crate::segment::SegmentWriter;
    use rand::{Rng, SeedableRng};

    /// Encodes the index of `keys` (key of document `i` at position `i`).
    fn build(ty: &FieldType, keys: &[&Value], hash: HashFn, layout: &PkLayout) -> Vec<u8> {
        let mut list = KeyList::new(ty).unwrap();
        for v in keys {
            list.push(v).unwrap();
        }
        list.build(hash, layout)
    }

    /// The index of `keys` in each layout that holds their keys: a
    /// segment of a table keyed by them (whose KEYWORD keys the index
    /// reads from their column), the section alone with its own keys, and
    /// the wide one.
    fn indexes(ty: FieldType, keys: &[Value], hash: HashFn) -> Vec<(&'static str, PkIndex)> {
        let refs: Vec<&Value> = keys.iter().collect();
        let mut out = Vec::new();
        if ty == FieldType::Keyword {
            let schema = Schema::new("t", vec![Field::new("id", FieldType::Keyword)], Some("id")).unwrap();
            assert!(matches!(PkLayout::of(&schema), PkLayout::Column(_)));
            let docs: Vec<Document> = keys.iter().map(|k| vec![k.clone()]).collect();
            let seg = SegmentFile::from_vec(crate::segment::build::build_segment(&schema, &docs, &Default::default()).unwrap()).unwrap();
            let bytes = seg.section(SECTION).unwrap().unwrap();
            assert_eq!(ByteReader::new(bytes.clone()).u64().unwrap(), KIND_COLUMN);
            // The section without the column's keys.
            assert_eq!(bytes.len(), 8 * 5 + 4 * ((1 << dir_bits(keys.len())) + 1usize).next_multiple_of(2) + 8 * keys.len() + 8 + 8 * schema.fields[0].key().len().div_ceil(8));
            let idx = PkIndex::read(OwnedBytes::new(build(&ty, &refs, hash, &PkLayout::of(&schema))), hash, Some(&seg)).unwrap();
            out.push(("column", idx));
        }
        for (what, layout) in [("own", PkLayout::Own), ("wide", PkLayout::Wide)] {
            out.push((what, PkIndex::read(OwnedBytes::new(build(&ty, &refs, hash, &layout)), hash, None).unwrap()));
        }
        out
    }

    #[test]
    fn random_string_and_int_keys_found() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        for n in [0usize, 1, 7, 8, 100, 5000] {
            let keys: Vec<Value> = (0..n).map(|i| Value::Str(format!("doc-{i}-{}", rng.gen::<u32>()))).collect();
            for (what, pk) in indexes(FieldType::Keyword, &keys, key_hash) {
                assert_eq!(pk.len(), n, "{what}");
                for (d, k) in keys.iter().enumerate() {
                    assert_eq!(pk.lookup(k), Some(d as u32), "{what}");
                    assert_eq!(&pk.key(d as u32), k, "{what}");
                }
                assert_eq!(pk.lookup(&Value::Str("absent".into())), None, "{what}");
                assert_eq!(pk.lookup(&Value::Int(3)), None, "{what}");
            }

            let ints: Vec<Value> = (0..n).map(|_| Value::Int(rng.gen_range(-1_000_000_000..1_000_000_000))).collect();
            let mut seen = std::collections::HashSet::new();
            let ints: Vec<Value> = ints.into_iter().filter(|v| seen.insert(v.as_i64().unwrap())).collect();
            for (what, mut pk) in indexes(FieldType::Int, &ints, key_hash) {
                pk.bijective_ints = true;
                for (d, k) in ints.iter().enumerate() {
                    assert_eq!(pk.lookup(k), Some(d as u32), "{what}");
                    assert_eq!(&pk.key(d as u32), k, "{what}");
                }
                assert_eq!(pk.lookup(&Value::Str("1".into())), None, "{what}");
                assert_eq!(pk.lookup(&Value::Int(1_000_000_001)), None, "{what}");
            }
        }
    }

    #[test]
    fn bloom_filters_answer_most_misses() {
        let n = 3 * BLOOM_MIN_KEYS;
        let keys: Vec<Value> = (0..n).map(|i| Value::Str(format!("user:{i}"))).collect();
        for (what, pk) in indexes(FieldType::Keyword, &keys, key_hash) {
            let bloom = pk.bloom().expect("a filter for a large segment");
            assert!(bloom.bytes() * 8 >= n * BLOOM_BITS_PER_KEY);
            for (d, k) in keys.iter().enumerate() {
                assert!(bloom.may_contain(pk.bloom_hash(key_hash(KeyRef::of(k).unwrap()))), "{what}");
                assert_eq!(pk.lookup(k), Some(d as u32), "{what}");
            }
            let passed = (0..20_000).filter(|i| bloom.may_contain(pk.bloom_hash(key_hash(KeyRef::Str(format!("other:{i}").as_bytes()))))).count();
            assert!(passed < 400, "{what}: {passed} of 20000 missing keys pass the filter");
            assert_eq!(pk.lookup(&Value::Str("other:1".into())), None, "{what}");
        }
        let ints: Vec<Value> = (0..n as i64).map(|i| Value::Int(i * 7)).collect();
        for (what, pk) in indexes(FieldType::Int, &ints, key_hash) {
            assert!((0..n as i64).all(|i| pk.lookup(&Value::Int(i * 7)) == Some(i as u32)), "{what}");
            let passed = (0..20_000i64).filter(|i| pk.bloom().unwrap().may_contain(pk.bloom_hash(key_hash(KeyRef::Int(i * 7 + 3))))).count();
            assert!(passed < 400, "{what}: {passed} of 20000 missing integer keys pass the filter");
        }
        // Small segments are probed without one.
        for (what, pk) in indexes(FieldType::Keyword, &keys[..100], key_hash) {
            assert!(pk.bloom().is_none(), "{what}");
        }
    }

    #[test]
    fn collisions_resolved_by_stored_key() {
        // Every key hashes to one of three values.
        fn bad(k: KeyRef) -> u64 {
            match k {
                KeyRef::Str(s) => s.len() as u64 % 3,
                KeyRef::Int(v) => v.rem_euclid(3) as u64,
            }
        }
        let keys: Vec<Value> = (0..300).map(|i| Value::Str(format!("k{i}"))).collect();
        for (what, pk) in indexes(FieldType::Keyword, &keys, bad) {
            for (d, k) in keys.iter().enumerate() {
                assert_eq!(pk.lookup(k), Some(d as u32), "{what}");
            }
            assert_eq!(pk.lookup(&Value::Str("k300".into())), None, "{what}");
        }
        let ints: Vec<Value> = (0..300).map(Value::Int).collect();
        for (what, pk) in indexes(FieldType::Int, &ints, bad) {
            assert!((0..300).all(|i| pk.lookup(&Value::Int(i)) == Some(i as u32)), "{what}");
            assert_eq!(pk.lookup(&Value::Int(300)), None, "{what}");
        }
        // Duplicates (the memtable segment keeps deleted versions) all come back.
        let dup: Vec<Value> = ["a", "b", "a", "c", "a"].iter().map(|s| Value::Str(s.to_string())).collect();
        for (what, pk) in indexes(FieldType::Keyword, &dup, bad) {
            assert_eq!(pk.lookup_all(&dup[0]).collect::<Vec<_>>(), vec![0, 2, 4], "{what}");
            assert_eq!(pk.key(3), dup[3], "{what}");
        }
    }

    #[test]
    fn hash_spreads_over_directory() {
        let n = 1 << 16;
        let keys: Vec<Value> = (0..n).map(|i| Value::Str(format!("{i}"))).collect();
        for (what, pk) in indexes(FieldType::Keyword, &keys, key_hash) {
            let max_bucket = pk.dir.windows(2).map(|w| w[1] - w[0]).max().unwrap();
            assert!(max_bucket < 24, "{what}: largest bucket {max_bucket}");
        }
        let mut hashes: Vec<u64> = keys.iter().map(|k| key_hash(KeyRef::of(k).unwrap())).collect();
        hashes.sort();
        hashes.dedup();
        assert_eq!(hashes.len(), n);
    }

    #[test]
    fn section_written_only_with_primary_key() {
        let schema = Schema::new("t", vec![Field::new("id", FieldType::Int)], Some("id")).unwrap();
        let docs = vec![vec![Value::Int(5)], vec![Value::Int(-2)]];
        let mut w = SegmentWriter::new(2);
        build_pk_index(&schema, &docs, &mut w).unwrap();
        let seg = SegmentFile::from_vec(w.finish()).unwrap();
        let pk = PkIndex::open(&seg).unwrap().unwrap();
        assert_eq!(pk.lookup(&Value::Int(-2)), Some(1));
        let schema = Schema::new("t", vec![Field::new("id", FieldType::Int)], None).unwrap();
        let mut w = SegmentWriter::new(2);
        build_pk_index(&schema, &docs, &mut w).unwrap();
        assert!(PkIndex::open(&SegmentFile::from_vec(w.finish()).unwrap()).unwrap().is_none());
    }

    /// A table of a database from before field ids keeps the wide layout,
    /// which the versions before read; one with ids reads a KEYWORD key
    /// from its column unless it is not indexed.
    #[test]
    fn layouts_follow_the_table() {
        let key = |indexed: bool, ty: FieldType| {
            let mut f = Field::new("id", ty);
            f.indexed = indexed;
            Schema::new("t", vec![f, Field::new("n", FieldType::Int)], Some("id")).unwrap()
        };
        let legacy = key(true, FieldType::Keyword).without_ids();
        assert!(legacy.is_legacy());
        assert_eq!(PkLayout::of(&legacy), PkLayout::Wide);
        let ids = key(true, FieldType::Keyword);
        assert_eq!(PkLayout::of(&ids), PkLayout::Column(ids.fields[0].key()));
        assert_eq!(PkLayout::of(&key(false, FieldType::Keyword)), PkLayout::Own);
        assert_eq!(PkLayout::of(&key(true, FieldType::Int)), PkLayout::Own);
    }
}
