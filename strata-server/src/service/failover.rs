//! Automatic failover (`--failover-after-ms`): the copies of a shard elect
//! its primary among themselves.
//!
//! A replica that hears nothing from its primary for the failover delay,
//! and a random part of it more (so that two replicas seldom ask at once),
//! asks the servers its cluster map names for its shard to elect it
//! ([`Follow::campaign`]): first whether they would, then for their votes.
//! Each copy grants one vote a term, kept beside its database, and only to
//! a copy holding every transaction it holds; a replica whose primary
//! called within the delay refuses, and so does a primary that serves. A
//! replica a majority elects begins the term and ships to the others.
//! Each round ends once its answers decide it, so that a copy that is
//! down or cut off holds none up, and a replica that granted a vote waits
//! a whole delay from then before it asks itself: the copy it voted for
//! may be a moment from winning, and a newer term asked for would fence
//! it.
//!
//! A write is acknowledged once a majority of the copies hold it (with
//! `--write-acks all` or `majority`), and an election needs a majority of
//! votes from copies that hold no transaction the winner lacks: the two
//! majorities share a copy, so the new primary holds every acknowledged
//! write. A primary that does not hear from a majority of its copies for
//! most of the delay serves no reads ([`crate::replication::PrimaryOptions::lease`]),
//! as they may have elected another; and a primary another replaced becomes
//! a replica of it ([`Service::step_down`]), so that the new primary ships
//! to it.
//!
//! A primary that cannot write until it restarts ([`Service::failing`]: a
//! write failed partway, as on a full disk) hands its shard over to a
//! replica in sync ([`Service::hand_over_failing`]), as an operator would
//! by hand, so that writes go on there at once.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::{conflict, Replication, Service};
use crate::cores::shard::Failover;
use crate::replication::{Follow, Primary, PrimaryOptions, STICKY};
use crate::{pb, ApiError};

/// The replicas a primary that cannot write asks to take over, in that
/// order: those in sync and not being sent a copy, of the latest
/// transaction first, from its `health()`.
pub fn takeover_order(health: &serde_json::Value) -> Vec<String> {
    let mut v: Vec<(u64, String)> = health["replicas"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|f| f["in_sync"] == true && f["copying"] != true && f["member"] != false)
        .filter_map(|f| Some((f["seq_no"].as_u64().unwrap_or(0), f["url"].as_str()?.to_string())))
        .collect();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v.into_iter().map(|(_, u)| u).collect()
}

/// How a primary ships when its copies may elect another after `after`:
/// it calls each replica at least four times within it (so that a replica
/// does not take a quiet primary for a dead one), and holds a lease on
/// reads of three quarters of it, which leaves room for clocks that run
/// apart a little.
pub fn tuned(mut o: PrimaryOptions, after: Duration) -> PrimaryOptions {
    if !after.is_zero() {
        o.lease = after * 3 / 4;
        o.heartbeat = o.heartbeat.min(after / 4);
    }
    o
}

/// A random part of `d`.
fn jitter(d: Duration) -> Duration {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
    d.mul_f64((h.finish() % 1_000_000) as f64 / 1_000_000.0)
}

impl Service {
    /// Answers a copy of this server's shard asking to be made its primary
    /// (`Vote`). A primary that grants a handover steps down at once.
    pub async fn vote(&self, r: pb::VoteRequest) -> Result<pb::VoteResponse, ApiError> {
        match self.replication() {
            Replication::Replica(f) => {
                let sticky = if self.failover_after.is_zero() { STICKY } else { self.failover_after };
                f.vote(&r, sticky).await
            }
            Replication::Primary(p) => {
                let a = p.answer(&r).await;
                if a.granted && !a.candidate && !r.pre {
                    self.step_down().await;
                }
                Ok(a)
            }
            Replication::None => Err(conflict("this server is not replicated: it has no shard copies to elect a primary among".into())),
        }
    }

    /// Makes a primary that another replaced a replica, which follows the
    /// new primary when it ships: the new primary sends it a copy when it
    /// holds transactions the new one does not.
    pub(crate) async fn step_down(&self) {
        let _one = self.promoting.lock().await;
        let Replication::Primary(p) = self.replication() else { return };
        let term = p.replaced_by();
        let Some(db) = self.local().cloned() else { return };
        if term == 0 {
            return;
        }
        match Follow::open(db, self.replication_file.as_deref()) {
            Ok(f) => {
                *self.replication.write().unwrap_or_else(|e| e.into_inner()) = Replication::Replica(Arc::new(f));
                tracing::warn!(replaced_by = term, "a primary of a newer term replaced this one, which is a replica now: it follows the new primary");
            }
            Err(e) => tracing::error!(error = %e, "could not make the replaced primary a replica"),
        }
    }

    /// Runs automatic failover while the server serves, when
    /// [`Self::failover_after`] is set: a replica whose primary is silent
    /// asks to be elected, and a replaced primary steps down, when the
    /// core says ([`Failover`]).
    pub fn start_failover(self: &Arc<Self>) {
        let after = self.failover_after;
        if after.is_zero() {
            return;
        }
        let me = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut f = Failover::new(after, Instant::now(), jitter(after));
            let mut asked: Option<Instant> = None;
            loop {
                tokio::time::sleep(f.every()).await;
                let Some(svc) = me.upgrade() else { return };
                svc.fail_over(&mut f, &mut asked).await;
            }
        });
    }

    /// One look of [`Self::start_failover`]: an election when `fo` says
    /// it is due, which moves the next on; on a primary that cannot
    /// write, a handover, once a failover delay at most (`asked`: when
    /// it last asked).
    async fn fail_over(&self, fo: &mut Failover, asked: &mut Option<Instant>) {
        let f = match self.replication() {
            Replication::Primary(p) if p.replaced_by() > 0 => return self.step_down().await,
            Replication::Primary(p) => {
                if self.failing().is_some() && asked.is_none_or(|t| t.elapsed() >= fo.after) {
                    *asked = Some(Instant::now());
                    self.hand_over_failing(&p, fo.call_timeout(self.primary_options().call_timeout)).await;
                }
                return;
            }
            Replication::Replica(f) => f,
            _ => return,
        };
        let Some((map, shard)) = self.slots() else { return };
        let copies = map.replicas().get(shard).cloned().unwrap_or_default();
        if !fo.due(copies.len(), f.silent_for(), Instant::now()) {
            return;
        }
        let _one = self.promoting.lock().await;
        // Promoted meanwhile, or its primary called.
        if !matches!(self.replication(), Replication::Replica(_)) || f.silent_for() < fo.after {
            return;
        }
        fo.started(Instant::now(), jitter(fo.after));
        let mut opts = self.primary_options();
        tracing::warn!(silent_ms = f.silent_for().as_millis() as u64, "its primary is silent: asking its shard's copies to elect this replica");
        // A copy that does not answer within the delay is taken for down.
        let ask = PrimaryOptions { call_timeout: fo.call_timeout(opts.call_timeout), ..opts.clone() };
        let won = match f.campaign(&copies, &ask, false, &[]).await {
            Ok(e) => e,
            Err(e) => {
                tracing::info!(error = %e.message, "not elected");
                return;
            }
        };
        opts.replicas = won.others.clone();
        match f.take_over(Some(won.term), opts).await {
            Ok((p, _)) => {
                *self.replication.write().unwrap_or_else(|e| e.into_inner()) = Replication::Primary(p);
                self.models_changed.send_modify(|n| *n += 1);
                tracing::warn!(term = won.term, replicas = %won.others.join(", "), "elected: this server is its shard's primary");
            }
            Err(e) => tracing::warn!(error = %e.message, "elected, but could not begin the term"),
        }
    }

    /// Asks a replica in sync to be promoted, as `POST
    /// /v1/admin/shards/{i}/promote` does from a coordinator, while this
    /// primary cannot write until it restarts ([`Self::failing`]): the
    /// replica asks the shard's copies for their votes, this primary grants
    /// its own and steps down ([`Self::vote`]), and writes go on at the new
    /// primary (finding chaos-4: a full disk on a primary stopped its
    /// shard's writes until an operator handed it over by hand). This
    /// server then follows it, and serves again once the cause is fixed and
    /// it restarts. It holds no lock while it asks: the vote it is asked
    /// for comes in meanwhile.
    pub(crate) async fn hand_over_failing(&self, p: &Primary, within: Duration) {
        let Some(why) = self.failing() else { return };
        let opts = self.primary_options();
        let order = takeover_order(&p.health());
        if order.is_empty() {
            tracing::debug!(cause = %why, "this primary cannot write, and no replica is in sync to hand its shard over to");
            return;
        }
        for url in order {
            let mut c = match crate::coord::client(&url, opts.token.as_ref(), opts.ca.as_deref()) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(replica = %url, error = %e.message, "cannot call the replica to hand over to");
                    continue;
                }
            };
            let r = pb::ConfigureRequest { role: pb::Role::Primary as i32, promote: true, ..Default::default() };
            match tokio::time::timeout(within.max(Duration::from_secs(5)), c.configure(r)).await {
                Ok(Ok(a)) => {
                    let a = a.into_inner();
                    tracing::warn!(cause = %why, to = %url, term = a.term, "this primary cannot write: it handed its shard over to a replica in sync");
                    return;
                }
                Ok(Err(s)) => tracing::warn!(cause = %why, to = %url, error = %s.message(), "this primary cannot write, and the replica it asked to take over was not promoted"),
                Err(_) => tracing::warn!(cause = %why, to = %url, "this primary cannot write, and the replica it asked to take over did not answer"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_failing_primary_asks_its_replicas_in_sync_newest_first() {
        let h = json!({"replicas": [
            {"url": "http://a", "in_sync": true, "seq_no": 90},
            {"url": "http://b", "in_sync": false, "seq_no": 99},
            {"url": "http://c", "in_sync": true, "seq_no": 95},
            {"url": "http://d", "in_sync": true, "copying": true, "seq_no": 99},
            {"url": "http://e", "in_sync": true, "member": false, "seq_no": 99},
        ]});
        assert_eq!(takeover_order(&h), ["http://c", "http://a"]);
        assert!(takeover_order(&json!({})).is_empty());
    }
}
