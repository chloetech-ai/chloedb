//! Reading a field's substring index (`chloedb/notes/substring.md`).
//!
//! Opening reads the meta section and each part's headers and checks them
//! against each other; it reads no level's blocks and no sample. Every
//! position a search computes from the data is checked before it is used,
//! and a walk is bounded by the part's sampling rate, so a damaged section
//! gives an error or a wrong answer, never a read outside it or a loop.

use crate::error::{corrupt, Error, Result};
use crate::postings::bits::{bit_width, get_bits};
use crate::postings::bytes::{OwnedBytes, Slice};
use crate::segment::{DocId, SegmentFile};

use super::{section, Meta, PartMeta, Shape, BLOCK_BYTES, BLOCK_DIGITS, BLOCK_WORDS, BUCKET, ENCODING, FOLD, LEVELS_AT, MAX_LEN, VERSION};

fn bad(what: impl std::fmt::Display) -> Error {
    corrupt(format!("substring index: {what}"))
}

/// A field's substring index in one segment: its parts.
#[derive(Debug)]
pub struct SubstringIndex {
    parts: Vec<Part>,
    docs: u64,
    bytes: u64,
}

impl SubstringIndex {
    /// The index of the field with key `key` in `seg`. `None` where the
    /// segment has none, or one this version does not read (another
    /// version, fold or encoding): those documents are scanned instead.
    pub fn open(seg: &SegmentFile, key: &str) -> Result<Option<SubstringIndex>> {
        let Some(meta) = seg.section(&section(key, "meta"))? else { return Ok(None) };
        let v: serde_json::Value = serde_json::from_slice(&meta).map_err(|e| bad(format!("meta: {e}")))?;
        let version = v.get("version").and_then(|x| x.as_u64()).ok_or_else(|| bad("meta without a version"))?;
        if version != VERSION || v.get("fold").and_then(|x| x.as_str()) != Some(FOLD) {
            return Ok(None);
        }
        let known = |p: &serde_json::Value| p.get("encoding").and_then(|x| x.as_str()) == Some(ENCODING);
        if !v.get("parts").and_then(|x| x.as_array()).is_some_and(|ps| ps.iter().all(known)) {
            return Ok(None);
        }
        let meta: Meta = serde_json::from_value(v).map_err(|e| bad(format!("meta: {e}")))?;
        let n_docs = seg.n_docs() as u64;
        let mut parts = Vec::with_capacity(meta.parts.len());
        for (k, pm) in meta.parts.iter().enumerate() {
            let bwt = seg.require(&section(key, &format!("p{k}.bwt")))?;
            let sa = seg.require(&section(key, &format!("p{k}.sa")))?;
            let docs = seg.section(&section(key, &format!("p{k}.docs")))?;
            parts.push(Part::open(pm, bwt, sa, docs, n_docs).map_err(|e| match e {
                Error::Corrupt(m) => corrupt(format!("{m} (part {k})")),
                e => e,
            })?);
        }
        if parts.iter().map(|p| p.n_docs).sum::<u64>() != meta.docs {
            return Err(bad("the parts' documents are not the meta's"));
        }
        Ok(Some(SubstringIndex { parts, docs: meta.docs, bytes: meta.bytes }))
    }

    pub fn parts(&self) -> &[Part] {
        &self.parts
    }

    /// The documents with a value.
    pub fn docs(&self) -> u64 {
        self.docs
    }

    /// The bytes of their values.
    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// The number of times `pattern` occurs in the indexed values (rows
    /// deleted since included). `pattern` is not empty and holds no 0xFF.
    pub fn count(&self, pattern: &[u8]) -> Result<u64> {
        let mut total = 0;
        for p in &self.parts {
            let (lo, hi) = p.search(pattern)?;
            total += hi - lo;
        }
        Ok(total)
    }
}

/// One level of a part's wavelet tree.
#[derive(Debug)]
struct Level {
    /// Blocks of sixteen words: three counters and 448 digits.
    blocks: Slice<u64>,
    /// Per internal node and once for the level's end: the node's start and
    /// the digits 0, 1 and 2 before it.
    nodes: Slice<u32>,
}

/// How a part's ordinals become docIDs.
#[derive(Debug)]
enum Docs {
    /// Ordinal `o` is docID `first + o`.
    From(u64),
    /// Entry `o` of `wd` bits each, `none` for no document.
    Map { words: Slice<u64>, wd: u32, none: u64 },
}

/// One part of a substring index: an FM-index of `n` bytes of text.
#[derive(Debug)]
pub struct Part {
    n: u64,
    primary: u64,
    sample: u64,
    n_docs: u64,
    runs: u64,
    /// `c[b]`: one more than the bytes of the text below `b`; `c[256]` is
    /// `n + 1`.
    c: Box<[u64; 257]>,
    shape: Shape,
    levels: Vec<Level>,
    /// The sampled rows, the bits of an ordinal, each bucket's first entry
    /// and the entries.
    m: u64,
    w: u32,
    dir: Slice<u32>,
    entries: Slice<u64>,
    docs: Docs,
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}

fn view<T: crate::postings::bytes::Pod>(bytes: &OwnedBytes, range: std::ops::Range<usize>, what: &str) -> Result<Slice<T>> {
    if range.start > range.end || range.end > bytes.len() {
        return Err(bad(format!("{what} out of its section")));
    }
    Slice::view(bytes.slice(range)).map_err(|e| bad(format!("{what}: {e}")))
}

impl Part {
    fn open(pm: &PartMeta, bwt: OwnedBytes, sa: OwnedBytes, docs: Option<OwnedBytes>, seg_docs: u64) -> Result<Part> {
        // The tree's header.
        if bwt.len() < LEVELS_AT {
            return Err(bad("a tree shorter than its header"));
        }
        let (k, header_len) = (u32_at(&bwt, 0) as usize, u32_at(&bwt, 4) as usize);
        let (n, primary) = (u64_at(&bwt, 8), u64_at(&bwt, 16));
        if !(1..=MAX_LEN).contains(&k) || header_len % BLOCK_BYTES != 0 || header_len > bwt.len() || header_len < LEVELS_AT + k * 16 {
            return Err(bad("a bad tree header"));
        }
        if n != pm.n || primary != pm.primary || n < 2 || n > u32::MAX as u64 - 1 || primary > n || pm.sample == 0 || pm.docs == 0 || pm.docs >= n {
            return Err(bad("a part's header and meta disagree"));
        }
        let mut counts = [0u32; 256];
        let mut len = [0u8; 256];
        let mut c = Box::new([0u64; 257]);
        c[0] = 1;
        for b in 0..256 {
            counts[b] = u32_at(&bwt, 24 + 4 * b);
            len[b] = bwt[24 + 1024 + b];
            if (counts[b] == 0) != (len[b] == 0) {
                return Err(bad("a byte with a count and no code, or a code and no count"));
            }
            c[b + 1] = c[b] + counts[b] as u64;
        }
        if c[256] != n + 1 {
            return Err(bad("byte counts that are not the text's length"));
        }
        let shape = Shape::from_lens(&len).map_err(bad)?;
        if shape.k != k {
            return Err(bad("a tree of other levels than its codes"));
        }
        let mut levels = Vec::with_capacity(k);
        let mut table_at = LEVELS_AT + k * 16;
        for d in 0..k {
            let (level_n, at) = (u64_at(&bwt, LEVELS_AT + d * 16), u64_at(&bwt, LEVELS_AT + d * 16 + 8));
            let want: u64 = (0..256).filter(|&b| len[b] as usize > d).map(|b| counts[b] as u64).sum();
            let at = usize::try_from(at).map_err(|_| bad("a level out of its section"))?;
            if level_n != want || at % BLOCK_BYTES != 0 || at < header_len {
                return Err(bad("a bad level"));
            }
            let blocks = level_n as usize / BLOCK_DIGITS + 1;
            let end = at.checked_add(blocks * BLOCK_BYTES).ok_or_else(|| bad("a level out of its section"))?;
            let table = (shape.internal[d] as usize + 1) * 16;
            if table_at + table > header_len {
                return Err(bad("a node table out of the header"));
            }
            let nodes: Slice<u32> = view(&bwt, table_at..table_at + table, "a node table")?;
            table_at += table;
            let mut prev = 0u32;
            for (i, t) in nodes.chunks_exact(4).enumerate() {
                let before = t[1] as u64 + t[2] as u64 + t[3] as u64;
                if (i == 0 && t[0] != 0) || t[0] < prev || before > t[0] as u64 {
                    return Err(bad("node starts that do not rise"));
                }
                prev = t[0];
            }
            if prev as u64 != level_n {
                return Err(bad("node starts that do not end at the level's length"));
            }
            levels.push(Level { blocks: view(&bwt, at..end, "a level")?, nodes });
        }

        // The samples.
        if sa.len() < 16 {
            return Err(bad("samples shorter than their header"));
        }
        let (m, w) = (u64_at(&sa, 0), u32_at(&sa, 8));
        if m != pm.samples || w != bit_width(pm.docs - 1).max(1) || m > n {
            return Err(bad("the samples' header and the meta disagree"));
        }
        let buckets = (n + 1).div_ceil(BUCKET) as usize;
        let dir_end = 16 + (buckets + 1) * 4;
        let dir: Slice<u32> = view(&sa, 16..dir_end, "the samples' directory")?;
        let at = dir_end.next_multiple_of(8);
        let words = (m as usize * (9 + w as usize)).div_ceil(64);
        let entries: Slice<u64> = view(&sa, at..at + words * 8, "the samples")?;
        if dir[0] != 0 || dir[buckets] as u64 != m {
            return Err(bad("a samples directory that does not count the samples"));
        }

        // Ordinals to docIDs.
        let docs = match (pm.first_doc, docs) {
            (Some(first), None) if first + pm.docs <= seg_docs => Docs::From(first),
            (None, Some(b)) => {
                let wd = bit_width(seg_docs).max(1);
                if b.len() < 16 || u64_at(&b, 0) != pm.docs || u32_at(&b, 8) != wd {
                    return Err(bad("a bad docs section"));
                }
                let words = (pm.docs as usize * wd as usize).div_ceil(64);
                Docs::Map { words: view(&b, 16..16 + words * 8, "the docs")?, wd, none: if wd == 64 { u64::MAX } else { (1u64 << wd) - 1 } }
            }
            _ => return Err(bad("a part with both a first document and a docs section, or neither, or documents past the segment's")),
        };
        Ok(Part { n, primary, sample: pm.sample, n_docs: pm.docs, runs: pm.runs, c, shape, levels, m, w, dir, entries, docs })
    }

    /// The bytes of the part's text, separators included.
    pub fn n(&self) -> u64 {
        self.n
    }

    /// The part's documents: its ordinals are `0..n_docs`.
    pub fn n_docs(&self) -> u64 {
        self.n_docs
    }

    /// The sampled rows.
    pub fn samples(&self) -> u64 {
        self.m
    }

    /// The runs of equal bytes in the part's transform, as its builder
    /// counted them: `n / runs` says how repetitive the text is (no query
    /// reads it).
    pub fn runs(&self) -> u64 {
        self.runs
    }

    /// The number of times byte `b` occurs in the part's text.
    pub fn byte_count(&self, b: u8) -> u64 {
        self.c[b as usize + 1] - self.c[b as usize]
    }

    /// Every row: the rows of the empty pattern.
    pub fn all(&self) -> (u64, u64) {
        (0, self.n + 1)
    }

    /// The rows of the suffixes that start with `pattern`: as many as it
    /// occurs in the text.
    pub fn search(&self, pattern: &[u8]) -> Result<(u64, u64)> {
        self.extend(self.all(), pattern)
    }

    /// The rows of the suffixes that start with `bytes` followed by what
    /// the suffixes of `rows` start with; an empty range where there are
    /// none.
    pub fn extend(&self, rows: (u64, u64), bytes: &[u8]) -> Result<(u64, u64)> {
        let (mut lo, mut hi) = rows;
        if hi > self.n + 1 || lo > hi {
            return Err(bad("rows out of the part"));
        }
        for &b in bytes.iter().rev() {
            if lo >= hi || self.shape.len[b as usize] == 0 {
                return Ok((0, 0));
            }
            lo = self.c[b as usize] + self.rank(b, lo)?;
            hi = self.c[b as usize] + self.rank(b, hi)?;
            if lo > hi || hi > self.n + 1 {
                return Err(bad("ranks that do not rise"));
            }
        }
        Ok(if lo < hi { (lo, hi) } else { (0, 0) })
    }

    /// The number of times byte `b` occurs in the transform's rows before
    /// `row`.
    fn rank(&self, b: u8, row: u64) -> Result<u64> {
        self.rank_in(b, row - (row > self.primary) as u64).ok_or_else(|| bad("a rank out of its level"))
    }

    fn rank_in(&self, b: u8, at: u64) -> Option<u64> {
        let (mut node, mut o) = (0usize, at);
        for d in 0..self.shape.len[b as usize] as usize {
            let level = &self.levels[d];
            let t = level.nodes.get(node * 4..node * 4 + 5)?;
            let (start, end) = (t[0] as u64, t[4] as u64);
            if o > end.checked_sub(start)? {
                return None;
            }
            let digit = self.shape.code[b as usize][d];
            o = rank_digit(&level.blocks, digit, start + o)?.checked_sub(before(t, digit)?)?;
            node = node * 4 + digit as usize;
        }
        Some(o)
    }

    /// The byte of the transform at `at`, and the number of times it occurs
    /// before.
    fn access(&self, at: u64) -> Option<(u8, u64)> {
        let (mut node, mut o) = (0usize, at);
        for d in 0..self.shape.k {
            let level = &self.levels[d];
            let t = level.nodes.get(node * 4..node * 4 + 5)?;
            let (start, end) = (t[0] as u64, t[4] as u64);
            if o >= end.checked_sub(start)? {
                return None;
            }
            let p = (start + o) as usize;
            let q = p % BLOCK_DIGITS;
            let word = level.blocks.get(p / BLOCK_DIGITS * BLOCK_WORDS + 2 + q / 32)?;
            let digit = (word >> (2 * (q % 32)) & 3) as u8;
            o = rank_digit(&level.blocks, digit, start + o)?.checked_sub(before(t, digit)?)?;
            let slot = node * 4 + digit as usize;
            let internal = self.shape.internal[d + 1] as usize;
            if slot >= internal {
                return self.shape.leaves[d + 1].get(slot - internal).map(|&b| (b, o));
            }
            node = slot;
        }
        None
    }

    /// The ordinal sampled at `row`, if the row is sampled.
    fn sampled(&self, row: u64) -> Option<Option<u64>> {
        let b = (row / BUCKET) as usize;
        let (lo, hi) = (*self.dir.get(b)? as u64, *self.dir.get(b + 1)? as u64);
        if lo > hi || hi > self.m || hi - lo > BUCKET {
            return None;
        }
        let width = 9 + self.w;
        for e in lo..hi {
            let v = get_bits(&self.entries, e as usize * width as usize, width);
            if v & (BUCKET - 1) == row % BUCKET {
                return Some(Some(v >> 9));
            }
        }
        Some(None)
    }

    /// The ordinal of the document that the suffix of `row` starts in: a
    /// walk back through the text to a sampled row, of fewer steps than
    /// the sampling rate.
    pub fn ordinal(&self, row: u64) -> Result<u64> {
        let mut row = row;
        for _ in 0..=self.sample {
            if row == 0 || row > self.n {
                break;
            }
            match self.sampled(row).ok_or_else(|| bad("a bad bucket of samples"))? {
                Some(o) if o < self.n_docs => return Ok(o),
                Some(_) => return Err(bad("a sample of a document the part does not have")),
                None => {}
            }
            if row == self.primary {
                break;
            }
            let (b, j) = self.access(row - (row > self.primary) as u64).ok_or_else(|| bad("a step out of its level"))?;
            row = self.c[b as usize] + j;
        }
        Err(bad("a walk that found no sample"))
    }

    /// The docID of ordinal `o`, or `None` where the segment does not hold
    /// its document.
    pub fn doc(&self, o: u64) -> Result<Option<DocId>> {
        if o >= self.n_docs {
            return Err(bad("an ordinal the part does not have"));
        }
        Ok(match &self.docs {
            Docs::From(first) => Some((first + o) as DocId),
            Docs::Map { words, wd, none } => match get_bits(words, o as usize * *wd as usize, *wd) {
                v if v == *none => None,
                v => Some(v as DocId),
            },
        })
    }

    /// The rows of the transform (for tests): the byte before each sorted
    /// suffix, without the row of the suffix that starts the text.
    #[cfg(test)]
    pub(crate) fn transform(&self) -> Vec<u8> {
        (0..self.n).map(|at| self.access(at).expect("a row of the transform").0).collect()
    }
}

/// The count of `digit` before a node's start, from its table entry.
fn before(t: &[u32], digit: u8) -> Option<u64> {
    Some(match digit {
        0..=2 => t[1 + digit as usize] as u64,
        _ => (t[0] as u64).checked_sub(t[1] as u64 + t[2] as u64 + t[3] as u64)?,
    })
}

/// The number of digits equal to `digit` in a level's first `pos` digits.
fn rank_digit(blocks: &[u64], digit: u8, pos: u64) -> Option<u64> {
    let (b, q) = ((pos / BLOCK_DIGITS as u64) as usize, (pos % BLOCK_DIGITS as u64) as usize);
    let block = blocks.get(b * BLOCK_WORDS..(b + 1) * BLOCK_WORDS)?;
    let counted = match digit {
        0 => block[0] & 0xFFFF_FFFF,
        1 => block[0] >> 32,
        2 => block[1] & 0xFFFF_FFFF,
        _ => (b as u64 * BLOCK_DIGITS as u64).checked_sub((block[0] & 0xFFFF_FFFF) + (block[0] >> 32) + (block[1] & 0xFFFF_FFFF))?,
    };
    // A word's digits equal to `digit`: the pairs of bits that are both
    // zero once the digit is xored out.
    const LOW: u64 = 0x5555_5555_5555_5555;
    let pattern = LOW * digit as u64;
    let equal = |w: u64| {
        let x = w ^ pattern;
        !(x | x >> 1) & LOW
    };
    let (full, rest) = (q / 32, q % 32);
    let mut count = 0u32;
    for &w in &block[2..2 + full] {
        count += equal(w).count_ones();
    }
    if rest > 0 {
        count += (equal(block[2 + full]) & ((1u64 << (2 * rest)) - 1)).count_ones();
    }
    Some(counted + count as u64)
}
