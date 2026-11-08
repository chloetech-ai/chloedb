//! A model of a cluster under a mix of requests, fitted to a test run
//! ([`super::run`]): what one request of each kind costs a shard and the
//! coordinator, how that grows with the rows a shard holds, and the
//! queues those costs make at a rate. From it, for any shape (shards,
//! copies a shard, the cores of a machine, coordinators, rows): the rate
//! where the cluster saturates, the most it holds within the SLO, and the
//! latency of each kind at a given rate.
//!
//! The parts, each from the test where it can be:
//!
//! - **A shard's work.** A call to a shard costs `a0 + b n^e` ms of CPU,
//!   n the rows a shard holds: `a0` is what a call costs whatever it asks
//!   (a key lookup's cost), `b` and `e` come from the kind's cost at two
//!   sizes of the test table. A kind asks every shard (a search) or one
//!   (a key lookup), as the test saw it.
//! - **Copies.** A shard's reads spread over its copies; its writes are
//!   applied on every copy, and by one writer a shard, so copies add room
//!   for reads only.
//! - **Rebuilds after writes.** A row added to a shard makes the next
//!   search on each of its copies rebuild the part of the memtable it
//!   changed before it answers (the test measures a search right after an
//!   insert against one alone): that search waits all of it, and searches
//!   that come meanwhile wait for the rest. Every copy rebuilds for itself,
//!   so copies add none of it back; rows written a batch a commit cut it.
//!   The rebuilds' CPU is the copies' too.
//! - **The coordinator.** Each request costs it some CPU of its own and a
//!   little for each shard it asks, and some wall time besides its calls
//!   (end to end less the slowest call, at no load).
//! - **Queues.** A copy is an M/M/c queue with a server a core; a call's
//!   time is its own (a shifted exponential through the test's median and
//!   p99, scaled by its CPU at the new size) plus the wait; a search waits
//!   for its slowest shard (the most of independent calls).
//! - **Calibration.** One factor, `kappa`, scales the cores a copy
//!   effectively has, so that the model's capacity at the tested shape is
//!   the one the test's rising rate found (cores go to merges,
//!   replication and the load generator too).

use serde::{Deserialize, Serialize};

/// Quantiles of a time, in milliseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Quant {
    pub mean: f64,
    pub p50: f64,
    pub p90: f64,
    pub p99: f64,
}

/// What the test measured of one read kind sent alone, one request at a time.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct KindStats {
    pub kind: String,
    /// Its share of the mix's reads.
    pub share: f64,
    pub requests: u64,
    pub errors: u64,
    /// End to end, at the full size and at the small one.
    pub lat: Quant,
    pub lat_small: Quant,
    /// Shards a request asked, on average.
    pub calls: f64,
    /// A shard's answer, as the coordinator timed it (None: one server, no calls).
    pub call: Option<Quant>,
    pub call_small: Option<Quant>,
    /// CPU of every copy for one request, ms, at the full size and the small one.
    pub cpu_shard_ms: Option<f64>,
    pub cpu_shard_ms_small: Option<f64>,
    /// CPU of the coordinator for one request, ms.
    pub cpu_coord_ms: Option<f64>,
}

/// What the test measured of writes sent alone.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WriteStats {
    pub requests: u64,
    pub errors: u64,
    pub lat: Quant,
    /// CPU of every copy for one written row, ms.
    pub cpu_copies_ms: Option<f64>,
    pub cpu_coord_ms: Option<f64>,
    /// A search of `after_kind` sent right after a one-row insert, end to end.
    #[serde(default)]
    pub after_insert: Option<Quant>,
    #[serde(default)]
    pub after_kind: Option<String>,
    /// Share of the writes that add a row (an insert): the others (a value
    /// set in place, a delete) leave the memtable's segments as they were.
    #[serde(default)]
    pub insert_share: f64,
}

/// The cluster as tested.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Tested {
    /// One server answering by itself: no coordinator, no calls.
    pub local: bool,
    pub shards: usize,
    /// Copies a shard, on average.
    pub copies: f64,
    /// Cores a copy had for itself: its machine's, shared with the other
    /// servers on it (a laptop running every copy has the cores once).
    pub copy_cores: f64,
    pub coord_cores: f64,
    pub coordinators: usize,
    /// Test rows at the small size and the full one.
    pub docs_small: u64,
    pub docs: u64,
}

/// A read kind, fitted.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct KindFit {
    pub kind: String,
    pub share: f64,
    /// Asks every shard (a search); else one (a read by key).
    pub fanout: bool,
    /// A call's CPU is `a0 + b n^e` ms, n the rows a shard holds.
    pub b: f64,
    pub e: f64,
    /// A call's CPU (ms) and its wall time (median, p99) at `n_ref`.
    pub cpu_ref: f64,
    pub call_p50: f64,
    pub call_p99: f64,
    /// The coordinator's wall time besides its calls, ms, at the tested shards.
    pub over: f64,
    /// The coordinator's CPU besides its calls, ms.
    pub h: f64,
    /// Measured end to end at no load (median, p99), for the report.
    pub lat_p50: f64,
    pub lat_p99: f64,
}

/// The model of a tested cluster.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Fit {
    pub tested: Tested,
    /// A shard call's fixed CPU, ms.
    pub a0: f64,
    /// The coordinator's CPU a shard call, ms.
    pub r: f64,
    /// Test rows a shard at the full size.
    pub n_ref: f64,
    pub kinds: Vec<KindFit>,
    /// A written row's CPU on each copy of its shard, and on the coordinator, ms.
    pub write_copy_ms: f64,
    pub write_coord_ms: f64,
    /// Share of requests that are writes.
    pub write_share: f64,
    /// What a copy takes to rebuild the part of its memtable a row added
    /// changed, ms (0: not measured), and the share of writes that add one.
    #[serde(default)]
    pub stall_ms: f64,
    #[serde(default)]
    pub insert_share: f64,
    /// Rows a commit: the test wrote one a request.
    #[serde(default = "one")]
    pub rows_per_commit: f64,
    /// The cores' factor that makes the model's capacity at the tested
    /// shape the test's (1 until [`calibrate`]).
    pub kappa: f64,
    /// Whether the copies' CPU counters were read (else wall times stand in).
    pub measured_cpu: bool,
    pub assumptions: Vec<String>,
}

fn one() -> f64 {
    1.0
}

/// A shape to predict.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shape {
    pub shards: usize,
    pub copies: usize,
    /// Cores a copy has: its machine's, less what the node keeps.
    pub cores: f64,
    pub coord_cores: f64,
    /// 0: one server answering by itself (shards and copies 1).
    pub coordinators: usize,
    /// Rows of the table, over all shards.
    pub docs: f64,
}

/// The utilisation of each part at a rate.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Busy {
    pub copy: f64,
    pub coordinator: f64,
    pub writer: f64,
}

impl Busy {
    /// The busiest part: "copies", "coordinator" or "writer".
    pub fn first(&self) -> &'static str {
        if self.coordinator >= self.copy && self.coordinator >= self.writer {
            "coordinator"
        } else if self.writer > self.copy {
            "writer"
        } else {
            "copies"
        }
    }
    fn max(&self) -> f64 {
        self.copy.max(self.coordinator).max(self.writer)
    }
}

/// What the rebuilds after writes do to each copy at a rate.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Stall {
    /// The rebuild's time, ms, and rebuilds a second on each copy.
    pub ms: f64,
    pub rebuilds_s: f64,
    /// The chance that a search's call to a copy finds a rebuild to make
    /// (it waits all of it), and that it comes while one is under way (it
    /// waits the rest).
    pub first: f64,
    pub during: f64,
    /// The chance that a search waits for one on some shard.
    pub search_waits: f64,
}

/// A kind's latency at a rate.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KindAt {
    pub kind: String,
    pub p50_ms: f64,
    pub p99_ms: f64,
}

/// The cluster at a rate.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct At {
    pub qps: f64,
    pub busy: Busy,
    /// The reads' median and p99 (None: past saturation).
    pub p50_ms: Option<f64>,
    pub p99_ms: Option<f64>,
    pub kinds: Vec<KindAt>,
    #[serde(default)]
    pub stall: Stall,
}

/// What a shape does.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Prediction {
    pub shape: Shape,
    /// The rate where the first part is busy all the time.
    pub saturation_qps: f64,
    /// The most it holds within the SLO's p99 (0: not even one request at a time).
    pub capacity_qps: f64,
    /// What saturates first: "copies", "coordinator" or "writer".
    pub bottleneck: String,
    /// At no load.
    pub idle: At,
    /// At the rate asked, if any.
    pub at: Option<At>,
}

// ------------------------------------------------------------- queues

/// Erlang's C: the chance that a request waits at an M/M/c queue of `c`
/// servers offered `a` erlangs (a = c ρ).
pub fn erlang_c(c: usize, a: f64) -> f64 {
    let c = c.max(1);
    if a <= 0.0 {
        return 0.0;
    }
    let rho = a / c as f64;
    if rho >= 1.0 {
        return 1.0;
    }
    // Erlang's B by its recurrence, then C from B.
    let mut b = 1.0;
    for k in 1..=c {
        b = a * b / (k as f64 + a * b);
    }
    b / (1.0 - rho * (1.0 - b))
}

/// P(S + W ≤ t): S a shifted exponential (`shift` plus a mean `ms`), W
/// none with chance 1 - `pw` and else exponential of mean `mw`.
fn sum_cdf(t: f64, shift: f64, ms: f64, pw: f64, mw: f64) -> f64 {
    let x = t - shift;
    if x <= 0.0 {
        return 0.0;
    }
    let es = if ms > 1e-12 { 1.0 - (-x / ms).exp() } else { 1.0 };
    if pw <= 0.0 || mw <= 1e-12 {
        return es;
    }
    let both = if ms <= 1e-12 {
        1.0 - (-x / mw).exp()
    } else {
        let (l1, l2) = (1.0 / ms, 1.0 / mw);
        if (l1 - l2).abs() < 1e-9 * l1.max(l2) {
            1.0 - (-l1 * x).exp() * (1.0 + l1 * x)
        } else {
            1.0 - (l2 * (-l1 * x).exp() - l1 * (-l2 * x).exp()) / (l2 - l1)
        }
    };
    (1.0 - pw) * es + pw * both
}

/// The integral of [`sum_cdf`] from minus infinity to `t`: what averages
/// it over a window (a uniform wait).
fn sum_cdf_int(t: f64, shift: f64, ms: f64, pw: f64, mw: f64) -> f64 {
    let x = t - shift;
    if x <= 0.0 {
        return 0.0;
    }
    // The integral of 1 - e^(-x/m) from 0 to x.
    let one = |m: f64| if m > 1e-12 { x - m * (1.0 - (-x / m).exp()) } else { x };
    let es = one(ms);
    if pw <= 0.0 || mw <= 1e-12 {
        return es;
    }
    let both = if ms <= 1e-12 {
        one(mw)
    } else {
        let (l1, l2) = (1.0 / ms, 1.0 / mw);
        if (l1 - l2).abs() < 1e-9 * l1.max(l2) {
            x - (2.0 * (1.0 - (-l1 * x).exp()) - l1 * x * (-l1 * x).exp()) / l1
        } else {
            x - ((l2 / l1) * (1.0 - (-l1 * x).exp()) - (l1 / l2) * (1.0 - (-l2 * x).exp())) / (l2 - l1)
        }
    };
    (1.0 - pw) * es + pw * both
}

/// P(S + W + D ≤ t), S and W as in [`sum_cdf`], D the wait for a rebuild
/// of `ms`: all of it with chance `first`, a part of it (uniform) with
/// chance `during`, and none else.
#[allow(clippy::too_many_arguments)]
fn stalled_cdf(t: f64, shift: f64, ms: f64, pw: f64, mw: f64, first: f64, during: f64, stall: f64) -> f64 {
    let g = |t: f64| sum_cdf(t, shift, ms, pw, mw);
    if stall <= 0.0 || first + during <= 0.0 {
        return g(t);
    }
    let part = (sum_cdf_int(t, shift, ms, pw, mw) - sum_cdf_int(t - stall, shift, ms, pw, mw)) / stall;
    (1.0 - first - during) * g(t) + first * g(t - stall) + during * part
}

/// Whether a kind's call waits for a rebuild after a write: a search does,
/// a read by key (which reads the row where it is) does not.
fn stalls(k: &KindFit) -> bool {
    k.kind != "key lookup"
}

/// The t where the increasing `f` reaches `q`.
fn solve(f: impl Fn(f64) -> f64, q: f64) -> f64 {
    let mut hi = 1.0;
    while f(hi) < q && hi < 1e9 {
        hi *= 2.0;
    }
    let mut lo = 0.0;
    for _ in 0..60 {
        let mid = (lo + hi) / 2.0;
        if f(mid) < q {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    hi
}

/// A shifted exponential through a median and a p99: (shift, mean of the rest).
pub fn shifted_exp(p50: f64, p99: f64) -> (f64, f64) {
    let p50 = p50.max(0.0);
    let p99 = p99.max(p50);
    let mean = (p99 - p50) / 50f64.ln();
    let shift = p50 - mean * 2f64.ln();
    if shift >= 0.0 {
        (shift, mean)
    } else {
        // A long tail from zero: the median alone.
        (0.0, p50 / 2f64.ln())
    }
}

// ------------------------------------------------------------- fitting

/// The model of the tested cluster, from what the test measured. `kinds`
/// must hold the key lookup (it gives a call's fixed cost) even when the
/// mix has none (its share 0).
pub fn fit(tested: &Tested, kinds: &[KindStats], write: &WriteStats, write_share: f64) -> Fit {
    let mut assumptions = Vec::new();
    let s0 = tested.shards.max(1) as f64;
    let n_ref = tested.docs as f64 / s0;
    let n_small = (tested.docs_small as f64 / s0).max(1.0);
    let measured_cpu = kinds.iter().filter(|k| k.requests > 0).all(|k| k.cpu_shard_ms.is_some());
    if !measured_cpu {
        assumptions.push("the copies' CPU counters could not be read: a call's wall time stands in for its CPU".into());
    }
    let calls_of = |k: &KindStats| if tested.local { 1.0 } else { k.calls.max(1.0) };
    // A call's CPU at each size; its wall time where CPU is unknown.
    let per_call = |k: &KindStats, small: bool| -> f64 {
        let c = calls_of(k);
        let cpu = if small { k.cpu_shard_ms_small } else { k.cpu_shard_ms };
        let wall = if small { k.call_small.map(|q| q.mean).unwrap_or(k.lat_small.mean) } else { k.call.map(|q| q.mean).unwrap_or(k.lat.mean) };
        match cpu {
            Some(x) if measured_cpu => (x / c).max(1e-4),
            _ => wall.max(1e-4),
        }
    };
    let live: Vec<&KindStats> = kinds.iter().filter(|k| k.requests > 0).collect();
    // The cheapest search's call, at the smaller size: a call's fixed
    // cost stays under it, so that every search has work that grows.
    let min_call = live.iter().filter(|k| k.kind != "key lookup").map(|k| per_call(k, true).min(per_call(k, false))).fold(f64::INFINITY, f64::min);
    let a0 = match live.iter().find(|k| k.kind == "key lookup") {
        Some(k) => per_call(k, false).min(0.9 * min_call),
        None => {
            assumptions.push("no key lookup was measured: a call's fixed cost is taken as a tenth of the cheapest kind's".into());
            0.1 * min_call
        }
    }
    .max(0.0);
    let a0 = if a0.is_finite() { a0 } else { 0.0 };
    let grow_span = (n_ref / n_small).ln();
    // The coordinator's CPU a call: half of a key lookup's coordinator CPU.
    let r = if tested.local {
        0.0
    } else {
        match live.iter().find(|k| k.kind == "key lookup").and_then(|k| k.cpu_coord_ms) {
            Some(c) => {
                assumptions.push("half of a key lookup's coordinator CPU is its one call to a shard; the coordinator's work grows by that a shard asked".into());
                c / 2.0
            }
            None => 0.0,
        }
    };
    let mut fits = Vec::new();
    for k in &live {
        let calls = calls_of(k);
        let fanout = !tested.local && tested.shards > 1 && calls >= 0.75 * s0;
        let c2 = per_call(k, false);
        let c1 = per_call(k, true);
        let (b, e) = if k.kind == "key lookup" || grow_span <= 0.05 {
            ((c2 - a0).max(0.0), 0.0)
        } else {
            let x2 = (c2 - a0).max(0.02 * c2);
            let x1 = (c1 - a0).max(0.02 * c1);
            let e = ((x2 / x1).ln() / grow_span).clamp(0.0, 1.2);
            (x2 / n_ref.powf(e), e)
        };
        let (call_p50, call_p99) = match k.call {
            Some(q) if !tested.local => (q.p50, q.p99),
            _ => (k.lat.p50, k.lat.p99),
        };
        let h = if tested.local { 0.0 } else { k.cpu_coord_ms.map_or(0.0, |c| (c - r * calls).max(0.0)) };
        // The coordinator's own wall time: end to end less the slowest of its calls, at no load.
        let over = if tested.local {
            0.0
        } else {
            let (shift, mean) = shifted_exp(call_p50, call_p99);
            let m = calls.round().max(1.0) as i32;
            let t50 = solve(|t| sum_cdf(t, shift, mean, 0.0, 0.0).powi(m), 0.5);
            (k.lat.p50 - t50).max(0.0)
        };
        fits.push(KindFit { kind: k.kind.clone(), share: k.share, fanout, b, e, cpu_ref: a0 + b * n_ref.powf(e), call_p50, call_p99, over, h, lat_p50: k.lat.p50, lat_p99: k.lat.p99 });
    }
    let copies = tested.copies.max(1.0);
    let write_copy_ms = match (write.cpu_copies_ms, write.requests > 0) {
        (Some(c), true) if measured_cpu => c / copies,
        (_, true) => write.lat.mean,
        _ => 0.0,
    };
    let write_coord_ms = if tested.local { 0.0 } else { write.cpu_coord_ms.unwrap_or(0.0) };
    if write_share > 0.0 && write.requests == 0 {
        assumptions.push("writes were not measured: they are left out of the model".into());
    }
    // The rebuild a row added makes: a search right after an insert less
    // the same kind alone.
    let stall_ms = match (write.after_insert, write.after_kind.as_ref().and_then(|a| kinds.iter().find(|k| &k.kind == a))) {
        (Some(after), Some(k)) if write_share > 0.0 && write.insert_share > 0.0 => {
            let t = (after.p50 - k.lat.p50).max(0.0);
            if t >= 0.05 {
                assumptions.push(format!(
                    "a row added makes the next search on each copy of its shard rebuild the part of the memtable it changed: {} ms, a search of {} right after an insert less one alone; the rebuild is the same at every size and on every machine, and searches that come during it wait for the rest",
                    (t * 100.0).round() / 100.0,
                    k.kind
                ));
                t
            } else {
                0.0
            }
        }
        _ => 0.0,
    };
    assumptions.push("a request's CPU costs the same on another machine's core (the same CPU family)".into());
    assumptions.push("the test sent its requests from inside the coordinator: making them is taken off its CPU, but the HTTP or gRPC of real clients (reading a request, checking its key) is not counted, so a coordinator does a little more for them".into());
    assumptions.push("every copy of a shard serves reads, and reads spread evenly over the copies and the shards".into());
    if tested.local {
        assumptions.push("one server answered by itself: a cluster's coordinator is taken to add 0.05 ms of CPU a request, 0.03 ms a shard asked, and 0.2 ms of network".into());
    }
    Fit { tested: tested.clone(), a0, r, n_ref, kinds: fits, write_copy_ms, write_coord_ms, write_share, stall_ms, insert_share: write.insert_share, rows_per_commit: 1.0, kappa: 1.0, measured_cpu, assumptions }
}

// ------------------------------------------------------------- predicting

/// What the coordinator adds when the tested server had none.
const LOCAL_COORD_MS: f64 = 0.05;
const LOCAL_CALL_MS: f64 = 0.03;
const LOCAL_HOP_MS: f64 = 0.2;

impl Fit {
    /// The tested shape, as the model reads it.
    pub fn tested_shape(&self) -> Shape {
        let t = &self.tested;
        Shape {
            shards: t.shards.max(1),
            copies: t.copies.round().max(1.0) as usize,
            cores: t.copy_cores,
            coord_cores: t.coord_cores,
            coordinators: if t.local { 0 } else { t.coordinators.max(1) },
            docs: t.docs as f64,
        }
    }

    fn reads(&self) -> f64 {
        1.0 - self.write_share
    }

    /// The parts' utilisation at `qps`.
    pub fn busy(&self, s: &Shape, qps: f64) -> Busy {
        let (copy, coord, writer) = self.demand(s);
        let k = self.kappa;
        Busy {
            copy: qps * copy / 1000.0 / (k * s.cores.max(0.1)),
            coordinator: if s.coordinators == 0 { 0.0 } else { qps * coord / 1000.0 / (k * s.coord_cores.max(0.1) * s.coordinators as f64) },
            writer: qps * writer / 1000.0 / k,
        }
    }

    /// CPU ms per request (of the mix) on one copy, on one coordinator's
    /// share, and on one shard's writer.
    fn demand(&self, s: &Shape) -> (f64, f64, f64) {
        let shards = s.shards.max(1) as f64;
        let n = s.docs / shards;
        let mut copy = 0.0;
        let mut coord = 0.0;
        for k in &self.kinds {
            let calls = self.calls(k, s);
            let cpu = self.a0 + k.b * n.powf(k.e);
            copy += k.share * (calls / shards) * cpu / s.copies.max(1) as f64;
            coord += k.share * self.coord_cpu(k, calls, s);
        }
        let w = self.write_share;
        let (calls, commits) = self.stall_rates(s);
        let rebuild = if self.stall_ms > 0.0 { calls.min(commits) * self.stall_ms } else { 0.0 };
        let copy = self.reads() * copy + w * self.write_copy_ms / shards + rebuild;
        let coord = self.reads() * coord + w * self.write_coord_ms;
        (copy, coord, w * self.write_copy_ms / shards)
    }

    /// A request of the mix's search calls to one copy, and its commits
    /// that add rows to one shard (applied on each of its copies).
    fn stall_rates(&self, s: &Shape) -> (f64, f64) {
        let shards = s.shards.max(1) as f64;
        let calls = self.reads() * self.kinds.iter().filter(|k| stalls(k)).map(|k| k.share * self.calls(k, s)).sum::<f64>() / shards / s.copies.max(1) as f64;
        let commits = self.write_share * self.insert_share / shards / self.rows_per_commit.max(1.0);
        (calls, commits)
    }

    /// What the rebuilds after writes do to a copy of `s` at `qps`.
    pub fn stall(&self, s: &Shape, qps: f64) -> Stall {
        let t = self.stall_ms;
        let (calls, commits) = self.stall_rates(s);
        if t <= 0.0 || calls <= 0.0 || commits <= 0.0 || qps <= 0.0 || !qps.is_finite() {
            return Stall::default();
        }
        // The next search on a copy after a commit rebuilds: no more
        // rebuilds than searches, nor than one a rebuild's time.
        let rebuilds = (qps * calls.min(commits)).min(1000.0 / t);
        let during = (rebuilds * t / 1000.0).min(1.0);
        let first = (rebuilds / (qps * calls)).min(1.0 - during);
        let p = first + during;
        let total: f64 = self.kinds.iter().filter(|k| stalls(k)).map(|k| k.share).sum();
        let search_waits = if total > 0.0 { self.kinds.iter().filter(|k| stalls(k)).map(|k| k.share * (1.0 - (1.0 - p).powf(self.calls(k, s)))).sum::<f64>() / total } else { 0.0 };
        Stall { ms: t, rebuilds_s: rebuilds, first, during, search_waits }
    }

    fn calls(&self, k: &KindFit, s: &Shape) -> f64 {
        if k.fanout || (self.tested.local && s.shards > 1 && k.kind != "key lookup") {
            s.shards.max(1) as f64
        } else {
            1.0
        }
    }

    fn coord_cpu(&self, k: &KindFit, calls: f64, s: &Shape) -> f64 {
        if s.coordinators == 0 {
            0.0
        } else if self.tested.local {
            LOCAL_COORD_MS + LOCAL_CALL_MS * calls
        } else {
            k.h + self.r * calls
        }
    }

    /// The rate at which the first part is busy all the time.
    pub fn saturation(&self, s: &Shape) -> f64 {
        let b = self.busy(s, 1.0).max();
        if b <= 0.0 {
            f64::INFINITY
        } else {
            1.0 / b
        }
    }

    /// The cluster at `qps` (None past saturation).
    pub fn at(&self, s: &Shape, qps: f64) -> At {
        let busy = self.busy(s, qps);
        if busy.max() >= 1.0 || !qps.is_finite() {
            return At { qps, busy, p50_ms: None, p99_ms: None, kinds: Vec::new(), stall: Stall::default() };
        }
        let stall = self.stall(s, qps);
        let shards = s.shards.max(1) as f64;
        let n = s.docs / shards;
        // Each kind's call: its wall time scaled by its CPU at the new size.
        let calls: Vec<(f64, f64, f64)> = self
            .kinds
            .iter()
            .map(|k| {
                let ratio = (self.a0 + k.b * n.powf(k.e)) / k.cpu_ref.max(1e-9);
                let (shift, mean) = shifted_exp(k.call_p50 * ratio, k.call_p99 * ratio);
                (shift, mean, self.calls(k, s))
            })
            .collect();
        // The copy's queue: c cores, the mix's mean call.
        let weight: f64 = self.kinds.iter().zip(&calls).map(|(k, c)| k.share * c.2 / shards).sum();
        let mean_call = if weight > 0.0 { self.kinds.iter().zip(&calls).map(|(k, c)| k.share * c.2 / shards * (c.0 + c.1)).sum::<f64>() / weight } else { 0.0 };
        let cores = (self.kappa * s.cores).max(1.0);
        let c = cores.round().max(1.0) as usize;
        let pw = erlang_c(c, c as f64 * busy.copy);
        let mw = mean_call / (c as f64 * (1.0 - busy.copy)).max(1e-9);
        // The coordinator's wait, on average.
        let coord_wait = if s.coordinators == 0 {
            0.0
        } else {
            let cc = (self.kappa * s.coord_cores).round().max(1.0) as usize;
            let (_, d, _) = self.demand(s);
            erlang_c(cc, cc as f64 * busy.coordinator) * d / (cc as f64 * (1.0 - busy.coordinator)).max(1e-9)
        };
        let tested = self.tested_shape();
        let cdfs: Vec<Box<dyn Fn(f64) -> f64 + '_>> = self
            .kinds
            .iter()
            .zip(&calls)
            .map(|(k, &(shift, mean, m))| {
                let over = if s.coordinators == 0 {
                    0.0
                } else if self.tested.local {
                    LOCAL_HOP_MS
                } else {
                    // The coordinator's own time grows with its work.
                    let now = self.coord_cpu(k, m, s).max(1e-9);
                    let was = self.coord_cpu(k, self.calls(k, &tested), &tested).max(1e-9);
                    k.over * now / was
                } + coord_wait;
                let m = m.round().max(1.0) as i32;
                let (first, during) = if stalls(k) { (stall.first, stall.during) } else { (0.0, 0.0) };
                Box::new(move |t: f64| stalled_cdf(t - over, shift, mean, pw, mw, first, during, stall.ms).powi(m)) as Box<dyn Fn(f64) -> f64>
            })
            .collect();
        let kinds: Vec<KindAt> = self.kinds.iter().zip(&cdfs).map(|(k, f)| KindAt { kind: k.kind.clone(), p50_ms: solve(f, 0.5), p99_ms: solve(f, 0.99) }).collect();
        // The reads' quantiles: the mix of the kinds' times.
        let total: f64 = self.kinds.iter().map(|k| k.share).sum::<f64>().max(1e-12);
        let mix = |t: f64| self.kinds.iter().zip(&cdfs).map(|(k, f)| k.share * f(t)).sum::<f64>() / total;
        At { qps, busy, p50_ms: Some(solve(mix, 0.5)), p99_ms: Some(solve(mix, 0.99)), kinds, stall }
    }

    /// The most `s` holds with the reads' p99 at most `slo_ms`.
    pub fn capacity(&self, s: &Shape, slo_ms: f64) -> f64 {
        let sat = self.saturation(s);
        if !sat.is_finite() {
            return f64::INFINITY;
        }
        let ok = |q: f64| self.at(s, q).p99_ms.is_some_and(|p| p <= slo_ms);
        if !ok(sat * 1e-6) {
            return 0.0;
        }
        let (mut lo, mut hi) = (sat * 1e-6, sat);
        for _ in 0..40 {
            let mid = (lo + hi) / 2.0;
            if ok(mid) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        lo
    }

    /// What `s` does, and at `qps` if given.
    pub fn predict(&self, s: &Shape, slo_ms: f64, qps: Option<f64>) -> Prediction {
        let saturation_qps = self.saturation(s);
        Prediction {
            shape: *s,
            saturation_qps,
            capacity_qps: self.capacity(s, slo_ms),
            bottleneck: self.busy(s, 1.0).first().into(),
            idle: self.at(s, saturation_qps.min(1e9) * 1e-6),
            at: qps.map(|q| self.at(s, q)),
        }
    }
}

/// Sets `kappa` so that the model's capacity at the tested shape is
/// `measured` (the test's), or at least `measured` when the test found
/// no limit (`bound`); answers kappa.
pub fn calibrate(fit: &mut Fit, slo_ms: f64, measured: f64, bound: bool) -> f64 {
    let s = fit.tested_shape();
    fit.kappa = 1.0;
    if measured <= 0.0 || !measured.is_finite() {
        return 1.0;
    }
    let cap = |f: &mut Fit, k: f64| {
        f.kappa = k;
        f.capacity(&s, slo_ms)
    };
    let now = cap(fit, 1.0);
    if bound && now >= measured {
        fit.kappa = 1.0;
        return 1.0;
    }
    let (mut lo, mut hi) = (0.2, 3.0);
    if cap(fit, hi) < measured {
        fit.kappa = hi;
        return hi;
    }
    if cap(fit, lo) > measured {
        fit.kappa = lo;
        return lo;
    }
    for _ in 0..30 {
        let mid = (lo + hi) / 2.0;
        if cap(fit, mid) < measured {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    fit.kappa = (lo + hi) / 2.0;
    fit.kappa
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(p50: f64, p99: f64) -> Quant {
        Quant { mean: (p50 + p99) / 2.0, p50, p90: (p50 + p99) / 2.0, p99 }
    }

    /// A cluster of 2 shards × 2 copies, 4 cores each, where a search's
    /// call costs 0.2 ms at 25K rows a shard and 0.35 at 100K, and a key
    /// lookup 0.05.
    fn measured() -> (Tested, Vec<KindStats>, WriteStats) {
        let t = Tested { local: false, shards: 2, copies: 2.0, copy_cores: 4.0, coord_cores: 4.0, coordinators: 1, docs_small: 50_000, docs: 200_000 };
        let kinds = vec![
            KindStats { kind: "bm25".into(), share: 0.8, requests: 1000, lat: q(0.6, 1.5), lat_small: q(0.45, 1.1), calls: 2.0, call: Some(q(0.4, 1.0)), call_small: Some(q(0.25, 0.7)), cpu_shard_ms: Some(0.7), cpu_shard_ms_small: Some(0.4), cpu_coord_ms: Some(0.08), ..Default::default() },
            KindStats { kind: "key lookup".into(), share: 0.2, requests: 1000, lat: q(0.15, 0.4), lat_small: q(0.15, 0.4), calls: 1.0, call: Some(q(0.08, 0.2)), call_small: Some(q(0.08, 0.2)), cpu_shard_ms: Some(0.05), cpu_shard_ms_small: Some(0.05), cpu_coord_ms: Some(0.04), ..Default::default() },
        ];
        (t, kinds, WriteStats::default())
    }

    #[test]
    fn erlang_c_is_right() {
        // One server: the chance to wait is the utilisation.
        assert!((erlang_c(1, 0.7) - 0.7).abs() < 1e-12);
        // Two servers at 1 erlang: 1/3.
        assert!((erlang_c(2, 1.0) - 1.0 / 3.0).abs() < 1e-12);
        assert_eq!(erlang_c(4, 0.0), 0.0);
        assert_eq!(erlang_c(4, 4.5), 1.0);
    }

    #[test]
    fn a_shifted_exponential_passes_its_quantiles() {
        let (shift, mean) = shifted_exp(1.0, 3.0);
        let f = |t: f64| sum_cdf(t, shift, mean, 0.0, 0.0);
        assert!((solve(f, 0.5) - 1.0).abs() < 1e-6);
        assert!((solve(f, 0.99) - 3.0).abs() < 1e-6);
        // Waiting moves the quantiles out.
        let g = |t: f64| sum_cdf(t, shift, mean, 0.5, 1.0);
        assert!(solve(g, 0.99) > 3.5);
    }

    #[test]
    fn the_fit_finds_how_work_grows() {
        let (t, kinds, w) = measured();
        let f = fit(&t, &kinds, &w, 0.0);
        let bm = f.kinds.iter().find(|k| k.kind == "bm25").unwrap();
        assert!(bm.fanout);
        assert!((f.a0 - 0.05).abs() < 1e-9, "{}", f.a0);
        // (0.35 - 0.05) / (0.2 - 0.05) = 2 over 4 times the rows: e = 0.5.
        assert!((bm.e - 0.5).abs() < 1e-9, "{}", bm.e);
        assert!((bm.cpu_ref - 0.35).abs() < 1e-9);
        let key = f.kinds.iter().find(|k| k.kind == "key lookup").unwrap();
        assert!(!key.fanout);
        assert_eq!(key.e, 0.0);
        assert!((f.r - 0.02).abs() < 1e-9);
    }

    #[test]
    fn copies_add_room_and_shards_cut_latency() {
        let (t, kinds, w) = measured();
        let f = fit(&t, &kinds, &w, 0.0);
        let s = f.tested_shape();
        let base = f.predict(&s, 20.0, None);
        assert!(base.capacity_qps > 0.0 && base.capacity_qps < base.saturation_qps);
        // A copy more a shard: about half again the room for reads.
        let more = f.predict(&Shape { copies: 3, ..s }, 20.0, None);
        let gain = more.capacity_qps / base.capacity_qps;
        assert!((1.3..1.6).contains(&gain), "{gain}");
        // Ten times the rows: a shard's work grows, the room shrinks.
        let big = Shape { docs: s.docs * 10.0, ..s };
        let idle_big = f.at(&big, 1.0).p99_ms.unwrap();
        // More shards over those rows: each searches fewer, a search is faster.
        let wide = Shape { shards: 8, ..big };
        let idle_wide = f.at(&wide, 1.0).p99_ms.unwrap();
        assert!(idle_wide < idle_big, "{idle_wide} vs {idle_big}");
        // Past saturation: no latency.
        assert!(f.at(&s, base.saturation_qps * 1.01).p99_ms.is_none());
    }

    #[test]
    fn writes_are_not_helped_by_copies() {
        let (t, kinds, _) = measured();
        let w = WriteStats { requests: 500, lat: q(2.0, 5.0), cpu_copies_ms: Some(1.0), cpu_coord_ms: Some(0.1), ..Default::default() };
        let f = fit(&t, &kinds, &w, 0.5);
        let s = f.tested_shape();
        let a = f.busy(&s, 1000.0);
        let b = f.busy(&Shape { copies: 4, ..s }, 1000.0);
        assert_eq!(a.writer, b.writer);
        assert!(b.copy < a.copy);
        let c = f.busy(&Shape { shards: 4, ..s }, 1000.0);
        assert!(c.writer < a.writer);
    }

    #[test]
    fn the_integral_of_the_cdf_is_right() {
        for &(shift, ms, pw, mw) in &[(0.5, 1.0, 0.0, 0.0), (0.5, 1.0, 0.4, 2.0), (0.2, 1.0, 0.3, 1.0), (0.0, 0.0, 0.5, 3.0), (0.3, 0.0, 0.0, 0.0)] {
            for &t in &[0.1, 0.6, 1.0, 4.0, 12.0] {
                let n = 100_000;
                let dx = t / n as f64;
                let sum: f64 = (0..n).map(|i| sum_cdf((i as f64 + 0.5) * dx, shift, ms, pw, mw)).sum::<f64>() * dx;
                let got = sum_cdf_int(t, shift, ms, pw, mw);
                assert!((got - sum).abs() < 1e-5 * t.max(1.0), "{shift} {ms} {pw} {mw} at {t}: {got} vs {sum}");
            }
        }
        // A wait of all of a rebuild moves the CDF by it; one of a part of it, less.
        let at = |first: f64, during: f64| solve(|t| stalled_cdf(t, 0.5, 1.0, 0.0, 0.0, first, during, 10.0), 0.5);
        assert!((at(1.0, 0.0) - at(0.0, 0.0) - 10.0).abs() < 1e-6);
        let half = at(0.0, 1.0) - at(0.0, 0.0);
        assert!((4.0..6.5).contains(&half), "{half}");
    }

    /// 2% writes, half of them inserts, each making the next search on
    /// each copy of its shard wait `stall` ms.
    fn stalled(stall: f64) -> Fit {
        let (t, kinds, _) = measured();
        let bm = kinds[0].lat;
        let w = WriteStats {
            requests: 500,
            lat: q(1.0, 2.0),
            cpu_copies_ms: Some(0.4),
            cpu_coord_ms: Some(0.05),
            after_insert: Some(q(bm.p50 + stall, bm.p99 + stall)),
            after_kind: Some("bm25".into()),
            insert_share: 0.5,
            ..Default::default()
        };
        fit(&t, &kinds, &w, 0.02)
    }

    #[test]
    fn rebuilds_after_writes_slow_searches_copies_do_not_help_batches_do() {
        let f = stalled(8.0);
        assert!((f.stall_ms - 8.0).abs() < 1e-9, "{}", f.stall_ms);
        let mut none = f.clone();
        none.stall_ms = 0.0;
        let s = f.tested_shape();
        // At any rate, some searches are the first on their copy after an
        // insert: the p99 shows it.
        let (p, p0) = (f.at(&s, 100.0).p99_ms.unwrap(), none.at(&s, 100.0).p99_ms.unwrap());
        assert!(p > p0 + 4.0, "{p} vs {p0}");
        // At ten times the rate, ten times the searches come during one.
        let (a, b) = (f.stall(&s, 100.0), f.stall(&s, 1000.0));
        assert!((b.during / a.during - 10.0).abs() < 1e-6, "{} {}", a.during, b.during);
        assert!((a.first - b.first).abs() < 1e-9);
        assert!(b.search_waits > a.search_waits);
        // Copies do not cut it: each one rebuilds, and gets fewer searches to share it.
        assert!(f.stall(&Shape { copies: 4, ..s }, 1000.0).search_waits >= b.search_waits);
        // Batches do: a tenth of the commits.
        let mut batched = f.clone();
        batched.rows_per_commit = 10.0;
        assert!(batched.stall(&s, 1000.0).search_waits < 0.15 * b.search_waits);
        // And the room: the rebuilds take the copies' CPU, and the p99.
        let slo = 20.0;
        assert!(f.capacity(&s, slo) < 0.97 * none.capacity(&s, slo), "{} vs {}", f.capacity(&s, slo), none.capacity(&s, slo));
        assert!(batched.capacity(&s, slo) > f.capacity(&s, slo));
        // A key lookup reads the row where it is: no rebuild.
        let k = |f: &Fit| f.at(&s, 1000.0).kinds.iter().find(|k| k.kind == "key lookup").unwrap().p99_ms;
        assert!((k(&f) - k(&none)).abs() < 0.5 * k(&none), "{} vs {}", k(&f), k(&none));
        // Nothing measured, or no inserts: no stall.
        let (t, kinds, _) = measured();
        let w = WriteStats { requests: 500, lat: q(1.0, 2.0), cpu_copies_ms: Some(0.4), insert_share: 0.5, ..Default::default() };
        assert_eq!(fit(&t, &kinds, &w, 0.02).stall_ms, 0.0);
        let w = WriteStats { after_insert: Some(q(20.0, 30.0)), after_kind: Some("bm25".into()), insert_share: 0.0, ..w };
        assert_eq!(fit(&t, &kinds, &w, 0.02).stall_ms, 0.0);
    }

    #[test]
    fn calibration_matches_the_measured_capacity() {
        let (t, kinds, w) = measured();
        let mut f = fit(&t, &kinds, &w, 0.0);
        let s = f.tested_shape();
        let model = f.capacity(&s, 20.0);
        let k = calibrate(&mut f, 20.0, model * 0.6, false);
        assert!(k < 1.0);
        assert!((f.capacity(&s, 20.0) / (model * 0.6) - 1.0).abs() < 0.02);
        // A test that found no limit only lifts the model.
        let mut g = fit(&t, &kinds, &w, 0.0);
        assert_eq!(calibrate(&mut g, 20.0, model * 0.5, true), 1.0);
    }

    #[test]
    fn a_single_server_predicts_clusters() {
        let t = Tested { local: true, shards: 1, copies: 1.0, copy_cores: 8.0, coord_cores: 8.0, coordinators: 0, docs_small: 25_000, docs: 100_000 };
        let kinds = vec![
            KindStats { kind: "bm25".into(), share: 1.0, requests: 100, lat: q(0.5, 1.2), lat_small: q(0.3, 0.8), calls: 0.0, cpu_shard_ms: Some(0.5), cpu_shard_ms_small: Some(0.3), ..Default::default() },
            KindStats { kind: "key lookup".into(), share: 0.0, requests: 100, lat: q(0.05, 0.1), lat_small: q(0.05, 0.1), calls: 0.0, cpu_shard_ms: Some(0.05), cpu_shard_ms_small: Some(0.05), ..Default::default() },
        ];
        let f = fit(&t, &kinds, &WriteStats::default(), 0.0);
        let one = f.tested_shape();
        assert_eq!(one.coordinators, 0);
        let p = f.predict(&one, 10.0, Some(100.0));
        assert!(p.capacity_qps > 1000.0, "{}", p.capacity_qps);
        // As a cluster of two shards, one coordinator: the hop shows at no load.
        let two = Shape { shards: 2, copies: 1, coordinators: 1, ..one };
        let q2 = f.predict(&two, 10.0, None);
        assert!(q2.idle.p50_ms.unwrap() > p.idle.p50_ms.unwrap());
    }
}
