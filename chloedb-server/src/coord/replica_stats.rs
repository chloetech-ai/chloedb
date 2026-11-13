//! What a coordinator observes of each replica it calls: calls in flight,
//! round-trip time (of every call, and of queries alone), the part of a
//! query's round trip the replica did not count in its own time (overhead:
//! the network and decoding; a replica's `took_ms` counts its admission
//! wait and its run), how often calls fail or are refused for load, and
//! the load the replica last reported of itself.
//!
//! It is shown (`/v1/cluster`, `/metrics`), and with `--route least-cost`
//! the order a shard's copies are asked in is decided by it
//! ([`crate::cores::route`], from [`ReplicaStats::route_view`]).
//!
//! Averages decay with time, and with the number of samples when those
//! come faster: a sample `dt` after the one before moves an average by
//! `1 - exp(-dt / tau)` of the difference, but by at least `1 / n` of it
//! while fewer than `cap` samples were taken, and `1 / cap` after. So an
//! average covers the last `tau` or the last `cap` samples, whichever is
//! shorter: a replica asked rarely is averaged over time, one asked often
//! over its last calls, and its first call (which may have opened the
//! connection or asked `Hello` first) does not stand for it for long, as
//! it would by time alone when calls come a millisecond apart. A fast
//! average ([`FAST`]) follows the last seconds or 20 calls, a slow one
//! ([`SLOW`]) the last minute or 200 calls. [`Ewma`] takes the time as an
//! argument, so its arithmetic is tested without a clock.
//!
//! A call that another replica outran (a hedge) or that the request
//! dropped never finished, so its time is only a lower bound: it raises an
//! average that is below it and leaves one above it alone
//! ([`Ewma::at_least`]), rather than counting as a full sample either way.
//! A replica slow enough to be outrun every time thus still looks slow.
//!
//! Reads that must see a write (`min_seq_nos`) may wait on a replica for
//! it, so their round trips say how far behind it is, not how fast it
//! answers. Besides the average of every query (which `--route
//! least-cost` alone routes by, as before), plain queries and such reads
//! are averaged apart, and the newest write each replica answered a read
//! after is kept: it holds that one ([`ReplicaStats::held`]). With
//! `--prefer-caught-up` a read after a write is routed by the second
//! unless the replica is known to hold the write, and other reads by the
//! first ([`ReplicaStats::route_view`]).
//!
//! A replica's tail ([`ReplicaStats::hedge_ms`], for `--hedge-adaptive`)
//! is the slow average of its plain query round trips plus
//! [`TAIL_DEVIATIONS`] times their slow average deviation from it, without
//! keeping samples: well past the 99th percentile of how its queries vary,
//! so that hedges go to a replica slower than usual, not to queries that
//! take long anywhere (three deviations, about the 95th percentile, hedged
//! one call in twenty on the variety of queries alone and spent the hedge
//! budget on them).

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value as Json};

use crate::cores::route;

/// The span of the fast averages: a time constant and a count of samples.
pub const FAST: Window = Window { tau: Duration::from_secs(2), cap: 20 };
/// The span of the slow averages and of the error rate.
pub const SLOW: Window = Window { tau: Duration::from_secs(30), cap: 200 };

/// What an average covers: the last `tau`, or the last `cap` samples when
/// those came within it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Window {
    pub tau: Duration,
    pub cap: u32,
}

/// A round trip older than this is not routed by: the replica counts as
/// unknown (the median of the others), is asked in its turn again, and so
/// measured again. A replica that a bad moment made look slow is not
/// passed over for longer than this.
pub const RTT_FRESH: Duration = Duration::from_secs(10);
/// Plain queries a replica answered before its tail is taken for a hedge
/// delay.
pub const TAIL_SAMPLES: u32 = 20;
/// Deviations above the average that a tail is.
pub const TAIL_DEVIATIONS: f64 = 6.0;
/// How long a replica's slow average of reads after a write is routed by
/// once its fast one is old: a replica found behind is asked such a read
/// again this often to see whether it caught up (each costs a wait),
/// rather than every [`RTT_FRESH`]. Plain reads still go to it.
pub const AFTER_FRESH: Duration = Duration::from_secs(60);
/// An error rate older than this is not routed by.
pub const ERRORS_FRESH: Duration = Duration::from_secs(120);
/// Load a replica reported longer ago than this is not routed by (the
/// coordinator surveys every 5 s).
pub const LOAD_FRESH: Duration = Duration::from_secs(15);

/// An average decaying with time; `None` before its first sample.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ewma {
    value: Option<f64>,
    at: Option<Instant>,
    /// Samples taken, up to the window's cap.
    n: u32,
}

impl Ewma {
    pub fn value(&self) -> Option<f64> {
        self.value
    }

    /// The value, when its last sample is no older than `fresh` at `now`.
    pub fn fresh(&self, now: Instant, fresh: Duration) -> Option<f64> {
        self.value.filter(|_| self.at.is_some_and(|at| now.saturating_duration_since(at) <= fresh))
    }

    /// The weight of the `n`-th sample (from 1), `dt` after the one
    /// before, in window `w`.
    fn weight(dt: Duration, n: u32, w: Window) -> f64 {
        let by_time = 1.0 - (-dt.as_secs_f64() / w.tau.as_secs_f64().max(1e-9)).exp();
        let by_count = 1.0 / n.clamp(1, w.cap.max(1)) as f64;
        by_time.max(by_count)
    }

    /// Takes sample `x` at `now`.
    pub fn sample(&mut self, x: f64, now: Instant, w: Window) {
        self.n = self.n.saturating_add(1).min(w.cap.max(1));
        self.value = Some(match (self.value, self.at) {
            (Some(v), Some(at)) => v + Self::weight(now.saturating_duration_since(at), self.n, w) * (x - v),
            _ => x,
        });
        self.at = Some(now);
    }

    /// Takes the knowledge that a value was at least `x` at `now`: a sample
    /// of `x` when the average is below it (or has none), else nothing.
    pub fn at_least(&mut self, x: f64, now: Instant, w: Window) {
        if self.value.is_none_or(|v| v < x) {
            self.sample(x, now, w);
        }
    }
}

/// The load a replica reported of itself in the coordinator's survey.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Reported {
    /// Its query slots taken (0 to 1).
    pub slots: Option<f64>,
    /// Its query memory used (0 to 1).
    pub memory: Option<f64>,
    pub at: Instant,
}

impl Reported {
    /// From a replica's `Health` detail: `load` (its latest point, absent
    /// until it sampled twice) and `limits`.
    pub fn of(detail: &Json, at: Instant) -> Option<Reported> {
        let load = &detail["load"];
        let max = detail["limits"]["max_concurrent"].as_f64().filter(|&m| m > 0.0);
        let slots = load["slots_free"].as_f64().zip(max).map(|(free, max)| (1.0 - free / max).clamp(0.0, 1.0));
        let memory = load["query_memory_ratio"].as_f64().map(|r| r.clamp(0.0, 1.0));
        (slots.is_some() || memory.is_some()).then_some(Reported { slots, memory, at })
    }

    /// How busy: its slots or its memory, whichever is fuller.
    pub fn busy(&self) -> Option<f64> {
        match (self.slots, self.memory) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        }
    }
}

/// The averages of one replica, behind one lock.
#[derive(Clone, Copy, Debug, Default)]
struct Averages {
    rtt_fast: Ewma,
    rtt_slow: Ewma,
    /// Of queries alone, which a shard's copies are compared by: other
    /// calls (statistics, fetches, gets) cost differently.
    query_fast: Ewma,
    query_slow: Ewma,
    overhead_fast: Ewma,
    overhead_slow: Ewma,
    /// Of queries that must see no write (plain), with their deviation
    /// from the slow average; and of reads that must see one (`after`).
    plain_fast: Ewma,
    plain_slow: Ewma,
    plain_dev: Ewma,
    after_fast: Ewma,
    after_slow: Ewma,
    /// 1 for a call that failed by the replica's fault, 0 for one that
    /// answered.
    errors: Ewma,
    /// 1 for a call refused for the replica's load, 0 for one answered.
    refusals: Ewma,
    reported: Option<Reported>,
}

/// What a coordinator observes of one replica.
#[derive(Debug, Default)]
pub(crate) struct ReplicaStats {
    inflight: AtomicU32,
    /// Calls by how they ended.
    ok: AtomicU64,
    failed: AtomicU64,
    /// Refused for the replica's load (429), which is not a failure.
    refused: AtomicU64,
    /// Ended without an answer: outrun by another replica, or dropped
    /// with the request.
    unfinished: AtomicU64,
    /// No answer within the shard's time.
    timeouts: AtomicU64,
    /// Queries it answered: the first is a warm-up, not a sample of its
    /// round trip (it may have asked `Hello` first, opened the connection,
    /// or found the replica's caches cold).
    queries: AtomicU64,
    /// The newest write it answered a read after: it holds that one.
    held: AtomicU64,
    averages: Mutex<Averages>,
}

/// How a call to a replica ended, as far as its stats go.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Ended {
    /// An answer, and the time the replica said it spent on it, when the
    /// answer says so (a query's `took_ms`).
    Answered { took_ms: Option<f64> },
    /// A failure another replica might not have had: the replica's fault.
    Failed,
    /// Refused for its load (429).
    Refused,
    /// A failure that says nothing of the replica (the request was wrong,
    /// or it was passed over without being sent the request).
    NotItsFault,
    /// It refused the read for now (it serves no reads, or did not get
    /// the write the read must see in time): not a failure, but what a
    /// read after a write paid there.
    Unavailable,
}

impl ReplicaStats {
    /// Counts a call as started (`query`: a query, whose round trip copies
    /// are compared by; `min`: the write it must see, 0 for none); the
    /// guard counts it as ended when it is dropped, as unfinished unless
    /// [`InFlight::ended`] was called.
    pub fn start(self: &Arc<Self>, query: bool, min: u64) -> InFlight {
        self.inflight.fetch_add(1, Ordering::Relaxed);
        InFlight { stats: self.clone(), t0: Instant::now(), query, min, done: false }
    }

    /// Counts a call that got no answer within the shard's time (besides
    /// what its guard records when it is dropped).
    pub fn timed_out(&self) {
        self.timeouts.fetch_add(1, Ordering::Relaxed);
        let mut a = self.averages.lock().unwrap_or_else(|e| e.into_inner());
        a.errors.sample(1.0, Instant::now(), SLOW);
    }

    /// Keeps the load the replica reported of itself.
    pub fn reported(&self, r: Option<Reported>) {
        if let Some(r) = r {
            self.averages.lock().unwrap_or_else(|e| e.into_inner()).reported = Some(r);
        }
    }

    fn end(&self, rtt: Duration, query: bool, min: u64, ended: Option<Ended>, now: Instant) {
        let ms = rtt.as_secs_f64() * 1e3;
        let counter = match ended {
            Some(Ended::Answered { .. }) => &self.ok,
            Some(Ended::Failed) => &self.failed,
            Some(Ended::Refused) => &self.refused,
            Some(Ended::Unavailable) if query && min > 0 => {
                let mut a = self.averages.lock().unwrap_or_else(|e| e.into_inner());
                a.after_fast.sample(ms, now, FAST);
                a.after_slow.sample(ms, now, SLOW);
                drop(a);
                self.inflight.fetch_sub(1, Ordering::Relaxed);
                return;
            }
            Some(Ended::NotItsFault | Ended::Unavailable) => {
                self.inflight.fetch_sub(1, Ordering::Relaxed);
                return;
            }
            None => &self.unfinished,
        };
        counter.fetch_add(1, Ordering::Relaxed);
        {
            let mut a = self.averages.lock().unwrap_or_else(|e| e.into_inner());
            match ended {
                Some(Ended::Answered { took_ms }) => {
                    a.rtt_fast.sample(ms, now, FAST);
                    a.rtt_slow.sample(ms, now, SLOW);
                    if query && self.queries.fetch_add(1, Ordering::Relaxed) > 0 {
                        a.query_fast.sample(ms, now, FAST);
                        a.query_slow.sample(ms, now, SLOW);
                        if min > 0 {
                            a.after_fast.sample(ms, now, FAST);
                            a.after_slow.sample(ms, now, SLOW);
                        } else {
                            let dev = a.plain_slow.value().map(|m| (ms - m).abs());
                            a.plain_fast.sample(ms, now, FAST);
                            a.plain_slow.sample(ms, now, SLOW);
                            if let Some(d) = dev {
                                a.plain_dev.sample(d, now, SLOW);
                            }
                        }
                    }
                    if query && min > 0 {
                        self.held.fetch_max(min, Ordering::Relaxed);
                    }
                    if let Some(took) = took_ms {
                        let overhead = (ms - took).max(0.0);
                        a.overhead_fast.sample(overhead, now, FAST);
                        a.overhead_slow.sample(overhead, now, SLOW);
                    }
                    a.errors.sample(0.0, now, SLOW);
                    a.refusals.sample(0.0, now, FAST);
                }
                Some(Ended::Failed) => a.errors.sample(1.0, now, SLOW),
                // Load is not a failure, and a refusal's time says nothing
                // of how long a query takes there.
                Some(Ended::Refused) => a.refusals.sample(1.0, now, FAST),
                Some(Ended::NotItsFault | Ended::Unavailable) => {}
                None => {
                    a.rtt_fast.at_least(ms, now, FAST);
                    a.rtt_slow.at_least(ms, now, SLOW);
                    if query {
                        a.query_fast.at_least(ms, now, FAST);
                        a.query_slow.at_least(ms, now, SLOW);
                        if min > 0 {
                            a.after_fast.at_least(ms, now, FAST);
                            a.after_slow.at_least(ms, now, SLOW);
                        } else {
                            a.plain_fast.at_least(ms, now, FAST);
                        }
                    }
                }
            }
        }
        self.inflight.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn inflight(&self) -> u32 {
        self.inflight.load(Ordering::Relaxed)
    }

    /// The fast and slow overhead, in ms.
    pub fn overhead_ms(&self) -> (Option<f64>, Option<f64>) {
        let a = self.averages.lock().unwrap_or_else(|e| e.into_inner());
        (a.overhead_fast.value(), a.overhead_slow.value())
    }

    /// The fast average of query round trips, in ms.
    pub fn query_rtt_ms(&self) -> Option<f64> {
        self.averages.lock().unwrap_or_else(|e| e.into_inner()).query_fast.value()
    }

    /// The share of recent calls that failed by the replica's fault.
    pub fn error_rate(&self) -> Option<f64> {
        self.averages.lock().unwrap_or_else(|e| e.into_inner()).errors.value()
    }

    /// The newest write it answered a read after, which it holds.
    pub fn held(&self) -> u64 {
        self.held.load(Ordering::Relaxed)
    }

    /// Its tail, in ms: the slow average of its plain query round trips
    /// plus [`TAIL_DEVIATIONS`] times their deviation, once it answered
    /// [`TAIL_SAMPLES`] of them.
    pub fn hedge_ms(&self) -> Option<f64> {
        let a = self.averages.lock().unwrap_or_else(|e| e.into_inner());
        if a.plain_slow.n < TAIL_SAMPLES {
            return None;
        }
        Some(a.plain_slow.value()? + TAIL_DEVIATIONS * a.plain_dev.value().unwrap_or(0.0))
    }

    /// What the order of a shard's copies is decided by, of this one at
    /// `now` ([`crate::cores::route`]); `avoided`: in its cool-off; `min`:
    /// the write the read must see (0 for none). With `split` (`--prefer-
    /// caught-up`) a read after a write it is not known to hold goes by
    /// its round trips of such reads (by its plain ones while it has none
    /// recent), and every other read by its plain ones; without, every
    /// read by every query's, as before. `load`: count what it reported.
    pub fn route_view(&self, now: Instant, avoided: bool, min: u64, split: bool, load: bool) -> route::Copy {
        let a = *self.averages.lock().unwrap_or_else(|e| e.into_inner());
        let rtt_ms = match (split, min > 0 && self.held() < min) {
            (false, _) => a.query_fast.fresh(now, RTT_FRESH),
            (true, false) => a.plain_fast.fresh(now, RTT_FRESH),
            (true, true) => a.after_fast.fresh(now, RTT_FRESH).or(a.after_slow.fresh(now, AFTER_FRESH)).or(a.plain_fast.fresh(now, RTT_FRESH)),
        };
        route::Copy {
            avoided,
            rtt_ms,
            inflight: self.inflight(),
            errors: a.errors.fresh(now, ERRORS_FRESH).unwrap_or(0.0),
            refusals: a.refusals.fresh(now, SLOW.tau).unwrap_or(0.0),
            busy: a.reported.filter(|r| load && now.saturating_duration_since(r.at) <= LOAD_FRESH).and_then(|r| r.busy()),
        }
    }

    /// For `/v1/cluster`.
    pub fn json(&self) -> Json {
        let a = *self.averages.lock().unwrap_or_else(|e| e.into_inner());
        let n = |c: &AtomicU64| c.load(Ordering::Relaxed);
        let ms = |e: Ewma| e.value().map(|v| (v * 1000.0).round() / 1000.0);
        let reported = a.reported.map(|r| json!({"slots": r.slots, "memory": r.memory, "age_ms": r.at.elapsed().as_millis() as u64}));
        json!({
            "inflight": self.inflight(),
            "rtt_ms": {"fast": ms(a.rtt_fast), "slow": ms(a.rtt_slow)},
            "query_rtt_ms": {"fast": ms(a.query_fast), "slow": ms(a.query_slow)},
            "overhead_ms": {"fast": ms(a.overhead_fast), "slow": ms(a.overhead_slow)},
            "plain_rtt_ms": {"fast": ms(a.plain_fast), "slow": ms(a.plain_slow), "dev": ms(a.plain_dev)},
            "after_write_rtt_ms": {"fast": ms(a.after_fast), "slow": ms(a.after_slow)},
            "held": self.held(),
            "tail_ms": self.hedge_ms().map(|v| (v * 1000.0).round() / 1000.0),
            "error_rate": a.errors.value(),
            "refusal_rate": a.refusals.value(),
            "reported": reported,
            "calls": {"ok": n(&self.ok), "failed": n(&self.failed), "refused": n(&self.refused), "unfinished": n(&self.unfinished), "timeouts": n(&self.timeouts)},
        })
    }
}

/// A call in flight to a replica ([`ReplicaStats::start`]).
pub(crate) struct InFlight {
    stats: Arc<ReplicaStats>,
    t0: Instant,
    query: bool,
    min: u64,
    done: bool,
}

impl InFlight {
    /// Records how the call ended.
    pub fn ended(mut self, ended: Ended) {
        self.done = true;
        self.stats.end(self.t0.elapsed(), self.query, self.min, Some(ended), Instant::now());
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        if !self.done {
            self.stats.end(self.t0.elapsed(), self.query, self.min, None, Instant::now());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_sample_is_the_average() {
        let mut e = Ewma::default();
        assert_eq!(e.value(), None);
        e.sample(10.0, Instant::now(), FAST);
        assert_eq!(e.value(), Some(10.0));
    }

    #[test]
    fn covers_the_last_tau_or_the_last_cap_samples() {
        let t0 = Instant::now();
        let w = Window { tau: Duration::from_secs(2), cap: 20 };
        // Past the cap, one sample a tau later moves the average by
        // 1 - 1/e of the way.
        let mut e = Ewma::default();
        for _ in 0..w.cap {
            e.sample(0.0, t0, w);
        }
        e.sample(100.0, t0 + w.tau, w);
        let v = e.value().unwrap();
        assert!((v - 100.0 * (1.0 - (-1.0f64).exp())).abs() < 1e-9, "{v}");
        // Samples a moment apart: a running mean up to the cap...
        let mut e = Ewma::default();
        e.sample(0.0, t0, w);
        e.sample(10.0, t0, w);
        assert_eq!(e.value(), Some(5.0));
        e.sample(20.0, t0, w);
        assert!((e.value().unwrap() - 10.0).abs() < 1e-9);
        // ...so a first sample does not stand for long: after a cap's worth
        // of others it weighs little.
        let mut e = Ewma::default();
        e.sample(1000.0, t0, w);
        for _ in 0..3 * w.cap {
            e.sample(1.0, t0, w);
        }
        assert!(e.value().unwrap() < 50.0, "{e:?}");
        // A sample long after replaces it.
        e.sample(100.0, t0 + 100 * w.tau, w);
        assert!(e.value().unwrap() > 99.99);
    }

    #[test]
    fn a_lower_bound_only_raises() {
        let t0 = Instant::now();
        let mut e = Ewma::default();
        e.at_least(5.0, t0, FAST);
        assert_eq!(e.value(), Some(5.0));
        // Long enough after that the average is about 50.
        let t1 = t0 + 20 * FAST.tau;
        e.sample(50.0, t1, FAST);
        let v = e.value();
        assert!(v.unwrap() > 49.9);
        e.at_least(10.0, t1 + FAST.tau, FAST);
        assert_eq!(e.value(), v, "below the average: no change");
        e.at_least(500.0, t1 + FAST.tau, FAST);
        assert!(e.value().unwrap() > v.unwrap());
    }

    #[test]
    fn an_old_value_is_not_fresh() {
        let t0 = Instant::now();
        let mut e = Ewma::default();
        e.sample(7.0, t0, FAST);
        assert_eq!(e.fresh(t0 + RTT_FRESH, RTT_FRESH), Some(7.0));
        assert_eq!(e.fresh(t0 + RTT_FRESH + Duration::from_millis(1), RTT_FRESH), None);
        assert_eq!(Ewma::default().fresh(t0, RTT_FRESH), None);
    }

    #[test]
    fn calls_are_counted_and_averaged() {
        let s = Arc::new(ReplicaStats::default());
        let a = s.start(true, 0);
        let b = s.start(false, 0);
        let c = s.start(true, 0);
        let d = s.start(true, 0);
        assert_eq!(s.inflight(), 4);
        a.ended(Ended::Answered { took_ms: Some(0.0) });
        b.ended(Ended::Failed);
        c.ended(Ended::Refused);
        drop(d);
        assert_eq!(s.inflight(), 0);
        let j = s.json();
        assert_eq!(j["calls"], json!({"ok": 1, "failed": 1, "refused": 1, "unfinished": 1, "timeouts": 0}));
        // One answer and one failure, a moment apart: about half and half.
        let rate = s.error_rate().unwrap();
        assert!(rate > 0.0 && rate <= 1.0, "{rate}");
        assert!(s.overhead_ms().0.is_some());
        // The first answer to a query is a warm-up; a lower bound counts.
        assert!(s.query_rtt_ms().is_some());
        assert!(j["refusal_rate"].as_f64().unwrap() > 0.0, "{j}");
    }

    #[test]
    fn the_first_query_answered_is_a_warm_up() {
        let s = Arc::new(ReplicaStats::default());
        s.start(true, 0).ended(Ended::Answered { took_ms: None });
        assert_eq!(s.query_rtt_ms(), None);
        s.start(true, 0).ended(Ended::Answered { took_ms: None });
        assert!(s.query_rtt_ms().is_some());
    }

    #[test]
    fn only_queries_make_the_query_round_trip() {
        let s = Arc::new(ReplicaStats::default());
        s.start(false, 0).ended(Ended::Answered { took_ms: None });
        drop(s.start(false, 0));
        assert_eq!(s.query_rtt_ms(), None);
        assert!(s.json()["rtt_ms"]["fast"].as_f64().is_some());
        // A query outrun is a lower bound of it.
        drop(s.start(true, 0));
        assert!(s.query_rtt_ms().is_some());
    }

    #[test]
    fn not_its_fault_records_nothing() {
        let s = Arc::new(ReplicaStats::default());
        s.start(true, 0).ended(Ended::NotItsFault);
        assert_eq!(s.inflight(), 0);
        assert_eq!(s.error_rate(), None);
        assert_eq!(s.json()["calls"]["failed"], 0);
    }

    #[test]
    fn overhead_is_rtt_less_the_replica_time_never_negative() {
        let s = Arc::new(ReplicaStats::default());
        // The replica says it took longer than the round trip (clocks):
        // the overhead is 0, not negative.
        s.start(true, 0).ended(Ended::Answered { took_ms: Some(1e9) });
        assert_eq!(s.overhead_ms().0, Some(0.0));
        // An answer that says nothing of its time leaves overhead alone.
        let s = Arc::new(ReplicaStats::default());
        s.start(true, 0).ended(Ended::Answered { took_ms: None });
        assert_eq!(s.overhead_ms(), (None, None));
    }

    #[test]
    fn reported_load_is_read_from_a_health_detail_and_goes_stale() {
        let t0 = Instant::now();
        let detail = json!({"load": {"slots_free": 12.0, "query_memory_ratio": 0.5}, "limits": {"max_concurrent": 16}});
        let r = Reported::of(&detail, t0).unwrap();
        assert_eq!((r.slots, r.memory, r.busy()), (Some(0.25), Some(0.5), Some(0.5)));
        // No point yet (a server samples twice first), or no limits.
        assert_eq!(Reported::of(&json!({"limits": {"max_concurrent": 16}}), t0), None);
        let only_memory = Reported::of(&json!({"load": {"slots_free": 3.0, "query_memory_ratio": 0.2}}), t0).unwrap();
        assert_eq!(only_memory.busy(), Some(0.2));
        let s = ReplicaStats::default();
        s.reported(Some(r));
        assert_eq!(s.route_view(t0, false, 0, false, true).busy, Some(0.5));
        assert_eq!(s.route_view(t0 + LOAD_FRESH + Duration::from_secs(1), false, 0, false, true).busy, None);
        // A survey that found no load keeps the last.
        s.reported(None);
        assert_eq!(s.route_view(t0, false, 0, false, true).busy, Some(0.5));
    }

    #[test]
    fn the_tail_is_known_after_enough_plain_queries_and_follows_their_spread() {
        let s = Arc::new(ReplicaStats::default());
        // The first is a warm-up; then 19 samples: not yet.
        for _ in 0..TAIL_SAMPLES {
            s.end(Duration::from_millis(2), true, 0, Some(Ended::Answered { took_ms: None }), Instant::now());
        }
        assert_eq!(s.hedge_ms(), None);
        s.end(Duration::from_millis(2), true, 0, Some(Ended::Answered { took_ms: None }), Instant::now());
        let even = s.hedge_ms().unwrap();
        assert!((even - 2.0).abs() < 0.01, "alike: the average: {even}");
        // Spread raises it above the average.
        for k in 0..100 {
            let ms = if k % 10 == 0 { 20 } else { 2 };
            s.end(Duration::from_millis(ms), true, 0, Some(Ended::Answered { took_ms: None }), Instant::now());
        }
        let spread = s.hedge_ms().unwrap();
        assert!(spread > 6.0 && spread < 20.0, "{spread}");
        // Reads after a write leave it alone.
        for _ in 0..100 {
            s.end(Duration::from_millis(500), true, 7, Some(Ended::Answered { took_ms: None }), Instant::now());
        }
        assert_eq!(s.hedge_ms().unwrap(), spread);
    }

    #[test]
    fn reads_after_a_write_are_averaged_apart_and_say_what_it_holds() {
        let s = Arc::new(ReplicaStats::default());
        let now = Instant::now();
        let plain = |s: &Arc<ReplicaStats>, ms| s.end(Duration::from_millis(ms), true, 0, Some(Ended::Answered { took_ms: None }), now);
        plain(&s, 1);
        plain(&s, 1);
        // It waited 50 ms for write 10, then answered.
        s.end(Duration::from_millis(50), true, 10, Some(Ended::Answered { took_ms: None }), now);
        assert_eq!(s.held(), 10);
        // Without --prefer-caught-up: every query's average, as before.
        assert!(s.route_view(now, false, 0, false, true).rtt_ms.unwrap() > 1.0);
        // With: plain reads by plain ones; one after write 10 too (held).
        assert_eq!(s.route_view(now, false, 0, true, true).rtt_ms, Some(1.0));
        assert_eq!(s.route_view(now, false, 10, true, true).rtt_ms, Some(1.0));
        // After write 11, not known to be held: by reads after a write.
        assert_eq!(s.route_view(now, false, 11, true, true).rtt_ms, Some(50.0));
        // A refusal for not having the write in time is what such a read
        // paid; it counts there, not as a failure.
        s.end(Duration::from_millis(1000), true, 12, Some(Ended::Unavailable), now);
        assert!(s.route_view(now, false, 13, true, true).rtt_ms.unwrap() > 50.0);
        assert_eq!(s.error_rate(), Some(0.0));
        assert_eq!(s.held(), 10);
    }

    #[test]
    fn reported_load_counts_only_when_routed_by() {
        let s = ReplicaStats::default();
        let t0 = Instant::now();
        s.reported(Some(Reported { slots: Some(0.5), memory: None, at: t0 }));
        assert_eq!(s.route_view(t0, false, 0, false, true).busy, Some(0.5));
        assert_eq!(s.route_view(t0, false, 0, false, false).busy, None);
    }

    #[test]
    fn a_replica_found_behind_stays_costed_so_for_a_minute() {
        let s = Arc::new(ReplicaStats::default());
        let t0 = Instant::now();
        s.end(Duration::from_millis(1), true, 0, Some(Ended::Answered { took_ms: None }), t0);
        s.end(Duration::from_millis(1), true, 0, Some(Ended::Answered { took_ms: None }), t0);
        s.end(Duration::from_millis(100), true, 5, Some(Ended::Answered { took_ms: None }), t0);
        let at = |d: Duration| s.route_view(t0 + d, false, 9, true, true).rtt_ms;
        assert_eq!(at(Duration::ZERO), Some(100.0));
        // Past the fast average's freshness: the slow one.
        assert_eq!(at(RTT_FRESH + Duration::from_secs(1)), Some(100.0));
        // Past that: unknown, asked again in its turn.
        assert_eq!(at(AFTER_FRESH + Duration::from_secs(1)), None);
    }
}
