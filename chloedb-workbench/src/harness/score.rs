//! The paired scorecard of a replay (HARNESS.md, "The scorecard"): A and B
//! compared case by case, so each case is its own control.
//!
//! - A case passes under a version only when every one of its runs passes
//!   (pass^k): one lucky run does not count.
//! - Each metric's difference is the median over cases of B's median minus
//!   A's, with a 90% interval by paired bootstrap over cases (a fixed seed,
//!   so the same runs always give the same interval).
//! - The noise floor of a metric is how much it moves between runs of A on
//!   the same case (the median over cases of A's spread). A difference
//!   smaller than that is noise, whatever its sign.
//! - *Better* needs the interval to exclude 0 in the good direction and the
//!   difference to exceed the floor; *worse* needs only the interval to
//!   exclude 0 in the bad direction (so it is harder to win than to lose).
//!   Neither counts a difference under what can be measured at all
//!   ([`RESOLUTION`]: a replay's wall time is read every 300 ms).
//! - A stretch has runs, not cases, and with two a side one version's runs
//!   all land above the other's one time in six by chance alone: there a
//!   worse difference must exceed the floor too.
//! - Success compares the paired pass^k counts (McNemar's exact test on the
//!   discordant cases). A case that passed under A and fails under B is a
//!   regression, and any regression rejects B, on train or held out.
//!
//! Every metric here is better lower. "No detectable difference" is a
//! result, and so is "too few cases to tell".

use serde_json::{json, Value as Json};

/// Fewer complete cases than this: too few to tell (HARNESS.md, Risks).
pub const MIN_CASES: usize = 6;
/// Fewer train cases than this: too few for an interval.
const MIN_TRAIN: usize = 3;
/// Bootstrap resamples.
const RESAMPLES: usize = 10_000;
/// The two-sided level of the intervals and of McNemar's test.
const ALPHA: f64 = 0.10;

/// The metrics, each better lower: key, label, unit.
pub const METRICS: [(&str, &str, &str); 6] = [
    ("interventions", "Interventions", "a run"),
    ("prompts", "Permission prompts", "a run"),
    ("turns", "Turns", "a run"),
    ("wall_s", "Wall time", "s"),
    ("tokens", "Tokens", "a run"),
    ("cost_usd", "Cost", "$ a run"),
];

/// The smallest difference of each metric that is measured at all, by
/// [`METRICS`]' order: wall time is read every 300 ms, so under a second
/// it is the clock's, not the agent's.
pub const RESOLUTION: [f64; 6] = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// The metrics a stretch's change is rejected on when it is worse.
pub const DECIDES: [&str; 3] = ["interventions", "wall_s", "cost_usd"];

/// One run's result.
#[derive(Clone, Debug, Default)]
pub struct RunM {
    pub passed: bool,
    /// It met something the log did not have (a question or a command the
    /// original never had): scored apart from failure.
    pub diverged: bool,
    /// By [`METRICS`]' order.
    pub m: [f64; 6],
}

/// One case's runs under each version.
#[derive(Clone, Debug, Default)]
pub struct CaseRuns {
    pub case: String,
    pub name: String,
    pub held_out: bool,
    pub a: Vec<RunM>,
    pub b: Vec<RunM>,
}

/// A target of "Optimize this": a metric (a key of [`METRICS`], or
/// `success`) and by how much it must improve, as a share of A's value.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PartialEq)]
pub struct Target {
    pub metric: String,
    #[serde(default = "default_pct")]
    pub pct: f64,
}

fn default_pct() -> f64 {
    30.0
}

/// Whether a target is on the pass rate (`pass_rate`, or `success`).
fn is_pass_rate(metric: &str) -> bool {
    matches!(metric, "pass_rate" | "success")
}

/// The keys a target may name.
pub fn target_metric(metric: &str) -> bool {
    is_pass_rate(metric) || METRICS.iter().any(|(k, _, _)| *k == metric)
}

fn median(v: &mut [f64]) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = v.len();
    Some(if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 })
}

fn quantile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let pos = q * (sorted.len() - 1) as f64;
    let (lo, hi) = (pos.floor() as usize, pos.ceil() as usize);
    sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo as f64)
}

/// A small fixed-seed generator (splitmix64): the same runs give the same
/// interval on every reading.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// The median of `diffs` and its paired bootstrap interval at 1 − ALPHA.
pub fn bootstrap(diffs: &[f64]) -> Option<(f64, f64, f64)> {
    let d = median(&mut diffs.to_vec())?;
    let mut rng = Rng(0x5EED_CA5E);
    let mut meds = Vec::with_capacity(RESAMPLES);
    let mut buf = vec![0.0; diffs.len()];
    for _ in 0..RESAMPLES {
        for x in buf.iter_mut() {
            *x = diffs[rng.below(diffs.len())];
        }
        meds.push(median(&mut buf).unwrap_or(0.0));
    }
    meds.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some((d, quantile(&meds, ALPHA / 2.0), quantile(&meds, 1.0 - ALPHA / 2.0)))
}

/// McNemar's exact two-sided p for `b` cases only B passes and `c` only A
/// passes.
pub fn mcnemar(b: usize, c: usize) -> f64 {
    let n = b + c;
    if n == 0 {
        return 1.0;
    }
    let k = b.min(c);
    // P(X <= k), X ~ Binomial(n, 1/2), summed in logs for large n.
    let ln_choose = |n: usize, i: usize| -> f64 { (1..=i).map(|j| ((n - i + j) as f64).ln() - (j as f64).ln()).sum() };
    let tail: f64 = (0..=k).map(|i| (ln_choose(n, i) - n as f64 * std::f64::consts::LN_2).exp()).sum();
    (2.0 * tail).min(1.0)
}

/// A case's pass^k under each version, when both sides have `n` runs.
fn passes(c: &CaseRuns, n: usize) -> Option<(bool, bool)> {
    if c.a.len() < n || c.b.len() < n || n == 0 {
        return None;
    }
    Some((c.a.iter().all(|r| r.passed), c.b.iter().all(|r| r.passed)))
}

fn med_of(runs: &[RunM], i: usize) -> f64 {
    median(&mut runs.iter().map(|r| r.m[i]).collect::<Vec<_>>()).unwrap_or(0.0)
}

fn spread_of(runs: &[RunM], i: usize) -> Option<f64> {
    if runs.len() < 2 {
        return None;
    }
    let (lo, hi) = runs.iter().map(|r| r.m[i]).fold((f64::MAX, f64::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)));
    Some(hi - lo)
}

fn round(x: f64) -> f64 {
    if x.abs() >= 100.0 {
        x.round()
    } else if x.abs() >= 1.0 {
        (x * 100.0).round() / 100.0
    } else {
        (x * 10_000.0).round() / 10_000.0
    }
}

/// The verdict of one metric over `cases` (complete ones), lower better.
fn metric_verdict(cases: &[&CaseRuns], i: usize, n: usize, enough: bool) -> Json {
    let diffs: Vec<f64> = cases.iter().map(|c| med_of(&c.b, i) - med_of(&c.a, i)).collect();
    let a = median(&mut cases.iter().map(|c| med_of(&c.a, i)).collect::<Vec<_>>());
    let b = median(&mut cases.iter().map(|c| med_of(&c.b, i)).collect::<Vec<_>>());
    let noise = median(&mut cases.iter().filter_map(|c| spread_of(&c.a, i)).collect::<Vec<_>>());
    let Some((d, lo, hi)) = bootstrap(&diffs) else {
        return json!({ "verdict": "too_few", "cases": 0 });
    };
    let verdict = if !enough || n < 2 {
        "too_few"
    } else if lo > 0.0 && d > RESOLUTION[i] {
        "worse"
    } else if hi < 0.0 && -d > noise.unwrap_or(0.0).max(RESOLUTION[i]) {
        "better"
    } else {
        "noise"
    };
    json!({
        "a": a.map(round), "b": b.map(round), "delta": round(d), "lo": round(lo), "hi": round(hi), "noise": noise.map(round),
        "pct": a.filter(|a| *a > 0.0).map(|a| round(-d / a * 100.0)), "verdict": verdict, "cases": cases.len(),
    })
}

/// The scorecard of a run: `n` runs a side were asked for; `finished`
/// says whether more runs will come.
pub fn scorecard(cases: &[CaseRuns], n: usize, finished: bool, targets: &[Target]) -> Json {
    let complete: Vec<&CaseRuns> = cases.iter().filter(|c| passes(c, n).is_some()).collect();
    let train: Vec<&CaseRuns> = complete.iter().copied().filter(|c| !c.held_out).collect();
    let held: Vec<&CaseRuns> = complete.iter().copied().filter(|c| c.held_out).collect();
    let enough = complete.len() >= MIN_CASES && train.len() >= MIN_TRAIN;

    // Each case's line.
    let lines: Vec<Json> = cases
        .iter()
        .map(|c| {
            let p = passes(c, n);
            let m = |runs: &[RunM]| -> Json {
                let mut o = serde_json::Map::new();
                for (i, (k, _, _)) in METRICS.iter().enumerate() {
                    if !runs.is_empty() {
                        o.insert((*k).into(), json!(round(med_of(runs, i))));
                    }
                }
                Json::Object(o)
            };
            json!({
                "case": c.case, "name": c.name, "set": if c.held_out { "held_out" } else { "train" },
                "a_runs": c.a.len(), "b_runs": c.b.len(), "a_passed": c.a.iter().filter(|r| r.passed).count(), "b_passed": c.b.iter().filter(|r| r.passed).count(),
                "a_pass": p.map(|x| x.0), "b_pass": p.map(|x| x.1), "regression": p.is_some_and(|(a, b)| a && !b), "gained": p.is_some_and(|(a, b)| !a && b),
                "a": m(&c.a), "b": m(&c.b),
            })
        })
        .collect();

    // Success, pass^k, paired.
    let count = |cs: &[&CaseRuns], side: usize| cs.iter().filter(|c| passes(c, n).is_some_and(|p| if side == 0 { p.0 } else { p.1 })).count();
    let regressions: Vec<Json> = complete.iter().filter(|c| passes(c, n).is_some_and(|(a, b)| a && !b)).map(|c| json!({ "case": c.case, "name": c.name, "set": if c.held_out { "held_out" } else { "train" } })).collect();
    let gained = complete.iter().filter(|c| passes(c, n).is_some_and(|(a, b)| !a && b)).count();
    let p = mcnemar(gained, regressions.len());
    let success_verdict = if !regressions.is_empty() {
        "worse"
    } else if !enough || n < 2 {
        "too_few"
    } else if gained > 0 && p < ALPHA {
        "better"
    } else {
        "noise"
    };
    let success = json!({
        "train": { "a": count(&train, 0), "b": count(&train, 1), "cases": train.len() },
        "held_out": { "a": count(&held, 0), "b": count(&held, 1), "cases": held.len() },
        "a": count(&complete, 0), "b": count(&complete, 1), "cases": complete.len(),
        "gained": gained, "lost": regressions.len(), "p": round(p), "verdict": success_verdict,
    });

    // The guardrails: verdicts on train, and worse-or-not on held out.
    let metrics: Vec<Json> = METRICS
        .iter()
        .enumerate()
        .map(|(i, (k, label, unit))| {
            let mut v = if train.is_empty() { json!({ "verdict": "too_few", "cases": 0 }) } else { metric_verdict(&train, i, n, enough) };
            let h = if held.len() >= 2 { metric_verdict(&held, i, n, true) } else { json!({ "verdict": "too_few", "cases": held.len() }) };
            v["key"] = json!(k);
            v["label"] = json!(label);
            v["unit"] = json!(unit);
            v["held_out"] = json!({ "verdict": h["verdict"], "delta": h["delta"], "cases": held.len() });
            v
        })
        .collect();

    // Divergence, apart from failure.
    let div = |side: usize| -> Json {
        let runs: Vec<&RunM> = cases.iter().flat_map(|c| if side == 0 { c.a.iter() } else { c.b.iter() }).collect();
        json!({ "runs": runs.len(), "diverged": runs.iter().filter(|r| r.diverged).count() })
    };

    // The decision, by HARNESS.md's keep rule.
    let mut why: Vec<String> = Vec::new();
    let worse: Vec<&Json> = metrics.iter().filter(|m| m["verdict"] == "worse" || m["held_out"]["verdict"] == "worse").collect();
    let better: Vec<&Json> = metrics.iter().filter(|m| m["verdict"] == "better").collect();
    let result = if !finished {
        "running"
    } else if !regressions.is_empty() {
        for r in &regressions {
            why.push(format!("{} passed all {n} runs under A and failed under B{}", r["name"].as_str().unwrap_or(""), if r["set"] == "held_out" { " (held out)" } else { "" }));
        }
        "reject"
    } else if !worse.is_empty() {
        for m in &worse {
            why.push(format!("{} is worse beyond the noise{}", m["label"].as_str().unwrap_or(""), if m["held_out"]["verdict"] == "worse" && m["verdict"] != "worse" { " on the held-out cases" } else { "" }));
        }
        "reject"
    } else if !enough || n < 2 {
        if n < 2 {
            why.push("One run a side measures no noise floor: run at least 2".into());
        }
        if complete.len() < MIN_CASES {
            why.push(format!("{} complete case{} is too few to tell: {MIN_CASES} are needed", complete.len(), if complete.len() == 1 { "" } else { "s" }));
        } else if train.len() < MIN_TRAIN {
            why.push(format!("{} train case{} is too few for an interval: {MIN_TRAIN} are needed", train.len(), if train.len() == 1 { "" } else { "s" }));
        }
        "too_few"
    } else if success_verdict == "better" || !better.is_empty() {
        if success_verdict == "better" {
            why.push(format!("B passes {gained} case{} A failed, and none A passed fails", if gained == 1 { "" } else { "s" }));
        }
        for m in &better {
            why.push(format!("{} is better beyond the noise: {} against {}", m["label"].as_str().unwrap_or(""), m["b"], m["a"]));
        }
        "keep"
    } else {
        why.push("Every metric is inside the noise of A against A".into());
        "no_detectable_difference"
    };

    // The targets of Optimize this: each met or not.
    let tj: Vec<Json> = targets
        .iter()
        .map(|t| {
            let met = if is_pass_rate(&t.metric) {
                success_verdict == "better"
            } else {
                metrics.iter().find(|m| m["key"] == t.metric.as_str()).is_some_and(|m| m["verdict"] == "better" && m["pct"].as_f64().is_some_and(|p| p >= t.pct))
            };
            let now = if is_pass_rate(&t.metric) { success["verdict"].clone() } else { metrics.iter().find(|m| m["key"] == t.metric.as_str()).map(|m| m["pct"].clone()).unwrap_or(Json::Null) };
            json!({ "metric": t.metric, "pct": t.pct, "met": met, "now": now })
        })
        .collect();
    let all_met = !tj.is_empty() && tj.iter().all(|t| t["met"] == true) && result == "keep";

    json!({
        "n": n, "cases": lines, "complete": complete.len(), "train": train.len(), "held_out": held.len(), "min_cases": MIN_CASES,
        "success": success, "regressions": regressions, "metrics": metrics,
        "divergence": { "a": div(0), "b": div(1) },
        "decision": { "result": result, "why": why },
        "targets": tj, "targets_met": all_met,
    })
}

/// B's median minus A's over their runs, with a bootstrap interval that
/// resamples each side's runs apart (one stretch has no cases to pair).
fn bootstrap_runs(a: &[f64], b: &[f64]) -> Option<(f64, f64, f64)> {
    let d = median(&mut b.to_vec())? - median(&mut a.to_vec())?;
    let mut rng = Rng(0x5EED_CA5E);
    let (mut ba, mut bb) = (vec![0.0; a.len()], vec![0.0; b.len()]);
    let mut meds = Vec::with_capacity(RESAMPLES);
    for _ in 0..RESAMPLES {
        for x in ba.iter_mut() {
            *x = a[rng.below(a.len())];
        }
        for x in bb.iter_mut() {
            *x = b[rng.below(b.len())];
        }
        meds.push(median(&mut bb).unwrap_or(0.0) - median(&mut ba).unwrap_or(0.0));
    }
    meds.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some((d, quantile(&meds, ALPHA / 2.0), quantile(&meds, 1.0 - ALPHA / 2.0)))
}

/// Fisher's exact one-sided p that B passes `pb` of `nb` runs or more by
/// chance, when A passed `pa` of `na`.
pub fn fisher_greater(pa: usize, na: usize, pb: usize, nb: usize) -> f64 {
    let ln_fact = |n: usize| -> f64 { (2..=n).map(|i| (i as f64).ln()).sum() };
    let ln_choose = |n: usize, k: usize| -> f64 { if k > n { f64::NEG_INFINITY } else { ln_fact(n) - ln_fact(k) - ln_fact(n - k) } };
    let (n, k) = (na + nb, pa + pb);
    if n == 0 {
        return 1.0;
    }
    let total = ln_choose(n, nb);
    let p: f64 = (pb..=nb.min(k)).map(|x| (ln_choose(k, x) + ln_choose(n - k, nb - x) - total).exp()).sum();
    p.min(1.0)
}

/// The scorecard of one stretch of a session ("Optimize this"): its runs
/// are the unit, not cases, so it says whether B is better on this stretch,
/// not whether it generalizes. A passes when every one of its runs passes,
/// and B failing a run of a stretch A always passed is a regression.
pub fn stretch(c: &CaseRuns, n: usize, finished: bool, targets: &[Target]) -> Json {
    let complete = n > 0 && c.a.len() >= n && c.b.len() >= n;
    let enough = complete && n >= 2;
    let (pa, pb) = (c.a.iter().filter(|r| r.passed).count(), c.b.iter().filter(|r| r.passed).count());
    let rate = |p: usize, of: usize| if of == 0 { None } else { Some(p as f64 / of as f64) };
    let (ra, rb) = (rate(pa, c.a.len()), rate(pb, c.b.len()));
    // Certain as soon as one run of B fails, once A has all of its runs.
    let regression = c.a.len() >= n && n > 0 && pa == c.a.len() && pb < c.b.len();
    let p = fisher_greater(pa, c.a.len(), pb, c.b.len());
    let success_verdict = if regression || (complete && rb < ra && fisher_greater(pb, c.b.len(), pa, c.a.len()) < ALPHA) {
        "worse"
    } else if !enough {
        "too_few"
    } else if rb > ra && p < ALPHA {
        "better"
    } else {
        "noise"
    };
    let success = json!({ "a": pa, "b": pb, "a_runs": c.a.len(), "b_runs": c.b.len(), "a_rate": ra.map(round), "b_rate": rb.map(round), "p": round(p), "verdict": success_verdict, "gained": usize::from(!c.a.is_empty() && pa < c.a.len() && pb == c.b.len() && complete), "lost": usize::from(regression) });
    let metrics: Vec<Json> = METRICS
        .iter()
        .enumerate()
        .map(|(i, (k, label, unit))| {
            let a: Vec<f64> = c.a.iter().map(|r| r.m[i]).collect();
            let b: Vec<f64> = c.b.iter().map(|r| r.m[i]).collect();
            let mut v = match bootstrap_runs(&a, &b) {
                None => json!({ "verdict": "too_few", "runs": 0 }),
                Some((d, lo, hi)) => {
                    let am = median(&mut a.clone());
                    let noise = spread_of(&c.a, i);
                    let floor = noise.unwrap_or(0.0).max(RESOLUTION[i]);
                    let verdict = if !enough {
                        "too_few"
                    } else if lo > 0.0 && d > floor {
                        "worse"
                    } else if hi < 0.0 && -d > floor {
                        "better"
                    } else {
                        "noise"
                    };
                    json!({
                        "a": am.map(round), "b": median(&mut b.clone()).map(round), "delta": round(d), "lo": round(lo), "hi": round(hi), "noise": noise.map(round),
                        "pct": am.filter(|a| *a > 0.0).map(|a| round(-d / a * 100.0)), "verdict": verdict, "runs": a.len().min(b.len()),
                    })
                }
            };
            v["key"] = json!(k);
            v["label"] = json!(label);
            v["unit"] = json!(unit);
            v
        })
        .collect();
    let mut why: Vec<String> = Vec::new();
    // What you pay decides a reject: your interventions, the time and the
    // money. More turns or tokens for less time and money (a faster model
    // that takes more steps) is still better.
    let worse: Vec<&Json> = metrics.iter().filter(|m| m["verdict"] == "worse" && DECIDES.contains(&m["key"].as_str().unwrap_or(""))).collect();
    let better: Vec<&Json> = metrics.iter().filter(|m| m["verdict"] == "better").collect();
    let result = if regression {
        why.push(format!("A passed all {} of its runs and B failed {} of {}", c.a.len(), c.b.len() - pb, c.b.len()));
        "reject"
    } else if !complete && !finished {
        "running"
    } else if !worse.is_empty() || success_verdict == "worse" {
        for m in &worse {
            why.push(format!("{} is worse beyond the noise", m["label"].as_str().unwrap_or("")));
        }
        if success_verdict == "worse" {
            why.push(format!("B passed {pb} of {} runs, A {pa} of {}", c.b.len(), c.a.len()));
        }
        "reject"
    } else if !enough {
        why.push(if n < 2 { "One run a side measures no noise floor: run at least 2".to_string() } else { format!("{} of A's and {} of B's {n} runs finished: too few to tell", c.a.len(), c.b.len()) });
        "too_few"
    } else if success_verdict == "better" || !better.is_empty() {
        if success_verdict == "better" {
            why.push(format!("B passed {pb} of {} runs, A {pa} of {}", c.b.len(), c.a.len()));
        }
        for m in &better {
            why.push(format!("{} is better beyond the noise: {} against {}", m["label"].as_str().unwrap_or(""), m["b"], m["a"]));
        }
        "keep"
    } else {
        why.push("Every metric is inside the noise of A's runs".into());
        "no_detectable_difference"
    };
    let tj: Vec<Json> = targets
        .iter()
        .map(|t| {
            let (met, now) = if is_pass_rate(&t.metric) {
                // In points of pass rate: 30 asks B to pass 30 more runs in
                // 100 than A, or all of them.
                let want = ra.map(|a| (a + t.pct / 100.0).min(1.0));
                (enough && !regression && rb.zip(want).is_some_and(|(b, w)| b + 1e-9 >= w), json!(rb.zip(ra).map(|(b, a)| round((b - a) * 100.0))))
            } else {
                let m = metrics.iter().find(|m| m["key"] == t.metric.as_str());
                (m.is_some_and(|m| m["verdict"] == "better" && m["pct"].as_f64().is_some_and(|p| p >= t.pct)), m.map(|m| m["pct"].clone()).unwrap_or(Json::Null))
            };
            json!({ "metric": t.metric, "pct": t.pct, "met": met, "now": now })
        })
        .collect();
    let all_met = !tj.is_empty() && tj.iter().all(|t| t["met"] == true) && result != "reject" && result != "running";
    let line = |runs: &[RunM]| -> Json {
        let mut o = serde_json::Map::new();
        for (i, (k, _, _)) in METRICS.iter().enumerate() {
            if !runs.is_empty() {
                o.insert((*k).into(), json!(round(med_of(runs, i))));
            }
        }
        Json::Object(o)
    };
    json!({
        "unit": "runs", "n": n, "complete": usize::from(complete),
        "cases": [{ "case": c.case, "name": c.name, "set": if c.held_out { "held_out" } else { "train" }, "a_runs": c.a.len(), "b_runs": c.b.len(), "a_passed": pa, "b_passed": pb, "a_pass": complete.then_some(pa == c.a.len()), "b_pass": complete.then_some(pb == c.b.len()), "regression": regression, "a": line(&c.a), "b": line(&c.b) }],
        "success": success, "regressions": if regression { json!([{ "case": c.case, "name": c.name }]) } else { json!([]) }, "metrics": metrics,
        "divergence": { "a": { "runs": c.a.len(), "diverged": c.a.iter().filter(|r| r.diverged).count() }, "b": { "runs": c.b.len(), "diverged": c.b.iter().filter(|r| r.diverged).count() } },
        "decision": { "result": result, "why": why },
        "targets": tj, "targets_met": all_met,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(passed: bool, interventions: f64, wall: f64) -> RunM {
        RunM { passed, diverged: false, m: [interventions, interventions, 3.0, wall, 1000.0, 0.1] }
    }

    fn case(i: usize, held: bool, a: Vec<RunM>, b: Vec<RunM>) -> CaseRuns {
        CaseRuns { case: format!("c{i}"), name: format!("case {i}"), held_out: held, a, b }
    }

    /// Eight cases, B with 4 fewer interventions on each, A's runs 1 apart.
    fn eight(b_shift: f64) -> Vec<CaseRuns> {
        (0..8)
            .map(|i| {
                let base = 6.0 + i as f64;
                case(i, i % 3 == 2, vec![run(true, base, 100.0), run(true, base + 1.0, 101.0), run(true, base, 99.0)], vec![run(true, base + b_shift, 100.0), run(true, base + b_shift + 1.0, 100.0), run(true, base + b_shift, 100.0)])
            })
            .collect()
    }

    #[test]
    fn a_change_beyond_the_noise_is_kept() {
        let s = scorecard(&eight(-4.0), 3, true, &[Target { metric: "interventions".into(), pct: 30.0 }]);
        let m = s["metrics"].as_array().unwrap().iter().find(|m| m["key"] == "interventions").unwrap();
        assert_eq!(m["verdict"], "better", "{m}");
        assert_eq!(m["noise"], 1.0);
        assert_eq!(m["delta"], -4.0);
        assert_eq!(s["decision"]["result"], "keep", "{s}");
        // 4 of a median of about 10: 40%, over the 30% target.
        assert_eq!(s["targets"][0]["met"], true, "{s}");
        assert_eq!(s["targets_met"], true);
        // Wall time did not move: inside the noise, not worse.
        let w = s["metrics"].as_array().unwrap().iter().find(|m| m["key"] == "wall_s").unwrap();
        assert_eq!(w["verdict"], "noise", "{w}");
    }

    #[test]
    fn a_difference_inside_the_noise_is_no_detectable_difference() {
        // Half a prompt less, where A's own runs move by one.
        let s = scorecard(&eight(-0.5), 3, true, &[]);
        assert_eq!(s["decision"]["result"], "no_detectable_difference", "{s}");
        let m = s["metrics"].as_array().unwrap().iter().find(|m| m["key"] == "interventions").unwrap();
        assert_eq!(m["verdict"], "noise", "{m}");
    }

    #[test]
    fn worse_rejects_and_a_regression_rejects_on_its_own() {
        let s = scorecard(&eight(3.0), 3, true, &[]);
        assert_eq!(s["decision"]["result"], "reject", "{s}");
        let mut cs = eight(-4.0);
        // One held-out case that passed under A fails once under B.
        let h = cs.iter_mut().find(|c| c.held_out).unwrap();
        h.b[1].passed = false;
        let s = scorecard(&cs, 3, true, &[]);
        assert_eq!(s["decision"]["result"], "reject", "{s}");
        assert_eq!(s["regressions"].as_array().unwrap().len(), 1);
        assert_eq!(s["regressions"][0]["set"], "held_out");
        assert_eq!(s["success"]["verdict"], "worse");
        // Even with too few cases to tell anything else.
        let few = vec![case(0, false, vec![run(true, 1.0, 1.0); 3], vec![run(true, 1.0, 1.0), run(false, 1.0, 1.0), run(true, 1.0, 1.0)])];
        assert_eq!(scorecard(&few, 3, true, &[])["decision"]["result"], "reject");
    }

    #[test]
    fn too_few_cases_or_runs_say_so() {
        let cs: Vec<CaseRuns> = eight(-4.0).into_iter().take(3).collect();
        let s = scorecard(&cs, 3, true, &[]);
        assert_eq!(s["decision"]["result"], "too_few", "{s}");
        assert!(s["decision"]["why"][0].as_str().unwrap().contains("too few"), "{s}");
        // One run a side: no noise floor.
        let one: Vec<CaseRuns> = eight(-4.0).into_iter().map(|mut c| {
            c.a.truncate(1);
            c.b.truncate(1);
            c
        }).collect();
        let s = scorecard(&one, 1, true, &[]);
        assert_eq!(s["decision"]["result"], "too_few", "{s}");
        // Still running.
        assert_eq!(scorecard(&eight(-4.0), 3, false, &[])["decision"]["result"], "running");
    }

    #[test]
    fn success_counts_pass_k_and_mcnemar() {
        // Six cases A fails once on each, B passes all: 6 gained, p = 1/32.
        let cs: Vec<CaseRuns> = (0..6).map(|i| case(i, i % 3 == 2, vec![run(true, 1.0, 1.0), run(false, 1.0, 1.0), run(true, 1.0, 1.0)], vec![run(true, 1.0, 1.0); 3])).collect();
        let s = scorecard(&cs, 3, true, &[Target { metric: "success".into(), pct: 0.0 }]);
        assert_eq!(s["success"]["gained"], 6, "{s}");
        assert_eq!(s["success"]["verdict"], "better", "{s}");
        assert_eq!(s["decision"]["result"], "keep");
        assert_eq!(s["targets"][0]["met"], true);
        assert!((mcnemar(6, 0) - 0.03125).abs() < 1e-9);
        assert!((mcnemar(3, 3) - 1.0).abs() < 1e-9);
        assert!(mcnemar(2, 0) > ALPHA, "2 of 2 is not enough");
    }

    #[test]
    fn the_interval_is_the_same_on_every_reading() {
        let d = [-1.0, -2.0, -3.0, 0.0, -4.0, -2.0, 1.0];
        assert_eq!(bootstrap(&d), bootstrap(&d));
        let (m, lo, hi) = bootstrap(&d).unwrap();
        assert_eq!(m, -2.0);
        assert!(lo <= m && m <= hi, "{lo} {m} {hi}");
    }

    fn runs(passed: &[bool], m0: &[f64]) -> Vec<RunM> {
        passed.iter().zip(m0).map(|(p, x)| RunM { passed: *p, diverged: false, m: [*x, 0.0, 1.0, 10.0, 100.0, 0.1] }).collect()
    }

    #[test]
    fn a_stretch_is_scored_by_its_runs() {
        let c = CaseRuns { case: "c1".into(), name: "fix".into(), held_out: false, a: runs(&[true, true, true], &[10.0, 11.0, 12.0]), b: runs(&[true, true, true], &[3.0, 4.0, 5.0]) };
        let s = stretch(&c, 3, true, &[Target { metric: "interventions".into(), pct: 30.0 }, Target { metric: "pass_rate".into(), pct: 30.0 }]);
        assert_eq!(s["metrics"][0]["verdict"], "better", "{s}");
        assert_eq!(s["decision"]["result"], "keep", "{s}");
        // 7 of 11: 64%; and A's 100% pass rate held.
        assert_eq!(s["targets"][0]["met"], true, "{s}");
        assert_eq!(s["targets"][1]["met"], true, "{s}");
        assert_eq!(s["targets_met"], true);
        // Inside A's spread: noise.
        let c2 = CaseRuns { b: runs(&[true, true, true], &[10.0, 10.0, 11.0]), ..c.clone() };
        assert_eq!(stretch(&c2, 3, true, &[])["decision"]["result"], "no_detectable_difference");
        // A always passed and B failed a run: a reject, even before B is done.
        let c3 = CaseRuns { b: runs(&[true, false], &[1.0, 1.0]), ..c.clone() };
        let s3 = stretch(&c3, 3, false, &[]);
        assert_eq!(s3["decision"]["result"], "reject", "{s3}");
        // Too few runs.
        let c4 = CaseRuns { a: runs(&[true], &[10.0]), b: runs(&[true], &[1.0]), ..c };
        assert_eq!(stretch(&c4, 1, true, &[])["decision"]["result"], "too_few");
        assert!(target_metric("wall_s") && target_metric("pass_rate") && !target_metric("dollars"));
        assert!((fisher_greater(0, 3, 3, 3) - 0.05).abs() < 1e-9);
    }

    #[test]
    fn more_turns_and_tokens_for_less_time_and_money_is_kept() {
        // A faster model: twice the turns and tokens, half the time and cost.
        let mk = |turns: f64, wall: f64, tokens: f64, cost: f64, jitter: &[f64]| -> Vec<RunM> { jitter.iter().map(|j| RunM { passed: true, diverged: false, m: [0.0, 0.0, turns + j, wall + j, tokens + j * 10.0, cost + j / 1000.0] }).collect() };
        let c = CaseRuns { case: "c1".into(), name: "fix".into(), held_out: false, a: mk(4.0, 60.0, 1000.0, 0.40, &[0.0, 1.0, 2.0]), b: mk(9.0, 30.0, 2500.0, 0.10, &[0.0, 1.0, 2.0]) };
        let s = stretch(&c, 3, true, &[Target { metric: "cost_usd".into(), pct: 30.0 }]);
        let v = |k: &str| s["metrics"].as_array().unwrap().iter().find(|m| m["key"] == k).unwrap()["verdict"].clone();
        assert_eq!((v("turns"), v("tokens"), v("wall_s"), v("cost_usd")), (json!("worse"), json!("worse"), json!("better"), json!("better")), "{s}");
        assert_eq!(s["decision"]["result"], "keep", "{s}");
        assert_eq!(s["targets_met"], true, "{s}");
        // Slower, though cheaper: a reject.
        let slow = CaseRuns { b: mk(4.0, 90.0, 1000.0, 0.10, &[0.0, 1.0, 2.0]), ..c };
        assert_eq!(stretch(&slow, 3, true, &[])["decision"]["result"], "reject");
    }

    #[test]
    fn two_runs_a_side_that_land_apart_by_chance_are_not_worse() {
        let mk = |walls: &[f64], cost: f64| -> Vec<RunM> { walls.iter().map(|w| RunM { passed: true, diverged: false, m: [0.0, 0.0, 1.0, *w, 30.0, cost] }).collect() };
        let wall = |s: &Json| s["metrics"].as_array().unwrap().iter().find(|m| m["key"] == "wall_s").unwrap()["verdict"].clone();
        let t = [Target { metric: "cost_usd".into(), pct: 30.0 }];
        // 23 ms apart: under what a replay's clock reads.
        let c = CaseRuns { case: "c1".into(), name: "fix".into(), held_out: false, a: mk(&[0.351, 0.329], 0.0003), b: mk(&[0.358, 0.368], 0.0002) };
        let s = stretch(&c, 2, true, &t);
        assert_eq!(wall(&s), "noise", "{s}");
        assert_eq!((s["decision"]["result"].as_str(), s["targets_met"].as_bool()), (Some("keep"), Some(true)), "{s}");
        // Both of B's above both of A's, by less than A's own spread.
        let c = CaseRuns { a: mk(&[150.0, 174.0], 0.4), b: mk(&[180.0, 190.0], 0.2), ..c };
        assert_eq!(wall(&stretch(&c, 2, true, &t)), "noise");
        // By more: worse, and a reject.
        let c = CaseRuns { b: mk(&[230.0, 260.0], 0.2), ..c };
        let s = stretch(&c, 2, true, &t);
        assert_eq!((wall(&s), s["decision"]["result"].clone()), (json!("worse"), json!("reject")), "{s}");
    }
}
