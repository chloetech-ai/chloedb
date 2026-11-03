//! The BM25 statistics a coordinator keeps of its shards' copies, so that
//! a BM25 query whose terms it has seen skips the statistics round.
//!
//! Every clustered BM25 query scores with statistics summed over the
//! shards (`strata::query::stats`): the document count and length of each
//! field, and each term's document frequency. Asking every shard for them
//! first is a round of calls before the query's own. But they change only
//! with a shard's version: a write, a flush, a merge or a restart
//! (`TermStatsResponse.version`, [`STATS_VERSION_PROTOCOL`]). So the
//! coordinator keeps, per copy, the version its last statistics round
//! answered and what it answered of each field and term. A query whose
//! terms follow from its text alone ([`strata::cluster::stat_terms`]: no
//! typos or synonyms, which only a shard can expand) and are all kept for
//! a copy of every shard is sent the kept sums at once, each copy with the
//! version they are of (`QueryRequest.stats_version`). A copy that has
//! moved on since answers stale without running it, and the query is sent
//! again with a statistics round, which keeps the new version's. Scores
//! are exact either way: a query runs only on the version its statistics
//! are of, as it does on the snapshot a statistics round leased.
//!
//! A copy that answers stale is not asked with kept statistics for a while
//! after, from 10 ms, doubling while it keeps moving on, to a second: a
//! shard taking writes all the time costs a wasted query a second, not
//! one each write.
//!
//! [`STATS_VERSION_PROTOCOL`]: crate::grpc::STATS_VERSION_PROTOCOL

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use strata::query::stats::{FieldStats, TermStats};

/// Terms kept per copy by default ([`super::CoordOptions::stats_cache_terms`]):
/// about 10 MB a copy when full, which a copy that reaches it empties.
pub const STATS_CACHE_TERMS: usize = 100_000;

/// How long a copy that answered stale is first passed over, and the most.
const COOL_FROM: Duration = Duration::from_millis(10);
const COOL_UP_TO: Duration = Duration::from_secs(1);

/// What a coordinator keeps of each copy's statistics, by its URL.
pub struct StatsCache {
    copies: Mutex<HashMap<String, Kept>>,
    max_terms: usize,
    /// Queries sent kept statistics, sent a statistics round, and answered
    /// stale (then sent again with a round).
    pub hits: AtomicU64,
    pub misses: AtomicU64,
    pub stale: AtomicU64,
}

/// One copy's statistics as of `version`.
#[derive(Default)]
struct Kept {
    version: u64,
    /// By table and field.
    fields: HashMap<(String, String), Field>,
    terms: usize,
    /// Passed over until then, after a stale answer; how long it was.
    cool_until: Option<Instant>,
    cool: Duration,
}

#[derive(Default)]
struct Field {
    n_docs: u64,
    total_len: u64,
    df: HashMap<String, u64>,
}

impl StatsCache {
    pub fn new(max_terms: usize) -> StatsCache {
        StatsCache { copies: Mutex::new(HashMap::new()), max_terms, hits: AtomicU64::new(0), misses: AtomicU64::new(0), stale: AtomicU64::new(0) }
    }

    pub fn on(&self) -> bool {
        self.max_terms > 0
    }

    /// Keeps what copy `url` answered a statistics round with for a query
    /// of `table`: `stats`, of its version `version` (0: none to keep).
    pub fn keep(&self, url: &str, version: u64, table: &str, stats: &TermStats) {
        if version == 0 || !self.on() {
            return;
        }
        let mut copies = self.copies.lock().unwrap_or_else(|e| e.into_inner());
        let k = copies.entry(url.to_string()).or_default();
        if k.version != version {
            k.version = version;
            k.fields.clear();
            k.terms = 0;
        }
        let new: usize = stats.fields.iter().map(|(f, s)| k.fields.get(&(table.to_string(), f.clone())).map_or(s.df.len(), |kf| s.df.keys().filter(|t| !kf.df.contains_key(*t)).count())).sum();
        if k.terms + new > self.max_terms {
            k.fields.clear();
            k.terms = 0;
        }
        for (f, s) in &stats.fields {
            let kf = k.fields.entry((table.to_string(), f.clone())).or_default();
            kf.n_docs = s.n_docs;
            kf.total_len = s.total_len;
            for (t, n) in &s.df {
                if kf.df.insert(t.clone(), *n).is_none() {
                    k.terms += 1;
                }
            }
        }
    }

    /// Copy `url`'s statistics of `terms` (by field) in `table`, as a
    /// statistics round would answer them, and their version: `None`
    /// unless every term is kept, or while the copy is passed over.
    pub fn get(&self, url: &str, table: &str, terms: &[(String, Vec<String>)]) -> Option<(TermStats, u64)> {
        let copies = self.copies.lock().unwrap_or_else(|e| e.into_inner());
        let k = copies.get(url)?;
        if k.version == 0 || k.cool_until.is_some_and(|t| Instant::now() < t) {
            return None;
        }
        let mut out = TermStats::default();
        for (f, ts) in terms {
            let kf = k.fields.get(&(table.to_string(), f.clone()))?;
            let mut df = std::collections::BTreeMap::new();
            for t in ts {
                df.insert(t.clone(), *kf.df.get(t)?);
            }
            // As a shard records a field scored by several inputs
            // (`TermStats::merge_field`): the terms of all of them.
            let e = out.fields.entry(f.clone()).or_insert_with(|| FieldStats { n_docs: kf.n_docs, total_len: kf.total_len, df: Default::default() });
            e.df.extend(df);
        }
        Some((out, k.version))
    }

    /// Copy `url` answered stale a query sent its statistics of `version`:
    /// they are dropped, and the copy is passed over for a while (longer
    /// each time it moved on again within a second). Answers of a version
    /// already dropped change nothing more.
    pub fn stale(&self, url: &str, version: u64) {
        self.stale.fetch_add(1, Ordering::Relaxed);
        let mut copies = self.copies.lock().unwrap_or_else(|e| e.into_inner());
        let Some(k) = copies.get_mut(url) else { return };
        if k.version != version {
            return;
        }
        let now = Instant::now();
        let again = k.cool_until.is_some_and(|t| now < t + COOL_UP_TO);
        k.cool = if again { (k.cool * 2).clamp(COOL_FROM, COOL_UP_TO) } else { COOL_FROM };
        k.cool_until = Some(now + k.cool);
        k.version = 0;
        k.fields.clear();
        k.terms = 0;
    }

    /// Terms kept, over every copy.
    pub fn terms(&self) -> usize {
        self.copies.lock().unwrap_or_else(|e| e.into_inner()).values().map(|k| k.terms).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(n_docs: u64, df: &[(&str, u64)]) -> TermStats {
        TermStats { fields: [("body".to_string(), FieldStats { n_docs, total_len: n_docs * 10, df: df.iter().map(|(t, n)| (t.to_string(), *n)).collect() })].into() }
    }

    fn terms(ts: &[&str]) -> Vec<(String, Vec<String>)> {
        vec![("body".to_string(), ts.iter().map(|t| t.to_string()).collect())]
    }

    #[test]
    fn answers_kept_terms_of_one_version() {
        let c = StatsCache::new(100);
        c.keep("a", 7, "docs", &stats(100, &[("cat", 3), ("dog", 0)]));
        c.keep("a", 7, "docs", &stats(100, &[("fish", 5)]));
        let (s, v) = c.get("a", "docs", &terms(&["cat", "fish", "dog"])).unwrap();
        assert_eq!(v, 7);
        assert_eq!(s, stats(100, &[("cat", 3), ("fish", 5), ("dog", 0)]));
        // A term not kept, another table, another copy: a round.
        assert!(c.get("a", "docs", &terms(&["cat", "bird"])).is_none());
        assert!(c.get("a", "other", &terms(&["cat"])).is_none());
        assert!(c.get("b", "docs", &terms(&["cat"])).is_none());
        // A new version replaces the old one's terms.
        c.keep("a", 8, "docs", &stats(101, &[("cat", 4)]));
        assert!(c.get("a", "docs", &terms(&["fish"])).is_none());
        assert_eq!(c.get("a", "docs", &terms(&["cat"])).unwrap(), (stats(101, &[("cat", 4)]), 8));
        // Version 0 (a coordinator's shard) is not kept.
        c.keep("z", 0, "docs", &stats(1, &[("cat", 1)]));
        assert!(c.get("z", "docs", &terms(&["cat"])).is_none());
    }

    #[test]
    fn a_stale_copy_is_passed_over_for_a_while() {
        let c = StatsCache::new(100);
        c.keep("a", 7, "docs", &stats(100, &[("cat", 3)]));
        c.stale("a", 7);
        assert!(c.get("a", "docs", &terms(&["cat"])).is_none());
        c.keep("a", 8, "docs", &stats(100, &[("cat", 3)]));
        // Kept again, but passed over until the cool-down ends.
        assert!(c.get("a", "docs", &terms(&["cat"])).is_none());
        std::thread::sleep(COOL_FROM + Duration::from_millis(5));
        assert_eq!(c.get("a", "docs", &terms(&["cat"])).unwrap().1, 8);
        // A stale answer of a dropped version changes nothing.
        c.stale("a", 7);
        assert_eq!(c.get("a", "docs", &terms(&["cat"])).unwrap().1, 8);
        // Moving on again within a second doubles the wait.
        c.stale("a", 8);
        c.keep("a", 9, "docs", &stats(100, &[("cat", 3)]));
        std::thread::sleep(COOL_FROM + Duration::from_millis(5));
        assert!(c.get("a", "docs", &terms(&["cat"])).is_none());
        std::thread::sleep(COOL_FROM);
        assert_eq!(c.get("a", "docs", &terms(&["cat"])).unwrap().1, 9);
    }

    #[test]
    fn a_full_copy_starts_again() {
        let c = StatsCache::new(3);
        c.keep("a", 1, "docs", &stats(10, &[("a", 1), ("b", 1)]));
        assert_eq!(c.terms(), 2);
        c.keep("a", 1, "docs", &stats(10, &[("b", 1), ("c", 1)]));
        assert_eq!(c.terms(), 3);
        c.keep("a", 1, "docs", &stats(10, &[("d", 1), ("e", 1)]));
        assert_eq!(c.terms(), 2);
        assert!(c.get("a", "docs", &terms(&["a"])).is_none());
        assert!(c.get("a", "docs", &terms(&["d", "e"])).is_some());
        // Off: nothing kept.
        let off = StatsCache::new(0);
        off.keep("a", 1, "docs", &stats(10, &[("a", 1)]));
        assert!(off.get("a", "docs", &terms(&["a"])).is_none());
    }
}
