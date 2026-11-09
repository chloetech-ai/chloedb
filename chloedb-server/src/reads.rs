//! How often a table's documents are retrieved and its keys asked for,
//! for the index inspector ([`crate::inspect`]). Kept in memory since the
//! server started, and cheap enough to stay on (`--read-stats off` turns
//! it off):
//!
//! - every row a query answers a client with counts as a retrieval of its
//!   document, by primary key (a table without one counts its rows only);
//!   every key a read by key asks for (`/v1/kv`, `Get`) counts as a
//!   request of that key, found or missing. Aggregates, facets and counts
//!   retrieve no documents; the rows of a compound query's leaves (joins,
//!   unions) are not counted, nor a profile's repeated runs.
//! - a server counts what it answers its own clients. A coordinator counts
//!   the rows it answers, each by the shard and replica that answered it
//!   (a shard's server does not count what it answers a coordinator, so
//!   nothing counts twice), and the index inspector adds both up.
//! - the totals are exact. The most read keys are a Space-Saving summary
//!   of [`TOP`] keys per table ([`Counts::top`]): every key read more
//!   than `total / TOP` times is in it, each count at most its `err`
//!   above the true one. How many distinct keys were read, and how often
//!   each, come from a hash sample of the keys ([`Counts::sample`]): every
//!   key whose hash ends in `level` zero bits, with its exact count; the
//!   level rises by one, halving the sample, whenever it passes
//!   [`SAMPLE`] keys. Both merge across servers, as the inspector does.
//! - an answer counts at most [`PER_ANSWER`] keyed rows by key; the rest
//!   only in the totals (`skipped`).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};
use chloedb::inspect::{hash64, Dist};
use chloedb::value::Value;

/// Keys a table's most-read summary keeps.
pub const TOP: usize = 256;
/// Keys the hash sample keeps at most.
pub const SAMPLE: usize = 4096;
/// Keyed rows of one answer counted by key.
pub const PER_ANSWER: usize = 10_000;

/// Who served the reads: this server (both empty), or through this
/// coordinator, a shard's replica.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct At {
    pub shard: String,
    pub replica: String,
}

impl At {
    pub fn here() -> At {
        At::default()
    }
}

/// The reads of one table, by one server or replica.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Counts {
    /// Documents retrieved, or keys asked for.
    pub total: u64,
    /// Answers that counted any.
    pub answers: u64,
    /// Rows without a key (a table without a primary key), in `total`.
    pub unkeyed: u64,
    /// Rows of large answers counted only in `total` ([`PER_ANSWER`]).
    pub skipped: u64,
    /// Keys asked for that no row has (reads by key), in `total`.
    pub missing: u64,
    /// The most read keys: each key's count and the most it can be over.
    pub top: HashMap<String, (u64, u64)>,
    /// The hash sample keeps keys whose hash's low `level` bits are 0.
    pub level: u32,
    /// The sampled keys' hashes and their exact counts.
    pub sample: HashMap<u64, u64>,
    /// Where [`Counts::add`] finds the least read key of a full summary.
    #[serde(skip)]
    least: Least,
}

/// Keys of a full summary that held its least count when it was last
/// scanned for it. Counts only grow, and a key that comes in takes more,
/// so while one of them still holds that count it is a least read key; a
/// scan is needed only once none does, which on keys read about evenly
/// (lookups by key over a large table) is about once every [`TOP`]
/// replacements rather than on each. A cache: not part of the counts.
#[derive(Clone, Debug, Default)]
struct Least {
    count: u64,
    keys: Vec<String>,
}

impl PartialEq for Least {
    fn eq(&self, _: &Least) -> bool {
        true
    }
}

impl Counts {
    fn mask(level: u32) -> u64 {
        if level >= 64 {
            u64::MAX
        } else {
            (1u64 << level) - 1
        }
    }

    /// Counts one read of `key`.
    pub fn add(&mut self, key: &str) {
        let full = self.top.len() >= TOP;
        match self.top.get_mut(key) {
            Some(e) => e.0 += 1,
            None if !full => {
                self.top.insert(key.to_string(), (1, 0));
            }
            None => {
                // Space-Saving: the new key takes the least read one's
                // place and count, which is then its error.
                let c = self.evict_least();
                self.top.insert(key.to_string(), (c + 1, c));
            }
        }
        let h = hash64(key.as_bytes());
        if h & Self::mask(self.level) == 0 {
            *self.sample.entry(h).or_default() += 1;
            self.trim();
        }
    }

    /// Takes a least read key out of a full summary and returns its count.
    fn evict_least(&mut self) -> u64 {
        loop {
            while let Some(k) = self.least.keys.pop() {
                if self.top.get(&k).is_some_and(|e| e.0 == self.least.count) {
                    self.top.remove(&k);
                    return self.least.count;
                }
            }
            let c = self.top.values().map(|v| v.0).min().expect("a full summary");
            self.least = Least { count: c, keys: self.top.iter().filter(|(_, v)| v.0 == c).map(|(k, _)| k.clone()).collect() };
        }
    }

    /// Halves the sample until it fits [`SAMPLE`].
    fn trim(&mut self) {
        while self.sample.len() > SAMPLE {
            self.level += 1;
            let m = Self::mask(self.level);
            self.sample.retain(|h, _| h & m == 0);
        }
    }

    /// The least count a full summary holds: what a key it lacks may have.
    fn floor(&self) -> u64 {
        if self.top.len() < TOP {
            0
        } else {
            self.top.values().map(|v| v.0).min().unwrap_or(0)
        }
    }

    /// Adds `o`'s reads: the summaries merge as mergeable summaries do (a
    /// key missing from a full one may have had its floor), the samples at
    /// the higher level.
    pub fn merge(&mut self, o: &Counts) {
        self.total += o.total;
        self.answers += o.answers;
        self.unkeyed += o.unkeyed;
        self.skipped += o.skipped;
        self.missing += o.missing;
        let (fa, fb) = (self.floor(), o.floor());
        let mut top: HashMap<String, (u64, u64)> = HashMap::with_capacity(self.top.len() + o.top.len());
        for (k, &(c, e)) in &self.top {
            let (oc, oe) = o.top.get(k).copied().unwrap_or((fb, fb));
            top.insert(k.clone(), (c + oc, e + oe));
        }
        for (k, &(c, e)) in &o.top {
            top.entry(k.clone()).or_insert((c + fa, e + fa));
        }
        if top.len() > TOP {
            let mut v: Vec<(String, (u64, u64))> = top.into_iter().collect();
            v.sort_unstable_by(|a, b| b.1 .0.cmp(&a.1 .0).then_with(|| a.0.cmp(&b.0)));
            v.truncate(TOP);
            top = v.into_iter().collect();
        }
        self.top = top;
        self.least = Least::default();
        self.level = self.level.max(o.level);
        let m = Self::mask(self.level);
        self.sample.retain(|h, _| h & m == 0);
        for (&h, &c) in &o.sample {
            if h & m == 0 {
                *self.sample.entry(h).or_default() += c;
            }
        }
        self.trim();
    }

    /// For people: the totals, the estimated distinct keys and the share
    /// of `live` documents they are, how often each key was read (a
    /// distribution and a concentration curve), and the `n` most read.
    pub fn summary(&self, live: Option<u64>, n: usize) -> Json {
        let scale = 2f64.powi(self.level as i32);
        let distinct = self.sample.len() as f64 * scale;
        let mut per_key = Dist::default();
        for &c in self.sample.values() {
            per_key.add(c, scale);
        }
        per_key.finish();
        // The concentration curve: the share of reads of the most read
        // fraction x of the keys read, from the sample.
        let mut counts: Vec<u64> = self.sample.values().copied().collect();
        counts.sort_unstable_by(|a, b| b.cmp(a));
        let sum: u64 = counts.iter().sum();
        let share_of_top = |x: f64| -> Option<f64> {
            if counts.is_empty() || sum == 0 {
                return None;
            }
            let k = ((counts.len() as f64 * x).ceil() as usize).clamp(1, counts.len());
            Some(counts[..k].iter().sum::<u64>() as f64 / sum as f64)
        };
        let curve: Vec<Json> = (0..=20).map(|i| i as f64 / 20.0).filter_map(|x| share_of_top(x).map(|y| json!([x, if x == 0.0 { 0.0 } else { y }]))).collect();
        let mut top: Vec<(&String, &(u64, u64))> = self.top.iter().collect();
        top.sort_unstable_by(|a, b| b.1 .0.cmp(&a.1 .0).then_with(|| a.0.cmp(b.0)));
        let total = self.total.max(1) as f64;
        json!({
            "total": self.total, "answers": self.answers, "unkeyed": self.unkeyed, "skipped": self.skipped, "missing": self.missing,
            "distinct": distinct, "exact": self.level == 0,
            "coverage": live.filter(|&l| l > 0).map(|l| (distinct / l as f64).min(1.0)),
            "per_key": per_key,
            "curve": curve,
            "top1": share_of_top(0.01), "top10": share_of_top(0.1),
            "top": top.into_iter().take(n).map(|(k, &(c, e))| json!({"key": k, "count": c, "err": e, "share": c as f64 / total})).collect::<Vec<_>>(),
            "sample": {"keys": self.sample.len(), "level": self.level},
        })
    }
}

/// A key as the summaries name it.
pub fn key_name(v: &Value) -> Option<String> {
    Some(match v {
        Value::Null => return None,
        Value::Str(s) => s.clone(),
        Value::Int(i) | Value::Timestamp(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Bool(b) => b.to_string(),
        other => serde_json::to_string(other).unwrap_or_default(),
    })
}

/// Rows of an answer that one shard's replica served, through a
/// coordinator, by key.
#[derive(Clone, Debug)]
pub struct Served {
    pub at: At,
    pub keys: Vec<Value>,
    /// For reads by key: whether a row has each key.
    pub found: Vec<bool>,
}

/// A table's reads by one server or replica: documents retrieved and
/// keys asked for.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TableReads {
    pub docs: Counts,
    pub keys: Counts,
}

impl TableReads {
    pub fn merge(&mut self, o: &TableReads) {
        self.docs.merge(&o.docs);
        self.keys.merge(&o.keys);
    }
}

/// Every table's reads, by who served them.
pub struct Reads {
    on: AtomicBool,
    tables: Mutex<HashMap<(String, At), TableReads>>,
}

impl Default for Reads {
    fn default() -> Reads {
        Reads { on: AtomicBool::new(true), tables: Mutex::default() }
    }
}

impl Reads {
    pub fn set_on(&self, on: bool) {
        self.on.store(on, Ordering::Relaxed);
    }

    pub fn on(&self) -> bool {
        self.on.load(Ordering::Relaxed)
    }

    fn with(&self, table: &str, at: At, f: impl FnOnce(&mut TableReads)) {
        let mut t = self.tables.lock().unwrap_or_else(|e| e.into_inner());
        f(t.entry((table.to_ascii_lowercase(), at)).or_default());
    }

    /// Counts the documents of `keys` (the primary keys of an answer's
    /// rows, `Null` without one) as retrieved from `table` by `at`.
    pub fn retrieved(&self, table: &str, at: At, keys: &[Value]) {
        if keys.is_empty() || !self.on() {
            return;
        }
        let names: Vec<Option<String>> = keys.iter().take(PER_ANSWER).map(key_name).collect();
        self.with(table, at, |t| {
            let c = &mut t.docs;
            c.answers += 1;
            c.total += keys.len() as u64;
            c.skipped += keys.len().saturating_sub(PER_ANSWER) as u64;
            for n in &names {
                match n {
                    Some(k) => c.add(k),
                    None => c.unkeyed += 1,
                }
            }
        });
    }

    /// Counts `keys` as asked of `table` by key at `at`, `found` saying
    /// which a row has.
    pub fn asked(&self, table: &str, at: At, keys: &[Value], found: &[bool]) {
        if keys.is_empty() || !self.on() {
            return;
        }
        let names: Vec<Option<String>> = keys.iter().take(PER_ANSWER).map(key_name).collect();
        self.with(table, at, |t| {
            let c = &mut t.keys;
            c.answers += 1;
            c.total += keys.len() as u64;
            c.skipped += keys.len().saturating_sub(PER_ANSWER) as u64;
            c.missing += found.iter().filter(|f| !**f).count() as u64;
            for n in names.iter().flatten() {
                c.add(n);
            }
        });
    }

    /// Table `table`'s reads, by who served them.
    pub fn of(&self, table: &str) -> Vec<(At, TableReads)> {
        let t = self.tables.lock().unwrap_or_else(|e| e.into_inner());
        let name = table.to_ascii_lowercase();
        t.iter().filter(|((n, _), _)| *n == name).map(|((_, at), r)| (at.clone(), r.clone())).collect()
    }

    /// Forgets every count.
    pub fn reset(&self) {
        self.tables.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heavy_keys_stay_in_the_summary_within_their_error() {
        let mut c = Counts::default();
        // Zipf-ish: key k read about 10000 / (k + 1) times, 5000 keys.
        let mut truth = HashMap::new();
        for round in 0..3 {
            for k in 0..5000u64 {
                let times = 10_000 / (k + 1) / 3 + (round == 0) as u64;
                for _ in 0..times {
                    c.add(&format!("doc{k}"));
                }
                *truth.entry(format!("doc{k}")).or_insert(0u64) += times;
            }
        }
        let total: u64 = truth.values().sum();
        for (k, &t) in &truth {
            if t > total / TOP as u64 {
                let &(count, err) = c.top.get(k).unwrap_or_else(|| panic!("{k} ({t} reads) is missing"));
                assert!(count >= t && count - err <= t, "{k}: {count} - {err} vs {t}");
            }
        }
        // The sample's distinct keys and per-key counts are near the truth.
        let s = c.summary(Some(10_000), 5);
        let d = s["distinct"].as_f64().unwrap();
        assert!((d / 5000.0 - 1.0).abs() < 0.15, "{d}");
        assert!((s["coverage"].as_f64().unwrap() - 0.5).abs() < 0.08, "{s}");
        assert_eq!(s["top"][0]["key"], "doc0");
    }

    #[test]
    fn keys_read_evenly_keep_space_saving_bounds() {
        // Lookups spread over many more keys than the summary holds: nearly
        // every read replaces a key, the case the cached least keys serve.
        let mut c = Counts::default();
        let mut truth: HashMap<String, u64> = HashMap::new();
        let mut x = 7u64;
        let n = 200_000u64;
        for i in 0..n {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            // A few keys read often among 50,000 read rarely.
            let k = if i % 10 == 0 { format!("hot{}", (x >> 33) % 8) } else { format!("k{}", (x >> 33) % 50_000) };
            c.add(&k);
            *truth.entry(k).or_default() += 1;
        }
        assert_eq!(c.top.len(), TOP);
        // Space-Saving: the counts sum to the reads, each key's count is at
        // least its reads and at most its error above them, and every key
        // read more than n / TOP times is kept.
        assert_eq!(c.top.values().map(|v| v.0).sum::<u64>(), n);
        for (k, &(count, err)) in &c.top {
            let t = truth[k];
            assert!(count >= t && count - err <= t, "{k}: {count} - {err} vs {t}");
        }
        for (k, &t) in &truth {
            if t > n / TOP as u64 {
                assert!(c.top.contains_key(k), "{k} ({t} reads) is missing");
            }
        }
        for h in 0..8 {
            assert!(c.top.contains_key(&format!("hot{h}")));
        }
        // A copy read back counts on as the original does.
        let j = serde_json::to_string(&c).unwrap();
        let mut back: Counts = serde_json::from_str(&j).unwrap();
        assert_eq!(back, c);
        for i in 0..1000 {
            back.add(&format!("new{i}"));
        }
        assert_eq!(back.top.values().map(|v| v.0).sum::<u64>(), n + 1000);
    }

    /// `add`'s time on keys read about evenly, each read replacing a key
    /// (`cargo test --profile ci --lib time_even_reads -- --ignored --nocapture`).
    #[test]
    #[ignore = "timing only"]
    fn time_even_reads() {
        let mut c = Counts::default();
        let keys: Vec<String> = (0..1_000_000u64).map(|i| format!("doc{}", i.wrapping_mul(2654435761) % 1_000_000)).collect();
        let t = std::time::Instant::now();
        for _ in 0..3 {
            for k in &keys {
                c.add(k);
            }
        }
        eprintln!("3M adds over 1M keys: {:.1} ns each", t.elapsed().as_nanos() as f64 / 3e6);
    }

    #[test]
    fn merged_counts_match_counting_together() {
        let (mut a, mut b, mut both) = (Counts::default(), Counts::default(), Counts::default());
        for i in 0..20_000u64 {
            let k = format!("k{}", i % 7000);
            if i % 3 == 0 {
                a.add(&k)
            } else {
                b.add(&k)
            }
            both.add(&k);
        }
        a.total = 20_000 / 3 + 1;
        b.total = 20_000 - a.total;
        a.merge(&b);
        assert_eq!(a.total, 20_000);
        // Same sample at the same level: the same distinct estimate.
        assert_eq!(a.level, both.level);
        assert_eq!(a.sample, both.sample);
        let j = serde_json::to_string(&a).unwrap();
        assert_eq!(serde_json::from_str::<Counts>(&j).unwrap(), a);
    }
}
