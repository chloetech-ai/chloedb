//! Blends valued off-policy: how another blend would have done, judged on
//! the searches a blend served while it explored.
//!
//! A logged blend ([`crate::query::blend::Blended::log_entries`]) keeps
//! every source's rows (the cells a blend reads), the page it showed and,
//! slot by slot, the chance its exploring fill had of placing each row
//! there given the rows above it. From that log:
//!
//! * **The replay.** Each request's rows are a blend's inputs again, so
//!   any blend of the same sources is replayed over the page shown
//!   ([`crate::query::blend::replay`]): its chance of each logged row,
//!   given the logged rows above it, and the rows it would have placed
//!   instead. Replaying the logging blend gives back its own chances.
//! * **Cascade doubly robust** ([`Estimate`], Kiyohara et al., WSDM 2022).
//!   People read down the page, so slot l's reward is taken to depend on
//!   the rows down to l only; its weight `w_1..l` is the product of the
//!   ratios (the blend's chance ÷ the log's) down to l, small and steady
//!   at the top. A click model (`q̂`, fitted on the other half of the
//!   requests) predicts each slot's reward, and `Q̂` is its value of the
//!   rest of the blend's page from a slot down. Per request:
//!
//!   `Σ_l w_1..l (γ_l r_l − Q̂(a_1..l)) + w_1..l−1 E_π Q̂(a_1..l−1, ·)`
//!
//!   Where a search follows the blend's page its clicks correct the
//!   model; where it leaves it, the model's value of the rest stands in.
//!   Unbiased when the logged chances are right whatever the model, and
//!   less variable the better the model is.
//! * **SNIPS beside it.** The same weights with no model, each slot's
//!   weighted rewards divided by the sum of its weights, over the top
//!   slots where each has [`CHECK_ESS`] effective searches. When the two
//!   disagree there beyond their intervals the model or the log's
//!   coverage is off, and the verdict says not proven.
//! * **Support and coverage.** The top slot's effective number of
//!   searches, `(Σw)² / Σw²`, trusted at [`TRUSTED`] or more; and the
//!   share of the blend's pages the exploring blend could have drawn at
//!   all (the logging blend replayed over each page), at least
//!   [`MIN_COVERAGE`]: a page it could not draw is never seen.
//! * **Tuning** ([`tune`]): blends near a start, their head weights,
//!   quota shares and diversity varied, valued on half the requests (its
//!   own click model fitted there), the best then valued on the other
//!   half, which chose nothing.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde_json::{json, Map, Value as Json};

use crate::error::{Error, Result};
use crate::query::blend::{self, Def, Explore, Input};
use crate::query::exec::ResultSet;
use crate::value::Value;

fn err(m: impl Into<String>) -> Error {
    Error::Query(m.into())
}

/// Effective searches at which an estimate is trusted.
pub const TRUSTED: f64 = 400.0;

/// The least share of a blend's pages the log must cover: a page the
/// exploring blend could not have drawn is never seen, and a value that
/// leaves it out is short of the truth.
pub const MIN_COVERAGE: f64 = 0.95;

/// The most one search counts for, as a share of the searches valued
/// (and at least [`MOST_WEIGHT`]): a page whose weight grows past it is
/// valued by the model from there (SWITCH, Wang et al., ICML 2017), so
/// that deep slots, which few searches follow, do not rest on one.
pub const MOST_SHARE: f64 = 0.01;

/// The least of the most one search counts for.
pub const MOST_WEIGHT: f64 = 10.0;

/// The ridge of the click model's least squares.
const RIDGE: f64 = 1.0;

/// The most numeric cells the click model reads.
const MAX_CELLS: usize = 16;

/// The value of a cell as a logged blend writes it
/// ([`crate::query::blend::cell_json`]).
pub fn cell_value(j: &Json) -> Value {
    match j {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => match n.as_i64() {
            Some(i) if !n.is_f64() => Value::Int(i),
            _ => Value::Float(n.as_f64().unwrap_or(f64::NAN)),
        },
        Json::String(s) => Value::Str(s.clone()),
        Json::Array(xs) => Value::Vector(xs.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect()),
        Json::Object(m) => match (m.get("ts"), m.get("list")) {
            (Some(t), _) => t.as_i64().map_or(Value::Null, Value::Timestamp),
            (_, Some(Json::Array(xs))) => Value::List(xs.iter().map(cell_value).collect()),
            _ => Value::Null,
        },
    }
}

/// A logged row of a blend's request, as the export gives it.
#[derive(Clone, Debug)]
struct Rec {
    position: i64,
    shown: bool,
    propensity: Option<f64>,
    label: f64,
    key: String,
    source: String,
    rank: usize,
    score: Option<f64>,
    cells: Map<String, Json>,
}

/// The rows shown at one slot of a logged request.
#[derive(Clone, Debug)]
pub struct Shown {
    pub key: String,
    /// Into [`Request::sources`].
    pub source: usize,
    /// From 1, in its source.
    pub rank: usize,
    /// The logging fill's chance of placing it here, given the rows above.
    pub chance: f64,
    pub reward: f64,
}

/// A logged request of a blend: every source's rows again, and the page
/// shown.
#[derive(Clone, Debug)]
pub struct Request {
    pub id: String,
    /// The logging blend's version.
    pub version: i64,
    pub sources: Vec<String>,
    /// Each source's rows in their order, as the source answered them.
    pub inputs: Vec<Input>,
    pub shown: Vec<Shown>,
    /// The logging fill's exploration.
    pub explore: Option<Explore>,
    /// Which half of the requests it is in, and which quarter.
    pub fold: usize,
    /// The click model's reading of each source's rows, by rank.
    feats: Vec<Vec<Vec<f64>>>,
    /// The logging blend replayed over the page: per slot, the chance it
    /// had of each row there (by key); `None` when its definition is not
    /// known.
    logging: Option<Vec<HashMap<String, f64>>>,
    /// The most a replayed chance differs from the one logged.
    drift: f64,
}

impl Request {
    /// The value its page got: the rewards of its top `k`, discounted
    /// when `discount`.
    pub fn value(&self, k: usize, discount: bool) -> f64 {
        self.shown.iter().take(k).enumerate().map(|(l, s)| gamma(l + 1, discount) * s.reward).sum()
    }
}

fn num(x: f64) -> String {
    if x.fract() == 0.0 && x.abs() < 1e15 { format!("{}", x as i64) } else { format!("{x}") }
}

fn gamma(slot: usize, discount: bool) -> f64 {
    if discount { 1.0 / ((slot + 1) as f64).log2() } else { 1.0 }
}

/// FNV-1a of `s`.
fn hash(s: &str) -> u64 {
    blend::seed(s)
}

/// A rank log's records of one blend, gathered by request ([`Logs::add`]
/// takes the export's records, [`Logs::finish`] makes the requests).
#[derive(Default)]
pub struct Logs {
    rows: HashMap<String, Vec<Rec>>,
    policy: HashMap<String, Json>,
}

/// What a log held of a blend's requests ([`Logs::finish`]).
#[derive(Clone, Debug, Default)]
pub struct Gathered {
    pub requests: Vec<Request>,
    /// Requests of the blend that did not explore: their pages say nothing
    /// of the pages it did not show.
    pub not_exploring: usize,
    /// Requests of other blends or other queries.
    pub other: usize,
    /// Requests whose rows did not read back.
    pub broken: usize,
}

impl Logs {
    /// Takes one record of a rank log's export ([`crate::rank_log::export`]).
    pub fn add(&mut self, r: &Json) {
        let Some(request) = r["request"].as_str() else { return };
        if r.get("policy").is_some_and(|p| !p.is_null()) {
            self.policy.insert(request.to_string(), r["policy"].clone());
        }
        let f = &r["features"];
        let (Some(source), Some(rank)) = (f["source"].as_str(), f["rank"].as_u64()) else {
            // A row of a query that is not a blend: its request is counted.
            self.rows.entry(request.to_string()).or_default();
            return;
        };
        let rec = Rec {
            position: r["position"].as_i64().unwrap_or(0),
            shown: r["shown"] == Json::Bool(true),
            propensity: r["propensity"].as_f64(),
            label: r["label"].as_f64().filter(|x| x.is_finite()).unwrap_or(0.0),
            key: r["row_key"].as_str().unwrap_or_default().to_string(),
            source: source.to_string(),
            rank: rank as usize,
            score: f["score"].as_f64(),
            cells: f["cells"].as_object().cloned().unwrap_or_default(),
        };
        self.rows.entry(request.to_string()).or_default().push(rec);
    }

    /// The requests of blend `name`, oldest version first in no order
    /// that matters.
    pub fn finish(self, name: &str) -> Gathered {
        let mut g = Gathered::default();
        let mut ids: Vec<&String> = self.rows.keys().collect();
        ids.sort();
        for id in ids {
            let rows = &self.rows[id];
            let p = self.policy.get(id);
            let of_blend = p.is_some_and(|p| p["policy"] == "blend" && p["blend"].as_str().is_some_and(|b| b.eq_ignore_ascii_case(name)));
            if !of_blend {
                g.other += 1;
                continue;
            }
            let p = p.expect("checked above");
            let explore = match (p["tau"].as_f64(), p["top"].as_u64()) {
                (Some(tau), Some(top)) if tau > 0.0 && top > 1 => Some(Explore { tau, top: top as usize }),
                _ => None,
            };
            if explore.is_none() {
                g.not_exploring += 1;
                continue;
            }
            match request(id, rows, p, explore) {
                Some(r) => g.requests.push(r),
                None => g.broken += 1,
            }
        }
        g
    }
}

/// A request from its records, or `None` when they do not make one.
fn request(id: &str, rows: &[Rec], policy: &Json, explore: Option<Explore>) -> Option<Request> {
    let sources: Vec<String> = match policy["sources"].as_array() {
        Some(s) => s.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
        None => rows.iter().map(|r| r.source.clone()).collect::<BTreeSet<_>>().into_iter().collect(),
    };
    let mut inputs = Vec::with_capacity(sources.len());
    for s in &sources {
        let mut mine: Vec<&Rec> = rows.iter().filter(|r| &r.source == s).collect();
        mine.sort_by_key(|r| r.rank);
        if mine.iter().enumerate().any(|(i, r)| r.rank != i + 1) {
            return None;
        }
        let columns: Vec<String> = mine.iter().flat_map(|r| r.cells.keys().cloned()).collect::<BTreeSet<_>>().into_iter().collect();
        let rows: Vec<Vec<Value>> = mine.iter().map(|r| columns.iter().map(|c| r.cells.get(c).map_or(Value::Null, cell_value)).collect()).collect();
        let scored = mine.iter().all(|r| r.score.is_some());
        let rs = ResultSet {
            columns,
            rows,
            keys: mine.iter().map(|r| Value::Str(r.key.clone())).collect(),
            scores: if scored { mine.iter().map(|r| r.score.unwrap_or(0.0)).collect() } else { Vec::new() },
            parts: None,
            stats: Default::default(),
        };
        inputs.push(Input { rs, took_ms: 0.0 });
    }
    let mut shown: Vec<&Rec> = rows.iter().filter(|r| r.shown).collect();
    shown.sort_by_key(|r| r.position);
    let shown: Vec<Shown> = shown
        .iter()
        .map(|r| Some(Shown { key: r.key.clone(), source: sources.iter().position(|s| *s == r.source)?, rank: r.rank, chance: r.propensity.filter(|p| *p > 0.0)?, reward: r.label }))
        .collect::<Option<_>>()?;
    if shown.is_empty() {
        return None;
    }
    let h = hash(id);
    Some(Request { id: id.to_string(), version: policy["version"].as_i64().unwrap_or(0), sources, inputs, shown, explore, fold: (h & 3) as usize, feats: Vec::new(), logging: None, drift: 0.0 })
}

/// What the click model reads of a row: its source, its rank, its
/// source's score and the numeric cells, each scaled to its source's
/// rows in the request.
#[derive(Clone, Debug)]
struct Reading {
    sources: Vec<String>,
    cells: Vec<String>,
}

impl Reading {
    fn of(reqs: &[Request]) -> Reading {
        let mut sources: BTreeSet<String> = BTreeSet::new();
        let mut count: BTreeMap<String, usize> = BTreeMap::new();
        for r in reqs {
            sources.extend(r.sources.iter().cloned());
            for inp in &r.inputs {
                for (i, c) in inp.rs.columns.iter().enumerate() {
                    if inp.rs.rows.iter().any(|row| matches!(row[i], Value::Int(_) | Value::Float(_))) {
                        *count.entry(c.clone()).or_default() += 1;
                    }
                }
            }
        }
        let mut cells: Vec<(String, usize)> = count.into_iter().collect();
        cells.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        Reading { sources: sources.into_iter().collect(), cells: cells.into_iter().take(MAX_CELLS).map(|c| c.0).collect() }
    }

    fn width(&self) -> usize {
        1 + self.sources.len() + 2 + 1 + self.cells.len()
    }

    /// Each source's rows read, by rank.
    fn read(&self, r: &Request) -> Vec<Vec<Vec<f64>>> {
        r.inputs
            .iter()
            .enumerate()
            .map(|(s, inp)| {
                let rs = &inp.rs;
                let n = rs.rows.len();
                let scale = |vals: Vec<Option<f64>>| -> Vec<f64> {
                    let present: Vec<f64> = vals.iter().flatten().copied().filter(|x| x.is_finite()).collect();
                    let (lo, hi) = present.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &x| (a.min(x), b.max(x)));
                    vals.iter().map(|v| match v.filter(|x| x.is_finite()) {
                        Some(x) if hi > lo => (x - lo) / (hi - lo),
                        Some(_) => 1.0,
                        None => 0.5,
                    }).collect()
                };
                let score = scale(if rs.scores.len() == n { rs.scores.iter().map(|&x| Some(x)).collect() } else { vec![None; n] });
                let cols: Vec<Vec<f64>> = self
                    .cells
                    .iter()
                    .map(|c| {
                        let at = rs.columns.iter().position(|x| x == c);
                        scale((0..n).map(|i| at.and_then(|a| match rs.rows[i][a] {
                            Value::Int(x) => Some(x as f64),
                            Value::Float(x) => Some(x),
                            _ => None,
                        })).collect())
                    })
                    .collect();
                let src = self.sources.iter().position(|x| *x == r.sources[s]);
                (0..n)
                    .map(|i| {
                        let mut f = Vec::with_capacity(self.width());
                        f.push(1.0);
                        f.extend((0..self.sources.len()).map(|j| if Some(j) == src { 1.0 } else { 0.0 }));
                        f.push(1.0 / (i + 1) as f64);
                        f.push(((i + 2) as f64).ln().recip());
                        f.push(score[i]);
                        f.extend(cols.iter().map(|c| c[i]));
                        f
                    })
                    .collect()
            })
            .collect()
    }
}

/// A click model: each slot's reward predicted from the row's reading,
/// as `x·β` on the slot (one-hot), the reading, and the reading damped by
/// the slot (`1 / log2(slot + 1)`), fitted by ridge least squares.
#[derive(Clone, Debug)]
struct Model {
    k: usize,
    width: usize,
    beta: Vec<f64>,
    most: f64,
}

impl Model {
    fn x(k: usize, f: &[f64], slot: usize) -> Vec<f64> {
        let d = 1.0 / ((slot + 1) as f64).log2();
        let mut x = vec![0.0; k];
        if slot <= k {
            x[slot - 1] = 1.0;
        }
        x.extend_from_slice(f);
        x.extend(f.iter().map(|v| v * d));
        x
    }

    /// Fitted on the pages of `reqs` down to slot `k`.
    fn fit<'a>(reqs: impl Iterator<Item = &'a Request>, k: usize, width: usize) -> Model {
        let n = k + 2 * width;
        let mut a = vec![0.0; n * n];
        let mut b = vec![0.0; n];
        let mut most: f64 = 0.0;
        for r in reqs {
            for (l, s) in r.shown.iter().take(k).enumerate() {
                let x = Model::x(k, &r.feats[s.source][s.rank - 1], l + 1);
                for i in 0..n {
                    if x[i] == 0.0 {
                        continue;
                    }
                    b[i] += x[i] * s.reward;
                    for j in 0..n {
                        a[i * n + j] += x[i] * x[j];
                    }
                }
                most = most.max(s.reward);
            }
        }
        for i in 0..n {
            a[i * n + i] += RIDGE;
        }
        Model { k, width, beta: solve(a, b, n), most }
    }

    fn predict(&self, f: &[f64], slot: usize) -> f64 {
        if f.len() != self.width {
            return 0.0;
        }
        let x = Model::x(self.k, f, slot);
        x.iter().zip(&self.beta).map(|(a, b)| a * b).sum::<f64>().clamp(0.0, self.most.max(0.0))
    }
}

/// Solves `a x = b` (a symmetric and positive definite, n × n) by
/// Gaussian elimination with partial pivoting.
fn solve(mut a: Vec<f64>, mut b: Vec<f64>, n: usize) -> Vec<f64> {
    for c in 0..n {
        let p = (c..n).max_by(|&i, &j| a[i * n + c].abs().total_cmp(&a[j * n + c].abs())).unwrap_or(c);
        if a[p * n + c].abs() < 1e-12 {
            continue;
        }
        if p != c {
            for j in 0..n {
                a.swap(p * n + j, c * n + j);
            }
            b.swap(p, c);
        }
        for i in c + 1..n {
            let f = a[i * n + c] / a[c * n + c];
            if f == 0.0 {
                continue;
            }
            for j in c..n {
                a[i * n + j] -= f * a[c * n + j];
            }
            b[i] -= f * b[c];
        }
    }
    let mut x = vec![0.0; n];
    for c in (0..n).rev() {
        let s: f64 = (c + 1..n).map(|j| a[c * n + j] * x[j]).sum();
        x[c] = if a[c * n + c].abs() < 1e-12 { 0.0 } else { (b[c] - s) / a[c * n + c] };
    }
    x
}

/// Each stored version of blend `name` in `set`, by version: what a log
/// of it is replayed with.
pub fn versions(set: &crate::query::function::FunctionSet, name: &str) -> HashMap<i64, Def> {
    blend::versions(set, name).into_iter().filter_map(|(d, f)| d.map(|d| (f.version, d))).collect()
}

/// A blend to value: a definition, filled as it would be served (the
/// best taken, or exploring).
#[derive(Clone, Debug)]
pub struct Target {
    /// What the answer calls it: `serp@3`, `draft`.
    pub label: String,
    pub def: Def,
    pub explore: Option<Explore>,
}

/// How the requests are valued.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// Rewards discounted by slot (`1 / log2(slot + 1)`), or summed.
    pub discount: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { discount: false }
    }
}

/// A value with its 95% interval.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Interval {
    pub value: f64,
    pub lo: f64,
    pub hi: f64,
    pub se: f64,
}

impl Interval {
    fn of(value: f64, se: f64) -> Interval {
        let se = if se.is_finite() { se } else { f64::INFINITY };
        Interval { value, lo: value - 1.96 * se, hi: value + 1.96 * se, se }
    }

    /// The mean of `xs` and its interval.
    fn mean(xs: &[f64]) -> Interval {
        let n = xs.len() as f64;
        if xs.is_empty() {
            return Interval::of(0.0, f64::INFINITY);
        }
        let m = xs.iter().sum::<f64>() / n;
        let var = if xs.len() > 1 { xs.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (n - 1.0) } else { f64::INFINITY };
        Interval::of(m, (var / n).sqrt())
    }

    fn overlaps(&self, o: &Interval) -> bool {
        self.lo <= o.hi && o.lo <= self.hi
    }

    fn json(&self) -> Json {
        let r = |x: f64| if x.is_finite() { json!((x * 1e6).round() / 1e6) } else { Json::Null };
        json!({"value": r(self.value), "lo": r(self.lo), "hi": r(self.hi), "se": r(self.se)})
    }
}

/// One slot's share of an estimate.
#[derive(Clone, Debug, Default)]
pub struct SlotEstimate {
    pub slot: usize,
    pub cdr: f64,
    pub snips: f64,
    /// Effective searches: `(Σw)² / Σw²` of the slot's weights.
    pub ess: f64,
    /// What the click model alone says the blend gets here.
    pub model: f64,
    /// What the logging blend got here.
    pub logged: f64,
    /// The share of the blend's pages, down to this slot, that the
    /// logging blend could have shown.
    pub coverage: f64,
}

/// A blend's value off-policy ([`evaluate`]).
#[derive(Clone, Debug)]
pub struct Estimate {
    pub label: String,
    /// The slots valued: the blend's k, at most the pages logged.
    pub k: usize,
    pub requests: usize,
    /// The logging blend's own value over the same requests and slots.
    pub logged: Interval,
    /// Cascade doubly robust.
    pub cdr: Interval,
    /// Its difference from [`Estimate::against`], paired by request.
    pub lift: Interval,
    /// What the lift is against: the logged blend's own value, or the
    /// first blend valued beside it.
    pub against: String,
    /// Cascade doubly robust over the top [`Estimate::check_depth`] slots.
    pub cdr_top: Interval,
    /// Self-normalized reweighting, slot by slot, no model, over the top
    /// [`Estimate::check_depth`] slots.
    pub snips: Interval,
    pub snips_lift: Interval,
    /// The slots, from the top, each with [`CHECK_ESS`] effective
    /// searches or more: where SNIPS is compared.
    pub check_depth: usize,
    /// What the click model alone says.
    pub model: f64,
    /// Effective searches at the top slot ([`TRUSTED`]).
    pub support: f64,
    /// The share of the blend's value on pages the logging blend could
    /// have shown ([`MIN_COVERAGE`]), each slot's weighed by what the model
    /// says the blend gets there.
    pub coverage: f64,
    /// The share of the blend's pages valued by the model from a slot
    /// down because one search's weight there grew past the most it
    /// counts for ([`MOST_SHARE`]).
    pub switched: f64,
    pub slots: Vec<SlotEstimate>,
    /// `better`, `worse`, `same` or `not proven`.
    pub verdict: &'static str,
    pub why: String,
    /// The most a replayed chance of the logging blend differs from the
    /// one logged.
    pub drift: f64,
    /// Whether the logging blend's definition was known (coverage is
    /// checked only then).
    pub known: bool,
    /// Per request valued: its share of the estimate.
    per_request: Vec<f64>,
}

impl Estimate {
    pub fn json(&self) -> Json {
        let r = |x: f64| if x.is_finite() { json!((x * 1e6).round() / 1e6) } else { Json::Null };
        json!({
            "label": self.label,
            "k": self.k,
            "requests": self.requests,
            "logged": self.logged.json(),
            "cdr": self.cdr.json(),
            "lift": self.lift.json(),
            "against": self.against,
            "cdr_top": self.cdr_top.json(),
            "snips": self.snips.json(),
            "snips_lift": self.snips_lift.json(),
            "check_depth": self.check_depth,
            "model": r(self.model),
            "support": r(self.support),
            "coverage": r(self.coverage),
            "switched": r(self.switched),
            "trusted": self.support >= TRUSTED,
            "slots": self.slots.iter().map(|s| json!({"slot": s.slot, "cdr": r(s.cdr), "snips": r(s.snips), "ess": r(s.ess), "model": r(s.model), "logged": r(s.logged), "coverage": r(s.coverage)})).collect::<Vec<_>>(),
            "verdict": self.verdict,
            "why": self.why,
        })
    }
}

/// One slot of a request replayed through a target.
struct Walk {
    /// The target's chance of the logged row ÷ the log's.
    ratio: f64,
    /// The model's reward of the logged row here.
    q_logged: f64,
    /// The model's reward expected of the target's rows here.
    q_target: f64,
    /// The target's chance here of rows the logging blend could not have
    /// placed.
    outside: f64,
    /// The model's rewards of the target's page below, after the logged
    /// rows above and its likeliest row here: (slot, reward).
    rest: Vec<(usize, f64)>,
    /// The weight grew past the most one search counts for: the model
    /// values the page from here.
    switched: bool,
}

/// The logged rows a target's sources come from, in its order.
fn inputs_for<'a>(def: &Def, r: &'a Request) -> Result<Vec<&'a Input>> {
    def.sources
        .iter()
        .map(|s| r.sources.iter().position(|x| x.eq_ignore_ascii_case(&s.name)).map(|i| &r.inputs[i]).ok_or_else(|| err(format!("the log has no rows of source {}: a blend is valued on the sources the logging blend ran", s.name))))
        .collect()
}

/// Replays request `r` through target `t` down to slot `k`: per slot
/// while the target could have shown the logged rows above it, the ratio
/// of chances, the model's rewards, and the model's value of the rest of
/// the target's page from there. A weight past `most` is switched to 0
/// (the model values the rest of that page): one search counts for at
/// most that many.
fn walk(t: &Target, r: &Request, k: usize, model: &Model, most: f64) -> Result<Vec<Walk>> {
    let inputs = inputs_for(&t.def, r)?;
    let keys: Vec<String> = r.shown.iter().take(k).map(|s| s.key.clone()).collect();
    let mut prev = 1.0;
    let mut keep = |l: usize, chance: f64| -> bool {
        let go = prev > 0.0;
        let w = prev * chance / r.shown[l].chance;
        prev = if w > most { 0.0 } else { w };
        go
    };
    let wk = blend::replay_with_rest(&t.def, k, &inputs, &keys, t.explore, &mut keep)?;
    let rp = &wk.replay;
    let src_of = |name: &str| r.sources.iter().position(|x| x.eq_ignore_ascii_case(name));
    let q = |c: &blend::Cand, slot: usize| -> f64 {
        src_of(&rp.sources[c.source]).and_then(|s| r.feats.get(s)).and_then(|f| f.get(c.rank - 1)).map_or(0.0, |f| model.predict(f, slot))
    };
    let mut out = Vec::with_capacity(rp.steps.len());
    let mut prev = 1.0;
    for (l, st) in rp.steps.iter().enumerate() {
        let slot = l + 1;
        let shown = &r.shown[l];
        let q_logged = r.feats.get(shown.source).and_then(|f| f.get(shown.rank - 1)).map_or(0.0, |f| model.predict(f, slot));
        let q_target: f64 = st.dist.iter().map(|&(c, p)| p * q(&rp.cands[c], slot)).sum();
        let outside: f64 = match r.logging.as_ref().and_then(|d| d.get(l)) {
            Some(d) => st.dist.iter().filter(|&&(c, _)| d.get(&blend::text(&rp.cands[c].key)).is_none_or(|p| *p <= 0.0)).map(|&(_, p)| p).sum(),
            None => 0.0,
        };
        let rest = wk.rest.get(l).map_or(Vec::new(), |after| after.iter().enumerate().map(|(j, &c)| (slot + 1 + j, q(&rp.cands[c], slot + 1 + j))).collect());
        let mut ratio = st.chance / shown.chance;
        let switched = prev * ratio > most;
        if switched {
            ratio = 0.0;
        }
        out.push(Walk { ratio, q_logged, q_target, outside, rest, switched });
        prev *= ratio;
        if prev == 0.0 {
            break;
        }
    }
    Ok(out)
}

/// Runs `f` over `items` on every core, in order.
fn each<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let n = std::thread::available_parallelism().map_or(1, |n| n.get()).min(16).max(1);
    if items.len() < 64 || n == 1 {
        return items.iter().map(&f).collect();
    }
    let chunk = items.len().div_ceil(n);
    std::thread::scope(|sc| {
        let hs: Vec<_> = items.chunks(chunk).map(|c| sc.spawn(|| c.iter().map(&f).collect::<Vec<R>>())).collect();
        hs.into_iter().flat_map(|h| h.join().expect("a valuing thread panicked")).collect()
    })
}

/// The least effective searches a slot needs for SNIPS to be compared
/// there: below it, a self-normalized mean is a handful of searches.
pub const CHECK_ESS: f64 = 100.0;

/// Values `target` over `reqs` down to slot `k`, each request's click
/// model `models[pick(request)]`.
///
/// Cascade doubly robust with the model's value of the rest of the page
/// (`Q̂`): for each slot l, with `w` the weight down to it,
/// `w_l (γ_l r_l − Q̂(a_1..l)) + w_l−1 E_π Q̂(a_1..l−1, ·)`, where
/// `Q̂(a_1..l) = γ_l q̂(a_l) + C_l` and `C_l` is the model's value of the
/// target's page below l after the logged rows above and its likeliest
/// row at l. Where a search leaves the target's page the weights fall to
/// 0 and `C` stands for the slots below; where it follows it, the clicks
/// correct the model.
fn estimate(t: &Target, reqs: &[&Request], k: usize, o: Options, models: &[Model], pick: &(dyn Fn(&Request) -> usize + Sync)) -> Result<Estimate> {
    let most = (reqs.len() as f64 * MOST_SHARE).max(MOST_WEIGHT);
    let walked: Vec<Result<Vec<Walk>>> = each(reqs, |r| walk(t, r, k, &models[pick(r)], most));
    let walked: Vec<Vec<Walk>> = walked.into_iter().collect::<Result<_>>()?;
    let n = reqs.len();
    let nf = n as f64;
    // Per request and slot: the estimate's share there, and the logged
    // value.
    let mut v = vec![vec![0.0; k]; n];
    let mut logged = vec![vec![0.0; k]; n];
    // Per slot and request: the weight down to it, and its reward.
    let mut w = vec![vec![0.0; n]; k];
    let mut rew = vec![vec![0.0; n]; k];
    let mut model_slot = vec![0.0; k];
    // The share of the target's pages, down to each slot, that leave what
    // the logging blend could draw there.
    let mut uncovered = vec![0.0; k];
    // The share of the target's pages whose weight grew past the most one
    // search counts for, valued by the model from there.
    let mut switched = 0.0;
    for (i, (r, wk)) in reqs.iter().zip(&walked).enumerate() {
        let mut prev = 1.0;
        for l in 0..k.min(r.shown.len()) {
            let g = gamma(l + 1, o.discount);
            let reward = r.shown[l].reward;
            logged[i][l] = g * reward;
            rew[l][i] = reward;
            // Past the walk the target could not have shown the rows
            // above: no weight, no share.
            let Some(st) = wk.get(l) else { continue };
            let wl = prev * st.ratio;
            w[l][i] = wl;
            v[i][l] += g * (wl * (reward - st.q_logged) + prev * st.q_target);
            for &(slot, q) in &st.rest {
                if slot <= k {
                    v[i][slot - 1] += (prev - wl) * gamma(slot, o.discount) * q;
                }
            }
            if l == 0 {
                model_slot[0] += g * st.q_target / nf;
                for &(slot, q) in &st.rest {
                    if slot <= k {
                        model_slot[slot - 1] += gamma(slot, o.discount) * q / nf;
                    }
                }
            }
            uncovered[l] += prev * st.outside / nf;
            if st.switched {
                switched += prev / nf;
            }
            prev = wl;
        }
    }
    let total = |rows: &[Vec<f64>], depth: usize| -> Vec<f64> { rows.iter().map(|x| x[..depth].iter().sum()).collect() };
    let cdr_i = total(&v, k);
    let logged_i = total(&logged, k);
    let cdr = Interval::mean(&cdr_i);
    let logged_v = Interval::mean(&logged_i);
    let lift: Vec<f64> = cdr_i.iter().zip(&logged_i).map(|(a, b)| a - b).collect();
    let cdr_lift = Interval::mean(&lift);
    // Each slot's effective searches, and SNIPS slot by slot.
    let ess: Vec<f64> = (0..k)
        .map(|l| {
            let sw: f64 = w[l].iter().sum();
            let sw2: f64 = w[l].iter().map(|x| x * x).sum();
            if sw2 > 0.0 { sw * sw / sw2 } else { 0.0 }
        })
        .collect();
    let snips_at: Vec<f64> = (0..k)
        .map(|l| {
            let sw: f64 = w[l].iter().sum();
            let swr: f64 = w[l].iter().zip(&rew[l]).map(|(a, b)| a * b).sum();
            if sw > 0.0 { gamma(l + 1, o.discount) * swr / sw } else { 0.0 }
        })
        .collect();
    // SNIPS is compared with cascade doubly robust over the top slots
    // where it has the searches to be.
    let depth = ess.iter().take_while(|e| **e >= CHECK_ESS).count();
    let mut psi = vec![0.0; n];
    for l in 0..depth {
        let g = gamma(l + 1, o.discount);
        let mean_w = w[l].iter().sum::<f64>() / nf;
        let vl = snips_at[l] / g;
        for i in 0..n {
            psi[i] += g * w[l][i] * (rew[l][i] - vl) / mean_w;
        }
    }
    let snips: f64 = snips_at[..depth].iter().sum();
    let snips_i = Interval::of(snips, Interval::mean(&psi).se);
    let cdr_top = Interval::mean(&total(&v, depth));
    let logged_top = total(&logged, depth);
    let mean_top = logged_top.iter().sum::<f64>() / nf.max(1.0);
    let lift_psi: Vec<f64> = psi.iter().zip(&logged_top).map(|(p, l)| p - (l - mean_top)).collect();
    let snips_lift = Interval::of(snips - mean_top, Interval::mean(&lift_psi).se);
    let mut slots = Vec::with_capacity(k);
    for l in 0..k {
        let covered = 1.0 - uncovered[..=l].iter().sum::<f64>();
        slots.push(SlotEstimate {
            slot: l + 1,
            cdr: v.iter().map(|x| x[l]).sum::<f64>() / nf.max(1.0),
            snips: snips_at[l],
            ess: ess[l],
            model: model_slot[l],
            logged: logged.iter().map(|x| x[l]).sum::<f64>() / nf.max(1.0),
            coverage: covered.clamp(0.0, 1.0),
        });
    }
    let model: f64 = model_slot.iter().sum();
    let support = ess.first().copied().unwrap_or(0.0);
    // Coverage where the value is: each slot's, weighed by what the model
    // says the target gets there.
    let coverage = {
        let wsum: f64 = model_slot.iter().map(|m| m.max(0.0)).sum();
        if wsum > 1e-12 { slots.iter().zip(&model_slot).map(|(s, m)| s.coverage * m.max(0.0)).sum::<f64>() / wsum } else { slots.last().map_or(1.0, |s| s.coverage) }
    };
    let drift = reqs.iter().map(|r| r.drift).fold(0.0, f64::max);
    let known = reqs.iter().all(|r| r.logging.is_some());
    let mut e = Estimate {
        label: t.label.clone(),
        k,
        requests: n,
        logged: logged_v,
        cdr,
        lift: cdr_lift,
        against: "the logged blend".into(),
        cdr_top,
        snips: snips_i,
        snips_lift,
        check_depth: depth,
        model,
        support,
        coverage,
        switched,
        slots,
        verdict: "not proven",
        why: String::new(),
        drift,
        known,
        per_request: cdr_i,
    };
    let base = e.logged.value;
    judge(&mut e, cdr_lift, base);
    Ok(e)
}

/// The verdict on `e` given its lift against what it is compared with
/// (worth `base`).
fn judge(e: &mut Estimate, lift: Interval, base: f64) {
    let pct = |x: f64| 100.0 * x / base.max(1e-12);
    let (verdict, why) = if e.requests == 0 {
        ("not proven", "no logged request of the blend explored".to_string())
    } else if e.drift > 1e-6 {
        ("not proven", format!("the logging blend's replay does not give back the chances it logged (off by up to {:.2e}): its rows or its definition changed", e.drift))
    } else if e.coverage < MIN_COVERAGE {
        ("not proven", format!("{:.0}% of what this blend would be worth lies on pages the exploring blend could not draw, valued by the click model alone: explore more widely (a larger top or tau) to value it", 100.0 * (1.0 - e.coverage)))
    } else if e.support < TRUSTED {
        ("not proven", format!("thin support: {:.0} effective searches at the top slot ({TRUSTED:.0} trusted); log more exploring searches, or value a blend nearer the one that logged", e.support))
    } else if e.check_depth > 0 && !e.cdr_top.overlaps(&e.snips) {
        ("not proven", format!("over the top {} slots the two estimates disagree beyond their intervals ({:.4} and {:.4}): the click model or the logged chances are off", e.check_depth, e.cdr_top.value, e.snips.value))
    } else if lift.lo > 0.0 {
        ("better", format!("{:+.1}% against {} ({:+.1}% to {:+.1}%)", pct(lift.value), e.against, pct(lift.lo), pct(lift.hi)))
    } else if lift.hi < 0.0 {
        ("worse", format!("{:+.1}% against {} ({:+.1}% to {:+.1}%)", pct(lift.value), e.against, pct(lift.lo), pct(lift.hi)))
    } else {
        ("same", format!("no difference shown against {}: {:+.1}% ({:+.1}% to {:+.1}%)", e.against, pct(lift.value), pct(lift.lo), pct(lift.hi)))
    };
    let why = if e.known { why } else { format!("{why} (the logging blend's definition is not known, so its coverage is not checked)") };
    e.lift = lift;
    e.verdict = verdict;
    e.why = why;
}

/// Judges `e` against `base`, valued over the same requests: their
/// difference paired request by request.
fn against(e: &mut Estimate, base: &Estimate) {
    let d: Vec<f64> = e.per_request.iter().zip(&base.per_request).map(|(a, b)| a - b).collect();
    e.against = base.label.clone();
    judge(e, Interval::mean(&d), base.cdr.value);
}

/// The deepest page logged.
fn logged_k(reqs: &[Request]) -> usize {
    reqs.iter().map(|r| r.shown.len()).max().unwrap_or(0)
}

/// Gets the requests ready to value: each one's rows read for the click
/// model, and the logging blend (its version's definition, from
/// `logging`) replayed over its page, which gives back the chances it
/// logged and the rows it could have placed instead.
fn ready(reqs: &mut [Request], logging: &(dyn Fn(i64) -> Option<Def> + Sync)) -> Result<Reading> {
    let reading = Reading::of(reqs);
    let replayed: Vec<Result<Option<(Vec<HashMap<String, f64>>, f64)>>> = each(reqs, |r| {
        let Some(def) = logging(r.version) else { return Ok(None) };
        let inputs = inputs_for(&def, r)?;
        let keys: Vec<String> = r.shown.iter().map(|s| s.key.clone()).collect();
        let rp = blend::replay(&def, keys.len(), &inputs, &keys, r.explore)?;
        let mut drift: f64 = if rp.steps.len() < r.shown.len() { 1.0 } else { 0.0 };
        let dist = rp
            .steps
            .iter()
            .zip(&r.shown)
            .map(|(st, s)| {
                drift = drift.max((st.chance - s.chance).abs() / s.chance.max(1e-300));
                st.dist.iter().map(|&(c, p)| (blend::text(&rp.cands[c].key), p)).collect()
            })
            .collect();
        Ok(Some((dist, drift)))
    });
    for (r, rp) in reqs.iter_mut().zip(replayed) {
        r.feats = reading.read(r);
        if let Some((dist, drift)) = rp? {
            r.logging = Some(dist);
            r.drift = drift;
        }
    }
    Ok(reading)
}

/// The values of `targets` over `reqs`, the click model cross-fitted
/// over the two halves of the requests; `logging` gives the logging
/// blend's definition of each version. The first is judged against the
/// logged blend's own value, each other one against the first (paired by
/// request, as two values of the same searches).
pub fn evaluate(reqs: &mut [Request], logging: &(dyn Fn(i64) -> Option<Def> + Sync), targets: &[Target], o: Options) -> Result<(Vec<Estimate>, Json)> {
    let reading = ready(reqs, logging)?;
    let kl = logged_k(reqs);
    let width = reading.width();
    let models = [Model::fit(reqs.iter().filter(|r| r.fold & 1 == 1), kl.max(1), width), Model::fit(reqs.iter().filter(|r| r.fold & 1 == 0), kl.max(1), width)];
    let all: Vec<&Request> = reqs.iter().collect();
    let pick = |r: &Request| r.fold & 1;
    let mut out: Vec<Estimate> = Vec::new();
    for t in targets {
        out.push(estimate(t, &all, t.def.k.min(kl).max(1), o, &models, &pick)?);
    }
    // Every blend after the first is judged against it.
    if let Some((first, rest)) = out.split_first_mut() {
        for e in rest {
            if e.k == first.k {
                against(e, first);
            }
        }
    }
    Ok((out, json!({"reads": reading.cells, "sources": reading.sources, "folds": 2})))
}

/// What tuning varies, and how much.
#[derive(Clone, Copy, Debug)]
pub struct TuneOptions {
    /// Blends tried besides the start.
    pub rounds: usize,
    pub seed: u64,
    pub discount: bool,
}

impl Default for TuneOptions {
    fn default() -> Self {
        TuneOptions { rounds: 24, seed: 1, discount: false }
    }
}

/// A blend tried while tuning.
#[derive(Clone, Debug)]
pub struct Tried {
    pub def: Def,
    /// What it changed from the start, in words.
    pub change: String,
    /// Its value on the half that chose.
    pub train: Estimate,
}

/// What tuning found ([`tune`]).
#[derive(Clone, Debug)]
pub struct Tuned {
    pub tried: Vec<Tried>,
    /// The best of them, valued on the half that chose nothing, and the
    /// start valued there too.
    pub best: Option<(Def, Estimate)>,
    pub start: Option<Estimate>,
}

/// Blends near `start`: head weights scaled, quota shares moved,
/// diversity changed.
fn variants(start: &Def, n: usize, seed: u64) -> Vec<(Def, String)> {
    let mut rng = crate::explore::Rng::seeded(seed);
    let scales: [f64; 7] = [0.0, 0.5, 0.75, 1.25, 1.5, 2.0, 3.0];
    let mut out: Vec<(Def, String)> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    seen.insert(start.sql(false));
    let mut tries = 0;
    while out.len() < n && tries < n * 20 {
        tries += 1;
        let mut d = start.clone();
        let mut what: Vec<String> = Vec::new();
        if d.heads.is_empty() {
            d.heads = d.heads();
        }
        // One or two changes.
        let changes = 1 + rng.below(2);
        for _ in 0..changes {
            let pick = rng.below(3);
            if pick == 0 || (d.quotas.is_empty() && d.diversify.is_none()) {
                let h = rng.below(d.heads.len().max(1));
                let Some(old) = d.heads.get(h).map(|x| x.weight) else { continue };
                let f = scales[rng.below(scales.len())];
                let w = if old == 0.0 { f.max(0.25) } else { old * f };
                let w = (w * 1000.0).round() / 1000.0;
                let others = d.heads.iter().enumerate().filter(|(i, _)| *i != h).any(|(_, x)| x.weight != 0.0);
                if w == old || (w == 0.0 && !others) {
                    continue;
                }
                what.push(format!("{} weight {} → {}", d.heads[h].name, num(old), num(w)));
                d.heads[h].weight = w;
            } else if pick == 1 && !d.quotas.is_empty() {
                let q = rng.below(d.quotas.len());
                let step = [-20.0, -10.0, 10.0, 20.0][rng.below(4)];
                let quota = &mut d.quotas[q];
                let before = quota.sql();
                if quota.min > 0.0 {
                    quota.min = (quota.min + step).clamp(0.0, quota.max.min(100.0));
                } else {
                    quota.max = (quota.max + step).clamp(quota.min, 100.0);
                }
                if quota.sql() != before {
                    what.push(format!("{before} → {}", quota.sql()));
                }
            } else if let Some(dv) = d.diversify.as_mut() {
                let l = [0.0, 0.1, 0.2, 0.4, 0.6, 0.8][rng.below(6)];
                if l != dv.lambda {
                    what.push(format!("DIVERSIFY {} → {}", dv.lambda, l));
                    dv.lambda = l;
                }
            }
        }
        if what.is_empty() || d.check().is_err() || !seen.insert(d.sql(false)) {
            continue;
        }
        d.note = Some(format!("tuned: {}", what.join("; ")));
        out.push((d, what.join("; ")));
    }
    out
}

/// Tunes `start` (served as the best taken) toward the log's reward: the
/// start and [`TuneOptions::rounds`] blends near it valued by cascade
/// doubly robust on half the requests, the click model cross-fitted
/// within that half; the best then valued, with the start, on the other
/// half, its click model fitted on the first.
pub fn tune(reqs: &mut [Request], logging: &(dyn Fn(i64) -> Option<Def> + Sync), start: &Target, o: TuneOptions) -> Result<Tuned> {
    let reading = ready(reqs, logging)?;
    let kl = logged_k(reqs);
    let width = reading.width();
    let k = start.def.k.min(kl).max(1);
    let opts = Options { discount: o.discount };
    let train: Vec<&Request> = reqs.iter().filter(|r| r.fold & 1 == 0).collect();
    let test: Vec<&Request> = reqs.iter().filter(|r| r.fold & 1 == 1).collect();
    // Within the half that chooses, its two quarters fit each other's
    // model; the other half is valued with the model of the first.
    let m = [
        Model::fit(train.iter().copied().filter(|r| r.fold & 2 != 0), kl.max(1), width),
        Model::fit(train.iter().copied().filter(|r| r.fold & 2 == 0), kl.max(1), width),
        Model::fit(train.iter().copied(), kl.max(1), width),
    ];
    let pick_train = |r: &Request| (r.fold >> 1) & 1;
    let pick_test = |_: &Request| 2;
    let mut tried = Vec::new();
    let base = estimate(start, &train, k, opts, &m, &pick_train)?;
    tried.push(Tried { def: start.def.clone(), change: "the start".into(), train: base });
    for (d, change) in variants(&start.def, o.rounds, o.seed) {
        let t = Target { label: format!("{} (tuned)", start.label), def: d.clone(), explore: start.explore };
        let e = estimate(&t, &train, k, opts, &m, &pick_train)?;
        tried.push(Tried { def: d, change, train: e });
    }
    let best_i = (0..tried.len()).max_by(|&a, &b| tried[a].train.cdr.value.total_cmp(&tried[b].train.cdr.value).then(b.cmp(&a))).unwrap_or(0);
    let (best, start_test) = if test.is_empty() {
        (None, None)
    } else {
        let bt = Target { label: format!("{} (tuned)", start.label), def: tried[best_i].def.clone(), explore: start.explore };
        let mut be = estimate(&bt, &test, k, opts, &m, &pick_test)?;
        let se = estimate(start, &test, k, opts, &m, &pick_test)?;
        against(&mut be, &se);
        (Some((tried[best_i].def.clone(), be)), Some(se))
    };
    Ok(Tuned { tried, best, start: start_test })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_read_back_as_they_were() {
        for v in [Value::Null, Value::Int(3), Value::Float(3.0), Value::Float(0.1), Value::Str("a".into()), Value::Bool(true), Value::Timestamp(1_700_000_000_000_000), Value::Vector(vec![0.25, -1.5e-3, 7.0])] {
            let j = blend::cell_json(&v, true).unwrap();
            let back = cell_value(&serde_json::from_str::<Json>(&j.to_string()).unwrap());
            assert_eq!(back, v, "{v:?} as {j}");
        }
    }

    #[test]
    fn ridge_least_squares_solves() {
        // x + y = 3, x - y = 1.
        let a = vec![2.0, 0.0, 0.0, 2.0];
        let b = vec![4.0, 2.0];
        let x = solve(a, b, 2);
        assert!((x[0] - 2.0).abs() < 1e-12 && (x[1] - 1.0).abs() < 1e-12);
    }
}
