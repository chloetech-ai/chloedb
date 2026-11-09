//! The public posting cursor (both formats) and the positions stream.
//!
//! Positions of a term are stored per posting, in posting order, as deltas
//! (the first one absolute), in one of two layouts:
//!
//! - **varint** (`text.<field>.pos`): a varint per delta;
//! - **packed** (`text.<field>.ppos`, [`write_packed_positions`]): for each
//!   block of [`BLOCK`] postings, the deltas of all its postings bit-packed
//!   as their least value (a varint) plus a fixed number of bits each (a
//!   byte before it), frame of reference. Log lines repeat their words at
//!   the same places, so a block of a common word's positions often takes
//!   0 to 3 bits a position where a varint takes 8: on four of Loghub's
//!   logs, positions went from 6.9-13.6 bytes a line to 1.3-5.0
//!   (`results/log-bytes/`), and phrase queries took as long.
//!
//! Lists of more than one block start with the byte offset of every
//! block's positions (`u32` each), so a phrase query reading the positions
//! of a posting skips at most the 127 postings before it in its block,
//! using their frequencies; a packed block then reads them in place.

use crate::postings::bytes::OwnedBytes;
use crate::postings::codecs::loom::Loom;
use crate::postings::codecs::PostingList;

use super::codec::{bit_width, get_varint, pack_bits, put_varint};
use super::postings::{read_frontier, BpCursor, Impact, LoomListCursor, BLOCK};
use crate::segment::DocId;

pub use super::postings::END;

/// Appends the positions of one term: `positions` holds every posting's
/// positions (increasing within a posting), `freqs` how many each has.
pub fn write_positions(out: &mut Vec<u8>, freqs: &[u32], positions: &[u32]) {
    let nb = freqs.len().div_ceil(BLOCK);
    let index = out.len();
    if nb > 1 {
        out.resize(index + 4 * nb, 0);
    }
    let data = out.len();
    let mut at = 0;
    for (r, &f) in freqs.iter().enumerate() {
        if nb > 1 && r.is_multiple_of(BLOCK) {
            let k = index + 4 * (r / BLOCK);
            let off = (out.len() - data) as u32;
            out[k..k + 4].copy_from_slice(&off.to_le_bytes());
        }
        let mut prev = 0;
        for &p in &positions[at..at + f as usize] {
            put_varint(out, (p - prev) as u64);
            prev = p;
        }
        at += f as usize;
    }
}

/// Appends the positions of one term in the packed layout: as
/// [`write_positions`] takes them, each block of [`BLOCK`] postings as one
/// byte of width `w`, the least delta of the block as a varint, and every
/// delta less that least in `w` bits, least significant bit first.
pub fn write_packed_positions(out: &mut Vec<u8>, freqs: &[u32], positions: &[u32]) {
    let nb = freqs.len().div_ceil(BLOCK);
    let index = out.len();
    if nb > 1 {
        out.resize(index + 4 * nb, 0);
    }
    let data = out.len();
    let (mut at, mut deltas) = (0, Vec::new());
    for (b, fs) in freqs.chunks(BLOCK).enumerate() {
        if nb > 1 {
            let k = index + 4 * b;
            let off = (out.len() - data) as u32;
            out[k..k + 4].copy_from_slice(&off.to_le_bytes());
        }
        deltas.clear();
        for &f in fs {
            let mut prev = 0;
            for &p in &positions[at..at + f as usize] {
                deltas.push(p - prev);
                prev = p;
            }
            at += f as usize;
        }
        let least = deltas.iter().copied().min().unwrap_or(0);
        deltas.iter_mut().for_each(|d| *d -= least);
        let width = deltas.iter().map(|&d| bit_width(d)).max().unwrap_or(0);
        out.push(width as u8);
        put_varint(out, least as u64);
        pack_bits(&deltas, width, out);
    }
}

/// [`get_varint`] that stops at the end of `data` (a damaged list reads
/// wrong positions, never out of bounds).
fn get_varint_checked(data: &[u8], pos: &mut usize) -> u32 {
    let (mut v, mut shift) = (0u64, 0);
    while let Some(&b) = data.get(*pos) {
        *pos += 1;
        v |= ((b & 0x7f) as u64) << shift.min(63);
        if b < 0x80 {
            break;
        }
        shift += 7;
    }
    v as u32
}

/// The `width`-bit value at index `i` of the bits starting at `data[0]`,
/// 0 past the end of `data` (a damaged list reads wrong positions, never
/// out of bounds).
#[inline]
fn bits_at(data: &[u8], i: usize, width: u32) -> u32 {
    if width == 0 {
        return 0;
    }
    let bit = i * width as usize;
    let at = bit / 8;
    let mut w = [0u8; 8];
    match data.get(at..at + 8) {
        Some(b) => w.copy_from_slice(b),
        None => {
            let b = data.get(at..).unwrap_or(&[]);
            w[..b.len()].copy_from_slice(b);
        }
    }
    ((u64::from_le_bytes(w) >> (bit % 8)) & ((1u64 << width) - 1)) as u32
}

/// Reads one term's positions; remembers where it stopped so that reading
/// postings in order costs one pass over the stream.
struct PosReader<'a> {
    /// Block offsets (empty for single-block lists).
    index: &'a [u8],
    data: &'a [u8],
    /// Rank of the posting whose positions start at `off`.
    next_rank: usize,
    off: usize,
    /// The packed layout's current block: its least delta, width, and the
    /// index of `next_rank`'s first delta among the block's (`off` is
    /// where its bits start then).
    packed: Option<Packed>,
}

#[derive(Clone, Copy)]
struct Packed {
    block: usize,
    least: u32,
    width: u32,
    at: usize,
}

impl<'a> PosReader<'a> {
    fn new(bytes: &'a [u8], n: u32, packed: bool) -> Self {
        let nb = (n as usize).div_ceil(BLOCK);
        let ilen = if nb > 1 { (4 * nb).min(bytes.len()) } else { 0 };
        let packed = packed.then_some(Packed { block: usize::MAX, least: 0, width: 0, at: 0 });
        PosReader { index: &bytes[..ilen], data: &bytes[ilen..], next_rank: 0, off: 0, packed }
    }

    /// The byte offset of block `b`'s positions.
    fn block_start(&self, b: usize) -> usize {
        match self.index.get(4 * b..4 * b + 4) {
            Some(o) => u32::from_le_bytes(o.try_into().unwrap()) as usize,
            None => 0,
        }
    }

    /// Positions of the posting of rank `r` (frequency `freq_at(r)`).
    fn read(&mut self, r: usize, mut freq_at: impl FnMut(usize) -> u32, out: &mut Vec<u32>) {
        let b = r / BLOCK;
        if let Some(mut k) = self.packed {
            if k.block != b || r < self.next_rank {
                let mut off = self.block_start(b);
                k = Packed { block: b, width: self.data.get(off).map_or(0, |&w| w.min(32) as u32), least: 0, at: 0 };
                off += 1;
                k.least = get_varint_checked(self.data, &mut off);
                self.off = off;
                self.next_rank = b * BLOCK;
            }
            while self.next_rank < r {
                k.at += freq_at(self.next_rank) as usize;
                self.next_rank += 1;
            }
            out.clear();
            let bits = self.data.get(self.off..).unwrap_or(&[]);
            let mut p = 0u32;
            let f = freq_at(r) as usize;
            for i in k.at..k.at + f {
                p = p.wrapping_add(k.least.wrapping_add(bits_at(bits, i, k.width)));
                out.push(p);
            }
            k.at += f;
            self.next_rank = r + 1;
            self.packed = Some(k);
            return;
        }
        if r < self.next_rank || b > self.next_rank / BLOCK {
            self.next_rank = b * BLOCK;
            self.off = if self.index.is_empty() { 0 } else { self.block_start(b) };
        }
        let data = self.data;
        while self.next_rank < r {
            let mut left = freq_at(self.next_rank);
            while left > 0 {
                left -= (data[self.off] < 0x80) as u32;
                self.off += 1;
            }
            self.next_rank += 1;
        }
        out.clear();
        let mut p = 0u32;
        for _ in 0..freq_at(r) {
            p += get_varint(data, &mut self.off) as u32;
            out.push(p);
        }
        self.next_rank = r + 1;
    }
}

enum Inner<'a> {
    Bp(BpCursor<'a>),
    Loom(LoomListCursor<'a>),
}

/// A forward cursor over one term's postings: docIDs, frequencies, block
/// impacts and (for fields with positions) positions.
///
/// A new cursor is on the first posting. [`doc`](Self::doc) is [`END`]
/// once exhausted.
pub struct PostingCursor<'a> {
    inner: Inner<'a>,
    df: u32,
    frontier: Vec<(u32, u8)>,
    positions: Option<PosReader<'a>>,
}

impl<'a> PostingCursor<'a> {
    /// A BP128 list starting at `data[0]` (`data` runs to the end of the
    /// postings section).
    pub(crate) fn bp(data: &'a [u8], df: u32, positions: Option<(&'a [u8], bool)>) -> Self {
        let mut pos = 0;
        let frontier = read_frontier(data, &mut pos);
        let inner = Inner::Bp(BpCursor::new(data, pos, df));
        PostingCursor { inner, df, frontier, positions: positions.map(|(p, packed)| PosReader::new(p, df, packed)) }
    }

    /// The Loom list `list` at `start` of the postings section, opened by
    /// [`super::postings::open_loom`] (which returned `pos`).
    pub(crate) fn loom(section: &'a OwnedBytes, start: usize, list: Loom, mut pos: usize, df: u32, positions: Option<(&'a [u8], bool)>) -> Self {
        debug_assert_eq!(list.len(), df as usize);
        let data = &section[start..];
        let frontier = read_frontier(data, &mut pos);
        let inner = Inner::Loom(LoomListCursor::new(list, data, pos));
        PostingCursor { inner, df, frontier, positions: positions.map(|(p, packed)| PosReader::new(p, df, packed)) }
    }

    /// Current docID ([`END`] when exhausted).
    #[inline]
    pub fn doc(&self) -> DocId {
        match &self.inner {
            Inner::Bp(c) => c.doc(),
            Inner::Loom(c) => c.doc(),
        }
    }

    /// Moves to the next posting.
    #[inline]
    pub fn next(&mut self) -> DocId {
        match &mut self.inner {
            Inner::Bp(c) => c.next(),
            Inner::Loom(c) => c.next(),
        }
    }

    /// Moves to the first posting `>= target` (NextGEQ); never backwards.
    #[inline]
    pub fn seek(&mut self, target: DocId) -> DocId {
        match &mut self.inner {
            Inner::Bp(c) => c.seek(target),
            Inner::Loom(c) => c.seek(target),
        }
    }

    /// Term frequency in the current document.
    #[inline]
    pub fn freq(&mut self) -> u32 {
        match &mut self.inner {
            Inner::Bp(c) => c.freq(),
            Inner::Loom(c) => c.freq(),
        }
    }

    /// Number of postings (document frequency in this segment).
    pub fn len(&self) -> u32 {
        self.df
    }

    pub fn is_empty(&self) -> bool {
        self.df == 0
    }

    /// Rank of the current posting (the list length once exhausted).
    #[inline]
    pub fn rank(&mut self) -> usize {
        match &mut self.inner {
            Inner::Bp(c) => c.rank(),
            Inner::Loom(c) => c.rank(),
        }
    }

    /// The list's impact frontier: `(tf, norm)` pairs, increasing in both,
    /// such that every posting has a smaller or equal tf than some pair with
    /// a smaller or equal norm.
    pub fn frontier(&self) -> &[(u32, u8)] {
        &self.frontier
    }

    /// Last docID and impact (`max_tf << 8 | min_norm`, or
    /// [`super::postings::LIST_IMPACT`] for "the list's frontier") of the
    /// block holding the first posting `>= target`, without decoding it;
    /// ([`END`], 0) past the end. Targets must not decrease between calls.
    #[inline]
    pub fn block_bound(&mut self, target: DocId) -> (DocId, Impact) {
        match &mut self.inner {
            Inner::Bp(c) => c.block_bound(target),
            Inner::Loom(c) => c.block_bound(target),
        }
    }

    /// Whether the field has positions.
    pub fn has_positions(&self) -> bool {
        self.positions.is_some()
    }

    /// Positions of the current posting, in increasing order (empty for
    /// fields without positions or an exhausted cursor).
    pub fn positions(&mut self, out: &mut Vec<u32>) {
        out.clear();
        if self.doc() == END {
            return;
        }
        let Some(pr) = self.positions.as_mut() else { return };
        match &mut self.inner {
            Inner::Bp(c) => {
                let r = c.rank();
                pr.read(r, |j| c.freq_at(j), out);
            }
            Inner::Loom(c) => {
                let r = c.rank();
                pr.read(r, |j| c.freq_at(j), out);
            }
        }
    }

    /// Whether the docIDs are a Loom list.
    pub fn is_loom(&self) -> bool {
        matches!(self.inner, Inner::Loom(_))
    }
}
