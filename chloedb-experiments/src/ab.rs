//! A/B experiments on a simulated site: users split between rankings by
//! a hash of who they are, each impression served by their arm's ranking
//! (drawn here, or as a logged query to chloedb-server), and a report of
//! every metric with what a real experiment cannot have: the exact truth
//! of each arm, from the click model.
//!
//! The unit of randomisation, and of analysis, is the user: a metric per
//! search is a ratio of two sums over users (clicks over searches), its
//! standard error by the delta method (Deng, Knoblich and Lu, KDD 2018),
//! so the many searches of one user are not taken for independent ones.
//! Each comparison of an arm with the control has its interval and p
//! value, the same with CUPED (Deng et al., WSDM 2013: each user's
//! pre-period under the control as the covariate), the effect the
//! experiment could detect with 80% power (MDE), its power for the true
//! effect and the users that would take, an always-valid p value over
//! the experiment's time (mSPRT, Johari et al., KDD 2017) beside the one a
//! peek at each checkpoint gives, the per-user distribution, and the same
//! within each cohort (age, gender, country, device, tenure), with
//! Benjamini-Hochberg q values across the metrics and across the cells of
//! each. The users assigned are checked against the split planned (sample
//! ratio mismatch).
//!
//! A share of the users (`explore_share`) is not in any arm: it is served
//! the control's ranking through an exploring policy, and its log values
//! every arm off-policy, as if no arm had run, beside what the arms
//! measured and the truth.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::click::{Model, Slot};
use crate::estimate::{self, Target};
use crate::policy::{Policy, Rng};
use crate::server::{Result, Server};
use crate::sim::{key_text, Impression, Ranked, World};
use crate::site::Site;
use crate::stats::{self, norm_cdf};
use crate::users::{Population, Shape, User, ATTRIBUTES};

/// The sums kept per user, per search: searches, clicks, satisfied clicks,
/// searches with a satisfied click, searches with no click, clicks on the
/// first row, seconds of dwell, milliseconds of the query, sessions ended.
pub const SUMS: usize = 9;
pub const N: usize = 0;
pub const CLICKS: usize = 1;
pub const SATISFIED: usize = 2;
pub const SUCCESS: usize = 3;
pub const ABANDON: usize = 4;
pub const TOP: usize = 5;
pub const DWELL: usize = 6;
pub const LATENCY: usize = 7;
pub const ENDS: usize = 8;

/// A metric: the ratio of two of a user's sums, over users.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Metric {
    pub key: &'static str,
    pub title: &'static str,
    /// What one unit of it is.
    pub unit: &'static str,
    /// Whether more is better.
    pub higher: bool,
    /// A guardrail: an arm may not make it worse by more than `margin`
    /// (relative) with p below 0.05.
    pub guardrail: bool,
    pub margin: f64,
    /// A diagnostic: it says why something moved, not whether the arm is
    /// better, so it has no better direction, is not a primary metric and
    /// stays out of the q values.
    pub diagnostic: bool,
    #[serde(skip)]
    pub num: usize,
    #[serde(skip)]
    pub den: usize,
    /// Whether the click model gives its exact value.
    pub exact: bool,
    pub about: &'static str,
}

pub const METRICS: [Metric; 8] = [
    Metric { key: "success", title: "Success rate", unit: "share of searches", higher: true, guardrail: false, margin: 0.0, diagnostic: false, num: SUCCESS, den: N, exact: true, about: "Searches with at least one satisfied click (a click the user stayed on for more than 30 seconds)." },
    Metric { key: "clicks", title: "Clicks per search", unit: "clicks", higher: true, guardrail: false, margin: 0.0, diagnostic: false, num: CLICKS, den: N, exact: true, about: "Every click, satisfied or not, over the searches." },
    Metric { key: "satisfied", title: "Satisfied clicks per search", unit: "clicks", higher: true, guardrail: false, margin: 0.0, diagnostic: true, num: SATISFIED, den: N, exact: true, about: "Clicks the user stayed on for more than 30 seconds, every one of a search's. Close to success rate, which counts a search once: a diagnostic, read beside it, not a second goal." },
    Metric { key: "abandonment", title: "Abandonment", unit: "share of searches", higher: false, guardrail: true, margin: 0.01, diagnostic: false, num: ABANDON, den: N, exact: true, about: "Searches with no click at all." },
    Metric { key: "top", title: "First-row clicks", unit: "share of searches", higher: true, guardrail: false, margin: 0.0, diagnostic: true, num: TOP, den: N, exact: true, about: "Searches whose first row was clicked. A diagnostic with no better direction: more can mean a better first row or a worse rest of the page." },
    Metric { key: "dwell", title: "Dwell time per search", unit: "seconds", higher: true, guardrail: false, margin: 0.0, diagnostic: false, num: DWELL, den: N, exact: true, about: "Seconds spent on the rows clicked, summed over a search's clicks." },
    Metric { key: "session", title: "Searches per session", unit: "searches", higher: true, guardrail: false, margin: 0.0, diagnostic: true, num: N, den: ENDS, exact: true, about: "A session goes on after each search with a chance set by how it went. A diagnostic with no better direction: more searches can mean people find more, or that they had to search again." },
    Metric { key: "latency", title: "Query time, mean", unit: "ms", higher: false, guardrail: false, margin: 0.1, diagnostic: true, num: LATENCY, den: N, exact: false, about: "The server's time to answer the search (took_ms), as it answered then, its answer cache included. Measured, not modelled: a served run times every search, a fast one each query once, the arms taking turns at going first. The guardrail is its 95th percentile, since the slow tail is what people notice; the mean is a diagnostic." },
];

pub fn metric(key: &str) -> Option<&'static Metric> {
    METRICS.iter().find(|m| m.key == key)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Arm {
    pub name: String,
    /// An ORDER BY expression over the site's table, of parameter `:t`
    /// (the search's text).
    pub rank: String,
    /// Its share of the users in the experiment (relative).
    #[serde(default = "one")]
    pub share: f64,
    /// What the arm does to its rows after ranking ([`THENS`]): empty, or
    /// `demote_first`, the first row moved to the end, a small loss of a
    /// known size to test that an experiment can see one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub then: String,
}

fn one() -> f64 {
    1.0
}

fn is_zero(x: &u64) -> bool {
    *x == 0
}

/// The steps an arm may take after ranking.
pub const THENS: &[&str] = &["demote_first"];

/// `then` applied to an arm's rows, in place.
pub fn then_apply<T>(then: &str, rows: &mut Vec<T>) {
    if then == "demote_first" && rows.len() > 1 {
        let first = rows.remove(0);
        rows.push(first);
    }
}

/// What an experiment runs.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Spec {
    pub name: String,
    pub site: String,
    /// The first is the control.
    pub arms: Vec<Arm>,
    #[serde(default = "dbn")]
    pub model: String,
    #[serde(default = "users")]
    pub users: usize,
    #[serde(default = "impressions")]
    pub impressions: u64,
    /// The share of users served the control through `explore` instead,
    /// whose log values the arms off-policy (0: none).
    #[serde(default = "explore_share")]
    pub explore_share: f64,
    #[serde(default = "explore")]
    pub explore: Value,
    #[serde(default = "limit")]
    pub limit: usize,
    /// The users, their tastes and their split.
    #[serde(default = "seed")]
    pub seed: u64,
    /// When not 0, the split is drawn anew for the same users (a repeat
    /// of [`crate::repeat`]).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub salt: u64,
    #[serde(default = "taste")]
    pub taste: f64,
    /// How much users differ in how readily they click (the log sd of a
    /// multiplier of every row's attractiveness).
    #[serde(default = "appetite")]
    pub appetite: f64,
    /// `fast` (drawn here) or `served` (each search a logged query).
    #[serde(default = "fast")]
    pub mode: String,
    /// The metric the experiment is decided on.
    #[serde(default = "primary")]
    pub primary: String,
    /// Searches in the pre-period under the control, for CUPED, as a
    /// share of the experiment's (0: none).
    #[serde(default = "pre")]
    pub pre: f64,
}

fn dbn() -> String {
    "dbn".into()
}
fn users() -> usize {
    20_000
}
fn impressions() -> u64 {
    400_000
}
fn explore_share() -> f64 {
    0.1
}
fn explore() -> Value {
    json!({"policy": "softmax", "tau": 1.0, "top": 10, "by": "rank"})
}
fn limit() -> usize {
    5
}
fn seed() -> u64 {
    fresh_seed()
}

/// A seed of its own for a run that names none: its users, their split
/// and every draw differ from the last run's, so running it again is a
/// check of its own. Below 10^9, which a page's numbers keep exactly.
pub fn fresh_seed() -> u64 {
    static N: AtomicU64 = AtomicU64::new(0);
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
    let n = N.fetch_add(1, Ordering::Relaxed);
    Rng::of(&format!("seed/{t}/{n}/{}", std::process::id())).next_u64() % 1_000_000_000
}
fn taste() -> f64 {
    0.35
}
fn appetite() -> f64 {
    0.6
}
fn fast() -> String {
    "fast".into()
}
fn primary() -> String {
    "success".into()
}
fn pre() -> f64 {
    0.5
}

/// The most a spec may ask for.
pub const MAX_USERS: usize = 500_000;
pub const MAX_FAST: u64 = 50_000_000;
pub const MAX_SERVED: u64 = 1_000_000;
pub const MAX_ARMS: usize = 5;

impl Spec {
    pub fn check(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err("an experiment needs a name".into());
        }
        if self.arms.len() < 2 || self.arms.len() > MAX_ARMS {
            return Err(format!("an experiment has 2 to {MAX_ARMS} arms, the first the control"));
        }
        let mut names = std::collections::HashSet::new();
        for a in &self.arms {
            if a.name.trim().is_empty() || a.rank.trim().is_empty() {
                return Err("each arm needs a name and a ranking".into());
            }
            if !names.insert(a.name.as_str()) {
                return Err(format!("two arms are called {}", a.name));
            }
            if !a.then.is_empty() && !THENS.contains(&a.then.as_str()) {
                return Err(format!("arm {}: then is one of {}", a.name, THENS.join(", ")));
            }
            if !(a.share.is_finite() && a.share > 0.0) {
                return Err(format!("arm {}'s share must be above 0", a.name));
            }
            if a.rank.contains(';') {
                return Err(format!("arm {}'s ranking is an expression, not statements", a.name));
            }
        }
        Model::parse(&self.model)?;
        if self.users < 100 || self.users > MAX_USERS {
            return Err(format!("users: from 100 to {MAX_USERS}"));
        }
        let max = if self.mode == "served" { MAX_SERVED } else { MAX_FAST };
        if !["fast", "served", "interleave"].contains(&self.mode.as_str()) {
            return Err("mode is fast, served or interleave".into());
        }
        if self.mode == "interleave" && (self.arms.len() != 2 || self.explore_share > 0.0) {
            return Err("an interleaving has two arms and no exploring share".into());
        }
        if self.impressions < 1000 || self.impressions > max {
            return Err(format!("searches: from 1,000 to {max} in a {} run", self.mode));
        }
        if !(0.0..=0.5).contains(&self.explore_share) {
            return Err("the exploring share is from 0 to 0.5".into());
        }
        if self.explore_share > 0.0 && !self.arms[0].then.is_empty() {
            return Err("an exploring share explores the control's ranking as the server gives it: the control takes no then step".into());
        }
        if self.explore_share > 0.0 {
            Policy::from_json(&self.explore)?;
        }
        if !(1..=10).contains(&self.limit) {
            return Err("rows shown: from 1 to 10".into());
        }
        if !(0.0..=1.0).contains(&self.taste) {
            return Err("taste: from 0 to 1".into());
        }
        if !(0.0..=1.5).contains(&self.appetite) {
            return Err("how much users differ: from 0 to 1.5".into());
        }
        if !(0.0..=2.0).contains(&self.pre) {
            return Err("the pre-period is from 0 to 2 times the experiment".into());
        }
        if metric(&self.primary).is_none_or(|m| m.diagnostic || m.guardrail) {
            return Err(format!("no primary metric {}: a primary metric is one of success, clicks, dwell", self.primary));
        }
        Ok(())
    }

    pub fn policy(&self) -> Result<Policy> {
        if self.explore_share > 0.0 {
            Policy::from_json(&self.explore)
        } else {
            Ok(Policy::None)
        }
    }
}

/// Which arm a user is in: `None`, the exploring share.
pub fn assign(spec: &Spec, user: u32) -> Option<usize> {
    let u = if spec.salt == 0 { Rng::of(&format!("{}/assign/{user}", spec.seed)) } else { Rng::of(&format!("{}/{}/assign/{user}", spec.seed, spec.salt)) }.uniform();
    if u < spec.explore_share {
        return None;
    }
    let v = (u - spec.explore_share) / (1.0 - spec.explore_share);
    let total: f64 = spec.arms.iter().map(|a| a.share).sum();
    let mut acc = 0.0;
    for (i, a) in spec.arms.iter().enumerate() {
        acc += a.share / total;
        if v < acc {
            return Some(i);
        }
    }
    Some(spec.arms.len() - 1)
}

/// A user's sums.
pub type Sums = [f64; SUMS];

/// One impression's contribution to its user's sums; `go_on` whether the
/// session went on after it.
pub fn sums_of(imp: &Impression, latency: f64, go_on: bool) -> Sums {
    let clicks = imp.click.iter().filter(|&&c| c).count() as f64;
    let sat = imp.satisfied.iter().filter(|&&c| c).count() as f64;
    let mut s = [0.0; SUMS];
    s[N] = 1.0;
    s[CLICKS] = clicks;
    s[SATISFIED] = sat;
    s[SUCCESS] = (sat > 0.0) as u8 as f64;
    s[ABANDON] = (clicks == 0.0) as u8 as f64;
    s[TOP] = imp.click.first().copied().unwrap_or(false) as u8 as f64;
    s[DWELL] = imp.dwell.iter().sum();
    s[LATENCY] = latency;
    s[ENDS] = (!go_on) as u8 as f64;
    s
}

pub fn add(a: &mut Sums, b: &Sums) {
    for k in 0..SUMS {
        a[k] += b[k];
    }
}

/// Whether the session goes on after impression `imp` of user `u`.
pub fn goes_on(u: &User, imp: &Impression, request: &str) -> bool {
    let sat = imp.satisfied.iter().any(|&s| s);
    Rng::of(&format!("{request}/session")).uniform() < if sat { u.go_on.0 } else { u.go_on.1 }
}

/// A ranked site: per arm, each query's rows and the server's time.
#[derive(Clone)]
pub struct Ranks {
    pub arms: Vec<Vec<Ranked>>,
    pub took: Vec<Vec<f64>>,
    /// The control's rows to the exploring policy's depth.
    pub explore: Vec<Ranked>,
}

/// Each arm's rows for each query of `site`, from the server, and its
/// time; the queries some arm ranks fewer than `limit` rows for (or the
/// exploring policy fewer than it draws from) are dropped from the site.
/// The arms take turns at going first, query by query, so that none is
/// timed on a warmer server than the others.
pub async fn rank(server: &Server, site: &mut Site, spec: &Spec, policy: &Policy) -> Result<Ranks> {
    let deep = World::depth(policy, spec.limit);
    let k = spec.arms.len();
    let sqls: Vec<String> = spec.arms.iter().map(|a| format!("SELECT id, score() FROM {} ORDER BY {} DESC LIMIT {}", site.table, a.rank, spec.limit)).collect();
    let mut arms: Vec<Vec<Ranked>> = vec![Vec::new(); k];
    let mut took: Vec<Vec<f64>> = vec![Vec::new(); k];
    let ranked = |ans: &Value| {
        let rows = ans["rows"].as_array().cloned().unwrap_or_default();
        Ranked { keys: rows.iter().map(|x| key_text(&x[0])).collect(), scores: rows.iter().map(|x| x[1].as_f64()).collect() }
    };
    for (qi, q) in site.queries.iter().enumerate() {
        let mut got: Vec<Option<(Ranked, f64)>> = vec![None; k];
        for j in 0..k {
            let a = (qi + j) % k;
            let ans = server.query(&sqls[a], json!({"t": q.text}), None).await.map_err(|e| format!("arm {}: {e}", spec.arms[a].name))?;
            let mut r = ranked(&ans);
            then_apply(&spec.arms[a].then, &mut r.keys);
            then_apply(&spec.arms[a].then, &mut r.scores);
            got[a] = Some((r, ans["took_ms"].as_f64().unwrap_or(0.0)));
        }
        for (a, g) in got.into_iter().enumerate() {
            let (r, t) = g.expect("each arm ranked");
            arms[a].push(r);
            took[a].push(t);
        }
    }
    let mut explore = Vec::new();
    if *policy != Policy::None {
        let sql = format!("SELECT id, score() FROM {} ORDER BY {} DESC LIMIT {deep}", site.table, spec.arms[0].rank);
        for q in &site.queries {
            explore.push(ranked(&server.query(&sql, json!({"t": q.text}), None).await?));
        }
    }
    let keep: Vec<bool> = (0..site.queries.len()).map(|q| arms.iter().all(|r| r[q].keys.len() >= spec.limit) && (explore.is_empty() || explore[q].keys.len() >= deep)).collect();
    if !keep.iter().any(|&k| k) {
        return Err(format!("no query ranks {} rows under every arm", spec.limit));
    }
    fn pick<T>(v: Vec<T>, keep: &[bool]) -> Vec<T> {
        v.into_iter().zip(keep).filter(|(_, k)| **k).map(|(r, _)| r).collect()
    }
    let arms = arms.into_iter().map(|r| pick(r, &keep)).collect();
    let took = took.into_iter().map(|t| pick(t, &keep)).collect();
    let explore = pick(explore, &keep);
    site.keep(&keep);
    Ok(Ranks { arms, took, explore })
}

/// Everything a run keeps to report on and to cut again: each user's
/// attributes, arm, sums in the experiment and in the pre-period, and
/// exact per-search values under each arm.
pub struct RunData {
    pub spec: Spec,
    pub users: Vec<User>,
    /// Per user: its arm (`None`: the exploring share).
    pub arm: Vec<Option<usize>>,
    pub sums: Vec<Sums>,
    pub pre: Vec<Sums>,
    /// Per user, per arm: the exact expected sums of one search.
    pub truth: Vec<Vec<Sums>>,
    /// Per user: their share of the searches.
    pub share: Vec<f64>,
    /// Per checkpoint: searches so far, and per metric, per arm after the
    /// control, the difference and its standard error then.
    pub checkpoints: Vec<(u64, Vec<Vec<(f64, f64)>>)>,
    /// Per arm, every search's query time (served), or each query's.
    pub latencies: Vec<Vec<f64>>,
    pub whatif: Option<Value>,
    pub seconds: f64,
    pub site: Value,
    /// Per query, each arm measured and exactly ([`queries_report`]).
    pub by_query: Option<ByQuery>,
}

/// How far a run has got: searches done of the total.
#[derive(Default)]
pub struct Progress {
    pub done: AtomicU64,
    pub total: AtomicU64,
    pub stage: std::sync::Mutex<String>,
}

impl Progress {
    pub fn stage(&self, s: &str) {
        *self.stage.lock().unwrap_or_else(|e| e.into_inner()) = s.to_string();
    }
}

const CHECKPOINTS: u64 = 20;
/// The exploring share's impressions kept to value the arms from.
const MAX_EXPLORED: usize = 1_000_000;

/// The exact sums of one search of user `u` under the lists `lists`
/// (world `w`'s rows), over the site's queries as they are drawn.
pub fn exact(w: &World, u: &User, lists: &[Vec<String>], qshares: &[f64]) -> Sums {
    let mut s = [0.0; SUMS];
    s[N] = 1.0;
    let mut go_on = 0.0;
    for (q, list) in lists.iter().enumerate() {
        let slots: Vec<Slot> = list.iter().take(w.limit).map(|k| w.slot(u, q, k)).collect();
        let e = w.model.expected(&slots, &u.behaviour);
        let p = qshares[q];
        s[CLICKS] += p * e.click.iter().sum::<f64>();
        s[SATISFIED] += p * e.satisfied.iter().sum::<f64>();
        s[SUCCESS] += p * e.any_satisfied;
        s[ABANDON] += p * (1.0 - e.any_click);
        s[TOP] += p * e.click.first().copied().unwrap_or(0.0);
        s[DWELL] += p * e.dwell;
        go_on += p * (e.any_satisfied * u.go_on.0 + (1.0 - e.any_satisfied) * u.go_on.1);
    }
    s[ENDS] = 1.0 - go_on;
    s
}

fn threads() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(16)
}

/// Runs `f` on `0..n` split over the threads, in order.
pub fn par<T: Send>(n: usize, f: impl Fn(std::ops::Range<usize>) -> T + Sync) -> Vec<T> {
    let t = threads();
    let per = n.div_ceil(t).max(1);
    std::thread::scope(|s| {
        let hs: Vec<_> = (0..t)
            .map(|i| (i * per, ((i + 1) * per).min(n)))
            .filter(|(a, b)| a < b)
            .map(|(a, b)| {
                let f = &f;
                s.spawn(move || f(a..b))
            })
            .collect();
        hs.into_iter().map(|h| h.join().expect("a thread")).collect()
    })
}

/// Per metric, per arm after the control: the difference from the
/// control and its standard error, over every user.
fn deltas(sums: &[Sums], arm: &[Option<usize>], k: usize) -> Vec<Vec<(f64, f64)>> {
    METRICS
        .iter()
        .map(|m| {
            let s0 = group_ratio(sums, arm, &|_| true, 0, m);
            (1..k)
                .map(|a| {
                    let s1 = group_ratio(sums, arm, &|_| true, a, m);
                    (s1.value - s0.value, (s0.se * s0.se + s1.se * s1.se).sqrt())
                })
                .collect()
        })
        .collect()
}

/// Runs an experiment: `site` ranked by `ranks`, its searches drawn here
/// or served by `server` (`spec.mode`).
pub async fn run(server: &Server, spec: &Spec, run_id: &str, site: Site, ranks: Ranks, progress: Arc<Progress>) -> Result<RunData> {
    let t0 = std::time::Instant::now();
    let model = Model::parse(&spec.model)?;
    let policy = spec.policy()?;
    let shape = Shape { taste: spec.taste, appetite: spec.appetite, ..Shape::default() };
    let pop = || Population::new(spec.users, spec.seed, shape);
    let arms: Vec<Arc<World>> = ranks.arms.iter().zip(&spec.arms).map(|(r, a)| Arc::new(World::new(site.clone(), pop(), model, Policy::None, spec.limit, a.rank.clone(), r.clone()).then(&a.then))).collect();
    let explorer = (policy != Policy::None).then(|| Arc::new(World::new(site.clone(), pop(), model, policy.clone(), spec.limit, spec.arms[0].rank.clone(), ranks.explore.clone())));
    let users = arms[0].users.users.clone();
    let nu = users.len();
    let k = arms.len();
    let assigned: Vec<Option<usize>> = users.iter().map(|u| assign(spec, u.id)).collect();
    let share = arms[0].users.shares();
    let qshares = site.shares();
    let lists: Vec<Vec<Vec<String>>> = ranks.arms.iter().map(|r| r.iter().map(|x| x.keys.clone()).collect()).collect();
    let pre_n = (spec.impressions as f64 * spec.pre).round() as u64;
    progress.total.store(spec.impressions + pre_n, Ordering::Relaxed);

    // The truth: each user's exact search under each arm.
    progress.stage("computing every user's exact values under each arm");
    let truth: Vec<Vec<Sums>> = {
        let (arms, lists, users, qs) = (&arms, &lists, &users, &qshares);
        par(nu, |r| r.map(|i| (0..k).map(|a| exact(&arms[a], &users[i], &lists[a], qs)).collect::<Vec<Sums>>()).collect::<Vec<_>>()).into_iter().flatten().collect()
    };
    let mut by_query = ByQuery::new(&site, &arms, &users, &share, &lists);

    // The pre-period: every user under the control, a run of its own.
    progress.stage("the pre-period under the control");
    let pre: Vec<Sums> = {
        let (w, took, users) = (&arms[0], &ranks.took[0], &users);
        let pre_run = format!("{run_id}pre");
        let parts = par(pre_n as usize, |r| {
            let mut s = vec![[0.0; SUMS]; nu];
            for i in r {
                let imp = w.fast(&pre_run, i as u64);
                let u = &users[imp.user as usize];
                add(&mut s[imp.user as usize], &sums_of(&imp, took[imp.query as usize], goes_on(u, &imp, &format!("{pre_run}-{i}"))));
            }
            s
        });
        progress.done.fetch_add(pre_n, Ordering::Relaxed);
        merge_users(parts, nu)
    };

    // The experiment, in checkpoints.
    progress.stage(if spec.mode == "served" { "serving every search through chloedb-server" } else { "drawing every search" });
    let mut sums = vec![[0.0; SUMS]; nu];
    let mut checkpoints = Vec::new();
    let mut latencies: Vec<Vec<f64>> = vec![Vec::new(); k];
    let mut explored: Vec<Impression> = Vec::new();
    let step = spec.impressions.div_ceil(CHECKPOINTS).max(1);
    let log = format!("exp_log_{run_id}");
    let events = format!("exp_events_{run_id}");
    if spec.mode == "served" {
        server.sql(&format!("CREATE TABLE IF NOT EXISTS {events} (id KEYWORD PRIMARY KEY, usr KEYWORD NOT NULL, item KEYWORD, action KEYWORD, ts INT, dwell_ms FLOAT)"), json!({})).await?;
    }
    let mut start = 0u64;
    while start < spec.impressions {
        let end = (start + step).min(spec.impressions);
        if spec.mode == "served" {
            for (imp, a, l) in served_batch(server, &arms, explorer.as_ref(), &assigned, run_id, &log, &events, start..end, &progress).await? {
                match a {
                    Some(a) => {
                        let u = &users[imp.user as usize];
                        add(&mut sums[imp.user as usize], &sums_of(&imp, l, goes_on(u, &imp, &format!("{run_id}-{}", imp.i))));
                        by_query.add(&imp, a);
                        latencies[a].push(l);
                    }
                    None if explored.len() < MAX_EXPLORED => explored.push(imp),
                    None => {}
                }
            }
        } else {
            let (arms, explorer, assigned, took, users) = (&arms, &explorer, &assigned, &ranks.took, &users);
            let room = MAX_EXPLORED.saturating_sub(explored.len());
            let nq = by_query.text.len();
            let parts = par((end - start) as usize, |r| {
                let mut s = vec![[0.0; SUMS]; nu];
                let mut bq = vec![[0.0; 4]; nq * k];
                let mut ex = Vec::new();
                for j in r {
                    let i = start + j as u64;
                    let c = arms[0].context(run_id, i);
                    match assigned[c.user as usize] {
                        Some(a) => {
                            let imp = arms[a].fast(run_id, i);
                            let u = &users[imp.user as usize];
                            add(&mut s[imp.user as usize], &sums_of(&imp, took[a][imp.query as usize], goes_on(u, &imp, &c.request)));
                            add_query(&mut bq, k, &imp, a);
                        }
                        None => {
                            if let Some(x) = explorer {
                                if ex.len() < room {
                                    ex.push(x.fast(run_id, i));
                                }
                            }
                        }
                    }
                }
                (s, bq, ex)
            });
            for (s, bq, ex) in parts {
                for (o, x) in sums.iter_mut().zip(&s) {
                    add(o, x);
                }
                for (o, x) in by_query.measured.iter_mut().zip(&bq) {
                    for (y, z) in o.iter_mut().zip(x) {
                        *y += z;
                    }
                }
                let room = MAX_EXPLORED.saturating_sub(explored.len());
                explored.extend(ex.into_iter().take(room));
            }
            progress.done.fetch_add(end - start, Ordering::Relaxed);
        }
        checkpoints.push((end, deltas(&sums, &assigned, k)));
        start = end;
    }
    if spec.mode != "served" {
        // A fast run times each query once: its searches' times are those.
        latencies = ranks.took.clone();
    }

    // What the exploring share's log says of each arm.
    let whatif = match &explorer {
        Some(x) if explored.len() >= 100 => {
            progress.stage("valuing every arm off-policy from the exploring share's log");
            let targets: Vec<Target> = spec.arms.iter().zip(&lists).map(|(a, l)| Target::new(x, &a.name, l.clone())).collect();
            let est = estimate::value_all(&explored, &targets, x.limit);
            let keep = |u: &User| assign(spec, u.id).is_none();
            let rows: Vec<Value> = targets
                .iter()
                .enumerate()
                .map(|(a, t)| {
                    let tr = x.truth(&t.keys, &keep);
                    let all = x.truth(&t.keys, &|_| true);
                    json!({
                        "arm": t.name,
                        "unsupported": t.unsupported(x),
                        "truth": {"clicks": all.clicks, "satisfied": all.satisfied},
                        "truth_explored": {"clicks": tr.clicks, "satisfied": tr.satisfied},
                        "measured": arm_value(&sums, &assigned, a),
                        "estimates": est[a].iter().map(|e| json!({"estimator": e.estimator, "reward": e.reward, "value": e.value, "se": e.se, "ess": e.ess, "weight_mean": e.weight_mean, "shown": e.trusted(), "holds": estimate::should_hold(e.estimator, model)})).collect::<Vec<_>>(),
                    })
                })
                .collect();
            Some(json!({"policy": x.policy.to_json(), "share": spec.explore_share, "searches": explored.len(), "users": assigned.iter().filter(|a| a.is_none()).count(), "arms": rows}))
        }
        _ => None,
    };
    let site_info = json!({"name": site.name, "table": site.table, "queries": site.queries.len()});
    Ok(RunData { spec: spec.clone(), users, arm: assigned, sums, pre, truth, share, checkpoints, latencies, whatif, seconds: t0.elapsed().as_secs_f64(), site: site_info, by_query: Some(by_query) })
}

/// Clicks and satisfied clicks per search an arm measured.
fn arm_value(sums: &[Sums], assigned: &[Option<usize>], a: usize) -> Value {
    let (mut n, mut c, mut s) = (0.0, 0.0, 0.0);
    for (u, x) in sums.iter().enumerate() {
        if assigned[u] == Some(a) {
            n += x[N];
            c += x[CLICKS];
            s += x[SATISFIED];
        }
    }
    json!({"clicks": c / n.max(1.0), "satisfied": s / n.max(1.0)})
}

fn merge_users(parts: Vec<Vec<Sums>>, n: usize) -> Vec<Sums> {
    let mut out = vec![[0.0; SUMS]; n];
    for p in parts {
        for (o, x) in out.iter_mut().zip(p) {
            add(o, &x);
        }
    }
    out
}

/// Serves searches `range` through the server, `CONCURRENCY` at a time.
#[allow(clippy::too_many_arguments)]
async fn served_batch(server: &Server, arms: &[Arc<World>], explorer: Option<&Arc<World>>, assigned: &[Option<usize>], run_id: &str, log: &str, events: &str, range: std::ops::Range<u64>, progress: &Arc<Progress>) -> Result<Vec<(Impression, Option<usize>, f64)>> {
    const CONCURRENCY: usize = 8;
    let next = Arc::new(AtomicU64::new(range.start));
    let mut tasks = Vec::new();
    for _ in 0..CONCURRENCY {
        let (server, next, progress) = (server.clone(), next.clone(), progress.clone());
        let arms: Vec<Arc<World>> = arms.to_vec();
        let explorer = explorer.cloned();
        let assigned = assigned.to_vec();
        let (run_id, log, events, end) = (run_id.to_string(), log.to_string(), events.to_string(), range.end);
        tasks.push(tokio::spawn(async move {
            let mut got = Vec::new();
            let mut ops: Vec<Value> = Vec::new();
            loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= end {
                    break;
                }
                let c = arms[0].context(&run_id, i);
                let (w, a, variant) = match assigned[c.user as usize] {
                    Some(a) => (&arms[a], Some(a), format!("arm{a}")),
                    None => (explorer.as_ref().unwrap_or(&arms[0]), None, "explore".to_string()),
                };
                let (imp, o) = w.served_as(&server, &run_id, &variant, &log, &events, i).await?;
                ops.extend(o);
                if ops.len() >= 500 {
                    server.write(std::mem::take(&mut ops)).await?;
                }
                let l = imp.took_ms;
                got.push((imp, a, l));
                progress.done.fetch_add(1, Ordering::Relaxed);
            }
            if !ops.is_empty() {
                server.write(ops).await?;
            }
            Ok::<_, String>(got)
        }));
    }
    let mut out = Vec::new();
    for t in tasks {
        out.extend(t.await.map_err(|e| e.to_string())??);
    }
    out.sort_by_key(|x| x.0.i);
    Ok(out)
}

// ------------------------------------------------------------ statistics

/// A ratio metric over a group of users: its value and standard error by
/// the delta method, and the users' linearised values for CUPED.
#[derive(Clone, Debug, Default)]
pub struct RatioStat {
    pub users: usize,
    pub num: f64,
    pub den: f64,
    pub value: f64,
    pub se: f64,
}

/// The delta-method ratio of sums `num`/`den` over the users `xs`.
pub fn ratio(xs: &[&Sums], num: usize, den: usize) -> RatioStat {
    let n = xs.len() as f64;
    if xs.is_empty() {
        return RatioStat::default();
    }
    let (mut sa, mut sb) = (0.0, 0.0);
    for x in xs {
        sa += x[num];
        sb += x[den];
    }
    if sb <= 0.0 {
        return RatioStat { users: xs.len(), num: sa, den: sb, value: f64::NAN, se: f64::NAN };
    }
    let r = sa / sb;
    let (ma, mb) = (sa / n, sb / n);
    let (mut vaa, mut vbb, mut vab) = (0.0, 0.0, 0.0);
    for x in xs {
        let (a, b) = (x[num] - ma, x[den] - mb);
        vaa += a * a;
        vbb += b * b;
        vab += a * b;
    }
    let d = (n - 1.0).max(1.0);
    let var = (vaa / d - 2.0 * r * vab / d + r * r * vbb / d) / (n * mb * mb);
    RatioStat { users: xs.len(), num: sa, den: sb, value: r, se: var.max(0.0).sqrt() }
}

/// Two-sided p value of a z statistic.
pub fn p_two(z: f64) -> f64 {
    if !z.is_finite() {
        return if z.is_nan() { f64::NAN } else { 0.0 };
    }
    (2.0 * (1.0 - norm_cdf(z.abs()))).clamp(0.0, 1.0)
}

pub const Z975: f64 = 1.959963984540054;
pub const Z80: f64 = 0.8416212335729143;

/// Benjamini-Hochberg q values of `ps` (NaN stays NaN).
pub fn bh(ps: &[f64]) -> Vec<f64> {
    let mut idx: Vec<usize> = (0..ps.len()).filter(|&i| ps[i].is_finite()).collect();
    idx.sort_by(|&a, &b| ps[a].total_cmp(&ps[b]));
    let m = idx.len() as f64;
    let mut q = vec![f64::NAN; ps.len()];
    let mut min: f64 = 1.0;
    for (rank, &i) in idx.iter().enumerate().rev() {
        min = min.min(ps[i] * m / (rank + 1) as f64);
        q[i] = min.min(1.0);
    }
    q
}

/// The mSPRT's always-valid p value after a sequence of estimates
/// `(delta, se)`, with a normal mixture of standard deviation `tau`.
pub fn always_valid(series: &[(f64, f64)], tau: f64) -> Vec<f64> {
    let mut p: f64 = 1.0;
    series
        .iter()
        .map(|&(d, se)| {
            let v = se * se;
            if v > 0.0 && v.is_finite() && d.is_finite() {
                let t2 = tau * tau;
                let ln = 0.5 * (v / (v + t2)).ln() + t2 * d * d / (2.0 * v * (v + t2));
                p = p.min((-ln).exp().min(1.0));
            }
            p
        })
        .collect()
}

/// The half-width of the always-valid 95% interval (a confidence
/// sequence) that goes with [`always_valid`]'s p value, for a difference
/// with standard error `se` and mixing scale `tau`: the differences it
/// keeps are those whose p value would stay above 0.05.
pub fn always_valid_half(se: f64, tau: f64) -> f64 {
    let (v, t2) = (se * se, tau * tau);
    if !(v > 0.0 && v.is_finite() && t2 > 0.0) {
        return f64::NAN;
    }
    (2.0 * v * (v + t2) / t2 * ((1.0f64 / 0.05).ln() + 0.5 * ((v + t2) / v).ln())).sqrt()
}

/// Cochran's Q over differences `(delta, se)`: whether they differ more
/// than their errors say, the test of an interaction between the arm and
/// the cells they come from. Cells with no error are left out. Returns
/// (Q, degrees of freedom, p), or none with fewer than two cells.
pub fn cochran_q(cells: &[(f64, f64)]) -> Option<(f64, f64, f64)> {
    let ok: Vec<(f64, f64)> = cells.iter().copied().filter(|&(d, se)| d.is_finite() && se.is_finite() && se > 0.0).collect();
    if ok.len() < 2 {
        return None;
    }
    let w: Vec<f64> = ok.iter().map(|&(_, se)| 1.0 / (se * se)).collect();
    let pooled = ok.iter().zip(&w).map(|(&(d, _), w)| w * d).sum::<f64>() / w.iter().sum::<f64>();
    let q: f64 = ok.iter().zip(&w).map(|(&(d, _), w)| w * (d - pooled) * (d - pooled)).sum();
    let df = (ok.len() - 1) as f64;
    Some((q, df, stats::chi2_upper(q, df)))
}

/// Quantile `q` of `v`, sorted, with its standard error from the order
/// statistics' distribution-free 95% interval (ranks n q ± 1.96 √(n q (1 − q))).
fn quantile_se(v: &[f64], q: f64) -> (f64, f64) {
    let n = v.len();
    if n == 0 {
        return (f64::NAN, f64::NAN);
    }
    let at = |r: f64| v[(r.round().max(0.0) as usize).min(n - 1)];
    let mid = (n - 1) as f64 * q;
    let half = Z975 * (n as f64 * q * (1.0 - q)).sqrt();
    (at(mid), (at(mid + half) - at(mid - half)) / (2.0 * Z975))
}

/// The guardrail on query time: each arm's 95th percentile against the
/// control's, a metric of the report's shape (no series, no truth: the
/// times are measured). `latencies` per arm; `av`, the always-valid p
/// value per arm after the control when the run keeps a series of them.
pub fn latency_p95(names: &[String], latencies: &[Vec<f64>], av: Option<&[f64]>) -> Value {
    latency_quantile(names, latencies, 0.95, av)
}

/// The 99th percentile of query time, read as a diagnostic: the far tail,
/// which moves before the guardrail's p95 does.
pub fn latency_p99(names: &[String], latencies: &[Vec<f64>]) -> Value {
    latency_quantile(names, latencies, 0.99, None)
}

fn latency_quantile(names: &[String], latencies: &[Vec<f64>], q: f64, av: Option<&[f64]>) -> Value {
    let sorted: Vec<Vec<f64>> = latencies.iter().map(|l| { let mut v = l.clone(); v.sort_by(f64::total_cmp); v }).collect();
    let qs: Vec<(f64, f64)> = sorted.iter().map(|v| quantile_se(v, q)).collect();
    let (q0, se0) = qs[0];
    let comps: Vec<Value> = (1..names.len())
        .map(|a| {
            let (q1, se1) = qs[a];
            let delta = q1 - q0;
            let se = (se0 * se0 + se1 * se1).sqrt();
            let lift = q1 / q0 - 1.0;
            let lift_se = (q1 / q0) * ((se1 / q1).powi(2) + (se0 / q0).powi(2)).sqrt();
            let p = if se > 0.0 { p_two(delta / se) } else { f64::NAN };
            let num = |x: f64| if x.is_finite() { json!(x) } else { Value::Null };
            json!({
                "arm": names[a],
                "control": {"users": Value::Null, "searches": sorted[0].len(), "value": num(q0), "se": num(se0)},
                "treatment": {"users": Value::Null, "searches": sorted[a].len(), "value": num(q1), "se": num(se1)},
                "delta": num(delta), "se": num(se), "lo": num(delta - Z975 * se), "hi": num(delta + Z975 * se), "p": num(p),
                "p_always_valid": av.and_then(|v| v.get(a - 1)).map_or(Value::Null, |&x| num(x)),
                "lift": num(lift), "lift_lo": num(lift - Z975 * lift_se), "lift_hi": num(lift + Z975 * lift_se),
                "true_delta": Value::Null, "true_lift": Value::Null, "mde": num((Z975 + Z80) * se), "mde_lift": num((Z975 + Z80) * se / q0.abs()), "power": Value::Null,
            })
        })
        .collect();
    let arms: Vec<Value> = names.iter().zip(&qs).zip(&sorted).map(|((n, &(q, se)), v)| json!({"arm": n, "value": if q.is_finite() { json!(q) } else { Value::Null }, "se": if se.is_finite() { json!(se) } else { Value::Null }, "users": Value::Null, "searches": v.len(), "truth": Value::Null})).collect();
    if q >= 0.99 {
        return json!({
            "key": "latency_p99", "title": "Query time, 99th percentile", "unit": "ms", "higher": Value::Null, "guardrail": false, "margin": Value::Null, "diagnostic": true, "quantile": true, "exact": false,
            "about": "The 99th percentile of the server's time to answer, per arm: the far tail, read beside the guardrail and not deciding. One search in a hundred waits this long or longer; its interval comes from the order statistics, and is wide until there are thousands of searches.",
            "arms": arms, "comparisons": comps,
        });
    }
    json!({
        "key": "latency_p95", "title": "Query time, 95th percentile", "unit": "ms", "higher": false, "guardrail": true, "margin": 0.1, "diagnostic": false, "quantile": true, "exact": false,
        "about": "The 95th percentile of the server's time to answer, per arm: the guardrail on speed, since the slow tail is what people notice. Its interval comes from the order statistics, which assume nothing of the times' shape.",
        "arms": arms, "comparisons": comps,
    })
}

/// The difference of arm `a`'s p95 query time from the control's and its
/// standard error, for a series of always-valid p values.
pub fn latency_p95_delta(latencies: &[Vec<f64>], a: usize) -> (f64, f64) {
    let q = |l: &Vec<f64>| { let mut v = l.clone(); v.sort_by(f64::total_cmp); quantile_se(&v, 0.95) };
    let ((q0, s0), (q1, s1)) = (q(&latencies[0]), q(&latencies[a]));
    (q1 - q0, (s0 * s0 + s1 * s1).sqrt())
}

/// The CUPED estimate of arm `b` against arm `a` for one metric: each
/// user's linearised value adjusted by their pre-period's, with one θ for
/// both arms. Returns (delta, se, the share of variance it removed).
fn cuped(ya: &[(&Sums, &Sums)], yb: &[(&Sums, &Sums)], num: usize, den: usize) -> Option<(f64, f64, f64)> {
    let lin = |g: &[(&Sums, &Sums)]| -> Option<(f64, Vec<(f64, f64)>)> {
        let n = g.len() as f64;
        let (sy, sn): (f64, f64) = g.iter().fold((0.0, 0.0), |a, (x, _)| (a.0 + x[num], a.1 + x[den]));
        let (sx, sm): (f64, f64) = g.iter().fold((0.0, 0.0), |a, (_, p)| (a.0 + p[num], a.1 + p[den]));
        if sn <= 0.0 || sm <= 0.0 {
            return None;
        }
        let (r, rx) = (sy / sn, sx / sm);
        let (nb, mb) = (sn / n, sm / n);
        Some((r, g.iter().map(|(x, p)| ((x[num] - r * x[den]) / nb, (p[num] - rx * p[den]) / mb)).collect()))
    };
    let (ra, la) = lin(ya)?;
    let (rb, lb) = lin(yb)?;
    // The pre-period's ratios, each arm's and both arms' together.
    let pre_ratio = |g: &[(&Sums, &Sums)]| {
        let (sx, sm): (f64, f64) = g.iter().fold((0.0, 0.0), |a, (_, p)| (a.0 + p[num], a.1 + p[den]));
        sx / sm
    };
    let (pa, pb) = (pre_ratio(ya), pre_ratio(yb));
    let all: Vec<&(f64, f64)> = la.iter().chain(lb.iter()).collect();
    let n = all.len() as f64;
    let (ml, mk) = all.iter().fold((0.0, 0.0), |a, (l, k)| (a.0 + l, a.1 + k));
    let (ml, mk) = (ml / n, mk / n);
    let (mut c, mut vk) = (0.0, 0.0);
    for (l, k) in &all {
        c += (l - ml) * (k - mk);
        vk += (k - mk) * (k - mk);
    }
    if vk <= 0.0 {
        return None;
    }
    let theta = c / vk;
    // The pre-period's ratio difference, scaled as the linearised values are.
    let var_of = |l: &[(f64, f64)]| {
        let n = l.len() as f64;
        let adj: Vec<f64> = l.iter().map(|(y, k)| y - theta * k).collect();
        let m = adj.iter().sum::<f64>() / n;
        adj.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (n - 1.0).max(1.0) / n
    };
    let raw = |l: &[(f64, f64)]| {
        let n = l.len() as f64;
        let m = l.iter().map(|x| x.0).sum::<f64>() / n;
        l.iter().map(|x| (x.0 - m) * (x.0 - m)).sum::<f64>() / (n - 1.0).max(1.0) / n
    };
    let delta = (rb - ra) - theta * (pb - pa);
    let se = (var_of(&la) + var_of(&lb)).sqrt();
    let se_raw = (raw(&la) + raw(&lb)).sqrt();
    Some((delta, se, 1.0 - (se / se_raw).powi(2)))
}

/// The share of user weights (`share`) of each user kept, and the exact
/// value of arm `a` for metric `m` over them.
fn truth_of(d: &RunData, keep: &dyn Fn(usize) -> bool, a: usize, m: &Metric) -> Option<f64> {
    if !m.exact {
        return None;
    }
    let (mut num, mut den) = (0.0, 0.0);
    for u in 0..d.users.len() {
        if keep(u) {
            num += d.share[u] * d.truth[u][a][m.num];
            den += d.share[u] * d.truth[u][a][m.den];
        }
    }
    (den > 0.0).then(|| num / den)
}

/// A metric over the users of arm `a` that `keep` keeps.
fn group_ratio(sums: &[Sums], arm: &[Option<usize>], keep: &dyn Fn(usize) -> bool, a: usize, m: &Metric) -> RatioStat {
    let g: Vec<&Sums> = (0..sums.len()).filter(|&u| arm[u] == Some(a) && sums[u][N] > 0.0 && keep(u)).map(|u| &sums[u]).collect();
    ratio(&g, m.num, m.den)
}

/// One arm against the control on one metric, among users `keep` keeps.
fn compare(d: &RunData, keep: &dyn Fn(usize) -> bool, a: usize, m: &Metric) -> Value {
    let (s0, s1) = (group_ratio(&d.sums, &d.arm, keep, 0, m), group_ratio(&d.sums, &d.arm, keep, a, m));
    let delta = s1.value - s0.value;
    let se = (s0.se * s0.se + s1.se * s1.se).sqrt();
    let p = p_two(delta / se);
    let lift = s1.value / s0.value - 1.0;
    let lift_se = (s1.value / s0.value) * ((s1.se / s1.value).powi(2) + (s0.se / s0.value).powi(2)).sqrt();
    let (t0, t1) = (truth_of(d, keep, 0, m), truth_of(d, keep, a, m));
    let true_delta = t0.zip(t1).map(|(a, b)| b - a);
    let mde = (Z975 + Z80) * se;
    let power = true_delta.map(|t| norm_cdf(t.abs() / se - Z975) + norm_cdf(-t.abs() / se - Z975));
    let total = (s0.users + s1.users) as f64;
    let needed = true_delta.filter(|t| t.abs() > 1e-12).map(|t| (total * (mde / t.abs()).powi(2)).ceil());
    json!({
        "arm": d.spec.arms[a].name,
        "control": {"users": s0.users, "searches": s0.den, "value": s0.value, "se": s0.se, "truth": t0},
        "treatment": {"users": s1.users, "searches": s1.den, "value": s1.value, "se": s1.se, "truth": t1},
        "delta": delta, "se": se, "lo": delta - Z975 * se, "hi": delta + Z975 * se, "p": p,
        "lift": lift, "lift_lo": lift - Z975 * lift_se, "lift_hi": lift + Z975 * lift_se,
        "true_delta": true_delta, "true_lift": t0.zip(t1).map(|(a, b)| b / a - 1.0),
        "covers": true_delta.map(|t| (t - delta).abs() <= Z975 * se),
        "mde": mde, "mde_lift": mde / s0.value.abs(), "power": power, "users_needed": needed,
    })
}

/// Per-user values of a metric per arm, binned alike.
fn histogram(d: &RunData, m: &Metric) -> Value {
    let vals: Vec<Vec<f64>> = (0..d.spec.arms.len()).map(|a| (0..d.users.len()).filter(|&u| d.arm[u] == Some(a) && d.sums[u][m.den] > 0.0).map(|u| d.sums[u][m.num] / d.sums[u][m.den]).filter(|v| v.is_finite()).collect()).collect();
    let mut all: Vec<f64> = vals.iter().flatten().copied().collect();
    if all.is_empty() {
        return json!(null);
    }
    all.sort_by(f64::total_cmp);
    let lo = all[0].min(0.0);
    let hi = all[((all.len() - 1) as f64 * 0.99) as usize].max(lo + 1e-9);
    const BINS: usize = 24;
    let w = (hi - lo) / BINS as f64;
    let counts: Vec<Vec<f64>> = vals
        .iter()
        .map(|v| {
            let mut c = vec![0.0; BINS];
            for x in v {
                let b = (((x - lo) / w) as usize).min(BINS - 1);
                c[b] += 1.0;
            }
            let n = v.len().max(1) as f64;
            c.iter().map(|x| x / n).collect()
        })
        .collect();
    json!({"lo": lo, "hi": hi, "bins": BINS, "share": counts})
}

/// Arm `a` against the control on metric `mi` at each checkpoint: the
/// difference, its interval, the p value a peek then gives and the
/// always-valid one.
fn series(d: &RunData, a: usize, mi: usize, tau: f64) -> Value {
    let est: Vec<(f64, f64)> = d.checkpoints.iter().map(|(_, c)| c[mi][a - 1]).collect();
    let av = always_valid(&est, tau);
    let pts: Vec<Value> = d
        .checkpoints
        .iter()
        .zip(&est)
        .zip(av)
        .map(|(((n, _), &(delta, se)), v)| json!({"searches": n, "delta": delta, "lo": delta - Z975 * se, "hi": delta + Z975 * se, "p": p_two(delta / se), "p_always_valid": v}))
        .collect();
    json!(pts)
}

/// The sample ratio check: users in each arm against the split planned.
fn srm(d: &RunData) -> Value {
    let k = d.spec.arms.len();
    let mut seen = vec![0.0; k];
    for (u, a) in d.arm.iter().enumerate() {
        if let Some(a) = a {
            if d.sums[u][N] > 0.0 {
                seen[*a] += 1.0;
            }
        }
    }
    let total: f64 = seen.iter().sum();
    let shares: f64 = d.spec.arms.iter().map(|a| a.share).sum();
    let chi: f64 = d.spec.arms.iter().zip(&seen).map(|(a, s)| { let e = total * a.share / shares; if e > 0.0 { (s - e) * (s - e) / e } else { 0.0 } }).sum();
    let p = stats::chi2_upper(chi, (k - 1) as f64);
    json!({"users": seen, "planned": d.spec.arms.iter().map(|a| a.share / shares).collect::<Vec<_>>(), "chi2": chi, "p": p, "ok": p >= 0.001})
}

/// Who the users were: the population, those in an arm who searched (the
/// ones every metric counts), those given to the exploring share, and
/// those in an arm who never searched.
fn counts(d: &RunData) -> Value {
    let searched = |u: usize| d.sums[u][N] > 0.0;
    let in_arms = (0..d.users.len()).filter(|&u| d.arm[u].is_some() && searched(u)).count();
    let exploring = d.arm.iter().filter(|a| a.is_none()).count();
    json!({"population": d.users.len(), "in_arms": in_arms, "exploring": exploring, "idle": d.users.len() - in_arms - exploring})
}

fn percentile(v: &mut [f64], p: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * p).round() as usize]
}

/// The cohort cells a report splits by, largest first: one attribute's
/// values, or a cross of two; and each user's cell.
pub fn cells(d: &RunData, by: &[&str]) -> (Vec<(String, usize)>, Vec<usize>) {
    let names: Vec<String> = d.users.iter().map(|u| by.iter().map(|a| u.attribute(a)).collect::<Vec<_>>().join(" · ")).collect();
    let mut count: HashMap<&str, usize> = HashMap::new();
    for n in &names {
        *count.entry(n.as_str()).or_default() += 1;
    }
    let mut out: Vec<(String, usize)> = count.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let at: HashMap<&str, usize> = out.iter().enumerate().map(|(i, (k, _))| (k.as_str(), i)).collect();
    let of = names.iter().map(|n| at[n.as_str()]).collect();
    (out, of)
}

/// Every metric for every arm within each cell of `by` (one attribute's
/// values, or a cross of two), with q values across the cells.
pub fn cohort_table(d: &RunData, by: &[&str]) -> Value {
    let (cells, of) = cells(d, by);
    // The users of each cell who searched (a live run's population is
    // larger than the users it has seen).
    let mut active = vec![0u64; cells.len()];
    for (u, c) in of.iter().enumerate() {
        if d.sums[u][N] > 0.0 {
            active[*c] += 1;
        }
    }
    let mut out = serde_json::Map::new();
    for m in METRICS.iter() {
        let mut per_arm = Vec::new();
        for a in 1..d.spec.arms.len() {
            let mut rows: Vec<Value> = cells
                .iter()
                .enumerate()
                .map(|(ci, (name, _))| {
                    let mut c = compare(d, &|u| of[u] == ci, a, m);
                    c["cell"] = json!(name);
                    c["users"] = json!(active[ci]);
                    c
                })
                .collect();
            let ps: Vec<f64> = rows.iter().map(|r| r["p"].as_f64().unwrap_or(f64::NAN)).collect();
            for (r, q) in rows.iter_mut().zip(bh(&ps)) {
                r["q"] = json!(q);
            }
            // Whether the cells' differences differ: the interaction test,
            // over the cells with users enough in both arms.
            let ds: Vec<(f64, f64)> = rows
                .iter()
                .filter(|r| r["control"]["users"].as_u64().unwrap_or(0) >= 30 && r["treatment"]["users"].as_u64().unwrap_or(0) >= 30)
                .map(|r| (r["delta"].as_f64().unwrap_or(f64::NAN), r["se"].as_f64().unwrap_or(f64::NAN)))
                .collect();
            let interaction = cochran_q(&ds).map_or(Value::Null, |(q, df, p)| json!({"q": q, "df": df, "p": p, "cells": df as usize + 1}));
            per_arm.push(json!({"arm": d.spec.arms[a].name, "cells": rows, "interaction": interaction}));
        }
        out.insert(m.key.to_string(), json!(per_arm));
    }
    json!({"by": by, "metrics": out})
}

/// The report of a run: every metric, every comparison, the cohorts by
/// each attribute, the latencies and what the exploring share's log
/// estimates.
/// Per query of a run, how each arm did: measured over the searches that
/// asked it, and exactly over the population, which says where the
/// difference between the arms comes from.
#[derive(Clone, Debug, Default)]
pub struct ByQuery {
    pub arms: usize,
    /// Per query and arm (`q * arms + a`): searches, successes, clicks,
    /// abandoned searches.
    pub measured: Vec<[f64; 4]>,
    /// Per query and arm: the exact success and clicks of one search.
    pub truth: Vec<[f64; 2]>,
    /// Per query and arm: nDCG of the rows the arm shows.
    pub ndcg: Vec<f64>,
    pub text: Vec<String>,
    /// Per query: the rows the judges grade relevant.
    pub relevant: Vec<usize>,
    /// Per query: its share of the searches.
    pub share: Vec<f64>,
}

impl ByQuery {
    /// The site's queries under `lists` (per arm, per query, the keys
    /// shown), with their exact values over `users` (each searching by its
    /// `share`), and nothing measured yet.
    pub fn new(site: &Site, worlds: &[Arc<World>], users: &[User], share: &[f64], lists: &[Vec<Vec<String>>]) -> ByQuery {
        let k = worlds.len();
        let nq = site.queries.len();
        let limit = worlds.first().map_or(10, |w| w.limit);
        let parts = par(users.len(), |r| {
            let mut acc = vec![[0.0f64; 2]; nq * k];
            for u in r {
                for (a, w) in worlds.iter().enumerate() {
                    for (q, list) in lists[a].iter().enumerate() {
                        let slots: Vec<Slot> = list.iter().take(w.limit).map(|key| w.slot(&users[u], q, key)).collect();
                        let e = w.model.expected(&slots, &users[u].behaviour);
                        let x = &mut acc[q * k + a];
                        x[0] += share[u] * e.any_satisfied;
                        x[1] += share[u] * e.click.iter().sum::<f64>();
                    }
                }
            }
            acc
        });
        let mut truth = vec![[0.0f64; 2]; nq * k];
        for p in parts {
            for (t, x) in truth.iter_mut().zip(p) {
                t[0] += x[0];
                t[1] += x[1];
            }
        }
        let total: f64 = share.iter().sum::<f64>().max(1e-12);
        for t in &mut truth {
            t[0] /= total;
            t[1] /= total;
        }
        let ndcg = (0..nq).flat_map(|q| (0..k).map(move |a| (q, a))).map(|(q, a)| crate::live::ndcg(&site.grades[q], site.max_grade, &lists[a][q], limit)).collect();
        let shares = site.shares();
        ByQuery {
            arms: k,
            measured: vec![[0.0; 4]; nq * k],
            truth,
            ndcg,
            text: site.queries.iter().map(|q| q.text.clone()).collect(),
            relevant: site.grades.iter().map(|g| g.values().filter(|&&x| x > 0).count()).collect(),
            share: shares,
        }
    }

    /// Search `imp`, of arm `a`, counted.
    pub fn add(&mut self, imp: &Impression, a: usize) {
        add_query(&mut self.measured, self.arms, imp, a);
    }
}

/// Search `imp` of arm `a` counted in `m` (per query and arm, of `k` arms).
pub fn add_query(m: &mut [[f64; 4]], k: usize, imp: &Impression, a: usize) {
    let clicks = imp.click.iter().filter(|&&c| c).count() as f64;
    let x = &mut m[imp.query as usize * k + a];
    x[0] += 1.0;
    x[1] += imp.satisfied.iter().any(|&s| s) as u8 as f64;
    x[2] += clicks;
    x[3] += (clicks == 0.0) as u8 as f64;
}

/// The bands of a query's relevant rows the per-query table groups by.
const RELEVANT_BANDS: [(usize, usize, &str); 5] = [(0, 1, "1"), (2, 3, "2 or 3"), (4, 10, "4 to 10"), (11, 30, "11 to 30"), (31, usize::MAX, "31 or more")];

/// Which queries each arm won and lost most on, by the exact difference
/// times the query's share (what it adds to the whole difference), and the
/// queries grouped by how many rows are relevant to them: where an arm's
/// difference comes from.
pub fn queries_report(d: &RunData) -> Value {
    let Some(b) = &d.by_query else { return Value::Null };
    let k = b.arms;
    let nq = b.text.len();
    let lift = |t0: f64, t1: f64| if t0 > 0.0 { json!(t1 / t0 - 1.0) } else { Value::Null };
    let rate = |x: &[f64; 4]| if x[0] > 0.0 { json!(x[1] / x[0]) } else { Value::Null };
    let mut arms = Vec::new();
    for a in 1..k {
        // Each query: what its exact difference adds to the whole one.
        let mut order: Vec<usize> = (0..nq).collect();
        let adds = |q: usize| b.share[q] * (b.truth[q * k + a][0] - b.truth[q * k][0]);
        order.sort_by(|&x, &y| adds(x).total_cmp(&adds(y)));
        let row = |q: usize| {
            json!({
                "q": q, "text": b.text[q], "share": b.share[q], "relevant": b.relevant[q], "adds": adds(q),
                "control": {"searches": b.measured[q * k][0], "success": rate(&b.measured[q * k]), "truth": b.truth[q * k][0], "ndcg": b.ndcg[q * k]},
                "arm": {"searches": b.measured[q * k + a][0], "success": rate(&b.measured[q * k + a]), "truth": b.truth[q * k + a][0], "ndcg": b.ndcg[q * k + a]},
                "true_lift": lift(b.truth[q * k][0], b.truth[q * k + a][0]),
            })
        };
        let losses: Vec<Value> = order.iter().take(25).filter(|&&q| adds(q) < 0.0).map(|&q| row(q)).collect();
        let gains: Vec<Value> = order.iter().rev().take(10).filter(|&&q| adds(q) > 0.0).map(|&q| row(q)).collect();
        let total: f64 = (0..nq).map(adds).sum();
        // By how many rows are relevant.
        let mut bands = Vec::new();
        for (lo, hi, label) in RELEVANT_BANDS {
            let qs: Vec<usize> = (0..nq).filter(|&q| b.relevant[q] >= lo.max(1) && b.relevant[q] <= hi).collect();
            if qs.is_empty() {
                continue;
            }
            let w: f64 = qs.iter().map(|&q| b.share[q]).sum();
            let t = |arm: usize| qs.iter().map(|&q| b.share[q] * b.truth[q * k + arm][0]).sum::<f64>() / w.max(1e-12);
            let m = |arm: usize| qs.iter().fold([0.0; 4], |mut acc, &q| {
                for (x, y) in acc.iter_mut().zip(b.measured[q * k + arm]) {
                    *x += y;
                }
                acc
            });
            let (m0, m1) = (m(0), m(a));
            let (t0, t1) = (t(0), t(a));
            bands.push(json!({
                "label": label, "queries": qs.len(), "share": w,
                "control": {"searches": m0[0], "success": rate(&m0), "truth": t0},
                "arm": {"searches": m1[0], "success": rate(&m1), "truth": t1},
                "lift": match (rate(&m0).as_f64(), rate(&m1).as_f64()) { (Some(x), Some(y)) if x > 0.0 => json!(y / x - 1.0), _ => Value::Null },
                "true_lift": lift(t0, t1), "adds": w * (t1 - t0),
            }));
        }
        let finding = band_finding(&d.spec.arms[a].name, &bands);
        arms.push(json!({"arm": d.spec.arms[a].name, "losses": losses, "gains": gains, "bands": bands, "total": total, "finding": finding}));
    }
    json!({"queries": nq, "metric": "success", "arms": arms})
}

/// One sentence on the bands of relevant rows where an arm moved most and
/// least, by the truth; none when the bands are too few or move alike.
fn band_finding(arm: &str, bands: &[Value]) -> Option<String> {
    let lifts: Vec<(f64, &str, u64)> = bands.iter().filter_map(|b| Some((b["true_lift"].as_f64()?, b["label"].as_str()?, b["queries"].as_u64()?))).filter(|x| x.2 >= 3).collect();
    if lifts.len() < 2 {
        return None;
    }
    let lo = lifts.iter().min_by(|x, y| x.0.total_cmp(&y.0))?;
    let hi = lifts.iter().max_by(|x, y| x.0.total_cmp(&y.0))?;
    if (hi.0 - lo.0).abs() < 0.05 {
        return None;
    }
    let pct = |x: f64| format!("{}{:.0}%", if x < 0.0 { "−" } else { "+" }, (x * 100.0).abs());
    let rows = |label: &str| if label == "1" { "1 relevant row".to_string() } else { format!("{label} relevant rows") };
    Some(if lo.0 < 0.0 && hi.0 <= 0.0 {
        format!("{arm} loses the most on queries with {} ({} success) and the least on those with {} ({}).", rows(lo.1), pct(lo.0), rows(hi.1), pct(hi.0))
    } else if lo.0 >= 0.0 {
        format!("{arm} gains the most on queries with {} ({} success) and the least on those with {} ({}).", rows(hi.1), pct(hi.0), rows(lo.1), pct(lo.0))
    } else {
        format!("{arm} gains on queries with {} ({} success) and loses on those with {} ({}).", rows(hi.1), pct(hi.0), rows(lo.1), pct(lo.0))
    })
}

pub fn report(d: &RunData) -> Value {
    let k = d.spec.arms.len();
    let mut metrics = Vec::new();
    for (mi, m) in METRICS.iter().enumerate() {
        let mut comps = Vec::new();
        for a in 1..k {
            let mut c = compare(d, &|_| true, a, m);
            // CUPED, when the pre-period has the metric.
            let pairs = |arm: usize| -> Vec<(&Sums, &Sums)> { (0..d.users.len()).filter(|&u| d.arm[u] == Some(arm) && d.sums[u][N] > 0.0).map(|u| (&d.sums[u], &d.pre[u])).collect() };
            if d.spec.pre > 0.0 {
                if let Some((delta, se, reduction)) = cuped(&pairs(0), &pairs(a), m.num, m.den) {
                    c["cuped"] = json!({"delta": delta, "se": se, "lo": delta - Z975 * se, "hi": delta + Z975 * se, "p": p_two(delta / se), "variance_reduction": reduction, "mde": (Z975 + Z80) * se});
                }
            }
            let base = c["control"]["value"].as_f64().unwrap_or(1.0).abs().max(1e-9);
            c["series"] = series(d, a, mi, 0.05 * base);
            // The always-valid p value and interval, which a run that
            // stopped once decided must be read by.
            let (delta, se, c0) = (c["delta"].as_f64().unwrap_or(f64::NAN), c["se"].as_f64().unwrap_or(f64::NAN), c["control"]["value"].as_f64().unwrap_or(f64::NAN));
            let half = always_valid_half(se, 0.05 * base);
            let lift = c["lift"].as_f64().unwrap_or(f64::NAN);
            let num = |x: f64| if x.is_finite() { json!(x) } else { Value::Null };
            c["p_always_valid"] = c["series"].as_array().and_then(|s| s.last()).map_or(Value::Null, |p| p["p_always_valid"].clone());
            c["av_lo"] = num(delta - half);
            c["av_hi"] = num(delta + half);
            c["lift_av_lo"] = num(lift - half / c0.abs());
            c["lift_av_hi"] = num(lift + half / c0.abs());
            comps.push(c);
        }
        let arms: Vec<Value> = (0..k)
            .map(|a| {
                let g: Vec<&Sums> = (0..d.users.len()).filter(|&u| d.arm[u] == Some(a) && d.sums[u][N] > 0.0).map(|u| &d.sums[u]).collect();
                let s = ratio(&g, m.num, m.den);
                json!({"arm": d.spec.arms[a].name, "value": s.value, "se": s.se, "users": s.users, "truth": truth_of(d, &|_| true, a, m)})
            })
            .collect();
        metrics.push(json!({"key": m.key, "title": m.title, "unit": m.unit, "higher": m.higher, "guardrail": m.guardrail, "margin": m.margin, "diagnostic": m.diagnostic, "exact": m.exact, "about": m.about, "arms": arms, "comparisons": comps, "histogram": histogram(d, m)}));
    }
    let names: Vec<String> = d.spec.arms.iter().map(|a| a.name.clone()).collect();
    metrics.push(latency_p95(&names, &d.latencies, None));
    metrics.push(latency_p99(&names, &d.latencies));
    q_values(&mut metrics, k);
    let mut cohorts = serde_json::Map::new();
    for attr in ATTRIBUTES {
        cohorts.insert(attr.to_string(), cohort_table(d, &[attr]));
    }
    let latency: Vec<Value> = d.latencies.iter().enumerate().map(|(a, l)| { let mut v = l.clone(); json!({"arm": d.spec.arms[a].name, "p50": percentile(&mut v, 0.5), "p95": percentile(&mut v, 0.95), "p99": percentile(&mut v, 0.99), "n": l.len()}) }).collect();
    let verdicts: Vec<Value> = (1..k).map(|a| verdict(d, &metrics, a)).collect();
    json!({
        "site": d.site,
        "arms": d.spec.arms.iter().enumerate().map(|(a, arm)| json!({"name": arm.name, "rank": arm.rank, "share": arm.share, "users": d.arm.iter().enumerate().filter(|(u, x)| **x == Some(a) && d.sums[*u][N] > 0.0).count(), "searches": d.arm.iter().zip(&d.sums).filter(|(x, _)| **x == Some(a)).map(|(_, s)| s[N]).sum::<f64>()})).collect::<Vec<_>>(),
        "users": d.users.len(),
        "searches": d.spec.impressions,
        "pre_searches": (d.spec.impressions as f64 * d.spec.pre).round(),
        "seconds": d.seconds,
        "srm": srm(d),
        "counts": counts(d),
        "metrics": metrics,
        "verdicts": verdicts,
        "cohorts": cohorts,
        "latency": latency,
        "whatif": d.whatif,
        "attributes": ATTRIBUTES,
        "by_query": queries_report(d),
    })
}

/// Benjamini-Hochberg q values across the metrics that decide (the
/// primary candidates and the guardrails), per arm; a diagnostic's q is
/// none, since it is not one of the questions asked.
pub fn q_values(metrics: &mut [Value], k: usize) {
    for a in 1..k {
        let at: Vec<usize> = (0..metrics.len()).filter(|&i| metrics[i]["diagnostic"] != json!(true)).collect();
        let ps: Vec<f64> = at.iter().map(|&i| metrics[i]["comparisons"][a - 1]["p"].as_f64().unwrap_or(f64::NAN)).collect();
        for (&i, q) in at.iter().zip(bh(&ps)) {
            metrics[i]["comparisons"][a - 1]["q"] = if q.is_finite() { json!(q) } else { Value::Null };
        }
    }
}

/// What an arm's result says: its primary metric's move, and any
/// guardrail it made worse. With users split unlike the plan, nothing
/// else is read: the call is that the test is invalid.
fn verdict(d: &RunData, metrics: &[Value], a: usize) -> Value {
    let primary = metrics.iter().find(|m| m["key"] == json!(d.spec.primary)).unwrap_or(&metrics[0]);
    let c = &primary["comparisons"][a - 1];
    let higher = primary["higher"].as_bool().unwrap_or(true);
    let p = c["p"].as_f64().unwrap_or(1.0);
    let delta = c["delta"].as_f64().unwrap_or(0.0);
    let better = (delta > 0.0) == higher;
    let worse_guards: Vec<String> = metrics
        .iter()
        .filter(|m| m["guardrail"].as_bool().unwrap_or(false))
        .filter(|m| {
            let c = &m["comparisons"][a - 1];
            let lift = c["lift"].as_f64().unwrap_or(0.0);
            let worse = (lift > 0.0) != m["higher"].as_bool().unwrap_or(true);
            worse && lift.abs() > m["margin"].as_f64().unwrap_or(0.0) && c["p"].as_f64().unwrap_or(1.0) < 0.05
        })
        .map(|m| m["title"].as_str().unwrap_or("").to_string())
        .collect();
    let srm_ok = srm(d)["ok"] != json!(false);
    let call = if !srm_ok {
        "invalid: users split unlike the plan"
    } else if p >= 0.05 {
        "no detectable difference"
    } else if better && worse_guards.is_empty() {
        "ship"
    } else if better {
        "better, but a guardrail got worse"
    } else {
        "worse"
    };
    let truth_call = c["true_delta"].as_f64().map(|t| if t.abs() < 1e-12 { "no true difference" } else if (t > 0.0) == higher { "truly better" } else { "truly worse" });
    json!({"arm": d.spec.arms[a].name, "metric": primary["key"], "call": call, "p": p, "lift": c["lift"], "lift_lo": c["lift_lo"], "lift_hi": c["lift_hi"], "true_lift": c["true_lift"], "truth_call": truth_call, "guardrails_worse": worse_guards, "power": c["power"], "users_needed": c["users_needed"]})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delta_method_matches_a_hand_computation() {
        // Users with (clicks, searches): (1, 2), (3, 4), (0, 1), (2, 2).
        let xs: Vec<Sums> = [(1.0, 2.0), (3.0, 4.0), (0.0, 1.0), (2.0, 2.0)].iter().map(|&(c, n)| { let mut s = [0.0; SUMS]; s[CLICKS] = c; s[N] = n; s }).collect();
        let refs: Vec<&Sums> = xs.iter().collect();
        let r = ratio(&refs, CLICKS, N);
        assert!((r.value - 6.0 / 9.0).abs() < 1e-12);
        // var(R) = (s_aa - 2R s_ab + R^2 s_bb) / (n mb^2), sample (co)variances.
        let (ma, mb) = (1.5, 2.25);
        let a = [1.0 - ma, 3.0 - ma, 0.0 - ma, 2.0 - ma];
        let b = [2.0 - mb, 4.0 - mb, 1.0 - mb, 2.0 - mb];
        let s = |x: &[f64], y: &[f64]| x.iter().zip(y).map(|(p, q)| p * q).sum::<f64>() / 3.0;
        let rr = 6.0 / 9.0;
        let var = (s(&a, &a) - 2.0 * rr * s(&a, &b) + rr * rr * s(&b, &b)) / (4.0 * mb * mb);
        assert!((r.se - var.sqrt()).abs() < 1e-12);
    }

    #[test]
    fn bh_q_values() {
        let q = bh(&[0.01, 0.04, 0.03, 0.2]);
        // Sorted: 0.01 (1), 0.03 (2), 0.04 (3), 0.2 (4); m = 4.
        assert!((q[0] - 0.04).abs() < 1e-12);
        // 0.03 × 4/2 = 0.06 is above the next one's 0.04 × 4/3: both 0.0533.
        assert!((q[2] - 0.16 / 3.0).abs() < 1e-12);
        assert!((q[1] - 0.16 / 3.0).abs() < 1e-12);
        assert!((q[3] - 0.2).abs() < 1e-12);
        assert!(bh(&[f64::NAN, 0.5])[0].is_nan());
    }

    #[test]
    fn the_always_valid_interval_ends_where_its_p_value_is_0_05() {
        let (se, tau) = (0.02, 0.01);
        let half = always_valid_half(se, tau);
        // A difference at the interval's edge has an always-valid p of 0.05.
        let p = always_valid(&[(half, se)], tau)[0];
        assert!((p - 0.05).abs() < 1e-9, "p {p}");
        // And it is wider than the fixed interval.
        assert!(half > Z975 * se);
    }

    #[test]
    fn cochran_q_finds_cells_that_differ() {
        // Alike: Q small. Unlike: Q large, p tiny.
        let (q, df, p) = cochran_q(&[(0.01, 0.01), (0.012, 0.01), (0.009, 0.01)]).unwrap();
        assert_eq!(df, 2.0);
        assert!(q < 0.1 && p > 0.9);
        let (_, _, p) = cochran_q(&[(0.05, 0.01), (-0.05, 0.01)]).unwrap();
        assert!(p < 1e-6);
        // Cells with no error are left out; one cell is no test.
        assert!(cochran_q(&[(0.05, 0.01), (0.0, 0.0)]).is_none());
    }

    #[test]
    fn latency_p95_reads_the_tail() {
        let fast: Vec<f64> = (0..1000).map(|i| 1.0 + (i % 100) as f64 * 0.01).collect();
        // The same times, but one in ten three times slower: the mean moves a little, the tail a lot.
        let slow: Vec<f64> = fast.iter().enumerate().map(|(i, t)| if i % 10 == 0 { t * 3.0 } else { *t }).collect();
        let m = latency_p95(&["A".into(), "B".into()], &[fast, slow], None);
        let c = &m["comparisons"][0];
        assert!(c["lift"].as_f64().unwrap() > 0.5, "{c}");
        assert!(c["p"].as_f64().unwrap() < 0.001);
        assert_eq!(m["guardrail"], json!(true));
    }

    #[test]
    fn always_valid_p_only_falls_and_is_above_the_fixed_one() {
        let series: Vec<(f64, f64)> = (1..=20).map(|i| (0.01, 0.05 / (i as f64).sqrt())).collect();
        let p = always_valid(&series, 0.02);
        assert!(p.windows(2).all(|w| w[1] <= w[0]));
        for (i, &(d, se)) in series.iter().enumerate() {
            assert!(p[i] >= p_two(d / se) * 0.999, "always valid p {} below the fixed {}", p[i], p_two(d / se));
        }
    }

    #[test]
    fn users_split_as_planned() {
        let spec = Spec { name: "x".into(), site: "s".into(), arms: vec![Arm { name: "A".into(), rank: "r".into(), share: 1.0, then: String::new() }, Arm { name: "B".into(), rank: "r".into(), share: 3.0, then: String::new() }], model: dbn(), users: users(), impressions: impressions(), explore_share: 0.2, explore: explore(), limit: 5, seed: 3, salt: 0, taste: taste(), appetite: appetite(), mode: fast(), primary: primary(), pre: pre() };
        let mut n = [0usize; 3];
        for u in 0..40_000 {
            match assign(&spec, u) {
                None => n[2] += 1,
                Some(a) => n[a] += 1,
            }
        }
        let f = |c: usize| c as f64 / 40_000.0;
        assert!((f(n[2]) - 0.2).abs() < 0.01, "{n:?}");
        assert!((f(n[0]) - 0.2).abs() < 0.01 && (f(n[1]) - 0.6).abs() < 0.01, "{n:?}");
    }

    /// The toy world's A/B: the oracle against the logger, its truth and
    /// its measured difference, and every interval's coverage of the truth
    /// over many seeds.
    #[test]
    fn an_ab_test_finds_the_true_effect_and_its_intervals_hold() {
        let w = crate::sim::tests::toy(Model::Dbn, Policy::None);
        let site = w.site.clone();
        let oracle: Vec<Ranked> = w
            .ranked
            .iter()
            .enumerate()
            .map(|(q, r)| {
                let mut idx: Vec<usize> = (0..r.keys.len()).collect();
                idx.sort_by(|&a, &b| site.slot(q, &r.keys[b]).attract.total_cmp(&site.slot(q, &r.keys[a]).attract));
                Ranked { keys: idx.iter().map(|&i| r.keys[i].clone()).collect(), scores: idx.iter().map(|&i| r.scores[i]).collect() }
            })
            .collect();
        let mut covered = 0;
        let reps = 40;
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let server = Server::new("http://127.0.0.1:1", None);
        let mut significant = 0;
        for rep in 0..reps {
            let spec = Spec { name: "t".into(), site: site.name.clone(), arms: vec![Arm { name: "A".into(), rank: w.rank.clone(), share: 1.0, then: String::new() }, Arm { name: "B".into(), rank: "oracle".into(), share: 1.0, then: String::new() }], model: "dbn".into(), users: 2000, impressions: 40_000, explore_share: 0.0, explore: explore(), limit: w.limit, seed: rep, salt: 0, taste: 0.35, appetite: appetite(), mode: fast(), primary: primary(), pre: 0.5 };
            let ranks = Ranks { arms: vec![w.ranked.clone(), oracle.clone()], took: vec![vec![1.0; site.queries.len()], vec![2.0; site.queries.len()]], explore: Vec::new() };
            let d = rt.block_on(run(&server, &spec, &format!("r{rep}"), site.clone(), ranks, Arc::new(Progress::default()))).unwrap();
            let r = report(&d);
            let m = r["metrics"].as_array().unwrap().iter().find(|m| m["key"] == "clicks").unwrap();
            let c = &m["comparisons"][0];
            if c["covers"].as_bool().unwrap() {
                covered += 1;
            }
            if c["p"].as_f64().unwrap() < 0.05 {
                significant += 1;
            }
            assert!(c["true_delta"].as_f64().unwrap() > 0.0, "the oracle is truly better");
            assert_eq!(r["srm"]["ok"], json!(true));
            if rep == 0 {
                let cu = &c["cuped"];
                assert!(cu["variance_reduction"].as_f64().unwrap() > 0.0, "CUPED removes variance: {cu}");
                assert!(r["cohorts"]["device"]["metrics"]["clicks"][0]["cells"].as_array().unwrap().len() == 3);
            }
        }
        // 95% intervals over 40 independent experiments: at least 33 by chance.
        assert!(covered >= 33, "{covered} of {reps} intervals held the true effect");
        assert!(significant >= 30, "the oracle's lift found in only {significant} of {reps}");
    }
}
