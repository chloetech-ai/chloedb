//! Typo tolerance: the edit distance between a query term and the indexed
//! terms, counting an insertion, a deletion, a substitution or a swap of two
//! neighbouring letters as one edit (optimal string alignment, the
//! Damerau-Levenshtein distance most spelling tools use).

/// The distance between `a` and `b` if it is at most `k`.
pub fn within(a: &[char], b: &str, k: u8) -> Option<u8> {
    let k = k as usize;
    let b: Vec<char> = b.chars().collect();
    let (n, m) = (a.len(), b.len());
    if n.abs_diff(m) > k {
        return None;
    }
    // Three rows of the dynamic programme: i-2, i-1 and i.
    let mut prev2 = vec![0usize; m + 1];
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut cur = vec![0usize; m + 1];
    for i in 1..=n {
        cur[0] = i;
        let mut row_min = cur[0];
        for j in 1..=m {
            let cost = (a[i - 1] != b[j - 1]) as usize;
            let mut d = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d = d.min(prev2[j - 2] + 1);
            }
            cur[j] = d;
            row_min = row_min.min(d);
        }
        if row_min > k {
            return None;
        }
        std::mem::swap(&mut prev2, &mut prev);
        std::mem::swap(&mut prev, &mut cur);
    }
    (prev[m] <= k).then_some(prev[m] as u8)
}

/// The weight of a term `edits` away from a query term of `len` letters in
/// a ranked query, as Lucene's fuzzy queries boost theirs: `1 − edits / len`.
pub fn weight(edits: u8, len: usize) -> f32 {
    if edits == 0 {
        return 1.0;
    }
    (1.0 - edits as f32 / len.max(1) as f32).max(0.1)
}

/// A word that `fuzzy => 'auto'` takes as spelled right, and does not
/// correct: one in at least 1 in [`KNOWN_WORD`] of the `n_docs` documents,
/// and in 2 or more. Typos are rare words, so this skips the dictionary
/// walk for most words of a query, and keeps them from adding the noise of
/// their neighbours to a query spelled right.
pub fn known(df: u64, n_docs: u64) -> bool {
    df >= 2 && df.saturating_mul(KNOWN_WORD) >= n_docs
}

/// See [`known`].
pub const KNOWN_WORD: u64 = 10_000;

/// The chance of each edit of a typo, in [`meant`].
pub const EDIT_CHANCE: f64 = 0.01;

/// The chance that a word in `df` documents was typed for a term `edits`
/// away in `near_df`: a noisy channel's, between the two, taking each as
/// likely as its documents and each edit as a slip of [`EDIT_CHANCE`].
/// A word absent from the index is a typo of each of its neighbours
/// (chance 1); a word in as many documents as its neighbour is almost
/// surely meant as written.
pub fn meant(df: u64, near_df: u64, edits: u8) -> f32 {
    let x = near_df as f64 * EDIT_CHANCE.powi(edits as i32);
    if x <= 0.0 {
        return 0.0;
    }
    (x / (x + df as f64)) as f32
}

/// Terms in byte order, read one after another, that can jump ahead: a
/// sorted dictionary seen as an implicit trie, whose nodes are the runs of
/// terms sharing a prefix ([`walk`]).
pub trait Sorted {
    /// The current term, `None` past the last.
    fn term(&self) -> Option<&[u8]>;
    /// The current term's position: the number of terms before it.
    fn pos(&self) -> usize;
    /// Moves to the next term.
    fn next(&mut self);
    /// Moves to the first term at or after `t` (never back).
    fn seek(&mut self, t: &[u8]);
    /// Moves past every term starting with the current term's first `len`
    /// bytes: to the first term after them, or past the last. A walk
    /// leaves a dead branch of the trie this way, mostly by a few terms,
    /// so a cursor that can tell where the run ends without a seek
    /// (front-coded blocks: an entry sharing fewer than `len` bytes with
    /// the previous) overrides it.
    fn skip_prefix(&mut self, len: usize) {
        let Some(t) = self.term() else { return };
        match past_prefix(&t[..len]) {
            Some(after) => self.seek(&after),
            None => skip_rest(self),
        }
    }
    /// The bytes the current term shares with the one before it, when the
    /// cursor knows them without comparing (`None` otherwise): a walk
    /// keeps its path's rows by them.
    fn shared_with_previous(&self) -> Option<usize> {
        None
    }
}

/// Sorted terms in a slice.
pub struct SliceTerms<'a, T: AsRef<[u8]>> {
    terms: &'a [T],
    pos: usize,
}

impl<'a, T: AsRef<[u8]>> SliceTerms<'a, T> {
    /// A cursor on the first of `terms`, which are in byte order.
    pub fn new(terms: &'a [T]) -> Self {
        SliceTerms { terms, pos: 0 }
    }
}

impl<T: AsRef<[u8]>> Sorted for SliceTerms<'_, T> {
    fn term(&self) -> Option<&[u8]> {
        self.terms.get(self.pos).map(|t| t.as_ref())
    }

    fn pos(&self) -> usize {
        self.pos
    }

    fn next(&mut self) {
        self.pos = (self.pos + 1).min(self.terms.len());
    }

    fn seek(&mut self, t: &[u8]) {
        self.pos += self.terms[self.pos.min(self.terms.len())..].partition_point(|x| x.as_ref() < t);
    }
}

/// The first byte string after every string starting with `p`, or `None`
/// when there is none (`p` is all `0xff`).
pub fn past_prefix(p: &[u8]) -> Option<Vec<u8>> {
    let mut s = p.to_vec();
    while let Some(last) = s.pop() {
        if last < 0xff {
            s.push(last + 1);
            return Some(s);
        }
    }
    None
}

/// What [`walk`] matches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Match {
    /// Terms within `k` edits of the query, as [`within`] decides.
    Whole,
    /// Terms with a prefix within `k` edits of the query, whose first
    /// letter is the query's: a typed prefix with typos.
    Prefix,
}

/// Calls `hit(terms, positions, edits)` for the terms that `m` matches
/// against `q` within `k` edits, in order: each matching term for
/// [`Match::Whole`] (`terms` still on it, `positions` its own), each run
/// of terms starting with a prefix within `k` edits for [`Match::Prefix`]
/// (`terms` past it), with the fewest edits of any prefix of those terms:
/// a term starting with `q` itself has 0, whatever shorter prefix is also
/// within `k`, so exact completions can be told from typo ones.
///
/// The dictionary is walked as a trie: one row of the edit-distance table
/// per letter of the path from the root, kept while the next term shares
/// that path, so a term costs only the rows of the letters after the ones
/// it shares with the previous term. A path whose row has no cell within
/// `k` cannot lead to a match, so the walk jumps past every term starting
/// with it, and a prefix match jumps past its run: the cost follows the
/// trie's nodes within `k` edits of `q`, not the dictionary's size. A
/// prefix match goes on down while a longer prefix could have fewer
/// edits (the row's least cell is below the best edits so far), and jumps
/// past the run once none can.
///
/// A row is `k + 1` bit masks, one per number of edits, of the prefixes
/// of `q` within that many ([`BitRows`]) when `q` has fewer than 64
/// letters, else its cells ([`CellRows`]).
pub fn walk<S: Sorted>(q: &[char], k: u8, m: Match, terms: &mut S, hit: impl FnMut(&S, std::ops::Range<usize>, u8)) {
    if q.len() < 64 {
        walk_rows(BitRows::new(q, k), q, k, m, terms, hit)
    } else {
        walk_rows(CellRows::new(q, k), q, k, m, terms, hit)
    }
}

/// The rows of the edit-distance table along a walk's path: per depth
/// `j`, the edits from the path's first `j` letters to each prefix of the
/// query, capped at `k + 1` (all that matters past `k`).
trait Rows {
    /// Keeps the rows of the path's first `depth` letters (and the empty
    /// path's).
    fn truncate(&mut self, depth: usize);
    /// Adds the row of letter `c` after the path's last letter `last`
    /// (`None` at the root): its least cell, and its edits to the whole
    /// query.
    fn push(&mut self, c: char, last: Option<char>) -> (u8, u8);
    /// The edits from the path's first `depth` letters to the whole query.
    fn whole(&self, depth: usize) -> u8;
}

/// Rows of `q.len() + 1` cells.
struct CellRows<'q> {
    q: &'q [char],
    k: u8,
    rows: Vec<u8>,
}

impl<'q> CellRows<'q> {
    fn new(q: &'q [char], k: u8) -> Self {
        CellRows { q, k, rows: (0..=q.len()).map(|i| i.min(k as usize + 1) as u8).collect() }
    }
}

impl Rows for CellRows<'_> {
    fn truncate(&mut self, depth: usize) {
        self.rows.truncate((depth + 1) * (self.q.len() + 1));
    }

    fn push(&mut self, c: char, last: Option<char>) -> (u8, u8) {
        let (q, n, cap) = (self.q, self.q.len(), self.k + 1);
        let w = n + 1;
        let rows = &mut self.rows;
        let d = rows.len() / w - 1;
        let (at, prev) = (rows.len(), rows.len() - w);
        rows.push((d + 1).min(cap as usize) as u8);
        let mut min = rows[at];
        for i in 1..=n {
            let mut x = (rows[prev + i] + 1).min(rows[at + i - 1] + 1).min(rows[prev + i - 1] + (q[i - 1] != c) as u8);
            // A swap of the path's last letter and this one.
            if i >= 2 && last == Some(q[i - 1]) && q[i - 2] == c {
                x = x.min(rows[prev - w + i - 2] + 1);
            }
            let x = x.min(cap);
            rows.push(x);
            min = min.min(x);
        }
        (min, rows[at + n])
    }

    fn whole(&self, depth: usize) -> u8 {
        let w = self.q.len() + 1;
        self.rows[depth * w + w - 1]
    }
}

/// Rows of a query of fewer than 64 letters as the states of its
/// Levenshtein automaton, simulated with bit masks (Wu and Manber's
/// shift-and, with Hyyrö's swap of neighbouring letters): per depth, for
/// each `e` in `0..=k`, the mask of the `i` (bit `i`) whose prefix
/// `q[..i]` is within `e` edits of the path, then the mask of the query's
/// letters equal to the path's last letter. A letter costs `k + 1` words
/// of shifts and ors rather than a cell per letter of the query.
struct BitRows {
    k: usize,
    n: usize,
    /// The query's positions `i` (bit `i`, from 1) whose letter `q[i - 1]`
    /// is an ASCII byte, by byte; other letters' masks follow in `other`.
    ascii: [u64; 128],
    other: Vec<(char, u64)>,
    /// Bits `0..=n`.
    full: u64,
    /// Per depth, `k + 2` words: the masks for 0 to `k` edits, then the
    /// letter's mask.
    rows: Vec<u64>,
}

impl BitRows {
    fn new(q: &[char], k: u8) -> Self {
        let (k, n) = (k as usize, q.len());
        let mut ascii = [0u64; 128];
        let mut other: Vec<(char, u64)> = Vec::new();
        for (i, &c) in q.iter().enumerate() {
            let bit = 1u64 << (i + 1);
            if c.is_ascii() {
                ascii[c as usize] |= bit;
            } else {
                match other.iter_mut().find(|(x, _)| *x == c) {
                    Some((_, m)) => *m |= bit,
                    None => other.push((c, bit)),
                }
            }
        }
        let full = if n + 1 >= 64 { u64::MAX } else { (1u64 << (n + 1)) - 1 };
        // The empty path: q[..i] is within e edits for i <= e.
        let mut rows: Vec<u64> = (0..=k).map(|e| if e >= n { full } else { (1u64 << (e + 1)) - 1 }).collect();
        rows.push(0);
        BitRows { k, n, ascii, other, full, rows }
    }

    #[inline]
    fn mask(&self, c: char) -> u64 {
        if c.is_ascii() {
            self.ascii[c as usize]
        } else {
            self.other.iter().find(|(x, _)| *x == c).map_or(0, |x| x.1)
        }
    }

    /// The fewest edits whose mask in `row` has a bit of `bits`, `k + 1`
    /// for none.
    #[inline]
    fn least(&self, row: &[u64], bits: u64) -> u8 {
        row[..=self.k].iter().position(|&r| r & bits != 0).unwrap_or(self.k + 1) as u8
    }
}

impl Rows for BitRows {
    fn truncate(&mut self, depth: usize) {
        self.rows.truncate((depth + 1) * (self.k + 2));
    }

    fn push(&mut self, c: char, _last: Option<char>) -> (u8, u8) {
        let (k, s) = (self.k, self.k + 2);
        let mc = self.mask(c);
        let at = self.rows.len();
        let prev = at - s;
        // A swap of the path's last letter (whose mask ends the previous
        // row) and this one: from q[..i - 2] two letters up to q[..i],
        // where q[i - 2] is this letter and q[i - 1] the last.
        let swap = if prev >= s { (mc << 1) & self.rows[prev + k + 1] } else { 0 };
        let mut lower = 0u64;
        for e in 0..=k {
            // A match of c with q[i - 1]; with one more edit, c deleted, c
            // for q[i - 1], q[i - 1] inserted, or the swap.
            let mut r = (self.rows[prev + e] << 1) & mc;
            if e > 0 {
                let p = self.rows[prev + e - 1];
                r |= p | (p << 1) | (lower << 1);
                if swap != 0 {
                    r |= (self.rows[prev - s + e - 1] << 2) & swap;
                }
            }
            r &= self.full;
            self.rows.push(r);
            lower = r;
        }
        self.rows.push(mc);
        let row = &self.rows[at..at + k + 1];
        (self.least(row, self.full), self.least(row, 1u64 << self.n))
    }

    fn whole(&self, depth: usize) -> u8 {
        let at = depth * (self.k + 2);
        self.least(&self.rows[at..at + self.k + 1], 1u64 << self.n)
    }
}

fn walk_rows<S: Sorted, R: Rows>(mut rows: R, q: &[char], k: u8, m: Match, terms: &mut S, mut hit: impl FnMut(&S, std::ops::Range<usize>, u8)) {
    let (n, k) = (q.len(), k as usize);
    let cap = |x: usize| x.min(k + 1) as u8;
    // The path's letters and their ends in bytes.
    let mut path: Vec<char> = Vec::new();
    let mut bytes: Vec<u8> = Vec::new();
    let mut ends: Vec<usize> = vec![0];
    // Per depth j, the fewest edits from a prefix of the path's first j
    // letters to `q` (k + 1: none within k), for a prefix match.
    let mut bests: Vec<u8> = vec![cap(n)];
    let first = match (m, q.first()) {
        (Match::Whole, _) => None,
        (Match::Prefix, None) => {
            // The empty prefix: every term.
            let start = terms.pos();
            skip_rest(terms);
            if terms.pos() > start {
                hit(terms, start..terms.pos(), 0);
            }
            return;
        }
        (Match::Prefix, Some(&c)) => {
            let first = c.encode_utf8(&mut [0; 4]).as_bytes().to_vec();
            terms.seek(&first);
            Some(first)
        }
    };
    while let Some(t) = terms.term() {
        if first.as_ref().is_some_and(|f| !t.starts_with(f)) {
            return;
        }
        // Keep the rows of the letters the path shares with this term. The
        // path starts the term before this one (the walk read it, or jumped
        // past the terms starting with the path), so it shares with this
        // term what the two terms share, up to its length.
        let shared = match terms.shared_with_previous() {
            Some(s) => s.min(bytes.len()),
            None => bytes.iter().zip(t).take_while(|(a, b)| a == b).count(),
        };
        // The letters within those bytes: as many when the path is ASCII.
        let j = if bytes.len() == path.len() { shared } else { ends.partition_point(|&e| e <= shared) - 1 };
        path.truncate(j);
        ends.truncate(j + 1);
        bests.truncate(j + 1);
        rows.truncate(j);
        bytes.truncate(ends[j]);
        // Past: no term starting with the path can match. Run: every term
        // starting with it does, with these edits. The letters are decoded
        // as the walk reads them, mostly few before it jumps.
        let (mut past, mut run, mut text) = (false, None, true);
        let mut at = ends[j];
        while at < t.len() {
            let Some((c, len)) = letter(&t[at..]) else {
                // Not text: no letters to match.
                text = false;
                break;
            };
            let d = path.len();
            let (min, whole) = rows.push(c, path.last().copied());
            let best = bests[d].min(whole);
            let min = min as usize;
            path.push(c);
            bytes.extend_from_slice(&t[at..at + len]);
            at += len;
            ends.push(bytes.len());
            bests.push(best);
            if m == Match::Prefix && best as usize <= k && min >= best as usize {
                // No longer prefix has fewer edits: every term starting
                // with the path matches with these.
                run = Some(best);
                break;
            }
            if min > k {
                past = true;
                break;
            }
        }
        if !text {
            terms.next();
            continue;
        }
        if !past && run.is_none() {
            // The whole term was read.
            let d = match m {
                Match::Whole => rows.whole(path.len()),
                Match::Prefix => bests[path.len()],
            };
            if d as usize <= k {
                let p = terms.pos();
                hit(terms, p..p + 1, d);
            }
            terms.next();
            continue;
        }
        // The path's bytes start the current term.
        let start = terms.pos();
        terms.skip_prefix(bytes.len());
        if let Some(d) = run {
            hit(terms, start..terms.pos(), d);
        }
    }
}

/// The letter `b` starts with and its length in bytes, `None` when `b`
/// does not start with one in UTF-8.
#[inline]
fn letter(b: &[u8]) -> Option<(char, usize)> {
    let x = *b.first()?;
    if x < 0x80 {
        return Some((x as char, 1));
    }
    let len = match x {
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return None,
    };
    let c = std::str::from_utf8(b.get(..len)?).ok()?.chars().next()?;
    Some((c, len))
}

fn skip_rest<S: Sorted + ?Sized>(terms: &mut S) {
    while terms.term().is_some() {
        terms.next();
    }
}

/// Whether `v` starts with a prefix within `k` edits of `q` whose first
/// letter is `q`'s ([`Match::Prefix`] for one value).
pub fn prefix_within(q: &[char], v: &str, k: u8) -> bool {
    let mut found = false;
    walk(q, k, Match::Prefix, &mut SliceTerms::new(&[v]), |_, _, _| found = true);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(a: &str, b: &str, k: u8) -> Option<u8> {
        within(&a.chars().collect::<Vec<_>>(), b, k)
    }

    #[test]
    fn distances() {
        assert_eq!(d("neural", "neural", 2), Some(0));
        assert_eq!(d("nueral", "neural", 2), Some(1)); // a swap
        assert_eq!(d("netwrk", "network", 2), Some(1));
        assert_eq!(d("grpah", "graph", 1), Some(1));
        assert_eq!(d("kitten", "sitting", 2), None);
        assert_eq!(d("kitten", "sitting", 3), Some(3));
        assert_eq!(d("ab", "abcd", 1), None);
        assert_eq!(d("", "ab", 2), Some(2));
    }

    /// Every term of a random dictionary, in order, that the walk matches
    /// is the full scan's, with its distance; and prefix runs are exactly
    /// the terms with a prefix within k.
    #[test]
    fn walks_equal_full_scans() {
        let mut x = 0x2545_f491_4f6c_dd1du64;
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        let letters: Vec<char> = "abcdeé日".chars().collect();
        let word = |next: &mut dyn FnMut() -> u64, max: u64| -> String { (0..next() % max).map(|_| letters[(next() % letters.len() as u64) as usize]).collect() };
        for round in 0..40 {
            let mut terms: Vec<String> = (0..300).map(|_| word(&mut next, 7)).collect();
            if round % 5 == 0 {
                terms.push(String::from_utf8_lossy(&[0xffu8, 0x61]).into_owned());
            }
            terms.sort();
            terms.dedup();
            for _ in 0..20 {
                let q: Vec<char> = word(&mut next, 6).chars().collect();
                for k in 0..=2u8 {
                    let want: Vec<(usize, u8)> = terms.iter().enumerate().filter_map(|(i, t)| within(&q, t, k).map(|d| (i, d))).collect();
                    let mut got = Vec::new();
                    walk(&q, k, Match::Whole, &mut SliceTerms::new(&terms), |_, r, d| got.push((r.start, d)));
                    assert_eq!(got, want, "{q:?} within {k}");
                    // The rows of cells (a query of 64 letters or more) find the same.
                    let mut cells = Vec::new();
                    walk_rows(CellRows::new(&q, k), &q, k, Match::Whole, &mut SliceTerms::new(&terms), |_, r, d| cells.push((r.start, d)));
                    assert_eq!(cells, want, "{q:?} within {k}, by cells");
                    // Each term with a prefix within k, and the fewest
                    // edits of its prefixes.
                    let want: Vec<(usize, u8)> = (0..terms.len())
                        .filter_map(|i| {
                            let t: Vec<char> = terms[i].chars().collect();
                            if t.first() != q.first() {
                                return None;
                            }
                            (1..=t.len()).filter_map(|j| within(&q, &t[..j].iter().collect::<String>(), k)).min().map(|d| (i, d))
                        })
                        .collect();
                    let mut got = Vec::new();
                    walk(&q, k, Match::Prefix, &mut SliceTerms::new(&terms), |_, r, d| got.extend(r.map(|i| (i, d))));
                    let want = if q.is_empty() { (0..terms.len()).map(|i| (i, 0)).collect() } else { want };
                    assert_eq!(got, want, "{q:?} prefix within {k}");
                    let mut cells = Vec::new();
                    walk_rows(CellRows::new(&q, k), &q, k, Match::Prefix, &mut SliceTerms::new(&terms), |_, r, d| cells.extend(r.map(|i| (i, d))));
                    assert_eq!(cells, want, "{q:?} prefix within {k}, by cells");
                }
            }
        }
        // Queries of 63 letters (the most a row of bits holds) and of 64
        // (rows of cells), against terms near them.
        for n in [63, 64, 70] {
            let q: Vec<char> = (0..n).map(|i| letters[i % 5]).collect();
            let base: String = q.iter().collect();
            let mut terms: Vec<String> = vec![base.clone(), base[1..].to_string(), format!("{base}x"), format!("x{}", &base[2..]), base[..n - 3].to_string()];
            let mut swapped = q.clone();
            swapped.swap(10, 11);
            terms.push(swapped.iter().collect());
            terms.sort();
            terms.dedup();
            for k in 0..=2u8 {
                let want: Vec<(usize, u8)> = terms.iter().enumerate().filter_map(|(i, t)| within(&q, t, k).map(|d| (i, d))).collect();
                let mut got = Vec::new();
                walk(&q, k, Match::Whole, &mut SliceTerms::new(&terms), |_, r, d| got.push((r.start, d)));
                assert_eq!(got, want, "{n} letters within {k}");
            }
        }
        assert!(prefix_within(&"hwo to ins".chars().collect::<Vec<_>>(), "how to install engram", 2));
        assert!(!prefix_within(&"wo to".chars().collect::<Vec<_>>(), "how to install", 2));
        // A completion of the prefix itself has no edits, though a shorter
        // prefix is within 2 too (IR-N1 of the review of 31 October).
        let terms = ["keyb", "keyboard", "keyboards", "keybx", "keybxyz", "keyborad"];
        let mut terms = terms.to_vec();
        terms.sort();
        let mut got = Vec::new();
        walk(&"keyboa".chars().collect::<Vec<_>>(), 2, Match::Prefix, &mut SliceTerms::new(&terms), |_, r, d| got.extend(r.map(|i| (terms[i], d))));
        got.sort();
        assert_eq!(got, [("keyb", 2), ("keyboard", 0), ("keyboards", 0), ("keyborad", 1), ("keybx", 2), ("keybxyz", 2)]);
        assert_eq!(past_prefix(b"ab"), Some(b"ac".to_vec()));
        assert_eq!(past_prefix(&[0x61, 0xff]), Some(b"b".to_vec()));
        assert_eq!(past_prefix(&[0xff]), None);
    }
}
