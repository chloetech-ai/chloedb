//! Off-policy estimators: what a ranking that never served would have
//! earned, from a log of what another served, with its propensities. Each
//! is computed for two rewards, clicks and satisfied clicks per impression,
//! with a standard error, so the checks can ask whether its 95% interval
//! holds the truth as often as it says.
//!
//! | estimator | weights each logged click by | unbiased when | reference |
//! |---|---|---|---|
//! | `naive` | 1 | the target is the logging policy | |
//! | `ips` | the whole list's chance under the target over the logger's | the logger could show the target's list | Horvitz and Thompson 1952; Li et al., WSDM 2011 |
//! | `snips` | as `ips`, self-normalised | as `ips`, as the log grows | Swaminathan and Joachims, NeurIPS 2015 |
//! | `iips` | the row's chance at its position, target over logger | clicks depend on the row and its position only (`pbm`, `trust`) | Li et al., KDD 2018 |
//! | `sniips` | as `iips`, self-normalised per position | as `iips`, as the log grows | |
//! | `rips` | the chance of the rows down to it, target over logger | clicks depend on the rows above and the row (all six models) | McInerney et al., KDD 2020 |
//! | `snrips` | as `rips`, self-normalised per position | as `rips`, as the log grows | |
//! | `dm-pbm` | nothing: a PBM fitted to the log predicts the target's clicks | PBM is the users' model and every row was seen | Chuklin et al., 2015 |
//! | `dm-sdbn` | nothing: a simplified DBN fitted to the log | the SDBN is right | Chapelle and Zhang, WWW 2009 |
//! | `dr-iips` | `dm-pbm` plus `iips` of what it got wrong | `iips`'s condition, or PBM is right | Dudík et al., ICML 2011 |
//! | `cdr` | `dm-sdbn` plus `rips` of what it got wrong (cascade doubly robust) | `rips`'s condition, or the SDBN is right | Kiyohara et al., WSDM 2022 |
//!
//! The models of `dm-*`, `dr-iips` and `cdr` are cross-fitted: impressions
//! are split in two by their index, each half valued by the model fitted
//! on the other, so a model never grades the clicks it learnt from and the
//! standard errors hold. For a target that shows a fixed list, `cdr`'s
//! sum reduces to `dm-sdbn` plus Σₖ ρ₁..ₖ (rₖ − m̂ₖ), ρ the prefix weight
//! and m̂ₖ the model's clicks at k on the target's list.

use std::collections::HashMap;

use serde::Serialize;

use crate::policy::Policy;
use crate::sim::{Impression, World};
use crate::stats::Running;

pub const ESTIMATORS: [&str; 11] = ["naive", "ips", "snips", "iips", "sniips", "rips", "snrips", "dm-pbm", "dm-sdbn", "dr-iips", "cdr"];
pub const REWARDS: [&str; 2] = ["clicks", "satisfied"];

/// A ranking to value: per query, the rows it shows, as keys and as the
/// logging ranking's indexes (`None`: a row it did not rank).
#[derive(Clone, Debug)]
pub struct Target {
    pub name: String,
    pub keys: Vec<Vec<String>>,
    pub lists: Vec<Vec<Option<u16>>>,
}

impl Target {
    pub fn new(w: &World, name: &str, keys: Vec<Vec<String>>) -> Target {
        let lists = keys.iter().enumerate().map(|(q, ks)| ks.iter().take(w.limit).map(|k| w.index(q, k)).collect()).collect();
        Target { name: name.to_string(), keys: keys.into_iter().map(|ks| ks.into_iter().take(w.limit).collect()).collect(), lists }
    }

    /// The share of its slots, weighted by traffic, the logging policy
    /// never fills with its row: what no weighting can value.
    pub fn unsupported(&self, w: &World) -> f64 {
        let shares = w.site.shares();
        let mut out = 0.0;
        for (q, list) in self.lists.iter().enumerate() {
            let m = w.policy.marginals(&w.ranked[q].scores).unwrap_or_default();
            let bad = list.iter().enumerate().filter(|(k, i)| !i.is_some_and(|i| m.get(i as usize).and_then(|r| r.get(*k)).is_some_and(|&p| p > 0.0))).count();
            out += shares[q] * bad as f64 / list.len().max(1) as f64;
        }
        out
    }
}

/// Counts of one fold of impressions, which the models are fitted from.
#[derive(Clone, Debug, Default)]
pub struct Counts {
    /// (query, row, position): shown, clicked, satisfied.
    pub cells: HashMap<(u32, u16, u8), [f64; 3]>,
    /// (query, row), for the SDBN: examined, clicked, clicked last, satisfied.
    pub rows: HashMap<(u32, u16), [f64; 4]>,
    pub depth: usize,
}

impl Counts {
    pub fn add(&mut self, imp: &Impression) {
        let n = imp.shown.len();
        self.depth = self.depth.max(n);
        let last = (0..n).rev().find(|&k| imp.click[k]);
        for k in 0..n {
            let c = self.cells.entry((imp.query, imp.shown[k], k as u8)).or_default();
            c[0] += 1.0;
            c[1] += imp.click[k] as u8 as f64;
            c[2] += imp.satisfied[k] as u8 as f64;
            // The SDBN's user read down to the last click, or all of a list without one.
            if last.map_or(true, |l| k <= l) {
                let r = self.rows.entry((imp.query, imp.shown[k])).or_default();
                r[0] += 1.0;
                r[1] += imp.click[k] as u8 as f64;
                r[2] += (Some(k) == last) as u8 as f64;
                r[3] += imp.satisfied[k] as u8 as f64;
            }
        }
    }
}

/// A position-based model fitted by EM (Chuklin, Markov and de Rijke,
/// "Click models for web search", 2015, §4.2) on the counts, with a
/// satisfaction rate per row.
#[derive(Clone, Debug)]
pub struct Pbm {
    pub theta: Vec<f64>,
    alpha: HashMap<(u32, u16), f64>,
    sat: HashMap<(u32, u16), f64>,
    prior: (f64, f64),
}

impl Pbm {
    pub fn fit(c: &Counts, iters: usize) -> Pbm {
        let depth = c.depth.max(1);
        let mut theta: Vec<f64> = (0..depth).map(|k| 1.0 / (k + 1) as f64).collect();
        let mut alpha: HashMap<(u32, u16), f64> = c.cells.keys().map(|&(q, i, _)| ((q, i), 0.3)).collect();
        let (mut shown, mut clicked, mut sat) = (HashMap::<(u32, u16), f64>::new(), HashMap::<(u32, u16), f64>::new(), HashMap::<(u32, u16), f64>::new());
        for (&(q, i, _), v) in &c.cells {
            *shown.entry((q, i)).or_default() += v[0];
            *clicked.entry((q, i)).or_default() += v[1];
            *sat.entry((q, i)).or_default() += v[2];
        }
        for _ in 0..iters {
            let mut e_num = vec![0.0; depth];
            let mut e_den = vec![0.0; depth];
            let mut a_num: HashMap<(u32, u16), f64> = HashMap::with_capacity(alpha.len());
            for (&(q, i, k), v) in &c.cells {
                let (t, a) = (theta[k as usize], alpha[&(q, i)]);
                let quiet = v[0] - v[1];
                let denom = (1.0 - t * a).max(1e-12);
                e_num[k as usize] += v[1] + quiet * t * (1.0 - a) / denom;
                e_den[k as usize] += v[0];
                *a_num.entry((q, i)).or_default() += v[1] + quiet * a * (1.0 - t) / denom;
            }
            for k in 0..depth {
                if e_den[k] > 0.0 {
                    theta[k] = (e_num[k] / e_den[k]).clamp(1e-4, 1.0);
                }
            }
            for (key, a) in alpha.iter_mut() {
                // A Beta(1, 2) prior keeps a row seen once from 0 or 1.
                *a = ((a_num[key] + 1.0) / (shown[key] + 3.0)).clamp(1e-4, 0.999);
            }
        }
        let total: f64 = shown.values().sum();
        let mean_alpha = if total > 0.0 { alpha.iter().map(|(k, a)| a * shown[k]).sum::<f64>() / total } else { 0.2 };
        let clicks: f64 = clicked.values().sum();
        let mean_sat = if clicks > 0.0 { sat.values().sum::<f64>() / clicks } else { 0.5 };
        let sat = clicked.iter().map(|(k, &c)| (*k, (sat[k] + 2.0 * mean_sat) / (c + 2.0))).collect();
        Pbm { theta, alpha, sat, prior: (mean_alpha, mean_sat) }
    }

    /// The chance of a click at position k on ranked row `i` of query `q`.
    pub fn click(&self, q: u32, i: Option<u16>, k: usize) -> f64 {
        let a = i.and_then(|i| self.alpha.get(&(q, i)).copied()).unwrap_or(self.prior.0);
        self.theta.get(k).copied().unwrap_or(*self.theta.last().unwrap_or(&0.1)) * a
    }

    pub fn satisfies(&self, q: u32, i: Option<u16>) -> f64 {
        i.and_then(|i| self.sat.get(&(q, i)).copied()).unwrap_or(self.prior.1)
    }
}

/// The simplified DBN (Chapelle and Zhang 2009, γ = 1), fitted by counting:
/// a row's attractiveness is its clicks over the times it was read (at or
/// above the last click), its chance of ending the reading its last clicks
/// over its clicks.
#[derive(Clone, Debug)]
pub struct Sdbn {
    rows: HashMap<(u32, u16), (f64, f64, f64)>,
    prior: (f64, f64, f64),
}

impl Sdbn {
    pub fn fit(c: &Counts) -> Sdbn {
        let s: [f64; 4] = c.rows.values().fold([0.0; 4], |a, v| [a[0] + v[0], a[1] + v[1], a[2] + v[2], a[3] + v[3]]);
        let prior = (if s[0] > 0.0 { s[1] / s[0] } else { 0.2 }, if s[1] > 0.0 { s[2] / s[1] } else { 0.5 }, if s[1] > 0.0 { s[3] / s[1] } else { 0.5 });
        let rows = c.rows.iter().map(|(k, v)| (*k, ((v[1] + 2.0 * prior.0) / (v[0] + 2.0), (v[2] + 2.0 * prior.1) / (v[1] + 2.0), (v[3] + 2.0 * prior.2) / (v[1] + 2.0)))).collect();
        Sdbn { rows, prior }
    }

    /// Its clicks and satisfied clicks per position of the list `list`.
    pub fn expected(&self, q: u32, list: &[Option<u16>]) -> (Vec<f64>, Vec<f64>) {
        let mut reach = 1.0;
        let (mut c, mut s) = (Vec::with_capacity(list.len()), Vec::with_capacity(list.len()));
        for i in list {
            let (a, stop, sat) = i.and_then(|i| self.rows.get(&(q, i)).copied()).unwrap_or(self.prior);
            c.push(reach * a);
            s.push(reach * a * sat);
            reach *= 1.0 - a * stop;
        }
        (c, s)
    }
}

/// The two halves' models: impressions of fold f are valued by `[1 − f]`.
pub struct Models {
    pub pbm: [Pbm; 2],
    pub sdbn: [Sdbn; 2],
}

impl Models {
    pub fn fit(counts: &[Counts; 2]) -> Models {
        Models { pbm: [Pbm::fit(&counts[1], 40), Pbm::fit(&counts[0], 40)], sdbn: [Sdbn::fit(&counts[1]), Sdbn::fit(&counts[0])] }
    }
}

pub fn fold(i: u64) -> usize {
    (i % 2) as usize
}

/// A sum of ratios, Σₖ Aₖ/Bₖ over per-impression terms aₖ and bₖ, with
/// its standard error by the delta method: the influence of impression i
/// is Σₖ (aᵢₖ − Vₖ bᵢₖ)/b̄ₖ, its variance from the moments kept.
#[derive(Clone, Debug)]
pub struct Ratio {
    k: usize,
    n: f64,
    a: Vec<f64>,
    b: Vec<f64>,
    aa: Vec<f64>,
    ab: Vec<f64>,
    bb: Vec<f64>,
}

impl Ratio {
    pub fn new(k: usize) -> Ratio {
        Ratio { k, n: 0.0, a: vec![0.0; k], b: vec![0.0; k], aa: vec![0.0; k * k], ab: vec![0.0; k * k], bb: vec![0.0; k * k] }
    }

    pub fn add(&mut self, a: &[f64], b: &[f64]) {
        self.n += 1.0;
        let k = self.k;
        for x in 0..k.min(a.len()) {
            self.a[x] += a[x];
            self.b[x] += b[x];
            if a[x] == 0.0 && b[x] == 0.0 {
                continue;
            }
            for y in 0..k.min(a.len()) {
                self.aa[x * k + y] += a[x] * a[y];
                self.ab[x * k + y] += a[x] * b[y];
                self.bb[x * k + y] += b[x] * b[y];
            }
        }
    }

    pub fn merge(&mut self, o: &Ratio) {
        self.n += o.n;
        for (x, y) in [(&mut self.a, &o.a), (&mut self.b, &o.b), (&mut self.aa, &o.aa), (&mut self.ab, &o.ab), (&mut self.bb, &o.bb)] {
            x.iter_mut().zip(y).for_each(|(x, y)| *x += y);
        }
    }

    pub fn value(&self) -> f64 {
        (0..self.k).filter(|&x| self.b[x] > 0.0).map(|x| self.a[x] / self.b[x]).sum()
    }

    pub fn se(&self) -> f64 {
        let k = self.k;
        let v: Vec<f64> = (0..k).map(|x| if self.b[x] > 0.0 { self.a[x] / self.b[x] } else { 0.0 }).collect();
        let bbar: Vec<f64> = (0..k).map(|x| self.b[x] / self.n).collect();
        let mut e2 = 0.0;
        for x in 0..k {
            for y in 0..k {
                if bbar[x] <= 0.0 || bbar[y] <= 0.0 {
                    continue;
                }
                let m = self.aa[x * k + y] - v[y] * self.ab[x * k + y] - v[x] * self.ab[y * k + x] + v[x] * v[y] * self.bb[x * k + y];
                e2 += m / (self.n * bbar[x] * bbar[y]);
            }
        }
        (e2.max(0.0) / self.n).sqrt()
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Estimate {
    pub estimator: &'static str,
    pub reward: &'static str,
    pub value: f64,
    pub se: f64,
    /// For an estimator that weights, the effective number of impressions
    /// its weights rest on, (Σw)²/Σw², at its thinnest position (Kish
    /// 1965): an interval on a few dozen is not to be trusted, however
    /// unbiased the estimator.
    pub ess: Option<f64>,
    /// The mean of its weights, at the position where it is furthest from
    /// 1 in its standard errors, and that distance (z): a target the
    /// logger supports has weights of mean 1, so a mean well short of it
    /// says the log rarely showed what the target would, and one well above
    /// it that a rare list's weight swamps the rest, whatever the effective
    /// size of what it did show.
    pub weight_mean: Option<f64>,
    pub weight_z: Option<f64>,
}

/// An estimate whose weights rest on fewer effective impressions than
/// this, or whose weights' mean is further than [`MAX_WEIGHT_Z`] standard
/// errors from 1, is shown with a warning, not as a result: its interval
/// cannot be trusted.
pub const MIN_ESS: f64 = 400.0;
pub const MAX_WEIGHT_Z: f64 = 3.0;

impl Estimate {
    pub fn covers(&self, truth: f64) -> bool {
        (self.value - truth).abs() <= 1.959963984540054 * self.se
    }

    /// Whether its weights average 1 within their noise, as a supported
    /// target's do. An estimator with no weights (naive, a model) has
    /// nothing to check.
    pub fn supported(&self) -> bool {
        self.weight_z.is_none_or(|z| z.abs() <= MAX_WEIGHT_Z)
    }

    /// Whether the app shows it as a result: supported, and on enough
    /// effective impressions for its interval to hold.
    pub fn trusted(&self) -> bool {
        self.supported() && self.ess.is_none_or(|e| e >= MIN_ESS)
    }
}

/// Every estimator of one target, accumulated over impressions.
pub struct Valuer<'a> {
    target: &'a Target,
    depth: usize,
    /// Per fold and query: the models' clicks and satisfied clicks per
    /// position of the target's list.
    pbm: [Vec<[Vec<f64>; 2]>; 2],
    sdbn: [Vec<[Vec<f64>; 2]>; 2],
    run: [[Running; 8]; 2],
    ratio: [[Ratio; 3]; 2],
    /// Σw and Σw² of the list's weight, and per position of the
    /// row's and of the prefix's.
    w_list: [f64; 2],
    w_row: Vec<[f64; 2]>,
    w_prefix: Vec<[f64; 2]>,
}

const NAIVE: usize = 0;
const IPS: usize = 1;
const IIPS: usize = 2;
const RIPS: usize = 3;
const DM_PBM: usize = 4;
const DM_SDBN: usize = 5;
const DR_IIPS: usize = 6;
const CDR: usize = 7;

impl<'a> Valuer<'a> {
    pub fn new(target: &'a Target, models: Option<&Models>, depth: usize) -> Valuer<'a> {
        let pred = |f: usize, pbm: bool| -> Vec<[Vec<f64>; 2]> {
            let Some(m) = models else { return Vec::new() };
            target
                .lists
                .iter()
                .enumerate()
                .map(|(q, list)| {
                    let q = q as u32;
                    if pbm {
                        let c: Vec<f64> = list.iter().enumerate().map(|(k, &i)| m.pbm[f].click(q, i, k)).collect();
                        let s = list.iter().zip(&c).map(|(&i, c)| c * m.pbm[f].satisfies(q, i)).collect();
                        [c, s]
                    } else {
                        let (c, s) = m.sdbn[f].expected(q, list);
                        [c, s]
                    }
                })
                .collect()
        };
        Valuer {
            target,
            depth,
            pbm: [pred(0, true), pred(1, true)],
            sdbn: [pred(0, false), pred(1, false)],
            run: Default::default(),
            ratio: [[Ratio::new(1), Ratio::new(depth), Ratio::new(depth)], [Ratio::new(1), Ratio::new(depth), Ratio::new(depth)]],
            w_list: [0.0; 2],
            w_row: vec![[0.0; 2]; depth],
            w_prefix: vec![[0.0; 2]; depth],
        }
    }

    pub fn add(&mut self, imp: &Impression) {
        let q = imp.query as usize;
        let tl = &self.target.lists[q];
        let n = imp.shown.len().min(self.depth);
        let mut wi = [0.0f64; 32];
        let mut wr = [0.0f64; 32];
        let mut prefix_ok = true;
        for k in 0..n {
            let hit = tl.get(k) == Some(&Some(imp.shown[k]));
            prefix_ok &= hit;
            wi[k] = if hit { 1.0 / imp.propensity[k] } else { 0.0 };
            wr[k] = if prefix_ok { 1.0 / imp.prefix[k] } else { 0.0 };
        }
        let wl = if n > 0 && n == tl.len() { wr[n - 1] } else { 0.0 };
        self.w_list[0] += wl;
        self.w_list[1] += wl * wl;
        for k in 0..n {
            self.w_row[k][0] += wi[k];
            self.w_row[k][1] += wi[k] * wi[k];
            self.w_prefix[k][0] += wr[k];
            self.w_prefix[k][1] += wr[k] * wr[k];
        }
        let f = fold(imp.i);
        for (r, sat) in [false, true].into_iter().enumerate() {
            let rw: Vec<f64> = (0..n).map(|k| imp.reward(sat, k)).collect();
            let total: f64 = rw.iter().sum();
            let run = &mut self.run[r];
            run[NAIVE].add(total);
            run[IPS].add(wl * total);
            run[IIPS].add((0..n).map(|k| wi[k] * rw[k]).sum());
            run[RIPS].add((0..n).map(|k| wr[k] * rw[k]).sum());
            if let (Some(p), Some(s)) = (self.pbm[f].get(q), self.sdbn[f].get(q)) {
                let (p, s) = (&p[r], &s[r]);
                let dm_p: f64 = p.iter().sum();
                let dm_s: f64 = s.iter().sum();
                run[DM_PBM].add(dm_p);
                run[DM_SDBN].add(dm_s);
                run[DR_IIPS].add(dm_p + (0..n.min(p.len())).map(|k| wi[k] * (rw[k] - p[k])).sum::<f64>());
                run[CDR].add(dm_s + (0..n.min(s.len())).map(|k| wr[k] * (rw[k] - s[k])).sum::<f64>());
            }
            let ratio = &mut self.ratio[r];
            ratio[0].add(&[wl * total], &[wl]);
            let ai: Vec<f64> = (0..n).map(|k| wi[k] * rw[k]).collect();
            ratio[1].add(&ai, &wi[..n]);
            let ar: Vec<f64> = (0..n).map(|k| wr[k] * rw[k]).collect();
            ratio[2].add(&ar, &wr[..n]);
        }
    }

    /// Adds what another valuer of the same target (and models) saw.
    pub fn merge(&mut self, o: Valuer) {
        for r in 0..2 {
            for e in 0..8 {
                self.run[r][e].merge(&o.run[r][e]);
            }
            for e in 0..3 {
                self.ratio[r][e].merge(&o.ratio[r][e]);
            }
        }
        for x in 0..2 {
            self.w_list[x] += o.w_list[x];
            for k in 0..self.depth {
                self.w_row[k][x] += o.w_row[k][x];
                self.w_prefix[k][x] += o.w_prefix[k][x];
            }
        }
    }

    pub fn estimates(&self) -> Vec<Estimate> {
        let ess = |w: &[f64; 2]| if w[1] > 0.0 { w[0] * w[0] / w[1] } else { 0.0 };
        let shown = self.target.lists.iter().map(|l| l.len()).max().unwrap_or(0).min(self.depth);
        let thinnest = |ws: &[[f64; 2]]| ws[..shown].iter().map(ess).fold(f64::INFINITY, f64::min);
        let (list, row, prefix) = (Some(ess(&self.w_list)), Some(thinnest(&self.w_row)), Some(thinnest(&self.w_prefix)));
        let n = self.run[0][NAIVE].n.max(1.0);
        // A position's weights' mean, and its distance from 1 in standard
        // errors (none: no weights at all, as far from 1 as can be).
        let mz = |w: &[f64; 2]| {
            let m = w[0] / n;
            let se = ((w[1] / n - m * m).max(0.0) / n).sqrt();
            (m, if se > 0.0 { (m - 1.0) / se } else if m == 1.0 { 0.0 } else { f64::INFINITY.copysign(m - 1.0) })
        };
        let worst = |ws: &[[f64; 2]]| ws[..shown].iter().map(mz).fold((1.0, 0.0), |a: (f64, f64), b| if b.1.abs() > a.1.abs() { b } else { a });
        let some = |(m, z): (f64, f64)| (Some(m), Some(z));
        let (m_list, m_row, m_prefix) = (some(mz(&self.w_list)), some(worst(&self.w_row)), some(worst(&self.w_prefix)));
        let mut out = Vec::new();
        for (r, reward) in REWARDS.iter().enumerate() {
            let run = &self.run[r];
            let ratio = &self.ratio[r];
            let mut push = |estimator: &'static str, value: f64, se: f64, (ess, (weight_mean, weight_z)): (Option<f64>, (Option<f64>, Option<f64>))| out.push(Estimate { estimator, reward, value, se, ess, weight_mean, weight_z });
            let (list, row, prefix) = ((list, m_list), (row, m_row), (prefix, m_prefix));
            push("naive", run[NAIVE].mean, run[NAIVE].se(), (None, (None, None)));
            push("ips", run[IPS].mean, run[IPS].se(), list);
            push("snips", ratio[0].value(), ratio[0].se(), list);
            push("iips", run[IIPS].mean, run[IIPS].se(), row);
            push("sniips", ratio[1].value(), ratio[1].se(), row);
            push("rips", run[RIPS].mean, run[RIPS].se(), prefix);
            push("snrips", ratio[2].value(), ratio[2].se(), prefix);
            if run[DM_PBM].n > 0.0 {
                push("dm-pbm", run[DM_PBM].mean, run[DM_PBM].se(), (None, (None, None)));
                push("dm-sdbn", run[DM_SDBN].mean, run[DM_SDBN].se(), (None, (None, None)));
                push("dr-iips", run[DR_IIPS].mean, run[DR_IIPS].se(), row);
                push("cdr", run[CDR].mean, run[CDR].se(), prefix);
            }
        }
        out
    }
}

/// Which estimators the theory says are unbiased (as the log grows, for
/// the self-normalised ones) for a target the logger supports, under
/// click model `m`, and for a target that is not the logging policy.
pub fn should_hold(estimator: &str, m: crate::click::Model) -> bool {
    match estimator {
        "ips" | "snips" | "rips" | "snrips" | "cdr" => true,
        "iips" | "sniips" | "dr-iips" => m.item_position(),
        _ => false,
    }
}

/// Values every target over the impressions `imps`, with models fitted
/// on them by halves.
pub fn value_all(imps: &[Impression], targets: &[Target], depth: usize) -> Vec<Vec<Estimate>> {
    let mut counts = [Counts::default(), Counts::default()];
    for imp in imps {
        counts[fold(imp.i)].add(imp);
    }
    let models = Models::fit(&counts);
    targets
        .iter()
        .map(|t| {
            let mut v = Valuer::new(t, Some(&models), depth);
            imps.iter().for_each(|imp| v.add(imp));
            v.estimates()
        })
        .collect()
}

/// Whether policy `p` is one a target can be valued from: it explores.
pub fn explores(p: &Policy) -> bool {
    !matches!(p, Policy::None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ratio_and_its_error() {
        // One position: the self-normalised mean of a over b.
        let mut r = Ratio::new(1);
        let mut rng = crate::policy::Rng::seeded(4);
        let mut phis = Vec::new();
        let mut pairs = Vec::new();
        for _ in 0..5000 {
            let b = rng.uniform() * 2.0;
            let a = b * (0.3 + rng.uniform());
            r.add(&[a], &[b]);
            pairs.push((a, b));
        }
        let (sa, sb): (f64, f64) = pairs.iter().fold((0.0, 0.0), |s, p| (s.0 + p.0, s.1 + p.1));
        let v = sa / sb;
        assert!((r.value() - v).abs() < 1e-12);
        let bbar = sb / pairs.len() as f64;
        for (a, b) in &pairs {
            phis.push((a - v * b) / bbar);
        }
        // The influence's mean is 0: its mean square is its variance.
        let ms = phis.iter().map(|p| p * p).sum::<f64>() / phis.len() as f64;
        let want = (ms / phis.len() as f64).sqrt();
        assert!((r.se() - want).abs() < 1e-9 * want, "{} vs {want}", r.se());
    }
}
