//! Scrubbing (`--scrub-every-s`): a database server reads every segment of
//! its database against its checksum now and then, at a bounded rate,
//! while it serves ([`chloedb::storage::Database::scrub`]).
//!
//! A query checks a section the first time it reads it, and a merge checks
//! the segments it reads, so damage on disk (bit rot, a bad sector, a byte
//! a failing controller wrote wrong) stays unseen in a segment nobody
//! reads: on a replica that serves few searches it may be found only when
//! that copy becomes the primary. A scrub reads every byte, and what it
//! finds is in `/v1/health` (`damaged`, `scrub`), in the series
//! (`damaged_segments`, which the built-in `damaged` alert watches), and
//! on fleet health (Home), as the copy to replace. `POST /v1/admin/scrub`
//! runs one now, and a coordinator runs one on a copy with `POST
//! /v1/admin/copies/scrub {url}`.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value as Json};

use super::Service;
use crate::ApiError;

/// The bytes a second a scheduled scrub reads at most unless told
/// (`--scrub-mib-s`): a slow disk's share, not its whole speed.
pub const DEFAULT_RATE: u64 = 64 << 20;

/// What scrubbing does here, and what the last scrub found.
#[derive(Default)]
pub struct Scrubs {
    /// How often a scrub starts (zero: only when asked).
    pub every: Duration,
    /// The bytes a second a scheduled scrub reads at most (zero: no limit).
    pub rate: u64,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    running: bool,
    /// The last scrub that finished: when (ms) and what it found.
    last: Option<(u64, chloedb::storage::ScrubReport)>,
}

impl Scrubs {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The damage the last scrub found, in words (none before one ran).
    pub fn damaged(&self) -> Vec<String> {
        self.state().last.as_ref().map_or_else(Vec::new, |(_, r)| r.damaged.clone())
    }

    /// For `/v1/health` (`scrub`).
    pub fn json(&self) -> Json {
        let st = self.state();
        let mut j = json!({"every_s": self.every.as_secs(), "running": st.running});
        if let Some((at, r)) = &st.last {
            j["last_at_ms"] = json!(at);
            j["last"] = json!(r);
        }
        j
    }
}

/// Clears `running` however the scrub ends.
struct Running<'a>(&'a Scrubs);

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.0.state().running = false;
    }
}

/// A random part of `d`.
fn jitter(d: Duration) -> Duration {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
    d.mul_f64((h.finish() % 1_000_000) as f64 / 1_000_000.0)
}

impl Service {
    /// Scrubs the database now ([`chloedb::storage::Database::scrub`]),
    /// reading `rate` bytes a second at most (zero: as fast as it reads);
    /// writes go on meanwhile. Answers the report; 409 while another
    /// scrub runs.
    pub async fn scrub(&self, rate: u64, by: &str) -> Result<Json, ApiError> {
        let db = self.admin_db()?;
        {
            let mut st = self.scrubs.state();
            if st.running {
                return Err(ApiError { code: None, status: 409, message: "a scrub runs already: wait for it (GET /v1/health says when it ends)".into() });
            }
            st.running = true;
        }
        let _running = Running(&self.scrubs);
        tracing::info!(by, rate_mib_s = rate >> 20, "scrub: reading every segment against its checksum");
        let r = tokio::task::spawn_blocking(move || {
            let t0 = Instant::now();
            let mut read = 0u64;
            db.storage().scrub(|b| {
                read += b;
                if rate > 0 {
                    let due = Duration::from_secs_f64(read as f64 / rate as f64);
                    if let Some(wait) = due.checked_sub(t0.elapsed()) {
                        std::thread::sleep(wait);
                    }
                }
            })
        })
        .await
        .map_err(ApiError::internal)??;
        if r.damaged.is_empty() {
            tracing::info!(segments = r.segments, bytes = r.bytes, seconds = r.seconds, "scrub: every segment matches its checksum");
        } else {
            tracing::error!(segments = r.segments, damaged = r.damaged.len(), first = %r.damaged[0], "scrub: segments fail their checksums: this copy's data is damaged on disk; replace the copy, or restore it from a checkpoint");
        }
        let j = json!({"scrub": &r});
        self.scrubs.state().last = Some((crate::telemetry::now_ms(), r));
        Ok(j)
    }

    /// Scrubs every [`Scrubs::every`] while the server serves, at
    /// [`Scrubs::rate`]: the first at a random time within the first
    /// interval (ten minutes at most), so that servers started together,
    /// as in a rolling upgrade, do not all read their disks at once.
    pub fn start_scrubs(self: &Arc<Self>) {
        let every = self.scrubs.every;
        if every.is_zero() || self.local_db().is_none() {
            return;
        }
        let me = Arc::downgrade(self);
        tokio::spawn(async move {
            tokio::time::sleep(jitter(every.min(Duration::from_secs(600)))).await;
            loop {
                let Some(svc) = me.upgrade() else { return };
                let rate = svc.scrubs.rate;
                if let Err(e) = svc.scrub(rate, "the schedule (--scrub-every-s)").await {
                    tracing::warn!(error = %e.message, "scrub: could not run");
                }
                drop(svc);
                tokio::time::sleep(every).await;
            }
        });
    }
}
