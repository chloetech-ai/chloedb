//! Idempotency keys of write requests. A client that sends a key with a
//! write and retries it after losing the answer (a timeout, a dropped
//! connection, a coordinator whose other replica failed, a primary that
//! failed before answering) gets the first attempt's answer instead of
//! writing twice: an `add` applied once, an `insert` that does not fail as
//! a duplicate of itself.
//!
//! A key is recorded when its request commits, by the writer thread, so a
//! client that disconnects mid-write still finds it, in two places:
//!
//! - in memory ([`Keys`]), for [`TTL`], the most recent [`CAPACITY`] of
//!   them, with the whole answer;
//! - in the database, for a request that wrote a row, as a row of
//!   `_sources` written in the request's own transaction ([`record`]):
//!   named [`ROW`] and the key, its offset the hash of the request and the
//!   rows it wrote ([`pack`]). It reaches the
//!   copies with the write, so the copy promoted when a primary fails
//!   before answering knows the key, and so does a server restarted. A
//!   key its memory does not hold is looked up there ([`recorded`]), and
//!   rows older than [`TTL`] are deleted ([`forget_expired`]).

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value as Json;
use strata::db::Db;
use strata::query::Params;
use strata::storage::{WriteTxn, SOURCES};
use strata::value::Value;

use crate::service::Written;
use crate::ApiError;

/// How long a key is remembered.
pub const TTL: Duration = Duration::from_secs(3600);
/// How many keys are remembered at most in memory.
pub const CAPACITY: usize = 100_000;

/// The start of the name of a key's row in `_sources`, one of the
/// server's own marks ([`crate::service::MARK`]), which clients' sources
/// may not take and the server's lists of sources leave out.
pub const ROW: &str = "_strata/idempotency ";

/// How many expired keys' rows one transaction deletes.
const FORGET_BATCH: usize = 1000;

/// What a key says about a request.
#[derive(Debug, PartialEq, Eq)]
pub enum Begin {
    /// Unseen: apply the request.
    New,
    /// Already committed, with this answer.
    Done(Written),
    /// A request with this key is being applied now.
    Busy,
    /// The key was used for different operations.
    Mismatch,
}

#[derive(Default)]
pub struct Keys {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    /// The hash of the key's operations, and what they wrote once
    /// committed (`None` while running).
    keys: HashMap<String, (u64, Option<Written>)>,
    /// Keys in the order they arrived, to forget the oldest.
    order: VecDeque<(Instant, String)>,
}

impl Keys {
    /// Looks `key` up for a request whose operations hash to `hash`,
    /// claiming it if it is new.
    pub fn begin(&self, key: &str, hash: u64) -> Begin {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        while g.order.front().is_some_and(|(t, _)| now.duration_since(*t) > TTL) || g.order.len() > CAPACITY {
            let (_, k) = g.order.pop_front().expect("not empty");
            g.keys.remove(&k);
        }
        match g.keys.get(key) {
            Some((h, _)) if *h != hash => Begin::Mismatch,
            Some((_, Some(n))) => Begin::Done(n.clone()),
            Some((_, None)) => Begin::Busy,
            None => {
                g.keys.insert(key.to_string(), (hash, None));
                g.order.push_back((now, key.to_string()));
                Begin::New
            }
        }
    }

    /// Records the outcome of the request that claimed `key`: what it
    /// wrote, or `None` if it did not commit (the key is then forgotten,
    /// so a retry applies it).
    pub fn finish(&self, key: &str, written: Option<Written>) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        match written {
            Some(n) => {
                if let Some(e) = g.keys.get_mut(key) {
                    e.1 = Some(n);
                }
            }
            None => {
                if g.keys.get(key).is_some_and(|e| e.1.is_none()) {
                    g.keys.remove(key);
                }
            }
        }
    }
}

/// A hash of a request's operations, to tell a retry from a different
/// request that reuses its key: the same in every build, as the server
/// that answers a retry from a key's row may not be the one that wrote it.
pub fn hash_ops(ops: &[Json]) -> u64 {
    twox_hash::XxHash3_64::oneshot(serde_json::to_string(ops).unwrap_or_default().as_bytes())
}

/// A key's row in `_sources`.
pub fn row_name(key: &str) -> String {
    format!("{ROW}{key}")
}

/// A key's row's offset: the high half of the request's hash, and the
/// rows it wrote (at most `u32::MAX`), which a retry answered from the row
/// is told.
pub fn pack(hash: u64, rows: u64) -> i64 {
    ((hash & 0xffff_ffff_0000_0000) | rows.min(u32::MAX as u64)) as i64
}

/// The hash's high half and the rows of an offset [`pack`] made.
pub fn unpack(offset: i64) -> (u64, u64) {
    let v = offset as u64;
    (v & 0xffff_ffff_0000_0000, v & 0xffff_ffff)
}

/// Records `key` of a request whose operations hash to `hash` and wrote
/// `rows`, in its transaction `txn`, at `now` (microseconds since the
/// epoch). The database needs `_sources` first
/// ([`strata::storage::Database::ensure_sources`]).
pub fn record(txn: &mut WriteTxn<'_>, key: &str, hash: u64, rows: u64, now: i64) -> strata::Result<()> {
    txn.set_source(&row_name(key), pack(hash, rows), now)
}

/// What `db` recorded of `key` ([`record`]), for a request whose
/// operations hash to `hash`, if it was recorded less than [`TTL`] ago:
/// the answer to give a retry (the rows it wrote, and the database's last
/// sequence number, which follows the write's), or `Err(())` if the key
/// was used for other operations.
pub fn recorded(db: &Db, key: &str, hash: u64) -> Result<Option<Result<Written, ()>>, ApiError> {
    if db.snapshot().table(SOURCES).is_none() {
        return Ok(None);
    }
    let Some(row) = db.get(SOURCES, &Value::Str(row_name(key)))? else { return Ok(None) };
    let Some(offset) = row.get(1).and_then(Value::as_i64) else { return Ok(None) };
    let updated = match row.get(2) {
        Some(Value::Timestamp(t)) => Some(*t),
        _ => None,
    };
    if updated.is_some_and(|u| strata::timestamp::now().saturating_sub(u) > TTL.as_micros() as i64) {
        return Ok(None);
    }
    let (high, rows) = unpack(offset);
    if high != hash & 0xffff_ffff_0000_0000 {
        return Ok(Some(Err(())));
    }
    Ok(Some(Ok(Written { rows, seq_no: db.storage().seq_no().map(|s| s.seq), ..Default::default() })))
}

/// Deletes the rows of keys recorded more than [`TTL`] before `now`
/// (microseconds since the epoch), [`FORGET_BATCH`] to a transaction,
/// each waiting for the writer until `deadline`; returns how many. Found
/// by their names' prefix and their time, not by reading all of
/// `_sources`. For a server that takes writes: a replica applies its
/// primary's deletes.
pub fn forget_expired(db: &Db, now: i64, deadline: std::time::Instant) -> Result<usize, ApiError> {
    if db.snapshot().table(SOURCES).is_none() {
        return Ok(0);
    }
    let sql = format!("SELECT name FROM {SOURCES} WHERE PREFIX(name, '{ROW}') AND updated < $cutoff LIMIT {FORGET_BATCH}");
    let params = Params::new().set("cutoff", Value::Timestamp(now.saturating_sub(TTL.as_micros() as i64)));
    let mut forgot = 0;
    loop {
        let old: Vec<Value> = db.query(&sql, &params)?.rows.into_iter().filter_map(|mut r| r.pop()).collect();
        if old.is_empty() {
            return Ok(forgot);
        }
        let n = crate::service::wait_for_writer(deadline, || {
            let mut t = db.storage().begin()?;
            let mut sources = t.table(SOURCES)?;
            let mut n = 0;
            for name in &old {
                n += sources.delete(name)? as usize;
            }
            t.commit()?;
            Ok(n)
        })?;
        forgot += n;
        if old.len() < FORGET_BATCH || n == 0 {
            return Ok(forgot);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_get_the_first_answer() {
        let k = Keys::default();
        assert_eq!(k.begin("a", 1), Begin::New);
        assert_eq!(k.begin("a", 1), Begin::Busy);
        assert_eq!(k.begin("a", 2), Begin::Mismatch);
        // The answer comes back whole, with its sequence number.
        let w = Written { rows: 3, seq_no: Some(7), ..Default::default() };
        k.finish("a", Some(w.clone()));
        assert_eq!(k.begin("a", 1), Begin::Done(w.clone()));
        // A request that did not commit can be retried.
        assert_eq!(k.begin("b", 1), Begin::New);
        k.finish("b", None);
        assert_eq!(k.begin("b", 1), Begin::New);
        // A late failure report does not forget a committed key.
        k.finish("a", None);
        assert_eq!(k.begin("a", 1), Begin::Done(w));
    }

    #[test]
    fn a_keys_row_keeps_the_hash_and_the_rows() {
        let h = hash_ops(&[serde_json::json!({"op": "add", "key": "a", "field": "n", "value": 1})]);
        // The same in every build: a promoted copy may run another one.
        assert_eq!(h, hash_ops(&[serde_json::json!({"op": "add", "key": "a", "field": "n", "value": 1})]));
        assert_ne!(h, hash_ops(&[serde_json::json!({"op": "add", "key": "a", "field": "n", "value": 2})]));
        for rows in [0, 1, 1000, u32::MAX as u64] {
            assert_eq!(unpack(pack(h, rows)), (h & 0xffff_ffff_0000_0000, rows));
        }
        assert_eq!(unpack(pack(u64::MAX, u64::MAX)), (0xffff_ffff_0000_0000, u32::MAX as u64), "rows past 2^32 - 1 are kept as that");
        assert_eq!(row_name("req-1"), "_strata/idempotency req-1");
    }
}
