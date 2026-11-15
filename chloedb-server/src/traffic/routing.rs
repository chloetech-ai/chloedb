//! The Simulator's routing what-ifs: how a coordinator's choice of a
//! shard's copy (`--route`, `--hedge-ms`, `--hedge-adaptive`,
//! `--prefer-caught-up`) would answer this cluster's reads, two ways.
//!
//! * **A model** ([`simulate`], `POST /v1/admin/simulator/routing`): a
//!   discrete-event simulation of one coordinator and every copy of its
//!   shards, which answers in a second or two and touches no server. A copy
//!   is a processor its calls share (`--max-concurrent` of them at once,
//!   the rest queued), each call's work drawn by its kind; the coordinator
//!   follows `coord.rs`: the turn, least-cost's draw and margin
//!   ([`crate::cores::route`]), the hedge after `--hedge-ms` or after the
//!   copy's own tail, the hedges' budget, the 5 s cool-off of a copy that
//!   failed or was outrun, cancelling the call that lost, and reads after a
//!   write. It is the study of load balancing of 2026-11-15 cut down to
//!   ChloeDB's own choices, with the numbers it was calibrated with on
//!   DigitalOcean c-2 copies (2 dedicated vCPUs, one shard of 1M rows,
//!   answer cache off); a test run's model of the cluster replaces them
//!   ([`Setup::from_fit`]). There it matched one shard's measured
//!   throughput within about 5% and its p99 within about 15%, but with three
//!   shards it put throughput 14 to 48% too high and had adaptive hedging
//!   help where it hurt: an estimate, which a measurement settles.
//! * **A measurement** (a test run of shape `routing`, [`super::run`]):
//!   the same reads sent through a coordinator of each setting's own, made
//!   for the test on the same shards, the settings taking turns, and the
//!   model of the same settings beside it, calibrated by that run.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, VecDeque};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};

use super::model::Fit;
use crate::coord::{CoordOptions, Route};
use crate::ApiError;

// ===================================================================== what is asked

/// A coordinator's routing settings, as its flags set them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Variant {
    pub label: Option<String>,
    /// `least-cost` or `round-robin` (`--route`).
    pub route: String,
    /// `--hedge-ms`.
    pub hedge_ms: f64,
    pub hedge_adaptive: bool,
    pub prefer_caught_up: bool,
}

impl Default for Variant {
    fn default() -> Variant {
        Variant::of(&CoordOptions::default())
    }
}

impl Variant {
    /// The settings a coordinator runs with.
    pub fn of(o: &CoordOptions) -> Variant {
        Variant { label: None, route: o.route.name().into(), hedge_ms: o.hedge.as_secs_f64() * 1e3, hedge_adaptive: o.hedge_adaptive, prefer_caught_up: o.prefer_caught_up }
    }

    pub fn check(&self) -> Result<(), ApiError> {
        if Route::parse(&self.route).is_none() {
            return Err(ApiError::bad(format!("a setting's route is least-cost or round-robin, not {:?}", self.route)));
        }
        if !(1.0..=60_000.0).contains(&self.hedge_ms) {
            return Err(ApiError::bad("a setting's hedge_ms is 1 to 60,000"));
        }
        Ok(())
    }

    fn lc(&self) -> bool {
        self.route != "round-robin"
    }

    /// Whether two settings route alike (their labels aside).
    pub fn same(&self, o: &Variant) -> bool {
        self.route == o.route
            && (self.hedge_ms - o.hedge_ms).abs() < 0.5
            && self.hedge_adaptive == o.hedge_adaptive
            // prefer-caught-up changes nothing under round-robin.
            && (self.prefer_caught_up == o.prefer_caught_up || !self.lc())
    }

    /// Its name: its label, or what it sets.
    pub fn name(&self) -> String {
        if let Some(l) = self.label.as_deref().map(str::trim).filter(|l| !l.is_empty()) {
            return l.to_string();
        }
        let mut s = String::from(if self.lc() { "least cost" } else { "round-robin" });
        if self.hedge_adaptive {
            s.push_str(", adaptive hedge");
        }
        if (self.hedge_ms - 100.0).abs() >= 0.5 {
            s.push_str(&format!(", hedge {} ms", self.hedge_ms.round()));
        }
        if self.lc() && !self.prefer_caught_up {
            s.push_str(", not preferring caught-up copies");
        }
        s
    }

    /// The coordinator options of this setting, `base`'s for the rest.
    pub fn options(&self, base: &CoordOptions) -> CoordOptions {
        CoordOptions {
            route: Route::parse(&self.route).unwrap_or_default(),
            hedge: Duration::from_secs_f64(self.hedge_ms / 1e3),
            hedge_adaptive: self.hedge_adaptive,
            prefer_caught_up: self.prefer_caught_up,
            ..base.clone()
        }
    }
}

/// The settings to compare when none are given: this coordinator's first,
/// then each route with and without adaptive hedges, and least cost not
/// preferring caught-up copies, at this coordinator's `--hedge-ms`.
pub fn standard(now: &Variant) -> Vec<Variant> {
    let at = |route: &str, adaptive: bool, prefer: bool| Variant { label: None, route: route.into(), hedge_ms: now.hedge_ms, hedge_adaptive: adaptive, prefer_caught_up: prefer };
    let mut out = vec![Variant { label: Some(format!("{} (this coordinator)", now.name())), ..now.clone() }];
    for v in [at("least-cost", false, true), at("round-robin", false, true), at("least-cost", true, true), at("round-robin", true, true), at("least-cost", false, false)] {
        if !out.iter().any(|o| o.same(&v)) {
            out.push(v);
        }
    }
    out
}

/// The cluster the model runs: its shape, its copies, and the work its
/// reads take.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Setup {
    pub shards: usize,
    pub copies: usize,
    /// Cores a copy has, and the queries it runs at once (`--max-concurrent`).
    pub cores: u32,
    pub slots: u32,
    /// The kinds of read and the work each takes.
    pub kinds: Vec<Work>,
    /// A written row's work on its primary, and on each other copy as it
    /// applies it; a read by key (a read after a write), ms of one core.
    pub write_ms: f64,
    pub apply_ms: f64,
    pub lookup_ms: f64,
    /// One way between the coordinator and a copy, ms.
    pub net_ms: f64,
    /// The coordinator's own time a query, besides its calls, ms.
    pub coord_ms: f64,
    /// Where these numbers come from, in words.
    pub source: String,
}

/// A kind of read: its share of the reads, a call's work (ms of one core,
/// as a lognormal's mean and its coefficient of variation), and whether
/// it asks every shard.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Work {
    pub kind: String,
    pub share: f64,
    pub ms: f64,
    pub cv: f64,
    pub fanout: bool,
}

impl Default for Setup {
    fn default() -> Setup {
        Setup::reference(1, 3, 2, 64)
    }
}

impl Setup {
    /// ChloeDB's numbers on DigitalOcean c-2 copies (2 dedicated vCPUs, a
    /// shard of 1M rows of 12 Zipf words, BM25 over two words), for the
    /// shape given.
    pub fn reference(shards: usize, copies: usize, cores: u32, slots: u32) -> Setup {
        Setup {
            shards: shards.max(1),
            copies: copies.max(1),
            cores: cores.max(1),
            slots: slots.max(1),
            kinds: vec![Work { kind: "bm25".into(), share: 1.0, ms: 0.95, cv: 0.6, fanout: true }],
            write_ms: 0.3,
            apply_ms: 0.1,
            lookup_ms: 0.15,
            net_ms: 0.1,
            coord_ms: 1.5,
            source: "ChloeDB measured on DigitalOcean c-2 copies (no test run of this cluster yet)".into(),
        }
    }

    /// The cluster as a test run measured it: each kind's CPU a call and
    /// its spread, the copies' cores, the coordinator's own time.
    pub fn from_fit(fit: &Fit, slots: u32, run: &str) -> Setup {
        let t = &fit.tested;
        let cores = (t.copy_cores.round() as u32).max(1);
        // A call alone runs on every core of its copy (two at 1.31 times one).
        let g = if cores >= 2 { S2 * (cores as f64 / 2.0).sqrt() } else { 1.0 };
        let mut kinds = Vec::new();
        let (mut net, mut over, mut lookup) = (Vec::new(), 0.0, None);
        for k in &fit.kinds {
            let ms = k.cpu_ref.max(0.02);
            if !k.fanout && lookup.is_none() {
                lookup = Some(ms);
            }
            if k.share <= 0.0 {
                continue;
            }
            // The spread of its calls' wall times, as a lognormal's.
            let ratio = if k.call_p50 > 0.0 { (k.call_p99 / k.call_p50).clamp(1.05, 20.0) } else { 2.5 };
            let sigma = ratio.ln() / 2.326;
            let cv = ((sigma * sigma).exp() - 1.0).sqrt().clamp(0.1, 1.5);
            kinds.push(Work { kind: k.kind.clone(), share: k.share, ms, cv, fanout: k.fanout });
            if k.call_p50 > 0.0 {
                net.push(((k.call_p50 - ms / g) / 2.0).clamp(0.05, 5.0));
            }
            over += k.share * k.over.max(0.0);
        }
        if kinds.is_empty() {
            return Setup { source: format!("test run {run} measured no read: ChloeDB's numbers on DigitalOcean instead"), ..Setup::reference(t.shards, t.copies.round() as usize, cores, slots) };
        }
        let total: f64 = kinds.iter().map(|k| k.share).sum();
        for k in &mut kinds {
            k.share /= total;
        }
        net.sort_by(f64::total_cmp);
        let net_ms = net.get(net.len() / 2).copied().unwrap_or(0.1);
        let shards = t.shards.max(1);
        Setup {
            shards,
            copies: (t.copies.round() as usize).max(1),
            cores,
            slots: slots.max(1),
            kinds,
            write_ms: fit.write_copy_ms.max(0.05),
            apply_ms: (fit.write_copy_ms * 0.33).max(0.02),
            lookup_ms: lookup.unwrap_or(0.15),
            net_ms,
            coord_ms: (over / total - COORD_SHARD_MS * shards as f64).max(0.2),
            source: format!("test run {run}: each kind's CPU a call and its spread, the copies' cores, the coordinator's own time"),
        }
    }

    pub fn check(&self) -> Result<(), ApiError> {
        if !(1..=64).contains(&self.shards) || !(1..=16).contains(&self.copies) {
            return Err(ApiError::bad("the model takes 1 to 64 shards of 1 to 16 copies"));
        }
        if !(1..=256).contains(&self.cores) || !(1..=4096).contains(&self.slots) {
            return Err(ApiError::bad("the model takes 1 to 256 cores and 1 to 4,096 slots a copy"));
        }
        if self.kinds.is_empty() || self.kinds.iter().any(|k| !(k.ms > 0.0 && k.ms < 60_000.0) || !(0.0..=5.0).contains(&k.cv) || !(k.share >= 0.0)) {
            return Err(ApiError::bad("the model needs a kind of read, each with ms above 0 and cv 0 to 5"));
        }
        for x in [self.write_ms, self.apply_ms, self.lookup_ms, self.net_ms, self.coord_ms] {
            if !(0.0..60_000.0).contains(&x) {
                return Err(ApiError::bad("the model's times are 0 to 60,000 ms"));
            }
        }
        Ok(())
    }
}

/// What the model is asked: a fault on copy 0 of shard 0, the clients,
/// and the share of requests that write a row and read it back.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Scenario {
    /// `none`; `slow` (its answers `value` ms late, 30); `cpu` (`value`
    /// CPU-bound threads beside it on its node, 2); `stall` (its answers
    /// held `value` ms once a second, 60); `slots` (it runs `value`
    /// queries at once, 1); `behind` (it applies writes `value` ms late,
    /// 50, with a fifth of the requests reads after a write).
    pub fault: String,
    pub value: Option<f64>,
    /// Clients, each sending its next request when it has an answer.
    pub clients: usize,
    pub after_write: Option<f64>,
    /// Seconds measured, after two of warming up.
    pub seconds: f64,
    pub seed: u64,
}

impl Default for Scenario {
    fn default() -> Scenario {
        Scenario { fault: "none".into(), value: None, clients: 16, after_write: None, seconds: 6.0, seed: 1 }
    }
}

/// The faults the model knows, with their default value and its unit.
pub const FAULTS: &[(&str, f64, &str)] = &[
    ("none", 0.0, ""),
    ("slow", 30.0, "ms"),
    ("cpu", 2.0, "threads"),
    ("stall", 60.0, "ms"),
    ("slots", 1.0, "slots"),
    ("behind", 50.0, "ms"),
];

impl Scenario {
    pub fn check(&self) -> Result<(), ApiError> {
        if !FAULTS.iter().any(|(f, _, _)| *f == self.fault) {
            return Err(ApiError::bad(format!("fault is one of {}, not {:?}", FAULTS.iter().map(|f| f.0).collect::<Vec<_>>().join(", "), self.fault)));
        }
        if self.value.is_some_and(|v| !(0.0..=10_000.0).contains(&v)) {
            return Err(ApiError::bad("a fault's value is 0 to 10,000"));
        }
        if !(1..=2048).contains(&self.clients) {
            return Err(ApiError::bad("clients is 1 to 2,048"));
        }
        if self.after_write.is_some_and(|a| !(0.0..=0.9).contains(&a)) {
            return Err(ApiError::bad("after_write is a share, 0 to 0.9"));
        }
        if !(1.0..=60.0).contains(&self.seconds) {
            return Err(ApiError::bad("seconds is 1 to 60"));
        }
        Ok(())
    }

    fn value(&self) -> f64 {
        self.value.unwrap_or_else(|| FAULTS.iter().find(|f| f.0 == self.fault).map_or(0.0, |f| f.1))
    }

    fn after_write(&self) -> f64 {
        self.after_write.unwrap_or(if self.fault == "behind" { 0.2 } else { 0.0 })
    }
}

/// What the model measured of one setting.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Outcome {
    /// Reads answered a second, and their latency, ms.
    pub qps: f64,
    pub p50_ms: f64,
    pub p90_ms: f64,
    pub p99_ms: f64,
    pub p999_ms: f64,
    /// Reads after a write: their p99, ms (None: none sent).
    pub after_write_p99_ms: Option<f64>,
    pub hedges_per_s: f64,
    /// Calls a shard's part of a read took (1: no hedge), and the work of
    /// the calls that lost, against the work of those that answered.
    pub calls_per_part: f64,
    pub extra_work: f64,
    /// The share of copies in their cool-off, on average.
    pub cooling: f64,
    /// Shard 0's answers, by copy, as shares.
    pub by_copy: Vec<f64>,
}

// ===================================================================== the model's constants

/// The averages the coordinator keeps of a copy's round trips (`replica_stats.rs`): a fast one, a slow one.
const FAST_TAU: f64 = 2000.0;
const FAST_CAP: u32 = 20;
const SLOW_TAU: f64 = 30_000.0;
const SLOW_CAP: u32 = 200;
/// A round trip older than this counts as unknown (the median of the others).
const RTT_FRESH: f64 = 10_000.0;
const AFTER_FRESH: f64 = 60_000.0;
/// The cool-off of a copy outrun or failed, and the hedges' budget (`coord.rs`).
const COOL_OFF_MS: f64 = 5000.0;
const HEDGE_SHARE: f64 = 0.05;
const HEDGE_BURST: f64 = 10.0;
const HEDGE_FLOOR_MS: f64 = 2.0;
const SURVEY_MS: f64 = 5000.0;
/// Fitted on DigitalOcean: a query on two threads runs 1.31 times as fast
/// as on one; a CPU-bound neighbour weighs as much as `HOG_K` of the
/// server's threads at load and delays each call's wake-up; calls vary
/// by `CALL_CV`, a key's work by shard by `SHARD_CV`, copies' speeds by
/// `SPEED_CV`; answers hiccup (`HIC_*`), more often on a copy whose cores
/// are all taken.
const S2: f64 = 1.31;
const HOG_K: f64 = 4.6;
const HOG_WAKE: f64 = 0.53;
const CALL_CV: f64 = 0.12;
const SHARD_CV: f64 = 0.15;
const SPEED_CV: f64 = 0.03;
const JITTER: f64 = 0.03;
const HIC_P: f64 = 0.08;
const HIC_BASE: f64 = 0.005;
const HIC_MS: f64 = 3.0;
/// Calls a copy runs at once at most, whatever its slots (its executor's threads).
const PS_MAX: u32 = 24;
/// The coordinator's time a shard, besides its own (`coord_ms`).
const COORD_SHARD_MS: f64 = 0.2;
/// The stall fault: answers within this window of each second are held.
const STALL_WINDOW: f64 = 50.0;
const STALL_PERIOD: f64 = 1000.0;
/// Distinct queries the clients cycle through (a fixed list, as a bench's).
const KEYS: u64 = 500;
const WARM_MS: f64 = 2000.0;
const DRAIN_MS: f64 = 1500.0;
const NONE: u32 = u32::MAX;

// ===================================================================== draws

#[derive(Clone)]
struct Rng(u64);

fn mix_raw(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}
fn mix(x: u64) -> u64 {
    mix_raw(x.wrapping_add(0x9e37_79b9_7f4a_7c15))
}
fn h2(a: u64, b: u64) -> u64 {
    mix(mix(a) ^ b.wrapping_mul(0x0000_0100_0000_01b3))
}

impl Rng {
    fn new(s: u64) -> Rng {
        Rng(mix(s ^ 0x5DEE_CE66))
    }
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        mix_raw(self.0)
    }
    fn unit(&mut self) -> f64 {
        ((self.next() >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    }
    fn below(&mut self, n: usize) -> usize {
        if n <= 1 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
    fn exp(&mut self, mean: f64) -> f64 {
        if mean <= 0.0 {
            0.0
        } else {
            -mean * self.unit().ln()
        }
    }
    fn normal(&mut self) -> f64 {
        let (u1, u2) = (self.unit(), self.unit());
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
    }
    fn lognormal(&mut self, mean: f64, cv: f64) -> f64 {
        if cv <= 0.0 {
            return mean;
        }
        let s2 = (1.0 + cv * cv).ln();
        (mean.ln() - s2 / 2.0 + s2.sqrt() * self.normal()).exp()
    }
}

fn quantile(v: &mut [f64], q: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(f64::total_cmp);
    let pos = q * (v.len() - 1) as f64;
    let (lo, hi) = (pos.floor() as usize, pos.ceil() as usize);
    v[lo] + (v[hi] - v[lo]) * (pos - lo as f64)
}

// ===================================================================== the simulation's state

enum Ev {
    Issue(u32),
    Dispatch(u32),
    Arrive(u32),
    Done(u32, u64),
    Resp(u32),
    Hedge(u32, u32),
    Cancel(u32),
    Finish(u32),
    Apply(u32, u64),
    Survey,
    Tick,
}

struct Item {
    t: f64,
    seq: u64,
    ev: Ev,
}
impl PartialEq for Item {
    fn eq(&self, o: &Self) -> bool {
        self.t == o.t && self.seq == o.seq
    }
}
impl Eq for Item {}
impl PartialOrd for Item {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Item {
    fn cmp(&self, o: &Self) -> Ordering {
        o.t.total_cmp(&self.t).then(o.seq.cmp(&self.seq))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CK {
    Read,
    Write,
    Apply,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CS {
    Net,
    Wait,
    Queued,
    Running,
    Done,
    Cancelled,
}

struct Call {
    group: u32,
    rep: u32,
    kind: CK,
    t_sent: f64,
    work: f64,
    vfin: f64,
    state: CS,
    abandoned: bool,
    cancel_early: bool,
    min_seq: u64,
    seq: u64,
    work_done: f64,
    winner: bool,
    wake: f64,
}

struct Group {
    q: u32,
    shard: u32,
    calls: Vec<u32>,
    asked: Vec<u32>,
    asked_at: Vec<f64>,
    order: Vec<u32>,
    answered: bool,
    gen: u32,
    hedging: bool,
}

struct Query {
    client: u32,
    kind: usize,
    key: u64,
    t0: f64,
    groups: Vec<u32>,
    need: u32,
    answered: u32,
    done: bool,
    measured: bool,
    min_seq: u64,
    /// A read of a row just written; the write itself.
    after: bool,
    write: bool,
    pinned: Option<u32>,
    turn: u64,
}

/// A call running on a copy: it ends when the copy's virtual time reaches its own.
#[derive(PartialEq)]
struct Job(f64, u32);
impl Eq for Job {}
impl PartialOrd for Job {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Job {
    fn cmp(&self, o: &Self) -> Ordering {
        o.0.total_cmp(&self.0).then(o.1.cmp(&self.1))
    }
}

struct Rep {
    shard: u32,
    cores: u32,
    slots: u32,
    hogs: f64,
    speed: f64,
    late_ms: f64,
    stall_ms: f64,
    stall_phase: f64,
    lag_ms: f64,
    running: BinaryHeap<Job>,
    n: u32,
    v: f64,
    t_last: f64,
    ver: u64,
    queue: VecDeque<u32>,
    queued_live: u32,
    waiting: Vec<u32>,
    applied: u64,
}

impl Rep {
    /// The virtual time each running call advances by in a millisecond:
    /// a call alone gets every core (with diminishing returns), many share them.
    fn rate(&self) -> f64 {
        if self.n == 0 {
            return 0.0;
        }
        let n = self.n;
        let t = (self.cores / n).max(1);
        let g = if t >= 2 { S2 * (t as f64 / 2.0).powf(0.5) } else { 1.0 };
        let mut share = (self.cores as f64 / (n * t) as f64).min(1.0);
        if self.hogs > 0.0 {
            let k = ((n * t) as f64).clamp(2.0, HOG_K);
            share *= k / (k + self.hogs);
        }
        self.speed * g * share
    }
}

/// An average that forgets with time, and holds at least `1/cap` of each sample.
#[derive(Clone, Copy, Default)]
struct Ewma {
    v: f64,
    at: f64,
    n: u32,
    init: bool,
}
impl Ewma {
    fn sample(&mut self, x: f64, now: f64, tau: f64, cap: u32) {
        self.n = (self.n + 1).min(cap);
        if !self.init {
            self.v = x;
            self.init = true;
        } else {
            let dt = (now - self.at).max(0.0);
            let w = (1.0 - (-dt / tau).exp()).max(1.0 / self.n.clamp(1, cap) as f64);
            self.v += w * (x - self.v);
        }
        self.at = now;
    }
    fn at_least(&mut self, x: f64, now: f64, tau: f64, cap: u32) {
        if !self.init || self.v < x {
            self.sample(x, now, tau, cap);
        }
    }
    fn fresh(&self, now: f64, fresh: f64) -> Option<f64> {
        (self.init && now - self.at <= fresh).then_some(self.v)
    }
}

/// What the coordinator knows of a copy.
#[derive(Clone, Default)]
struct Seen {
    inflight: u32,
    q_fast: Ewma,
    plain_fast: Ewma,
    plain_slow: Ewma,
    plain_dev: Ewma,
    after_fast: Ewma,
    after_slow: Ewma,
    held: u64,
    avoid_until: f64,
    busy: Option<f64>,
}

#[derive(Default)]
struct Stats {
    lat: Vec<f64>,
    after: Vec<f64>,
    calls: u64,
    groups: u64,
    hedges: u64,
    by_copy: Vec<u64>,
    cooling: f64,
    ticks: u64,
}

struct Sim<'a> {
    setup: &'a Setup,
    v: &'a Variant,
    sc: &'a Scenario,
    cum: Vec<f64>,
    after_write: f64,
    measure_ms: f64,
    now: f64,
    seq: u64,
    heap: BinaryHeap<Item>,
    rng: Rng,
    copies: Vec<Rep>,
    shard_copies: Vec<Vec<u32>>,
    shard_seq: Vec<u64>,
    seen: Vec<Seen>,
    budget: f64,
    turn: u64,
    calls: Vec<Call>,
    groups: Vec<Group>,
    queries: Vec<Query>,
    counter: u64,
    st: Stats,
}

/// Runs setting `v` on `setup` under `sc`: what its reads would see.
pub fn simulate(setup: &Setup, v: &Variant, sc: &Scenario) -> Outcome {
    let mut sim = Sim::new(setup, v, sc);
    sim.run();
    sim.report()
}

impl<'a> Sim<'a> {
    fn new(setup: &'a Setup, v: &'a Variant, sc: &'a Scenario) -> Sim<'a> {
        let mut rng = Rng::new(sc.seed);
        let (mut copies, mut shard_copies) = (Vec::new(), Vec::new());
        for s in 0..setup.shards {
            let mut ids = Vec::new();
            for _ in 0..setup.copies {
                ids.push(copies.len() as u32);
                copies.push(Rep {
                    shard: s as u32,
                    cores: setup.cores,
                    slots: setup.slots,
                    hogs: 0.0,
                    speed: rng.lognormal(1.0, SPEED_CV),
                    late_ms: 0.0,
                    stall_ms: 0.0,
                    stall_phase: rng.unit() * STALL_PERIOD,
                    lag_ms: 1.0,
                    running: BinaryHeap::new(),
                    n: 0,
                    v: 0.0,
                    t_last: 0.0,
                    ver: 0,
                    queue: VecDeque::new(),
                    queued_live: 0,
                    waiting: Vec::new(),
                    applied: 0,
                });
            }
            shard_copies.push(ids);
        }
        let c0 = &mut copies[0];
        let value = sc.value();
        match sc.fault.as_str() {
            "slow" => c0.late_ms = value,
            "cpu" => c0.hogs = value,
            "stall" => c0.stall_ms = value,
            "slots" => c0.slots = (value.round() as u32).max(1),
            "behind" => c0.lag_ms = value.max(1.0),
            _ => {}
        }
        let total: f64 = setup.kinds.iter().map(|k| k.share.max(0.0)).sum::<f64>().max(1e-12);
        let cum = setup.kinds.iter().scan(0.0, |a, k| {
            *a += k.share.max(0.0) / total;
            Some(*a)
        }).collect();
        let n = copies.len();
        Sim {
            setup,
            v,
            sc,
            cum,
            after_write: sc.after_write(),
            measure_ms: sc.seconds * 1000.0,
            now: 0.0,
            seq: 0,
            heap: BinaryHeap::new(),
            turn: rng.next() % 1000,
            rng,
            copies,
            shard_copies,
            shard_seq: vec![0; setup.shards],
            seen: vec![Seen { avoid_until: -1.0, ..Seen::default() }; n],
            budget: HEDGE_BURST,
            calls: Vec::new(),
            groups: Vec::new(),
            queries: Vec::new(),
            counter: 0,
            st: Stats { by_copy: vec![0; setup.copies], ..Stats::default() },
        }
    }

    fn at(&mut self, t: f64, ev: Ev) {
        self.seq += 1;
        self.heap.push(Item { t, seq: self.seq, ev });
    }

    fn net(&mut self) -> f64 {
        self.setup.net_ms + self.rng.exp(JITTER)
    }

    fn measuring(&self) -> bool {
        self.now >= WARM_MS && self.now <= WARM_MS + self.measure_ms
    }

    fn run(&mut self) {
        for c in 0..self.sc.clients {
            let t = self.rng.unit() * 2.0;
            self.at(t, Ev::Issue(c as u32));
        }
        if self.v.lc() {
            self.at(SURVEY_MS, Ev::Survey);
        }
        self.at(WARM_MS, Ev::Tick);
        let end = WARM_MS + self.measure_ms + DRAIN_MS;
        while let Some(Item { t, ev, .. }) = self.heap.pop() {
            if t > end {
                break;
            }
            self.now = t;
            match ev {
                Ev::Issue(c) => self.issue(c),
                Ev::Dispatch(q) => self.dispatch(q),
                Ev::Arrive(c) => self.arrive(c),
                Ev::Done(r, ver) => self.done(r as usize, ver),
                Ev::Resp(c) => self.resp(c),
                Ev::Hedge(g, gen) => self.hedge_timer(g, gen),
                Ev::Cancel(c) => self.cancel_arrive(c),
                Ev::Finish(q) => self.finish(q),
                Ev::Apply(r, seq) => self.apply_arrive(r as usize, seq),
                Ev::Survey => {
                    // The load each copy reports: its slots taken.
                    for r in 0..self.copies.len() {
                        let c = &self.copies[r];
                        self.seen[r].busy = Some((c.n.min(c.slots) as f64 / c.slots as f64).clamp(0.0, 1.0));
                    }
                    self.at(t + SURVEY_MS, Ev::Survey);
                }
                Ev::Tick => {
                    if self.measuring() {
                        let now = self.now;
                        let cooling = self.seen.iter().filter(|s| s.avoid_until > now).count();
                        self.st.cooling += cooling as f64 / self.seen.len() as f64;
                        self.st.ticks += 1;
                        self.at(t + 100.0, Ev::Tick);
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------ clients and queries

    fn issue(&mut self, client: u32) {
        let shards = self.setup.shards;
        let (key, kind, pinned, write) = if self.after_write > 0.0 && self.rng.unit() < self.after_write {
            // A write, then a read of it.
            let k = self.rng.below(30_000) as u64;
            (k | (1 << 62), 0, Some((mix(k) % shards as u64) as u32), true)
        } else {
            let k = self.counter % KEYS;
            let u = (mix_raw(k ^ 0xA5A5_1234) >> 11) as f64 / (1u64 << 53) as f64;
            let kind = self.cum.iter().position(|&c| u < c).unwrap_or(self.cum.len() - 1);
            let pinned = (!self.setup.kinds[kind].fanout).then(|| (mix(k ^ 0x51) % shards as u64) as u32);
            (k, kind, pinned, false)
        };
        self.counter += 1;
        let turn = self.turn;
        self.turn += 1;
        let need = if pinned.is_some() { 1 } else { shards as u32 };
        let qi = self.queries.len() as u32;
        let measured = self.measuring();
        self.queries.push(Query { client, kind, key, t0: self.now, groups: Vec::new(), need, answered: 0, done: false, measured, min_seq: 0, after: false, write, pinned, turn });
        let t = self.now + self.setup.coord_ms * 0.6 + COORD_SHARD_MS / 2.0 * need as f64;
        self.at(t, Ev::Dispatch(qi));
    }

    fn dispatch(&mut self, qi: u32) {
        let q = &self.queries[qi as usize];
        if q.done {
            return;
        }
        let shards: Vec<u32> = match q.pinned {
            Some(s) => vec![s],
            None => (0..self.setup.shards as u32).collect(),
        };
        for s in shards {
            let gi = self.groups.len() as u32;
            self.groups.push(Group { q: qi, shard: s, calls: Vec::new(), asked: Vec::new(), asked_at: Vec::new(), order: Vec::new(), answered: false, gen: 0, hedging: true });
            self.queries[qi as usize].groups.push(gi);
            if self.queries[qi as usize].measured {
                self.st.groups += 1;
            }
            let order = if self.queries[qi as usize].write {
                // Writes go to the shard's primary, its last copy here.
                vec![(self.setup.copies - 1) as u32]
            } else {
                self.order(s as usize, qi)
            };
            self.groups[gi as usize].order = order;
            // The budget earns its share a request (HedgeBudget::earn).
            self.budget = (self.budget + HEDGE_SHARE).min(HEDGE_BURST);
            let first = self.groups[gi as usize].order[0];
            self.send_call(gi, first);
            self.arm_hedge(gi, first);
        }
    }

    /// Work of query `qi`'s call on shard `s`, ms of one core: the same key
    /// takes the same work on a shard whichever setting runs.
    fn work_of(&mut self, qi: u32, s: u32) -> f64 {
        let q = &self.queries[qi as usize];
        let w = if q.write {
            self.setup.write_ms
        } else if q.after {
            self.setup.lookup_ms
        } else {
            let k = &self.setup.kinds[q.kind];
            let base = Rng::new(h2(q.key, 77 + q.kind as u64)).lognormal(k.ms, k.cv);
            base * Rng::new(h2(q.key ^ (s as u64) << 32, 99)).lognormal(1.0, SHARD_CV)
        };
        w * self.rng.lognormal(1.0, CALL_CV)
    }

    // ------------------------------------------------------------ the coordinator's choices

    /// The order to ask shard `s`'s copies in (`Coordinator::order_for`).
    fn order(&self, s: usize, qi: u32) -> Vec<u32> {
        let n = self.shard_copies[s].len();
        let now = self.now;
        let (turn, min_seq) = (self.queries[qi as usize].turn, self.queries[qi as usize].min_seq);
        let rotation: Vec<usize> = (0..n).map(|k| (turn as usize + k) % n).collect();
        let reps = &self.shard_copies[s];
        if !self.v.lc() {
            let mut o = rotation;
            o.sort_by_key(|&l| self.seen[reps[l] as usize].avoid_until > now);
            return o.into_iter().map(|x| x as u32).collect();
        }
        // least-cost (cores::route::order): in turn, unless another drawn at
        // random costs clearly less; the rest by cost; those cooling last.
        let split = self.v.prefer_caught_up;
        let views: Vec<(bool, Option<f64>, u32, Option<f64>)> = reps
            .iter()
            .map(|&r| {
                let x = &self.seen[r as usize];
                let rtt = match (split, min_seq > 0 && x.held < min_seq) {
                    (false, _) => x.q_fast.fresh(now, RTT_FRESH),
                    (true, false) => x.plain_fast.fresh(now, RTT_FRESH),
                    (true, true) => x.after_fast.fresh(now, RTT_FRESH).or(x.after_slow.fresh(now, AFTER_FRESH)).or(x.plain_fast.fresh(now, RTT_FRESH)),
                };
                (x.avoid_until > now, rtt, x.inflight, x.busy)
            })
            .collect();
        let mut known: Vec<f64> = views.iter().filter_map(|v| v.1).collect();
        let neutral = if known.is_empty() { 1.0 } else { quantile(&mut known, 0.5) };
        let costs: Vec<f64> = views.iter().map(|v| v.1.unwrap_or(neutral).max(1.0) * (1.0 + v.2 as f64) * (1.0 + v.3.unwrap_or(0.0).clamp(0.0, 1.0))).collect();
        let open: Vec<usize> = rotation.iter().copied().filter(|&r| !views[r].0).collect();
        let cooling: Vec<usize> = rotation.iter().copied().filter(|&r| views[r].0).collect();
        let mut out: Vec<u32> = Vec::new();
        if !open.is_empty() {
            let mut lead = open[0];
            if open.len() > 1 {
                let seed = mix((turn << 16) ^ s as u64);
                let other = open[1 + (seed % (open.len() as u64 - 1)) as usize];
                if costs[other] < costs[lead] * 0.75 && costs[lead] - costs[other] >= 1.0 {
                    lead = other;
                }
            }
            out.push(lead as u32);
            let mut rest: Vec<usize> = open.iter().copied().filter(|&r| r != lead).collect();
            rest.sort_by(|&a, &b| costs[a].total_cmp(&costs[b]));
            out.extend(rest.into_iter().map(|x| x as u32));
        }
        out.extend(cooling.into_iter().map(|x| x as u32));
        out
    }

    /// When to hedge a call to copy `rep`: after `--hedge-ms`, or with
    /// adaptive hedges after its plain queries' slow average plus six
    /// deviations, once it answered 20.
    fn hedge_after(&self, rep: usize) -> f64 {
        let x = &self.seen[rep];
        if self.v.hedge_adaptive && x.plain_slow.init && x.plain_slow.n >= 20 {
            (x.plain_slow.v + 6.0 * if x.plain_dev.init { x.plain_dev.v } else { 0.0 }).clamp(HEDGE_FLOOR_MS, self.v.hedge_ms.max(HEDGE_FLOOR_MS))
        } else {
            self.v.hedge_ms
        }
    }

    fn arm_hedge(&mut self, gi: u32, local: u32) {
        let g = &self.groups[gi as usize];
        if g.asked.len() >= g.order.len() {
            return;
        }
        let rep = self.shard_copies[g.shard as usize][local as usize] as usize;
        let after = self.hedge_after(rep);
        self.groups[gi as usize].gen += 1;
        let gen = self.groups[gi as usize].gen;
        self.at(self.now + after, Ev::Hedge(gi, gen));
    }

    fn send_call(&mut self, gi: u32, local: u32) -> u32 {
        let (qi, shard) = (self.groups[gi as usize].q, self.groups[gi as usize].shard as usize);
        let rep = self.shard_copies[shard][local as usize];
        let ci = self.calls.len() as u32;
        let work = self.work_of(qi, shard as u32);
        let q = &self.queries[qi as usize];
        let (min_seq, measured, kind) = (q.min_seq, q.measured, if q.write { CK::Write } else { CK::Read });
        self.seen[rep as usize].inflight += 1;
        self.calls.push(Call { group: gi, rep, kind, t_sent: self.now, work, vfin: 0.0, state: CS::Net, abandoned: false, cancel_early: false, min_seq, seq: 0, work_done: 0.0, winner: false, wake: 0.0 });
        let g = &mut self.groups[gi as usize];
        g.calls.push(ci);
        g.asked.push(local);
        g.asked_at.push(self.now);
        if measured {
            self.st.calls += 1;
        }
        let t = self.now + self.net();
        self.at(t, Ev::Arrive(ci));
        ci
    }

    fn hedge_timer(&mut self, gi: u32, gen: u32) {
        let g = &self.groups[gi as usize];
        if g.answered || g.gen != gen || !g.hedging || self.queries[g.q as usize].done {
            return;
        }
        let Some(next) = g.order.iter().copied().find(|l| !g.asked.contains(l)) else { return };
        let measured = self.queries[g.q as usize].measured;
        if self.budget < 1.0 {
            self.groups[gi as usize].hedging = false;
            return;
        }
        self.budget -= 1.0;
        if measured {
            self.st.hedges += 1;
        }
        self.send_call(gi, next);
        self.arm_hedge(gi, next);
    }

    // ------------------------------------------------------------ a copy

    fn advance(&mut self, r: usize) {
        let now = self.now;
        let rate = self.copies[r].rate();
        let c = &mut self.copies[r];
        if c.n > 0 && rate > 0.0 {
            c.v += (now - c.t_last) * rate;
        }
        c.t_last = now;
    }

    fn reschedule(&mut self, r: usize) {
        loop {
            match self.copies[r].running.peek().map(|j| j.1) {
                Some(c) if self.calls[c as usize].state != CS::Running => {
                    self.copies[r].running.pop();
                }
                _ => break,
            }
        }
        let rate = self.copies[r].rate();
        let c = &mut self.copies[r];
        c.ver += 1;
        let ver = c.ver;
        if let Some(top) = c.running.peek() {
            if rate > 0.0 {
                let t = self.now + (top.0 - c.v).max(0.0) / rate;
                self.at(t, Ev::Done(r as u32, ver));
            }
        }
    }

    fn arrive(&mut self, ci: u32) {
        let r = self.calls[ci as usize].rep as usize;
        let c = &mut self.calls[ci as usize];
        if c.cancel_early || c.state == CS::Cancelled {
            c.state = CS::Cancelled;
            return;
        }
        if c.kind == CK::Read && c.min_seq > self.copies[r].applied {
            c.state = CS::Wait;
            self.copies[r].waiting.push(ci);
            return;
        }
        self.admit_or_queue(ci);
    }

    fn admit_or_queue(&mut self, ci: u32) {
        let r = self.calls[ci as usize].rep as usize;
        let c = &self.copies[r];
        if c.n < c.slots.min(PS_MAX) && c.queued_live == 0 {
            self.start_job(ci);
        } else {
            self.calls[ci as usize].state = CS::Queued;
            let c = &mut self.copies[r];
            c.queue.push_back(ci);
            c.queued_live += 1;
        }
    }

    fn start_job(&mut self, ci: u32) {
        let r = self.calls[ci as usize].rep as usize;
        self.advance(r);
        let wake = if self.copies[r].hogs > 0.0 { HOG_WAKE * self.copies[r].hogs } else { 0.0 };
        let c = &mut self.calls[ci as usize];
        c.state = CS::Running;
        c.wake = wake;
        let copy = &mut self.copies[r];
        c.vfin = copy.v + c.work;
        copy.running.push(Job(c.vfin, ci));
        copy.n += 1;
        self.reschedule(r);
    }

    fn done(&mut self, r: usize, ver: u64) {
        if self.copies[r].ver != ver {
            return;
        }
        self.advance(r);
        let mut finished = Vec::new();
        loop {
            let v = self.copies[r].v;
            match self.copies[r].running.peek() {
                Some(j) if j.0 <= v + 1e-9 => {
                    let ci = j.1;
                    self.copies[r].running.pop();
                    if self.calls[ci as usize].state == CS::Running {
                        self.copies[r].n -= 1;
                        finished.push(ci);
                    }
                }
                Some(j) if self.calls[j.1 as usize].state != CS::Running => {
                    self.copies[r].running.pop();
                }
                _ => break,
            }
        }
        for ci in finished {
            self.complete(ci);
        }
        self.admit_from_queue(r);
        self.reschedule(r);
    }

    fn admit_from_queue(&mut self, r: usize) {
        while self.copies[r].n < self.copies[r].slots.min(PS_MAX) {
            let Some(ci) = self.copies[r].queue.pop_front() else { break };
            if self.calls[ci as usize].state != CS::Queued {
                continue;
            }
            self.copies[r].queued_live -= 1;
            self.start_job(ci);
        }
    }

    fn complete(&mut self, ci: u32) {
        let r = self.calls[ci as usize].rep as usize;
        let c = &mut self.calls[ci as usize];
        c.state = CS::Done;
        c.work_done = c.work;
        let (kind, wake) = (c.kind, c.wake);
        let shard = self.copies[r].shard as usize;
        match kind {
            CK::Read => self.respond_at(ci, self.now + wake),
            CK::Write => {
                // The primary took it: numbered, and shipped to the others.
                self.shard_seq[shard] += 1;
                let seq = self.shard_seq[shard];
                self.calls[ci as usize].seq = seq;
                self.copies[r].applied = self.copies[r].applied.max(seq);
                for o in self.shard_copies[shard].clone() {
                    if o as usize != r {
                        let t = self.now + self.net() + self.copies[o as usize].lag_ms;
                        self.at(t, Ev::Apply(o, seq));
                    }
                }
                self.release_waiting(r);
                self.respond_at(ci, self.now + wake);
            }
            CK::Apply => {
                let seq = self.calls[ci as usize].seq;
                self.copies[r].applied = self.copies[r].applied.max(seq);
                self.release_waiting(r);
            }
        }
    }

    fn apply_arrive(&mut self, r: usize, seq: u64) {
        let ci = self.calls.len() as u32;
        let work = self.setup.apply_ms;
        self.calls.push(Call { group: NONE, rep: r as u32, kind: CK::Apply, t_sent: self.now, work, vfin: 0.0, state: CS::Running, abandoned: false, cancel_early: false, min_seq: 0, seq, work_done: 0.0, winner: false, wake: 0.0 });
        // The replication stream's, not behind the query slots.
        self.advance(r);
        let copy = &mut self.copies[r];
        let vfin = copy.v + work;
        self.calls[ci as usize].vfin = vfin;
        copy.running.push(Job(vfin, ci));
        copy.n += 1;
        self.reschedule(r);
    }

    fn release_waiting(&mut self, r: usize) {
        let applied = self.copies[r].applied;
        let ready: Vec<u32> = self.copies[r].waiting.iter().copied().filter(|&c| self.calls[c as usize].min_seq <= applied).collect();
        if ready.is_empty() {
            return;
        }
        self.copies[r].waiting.retain(|&c| self.calls[c as usize].min_seq > applied);
        for ci in ready {
            if self.calls[ci as usize].state == CS::Wait {
                self.admit_or_queue(ci);
            }
        }
    }

    fn respond_at(&mut self, ci: u32, leave: f64) {
        let r = self.calls[ci as usize].rep as usize;
        let c = &self.copies[r];
        let mut t = leave + c.late_ms;
        let p = HIC_BASE + if c.n >= c.cores { HIC_P } else { 0.0 };
        if self.rng.unit() < p {
            t += self.rng.exp(HIC_MS);
        }
        let c = &self.copies[r];
        if c.stall_ms > 0.0 && (leave - c.stall_phase).rem_euclid(STALL_PERIOD) < STALL_WINDOW {
            // Answers within the window are held, once a second.
            t += c.stall_ms;
        }
        let t = t + self.net();
        self.at(t, Ev::Resp(ci));
    }

    fn cancel_arrive(&mut self, ci: u32) {
        let r = self.calls[ci as usize].rep as usize;
        match self.calls[ci as usize].state {
            CS::Net => self.calls[ci as usize].cancel_early = true,
            CS::Queued => {
                self.calls[ci as usize].state = CS::Cancelled;
                self.copies[r].queued_live -= 1;
            }
            CS::Wait => {
                self.calls[ci as usize].state = CS::Cancelled;
                self.copies[r].waiting.retain(|&x| x != ci);
            }
            CS::Running => {
                self.advance(r);
                let c = &self.calls[ci as usize];
                let done = (c.work - (c.vfin - self.copies[r].v)).max(0.0);
                self.calls[ci as usize].state = CS::Cancelled;
                self.calls[ci as usize].work_done = done;
                self.copies[r].n -= 1;
                self.admit_from_queue(r);
                self.reschedule(r);
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------ the coordinator's side

    fn resp(&mut self, ci: u32) {
        let gi = self.calls[ci as usize].group;
        if gi == NONE || self.calls[ci as usize].abandoned {
            return;
        }
        let r = self.calls[ci as usize].rep as usize;
        let now = self.now;
        let rtt = now - self.calls[ci as usize].t_sent;
        self.seen[r].inflight = self.seen[r].inflight.saturating_sub(1);
        self.observe(ci, rtt);
        if self.groups[gi as usize].answered {
            return;
        }
        let qi = self.groups[gi as usize].q;
        self.groups[gi as usize].answered = true;
        self.calls[ci as usize].winner = true;
        let shard = self.groups[gi as usize].shard as usize;
        if shard == 0 && self.queries[qi as usize].measured {
            if let Some(k) = self.shard_copies[0].iter().position(|&x| x as usize == r) {
                self.st.by_copy[k] += 1;
            }
        }
        // The others were outrun: abandoned, cancelled, and cooling off
        // unless an adaptive hedge outran them early, or they are behind
        // the write a read after it must see (coord.rs).
        let others: Vec<(u32, f64)> = {
            let g = &self.groups[gi as usize];
            g.calls.iter().zip(&g.asked_at).filter(|(&c, _)| c != ci).map(|(&c, &t)| (c, t)).collect()
        };
        let min_seq = self.queries[qi as usize].min_seq;
        for (oc, asked_at) in others {
            if self.calls[oc as usize].abandoned {
                continue;
            }
            self.abandon(oc, true);
            let orep = self.calls[oc as usize].rep as usize;
            let early = self.v.hedge_adaptive && now - asked_at < self.v.hedge_ms;
            let behind = self.v.prefer_caught_up && self.v.lc() && min_seq > 0 && self.seen[orep].held < min_seq;
            if !early && !behind {
                self.seen[orep].avoid_until = now + COOL_OFF_MS;
            }
        }
        let q = &mut self.queries[qi as usize];
        if !q.done {
            q.answered += 1;
            if q.answered >= q.need {
                self.complete_query(qi);
            }
        }
    }

    /// The coordinator stops waiting for call `ci`: the copy is sent a cancel.
    fn abandon(&mut self, ci: u32, outrun: bool) {
        if self.calls[ci as usize].abandoned {
            return;
        }
        let r = self.calls[ci as usize].rep as usize;
        self.calls[ci as usize].abandoned = true;
        self.seen[r].inflight = self.seen[r].inflight.saturating_sub(1);
        if outrun {
            // A copy outrun is at least as slow as the time it was given.
            let (now, rtt, min_seq) = (self.now, self.now - self.calls[ci as usize].t_sent, self.calls[ci as usize].min_seq);
            let x = &mut self.seen[r];
            x.q_fast.at_least(rtt, now, FAST_TAU, FAST_CAP);
            if min_seq > 0 {
                x.after_fast.at_least(rtt, now, FAST_TAU, FAST_CAP);
                x.after_slow.at_least(rtt, now, SLOW_TAU, SLOW_CAP);
            } else {
                x.plain_fast.at_least(rtt, now, FAST_TAU, FAST_CAP);
            }
        }
        let st = self.calls[ci as usize].state;
        if st != CS::Done && st != CS::Cancelled {
            let t = self.now + self.net();
            self.at(t, Ev::Cancel(ci));
        }
    }

    fn observe(&mut self, ci: u32, rtt: f64) {
        let now = self.now;
        let (r, min_seq) = (self.calls[ci as usize].rep as usize, self.calls[ci as usize].min_seq);
        let x = &mut self.seen[r];
        x.q_fast.sample(rtt, now, FAST_TAU, FAST_CAP);
        if min_seq > 0 {
            x.after_fast.sample(rtt, now, FAST_TAU, FAST_CAP);
            x.after_slow.sample(rtt, now, SLOW_TAU, SLOW_CAP);
            x.held = x.held.max(min_seq);
        } else if self.calls[ci as usize].kind == CK::Read {
            let dev = x.plain_slow.init.then(|| (rtt - x.plain_slow.v).abs());
            x.plain_fast.sample(rtt, now, FAST_TAU, FAST_CAP);
            x.plain_slow.sample(rtt, now, SLOW_TAU, SLOW_CAP);
            if let Some(d) = dev {
                x.plain_dev.sample(d, now, SLOW_TAU, SLOW_CAP);
            }
        }
    }

    fn complete_query(&mut self, qi: u32) {
        self.queries[qi as usize].done = true;
        for g in self.queries[qi as usize].groups.clone() {
            if !self.groups[g as usize].answered {
                for c in self.groups[g as usize].calls.clone() {
                    self.abandon(c, true);
                }
                self.groups[g as usize].answered = true;
            }
        }
        let need = self.queries[qi as usize].need as f64;
        let t = self.now + self.setup.coord_ms * 0.4 + COORD_SHARD_MS / 2.0 * need;
        self.at(t, Ev::Finish(qi));
    }

    fn finish(&mut self, qi: u32) {
        let now = self.now;
        let q = &self.queries[qi as usize];
        let (client, measured, lat) = (q.client, q.measured, now - q.t0);
        if q.write {
            // The write answered: read it back, as an application would.
            let gi = q.groups[0];
            let g = &self.groups[gi as usize];
            let won = g.calls.iter().copied().find(|&c| self.calls[c as usize].winner).unwrap_or(g.calls[0]);
            let seq = self.calls[won as usize].seq;
            let nq = Query { client, kind: 0, key: q.key, t0: now, groups: Vec::new(), need: 1, answered: 0, done: false, measured, min_seq: seq, after: true, write: false, pinned: q.pinned, turn: q.turn };
            let ni = self.queries.len() as u32;
            self.queries.push(nq);
            let t = now + self.setup.coord_ms * 0.6 + COORD_SHARD_MS / 2.0;
            self.at(t, Ev::Dispatch(ni));
            return;
        }
        if measured {
            if q.after {
                self.st.after.push(lat);
            } else {
                self.st.lat.push(lat);
            }
        }
        self.at(now, Ev::Issue(client));
    }

    fn report(&mut self) -> Outcome {
        // Reads still unanswered at the end count with the time they waited.
        let now = self.now;
        for q in &self.queries {
            if q.measured && !q.done && !q.write {
                if q.after {
                    self.st.after.push(now - q.t0);
                } else {
                    self.st.lat.push(now - q.t0);
                }
            }
        }
        let (mut win, mut lose) = (0.0, 0.0);
        for c in &self.calls {
            if c.group == NONE || !self.queries[self.groups[c.group as usize].q as usize].measured {
                continue;
            }
            let w = match c.state {
                CS::Done | CS::Cancelled => c.work_done,
                CS::Running => c.work - (c.vfin - self.copies[c.rep as usize].v).max(0.0),
                _ => 0.0,
            };
            if c.winner {
                win += w;
            } else {
                lose += w;
            }
        }
        let st = &mut self.st;
        let secs = self.measure_ms / 1000.0;
        let n = st.lat.len();
        let total: u64 = st.by_copy.iter().sum();
        let mut lat = std::mem::take(&mut st.lat);
        let mut after = std::mem::take(&mut st.after);
        let r = |x: f64| if x.is_finite() { (x * 1000.0).round() / 1000.0 } else { 0.0 };
        Outcome {
            qps: (n as f64 / secs * 10.0).round() / 10.0,
            p50_ms: r(quantile(&mut lat, 0.5)),
            p90_ms: r(quantile(&mut lat, 0.9)),
            p99_ms: r(quantile(&mut lat, 0.99)),
            p999_ms: r(quantile(&mut lat, 0.999)),
            after_write_p99_ms: (!after.is_empty()).then(|| r(quantile(&mut after, 0.99))),
            hedges_per_s: r(st.hedges as f64 / secs),
            calls_per_part: r(if st.groups > 0 { st.calls as f64 / st.groups as f64 } else { 1.0 }),
            extra_work: r(if win > 0.0 { lose / win } else { 0.0 }),
            cooling: r(if st.ticks > 0 { st.cooling / st.ticks as f64 } else { 0.0 }),
            by_copy: st.by_copy.iter().map(|&k| r(k as f64 / total.max(1) as f64)).collect(),
        }
    }
}

// ===================================================================== many runs

/// The median of each number over `outs` (seeds of one setting).
fn median_of(outs: &[Outcome]) -> Outcome {
    let m = |f: &dyn Fn(&Outcome) -> f64| {
        let mut v: Vec<f64> = outs.iter().map(f).collect();
        quantile(&mut v, 0.5)
    };
    let after: Vec<f64> = outs.iter().filter_map(|o| o.after_write_p99_ms).collect();
    let k = outs.first().map_or(0, |o| o.by_copy.len());
    Outcome {
        qps: m(&|o| o.qps),
        p50_ms: m(&|o| o.p50_ms),
        p90_ms: m(&|o| o.p90_ms),
        p99_ms: m(&|o| o.p99_ms),
        p999_ms: m(&|o| o.p999_ms),
        after_write_p99_ms: (!after.is_empty()).then(|| {
            let mut a = after;
            quantile(&mut a, 0.5)
        }),
        hedges_per_s: m(&|o| o.hedges_per_s),
        calls_per_part: m(&|o| o.calls_per_part),
        extra_work: m(&|o| o.extra_work),
        cooling: m(&|o| o.cooling),
        by_copy: (0..k).map(|i| m(&|o| o.by_copy.get(i).copied().unwrap_or(0.0))).collect(),
    }
}

/// Each setting at each count of clients, the median of `seeds` runs,
/// spread over the cores given: rows of `{variant, clients, ...}`.
pub fn sweep(setup: &Setup, variants: &[Variant], clients: &[usize], base: &Scenario, seeds: u64, threads: usize) -> Vec<Json> {
    let mut jobs = Vec::new();
    for (vi, _) in variants.iter().enumerate() {
        for &c in clients {
            for s in 0..seeds.max(1) {
                jobs.push((vi, c, base.seed.wrapping_add(s * 7919)));
            }
        }
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let outs: Vec<std::sync::Mutex<Option<Outcome>>> = jobs.iter().map(|_| std::sync::Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..threads.clamp(1, jobs.len().max(1)) {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(&(vi, c, seed)) = jobs.get(i) else { break };
                let sc = Scenario { clients: c, seed, ..base.clone() };
                let o = simulate(setup, &variants[vi], &sc);
                *outs[i].lock().unwrap_or_else(|e| e.into_inner()) = Some(o);
            });
        }
    });
    let outs: Vec<Option<Outcome>> = outs.into_iter().map(|m| m.into_inner().unwrap_or_else(|e| e.into_inner())).collect();
    let mut rows = Vec::new();
    for (vi, v) in variants.iter().enumerate() {
        for &c in clients {
            let mine: Vec<Outcome> = jobs.iter().zip(&outs).filter(|((j, k, _), _)| *j == vi && *k == c).filter_map(|(_, o)| o.clone()).collect();
            let o = median_of(&mine);
            let mut row = serde_json::to_value(&o).unwrap_or(Json::Null);
            row["variant"] = json!(vi);
            row["name"] = json!(v.name());
            row["clients"] = json!(c);
            rows.push(row);
        }
    }
    rows
}

/// What a page asks of the model (`POST /v1/admin/simulator/routing`).
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Preview {
    /// The settings to compare (none: [`standard`]).
    pub variants: Vec<Variant>,
    pub clients: Vec<usize>,
    pub fault: String,
    pub value: Option<f64>,
    pub after_write: Option<f64>,
    pub seconds: f64,
    pub seeds: u64,
    /// The test run whose model calibrates it (None: the newest with one).
    pub run: Option<String>,
    /// The cluster to run, in place of the calibration.
    pub setup: Option<Setup>,
}

impl Default for Preview {
    fn default() -> Preview {
        Preview { variants: Vec::new(), clients: vec![1, 16, 64], fault: "none".into(), value: None, after_write: None, seconds: 6.0, seeds: 2, run: None, setup: None }
    }
}

impl Preview {
    pub fn check(&self) -> Result<(), ApiError> {
        if self.variants.len() > 8 {
            return Err(ApiError::bad("at most 8 settings at once"));
        }
        for v in &self.variants {
            v.check()?;
        }
        if self.clients.is_empty() || self.clients.len() > 6 {
            return Err(ApiError::bad("clients is a list of 1 to 6 counts"));
        }
        if !(1..=5).contains(&self.seeds) {
            return Err(ApiError::bad("seeds is 1 to 5"));
        }
        for &c in &self.clients {
            Scenario { clients: c, ..self.scenario() }.check()?;
        }
        if let Some(s) = &self.setup {
            s.check()?;
        }
        Ok(())
    }

    pub fn scenario(&self) -> Scenario {
        Scenario { fault: self.fault.clone(), value: self.value, clients: self.clients.first().copied().unwrap_or(16), after_write: self.after_write, seconds: self.seconds, seed: 1 }
    }
}

/// The model's answer: the calibration, the settings, a row a setting and
/// count of clients, and which setting is best at each count.
pub fn answer(setup: &Setup, variants: &[Variant], p: &Preview, threads: usize) -> Json {
    let t0 = std::time::Instant::now();
    let rows = sweep(setup, variants, &p.clients, &p.scenario(), p.seeds, threads);
    let best: Vec<Json> = p
        .clients
        .iter()
        .map(|&c| {
            let mine: Vec<&Json> = rows.iter().filter(|r| r["clients"] == c).collect();
            let by = |k: &str| mine.iter().min_by(|a, b| a[k].as_f64().unwrap_or(f64::MAX).total_cmp(&b[k].as_f64().unwrap_or(f64::MAX))).map(|r| r["variant"].clone());
            // The first of equals: this coordinator's setting, when it ties.
            let qps = mine.iter().min_by(|a, b| b["qps"].as_f64().unwrap_or(0.0).total_cmp(&a["qps"].as_f64().unwrap_or(0.0))).map(|r| r["variant"].clone());
            json!({"clients": c, "p99": by("p99_ms"), "p999": by("p999_ms"), "qps": qps})
        })
        .collect();
    let sc = p.scenario();
    json!({
        "setup": setup,
        "scenario": {"fault": sc.fault, "value": sc.value(), "after_write": sc.after_write(), "seconds": sc.seconds, "seeds": p.seeds},
        "variants": variants.iter().map(|v| { let mut j = serde_json::to_value(v).unwrap_or(Json::Null); j["name"] = json!(v.name()); j }).collect::<Vec<_>>(),
        "clients": p.clients,
        "rows": rows,
        "best": best,
        "faults": FAULTS.iter().map(|(f, v, u)| json!({"fault": f, "value": v, "unit": u})).collect::<Vec<_>>(),
        "ms": (t0.elapsed().as_secs_f64() * 1e3).round(),
        "trust": "A model: on DigitalOcean it matched one shard within about 5% (throughput) and 15% (p99), but put three shards' throughput 14 to 48% too high and had adaptive hedging help where it hurt. Measure before you change a production cluster.",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lc() -> Variant {
        Variant { route: "least-cost".into(), ..Variant::default() }
    }
    fn rr() -> Variant {
        Variant { route: "round-robin".into(), ..Variant::default() }
    }

    /// The model reproduces what it was calibrated on: one shard of three
    /// c-2 copies alike, BM25, round-robin at 16 clients about 4,800
    /// queries a second (4,843 measured) at a p99 near 10 ms (9.99).
    #[test]
    fn it_holds_its_calibration() {
        let setup = Setup::reference(1, 3, 2, 64);
        let o = simulate(&setup, &rr(), &Scenario { clients: 16, seconds: 5.0, ..Scenario::default() });
        assert!((4000.0..5800.0).contains(&o.qps), "{o:?}");
        assert!((5.0..16.0).contains(&o.p99_ms), "{o:?}");
        assert!(o.by_copy.iter().all(|&s| (0.25..0.42).contains(&s)), "copies alike share the reads: {o:?}");
    }

    /// A copy given a third of its node's CPU: least cost asks it less and
    /// holds more than round-robin (measured 5,575 against 3,523 at 64
    /// clients).
    #[test]
    fn least_cost_asks_a_busy_copy_less() {
        let setup = Setup::reference(1, 3, 2, 64);
        let sc = Scenario { fault: "cpu".into(), clients: 64, seconds: 5.0, ..Scenario::default() };
        let (a, b) = (simulate(&setup, &lc(), &sc), simulate(&setup, &rr(), &sc));
        assert!(a.qps > b.qps * 1.1, "least cost {a:?} round-robin {b:?}");
        assert!(a.by_copy[0] < b.by_copy[0], "least cost asks the busy copy less: {a:?} {b:?}");
    }

    /// Adaptive hedges cut the tail of a copy whose answers stall now and
    /// then (measured: p99.9 61 to 15 ms at 16 clients).
    #[test]
    fn adaptive_hedges_cut_a_stalls_tail() {
        let setup = Setup::reference(1, 3, 2, 64);
        let sc = Scenario { fault: "stall".into(), clients: 16, seconds: 8.0, ..Scenario::default() };
        let fixed = simulate(&setup, &lc(), &sc);
        let adaptive = simulate(&setup, &Variant { hedge_adaptive: true, ..lc() }, &sc);
        assert!(adaptive.p999_ms < fixed.p999_ms * 0.6, "fixed {fixed:?} adaptive {adaptive:?}");
        assert!(adaptive.hedges_per_s > fixed.hedges_per_s);
    }

    /// Reads after a write go to the copies that hold it with
    /// prefer-caught-up, so a replica that applies writes late slows them less.
    #[test]
    fn prefer_caught_up_spares_reads_after_a_write() {
        let setup = Setup::reference(1, 3, 2, 64);
        let sc = Scenario { fault: "behind".into(), clients: 16, seconds: 5.0, ..Scenario::default() };
        let with = simulate(&setup, &lc(), &sc);
        let without = simulate(&setup, &Variant { prefer_caught_up: false, ..lc() }, &sc);
        let (a, b) = (with.after_write_p99_ms.unwrap(), without.after_write_p99_ms.unwrap());
        assert!(a < b, "with {with:?} without {without:?}");
    }

    /// The same settings and seed give the same numbers, and a sweep gives
    /// a row for each setting and count of clients, with the best of each.
    #[test]
    fn a_sweep_is_repeatable() {
        let setup = Setup::reference(2, 2, 2, 64);
        let p = Preview { clients: vec![1, 8], seconds: 2.0, seeds: 1, ..Preview::default() };
        let vs = standard(&lc());
        let a = answer(&setup, &vs, &p, 2);
        let b = answer(&setup, &vs, &p, 1);
        assert_eq!(a["rows"], b["rows"]);
        assert_eq!(a["rows"].as_array().unwrap().len(), vs.len() * 2);
        assert_eq!(a["best"].as_array().unwrap().len(), 2);
        assert_eq!(vs[0].name(), "least cost (this coordinator)");
        assert!(vs.iter().skip(1).all(|v| !v.same(&vs[0])), "{vs:?}");
        assert_eq!(standard(&rr()).len(), 5, "prefer-caught-up is no setting of its own under round-robin's turn");
    }

    #[test]
    fn a_fit_calibrates_it() {
        let fit = Fit {
            tested: super::super::model::Tested { shards: 2, copies: 3.0, copy_cores: 4.0, ..Default::default() },
            kinds: vec![
                super::super::model::KindFit { kind: "bm25".into(), share: 0.8, fanout: true, cpu_ref: 1.2, call_p50: 1.0, call_p99: 3.0, over: 2.0, ..Default::default() },
                super::super::model::KindFit { kind: "key lookup".into(), share: 0.2, fanout: false, cpu_ref: 0.1, call_p50: 0.3, call_p99: 0.6, over: 1.0, ..Default::default() },
            ],
            write_copy_ms: 0.5,
            ..Default::default()
        };
        let s = Setup::from_fit(&fit, 32, "s1");
        assert_eq!((s.shards, s.copies, s.cores, s.slots, s.kinds.len()), (2, 3, 4, 32, 2));
        assert!((s.lookup_ms - 0.1).abs() < 1e-9 && s.kinds[1].fanout == false);
        s.check().unwrap();
        let o = simulate(&s, &lc(), &Scenario { clients: 4, seconds: 2.0, ..Scenario::default() });
        assert!(o.qps > 0.0 && o.p50_ms > 0.0, "{o:?}");
    }
}
