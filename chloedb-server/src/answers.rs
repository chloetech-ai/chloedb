//! The answer cache: a server's recent answers to read queries, kept by
//! what the query asked and by the version of the database it read.
//!
//! A query's key ([`Key`]) is made where the request arrives, from what
//! names the query: its SQL, its parameters, the plan a coordinator sent
//! and the term statistics it sent (HTTP's `/v1/query` and gRPC's `Query`;
//! a server's own queries have none, and are never cached). The answer is
//! kept under that key and the transaction the snapshot reflects
//! ([`chloedb::storage::Snapshot::txn`]), which every write moves on: a
//! write leaves the kept answers unreachable, so an answer served from the
//! cache is the one the query would compute on the same version. Nothing
//! is ever invalidated; old answers fall out as the cache fills, least
//! recently used first.
//!
//! Left out: queries whose answer is not a function of the data and the
//! query alone (the time, random numbers, a language model: [`volatile`]),
//! profiled queries (their steps are measured), scrolls, a coordinator's
//! Arrow leaves and queries that hold their snapshot for a fetch.
//!
//! A hit skips the query slot and the executor; the answer's steps say it
//! came from the cache (`answer cache`).

use chloedb::query::exec::ResultSet;
use chloedb::Value;
use std::collections::{BTreeMap, HashMap};
use std::hash::Hasher;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// The default size of the cache, `--answer-cache-mb`.
pub const DEFAULT_MB: usize = 64;

/// Parts, each with its own lock and an equal share of the bytes.
const PARTS: usize = 16;

/// An answer larger than this share of a part is not kept.
const LARGEST_SHARE: usize = 8;

/// What names a query: 128 bits of a hash of everything its answer
/// depends on besides the data.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Key(u64, u64);

/// Builds a [`Key`] from the parts of a request.
pub struct KeyBuilder(std::hash::DefaultHasher, std::hash::DefaultHasher);

impl Default for KeyBuilder {
    fn default() -> Self {
        // Two SipHashes, told apart by a first word of their own.
        let (mut a, mut b) = (std::hash::DefaultHasher::new(), std::hash::DefaultHasher::new());
        a.write_u64(0x6368_6c6f_6564_6231);
        b.write_u64(0x616e_7377_6572_7332);
        KeyBuilder(a, b)
    }
}

impl KeyBuilder {
    /// Adds a part, with its length, so that parts never run together.
    pub fn part(mut self, b: &[u8]) -> Self {
        for h in [&mut self.0, &mut self.1] {
            h.write_u64(b.len() as u64);
            h.write(b);
        }
        self
    }

    /// Adds a key made before, as one part.
    pub fn key(self, k: Key) -> Self {
        let mut b = [0u8; 16];
        b[..8].copy_from_slice(&k.0.to_le_bytes());
        b[8..].copy_from_slice(&k.1.to_le_bytes());
        self.part(&b)
    }

    /// Adds a query's parameters, in the order of their names.
    pub fn params(mut self, p: &chloedb::query::sql::Params) -> Self {
        let mut names: Vec<&String> = p.0.keys().collect();
        names.sort();
        for n in names {
            self = self.part(n.as_bytes()).value(&p.0[n]);
        }
        self
    }

    fn value(self, v: &Value) -> Self {
        match v {
            Value::Null => self.part(b"n"),
            Value::Int(i) => self.part(b"i").part(&i.to_le_bytes()),
            Value::Float(f) => self.part(b"f").part(&f.to_bits().to_le_bytes()),
            Value::Str(s) => self.part(b"s").part(s.as_bytes()),
            Value::Vector(x) => {
                let bytes: Vec<u8> = x.iter().flat_map(|f| f.to_bits().to_le_bytes()).collect();
                self.part(b"v").part(&bytes)
            }
            Value::List(l) => l.iter().fold(self.part(b"l").part(&(l.len() as u64).to_le_bytes()), |b, v| b.value(v)),
            Value::Bool(b) => self.part(if *b { b"t" } else { b"F" }),
            Value::Timestamp(t) => self.part(b"T").part(&t.to_le_bytes()),
        }
    }

    pub fn finish(self) -> Key {
        Key(self.0.finish(), self.1.finish())
    }
}

/// Whether a query's answer may change with nothing written: it reads the
/// clock, draws random numbers or asks a language model. Read from its
/// text, conservatively (a column named `now_x` is fine; a string holding
/// `now(` only costs the query its caching).
pub fn volatile(sql: &str) -> bool {
    let s = sql.to_ascii_lowercase();
    const CALLS: [&str; 9] = ["now(", "current_timestamp", "current_date", "current_time", "localtime", "random(", "rand(", "uuid", "llm("];
    CALLS.iter().any(|c| s.contains(c))
}

/// A server's answer cache ([`crate::answers`]).
pub struct AnswerCache {
    parts: Vec<Mutex<Part>>,
    /// The bytes each part may hold.
    part_bytes: usize,
    pub hits: AtomicU64,
    pub misses: AtomicU64,
    /// Answers kept, and passed over as too large.
    pub kept: AtomicU64,
    pub too_large: AtomicU64,
}

#[derive(Default)]
struct Part {
    map: HashMap<(Key, u64), Entry>,
    /// Entries by when they were last used, oldest first.
    order: BTreeMap<u64, (Key, u64)>,
    clock: u64,
    bytes: usize,
}

struct Entry {
    rs: Arc<ResultSet>,
    bytes: usize,
    used: u64,
}

impl AnswerCache {
    /// A cache of `mb` megabytes; `None` for 0, no cache.
    pub fn new(mb: usize) -> Option<AnswerCache> {
        (mb > 0).then(|| AnswerCache {
            parts: (0..PARTS).map(|_| Mutex::new(Part::default())).collect(),
            part_bytes: (mb << 20) / PARTS,
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            kept: AtomicU64::new(0),
            too_large: AtomicU64::new(0),
        })
    }

    fn part(&self, key: Key) -> &Mutex<Part> {
        &self.parts[(key.0 as usize) % PARTS]
    }

    /// The answer kept for `key` on version `txn`.
    pub fn get(&self, key: Key, txn: u64) -> Option<Arc<ResultSet>> {
        let mut p = self.part(key).lock().unwrap_or_else(|e| e.into_inner());
        let p = &mut *p;
        p.clock += 1;
        let now = p.clock;
        let got = p.map.get_mut(&(key, txn)).map(|e| {
            let was = std::mem::replace(&mut e.used, now);
            (e.rs.clone(), was)
        });
        match got {
            Some((rs, was)) => {
                p.order.remove(&was);
                p.order.insert(now, (key, txn));
                self.hits.fetch_add(1, Ordering::Relaxed);
                Some(rs)
            }
            None => {
                self.misses.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }

    /// Keeps `rs` as the answer for `key` on version `txn`, without its
    /// steps, making room by dropping the answers least recently used.
    pub fn put(&self, key: Key, txn: u64, rs: &ResultSet) {
        let bytes = size_of(rs);
        if bytes > self.part_bytes / LARGEST_SHARE {
            self.too_large.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let mut kept = rs.clone();
        kept.stats.steps = Vec::new();
        kept.stats.steps_dropped = 0;
        let mut p = self.part(key).lock().unwrap_or_else(|e| e.into_inner());
        let p = &mut *p;
        p.clock += 1;
        let now = p.clock;
        if let Some(old) = p.map.insert((key, txn), Entry { rs: Arc::new(kept), bytes, used: now }) {
            p.order.remove(&old.used);
            p.bytes -= old.bytes;
        }
        p.order.insert(now, (key, txn));
        p.bytes += bytes;
        while p.bytes > self.part_bytes {
            let Some((_, k)) = p.order.pop_first() else { break };
            if let Some(e) = p.map.remove(&k) {
                p.bytes -= e.bytes;
            }
        }
        self.kept.fetch_add(1, Ordering::Relaxed);
    }

    /// The answers kept and their bytes, over every part.
    pub fn size(&self) -> (usize, usize) {
        self.parts.iter().fold((0, 0), |(n, b), p| {
            let p = p.lock().unwrap_or_else(|e| e.into_inner());
            (n + p.map.len(), b + p.bytes)
        })
    }

    /// What `/metrics` shows of the cache.
    pub fn metrics(&self) -> Vec<crate::metrics::Family> {
        use crate::metrics::Family;
        let (n, bytes) = self.size();
        vec![
            Family {
                name: "chloedb_answer_cache_total",
                ty: "counter",
                help: "Read queries the answer cache was asked about (--answer-cache-mb): hit (answered from it), miss (run).",
                values: [("hit", &self.hits), ("miss", &self.misses)].iter().map(|(o, c)| (format!("outcome=\"{o}\""), c.load(Ordering::Relaxed) as f64)).collect(),
            },
            Family::one("chloedb_answer_cache_too_large_total", "counter", "Answers not kept, too large for the cache.", self.too_large.load(Ordering::Relaxed) as f64),
            Family::one("chloedb_answer_cache_answers", "gauge", "Answers the cache holds.", n as f64),
            Family::one("chloedb_answer_cache_bytes", "gauge", "About the bytes of the answers the cache holds.", bytes as f64),
            Family::one("chloedb_answer_cache_limit_bytes", "gauge", "The bytes the cache may hold.", self.capacity() as f64),
        ]
    }

    /// The bytes the cache may hold.
    pub fn capacity(&self) -> usize {
        self.part_bytes * PARTS
    }
}

/// About the bytes an answer takes in memory.
fn size_of(rs: &ResultSet) -> usize {
    fn value(v: &Value) -> usize {
        std::mem::size_of::<Value>()
            + match v {
                Value::Str(s) => s.len(),
                Value::Vector(x) => x.len() * 4,
                Value::List(l) => l.iter().map(value).sum(),
                _ => 0,
            }
    }
    let cells: usize = rs.rows.iter().map(|r| 24 + r.iter().map(value).sum::<usize>()).sum();
    let keys: usize = rs.keys.iter().map(value).sum();
    let plan: usize = rs.stats.plan.iter().map(|l| 24 + l.len()).sum();
    256 + cells + keys + rs.scores.len() * 8 + rs.columns.iter().map(|c| 24 + c.len()).sum::<usize>() + plan
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rs(n: usize, text: &str) -> ResultSet {
        ResultSet {
            columns: vec!["id".into()],
            rows: (0..n).map(|i| vec![Value::Str(format!("{text}{i}"))]).collect(),
            keys: Vec::new(),
            scores: Vec::new(),
            parts: None,
            stats: Default::default(),
        }
    }

    #[test]
    fn an_answer_is_kept_by_its_key_and_its_version() {
        let c = AnswerCache::new(1).unwrap();
        let k = KeyBuilder::default().part(b"SELECT 1").finish();
        let other = KeyBuilder::default().part(b"SELECT").part(b" 1").finish();
        assert_ne!(k, other, "parts do not run together");
        assert!(c.get(k, 7).is_none());
        c.put(k, 7, &rs(3, "a"));
        assert_eq!(c.get(k, 7).unwrap().rows.len(), 3);
        // A write moves the version on: the answer is out of reach.
        assert!(c.get(k, 8).is_none());
        assert!(c.get(other, 7).is_none());
        assert_eq!((c.hits.load(Ordering::Relaxed), c.misses.load(Ordering::Relaxed)), (1, 3));
    }

    #[test]
    fn the_least_recently_used_answers_make_room() {
        let c = AnswerCache::new(1).unwrap();
        let keys: Vec<Key> = (0..4000u32).map(|i| KeyBuilder::default().part(&i.to_le_bytes()).finish()).collect();
        for (i, k) in keys.iter().enumerate() {
            c.put(*k, 1, &rs(4, "passage text of some length "));
            // The first answer, used again and again, stays.
            if i % 10 == 0 {
                assert!(c.get(keys[0], 1).is_some(), "{i}");
            }
        }
        let (n, bytes) = c.size();
        assert!(bytes <= c.capacity() && n < 4000, "{n} answers, {bytes} bytes");
        assert!(c.get(keys[0], 1).is_some());
        assert!(c.get(keys[1], 1).is_none());
        // Too large for its part: not kept.
        c.put(keys[1], 2, &rs(2000, &"x".repeat(100)));
        assert!(c.get(keys[1], 2).is_none());
        assert_eq!(c.too_large.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn queries_that_read_the_clock_or_draw_numbers_are_volatile() {
        assert!(volatile("SELECT id FROM t WHERE ts > NOW() - INTERVAL '1 hour'"));
        assert!(volatile("SELECT id FROM t ORDER BY RANDOM() LIMIT 3"));
        assert!(volatile("SELECT llm('sum up', body) FROM t LIMIT 1"));
        assert!(!volatile("SELECT id FROM t ORDER BY bm25(body, :q) DESC LIMIT 10"));
    }
}
