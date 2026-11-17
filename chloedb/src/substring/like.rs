//! `LIKE` and `ILIKE` patterns: what a value must be to match, and which
//! byte strings a substring index is searched for to find the values that
//! may.
//!
//! The answer is defined by [`Like::matches`], which is the row
//! evaluator's `LIKE` ([`crate::query::ir::eval::like`]): `%` any run of
//! characters, `_` one character, and for `ILIKE` both sides lowercased
//! first. The index only narrows the values that are tested, or, for a
//! pattern of one literal, names them outright: `ILIKE` folds a character
//! at a time, so a literal's spellings are the case variants of each of
//! its characters, which the index follows all of.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use crate::error::Result;
use crate::segment::{DocId, DocSet};

use super::read::Part;
use super::{SubstringIndex, SEP};

/// A pattern's pieces.
#[derive(Clone, Debug, PartialEq)]
enum Tok {
    /// Literal text.
    Lit(String),
    /// `_`: one character.
    One,
    /// `%`: any run of characters.
    Any,
}

/// What a case-sensitive pattern of at most one literal asks of a value.
#[derive(Clone, Debug, PartialEq)]
enum Simple {
    Equals(String),
    Starts(String),
    Ends(String),
    Contains(String),
}

/// A `LIKE` (or, with `ci`, `ILIKE`) pattern.
#[derive(Clone, Debug)]
pub struct Like {
    ci: bool,
    /// The pattern's characters, lowercased for `ILIKE`.
    chars: Vec<char>,
    toks: Vec<Tok>,
    simple: Option<Simple>,
    /// The same shapes of an `ILIKE` pattern, for [`Like::matches`] alone:
    /// the index still looks at the candidates' values.
    folded: Option<Simple>,
}

/// How a part of a substring index answers a pattern.
#[derive(Clone, Debug, PartialEq)]
pub enum Plan {
    /// Every value matches.
    All,
    /// The values in which these bytes occur are the matches.
    Exact(Vec<u8>),
    /// The values in which one of the piece's spellings occurs are the
    /// matches (an `ILIKE` of one literal, which folds each character
    /// alone): where the search follows every one of them, no value is
    /// read.
    Spelled(Piece),
    /// A value that matches holds every piece; a value that holds them is
    /// tested with [`Like::matches`]. Without pieces, every value is.
    Pieces(Vec<Piece>),
}

/// A string a matching value holds: one of the alternatives of each unit,
/// one unit after another.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub units: Vec<Vec<Vec<u8>>>,
}

impl Piece {
    fn bytes(b: Vec<u8>) -> Piece {
        Piece { units: vec![vec![b]] }
    }
}

/// For lowercase character `c`, the other characters whose lowercase starts
/// with it, and the characters that are a later part of some character's
/// lowercase (the dot above of `İ`, whose lowercase is `i` and the dot).
struct Folds {
    sources: HashMap<char, Vec<char>>,
    later: HashSet<char>,
}

fn folds() -> &'static Folds {
    static FOLDS: OnceLock<Folds> = OnceLock::new();
    FOLDS.get_or_init(|| {
        let mut f = Folds { sources: HashMap::new(), later: HashSet::new() };
        for x in (0..=char::MAX as u32).filter_map(char::from_u32) {
            let mut lower = x.to_lowercase();
            let first = lower.next().expect("a character's lowercase");
            if first != x {
                f.sources.entry(first).or_default().push(x);
            }
            f.later.extend(lower);
        }
        // ILIKE folds a final sigma as the other sigma
        // ([`crate::query::ir::eval::fold_case`]): a pattern's `σ` is a
        // value's `ς` too.
        f.sources.entry('σ').or_default().push('ς');
        f
    })
}

impl Like {
    pub fn new(pattern: &str, ci: bool) -> Like {
        let folded = if ci { crate::query::ir::eval::fold_case(pattern) } else { pattern.to_string() };
        let chars: Vec<char> = folded.chars().collect();
        let mut toks: Vec<Tok> = Vec::new();
        for &c in &chars {
            match (c, toks.last_mut()) {
                ('%', Some(Tok::Any)) => {}
                ('%', _) => toks.push(Tok::Any),
                ('_', _) => toks.push(Tok::One),
                (c, Some(Tok::Lit(s))) => s.push(c),
                (c, _) => toks.push(Tok::Lit(c.to_string())),
            }
        }
        let shape = match &toks[..] {
            [] => Some(Simple::Equals(String::new())),
            [Tok::Lit(a)] => Some(Simple::Equals(a.clone())),
            [Tok::Lit(a), Tok::Any] => Some(Simple::Starts(a.clone())),
            [Tok::Any, Tok::Lit(a)] => Some(Simple::Ends(a.clone())),
            [Tok::Any, Tok::Lit(a), Tok::Any] => Some(Simple::Contains(a.clone())),
            _ => None,
        };
        let (simple, folded) = if ci { (None, shape) } else { (shape, None) };
        Like { ci, chars, toks, simple, folded }
    }

    /// Whether `value` matches.
    pub fn matches(&self, value: &str) -> bool {
        if self.every_value() {
            return true;
        }
        // ILIKE compares lowercase with lowercase, as the row evaluator
        // does; ASCII's lowercase needs no table.
        let lower;
        let (value, shape) = match self.ci {
            true => {
                lower = if value.is_ascii() { value.to_ascii_lowercase() } else { crate::query::ir::eval::fold_case(value) };
                (lower.as_str(), &self.folded)
            }
            false => (value, &self.simple),
        };
        match shape {
            Some(Simple::Equals(a)) => value == a,
            Some(Simple::Starts(a)) => value.starts_with(a.as_str()),
            Some(Simple::Ends(a)) => value.ends_with(a.as_str()),
            Some(Simple::Contains(a)) => value.contains(a.as_str()),
            None => crate::query::ir::eval::like(&value.chars().collect::<Vec<_>>(), &self.chars),
        }
    }

    /// Whether the pattern is tested by the row evaluator's rule (`_`, or
    /// more than one literal), not by a search of one literal in the value.
    pub fn general(&self) -> bool {
        self.simple.is_none() && self.folded.is_none() && !self.every_value()
    }

    /// Whether every value matches (`%`).
    pub fn every_value(&self) -> bool {
        self.toks == [Tok::Any]
    }

    /// How an index of the values' bytes answers the pattern.
    pub fn plan(&self) -> Plan {
        if self.every_value() {
            return Plan::All;
        }
        let around = |a: &str, before: bool, after: bool| {
            let mut b = Vec::with_capacity(a.len() + 2);
            b.extend(before.then_some(SEP));
            b.extend_from_slice(a.as_bytes());
            b.extend(after.then_some(SEP));
            b
        };
        match &self.simple {
            Some(Simple::Equals(a)) => return Plan::Exact(around(a, true, true)),
            Some(Simple::Starts(a)) => return Plan::Exact(around(a, true, false)),
            Some(Simple::Ends(a)) => return Plan::Exact(around(a, false, true)),
            Some(Simple::Contains(a)) => return Plan::Exact(around(a, false, false)),
            None => {}
        }
        // The empty pattern, in any case: the empty value alone.
        let Some(last) = self.toks.len().checked_sub(1) else { return Plan::Exact(around("", true, true)) };
        let mut pieces = Vec::new();
        for (i, t) in self.toks.iter().enumerate() {
            let Tok::Lit(a) = t else { continue };
            if !self.ci {
                pieces.push(Piece::bytes(around(a, i == 0, i == last)));
                continue;
            }
            // A value's text matches the lowercase literal where each of
            // its characters is one whose lowercase starts with the
            // literal's. The later parts of a lowercase of several
            // characters have no character of their own: the literal is
            // cut there.
            let f = folds();
            let chars: Vec<char> = a.chars().collect();
            let mut units: Vec<Vec<Vec<u8>>> = Vec::new();
            let mut start = i == 0;
            for (j, &c) in chars.iter().enumerate() {
                if f.later.contains(&c) {
                    if !units.is_empty() {
                        pieces.push(Piece { units: std::mem::take(&mut units) });
                    }
                    start = false;
                    continue;
                }
                if units.is_empty() && start && j == 0 {
                    units.push(vec![vec![SEP]]);
                }
                let mut alts = vec![c.to_string().into_bytes()];
                alts.extend(f.sources.get(&c).into_iter().flatten().map(|x| x.to_string().into_bytes()));
                units.push(alts);
            }
            if !units.is_empty() {
                if i == last {
                    units.push(vec![vec![SEP]]);
                }
                pieces.push(Piece { units });
            }
        }
        // One literal, whole (no cut) and folded already (each of its
        // characters its own lowercase): ILIKE folds a value a character
        // at a time to one character each ([`crate::query::ir::eval::fold_case`]),
        // so a value matches where a run of its characters folds to the
        // literal's, which is where one of the spellings the units list
        // occurs. A capital İ folds to an i: it is one of the i's spellings
        // anywhere in the literal.
        if let (Some(Simple::Contains(a) | Simple::Starts(a) | Simple::Ends(a) | Simple::Equals(a)), [_]) = (&self.folded, &pieces[..]) {
            let whole = pieces[0].units.iter().filter(|u| u[..] != [vec![SEP]]).count() == a.chars().count();
            if whole && a.chars().all(|c| crate::query::ir::eval::fold_case(&c.to_string()).chars().eq([c])) {
                return Plan::Spelled(pieces.pop().expect("one piece"));
            }
        }
        Plan::Pieces(pieces)
    }
}

/// The rows of `part` whose suffixes start with `piece`, as ranges, and
/// whether the whole piece was searched. Past `max_ranges` alternatives
/// still alive, the rest of the piece is left unread: the ranges then hold
/// more rows, of suffixes that start with the piece's end only.
pub fn rows(part: &Part, piece: &Piece, max_ranges: usize) -> Result<(Vec<(u64, u64)>, bool)> {
    let mut ranges = vec![part.all()];
    for (i, unit) in piece.units.iter().enumerate().rev() {
        let mut next = Vec::new();
        for &r in &ranges {
            for alt in unit {
                let e = part.extend(r, alt)?;
                if e.0 < e.1 {
                    next.push(e);
                }
            }
        }
        if next.len() > max_ranges && i + 1 < piece.units.len() {
            return Ok((ranges, false));
        }
        ranges = next;
        if ranges.is_empty() {
            break;
        }
    }
    Ok((ranges, true))
}

/// How a pattern was answered in a segment, for a query's plan.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    /// Parts answered from the index alone.
    pub exact: u32,
    /// Parts whose candidates were tested against their values.
    pub tested: u32,
    /// Parts scanned: too many occurrences to list, or no literal.
    pub scanned: u32,
    /// Rows of the index walked to a sample.
    pub located: u64,
    /// Values read and tested.
    pub read: u64,
}

/// Bytes of a segment's stored rows that a scan reads and tests in the time
/// a candidate row costs to walk to its sample and have its value read and
/// tested: a pattern with more candidates than one per this many bytes is
/// answered by a scan of the part. A row the index answers alone
/// ([`Plan::Exact`], [`Plan::Spelled`]) reads no value and costs
/// [`EXACT_ROWS`] times less; a pattern the row evaluator tests (`_`, or
/// several literals) costs [`GENERAL_SCAN`] times as much a byte to scan.
/// [`locate_bytes`] puts it in bytes of the indexed text, which is what
/// [`eval`] takes.
///
/// Measured on Loghub's SSH log and on 1.2 million Hacker News titles
/// (`results/substring/costs.md`, medians): a scan reads 1.7 to 2.4 ns a
/// byte of the rows for one literal and 2.9 to 7.9 for other patterns; a
/// row costs 1.7 to 4.5 µs to walk, and a candidate 3.9 to 16.5 µs with its
/// value, its values read in docID order. 4096 and 2 pick the faster way
/// for every pattern measured there but a tie.
pub const LOCATE_BYTES: u64 = 4096;

/// How many more rows are listed when no value is read for them.
pub const EXACT_ROWS: u64 = 2;

/// How many times a byte of the rows costs more to scan for a pattern the
/// row evaluator tests than for one literal, which a value's `contains`
/// finds.
pub const GENERAL_SCAN: u64 = 2;

/// [`LOCATE_BYTES`] in bytes of `index`'s text, for a segment whose doc
/// store's rows take `stored` bytes uncompressed: a scan reads whole rows,
/// so where the field is a sixth of its rows (a title beside its link, its
/// author and its counts), a byte of it costs six to scan, and listing
/// pays up to six times as many rows.
pub fn locate_bytes(index: &SubstringIndex, stored: u64) -> u64 {
    let text = index.bytes().max(1);
    let scaled = LOCATE_BYTES as u128 * text as u128 / stored.max(text) as u128;
    (scaled as u64).max(EXACT_ROWS)
}

/// The alternatives of an `ILIKE` literal followed at most.
pub const MAX_RANGES: usize = 64;

/// The documents of a segment of `n_docs` documents whose value matches
/// `like`, from `index`; `value` reads a document's value where the index
/// alone does not say (`None` for a document without one). A part in which
/// the pattern has more candidates than one per `locate_bytes` bytes of its
/// text is scanned ([`locate_bytes`]; 0 never scans for that).
pub fn eval(index: &SubstringIndex, like: &Like, n_docs: u32, value: &mut dyn FnMut(DocId) -> Result<Option<String>>, locate_bytes: u64, stats: &mut Stats) -> Result<DocSet> {
    eval_with(index, like, n_docs, value, locate_bytes, stats, &mut || Ok(()))
}

/// [`eval`], calling `tick` once a document it tests or locates, so that a
/// query's deadline and cancel flag stop it inside a part: an error from
/// `tick` ends it with that error.
pub fn eval_with(index: &SubstringIndex, like: &Like, n_docs: u32, value: &mut dyn FnMut(DocId) -> Result<Option<String>>, locate_bytes: u64, stats: &mut Stats, tick: &mut dyn FnMut() -> Result<()>) -> Result<DocSet> {
    let mut out = Out::new(n_docs, None);
    let plan = like.plan();
    for part in index.parts() {
        tick()?;
        eval_part(part, like, &plan, value, locate_bytes, stats, &mut out, tick)?;
    }
    Ok(out.docs())
}

/// The first matches a query wants, by docID: a `LIMIT` without an
/// `ORDER BY`.
#[derive(Clone, Copy)]
pub struct First<'a> {
    /// How many.
    pub need: usize,
    /// Whether a document counts (the segment's live rows).
    pub live: &'a dyn Fn(DocId) -> bool,
}

/// [`eval_with`] for the first `first.need` matches that `first.live`
/// keeps, in docID order: it stops once it has them, and the set it
/// returns holds them and the matches before them that `live` does not
/// keep (all the matches, where there are fewer). A part's candidates are
/// listed and their documents then taken in docID order; where they are
/// so many that the first matches should be near the part's start ([`near`])
/// and a few of them walked say they are, the part's values are scanned
/// from there instead, until the scan has cost what the listing would, and
/// the rest is then listed.
#[allow(clippy::too_many_arguments)]
pub fn eval_first(index: &SubstringIndex, like: &Like, n_docs: u32, value: &mut dyn FnMut(DocId) -> Result<Option<String>>, locate_bytes: u64, stats: &mut Stats, tick: &mut dyn FnMut() -> Result<()>, first: First<'_>) -> Result<DocSet> {
    let mut out = Out::new(n_docs, Some(first));
    let plan = like.plan();
    for part in index.parts() {
        if out.done() {
            break;
        }
        tick()?;
        eval_part(part, like, &plan, value, locate_bytes, stats, &mut out, tick)?;
    }
    Ok(out.docs())
}

/// The matches found so far, and for [`eval_first`] how many it wants.
struct Out<'a> {
    bits: Vec<u64>,
    n_docs: u32,
    first: Option<First<'a>>,
    /// The matches `first.live` keeps.
    found: usize,
}

impl<'a> Out<'a> {
    fn new(n_docs: u32, first: Option<First<'a>>) -> Out<'a> {
        Out { bits: vec![0u64; (n_docs as usize).div_ceil(64)], n_docs, first, found: 0 }
    }

    fn set(&mut self, d: DocId) -> Result<()> {
        if d >= self.n_docs {
            return Err(crate::error::corrupt("substring index: a document the segment does not have"));
        }
        self.bits[d as usize / 64] |= 1 << (d % 64);
        if self.first.is_some_and(|f| (f.live)(d)) {
            self.found += 1;
        }
        Ok(())
    }

    /// The matches still wanted, for [`eval_first`].
    fn need(&self) -> Option<usize> {
        self.first.map(|f| f.need.saturating_sub(self.found))
    }

    fn done(&self) -> bool {
        self.need() == Some(0)
    }

    fn docs(self) -> DocSet {
        DocSet::from_bits(self.bits, self.n_docs)
    }
}

/// Tests each of the part's documents, in order, until `out` has what it
/// wants or, with a `budget`, the values it read come to more bytes than
/// that: the ordinal it stopped before (the part's documents, where it went
/// through them all).
fn scan_part(part: &Part, like: &Like, value: &mut dyn FnMut(DocId) -> Result<Option<String>>, stats: &mut Stats, out: &mut Out, tick: &mut dyn FnMut() -> Result<()>, budget: Option<u64>) -> Result<u64> {
    stats.scanned += 1;
    let mut read = 0u64;
    for o in 0..part.n_docs() {
        if out.done() || budget.is_some_and(|b| read > b) {
            return Ok(o);
        }
        tick()?;
        let Some(d) = part.doc(o)? else { continue };
        stats.read += 1;
        if let Some(v) = value(d)? {
            read += v.len() as u64 + 1;
            if like.matches(&v) {
                out.set(d)?;
            }
        }
    }
    Ok(part.n_docs())
}

/// Where a part's matches are, by a pattern's plan.
enum Rows {
    /// Every document of the part.
    All,
    /// No literal to search: any document may match.
    Any,
    /// The rows of the candidates, and whether each is a match without a
    /// look at its value.
    Ranges(Vec<(u64, u64)>, bool),
}

fn candidates(part: &Part, plan: &Plan) -> Result<Rows> {
    Ok(match plan {
        Plan::All => Rows::All,
        Plan::Exact(bytes) => Rows::Ranges(vec![part.search(bytes)?], true),
        Plan::Pieces(pieces) if pieces.is_empty() => Rows::Any,
        Plan::Pieces(pieces) => {
            // The piece with the fewest rows; a piece with none means no
            // value of the part matches.
            let total = |r: &[(u64, u64)]| r.iter().map(|x| x.1 - x.0).sum::<u64>();
            let mut best: Option<Vec<(u64, u64)>> = None;
            for piece in pieces {
                let (r, _) = rows(part, piece, MAX_RANGES)?;
                if best.as_ref().is_none_or(|b| total(&r) < total(b)) {
                    best = Some(r);
                }
                if best.as_ref().is_some_and(|b| total(b) == 0) {
                    break;
                }
            }
            Rows::Ranges(best.unwrap_or_default(), false)
        }
        Plan::Spelled(piece) => {
            let (r, whole) = rows(part, piece, MAX_RANGES)?;
            Rows::Ranges(r, whole)
        }
    })
}

/// What a part's candidate rows cost to list and its bytes to scan, in
/// bytes of its text a scan for one literal reads in that time.
struct Costs {
    /// A row walked to its sample.
    walk: u64,
    /// A candidate's value read and tested, beside its walk (0 where the
    /// index answers alone).
    test: u64,
    /// A byte scanned.
    byte: u64,
}

impl Costs {
    fn of(like: &Like, exact: bool, locate_bytes: u64) -> Costs {
        let walk = locate_bytes / EXACT_ROWS;
        Costs { walk, test: if exact { 0 } else { locate_bytes - walk }, byte: if like.general() { GENERAL_SCAN } else { 1 } }
    }
}

/// Whether `total` candidate rows of `part` are too many to list: listing
/// them costs more than scanning every value of the part.
fn too_many(part: &Part, total: u64, c: &Costs) -> bool {
    total > 0 && total as u128 * (c.walk + c.test) as u128 > part.n() as u128 * c.byte as u128
}

/// What [`eval`] would do for a pattern, worked out from the index's
/// counts alone (a search per literal and part, no row walked).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Estimate {
    /// No more documents than this match.
    pub candidates: u64,
    /// Rows to walk to a sample.
    pub located: u64,
    /// Candidates the index does not settle, whose values are read and
    /// tested one by one.
    pub tested: u64,
    /// Documents of the parts that are scanned, value after value.
    pub scanned: u64,
}

/// What answering `like` from `index` takes, under [`eval`]'s rule for
/// `locate_bytes`.
pub fn estimate(index: &SubstringIndex, like: &Like, locate_bytes: u64) -> Result<Estimate> {
    let plan = like.plan();
    let mut e = Estimate::default();
    for part in index.parts() {
        match candidates(part, &plan)? {
            Rows::All => e.candidates += part.n_docs(),
            Rows::Any => {
                e.candidates += part.n_docs();
                e.scanned += part.n_docs();
            }
            Rows::Ranges(ranges, exact) => {
                let total: u64 = ranges.iter().map(|r| r.1 - r.0).sum();
                let docs = total.min(part.n_docs());
                e.candidates += docs;
                if too_many(part, total, &Costs::of(like, exact, locate_bytes)) {
                    e.scanned += part.n_docs();
                } else {
                    e.located += total;
                    e.tested += if exact { 0 } else { docs };
                }
            }
        }
    }
    Ok(e)
}

fn eval_part(part: &Part, like: &Like, plan: &Plan, value: &mut dyn FnMut(DocId) -> Result<Option<String>>, locate_bytes: u64, stats: &mut Stats, out: &mut Out, tick: &mut dyn FnMut() -> Result<()>) -> Result<()> {
    let (ranges, exact) = match candidates(part, plan)? {
        Rows::All => {
            for o in 0..part.n_docs() {
                if out.done() {
                    break;
                }
                if let Some(d) = part.doc(o)? {
                    out.set(d)?;
                }
            }
            stats.exact += 1;
            return Ok(());
        }
        Rows::Any => return scan_part(part, like, value, stats, out, tick, None).map(|_| ()),
        Rows::Ranges(ranges, exact) => (ranges, exact),
    };
    let total: u64 = ranges.iter().map(|r| r.1 - r.0).sum();
    let costs = Costs::of(like, exact, locate_bytes);
    if too_many(part, total, &costs) {
        return scan_part(part, like, value, stats, out, tick, None).map(|_| ());
    }
    // The first ordinal the listing below looks at: the scan's for the
    // first matches has tested those before it.
    let mut from = 0;
    if let Some(need) = out.need().filter(|&need| near(part, total, need, &costs)) {
        // [`near`] takes the candidates to be spread evenly over the part,
        // and they bunch up (a word that came into use late, in a table
        // loaded in time order). A few rows walked say where they are: the
        // part is scanned where reaching `need` of them costs less than the
        // listing, and the scan stops once it has read what the listing
        // costs, the rest of the part then listed, which is never more than
        // twice the cheaper of the two.
        let list = total as u128 * costs.walk as u128 + need as u128 * costs.test as u128;
        let reached = reach(part, &ranges, total, need, stats, tick)?;
        let before = part.n() as u128 * reached as u128 / part.n_docs().max(1) as u128 * costs.byte as u128;
        if before < list {
            let budget = list / costs.byte.max(1) as u128;
            from = scan_part(part, like, value, stats, out, tick, Some(budget.min(u64::MAX as u128) as u64))?;
            if from == part.n_docs() || out.done() {
                return Ok(());
            }
        }
    }
    match exact {
        true => stats.exact += 1,
        false => stats.tested += 1,
    }
    // Each row's document, once: the rows are walked in the order of the
    // suffixes, which is no order of the documents, and the documents are
    // then taken in theirs, which is the doc store's. A value read for
    // each row as it came would decompress a block of the store a row.
    let mut seen = vec![0u64; (part.n_docs() as usize).div_ceil(64)];
    for (lo, hi) in ranges {
        for row in lo..hi {
            tick()?;
            stats.located += 1;
            let o = part.ordinal(row)?;
            if o >= from {
                seen[(o / 64) as usize] |= 1u64 << (o % 64);
            }
        }
    }
    for (w, &word) in seen.iter().enumerate() {
        let mut bits = word;
        while bits != 0 {
            if out.done() {
                return Ok(());
            }
            let o = w as u64 * 64 + bits.trailing_zeros() as u64;
            bits &= bits - 1;
            let Some(d) = part.doc(o)? else { continue };
            if exact {
                out.set(d)?;
            } else {
                tick()?;
                stats.read += 1;
                if value(d)?.is_some_and(|v| like.matches(&v)) {
                    out.set(d)?;
                }
            }
        }
    }
    Ok(())
}

/// How many of a part's candidate rows [`eval_first`] walks to see where
/// the first are, before it scans for them.
const SAMPLE_ROWS: u64 = 16;

/// About the ordinal by which a scan of `part` from its start has seen
/// `need` of the documents of its `total` candidate rows in `ranges`, from
/// the ordinals of [`SAMPLE_ROWS`] of them, spread over the rows (which are
/// in the order of the suffixes, no order of the documents): the smallest
/// and the largest say where the candidates begin and how far they spread,
/// and so how many documents hold `need` of them.
fn reach(part: &Part, ranges: &[(u64, u64)], total: u64, need: usize, stats: &mut Stats, tick: &mut dyn FnMut() -> Result<()>) -> Result<u64> {
    let m = SAMPLE_ROWS.min(total);
    let mut seen = Vec::with_capacity(m as usize);
    for i in 0..m {
        let mut k = (i as u128 * total as u128 / m as u128) as u64;
        for &(lo, hi) in ranges {
            if k < hi - lo {
                tick()?;
                stats.located += 1;
                seen.push(part.ordinal(lo + k)?);
                break;
            }
            k -= hi - lo;
        }
    }
    seen.sort_unstable();
    let (Some(&first), Some(&last)) = (seen.first(), seen.last()) else { return Ok(part.n_docs()) };
    if m == total || m < 3 {
        // Every row: the need-th document is known.
        return Ok(seen[(need.max(1) - 1).min(seen.len() - 1)]);
    }
    // `m` rows spread over `span` documents leave a gap of `span / (m + 1)`
    // before the first of them and after the last.
    let span = (last - first) as u128 * (m as u128 + 1) / (m as u128 - 1);
    let start = (first as u128).saturating_sub(span / (m as u128 + 1));
    Ok((start + span * need as u128 / total as u128).min(part.n_docs() as u128) as u64)
}

/// Whether scanning `part` from its start for the first `need` matches is
/// taken to cost less than listing its `total` candidate rows: the
/// candidates are in some `docs` of its documents, at most `total`, so the
/// scan reads about `need` in `docs` of its bytes before it has them, were
/// they spread evenly, where a listing walks every row and then tests
/// values in docID order until it has `need`.
fn near(part: &Part, total: u64, need: usize, c: &Costs) -> bool {
    let docs = total.min(part.n_docs()).max(1) as u128;
    let scan = part.n() as u128 * c.byte as u128 * need as u128;
    let list = (total as u128 * c.walk as u128 + need as u128 * c.test as u128) * docs;
    total > 0 && scan < list
}

/// The documents of `0..n_docs` whose value matches `like`, each read with
/// `value`: what a segment without an index answers with.
pub fn scan(like: &Like, n_docs: u32, value: &mut dyn FnMut(DocId) -> Result<Option<String>>) -> Result<DocSet> {
    scan_with(like, n_docs, value, &mut || Ok(()))
}

/// [`scan`], calling `tick` once a document, as [`eval_with`] does.
pub fn scan_with(like: &Like, n_docs: u32, value: &mut dyn FnMut(DocId) -> Result<Option<String>>, tick: &mut dyn FnMut() -> Result<()>) -> Result<DocSet> {
    let mut bits = vec![0u64; (n_docs as usize).div_ceil(64)];
    for d in 0..n_docs {
        tick()?;
        if value(d)?.is_some_and(|v| like.matches(&v)) {
            bits[d as usize / 64] |= 1 << (d % 64);
        }
    }
    Ok(DocSet::from_bits(bits, n_docs))
}
