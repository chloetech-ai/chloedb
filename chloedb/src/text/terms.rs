//! A segment's terms of a text field laid out flat in memory, for typo
//! walks ([`super::fuzzy::walk`]).
//!
//! A walk of the [`Dict`](super::dict::Dict) itself spends most of its time
//! decoding: each term it steps over or jumps past is a front-coded entry
//! with three varint lengths after it, read to keep the postings offsets
//! right, although the walk wants only the term's bytes. The table keeps
//! every term whole, one after another, with where each ends and how many
//! bytes it shares with the one before: a step is an index increment, and
//! a jump past the terms starting with a dead prefix is a scan of those
//! shared counts, with no bytes compared. On a 5M-row shard of three
//! segments a two-edit walk takes 2.4 times less time than the
//! dictionary's.
//!
//! It is built from the dictionary the first time a segment answers a typo
//! query (12 ms for 530K terms) and lives as long as the segment's readers
//! (a segment never changes), at the terms' bytes plus 5 bytes a term
//! (6.5 MB for 2M MS MARCO passages, against 4.7 MB for the dictionary).

use super::dict::Dict;
use super::fuzzy::{Match, Sorted};

/// The terms of a dictionary, in its order (see the module).
pub struct TermTable {
    bytes: Vec<u8>,
    /// Where each term ends in `bytes`; it starts where the one before ends.
    ends: Vec<u32>,
    /// The bytes each term shares with the one before (0 for the first),
    /// [`LONG`] for that many or more: a run of terms with a prefix ends at
    /// the first sharing fewer, found without comparing bytes.
    shared: Vec<u8>,
}

/// [`TermTable::shared`]'s cap.
const LONG: u8 = u8::MAX;

impl TermTable {
    /// The table of `dict`'s terms, `None` when they do not fit in 4 GiB
    /// (the ends are 32-bit).
    pub fn of(dict: &Dict) -> Option<Self> {
        let n = dict.n_terms() as usize;
        let mut bytes: Vec<u8> = Vec::new();
        let mut ends = Vec::with_capacity(n);
        let mut shared = Vec::with_capacity(n);
        let mut fits = true;
        dict.for_each(|t, _| {
            if !fits {
                return;
            }
            let start = if ends.len() >= 2 { ends[ends.len() - 2] as usize } else { 0 };
            let prev = if ends.is_empty() { &[][..] } else { &bytes[start..] };
            shared.push(prev.iter().zip(t).take_while(|(a, b)| a == b).count().min(LONG as usize) as u8);
            bytes.extend_from_slice(t);
            match u32::try_from(bytes.len()) {
                Ok(e) => ends.push(e),
                Err(_) => fits = false,
            }
        });
        if !fits {
            return None;
        }
        bytes.shrink_to_fit();
        Some(TermTable { bytes, ends, shared })
    }

    /// The number of terms.
    pub fn len(&self) -> usize {
        self.ends.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ends.is_empty()
    }

    /// Bytes the table holds in memory.
    pub fn byte_len(&self) -> usize {
        self.bytes.capacity() + 4 * self.ends.capacity() + self.shared.capacity()
    }

    /// Term `i` (its rank in the dictionary).
    #[inline]
    pub fn term(&self, i: usize) -> &[u8] {
        let start = if i == 0 { 0 } else { self.ends[i - 1] as usize };
        &self.bytes[start..self.ends[i] as usize]
    }

    /// The ranks of the terms within `edits` edits of `term` (itself
    /// included), with their distances, in order: the same terms as
    /// [`Dict::fuzzy`] finds in the dictionary the table was built from.
    pub fn fuzzy(&self, term: &str, edits: u8) -> Vec<(u32, u8)> {
        let q: Vec<char> = term.chars().collect();
        let mut out = Vec::new();
        super::fuzzy::walk(&q, edits, Match::Whole, &mut self.cursor(), |c, _, d| out.push((c.pos() as u32, d)));
        out
    }

    /// A cursor on the first term.
    pub fn cursor(&self) -> TableCursor<'_> {
        TableCursor { table: self, pos: 0 }
    }
}

/// The terms of a [`TermTable`] in order, for [`super::fuzzy::walk`].
pub struct TableCursor<'t> {
    table: &'t TermTable,
    pos: usize,
}

/// How many terms past a dead branch [`TableCursor::skip_prefix`] reads
/// one by one before it searches: most of the walk's jumps pass fewer.
const SCAN: usize = 8;

impl Sorted for TableCursor<'_> {
    #[inline]
    fn term(&self) -> Option<&[u8]> {
        (self.pos < self.table.len()).then(|| self.table.term(self.pos))
    }

    #[inline]
    fn pos(&self) -> usize {
        self.pos
    }

    #[inline]
    fn next(&mut self) {
        self.pos = (self.pos + 1).min(self.table.len());
    }

    #[inline]
    fn shared_with_previous(&self) -> Option<usize> {
        self.table.shared.get(self.pos).filter(|&&s| s < LONG).map(|&s| s as usize)
    }

    fn seek(&mut self, t: &[u8]) {
        let (table, n) = (self.table, self.table.len());
        let (mut lo, mut hi) = (self.pos.min(n), n);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if table.term(mid) < t {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        self.pos = lo;
    }

    fn skip_prefix(&mut self, len: usize) {
        // The terms starting with the current term's first `len` bytes are
        // the run from it, each sharing `len` bytes or more with the one
        // before: the run ends at the first sharing fewer.
        let (table, n) = (self.table, self.table.len());
        if self.pos >= n {
            return;
        }
        let mut i = self.pos + 1;
        if len < LONG as usize {
            let len = len as u8;
            while let Some(chunk) = table.shared.get(i..(i + 32).min(n)).filter(|c| !c.is_empty()) {
                match chunk.iter().position(|&s| s < len) {
                    Some(j) => {
                        self.pos = i + j;
                        return;
                    }
                    None => i += chunk.len(),
                }
            }
            self.pos = n;
            return;
        }
        // A prefix this long: compare, the first few one by one, then search.
        let p = &table.term(self.pos)[..len];
        let scan_end = (i + SCAN).min(n);
        while i < scan_end && table.term(i).starts_with(p) {
            i += 1;
        }
        if i == scan_end && i < n {
            let (mut lo, mut hi) = (i, n);
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                if table.term(mid).starts_with(p) {
                    lo = mid + 1;
                } else {
                    hi = mid;
                }
            }
            i = lo;
        }
        self.pos = i;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::postings::bytes::OwnedBytes;
    use crate::text::dict::DictBuilder;

    fn dict(terms: &[String]) -> Dict {
        let mut b = DictBuilder::new();
        for t in terms {
            b.add(t.as_bytes(), 1, 1, 0);
        }
        Dict::open(OwnedBytes::new(b.finish())).unwrap()
    }

    /// The table walk finds the dictionary walk's terms, for random
    /// dictionaries across many blocks, with runs longer than the scan.
    #[test]
    fn table_walks_equal_dictionary_walks() {
        let mut x = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        let letters: Vec<char> = "abcdeé".chars().collect();
        let word = |next: &mut dyn FnMut() -> u64, max: u64| -> String { (0..1 + next() % max).map(|_| letters[(next() % letters.len() as u64) as usize]).collect() };
        for round in 0..12 {
            let mut terms: Vec<String> = (0..2000).map(|_| word(&mut next, 8)).collect();
            if round % 3 == 0 {
                // A long run of terms under one prefix.
                terms.extend((0..200).map(|i| format!("abab{i}")));
            }
            terms.sort();
            terms.dedup();
            let d = dict(&terms);
            let t = TermTable::of(&d).unwrap();
            assert_eq!(t.len(), terms.len());
            for (i, w) in terms.iter().enumerate() {
                assert_eq!(t.term(i), w.as_bytes());
            }
            for _ in 0..30 {
                let q = word(&mut next, 6);
                for k in 0..=2 {
                    let want: Vec<(u32, u8)> = d.fuzzy(&q, k).into_iter().map(|(_, i, e)| (i.ord, e)).collect();
                    assert_eq!(t.fuzzy(&q, k), want, "{q:?} within {k}");
                }
            }
        }
    }

    #[test]
    fn empty_dictionary() {
        let d = dict(&[]);
        let t = TermTable::of(&d).unwrap();
        assert!(t.is_empty());
        assert!(t.fuzzy("abc", 2).is_empty());
    }
}
