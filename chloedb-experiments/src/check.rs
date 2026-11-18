//! The checks: every estimator against the truth the simulator knows,
//! over many independent logs of the same world, under each click model and
//! logging policy. An estimator the theory calls unbiased there must be
//! (its mean within three of its own standard errors of the truth) and its
//! 95% intervals must hold the truth at least 90% of the time; one the
//! theory says fails must fail, or the check says it did not. The logged
//! propensities must be the chances the lists were drawn with (a
//! chi-square over every query, row and position), and a swap policy's log
//! must measure position bias as PBM's users have it.

use std::collections::HashMap;

use serde::Serialize;
use serde_json::{json, Value};

use crate::click::Model;
use crate::estimate::{self, Estimate, Target};
use crate::policy::Policy;
use crate::server::{Result, Server};
use crate::sim::{self, Impression, Ranked, Truth, World};
use crate::stats;

/// The four rankings every check values, and a fifth the logger cannot
/// support. `rerank` holds, per query, the logging ranking's rows ordered
/// by another ranking (fetched from the server); `wide` that ranking's own
/// first rows over the whole table.
pub fn targets(w: &World, rerank: &[Vec<String>], wide: &[Vec<String>]) -> Vec<Target> {
    let oracle = sim::reordered(w, |q, r| {
        let mut idx: Vec<usize> = (0..r.keys.len()).collect();
        // By the row's grade, the ranking's order among equals.
        idx.sort_by(|&a, &b| w.site.slot(q, &r.keys[b]).attract.total_cmp(&w.site.slot(q, &r.keys[a]).attract).then(a.cmp(&b)));
        idx
    });
    let logging = sim::reordered(w, |_, r| (0..r.keys.len()).collect());
    let reverse = sim::reordered(w, |_, r| (0..r.keys.len()).rev().collect());
    vec![
        Target::new(w, "ranking", logging),
        Target::new(w, "rerank", rerank.to_vec()),
        Target::new(w, "oracle", oracle),
        Target::new(w, "reverse", reverse),
        Target::new(w, "wide", wide.to_vec()),
    ]
}

/// Per query, the logging ranking's rows reordered by `rank`, and `rank`'s
/// own first `limit` rows of the table.
pub async fn other_ranking(server: &Server, w: &World, rank: &str) -> Result<(Vec<Vec<String>>, Vec<Vec<String>>)> {
    let (mut rerank, mut wide) = (Vec::new(), Vec::new());
    for (q, r) in w.ranked.iter().enumerate() {
        let t = &w.site.queries[q].text;
        let ids = r.keys.iter().map(|k| crate::server::lit(k)).collect::<Vec<_>>().join(", ");
        let a = server.query(&format!("SELECT id FROM {} WHERE id IN ({ids}) ORDER BY {rank} DESC LIMIT {}", w.site.table, r.keys.len()), json!({"t": t}), None).await?;
        let mut got: Vec<String> = a["keys"].as_array().into_iter().flatten().map(sim::key_text).collect();
        // Rows it ranks alike keep the logging ranking's order; none is lost.
        for k in &r.keys {
            if !got.contains(k) {
                got.push(k.clone());
            }
        }
        rerank.push(got);
        let a = server.query(&format!("SELECT id FROM {} ORDER BY {rank} DESC LIMIT {}", w.site.table, w.limit), json!({"t": t}), None).await?;
        wide.push(a["keys"].as_array().into_iter().flatten().map(sim::key_text).collect());
    }
    Ok((rerank, wide))
}

/// An estimator's bias is judged when at least half its logs' weights
/// rest on this many effective impressions and average 1 within their
/// noise ([`Estimate::supported`]). On fewer, its estimates are mostly the
/// few lists that matched; on weights that do not average 1, the logs
/// rarely show what the target would, and a self-normalised estimator
/// then misses by about the share they miss.
pub const MIN_ESS_BIAS: f64 = 30.0;
/// Its intervals are judged on the logs whose estimate the app shows as a
/// result ([`Estimate::trusted`]), when there are this many: what the
/// app promises is that an interval it shows holds the truth 95% of the
/// time.
pub const MIN_TRUSTED: usize = 20;
/// An estimator's intervals fail when so few logs' intervals held the
/// truth that 95% intervals would do so this rarely (a one-sided binomial
/// test); its bias fails beyond this many standard errors. Both are set so
/// that the few thousand rows of a full check pass by chance when every
/// estimator is right.
pub const COVERAGE_P: f64 = 1e-3;
pub const BIAS_SE: f64 = 4.0;

#[derive(Clone, Debug, Serialize)]
pub struct Row {
    pub target: String,
    pub estimator: &'static str,
    pub reward: &'static str,
    pub truth: f64,
    pub mean: f64,
    pub bias: f64,
    /// The bias's standard error over the replications.
    pub bias_se: f64,
    pub rmse: f64,
    /// The estimator's own standard error, on average.
    pub se: f64,
    /// The share of replications whose 95% interval held the truth.
    pub coverage: f64,
    /// Its weights' effective impressions, on average over the logs.
    pub ess: Option<f64>,
    /// Its weights' mean at its worst position, on average over the logs.
    pub weight_mean: Option<f64>,
    /// The share of logs whose estimate the app would show as a result
    /// ([`Estimate::trusted`]), and how often those intervals held the
    /// truth.
    pub trusted: f64,
    pub trusted_coverage: Option<f64>,
    pub holds: bool,
    pub verdict: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct PropensityCheck {
    pub impressions: u64,
    /// Lists whose logged propensity is not the policy's marginal.
    pub mismatched: u64,
    pub chi2: f64,
    pub df: f64,
    pub p: f64,
    /// The largest standardised difference of a (query, row, position)'s
    /// count from its chance.
    pub max_z: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct CaseResult {
    pub site: String,
    pub model: &'static str,
    pub policy: Value,
    pub reps: usize,
    pub impressions: usize,
    pub targets: Vec<TargetInfo>,
    pub rows: Vec<Row>,
    pub propensity: PropensityCheck,
    pub seconds: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct TargetInfo {
    pub name: String,
    pub unsupported: f64,
    pub truth: Truth,
}

/// Counts of the rows logged at each position, per query.
#[derive(Default)]
struct Seen {
    per_query: HashMap<u32, f64>,
    cells: HashMap<(u32, u16, u8), f64>,
    mismatched: u64,
    n: u64,
}

impl Seen {
    fn add(&mut self, imp: &Impression, m: &[Vec<f64>]) {
        self.n += 1;
        *self.per_query.entry(imp.query).or_default() += 1.0;
        let mut bad = false;
        for (k, &i) in imp.shown.iter().enumerate() {
            *self.cells.entry((imp.query, i, k as u8)).or_default() += 1.0;
            let want = m[i as usize][k];
            bad |= (imp.propensity[k] - want).abs() > 1e-12 * want.max(1e-300);
        }
        self.mismatched += bad as u64;
    }

    fn merge(&mut self, o: Seen) {
        for (k, v) in o.per_query {
            *self.per_query.entry(k).or_default() += v;
        }
        for (k, v) in o.cells {
            *self.cells.entry(k).or_default() += v;
        }
        self.mismatched += o.mismatched;
        self.n += o.n;
    }

    /// Per (query, position), a chi-square of the rows' counts against
    /// the policy's marginals; rows expected fewer than 5 times pooled.
    fn check(&self, marginals: &[Vec<Vec<f64>>], limit: usize) -> PropensityCheck {
        let (mut chi2, mut df, mut max_z) = (0.0, 0.0, 0.0f64);
        for (&q, &nq) in &self.per_query {
            let m = &marginals[q as usize];
            for k in 0..limit {
                let (mut pooled_o, mut pooled_e, mut cells) = (0.0, 0.0, 0);
                for (i, row) in m.iter().enumerate() {
                    let p = row.get(k).copied().unwrap_or(0.0);
                    if p <= 0.0 {
                        continue;
                    }
                    let e = nq * p;
                    let o = self.cells.get(&(q, i as u16, k as u8)).copied().unwrap_or(0.0);
                    if e >= 5.0 {
                        chi2 += (o - e) * (o - e) / e;
                        cells += 1;
                        if p < 1.0 {
                            max_z = max_z.max((o - e).abs() / (e * (1.0 - p)).sqrt());
                        }
                    } else {
                        pooled_o += o;
                        pooled_e += e;
                    }
                }
                if pooled_e >= 5.0 {
                    chi2 += (pooled_o - pooled_e) * (pooled_o - pooled_e) / pooled_e;
                    cells += 1;
                }
                if cells > 1 {
                    df += (cells - 1) as f64;
                }
            }
        }
        PropensityCheck { impressions: self.n, mismatched: self.mismatched, chi2, df, p: stats::chi2_upper(chi2, df), max_z }
    }
}

/// Runs `reps` logs of `n` impressions of world `w` (fast mode, in
/// parallel), values every target on each, and compares with the truth.
pub fn run_case(w: &World, targets: &[Target], reps: usize, n: usize, label: &str) -> CaseResult {
    let started = std::time::Instant::now();
    let truths: Vec<Truth> = targets.iter().map(|t| w.truth(&t.keys, &|_| true)).collect();
    let marginals: Vec<Vec<Vec<f64>>> = w.ranked.iter().map(|r| w.policy.marginals(&r.scores).expect("marginals")).collect();
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(1, 32);
    let next = std::sync::atomic::AtomicUsize::new(0);
    type PerRep = Vec<Vec<Estimate>>;
    let (results, seen): (Vec<(usize, PerRep)>, Seen) = std::thread::scope(|s| {
        let hs: Vec<_> = (0..threads)
            .map(|_| {
                let (next, marginals) = (&next, &marginals);
                s.spawn(move || {
                    let mut out = Vec::new();
                    let mut seen = Seen::default();
                    loop {
                        let r = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if r >= reps {
                            break;
                        }
                        let run = format!("{label}-r{r}");
                        let imps: Vec<Impression> = (0..n as u64).map(|i| w.fast(&run, i)).collect();
                        for imp in &imps {
                            seen.add(imp, &marginals[imp.query as usize]);
                        }
                        out.push((r, estimate::value_all(&imps, targets, w.limit)));
                    }
                    (out, seen)
                })
            })
            .collect();
        let mut all = Vec::new();
        let mut seen = Seen::default();
        for h in hs {
            let (o, s) = h.join().expect("a check thread");
            all.extend(o);
            seen.merge(s);
        }
        (all, seen)
    });
    let mut rows = Vec::new();
    for (t, target) in targets.iter().enumerate() {
        let unsupported = target.unsupported(w);
        for (e_at, est) in results[0].1[t].iter().enumerate() {
            let truth = truths[t].metric(est.reward);
            let vals: Vec<&Estimate> = results.iter().map(|(_, r)| &r[t][e_at]).collect();
            let xs: Vec<f64> = vals.iter().map(|e| e.value).collect();
            let (mean, sd) = stats::mean_sd(&xs);
            let bias = mean - truth;
            let bias_se = sd / (xs.len() as f64).sqrt();
            let rmse = (xs.iter().map(|x| (x - truth) * (x - truth)).sum::<f64>() / xs.len() as f64).sqrt();
            let se = vals.iter().map(|e| e.se).sum::<f64>() / vals.len() as f64;
            let coverage = vals.iter().filter(|e| e.covers(truth)).count() as f64 / vals.len() as f64;
            let ess = est.ess.map(|_| vals.iter().filter_map(|e| e.ess).sum::<f64>() / vals.len() as f64);
            let weight_mean = est.weight_mean.map(|_| vals.iter().filter_map(|e| e.weight_mean).sum::<f64>() / vals.len() as f64);
            // The logs whose weights can say anything of its bias: they
            // average 1 within their noise, on a few dozen impressions.
            let supported = vals.iter().filter(|e| e.supported() && e.ess.is_none_or(|x| x >= MIN_ESS_BIAS)).count() as f64 / vals.len() as f64;
            let thin = supported < 0.5;
            let shown: Vec<&&Estimate> = vals.iter().filter(|e| e.trusted()).collect();
            let trusted = shown.len() as f64 / vals.len() as f64;
            let held = shown.iter().filter(|e| e.covers(truth)).count() as u64;
            let trusted_coverage = (!shown.is_empty()).then(|| held as f64 / shown.len() as f64);
            let thin_ci = shown.len() < MIN_TRUSTED;
            let holds = unsupported == 0.0 && estimate::should_hold(est.estimator, w.model);
            let biased = bias.abs() > BIAS_SE * bias_se + 0.002 * truth.abs();
            let covers_too_rarely = stats::binom_cdf(held, shown.len() as u64, 0.95) < COVERAGE_P;
            let verdict = if unsupported > 0.0 && est.estimator != "naive" && !est.estimator.starts_with("dm") {
                if biased { format!("biased, as expected: {:.0}% of its slots unsupported", unsupported * 100.0) } else { "unbiased though unsupported".into() }
            } else if holds && thin {
                format!("not judged: the logs rarely show its lists (weights on {:.0} impressions, mean {:.2}; {:.0}% of logs usable)", ess.unwrap_or(0.0), weight_mean.unwrap_or(f64::NAN), supported * 100.0)
            } else if holds {
                let cover_ok = thin_ci || !covers_too_rarely;
                match (biased, cover_ok) {
                    (false, true) => "ok".into(),
                    (true, _) => "FAIL: biased".into(),
                    (false, false) => format!("FAIL: the intervals it shows hold the truth {:.0}% of the time", trusted_coverage.unwrap_or(0.0) * 100.0),
                }
            } else if est.estimator.starts_with("dm") {
                if coverage < 0.9 { "intervals too narrow, as expected of a model".into() } else { "model right here".into() }
            } else if biased {
                "biased, as expected".into()
            } else {
                "unbiased here".into()
            };
            let verdict = if verdict == "ok" && thin_ci { format!("unbiased; the app warns on {:.0}% of its logs, so its intervals are not judged", (1.0 - trusted) * 100.0) } else { verdict };
            rows.push(Row { target: target.name.clone(), estimator: est.estimator, reward: est.reward, truth, mean, bias, bias_se, rmse, se, coverage, ess, weight_mean, trusted, trusted_coverage, holds, verdict });
        }
    }
    let infos = targets.iter().zip(&truths).map(|(t, tr)| TargetInfo { name: t.name.clone(), unsupported: t.unsupported(w), truth: tr.clone() }).collect();
    CaseResult {
        site: w.site.name.clone(),
        model: w.model.name(),
        policy: w.policy.to_json(),
        reps,
        impressions: n,
        targets: infos,
        rows,
        propensity: seen.check(&marginals, w.limit),
        seconds: started.elapsed().as_secs_f64(),
    }
}

impl CaseResult {
    /// The rows that fail, and a failed propensity check.
    pub fn failures(&self) -> Vec<String> {
        let mut out: Vec<String> = self.rows.iter().filter(|r| r.verdict.starts_with("FAIL")).map(|r| format!("{} {} {} {}: {} (bias {:+.5} ± {:.5}, coverage {:.2})", self.model, self.site, r.target, r.estimator, r.verdict, r.bias, r.bias_se, r.coverage)).collect();
        let p = &self.propensity;
        if p.mismatched > 0 {
            out.push(format!("{} {}: {} lists logged a propensity that is not the policy's", self.model, self.site, p.mismatched));
        }
        if p.df > 0.0 && p.p < 1e-4 {
            out.push(format!("{} {}: the rows logged at each position are not the policy's chances (chi-square {:.0} on {:.0} df, p {:.2e})", self.model, self.site, p.chi2, p.df, p.p));
        }
        out
    }
}

/// Position bias from a swap policy's log under PBM: the clicks on the
/// ranking's first row at each position over its clicks at the top, with
/// a delta-method standard error, against the truth.
#[derive(Clone, Debug, Serialize)]
pub struct SwapCheck {
    pub positions: Vec<SwapAt>,
    pub impressions: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct SwapAt {
    pub position: usize,
    pub truth: f64,
    pub estimate: f64,
    pub se: f64,
}

pub fn swap_check(w: &World, n: usize, label: &str) -> SwapCheck {
    assert!(matches!(w.policy, Policy::Swap { .. }) && w.model == Model::Pbm);
    let truth = w.first_row_clicks();
    let mut at = vec![(0.0f64, 0.0f64); w.limit];
    for i in 0..n as u64 {
        let imp = w.fast(label, i);
        if let Some(k) = imp.shown.iter().position(|&x| x == 0) {
            at[k].0 += 1.0;
            at[k].1 += imp.click[k] as u8 as f64;
        }
    }
    let (n0, c0) = at[0];
    let p0 = c0 / n0;
    let positions = (0..w.limit)
        .map(|k| {
            let (nk, ck) = at[k];
            let pk = ck / nk;
            // Var(pk/p0) ≈ (pk/p0)² (Var pk/pk² + Var p0/p0²).
            let r = pk / p0;
            let se = r * ((1.0 - pk) / (nk * pk) + (1.0 - p0) / (n0 * p0)).sqrt();
            SwapAt { position: k, truth: truth[k] / truth[0], estimate: r, se }
        })
        .collect();
    SwapCheck { positions, impressions: n as u64 }
}

impl SwapCheck {
    pub fn failures(&self) -> Vec<String> {
        self.positions.iter().filter(|p| p.position > 0 && (p.estimate - p.truth).abs() > 3.0 * p.se).map(|p| format!("swap: position bias at {} measured {:.4} ± {:.4}, truth {:.4}", p.position, p.estimate, p.se, p.truth)).collect()
    }
}

/// A Markdown report of the cases.
pub fn markdown(cases: &[CaseResult], swaps: &[(String, SwapCheck)], note: &str) -> String {
    let mut s = String::new();
    s.push_str("# Off-policy estimators against the truth\n\n");
    s.push_str(note);
    s.push_str("\n\n");
    s.push_str("Each case is one world (a collection as the site, a click model, a logging policy); each row is one estimator of one target ranking's clicks or satisfied clicks per impression, over independent logs. *bias* is the mean estimate less the truth, ± its standard error over the logs; *coverage* is the share of logs whose 95% interval held the truth. *holds* says whether the theory calls the estimator unbiased there.\n\n");
    let fails: Vec<String> = cases.iter().flat_map(|c| c.failures()).chain(swaps.iter().flat_map(|(_, s)| s.failures())).collect();
    if fails.is_empty() {
        s.push_str("**Every check passed.**\n\n");
    } else {
        s.push_str(&format!("**{} checks failed:**\n\n", fails.len()));
        for f in &fails {
            s.push_str(&format!("- {f}\n"));
        }
        s.push('\n');
    }
    for c in cases {
        s.push_str(&format!("## {} · {} · {}\n\n", c.site, c.model, c.policy));
        s.push_str(&format!("{} logs of {} impressions, {:.0} s. ", c.reps, c.impressions, c.seconds));
        let p = &c.propensity;
        s.push_str(&format!("Propensities: {} lists, {} mismatched; chi-square {:.0} on {:.0} df (p {:.3}), largest |z| {:.2}.\n\n", p.impressions, p.mismatched, p.chi2, p.df, p.p, p.max_z));
        s.push_str("| target | unsupported | clicks | satisfied | success | session |\n|---|---|---|---|---|---|\n");
        for t in &c.targets {
            s.push_str(&format!("| {} | {:.1}% | {:.4} | {:.4} | {:.4} | {:.3} |\n", t.name, t.unsupported * 100.0, t.truth.clicks, t.truth.satisfied, t.truth.success, t.truth.session));
        }
        s.push_str("\n| target | reward | estimator | truth | bias ± se | rmse | coverage | ess | mean weight | shown | shown coverage | holds | verdict |\n|---|---|---|---|---|---|---|---|---|---|---|---|---|\n");
        for r in &c.rows {
            let ess = r.ess.map(|e| format!("{e:.0}")).unwrap_or_default();
            let wm = r.weight_mean.map(|e| format!("{e:.3}")).unwrap_or_default();
            let tc = r.trusted_coverage.map(|e| format!("{e:.2}")).unwrap_or_default();
            s.push_str(&format!("| {} | {} | {} | {:.4} | {:+.5} ± {:.5} | {:.5} | {:.2} | {ess} | {wm} | {:.0}% | {tc} | {} | {} |\n", r.target, r.reward, r.estimator, r.truth, r.bias, r.bias_se, r.rmse, r.coverage, r.trusted * 100.0, if r.holds { "yes" } else { "" }, r.verdict));
        }
        s.push('\n');
    }
    for (name, sw) in swaps {
        s.push_str(&format!("## Position bias from a swap policy: {name}\n\n{} impressions.\n\n| position | truth | measured | se |\n|---|---|---|---|\n", sw.impressions));
        for p in &sw.positions {
            s.push_str(&format!("| {} | {:.4} | {:.4} | {:.4} |\n", p.position, p.truth, p.estimate, p.se));
        }
        s.push('\n');
    }
    s
}

/// A world's fetched state, kept so a check runs again without a server:
/// the site's ranked rows and the other ranking's lists.
#[derive(Clone, Debug, Serialize, serde::Deserialize)]
pub struct Fetched {
    /// The site's queries kept (by id), in order.
    pub queries: Vec<String>,
    pub ranked: Vec<(Vec<String>, Vec<Option<f64>>)>,
    pub rerank: Vec<Vec<String>>,
    pub wide: Vec<Vec<String>>,
}

impl Fetched {
    pub fn ranked(&self) -> Vec<Ranked> {
        self.ranked.iter().map(|(k, s)| Ranked { keys: k.clone(), scores: s.clone() }).collect()
    }
}
