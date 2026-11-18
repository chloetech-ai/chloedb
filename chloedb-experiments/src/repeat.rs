//! "Check this": an experiment run again many times, the same arms and
//! the same users (and so the same truth), each time split anew and with
//! new searches, to see how often each metric's 95% interval held the
//! truth the simulator knows. One run's interval
//! misses one time in twenty by design; a run that misses often says
//! the interval is wrong, and the strip of the primary metric's
//! intervals shows how far one run's answer moves from run to run.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use serde_json::{json, Value};

use crate::ab::{self, Progress, Spec};
use crate::server::{Result, Server};
use crate::site::Site;

/// Repeats a check runs by default, and at most.
pub const REPEATS: usize = 30;
pub const MAX_REPEATS: usize = 100;

/// The spec a repeat runs: `spec` as it was (its exploring share too),
/// drawn in the process (fast), its searches capped at `MAX_SEARCHES`.
pub fn repeat_spec(spec: &Spec) -> Spec {
    let mut s = spec.clone();
    s.mode = "fast".into();
    s.impressions = s.impressions.clamp(1000, MAX_SEARCHES);
    s
}

/// The most searches one repeat draws (a run of more is checked at this size).
pub const MAX_SEARCHES: u64 = 2_000_000;

/// Runs `spec` `repeats` times, its users split anew and new searches
/// drawn each time, and sums up how often the intervals held the truth.
/// `progress` counts repeats.
pub async fn check(server: &Server, spec: &Spec, id: &str, mut site: Site, repeats: usize, progress: Arc<Progress>) -> Result<Value> {
    let t0 = std::time::Instant::now();
    // Ranked with the run's exploring policy, if it had one: the queries
    // it could not explore were dropped from that run, and so from these.
    let policy = spec.policy()?;
    let spec = repeat_spec(spec);
    progress.total.store(repeats as u64, Ordering::Relaxed);
    progress.stage("ranking every query under each arm");
    let ranks = ab::rank(server, &mut site, &spec, &policy).await?;
    let mut reports = Vec::with_capacity(repeats);
    for r in 0..repeats {
        progress.stage(&format!("repeat {} of {repeats}", r + 1));
        let mut s = spec.clone();
        s.salt = ab::fresh_seed() | 1;
        let d = ab::run(server, &s, &format!("{id}c{r}"), site.clone(), ranks.clone(), Arc::new(Progress::default())).await?;
        let rep = tokio::task::spawn_blocking(move || ab::report(&d)).await.map_err(|e| e.to_string())?;
        reports.push(rep);
        progress.done.fetch_add(1, Ordering::Relaxed);
    }
    let mut out = sum_up(&spec, &reports);
    out["seconds"] = json!(t0.elapsed().as_secs_f64());
    out["checked"] = json!(crate::app::now_ms());
    Ok(out)
}

/// P(X <= k) for X ~ Binomial(n, p).
pub fn binom_cdf(k: usize, n: usize, p: f64) -> f64 {
    let mut term = (1.0 - p).powi(n as i32);
    let mut sum = 0.0;
    for i in 0..=k.min(n) {
        sum += term;
        term *= (n - i) as f64 / (i + 1) as f64 * p / (1.0 - p);
    }
    sum.min(1.0)
}

/// The sums over the repeats' reports: per metric and arm, the intervals
/// that held the truth; the primary metric's intervals one by one; the
/// calls made, against the truth's.
pub fn sum_up(spec: &Spec, reports: &[Value]) -> Value {
    let n = reports.len();
    let expected = 0.95 * n as f64;
    let Some(first) = reports.first() else { return json!({"repeats": 0}) };
    let k = spec.arms.len();
    let mut metrics = Vec::new();
    let mut worst: Option<(f64, String)> = None;
    for (mi, m) in first["metrics"].as_array().into_iter().flatten().enumerate() {
        let mut arms = Vec::new();
        for a in 1..k {
            let cs: Vec<&Value> = reports.iter().map(|r| &r["metrics"][mi]["comparisons"][a - 1]).collect();
            let known: Vec<bool> = cs.iter().filter_map(|c| c["covers"].as_bool()).collect();
            if known.is_empty() {
                continue;
            }
            let covered = known.iter().filter(|&&c| c).count();
            let p_low = binom_cdf(covered, known.len(), 0.95);
            let cuped: Vec<bool> = cs.iter().filter_map(|c| Some((c["cuped"]["lo"].as_f64()? <= c["true_delta"].as_f64()?) && (c["true_delta"].as_f64()? <= c["cuped"]["hi"].as_f64()?))).collect();
            let bias: Vec<f64> = cs.iter().filter_map(|c| Some((c["delta"].as_f64()? - c["true_delta"].as_f64()?) / c["se"].as_f64().filter(|s| *s > 0.0)?)).collect();
            let mean_z = if bias.is_empty() { None } else { Some(bias.iter().sum::<f64>() / bias.len() as f64) };
            if worst.as_ref().is_none_or(|w| p_low < w.0) {
                worst = Some((p_low, format!("{} ({})", m["title"].as_str().unwrap_or(""), spec.arms[a].name)));
            }
            arms.push(json!({
                "arm": spec.arms[a].name, "covered": covered, "of": known.len(), "expected": 0.95 * known.len() as f64,
                "p_low": p_low, "ok": p_low >= 0.01,
                "cuped_covered": (!cuped.is_empty()).then(|| cuped.iter().filter(|&&c| c).count()), "cuped_of": cuped.len(),
                "mean_z": mean_z,
            }));
        }
        if !arms.is_empty() {
            metrics.push(json!({"key": m["key"], "title": m["title"], "guardrail": m["guardrail"], "primary": m["key"] == json!(spec.primary), "arms": arms}));
        }
    }
    // The primary metric: each repeat's interval of the lift, and its call.
    let pi = first["metrics"].as_array().and_then(|ms| ms.iter().position(|m| m["key"] == json!(spec.primary))).unwrap_or(0);
    let primary: Vec<Value> = (1..k)
        .map(|a| {
            let runs: Vec<Value> = reports.iter().map(|r| {
                let c = &r["metrics"][pi]["comparisons"][a - 1];
                json!([c["lift"], c["lift_lo"], c["lift_hi"], c["covers"]])
            }).collect();
            let mut calls: Vec<(String, usize)> = Vec::new();
            for r in reports {
                let call = r["verdicts"][a - 1]["call"].as_str().unwrap_or("").to_string();
                match calls.iter_mut().find(|(c, _)| *c == call) {
                    Some(x) => x.1 += 1,
                    None => calls.push((call, 1)),
                }
            }
            calls.sort_by(|x, y| y.1.cmp(&x.1));
            let c0 = &first["metrics"][pi]["comparisons"][a - 1];
            json!({
                "arm": spec.arms[a].name, "true_lift": c0["true_lift"], "truth_call": first["verdicts"][a - 1]["truth_call"],
                "runs": runs, "calls": calls.iter().map(|(c, n)| json!({"call": c, "n": n})).collect::<Vec<_>>(),
            })
        })
        .collect();
    let p = metrics.iter().find(|m| m["primary"] == json!(true));
    let p_text = p.map(|m| {
        m["arms"].as_array().into_iter().flatten().map(|a| format!("{} of {} for {}", a["covered"], a["of"], a["arm"].as_str().unwrap_or(""))).collect::<Vec<_>>().join(", ")
    });
    let mut text = match p_text {
        Some(t) => format!("{n} runs on the same users, each split anew with new searches: the interval of {} held the truth in {t} (95% intervals hold it {expected:.1} times in {n} on average).", p.and_then(|m| m["title"].as_str()).unwrap_or("the primary metric")),
        None => format!("{n} runs on the same users, each split anew with new searches."),
    };
    let bad: Vec<&Value> = metrics.iter().flat_map(|m| m["arms"].as_array().into_iter().flatten().map(move |a| (m, a))).filter(|(_, a)| a["ok"] == json!(false)).map(|(m, _)| m).collect();
    if bad.is_empty() {
        text.push_str(" Every metric's intervals held it about as often as they should.");
    } else if let Some((pl, name)) = worst {
        text.push_str(&format!(" {name} held it less often than chance allows (p = {pl:.3} of so few): its interval is too narrow there, or its truth is off."));
    }
    json!({
        "repeats": n, "searches": spec.impressions, "users": spec.users, "level": 0.95, "expected": expected,
        "metrics": metrics, "primary": {"key": spec.primary, "arms": primary}, "text": text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binomial_tail() {
        // P(X <= 0) for Binomial(3, 0.5) = 1/8; P(X <= 3) = 1.
        assert!((binom_cdf(0, 3, 0.5) - 0.125).abs() < 1e-12);
        assert!((binom_cdf(3, 3, 0.5) - 1.0).abs() < 1e-12);
        // P(X <= 25) for Binomial(30, 0.95): 0.0156 by a table.
        let p = binom_cdf(25, 30, 0.95);
        assert!((p - 0.0156).abs() < 0.001, "{p}");
    }

    #[test]
    fn sums_up_intervals_and_calls() {
        let spec: Spec = serde_json::from_value(json!({"name": "t", "site": "s", "arms": [{"name": "A", "rank": "x"}, {"name": "B", "rank": "y"}]})).unwrap();
        let rep = |covers: bool, call: &str| json!({
            "metrics": [{"key": "success", "title": "Success", "guardrail": false, "comparisons": [{"covers": covers, "delta": 0.1, "se": 0.05, "true_delta": 0.08, "lift": 0.2, "lift_lo": 0.1, "lift_hi": 0.3, "true_lift": 0.18}]}],
            "verdicts": [{"call": call, "truth_call": "truly better"}],
        });
        let mut reps: Vec<Value> = (0..28).map(|_| rep(true, "ship")).collect();
        reps.push(rep(false, "ship"));
        reps.push(rep(true, "no detectable difference"));
        let s = sum_up(&spec, &reps);
        assert_eq!(s["metrics"][0]["arms"][0]["covered"], json!(29));
        assert_eq!(s["metrics"][0]["arms"][0]["ok"], json!(true));
        assert_eq!(s["primary"]["arms"][0]["runs"].as_array().unwrap().len(), 30);
        assert_eq!(s["primary"]["arms"][0]["calls"][0], json!({"call": "ship", "n": 29}));
        assert!(s["text"].as_str().unwrap().contains("29 of 30 for B"), "{}", s["text"]);
    }
}
