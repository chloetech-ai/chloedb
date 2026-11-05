//! `engine-compare plan`: a profile and a seed made into a schedule.
//!
//! Sessions arrive as a Poisson process whose rate follows the day's shape,
//! its bursts and flash crowds (thinning); each session is a few steps, each
//! step a request: what the user set out to do (the profile's mix), keys
//! typed before it (search as you type), then follow-ups (a filter added, a
//! result opened, the items like it), each sent a think time after the step
//! before is answered. Writes are a Poisson process of their own, and batch
//! jobs send theirs back to back. Every draw comes from a stream of its own
//! seeded by the seed (`dist::stream`), so one seed makes one file, byte for
//! byte, and the warm-up never shifts the timed requests.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, HashSet, VecDeque};
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rand::distributions::{Distribution, WeightedIndex};
use rand::seq::SliceRandom;
use rand::Rng;
use rand_distr::WeightedAliasIndex;
use serde::Deserialize;

use super::dist::{busy_spells, day_shape, exp_gap, in_spells, log_uniform, splitmix, stream, LogNorm, Rand, Ranks, Scatter};
use super::profile::{DocPick, FilterMode, Order, Profile, Source, TypoMode};
use super::schedule::{Entry, Header, Op, Shape, Stats, Step, Write, Writer, FORMAT};
use crate::{data, marker, match_word, phrase_of, prefix_of, typo_of, word_list, Knobs, KINDS, SEEN};

/// Kinds whose request is about a query's text (and its vector, for the hybrids).
pub const TEXT_KINDS: &[&str] = &["bm25", "bm25 + filter", "hybrid (RRF)", "hybrid (weighted)", "boosted (bm25 × decay)", "facet", "phrase", "typo", "prefix"];
/// Kinds about a query's vector alone.
pub const VECTOR_KINDS: &[&str] = &["vector", "vector + filter"];
/// The kinds whose filters the knobs set.
pub const FILTERED: &[&str] = &["bm25 + filter", "vector + filter", "similar items"];
/// Words of marks start past those `writes` gives its documents.
const MARK_BASE: usize = 1 << 20;
/// Rows of the query source scanned for a flash crowd's nearest queries.
const FLASH_SCAN: usize = 1_000_000;
/// Documents `run` picks its keys and items from.
const LEGACY_DOCS: usize = 20_000;

fn needs_query(kind: &str) -> bool {
    TEXT_KINDS.contains(&kind) || VECTOR_KINDS.contains(&kind)
}

/// Whether a request of `kind` can be made of the text (prefix: with `n` letters).
pub fn eligible(kind: &str, text: &str, n: usize) -> bool {
    match kind {
        "facet" => match_word(text).is_some(),
        "phrase" => phrase_of(text).is_some(),
        "typo" => typo_of(text).is_some(),
        "prefix" => prefix_of(text, n).is_some(),
        _ => !text.trim().is_empty() || VECTOR_KINDS.contains(&kind),
    }
}

/// The filtered twin of a kind, for a refinement.
fn twin(kind: &str) -> Option<&'static str> {
    match kind {
        "bm25" | "typo" => Some("bm25 + filter"),
        "vector" => Some("vector + filter"),
        _ => None,
    }
}

// --- What the collection holds -------------------------------------------

/// Document keys by row, in one buffer.
pub struct Ids {
    buf: String,
    ends: Vec<usize>,
}

impl Ids {
    pub fn get(&self, i: usize) -> &str {
        let start = if i == 0 { 0 } else { self.ends[i - 1] };
        &self.buf[start..self.ends[i]]
    }

    pub fn len(&self) -> usize {
        self.ends.len()
    }
}

#[derive(Deserialize)]
struct DocAttrs {
    id: String,
    #[serde(default)]
    year: i64,
    #[serde(default)]
    category: String,
    #[serde(default)]
    rating: f64,
}

/// Bit i of a category mask: `c<i>`.
pub fn cat_bit(c: &str) -> Option<usize> {
    let n: usize = c.strip_prefix('c')?.parse().ok()?;
    (n < 64).then_some(n)
}

/// The collection as the planner sees it: its keys, and how its documents
/// spread over years, categories and ratings (to give a filter the share of
/// documents it was drawn to keep).
pub struct Catalog {
    pub collection: String,
    pub dir: PathBuf,
    pub dim: usize,
    pub ids: Ids,
    /// Documents by year and category.
    pub year_cat: BTreeMap<i64, [u64; 64]>,
    /// Documents by rating, in hundredths (0 to 500).
    pub ratings: Vec<u64>,
    pub popularity: Option<Vec<f32>>,
    /// Documents of heldout.jsonl (prepare_msmarco_compare.py --heldout), never loaded: inserts copy them.
    pub heldout: usize,
}

impl Catalog {
    pub fn load(collection: &str) -> Result<Catalog, String> {
        Catalog::load_dir(collection, &data::data_dir().join(collection))
    }

    pub fn load_dir(collection: &str, dir: &Path) -> Result<Catalog, String> {
        let dir = dir.to_path_buf();
        let path = dir.join("meta.json");
        let meta: crate::Meta = serde_json::from_reader(File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?).map_err(|e| e.to_string())?;
        let f = File::open(dir.join("docs.jsonl")).map_err(|e| format!("{}: {e}", dir.join("docs.jsonl").display()))?;
        let mut ids = Ids { buf: String::new(), ends: Vec::with_capacity(meta.docs) };
        let mut year_cat: BTreeMap<i64, [u64; 64]> = BTreeMap::new();
        let mut ratings = vec![0u64; 501];
        for line in BufReader::with_capacity(1 << 20, f).lines() {
            let d: DocAttrs = serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| format!("docs.jsonl: {e}"))?;
            ids.buf.push_str(&d.id);
            ids.ends.push(ids.buf.len());
            if let Some(b) = cat_bit(&d.category) {
                year_cat.entry(d.year).or_insert([0; 64])[b] += 1;
            }
            ratings[((d.rating * 100.0).round().max(0.0) as usize).min(500)] += 1;
        }
        if ids.len() != meta.docs {
            return Err(format!("docs.jsonl has {} documents, meta.json says {}", ids.len(), meta.docs));
        }
        let pop_path = dir.join("doc_popularity.f32");
        let popularity = if pop_path.exists() {
            let mut b = Vec::new();
            File::open(&pop_path).and_then(|mut f| f.read_to_end(&mut b)).map_err(|e| format!("{}: {e}", pop_path.display()))?;
            let w: Vec<f32> = b.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
            if w.len() != ids.len() {
                return Err(format!("doc_popularity.f32 has {} weights for {} documents", w.len(), ids.len()));
            }
            Some(w)
        } else {
            None
        };
        let heldout = match File::open(dir.join("heldout.jsonl")) {
            Ok(f) => BufReader::new(f).lines().count(),
            Err(_) => 0,
        };
        Ok(Catalog { collection: collection.to_string(), dir, dim: meta.dim, ids, year_cat, ratings, popularity, heldout })
    }

    pub fn docs(&self) -> usize {
        self.ids.len()
    }

    /// The query source's files: its texts and its vectors.
    pub fn query_files(&self, source: Source) -> (PathBuf, PathBuf) {
        match source {
            Source::Collection => (self.dir.join("queries.jsonl"), self.dir.join("query_vecs.f32")),
            Source::Pool => (self.dir.join("pool_queries.jsonl"), self.dir.join("pool_vecs.f32")),
        }
    }
}

// --- Queries --------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Q {
    pub row: u64,
    pub id: String,
    pub text: String,
}

#[derive(Deserialize)]
struct QueryLine {
    #[serde(alias = "_id")]
    id: String,
    #[serde(default)]
    text: String,
}

/// A query file read in order: the queries with text, each with its row (its vector's).
pub struct QueryStream {
    path: PathBuf,
    lines: std::io::Lines<BufReader<File>>,
    row: u64,
}

impl QueryStream {
    pub fn open(path: &Path) -> Result<QueryStream, String> {
        let f = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(QueryStream { path: path.to_path_buf(), lines: BufReader::with_capacity(1 << 20, f).lines(), row: 0 })
    }

    pub fn next_query(&mut self) -> Result<Option<Q>, String> {
        for line in self.lines.by_ref() {
            let row = self.row;
            self.row += 1;
            let q: QueryLine = serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| format!("{}: {e}", self.path.display()))?;
            if !q.text.trim().is_empty() {
                return Ok(Some(Q { row, id: q.id, text: q.text }));
            }
        }
        Ok(None)
    }

    /// From the file's start again.
    fn rewind(&mut self) -> Result<(), String> {
        *self = QueryStream::open(&self.path.clone())?;
        Ok(())
    }
}

/// Queries with text of a file.
pub fn count_queries(path: &Path) -> Result<usize, String> {
    let mut s = QueryStream::open(path)?;
    let mut n = 0;
    while s.next_query()?.is_some() {
        n += 1;
    }
    Ok(n)
}

/// A query's vector, read from the vectors' file.
pub fn query_vector(path: &Path, row: u64, dim: usize) -> Result<Vec<f32>, String> {
    use std::os::unix::fs::FileExt;
    let f = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut b = vec![0u8; dim * 4];
    f.read_exact_at(&mut b, row * (dim as u64) * 4).map_err(|e| format!("{}: row {row}: {e}", path.display()))?;
    Ok(b.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect())
}

/// The queries of a flash crowd: the `n` nearest a seed query (by its
/// vector, among the first FLASH_SCAN rows), nearest first.
fn flash_queries(texts: &Path, vecs: &Path, dim: usize, seed: &Q, n: usize) -> Result<Vec<Q>, String> {
    let v = query_vector(vecs, seed.row, dim)?;
    let mut r = BufReader::with_capacity(1 << 20, File::open(vecs).map_err(|e| format!("{}: {e}", vecs.display()))?);
    let mut row = vec![0u8; dim * 4];
    // The best n by dot product: a min-heap of (score bits, row).
    let mut best: BinaryHeap<Reverse<(i64, u64)>> = BinaryHeap::new();
    for i in 0..FLASH_SCAN as u64 {
        if r.read_exact(&mut row).is_err() {
            break;
        }
        let dot: f32 = row.chunks_exact(4).zip(&v).map(|(c, x)| f32::from_le_bytes(c.try_into().unwrap()) * x).sum();
        let key = (dot as f64 * 1e9) as i64;
        best.push(Reverse((key, u64::MAX - i)));
        if best.len() > n * 2 + 8 {
            best.pop();
        }
    }
    let mut rows: Vec<(i64, u64)> = best.into_iter().map(|Reverse((s, r))| (s, u64::MAX - r)).collect();
    rows.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let order: BTreeMap<u64, usize> = rows.iter().enumerate().map(|(i, (_, r))| (*r, i)).collect();
    let mut found: Vec<(usize, Q)> = Vec::new();
    let mut s = QueryStream::open(texts)?;
    while let Some(q) = s.next_query()? {
        if let Some(&i) = order.get(&q.row) {
            found.push((i, q));
        }
        if q_done(&found, order.len()) {
            break;
        }
    }
    found.sort_by_key(|(i, _)| *i);
    Ok(found.into_iter().map(|(_, q)| q).take(n).collect())
}

fn q_done(found: &[(usize, Q)], of: usize) -> bool {
    found.len() == of
}

/// Where queries come from as the schedule goes: a head drawn by Zipf's
/// law, a tail of queries never sent before, and the head's drift.
struct Queries {
    order: Order,
    head: Vec<Q>,
    ranks: Ranks,
    tail_share: f64,
    tail: Option<QueryStream>,
    cycle: usize,
    drift_every: f64,
    drift_share: f64,
    next_drift: f64,
    // What was drawn.
    seen: HashSet<u64>,
    draws: u64,
    first_time: u64,
    top: u64,
    tail_draws: u64,
    wrapped: u64,
}

impl Queries {
    fn draw(&mut self, rng: &mut Rand, t: f64, drift_rng: &mut Rand) -> Result<Q, String> {
        while self.drift_every > 0.0 && t >= self.next_drift {
            self.next_drift += self.drift_every;
            let swaps = (self.head.len() as f64 * self.drift_share).round() as usize;
            for _ in 0..swaps {
                let r = drift_rng.gen_range(0..self.head.len());
                if let Some(q) = self.fresh()? {
                    self.head[r] = q;
                }
            }
        }
        let q = match self.order {
            Order::Cycle => {
                let q = self.head[self.cycle % self.head.len()].clone();
                self.cycle += 1;
                q
            }
            Order::Zipf => {
                if self.tail.is_some() && rng.gen::<f64>() < self.tail_share {
                    self.tail_draws += 1;
                    match self.fresh()? {
                        Some(q) => q,
                        None => self.head[self.ranks.draw(rng)].clone(),
                    }
                } else {
                    let r = self.ranks.draw(rng);
                    if r == 0 {
                        self.top += 1;
                    }
                    self.head[r].clone()
                }
            }
        };
        self.draws += 1;
        if self.seen.insert(q.row) {
            self.first_time += 1;
        }
        Ok(q)
    }

    /// The next query of the tail (from the file's start again when it runs out).
    fn fresh(&mut self) -> Result<Option<Q>, String> {
        let Some(tail) = self.tail.as_mut() else { return Ok(None) };
        if let Some(q) = tail.next_query()? {
            return Ok(Some(q));
        }
        self.wrapped += 1;
        tail.rewind()?;
        tail.next_query()
    }
}

// --- Documents and filters -----------------------------------------------

enum Picker {
    /// `run`'s strides over the first 20,000 documents.
    Legacy(usize),
    Zipf(Ranks, Scatter),
    Weights(WeightedAliasIndex<f32>),
}

impl Picker {
    fn pick(&self, rng: &mut Rand) -> usize {
        match self {
            Picker::Legacy(n) => rng.gen_range(0..*n),
            Picker::Zipf(r, s) => s.map(r.draw(rng)),
            Picker::Weights(w) => w.sample(rng),
        }
    }
}

/// The shares of documents filters keep, from the catalog's counts.
struct FilterSpace {
    n: f64,
    years: Vec<i64>,
    /// Documents of year >= years[i], by category.
    suffix: Vec<[u64; 64]>,
    /// Documents of rating >= h / 100.
    rating_suffix: Vec<u64>,
}

impl FilterSpace {
    fn new(c: &Catalog) -> FilterSpace {
        let years: Vec<i64> = c.year_cat.keys().copied().collect();
        let mut suffix = vec![[0u64; 64]; years.len()];
        let mut acc = [0u64; 64];
        for (i, y) in years.iter().enumerate().rev() {
            for (a, x) in acc.iter_mut().zip(&c.year_cat[y]) {
                *a += x;
            }
            suffix[i] = acc;
        }
        let mut rating_suffix = vec![0u64; c.ratings.len() + 1];
        for h in (0..c.ratings.len()).rev() {
            rating_suffix[h] = rating_suffix[h + 1] + c.ratings[h];
        }
        FilterSpace { n: c.docs().max(1) as f64, years, suffix, rating_suffix }
    }

    /// bm25 + filter: a year and categories that keep about `target` of the documents.
    fn year_cats(&self, rng: &mut Rand, target: f64) -> Option<(i64, u64, f64)> {
        let want = target * self.n;
        let fits: Vec<usize> = (0..self.years.len()).filter(|&i| self.suffix[i].iter().sum::<u64>() as f64 >= want).collect();
        let i = *fits.choose(rng)?;
        let mut cats: Vec<usize> = (0..64).filter(|&c| self.suffix[i][c] > 0).collect();
        cats.shuffle(rng);
        let (mut sum, mut mask, mut best) = (0u64, 0u64, None::<(f64, u64, u64)>);
        for c in cats {
            sum += self.suffix[i][c];
            mask |= 1 << c;
            let off = ((sum as f64).ln() - want.max(1.0).ln()).abs();
            if best.is_none_or(|b| off < b.0) {
                best = Some((off, mask, sum));
            }
            if sum as f64 >= want {
                break;
            }
        }
        best.map(|(_, mask, sum)| (self.years[i], mask, sum as f64 / self.n))
    }

    /// A rating floor that keeps about `target` of the documents.
    fn rating(&self, target: f64) -> Option<(f64, f64)> {
        let want = target * self.n;
        (0..self.rating_suffix.len() - 1)
            .filter(|&h| self.rating_suffix[h] > 0)
            .min_by(|&a, &b| {
                let off = |h: usize| ((self.rating_suffix[h] as f64).ln() - want.max(1.0).ln()).abs();
                off(a).total_cmp(&off(b))
            })
            .map(|h| (h as f64 / 100.0, self.rating_suffix[h] as f64 / self.n))
    }
}

// --- The planner ----------------------------------------------------------

/// What one plan needs, read once (the warm-up's planner and the timed one share it).
pub struct Shared {
    pub head: Arc<Vec<Q>>,
    pub queries: usize,
    pub texts: PathBuf,
    pub vecs: PathBuf,
    /// The head's last row: the tail starts after it.
    pub head_end: u64,
}

impl Shared {
    pub fn load(p: &Profile, c: &Catalog, seed: u64) -> Result<Shared, String> {
        let (texts, vecs) = c.query_files(p.queries.source);
        let queries = count_queries(&texts)?;
        if queries == 0 {
            return Err(format!("{}: no query with text", texts.display()));
        }
        // The head: the first `head` queries, in a seeded order for Zipf's
        // law (the tail keeps queries of the source for itself: half of them at most).
        let take = match p.queries.order {
            Order::Cycle => p.queries.head.min(queries),
            Order::Zipf if p.queries.tail_share > 0.0 => p.queries.head.min(queries.div_ceil(2)).max(1),
            Order::Zipf => p.queries.head.min(queries),
        };
        let mut s = QueryStream::open(&texts)?;
        let mut head = Vec::with_capacity(take);
        while head.len() < take {
            match s.next_query()? {
                Some(q) => head.push(q),
                None => break,
            }
        }
        let head_end = head.last().map_or(0, |q| q.row + 1);
        if p.queries.order == Order::Zipf {
            head.shuffle(&mut stream(seed, "head"));
        }
        Ok(Shared { head: Arc::new(head), queries, texts, vecs, head_end })
    }
}

/// Makes the requests of one part of a schedule (the warm-up, or the timed part).
pub struct Planner<'a> {
    p: &'a Profile,
    c: &'a Catalog,
    warm: bool,
    kinds: Vec<&'static str>,
    mix: WeightedIndex<f64>,
    queries: Queries,
    docs: Picker,
    legacy_i: usize,
    filters: FilterSpace,
    think: LogNorm,
    key_gap: LogNorm,
    batch: LogNorm,
    flash_sets: Vec<Vec<Q>>,
    drift_rng: Rand,
    next_session: u64,
    key_prefix: String,
    inserted: VecDeque<String>,
    next_insert: u64,
    marks: usize,
    pub stats: Stats,
    sel: BTreeMap<&'static str, Vec<f64>>,
}

impl<'a> Planner<'a> {
    pub fn new(p: &'a Profile, c: &'a Catalog, sh: &Shared, seed: u64, warm: bool) -> Result<Planner<'a>, String> {
        let kinds: Vec<&'static str> = KINDS.iter().map(|(k, _)| *k).filter(|k| p.mix.get(*k).copied().unwrap_or(0.0) > 0.0).collect();
        let mix = WeightedIndex::new(kinds.iter().map(|k| p.mix[*k])).map_err(|e| format!("profile: mix: {e}"))?;
        let head = (*sh.head).clone();
        // The warm-up draws the head alone: its tail would take queries the timed part would then not send for the first time.
        let tail = if p.queries.order == Order::Zipf && p.queries.tail_share > 0.0 && !warm {
            let mut s = QueryStream::open(&sh.texts)?;
            while let Some(q) = s.next_query()? {
                if q.row + 1 >= sh.head_end {
                    break;
                }
            }
            Some(s)
        } else {
            None
        };
        let ranks = Ranks::new(head.len(), p.queries.zipf_s);
        let queries = Queries {
            order: p.queries.order,
            head,
            ranks,
            tail_share: p.queries.tail_share,
            tail,
            cycle: 0,
            drift_every: if warm { 0.0 } else { p.queries.drift_every_s },
            drift_share: p.queries.drift_share,
            next_drift: p.queries.drift_every_s,
            seen: HashSet::new(),
            draws: 0,
            first_time: 0,
            top: 0,
            tail_draws: 0,
            wrapped: 0,
        };
        let docs = match (p.docs.pick, &c.popularity) {
            (DocPick::Legacy, _) => Picker::Legacy(c.docs().min(LEGACY_DOCS)),
            (DocPick::Popular, Some(w)) => Picker::Weights(WeightedAliasIndex::new(w.clone()).map_err(|e| format!("doc_popularity.f32: {e}"))?),
            (DocPick::Popular, None) => Picker::Zipf(Ranks::new(c.docs(), p.docs.zipf_s), Scatter::new(c.docs(), splitmix(seed ^ 0xd0c5))),
        };
        let mut flash_sets = Vec::new();
        if !warm {
            let mut rng = stream(seed, "flash");
            for f in &p.traffic.flash {
                let ranks = Ranks::new(sh.head.len(), p.queries.zipf_s);
                let seed_q = &sh.head[ranks.draw(&mut rng)];
                flash_sets.push(flash_queries(&sh.texts, &sh.vecs, c.dim, seed_q, f.queries)?);
            }
        }
        Ok(Planner {
            p,
            c,
            warm,
            kinds,
            mix,
            queries,
            docs,
            legacy_i: 0,
            filters: FilterSpace::new(c),
            think: LogNorm::new(p.sessions.think_median_s * 1e3, p.sessions.think_sigma),
            key_gap: LogNorm::new(p.sessions.key_gap_median_ms, p.sessions.key_gap_sigma),
            batch: LogNorm::new(p.writes.batch_median, p.writes.batch_sigma),
            flash_sets,
            drift_rng: stream(seed, if warm { "warm/drift" } else { "drift" }),
            next_session: if warm { 1 << 40 } else { 1 },
            key_prefix: format!("sim{:06x}{}-", splitmix(seed) & 0xff_ffff, if warm { "w" } else { "" }),
            inserted: VecDeque::new(),
            next_insert: 0,
            marks: 0,
            stats: Stats::default(),
            sel: BTreeMap::new(),
        })
    }

    fn query(&mut self, rng: &mut Rand, t: f64, flash: Option<usize>) -> Result<Q, String> {
        if let Some(f) = flash {
            let set = &self.flash_sets[f];
            let r = Ranks::new(set.len(), 1.0).draw(rng);
            return Ok(set[r].clone());
        }
        self.queries.draw(rng, t, &mut self.drift_rng)
    }

    fn doc(&mut self, rng: &mut Rand) -> usize {
        self.docs.pick(rng)
    }

    /// A query of `kind` (a few draws for one that makes a request of it).
    fn query_for(&mut self, rng: &mut Rand, kind: &str, t: f64, flash: Option<usize>) -> Result<Q, String> {
        let mut q = self.query(rng, t, flash)?;
        for _ in 0..10 {
            if eligible(kind, &q.text, 3) {
                break;
            }
            q = self.query(rng, t, flash)?;
        }
        Ok(q)
    }

    /// The fields of a step of `kind` that are not about a query.
    fn fill(&mut self, rng: &mut Rand, e: &mut Entry) {
        let legacy = matches!(self.docs, Picker::Legacy(_));
        let n = self.c.docs().min(LEGACY_DOCS).max(1);
        let i = self.legacy_i;
        match e.k.as_str() {
            "key lookup" => {
                let r = if legacy { (i * 7919) % n } else { self.doc(rng) };
                e.key = Some(self.c.ids.get(r).to_string());
                e.row = Some(r as u64);
            }
            "similar items" => {
                let it = if legacy { (i * 104_729 + 13) % n } else { e.item.map_or_else(|| self.doc(rng), |x| x as usize) };
                let mut rows = vec![it];
                if legacy {
                    rows.extend((1..SEEN).map(|j| (it + j * 997) % n));
                } else {
                    let mut tries = 0;
                    while rows.len() < SEEN.min(self.c.docs()) && tries < SEEN * 20 {
                        let r = self.doc(rng);
                        if !rows.contains(&r) {
                            rows.push(r);
                        }
                        tries += 1;
                    }
                }
                e.item = Some(it as u64);
                e.seen = rows.iter().map(|&r| self.c.ids.get(r).to_string()).collect();
            }
            "sql aggregate" => e.year = Some(1990 + if legacy { (i % 30) as i64 } else { rng.gen_range(0..30) }),
            _ => {}
        }
        if self.p.filters.mode == FilterMode::Drawn && FILTERED.contains(&e.k.as_str()) {
            let target = log_uniform(rng, self.p.filters.min_selectivity, self.p.filters.max_selectivity);
            let mut k = Knobs::default();
            let got = match e.k.as_str() {
                "bm25 + filter" => self.filters.year_cats(rng, target).map(|(y, m, s)| {
                    (k.year_min, k.cats) = (y, m);
                    s
                }),
                "vector + filter" => self.filters.rating(target).map(|(r, s)| {
                    k.rating_min = r;
                    s
                }),
                _ => self.filters.rating(target).map(|(r, s)| {
                    k.similar_rating = r;
                    s
                }),
            };
            if let Some(s) = got {
                e.knobs = Some(k);
                e.sel = Some(s);
            }
        }
        if let Some(s) = e.sel {
            let kind = FILTERED.iter().find(|k| **k == e.k).unwrap();
            self.sel.entry(kind).or_default().push(s);
        }
    }

    fn set_query(e: &mut Entry, q: &Q) {
        e.q = Some(q.row);
        e.qid = Some(q.id.clone());
        e.text = Some(q.text.clone());
    }

    /// One session from `t0`: its steps, in order.
    pub fn session(&mut self, rng: &mut Rand, t0: f64, flash: Option<usize>) -> Result<Vec<Entry>, String> {
        let p = self.p;
        let mut kind = self.kinds[self.mix.sample(rng)];
        let mut out = Vec::new();
        let mut main = Entry { t: t0, k: kind.to_string(), warm: self.warm, flash: flash.is_some(), ..Entry::default() };
        let mut start = t0;
        if needs_query(kind) {
            let q = self.query_for(rng, kind, t0, flash)?;
            Self::set_query(&mut main, &q);
            if TEXT_KINDS.contains(&kind) && p.queries.typo_share > 0.0 && rng.gen::<f64>() < p.queries.typo_share {
                if let Some(x) = typo_of(&q.text) {
                    if p.queries.typos == TypoMode::Fuzzy && kind == "bm25" {
                        kind = "typo";
                        main.k = kind.to_string();
                    } else if kind != "typo" {
                        main.text = Some(x);
                        main.typo = true;
                    }
                }
            }
            // Search as you type: the query's words typed key by key, a prefix request at each pause.
            if p.sessions.typing && TEXT_KINDS.contains(&kind) {
                let words = word_list(main.text.as_deref().unwrap_or(""));
                let mut t = 0.0f64;
                for (wi, w) in words.iter().enumerate() {
                    let letters = w.chars().count();
                    for typed in 1..=letters + 1 {
                        // The key after `typed` letters of the word (its last: the space or Enter).
                        let gap = self.key_gap.draw(rng);
                        if wi > 0 && gap > p.sessions.pause_ms && typed >= 2 && typed < letters && w.is_ascii() {
                            let text = words[..=wi].join(" ");
                            if prefix_of(&text, typed).is_some() {
                                let mut e = Entry { t: t0 + (t + p.sessions.pause_ms) / 1e3, k: "prefix".into(), warm: self.warm, flash: flash.is_some(), ..Entry::default() };
                                e.q = main.q;
                                e.qid = main.qid.clone();
                                e.text = Some(text);
                                e.knobs = Some(Knobs { prefix_len: typed, ..Knobs::default() });
                                out.push(e);
                            }
                        }
                        t += gap;
                    }
                }
                start = t0 + t / 1e3;
                main.t = start;
            }
        }
        self.fill(rng, &mut main);
        self.legacy_i += 1;
        let mut last_kind = kind;
        let mut opened: Option<u64> = None;
        let mut prev_t = start;
        let main_q = (main.q, main.qid.clone(), main.text.clone(), main.typo, main.flash);
        out.push(main);
        // Follow-ups, each a think time after the answer before.
        let weights = [p.sessions.refine, p.sessions.open, p.sessions.similar];
        let mut steps = 0;
        while p.sessions.follow_up > 0.0 && steps < 50 && rng.gen::<f64>() < p.sessions.follow_up {
            steps += 1;
            let can = [twin(last_kind).is_some() && main_q.0.is_some(), true, true];
            let w: Vec<f64> = weights.iter().zip(can).map(|(w, c)| if c { *w } else { 0.0 }).collect();
            let Ok(pick) = WeightedIndex::new(&w) else { break };
            let think = self.think.draw(rng);
            let mut e = Entry { t: prev_t + think / 1e3, think_ms: Some((think * 1e3).round() / 1e3), warm: self.warm, ..Entry::default() };
            match pick.sample(rng) {
                0 => {
                    let k = twin(last_kind).unwrap();
                    e.k = k.to_string();
                    (e.q, e.qid, e.text, e.typo, e.flash) = main_q.clone();
                    last_kind = k;
                }
                1 => {
                    e.k = "key lookup".into();
                }
                _ => {
                    e.k = "similar items".into();
                    e.item = opened;
                }
            }
            self.fill(rng, &mut e);
            if e.k == "key lookup" {
                opened = e.row;
            }
            prev_t = e.t;
            out.push(e);
        }
        if out.iter().any(|e| e.think_ms.is_some()) {
            let s = self.next_session;
            self.next_session += 1;
            for e in &mut out {
                e.s = s;
            }
            out.last_mut().unwrap().last = true;
            self.stats.waiting_sessions += 1;
        }
        self.stats.sessions += 1;
        Ok(out)
    }

    /// One write at `t` (a job's: `job`, `docs` documents).
    pub fn write(&mut self, rng: &mut Rand, t: f64, job: Option<(u32, u32)>) -> Entry {
        let w = &self.p.writes;
        let op = match job {
            Some(_) => Op::Insert,
            None => {
                let i = WeightedIndex::new([w.insert, w.update, w.delete]).map(|d| d.sample(rng)).unwrap_or(0);
                match i {
                    0 => Op::Insert,
                    1 => Op::Update,
                    _ if self.inserted.is_empty() => Op::Insert,
                    _ => Op::Delete,
                }
            }
        };
        let mut wr = Write { op, job: job.map(|j| j.0), ..Write::default() };
        match op {
            Op::Insert => {
                let n = job.map_or_else(|| (self.batch.draw(rng).round() as u32).clamp(1, w.batch_max), |j| j.1).max(1);
                let avail = if self.c.heldout > 0 { self.c.heldout } else { self.c.docs() } as u64;
                let mut marked = None;
                for i in 0..n {
                    let key = format!("{}{}", self.key_prefix, self.next_insert);
                    wr.rows.push(self.next_insert % avail.max(1));
                    if w.mark_every > 0 && job.is_none() && marked.is_none() && self.next_insert % w.mark_every as u64 == 0 {
                        wr.mark = Some(marker(MARK_BASE + self.marks));
                        self.marks += 1;
                        marked = Some(i as usize);
                    }
                    self.inserted.push_back(key.clone());
                    wr.keys.push(key);
                    self.next_insert += 1;
                }
                // The marked document goes first (keys and rows stay paired).
                if let Some(i) = marked {
                    wr.keys.swap(0, i);
                    wr.rows.swap(0, i);
                }
                self.stats.docs_inserted += n as u64;
            }
            Op::Update => {
                let r = self.doc(rng);
                wr.keys.push(self.c.ids.get(r).to_string());
                wr.rows.push(r as u64);
                wr.rating = Some(rng.gen_range(0..=500) as f64 / 100.0);
            }
            Op::Delete => {
                wr.keys.push(self.inserted.pop_front().expect("a document to delete"));
            }
        }
        *self.stats.writes.entry(format!("{op:?}").to_lowercase()).or_default() += 1;
        Entry { t, k: "write".into(), w: Some(wr), ..Entry::default() }
    }

    fn finish_stats(&mut self) {
        let q = &self.queries;
        self.stats.query_draws = q.draws;
        self.stats.distinct_queries = q.seen.len() as u64;
        self.stats.first_time_share = q.first_time as f64 / q.draws.max(1) as f64;
        self.stats.top_query_share = q.top as f64 / q.draws.max(1) as f64;
        self.stats.tail_draws = q.tail_draws;
        self.stats.tail_wrapped = q.wrapped;
        self.stats.marks = self.marks as u64;
        for (k, v) in &mut self.sel {
            v.sort_by(f64::total_cmp);
            let at = |p: f64| v[((v.len() - 1) as f64 * p).round() as usize];
            self.stats.selectivity.insert(k.to_string(), [0.01, 0.1, 0.5, 0.9, 0.99].iter().map(|&p| at(p)).collect());
        }
    }
}

// --- The schedule ---------------------------------------------------------

/// Entries waiting for their time to be written, earliest first (ties in the order made).
struct Pending {
    heap: BinaryHeap<Reverse<(u64, u64)>>,
    entries: BTreeMap<u64, Entry>,
    seq: u64,
}

impl Pending {
    fn new() -> Pending {
        Pending { heap: BinaryHeap::new(), entries: BTreeMap::new(), seq: 0 }
    }

    fn push(&mut self, mut e: Entry) {
        e.t = (e.t * 1e6).round() / 1e6;
        let key = (e.t * 1e6) as u64;
        self.heap.push(Reverse((key, self.seq)));
        self.entries.insert(self.seq, e);
        self.seq += 1;
    }

    /// The entries planned before `t`, in order.
    fn before(&mut self, t: f64) -> Vec<Entry> {
        let mut out = Vec::new();
        while let Some(Reverse((key, seq))) = self.heap.peek().copied() {
            if key as f64 / 1e6 > t {
                break;
            }
            self.heap.pop();
            out.push(self.entries.remove(&seq).unwrap());
        }
        out
    }
}

/// Counts what the timed schedule holds, as it is written.
struct Tally {
    stats: Stats,
    windows: Vec<u64>,
    steps: Vec<Step>,
    per_step: Vec<u64>,
    warmup: f64,
}

impl Tally {
    fn add(&mut self, e: &Entry) {
        if e.warm {
            self.stats.warm_requests += 1;
            return;
        }
        self.stats.requests += 1;
        *self.stats.by_kind.entry(e.k.clone()).or_default() += 1;
        if e.typo || e.k == "typo" {
            self.stats.typos += 1;
        }
        if e.flash {
            self.stats.flash_requests += 1;
        }
        let rel = e.t - self.warmup;
        let w = (rel.max(0.0) / 10.0) as usize;
        if self.windows.len() <= w {
            self.windows.resize(w + 1, 0);
        }
        self.windows[w] += 1;
        if let Some(i) = self.steps.iter().position(|s| rel >= s.from_s && rel < s.to_s) {
            self.per_step[i] += 1;
        }
    }
}

pub struct Plan {
    pub header: Header,
    pub stats: Stats,
    pub sha256: String,
}

/// Mean requests a session makes, from sessions drawn by a planner of their own.
fn requests_per_session(p: &Profile, c: &Catalog, sh: &Shared, seed: u64) -> Result<f64, String> {
    if p.sessions.follow_up == 0.0 && !p.sessions.typing {
        return Ok(1.0);
    }
    let mut pl = Planner::new(p, c, sh, splitmix(seed ^ 0xca1), true)?;
    let mut rng = stream(seed, "calibrate");
    let n = 4000;
    let mut total = 0usize;
    for _ in 0..n {
        total += pl.session(&mut rng, 0.0, None)?.len();
    }
    Ok(total as f64 / n as f64)
}

/// Plans a schedule into `out`.
pub fn plan(p: &Profile, profile_sha256: &str, c: &Catalog, seed: u64, shape: Shape, out: &Path) -> Result<Plan, String> {
    let sh = Shared::load(p, c, seed)?;
    let warmup = if shape == Shape::Closed { 0.0 } else { p.traffic.warmup_s };
    let steps: Vec<Step> = match &shape {
        Shape::Day { rate } => vec![Step { from_s: 0.0, to_s: p.traffic.day_s, rate: *rate }],
        Shape::Ramp => {
            let r = &p.traffic.ramp;
            (0..r.steps).map(|i| Step { from_s: i as f64 * r.step_s, to_s: (i + 1) as f64 * r.step_s, rate: r.start * r.factor.powi(i as i32) }).collect()
        }
        Shape::Closed => Vec::new(),
    };
    let end_s = steps.last().map_or(0.0, |s| s.to_s);
    let header = Header {
        format: FORMAT,
        profile: p.clone(),
        profile_sha256: profile_sha256.to_string(),
        seed,
        shape: shape.clone(),
        collection: c.collection.clone(),
        docs: c.docs(),
        dim: c.dim,
        source: p.queries.source,
        queries: sh.queries,
        inserts_from: if c.heldout > 0 { "heldout".into() } else { "docs".into() },
        inserts_available: if c.heldout > 0 { c.heldout } else { c.docs() },
        warmup_s: warmup,
        end_s: warmup + end_s,
        steps: steps.clone(),
    };
    let mut w = Writer::create(out, &header)?;
    let mut tally = Tally { stats: Stats::default(), windows: Vec::new(), per_step: vec![0; steps.len()], steps: steps.clone(), warmup };
    let mut timed = Planner::new(p, c, &sh, seed, false)?;

    if shape == Shape::Closed {
        // run's requests: each kind, the head's queries in order, keys and items by run's strides.
        for &kind in &timed.kinds.clone() {
            timed.legacy_i = 0;
            timed.queries.cycle = 0;
            let mut rng = stream(seed, &format!("closed/{kind}"));
            for _ in 0..timed.queries.head.len() {
                let mut e = Entry { t: 0.0, k: kind.to_string(), ..Entry::default() };
                let q = timed.queries.draw(&mut rng, 0.0, &mut stream(seed, "none"))?;
                Planner::set_query(&mut e, &q);
                timed.fill(&mut rng, &mut e);
                timed.legacy_i += 1;
                tally.add(&e);
                w.entry(&e)?;
            }
        }
    } else {
        let e_r = requests_per_session(p, c, &sh, seed)?;
        let ws = p.writes.share;
        let mut pending = Pending::new();
        // The warm-up: the first step's rate, reads alone, from a planner of its own.
        if warmup > 0.0 {
            let mut warm = Planner::new(p, c, &sh, splitmix(seed ^ 0x3a4d), true)?;
            let rate0 = match &shape {
                Shape::Day { rate } => rate * day_shape(0.0, p.traffic.peak_to_trough, p.traffic.humps, p.traffic.peak_at),
                _ => steps[0].rate,
            };
            let (mut arr, mut ses) = (stream(seed, "warm/arrivals"), stream(seed, "warm/sessions"));
            let lam = rate0 * (1.0 - ws) / e_r;
            let mut t = exp_gap(&mut arr, lam);
            while t < warmup {
                for e in pending.before(t) {
                    tally.add(&e);
                    w.entry(&e)?;
                }
                for e in warm.session(&mut ses, t, None)? {
                    pending.push(e);
                }
                t += exp_gap(&mut arr, lam);
            }
        }
        // The timed part: sessions and writes, each a Poisson process thinned to the rate of the moment.
        let bursts = match (&shape, &p.traffic.bursts) {
            (Shape::Day { .. }, Some(b)) => (b.factor, busy_spells(&mut stream(seed, "bursts"), b.busy_s, b.calm_s, end_s)),
            _ => (1.0, Vec::new()),
        };
        let flashes: Vec<(f64, f64, f64)> = match &shape {
            Shape::Day { .. } => p.traffic.flash.iter().map(|f| (f.at_s, f.at_s + f.for_s, f.factor)).collect(),
            _ => Vec::new(),
        };
        let day = |t: f64| match &shape {
            Shape::Day { .. } => day_shape(t / p.traffic.day_s, p.traffic.peak_to_trough, p.traffic.humps, p.traffic.peak_at),
            _ => 1.0,
        };
        // The rate at t (requests a second) and the flash crowd's part of it.
        let rate_at = |t: f64, step: &Step| -> (f64, f64, Option<usize>) {
            let base = step.rate * day(t) * if in_spells(&bursts.1, t) { bursts.0 } else { 1.0 };
            match flashes.iter().position(|f| t >= f.0 && t < f.1) {
                Some(i) => (base * flashes[i].2, base * (flashes[i].2 - 1.0), Some(i)),
                None => (base, 0.0, None),
            }
        };
        let max_flash = flashes.iter().map(|f| f.2).fold(1.0, f64::max);
        let (mut arr, mut ses, mut wrr, mut wrs) = (stream(seed, "arrivals"), stream(seed, "sessions"), stream(seed, "write-arrivals"), stream(seed, "writes"));
        let mut jobs: Vec<(f64, u32)> = p.writes.jobs.iter().enumerate().map(|(i, j)| (j.at_s, i as u32)).collect();
        jobs.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut jobs = jobs.into_iter().peekable();
        for step in &steps {
            let top = step.rate * bursts.0 * max_flash;
            let (lam_s, lam_w) = (top * (1.0 - ws) / e_r, top * ws);
            let mut ts = step.from_s + exp_gap(&mut arr, lam_s);
            let mut tw = if lam_w > 0.0 { step.from_s + exp_gap(&mut wrr, lam_w) } else { f64::INFINITY };
            loop {
                let tj = jobs.peek().map_or(f64::INFINITY, |j| j.0);
                let t = ts.min(tw).min(tj);
                if t >= step.to_s {
                    break;
                }
                for e in pending.before(warmup + t) {
                    tally.add(&e);
                    w.entry(&e)?;
                }
                if t == tj {
                    let (_, i) = jobs.next().unwrap();
                    let job = &p.writes.jobs[i as usize];
                    let reqs = job.docs.div_ceil(job.batch.max(1));
                    for client in 0..job.clients.max(1) {
                        let s = timed.next_session;
                        timed.next_session += 1;
                        let mine: Vec<u32> = (client..reqs).step_by(job.clients.max(1) as usize).collect();
                        for (n, r) in mine.iter().enumerate() {
                            let docs = job.batch.min(job.docs - r * job.batch);
                            let mut e = timed.write(&mut wrs, warmup + t, Some((i, docs)));
                            e.s = s;
                            if n > 0 {
                                e.think_ms = Some(0.0);
                            }
                            e.last = n + 1 == mine.len();
                            pending.push(e);
                        }
                    }
                } else if t == tw {
                    let (rate, _, _) = rate_at(t, step);
                    if wrr.gen::<f64>() < rate / top {
                        pending.push(timed.write(&mut wrs, warmup + t, None));
                    }
                    tw += exp_gap(&mut wrr, lam_w);
                } else {
                    let (rate, extra, flash) = rate_at(t, step);
                    if arr.gen::<f64>() < rate / top {
                        let flash = flash.filter(|_| arr.gen::<f64>() < extra / rate);
                        for e in timed.session(&mut ses, warmup + t, flash)? {
                            pending.push(e);
                        }
                    }
                    ts += exp_gap(&mut arr, lam_s);
                }
            }
        }
        for e in pending.before(f64::INFINITY) {
            tally.add(&e);
            w.entry(&e)?;
        }
    }
    timed.finish_stats();
    let mut stats = tally.stats;
    let t = &timed.stats;
    (stats.sessions, stats.waiting_sessions, stats.writes, stats.docs_inserted, stats.marks) = (t.sessions, t.waiting_sessions, t.writes.clone(), t.docs_inserted, t.marks);
    (stats.query_draws, stats.distinct_queries, stats.first_time_share, stats.top_query_share, stats.tail_draws, stats.tail_wrapped) =
        (t.query_draws, t.distinct_queries, t.first_time_share, t.top_query_share, t.tail_draws, t.tail_wrapped);
    stats.selectivity = t.selectivity.clone();
    let reads = stats.requests - stats.by_kind.get("write").copied().unwrap_or(0);
    stats.requests_per_session = reads as f64 / stats.sessions.max(1) as f64;
    stats.mean_rate = if end_s > 0.0 { stats.requests as f64 / end_s } else { 0.0 };
    stats.peak_10s_rate = tally.windows.iter().copied().max().unwrap_or(0) as f64 / 10.0;
    stats.per_step = tally.per_step;
    let sha256 = w.finish(&stats)?;
    Ok(Plan { header, stats, sha256 })
}

/// The plan's summary, for people.
pub fn summary(plan: &Plan) -> String {
    let s = &plan.stats;
    let mut out = format!(
        "{} requests ({} warm-up) over {:.0} s: {:.1} a second on average, {:.1} at the busiest 10 s\n",
        s.requests,
        s.warm_requests,
        plan.header.end_s - plan.header.warmup_s,
        s.mean_rate,
        s.peak_10s_rate
    );
    for (k, n) in &s.by_kind {
        out += &format!("  {k:<24} {n:>9}  {:>5.1}%\n", 100.0 * *n as f64 / s.requests.max(1) as f64);
    }
    out += &format!(
        "sessions {} ({} with follow-ups), {:.2} reads a session; queries drawn {}, distinct {}, first-time {:.1}%, top query {:.2}%, tail {} (wrapped {}); typos {}; flash {}\n",
        s.sessions,
        s.waiting_sessions,
        s.requests_per_session,
        s.query_draws,
        s.distinct_queries,
        100.0 * s.first_time_share,
        100.0 * s.top_query_share,
        s.tail_draws,
        s.tail_wrapped,
        s.typos,
        s.flash_requests
    );
    if !s.writes.is_empty() {
        out += &format!("writes {:?}, {} documents inserted, {} marked\n", s.writes, s.docs_inserted, s.marks);
    }
    for (k, q) in &s.selectivity {
        out += &format!("  {k}: documents kept p1 {:.4}% p50 {:.3}% p99 {:.2}%\n", 100.0 * q[0], 100.0 * q[2], 100.0 * q[4]);
    }
    out += &format!("schedule sha256 {}\n", plan.sha256);
    out
}
