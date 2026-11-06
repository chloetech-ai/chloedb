//! Filters kept between queries: the documents a `WHERE` clause keeps in
//! one segment, before deletes, for the queries that repeat it (a search
//! page's fixed filters, a prefix being typed), as Elasticsearch's node
//! query cache and Solr's filter cache keep theirs.
//!
//! A filter is kept the second time a segment sees it, so that a filter
//! used once (a key, a time that moves) takes no room, and the least
//! recently used go first once the cache holds its limit in bytes. Only a
//! filter that reads nothing but the segment's own data is kept
//! ([`super::Executor::cache_key`]); deletes are checked apart, and a
//! field with values written in place is never part of one, so a kept set
//! stays true for as long as its segment's readers live.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use super::SegmentReaders;
use crate::segment::DocSet;

/// The bytes of filters a database keeps by default.
pub const FILTER_CACHE_BYTES: usize = 64 << 20;

/// Times a segment sees a filter before keeping it.
const KEEP_AT: u32 = 2;

/// Filters seen and not kept yet that the cache counts, at most.
const SEEN: usize = 4096;

/// A count in `seen` for a filter too large to keep, so that it is not
/// made again to be refused.
const TOO_BIG: u32 = u32::MAX;

/// Filters kept for every segment of a database, up to a limit in bytes
/// ([`super::ExecOptions::filter_cache`]).
pub struct FilterCache {
    limit: usize,
    inner: Mutex<Inner>,
}

/// A segment's readers (by address) and a filter (its bound predicate,
/// written out).
type Key = (usize, String);

#[derive(Default)]
struct Inner {
    kept: HashMap<Key, Kept>,
    seen: HashMap<Key, u32>,
    bytes: usize,
    clock: u64,
    hits: u64,
}

struct Kept {
    /// The segment's readers: the address in the key is theirs only while
    /// they live.
    readers: Weak<SegmentReaders>,
    set: Arc<DocSet>,
    bytes: usize,
    used: u64,
}

/// What the cache has of a filter of a segment.
pub(super) enum Look {
    /// The filter's documents, deletes not taken out.
    Kept(Arc<DocSet>),
    /// Seen often enough: keep it ([`FilterCache::keep`]).
    Keep,
    /// Not kept.
    No,
}

impl std::fmt::Debug for FilterCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let i = self.lock();
        f.debug_struct("FilterCache").field("limit", &self.limit).field("filters", &i.kept.len()).field("bytes", &i.bytes).finish()
    }
}

impl FilterCache {
    /// A cache of at most `limit` bytes of filters (0 keeps none).
    pub fn new(limit: usize) -> FilterCache {
        FilterCache { limit, inner: Mutex::new(Inner::default()) }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The limit in bytes.
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// The bytes of the filters kept.
    pub fn bytes(&self) -> usize {
        self.lock().bytes
    }

    /// The filters kept.
    pub fn len(&self) -> usize {
        self.lock().kept.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The queries' segments that found their filter kept, since the cache
    /// was made.
    pub fn hits(&self) -> u64 {
        self.lock().hits
    }

    /// Drops every filter kept (e.g. before cold-cache measurements).
    pub fn clear(&self) {
        let mut i = self.lock();
        i.kept.clear();
        i.seen.clear();
        i.bytes = 0;
    }

    /// The filter `filter` of the segment `readers` read, as kept; else
    /// whether to keep it now, counting this time it was seen.
    pub(super) fn look(&self, readers: &Arc<SegmentReaders>, filter: &str) -> Look {
        if self.limit == 0 {
            return Look::No;
        }
        let key = (Arc::as_ptr(readers) as usize, filter.to_string());
        let mut i = self.lock();
        i.clock += 1;
        let now = i.clock;
        if let Some(k) = i.kept.get_mut(&key) {
            if k.readers.upgrade().is_some_and(|r| Arc::ptr_eq(&r, readers)) {
                k.used = now;
                let set = k.set.clone();
                i.hits += 1;
                return Look::Kept(set);
            }
            // Kept for readers since dropped, whose address these took.
            let gone = i.kept.remove(&key).expect("just found");
            i.bytes -= gone.bytes;
        }
        if i.seen.len() >= SEEN && !i.seen.contains_key(&key) {
            i.seen.clear();
        }
        let n = i.seen.entry(key.clone()).or_insert(0);
        if *n == TOO_BIG {
            return Look::No;
        }
        *n += 1;
        if *n >= KEEP_AT {
            i.seen.remove(&key);
            return Look::Keep;
        }
        Look::No
    }

    /// Keeps `set`, the documents of filter `filter` in the segment
    /// `readers` read, making room by dropping the filters used longest
    /// ago; a set larger than a quarter of the limit is not kept, nor made
    /// again until the cache forgets it saw it.
    pub(super) fn keep(&self, readers: &Arc<SegmentReaders>, filter: &str, set: Arc<DocSet>) {
        let bytes = set_bytes(&set);
        let key = (Arc::as_ptr(readers) as usize, filter.to_string());
        let mut i = self.lock();
        if bytes > self.limit / 4 {
            if i.seen.len() < SEEN {
                i.seen.insert(key, TOO_BIG);
            }
            return;
        }
        // Filters of segments whose readers are gone (merged away, or
        // opened again for a new schema) are never found again.
        if i.bytes + bytes > self.limit {
            let mut freed = 0;
            i.kept.retain(|_, k| {
                let live = k.readers.strong_count() > 0;
                if !live {
                    freed += k.bytes;
                }
                live
            });
            i.bytes -= freed;
        }
        while i.bytes + bytes > self.limit {
            let Some(old) = i.kept.iter().min_by_key(|(_, k)| k.used).map(|(key, _)| key.clone()) else { break };
            let k = i.kept.remove(&old).expect("just found");
            i.bytes -= k.bytes;
        }
        i.clock += 1;
        let used = i.clock;
        if let Some(k) = i.kept.insert(key, Kept { readers: Arc::downgrade(readers), set, bytes, used }) {
            i.bytes -= k.bytes;
        }
        i.bytes += bytes;
    }
}

/// What a set takes in memory, about.
fn set_bytes(set: &DocSet) -> usize {
    128 + match set {
        DocSet::Sorted { docs, .. } => docs.len() * std::mem::size_of::<u32>(),
        DocSet::Bits { words, .. } => words.len() * std::mem::size_of::<u64>(),
    }
}
