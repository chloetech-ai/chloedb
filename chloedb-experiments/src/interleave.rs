//! Interleaving: both arms' rows in one list, by team draft, and every
//! click credited to the arm that put its row there. Each search compares
//! the two rankings on the same user and the same query, so the noise an
//! A/B fights (who the users are, what they ask) cancels, and far fewer
//! searches tell the two apart. The truth is exact, as an A/B's is: every
//! coin sequence of the draft, each list through the click model.
//!
//! A run here also runs the A/B of the same pair on the same users and
//! searches, so the report can say how many users each would need.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use serde_json::{json, Value};

use crate::ab::{self, p_two, Progress, Spec, Z80, Z975};
use crate::click::Model;
use crate::policy::{Policy, Rng};
use crate::server::{Result, Server};
use crate::sim::{Ranked, World};
use crate::site::Site;
use crate::stats::norm_cdf;
use crate::users::{Population, Shape};

/// Team-draft interleaving of `a` and `b` to `limit` rows; the draft's
/// coin flips are the bits of `coins`, the j-th flip bit j (1: A picks).
/// Returns the rows and, per row, the team that picked it (true: B).
pub fn team_draft(a: &[String], b: &[String], limit: usize, coins: u32) -> (Vec<String>, Vec<bool>) {
    let mut rows: Vec<String> = Vec::with_capacity(limit);
    let mut team = Vec::with_capacity(limit);
    let (mut na, mut nb, mut flips, mut ia, mut ib) = (0usize, 0usize, 0u32, 0usize, 0usize);
    while rows.len() < limit {
        while ia < a.len() && rows.contains(&a[ia]) {
            ia += 1;
        }
        while ib < b.len() && rows.contains(&b[ib]) {
            ib += 1;
        }
        let (has_a, has_b) = (ia < a.len(), ib < b.len());
        if !has_a && !has_b {
            break;
        }
        let a_turn = if na != nb {
            na < nb
        } else {
            let c = coins >> flips & 1 == 1;
            flips += 1;
            c
        };
        let a_picks = if !has_b { true } else if !has_a { false } else { a_turn };
        if a_picks {
            rows.push(a[ia].clone());
            team.push(false);
            na += 1;
        } else {
            rows.push(b[ib].clone());
            team.push(true);
            nb += 1;
        }
    }
    (rows, team)
}

/// Coin flips a draft of `limit` rows takes at most.
pub fn flips(limit: usize) -> u32 {
    limit.div_ceil(2) as u32
}

/// One query's interleavings: each distinct list (indexes into the
/// union's rows) with its teams and its chance, and per coin pattern the
/// list it gives.
#[derive(Clone, Debug)]
pub struct Drafts {
    pub lists: Vec<(Vec<u16>, Vec<bool>, f64)>,
    pub by_pattern: Vec<usize>,
}

/// The rows of `a` then those of `b` not in `a`: the union a world ranks.
pub fn union(a: &[String], b: &[String]) -> Vec<String> {
    let mut u = a.to_vec();
    for k in b {
        if !u.contains(k) {
            u.push(k.clone());
        }
    }
    u
}

pub fn drafts(a: &[String], b: &[String], limit: usize) -> Drafts {
    let u = union(a, b);
    let at: HashMap<&str, u16> = u.iter().enumerate().map(|(i, k)| (k.as_str(), i as u16)).collect();
    let m = flips(limit);
    let n = 1usize << m;
    let mut lists: Vec<(Vec<u16>, Vec<bool>, f64)> = Vec::new();
    let mut by_pattern = Vec::with_capacity(n);
    for c in 0..n {
        let (rows, team) = team_draft(a, b, limit, c as u32);
        let idx: Vec<u16> = rows.iter().map(|k| at[k.as_str()]).collect();
        match lists.iter().position(|(r, t, _)| *r == idx && *t == team) {
            Some(i) => {
                lists[i].2 += 1.0 / n as f64;
                by_pattern.push(i);
            }
            None => {
                lists.push((idx, team, 1.0 / n as f64));
                by_pattern.push(lists.len() - 1);
            }
        }
    }
    Drafts { lists, by_pattern }
}

/// Per user: clicks to B minus clicks to A, the same of satisfied clicks,
/// searches, searches B won, searches A won, clicks to A, clicks to B.
type Tally = [f64; 7];
const D: usize = 0;
const DS: usize = 1;
const N: usize = 2;
const WB: usize = 3;
const WA: usize = 4;
const CA: usize = 5;
const CB: usize = 6;

fn ratio(xs: &[(f64, f64)]) -> (f64, f64) {
    let n = xs.len() as f64;
    let (sa, sb) = xs.iter().fold((0.0, 0.0), |s, x| (s.0 + x.0, s.1 + x.1));
    if sb <= 0.0 || xs.len() < 2 {
        return (f64::NAN, f64::NAN);
    }
    let r = sa / sb;
    let (ma, mb) = (sa / n, sb / n);
    let (mut vaa, mut vbb, mut vab) = (0.0, 0.0, 0.0);
    for &(a, b) in xs {
        let (a, b) = (a - ma, b - mb);
        vaa += a * a;
        vbb += b * b;
        vab += a * b;
    }
    let d = n - 1.0;
    let var = (vaa / d - 2.0 * r * vab / d + r * r * vbb / d) / (n * mb * mb);
    (r, var.max(0.0).sqrt())
}

const CHECKPOINTS: u64 = 40;

/// Runs an interleaving of `spec`'s two arms on its users and searches
/// (drawn here), and the A/B of the same pair beside it.
pub async fn run(server: &Server, spec: &Spec, run_id: &str, mut site: Site, progress: Arc<Progress>) -> Result<Value> {
    let t0 = std::time::Instant::now();
    if spec.arms.len() != 2 {
        return Err("an interleaving has two arms".into());
    }
    progress.stage("ranking every query under each arm");
    let ranks = ab::rank(server, &mut site, spec, &Policy::None).await?;
    let model = Model::parse(&spec.model)?;
    let shape = Shape { taste: spec.taste, appetite: spec.appetite, ..Shape::default() };
    let lists: Vec<Vec<Vec<String>>> = ranks.arms.iter().map(|r| r.iter().map(|x| x.keys.clone()).collect()).collect();
    let nq = site.queries.len();
    let ds: Vec<Drafts> = (0..nq).map(|q| drafts(&lists[0][q], &lists[1][q], spec.limit)).collect();
    let ranked: Vec<Ranked> = (0..nq).map(|q| Ranked { keys: union(&lists[0][q], &lists[1][q]), scores: Vec::new() }).collect();
    let w = World::new(site.clone(), Population::new(spec.users, spec.seed, shape), model, Policy::None, spec.limit, format!("interleave {} {}", spec.arms[0].rank, spec.arms[1].rank), ranked);
    let users = w.users.users.clone();
    let nu = users.len();
    let share = w.users.shares();
    let qs = site.shares();
    progress.total.store(spec.impressions * 2, Ordering::Relaxed);

    // The truth: each user's expected credit, every draft weighed by its chance.
    progress.stage("working out every user's exact preference");
    let truth: Vec<(f64, f64, f64, f64)> = {
        let (w, ds, users, qs) = (&w, &ds, &users, &qs);
        ab::par(nu, |r| {
            r.map(|i| {
                let u = &users[i];
                let (mut d, mut dsat, mut ca, mut cb) = (0.0, 0.0, 0.0, 0.0);
                for q in 0..ds.len() {
                    for (idx, team, p) in &ds[q].lists {
                        let slots: Vec<_> = idx.iter().map(|&j| w.slot(u, q, &w.ranked[q].keys[j as usize])).collect();
                        let e = w.model.expected(&slots, &u.behaviour);
                        for (k, &b) in team.iter().enumerate() {
                            let s = if b { 1.0 } else { -1.0 };
                            let weight = qs[q] * p;
                            d += weight * s * e.click[k];
                            dsat += weight * s * e.satisfied[k];
                            if b {
                                cb += weight * e.click[k];
                            } else {
                                ca += weight * e.click[k];
                            }
                        }
                    }
                }
                (d, dsat, ca, cb)
            })
            .collect::<Vec<_>>()
        })
        .into_iter()
        .flatten()
        .collect()
    };
    let total_share: f64 = share.iter().sum();
    let t = |f: &dyn Fn(&(f64, f64, f64, f64)) -> f64| truth.iter().zip(&share).map(|(x, s)| s * f(x)).sum::<f64>() / total_share;
    let (true_d, true_ds, true_ca, true_cb) = (t(&|x| x.0), t(&|x| x.1), t(&|x| x.2), t(&|x| x.3));

    // The searches, in checkpoints.
    progress.stage("drawing every search, both arms in one list");
    let mut tally: Vec<Tally> = vec![[0.0; 7]; nu];
    let mut series: Vec<(u64, f64, f64, f64, f64)> = Vec::new();
    let step = spec.impressions.div_ceil(CHECKPOINTS).max(1);
    let mask = (1u32 << flips(spec.limit)) - 1;
    let mut start = 0u64;
    while start < spec.impressions {
        let end = (start + step).min(spec.impressions);
        let parts = {
            let (w, ds) = (&w, &ds);
            ab::par((end - start) as usize, |r| {
                let mut s: HashMap<u32, Tally> = HashMap::new();
                for i in r {
                    let i = start + i as u64;
                    let c = w.context(run_id, i);
                    let q = c.query as usize;
                    let pat = (Rng::of(&format!("{}/coins", c.request)).next_u64() as u32) & mask;
                    let (idx, team, _) = &ds[q].lists[ds[q].by_pattern[pat as usize]];
                    let o = w.react(&c, idx);
                    let (mut ca, mut cb, mut sa, mut sb) = (0.0, 0.0, 0.0, 0.0);
                    for (k, &b) in team.iter().enumerate() {
                        let (cl, sat) = (o.click[k] as u8 as f64, o.satisfied[k] as u8 as f64);
                        if b {
                            cb += cl;
                            sb += sat;
                        } else {
                            ca += cl;
                            sa += sat;
                        }
                    }
                    let x = s.entry(c.user).or_insert([0.0; 7]);
                    x[D] += cb - ca;
                    x[DS] += sb - sa;
                    x[N] += 1.0;
                    x[WB] += (cb > ca) as u8 as f64;
                    x[WA] += (ca > cb) as u8 as f64;
                    x[CA] += ca;
                    x[CB] += cb;
                }
                s
            })
        };
        for p in parts {
            for (u, x) in p {
                for j in 0..7 {
                    tally[u as usize][j] += x[j];
                }
            }
        }
        progress.done.fetch_add(end - start, Ordering::Relaxed);
        let seen: Vec<(f64, f64)> = tally.iter().filter(|x| x[N] > 0.0).map(|x| (x[D], x[N])).collect();
        let (d, se) = ratio(&seen);
        series.push((end, d, se, 0.0, 0.0));
        start = end;
    }

    // The statistics.
    let seen: Vec<&Tally> = tally.iter().filter(|x| x[N] > 0.0).collect();
    let pick = |a: usize| seen.iter().map(|x| (x[a], x[N])).collect::<Vec<_>>();
    let (d, se) = ratio(&pick(D));
    let (dsat, se_sat) = ratio(&pick(DS));
    let (ca, _) = ratio(&pick(CA));
    let (cb, _) = ratio(&pick(CB));
    let searches: f64 = seen.iter().map(|x| x[N]).sum();
    let (wb, wa) = (seen.iter().map(|x| x[WB]).sum::<f64>() / searches, seen.iter().map(|x| x[WA]).sum::<f64>() / searches);
    let tau = 0.05 * ((ca + cb) / 2.0).max(1e-6);
    let pav = ab::always_valid(&series.iter().map(|s| (s.1, s.2)).collect::<Vec<_>>(), tau);
    for (s, p) in series.iter_mut().zip(&pav) {
        s.3 = p_two(s.1 / s.2);
        s.4 = *p;
    }
    let users_seen = seen.len() as f64;
    let stat = |d: f64, se: f64, truth: f64| {
        let mde = (Z975 + Z80) * se;
        json!({
            "delta": d, "se": se, "lo": d - Z975 * se, "hi": d + Z975 * se, "p": p_two(d / se),
            "true_delta": truth, "covers": (truth - d).abs() <= Z975 * se,
            "power": norm_cdf(truth.abs() / se - Z975) + norm_cdf(-truth.abs() / se - Z975),
            "users_needed": (truth.abs() > 1e-12).then(|| (users_seen * (mde / truth.abs()).powi(2)).ceil()),
            "mde": mde,
        })
    };
    let mut clicks = stat(d, se, true_d);
    clicks["p_always_valid"] = json!(pav.last());
    let satisfied = stat(dsat, se_sat, true_ds);
    let call = if clicks["p"].as_f64().unwrap_or(1.0) >= 0.05 { "no detectable difference" } else if d > 0.0 { "ship" } else { "worse" };
    let truth_call = if true_d.abs() < 1e-12 { "no true difference" } else if true_d > 0.0 { "truly better" } else { "truly worse" };
    let lift = d / ca.max(1e-12);
    let true_lift = true_d / true_ca.max(1e-12);
    let lift_se = se / ca.max(1e-12);

    // The A/B of the same pair, same users and searches.
    progress.stage("the A/B of the same pair, to compare");
    let mut ab_spec = spec.clone();
    ab_spec.mode = "fast".into();
    ab_spec.explore_share = 0.0;
    ab_spec.pre = 0.0;
    let ab_progress = Arc::new(Progress::default());
    let rd = ab::run(server, &ab_spec, &format!("{run_id}ab"), site.clone(), ranks, ab_progress).await?;
    let ab_rep = tokio::task::spawn_blocking(move || ab::report(&rd)).await.map_err(|e| e.to_string())?;
    progress.done.store(spec.impressions * 2, Ordering::Relaxed);
    let ab_metric = |key: &str| {
        let m = ab_rep["metrics"].as_array().and_then(|ms| ms.iter().find(|m| m["key"] == json!(key))).cloned().unwrap_or(Value::Null);
        let c = &m["comparisons"][0];
        json!({"key": key, "title": m["title"], "higher": m["higher"], "lift": c["lift"], "lift_lo": c["lift_lo"], "lift_hi": c["lift_hi"], "true_lift": c["true_lift"], "p": c["p"], "power": c["power"], "users_needed": c["users_needed"], "covers": c["covers"]})
    };
    let ab_metrics: Vec<Value> = ["clicks", "success", "satisfied"].iter().map(|k| ab_metric(k)).collect();
    // Each at its most sensitive: the A/B's metric, and the credit, that
    // need the fewest users for 80% power on their true effect.
    fn best(xs: Vec<(String, Option<f64>)>) -> Option<(String, f64)> {
        xs.into_iter().filter_map(|(k, n)| Some((k, n?))).min_by(|a, b| a.1.total_cmp(&b.1))
    }
    let ab_best = best(ab_metrics.iter().map(|m| (m["key"].as_str().unwrap_or("").to_string(), m["users_needed"].as_f64())).collect());
    let il_best = best(vec![("clicks".into(), clicks["users_needed"].as_f64()), ("satisfied".into(), satisfied["users_needed"].as_f64())]);
    let (ab_users, il_users) = (ab_best.as_ref().map(|x| x.1), il_best.as_ref().map(|x| x.1));
    let times = ab_users.zip(il_users).filter(|(_, b)| *b > 0.0).map(|(a, b)| a / b);
    let agrees = ab_metrics[0]["true_lift"].as_f64().map(|l| (l > 0.0) == (true_d > 0.0));
    let b_name = &spec.arms[1].name;
    let a_name = &spec.arms[0].name;
    let mut text = match call {
        "ship" => format!("Users preferred {b_name}: {:.3} more clicks per search went to its rows than to {a_name}'s (p {}).", d, p_text(clicks["p"].as_f64())),
        "worse" => format!("Users preferred {a_name}: {:.3} more clicks per search went to its rows than to {b_name}'s (p {}).", -d, p_text(clicks["p"].as_f64())),
        _ => format!("No detectable preference after {} searches.", fmt_n(searches as u64)),
    };
    if let Some(x) = times {
        let what = |k: &str| match k { "success" => "success rate", "satisfied" => "satisfied clicks", _ => "clicks" };
        text.push_str(&format!(" For 80% power the A/B needs about {} users (on {}, its most sensitive metric here); interleaving about {} (crediting {}): {}.", fmt_n(ab_users.unwrap_or(0.0) as u64), what(ab_best.as_ref().map_or("", |x| x.0.as_str())), fmt_n(il_users.unwrap_or(0.0) as u64), what(il_best.as_ref().map_or("", |x| x.0.as_str())), times_text(x)));
    }
    if agrees == Some(false) {
        text.push_str(" Beware: the true preference and the A/B's true change in clicks point different ways here.");
    }
    let verdict = json!({"arm": b_name, "metric": "preference", "call": call, "p": clicks["p"], "lift": lift, "lift_lo": lift - Z975 * lift_se, "lift_hi": lift + Z975 * lift_se, "true_lift": true_lift, "truth_call": truth_call, "power": clicks["power"], "users_needed": clicks["users_needed"], "guardrails_worse": []});
    Ok(json!({
        "kind": "interleave",
        "site": {"name": site.name, "table": site.table, "queries": nq},
        "arms": spec.arms.iter().map(|a| json!({"name": a.name, "rank": a.rank, "then": a.then, "share": 1.0})).collect::<Vec<_>>(),
        "users": nu, "users_seen": users_seen, "searches": spec.impressions,
        "seconds": t0.elapsed().as_secs_f64(),
        "interleave": {
            "clicks": clicks, "satisfied": satisfied,
            "per_search": {"a": ca, "b": cb, "true_a": true_ca, "true_b": true_cb},
            "wins": {"b": wb, "a": wa, "ties": 1.0 - wb - wa},
            "series": series.iter().map(|s| json!([s.0, s.1, s.1 - Z975 * s.2, s.1 + Z975 * s.2, s.3, s.4])).collect::<Vec<_>>(),
            "drafts": ds.iter().map(|x| x.lists.len()).sum::<usize>() as f64 / nq.max(1) as f64,
            "call": call, "truth_call": truth_call,
        },
        "ab": {"metrics": ab_metrics, "verdict": ab_rep["verdicts"][0], "users": ab_rep["users"], "arms": ab_rep["arms"]},
        "saving": {"ab_metric": ab_best.as_ref().map(|x| x.0.clone()), "ab_users": ab_users, "interleave_metric": il_best.as_ref().map(|x| x.0.clone()), "interleave_users": il_users, "times": times, "agrees": agrees},
        "verdicts": [verdict],
        "text": text,
    }))
}

fn p_text(p: Option<f64>) -> String {
    match p {
        Some(p) if p < 1e-4 => "< 0.0001".into(),
        Some(p) => format!("{p:.3}"),
        None => "–".into(),
    }
}

fn times_text(x: f64) -> String {
    let n = |x: f64| if x >= 10.0 { format!("{x:.0}") } else { format!("{x:.1}") };
    if x >= 1.5 {
        format!("{} times fewer", n(x))
    } else if x >= 1.0 / 1.5 {
        "about as many".into()
    } else {
        format!("{} times more", n(1.0 / x))
    }
}

fn fmt_n(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn team_draft_alternates_and_skips_rows_shown() {
        let a = v(&["1", "2", "3", "4"]);
        let b = v(&["2", "5", "1", "6"]);
        // Coins all 1: A picks first in each round.
        let (rows, team) = team_draft(&a, &b, 4, 0b11);
        assert_eq!(rows, v(&["1", "2", "3", "5"]));
        assert_eq!(team, vec![false, true, false, true]);
        // Coins all 0: B first.
        let (rows, team) = team_draft(&a, &b, 4, 0);
        assert_eq!(rows, v(&["2", "1", "5", "3"]));
        assert_eq!(team, vec![true, false, true, false]);
    }

    #[test]
    fn same_lists_give_even_teams() {
        let a = v(&["1", "2", "3", "4", "5"]);
        let d = drafts(&a, &a, 5);
        // Every draft shows the list in its order; teams split 3:2 or 2:3.
        assert!((d.lists.iter().map(|x| x.2).sum::<f64>() - 1.0).abs() < 1e-12);
        for (idx, team, _) in &d.lists {
            assert_eq!(idx, &vec![0, 1, 2, 3, 4]);
            let b = team.iter().filter(|&&t| t).count();
            assert!(b == 2 || b == 3);
        }
        // On average, each position is B's half the time: no preference.
        for k in 0..5 {
            let pb: f64 = d.lists.iter().filter(|x| x.1[k]).map(|x| x.2).sum();
            assert!((pb - 0.5).abs() < 1e-12, "position {k}: {pb}");
        }
    }

    #[test]
    fn ratio_is_the_delta_method() {
        let (r, se) = ratio(&[(1.0, 2.0), (3.0, 4.0), (0.0, 1.0), (2.0, 2.0)]);
        assert!((r - 6.0 / 9.0).abs() < 1e-12);
        assert!(se > 0.0 && se < 1.0);
    }
}
