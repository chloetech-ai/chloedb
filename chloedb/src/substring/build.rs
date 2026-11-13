//! Writing a field's substring index (`chloedb/notes/substring.md`).

use std::collections::BinaryHeap;

use crate::error::{Error, Result};
use crate::postings::bits::{bit_width, BitVec};
use crate::schema::Field;
use crate::segment::build::BuildOptions;
use crate::segment::{DocId, FieldIndexBuilder, SegmentWriter};
use crate::value::Value;

use super::{section, suffix_array, Meta, PartMeta, Shape, BLOCK_BYTES, BLOCK_DIGITS, BLOCK_WORDS, BUCKET, DEFAULT_PART_BYTES, DEFAULT_SAMPLE, ENCODING, FOLD, LEVELS_AT, MAX_LEN, MAX_PART_BYTES, SEP, VERSION};

/// The builder of `field`'s substring index, or `None` where the build is
/// told to leave it out (`substring = skip`: a memtable's segment, which is
/// scanned).
pub fn field_builder(field: &Field, opts: &BuildOptions) -> Result<Option<Box<dyn FieldIndexBuilder>>> {
    if opts.get("substring") == Some("skip") {
        return Ok(None);
    }
    let bad = |k: &str, v: &str| Error::Schema(format!("{k}: bad value {v:?}"));
    let sample = match opts.get("substring.sample") {
        Some(v) => v.parse().ok().filter(|&s: &u32| (1..=1 << 16).contains(&s)).ok_or_else(|| bad("substring.sample", v))?,
        None => DEFAULT_SAMPLE,
    };
    let part_bytes = match opts.get("substring.part_bytes") {
        Some(v) => v.parse().ok().filter(|&b: &usize| (1..=MAX_PART_BYTES).contains(&b)).ok_or_else(|| bad("substring.part_bytes", v))?,
        None => DEFAULT_PART_BYTES,
    };
    Ok(Some(Box::new(SubstringBuilder::new(&field.key(), sample, part_bytes))))
}

/// Builds one field's substring index from its values in docID order: a
/// part's text is collected, and the part is built when the next value
/// would pass `part_bytes`, so a build holds one part's text and suffixes
/// at a time.
pub struct SubstringBuilder {
    key: String,
    sample: u32,
    part_bytes: usize,
    /// The part being collected: its text so far (the first separator and
    /// one after each value), where each value starts, and each one's
    /// docID.
    text: Vec<u8>,
    starts: Vec<u32>,
    ids: Vec<DocId>,
    /// The parts built: their `bwt`, `sa` and (when the docIDs do not
    /// follow each other) `docs` sections.
    built: Vec<(PartMeta, Vec<u8>, Vec<u8>, Option<Vec<DocId>>)>,
    docs: u64,
    bytes: u64,
    n_docs: u64,
}

impl SubstringBuilder {
    pub fn new(key: &str, sample: u32, part_bytes: usize) -> Self {
        SubstringBuilder { key: key.to_string(), sample: sample.max(1), part_bytes: part_bytes.clamp(1, MAX_PART_BYTES), text: vec![SEP], starts: Vec::new(), ids: Vec::new(), built: Vec::new(), docs: 0, bytes: 0, n_docs: 0 }
    }

    /// Builds the part collected so far, if it has a document.
    fn seal(&mut self) -> Result<()> {
        if self.ids.is_empty() {
            return Ok(());
        }
        let text = std::mem::replace(&mut self.text, vec![SEP]);
        let starts = std::mem::take(&mut self.starts);
        let ids = std::mem::take(&mut self.ids);
        let (mut meta, bwt, sa) = build_part(&text, &starts, self.sample)?;
        // Ordinals are docIDs from `first_doc` unless a document between
        // them has no value.
        let consecutive = ids.windows(2).all(|w| w[1] == w[0] + 1);
        if consecutive {
            meta.first_doc = Some(ids[0] as u64);
        }
        self.built.push((meta, bwt, sa, (!consecutive).then_some(ids)));
        Ok(())
    }
}

impl FieldIndexBuilder for SubstringBuilder {
    fn add(&mut self, doc: DocId, value: &Value) -> Result<()> {
        self.n_docs = self.n_docs.max(doc as u64 + 1);
        let s = match value {
            Value::Null => return Ok(()),
            Value::Str(s) => s.as_bytes(),
            v => return Err(Error::Schema(format!("a substring index takes text, not {v:?}"))),
        };
        if s.len() + 2 > MAX_PART_BYTES {
            return Err(Error::Schema(format!("a value of {} bytes is too long for a substring index", s.len())));
        }
        if !self.ids.is_empty() && self.text.len() + s.len() + 1 > self.part_bytes.max(2) {
            self.seal()?;
        }
        self.starts.push(self.text.len() as u32);
        self.ids.push(doc);
        self.text.extend_from_slice(s);
        self.text.push(SEP);
        self.docs += 1;
        self.bytes += s.len() as u64;
        Ok(())
    }

    fn finish(mut self: Box<Self>, out: &mut SegmentWriter) -> Result<()> {
        self.seal()?;
        let n_docs = (out.n_docs() as u64).max(self.n_docs);
        let mut parts = Vec::with_capacity(self.built.len());
        for (k, (meta, bwt, sa, ids)) in std::mem::take(&mut self.built).into_iter().enumerate() {
            out.put(section(&self.key, &format!("p{k}.bwt")), bwt);
            out.put(section(&self.key, &format!("p{k}.sa")), sa);
            if let Some(ids) = ids {
                let ids: Vec<Option<DocId>> = ids.into_iter().map(Some).collect();
                out.put(section(&self.key, &format!("p{k}.docs")), encode_docs(&ids, n_docs));
            }
            parts.push(meta);
        }
        let meta = Meta { version: VERSION, fold: FOLD.to_string(), docs: self.docs, bytes: self.bytes, parts };
        out.put(section(&self.key, "meta"), serde_json::to_vec(&meta).expect("a substring index's meta as JSON"));
        Ok(())
    }
}

/// A `docs` section: the docID of each ordinal of a part, `None` for an
/// ordinal whose document the segment of `n_docs` documents does not hold.
pub fn encode_docs(ids: &[Option<DocId>], n_docs: u64) -> Vec<u8> {
    let wd = bit_width(n_docs).max(1);
    let none = if wd == 64 { u64::MAX } else { (1u64 << wd) - 1 };
    let mut bits = BitVec::with_capacity(ids.len() * wd as usize);
    for id in ids {
        bits.push_bits(id.map_or(none, |d| d as u64), wd);
    }
    let mut out = Vec::with_capacity(16 + bits.words().len() * 8);
    out.extend_from_slice(&(ids.len() as u64).to_le_bytes());
    out.extend_from_slice(&wd.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    put_words(&mut out, bits.words());
    out
}

fn put_words(out: &mut Vec<u8>, words: &[u64]) {
    out.reserve(words.len() * 8);
    for w in words {
        out.extend_from_slice(&w.to_le_bytes());
    }
}

fn put_u32s(out: &mut Vec<u8>, xs: &[u32]) {
    out.reserve(xs.len() * 4);
    for x in xs {
        out.extend_from_slice(&x.to_le_bytes());
    }
}

/// Code lengths of a 4-ary Huffman code for the bytes with a count, none
/// over [`MAX_LEN`]: where the code of the counts would be longer, the
/// counts are halved (never to zero) until it is not.
pub(crate) fn code_lengths(counts: &[u32; 256]) -> [u8; 256] {
    let mut shift = 0;
    loop {
        let weights: Vec<(u8, u64)> = (0..256).filter(|&c| counts[c] > 0).map(|c| (c as u8, ((counts[c] as u64) >> shift).max(1))).collect();
        let lens = huffman4(&weights);
        if lens.iter().all(|&l| l as usize <= MAX_LEN) {
            return lens;
        }
        shift += 1;
    }
}

/// The depths of a 4-ary Huffman tree of `weights` (byte, weight); one byte
/// alone gets one digit.
fn huffman4(weights: &[(u8, u64)]) -> [u8; 256] {
    let mut lens = [0u8; 256];
    if weights.len() <= 1 {
        for &(c, _) in weights {
            lens[c as usize] = 1;
        }
        return lens;
    }
    // Nodes 0..leaves are the bytes and weightless fillers that make every
    // merge one of four; `parent` of each node made after them.
    let mut n = weights.len();
    while (n - 1) % 3 != 0 {
        n += 1;
    }
    let mut parent: Vec<usize> = vec![usize::MAX; n];
    let mut heap: BinaryHeap<std::cmp::Reverse<(u64, usize)>> = (0..n).map(|i| std::cmp::Reverse((weights.get(i).map_or(0, |w| w.1), i))).collect();
    while heap.len() > 1 {
        let id = parent.len();
        let mut total = 0;
        for _ in 0..4 {
            let std::cmp::Reverse((w, i)) = heap.pop().expect("a multiple of three above one");
            total += w;
            parent[i] = id;
        }
        parent.push(usize::MAX);
        heap.push(std::cmp::Reverse((total, id)));
    }
    for (i, &(c, _)) in weights.iter().enumerate() {
        let (mut at, mut depth) = (i, 0u32);
        while parent[at] != usize::MAX {
            at = parent[at];
            depth += 1;
        }
        lens[c as usize] = depth.min(255) as u8;
    }
    lens
}

/// One part's `bwt` and `sa` sections and its meta (without `first_doc`),
/// from its text `FF v_0 FF ... v_(D-1) FF` and where each value starts.
pub(crate) fn build_part(text: &[u8], starts: &[u32], sample: u32) -> Result<(PartMeta, Vec<u8>, Vec<u8>)> {
    let n = text.len();
    debug_assert!(n >= 2 && text[0] == SEP && text[n - 1] == SEP && !starts.is_empty());
    let sa = suffix_array(text)?;

    // The transform: row 0 is the empty suffix, row r > 0 suffix sa[r - 1];
    // the row of the suffix at 0 has no byte before it and is left out.
    let mut l = Vec::with_capacity(n);
    l.push(text[n - 1]);
    let mut primary = 0u64;
    for (i, &p) in sa.iter().enumerate() {
        match p {
            0 => primary = i as u64 + 1,
            p => l.push(text[p as usize - 1]),
        }
    }
    debug_assert_eq!(l.len(), n);
    let mut counts = [0u32; 256];
    for &c in text {
        counts[c as usize] += 1;
    }
    let runs = 1 + l.windows(2).filter(|w| w[0] != w[1]).count() as u64;

    let lens = code_lengths(&counts);
    let shape = Shape::from_lens(&lens).map_err(Error::Schema)?;
    let bwt = encode_tree(&shape, &counts, l, n as u64, primary);
    let (sa_section, samples) = encode_samples(text, starts, &sa, sample);
    let meta = PartMeta { encoding: ENCODING.to_string(), n: n as u64, docs: starts.len() as u64, first_doc: None, primary, sample: sample as u64, samples, runs };
    Ok((meta, bwt, sa_section))
}

/// The `bwt` section of transform `l` (consumed level by level).
fn encode_tree(shape: &Shape, counts: &[u32; 256], l: Vec<u8>, n: u64, primary: u64) -> Vec<u8> {
    let k = shape.k;
    // Per level: for each internal node, the number of each digit in it.
    let mut in_node: Vec<Vec<[u64; 4]>> = (0..k).map(|d| vec![[0u64; 4]; shape.internal[d] as usize]).collect();
    // The internal node each byte passes through at each depth below its
    // length.
    let mut node_at = vec![[0u32; MAX_LEN]; 256];
    for c in 0..256usize {
        for d in 0..shape.len[c] as usize {
            node_at[c][d] = shape.node_at(c as u8, d);
            in_node[d][node_at[c][d] as usize][shape.code[c][d] as usize] += counts[c] as u64;
        }
    }
    let level_n: Vec<u64> = in_node.iter().map(|nodes| nodes.iter().map(|c| c.iter().sum::<u64>()).sum()).collect();
    debug_assert_eq!(level_n[0], n);

    let tables: usize = (0..k).map(|d| (shape.internal[d] as usize + 1) * 16).sum();
    let header_len = (LEVELS_AT + k * 16 + tables).next_multiple_of(BLOCK_BYTES);
    let blocks = |d: usize| level_n[d] as usize / BLOCK_DIGITS + 1;
    let total = header_len + (0..k).map(|d| blocks(d) * BLOCK_BYTES).sum::<usize>();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&(k as u32).to_le_bytes());
    out.extend_from_slice(&(header_len as u32).to_le_bytes());
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(&primary.to_le_bytes());
    put_u32s(&mut out, counts);
    out.extend_from_slice(&shape.len);
    let mut at = header_len;
    for d in 0..k {
        out.extend_from_slice(&level_n[d].to_le_bytes());
        out.extend_from_slice(&(at as u64).to_le_bytes());
        at += blocks(d) * BLOCK_BYTES;
    }
    // Each node's start in its level and the digits 0, 1 and 2 before it,
    // and once more for the level's end.
    let mut starts: Vec<Vec<u64>> = Vec::with_capacity(k);
    for nodes in &in_node {
        let (mut start, mut before) = (0u64, [0u64; 3]);
        let mut level_starts = Vec::with_capacity(nodes.len());
        for c in nodes.iter().chain(std::iter::once(&[0u64; 4])) {
            level_starts.push(start);
            put_u32s(&mut out, &[start as u32, before[0] as u32, before[1] as u32, before[2] as u32]);
            start += c.iter().sum::<u64>();
            for d in 0..3 {
                before[d] += c[d];
            }
        }
        starts.push(level_starts);
    }
    out.resize(header_len, 0);

    // Level by level: the digits of the bytes still above their leaves, in
    // the order of their nodes, then those bytes placed by their next node.
    let mut cur = l;
    for d in 0..k {
        let mut words = vec![0u64; blocks(d) * BLOCK_WORDS];
        let mut seen = [0u64; 4];
        for (p, &c) in cur.iter().enumerate() {
            let (b, q) = (p / BLOCK_DIGITS, p % BLOCK_DIGITS);
            if q == 0 {
                words[b * BLOCK_WORDS] = seen[0] | seen[1] << 32;
                words[b * BLOCK_WORDS + 1] = seen[2];
            }
            let digit = shape.code[c as usize][d] as u64;
            words[b * BLOCK_WORDS + 2 + q / 32] |= digit << (2 * (q % 32));
            seen[digit as usize] += 1;
        }
        if cur.len() % BLOCK_DIGITS == 0 {
            // The block a rank at the level's end reads.
            let b = cur.len() / BLOCK_DIGITS;
            words[b * BLOCK_WORDS] = seen[0] | seen[1] << 32;
            words[b * BLOCK_WORDS + 1] = seen[2];
        }
        put_words(&mut out, &words);
        if d + 1 < k {
            let mut next = vec![0u8; level_n[d + 1] as usize];
            let mut fill: Vec<u64> = starts[d + 1].clone();
            for &c in &cur {
                if shape.len[c as usize] as usize > d + 1 {
                    let slot = &mut fill[node_at[c as usize][d + 1] as usize];
                    next[*slot as usize] = c;
                    *slot += 1;
                }
            }
            cur = next;
        }
    }
    debug_assert_eq!(out.len(), total);
    out
}

/// The `sa` section, and the number of sampled rows: the rows whose suffix
/// starts at the separator before a value, or at every `sample`-th byte of
/// a value from its start, each with the value's ordinal.
fn encode_samples(text: &[u8], starts: &[u32], sa: &[i32], sample: u32) -> (Vec<u8>, u64) {
    let n = text.len();
    let mut marked = BitVec::zeros(n);
    for (o, &a) in starts.iter().enumerate() {
        let end = starts.get(o + 1).map_or(n - 1, |&next| next as usize - 1);
        marked.set(a as usize - 1);
        for p in (a as usize..end).step_by(sample as usize) {
            marked.set(p);
        }
    }
    let w = bit_width(starts.len() as u64 - 1).max(1);
    let buckets = (n as u64 + 1).div_ceil(BUCKET) as usize;
    let mut dir = vec![0u32; buckets + 1];
    let mut entries = BitVec::new();
    let mut m = 0u64;
    // Row 0, the empty suffix, is never sampled.
    for (i, &p) in sa.iter().enumerate() {
        let (row, p) = (i as u64 + 1, p as usize);
        if !marked.get(p) {
            continue;
        }
        // The value at `p`, or the one after the separator at `p`.
        let ordinal = starts.partition_point(|&a| a as usize - 1 <= p) - 1;
        entries.push_bits(row % BUCKET | (ordinal as u64) << 9, 9 + w);
        dir[(row / BUCKET) as usize + 1] += 1;
        m += 1;
    }
    for b in 0..buckets {
        dir[b + 1] += dir[b];
    }
    let mut out = Vec::with_capacity(16 + dir.len() * 4 + 8 + entries.words().len() * 8);
    out.extend_from_slice(&m.to_le_bytes());
    out.extend_from_slice(&w.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    put_u32s(&mut out, &dir);
    out.resize(out.len().next_multiple_of(8), 0);
    put_words(&mut out, entries.words());
    (out, m)
}
