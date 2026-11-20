//! An experiment on a real site: one question, a control and a treatment
//! (each a set of the site's knobs), and the evidence for it gathered in
//! steps, each cheaper than the next, up to a decision:
//!
//! 1. Side by side: both arms' first pages for the site's most asked
//!    queries (its search log, then the personas' needs), and how much
//!    the treatment changes them. Free, at the start.
//! 2. Judged queries: every need of a persona set searched on both arms,
//!    each row graded by the judge ([`crate::judge`]), compared need by
//!    need: nDCG@10, the known items lost from the first page (the
//!    regressions), and what that many needs can detect.
//! 3. Simulated users: a study of the two arms ([`crate::study`]).
//! 4. Real visitors: the two arms interleaved on the site's own searches
//!    (team draft, [`crate::source::Source::interleave_start`]), each
//!    story opened credited to the arm that put it there; the preference
//!    with an always-valid p value, so it can be read every day.
//! 5. The decision, by a rule declared in advance ([`rule`]), and the
//!    call and note the person records.
//!
//! The analysis of each step is a function of its units (needs, searches),
//! which the report also gets, to cut by any attribute.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::ab::{always_valid, p_two, Z975};
use crate::judge::Judge;
use crate::personas::{Need, PersonaSet};
use crate::source::{Hit, Source};
use crate::study::Session;
use crate::tune::need_score;

/// Queries of the side by side.
pub const SIDE_QUERIES: usize = 30;
/// Needs the judged step takes at most.
pub const MAX_NEEDS: usize = 80;
/// Queries of a need searched in the judged step.
const QUERIES_PER_NEED: usize = 3;
/// Rows of a first page.
pub const K: usize = 10;
/// The mixture's spread of the always-valid p value, in preference.
const TAU: f64 = 0.05;
/// The smallest preference worth acting on, by default.
pub const SMALLEST: f64 = 0.02;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Guardrails {
    /// p95 latency may grow by at most this share.
    #[serde(default = "latency")]
    pub latency_p95: f64,
    /// Known items the treatment may lose from the first page.
    #[serde(default)]
    pub regressions: usize,
}

fn latency() -> f64 {
    0.10
}

impl Default for Guardrails {
    fn default() -> Self {
        Guardrails { latency_p95: latency(), regressions: 0 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TrialSpec {
    pub name: String,
    #[serde(default)]
    pub hypothesis: String,
    pub source: String,
    #[serde(default = "control_name")]
    pub control_name: String,
    #[serde(default)]
    pub control: Value,
    pub treatment_name: String,
    pub treatment: Value,
    /// The persona set of the judged queries and the simulated users ("": none yet).
    #[serde(default)]
    pub personas: String,
    #[serde(default)]
    pub guardrails: Guardrails,
    /// The smallest preference worth shipping for (interleaved).
    #[serde(default = "smallest")]
    pub smallest: f64,
}

fn control_name() -> String {
    "As it is".into()
}
fn smallest() -> f64 {
    SMALLEST
}

impl TrialSpec {
    pub fn check(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("an experiment has a name: what it asks".into());
        }
        if self.treatment_name.trim().is_empty() || self.control_name.trim() == self.treatment_name.trim() {
            return Err("the control and the treatment have two names".into());
        }
        let norm = |v: &Value| if v.is_null() { json!({}) } else { v.clone() };
        if norm(&self.control) == norm(&self.treatment) {
            return Err("the treatment searches like the control: change a knob".into());
        }
        if !(self.guardrails.latency_p95 >= 0.0 && self.guardrails.latency_p95 <= 5.0) {
            return Err("the latency guardrail is a share, 0 to 5".into());
        }
        if !(self.smallest > 0.0 && self.smallest < 1.0) {
            return Err("the smallest preference is between 0 and 1".into());
        }
        Ok(())
    }
}

pub fn now_s() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

fn mean(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        0.0
    } else {
        xs.iter().sum::<f64>() / xs.len() as f64
    }
}

fn sd(xs: &[f64]) -> f64 {
    if xs.len() < 2 {
        return 0.0;
    }
    let m = mean(xs);
    (xs.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (xs.len() - 1) as f64).sqrt()
}

pub fn quantile(xs: &[f64], q: f64) -> Option<f64> {
    let mut v: Vec<f64> = xs.iter().copied().filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    Some(v[((v.len() as f64 * q).ceil() as usize).saturating_sub(1).min(v.len() - 1)])
}

/// The smallest difference of means `n` paired units of spread `sd` can
/// find with 80% power at 5%.
pub fn detectable(sd: f64, n: usize) -> f64 {
    if n == 0 {
        return f64::INFINITY;
    }
    (Z975 + crate::ab::Z80) * sd / (n as f64).sqrt()
}

/// Units needed to find a difference `d` with 80% power, at spread `sd`.
pub fn needed(sd: f64, d: f64) -> f64 {
    ((Z975 + crate::ab::Z80) * sd / d).powi(2)
}

// ------------------------------------------------------------ side by side

/// The queries of the side by side: the site's most asked (its log), then
/// the needs' own words, at most `n`.
pub fn side_queries(logged: &[String], needs: &[String], n: usize) -> Vec<(String, &'static str)> {
    let mut count: HashMap<String, (usize, String)> = HashMap::new();
    for q in logged {
        let k = q.trim().to_lowercase();
        if k.is_empty() {
            continue;
        }
        count.entry(k).or_insert((0, q.trim().to_string())).0 += 1;
    }
    let mut top: Vec<(usize, String)> = count.into_values().collect();
    top.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut out: Vec<(String, &'static str)> = top.into_iter().map(|(_, q)| (q, "log")).take(n).collect();
    for q in needs {
        if out.len() >= n {
            break;
        }
        if !out.iter().any(|(x, _)| x.eq_ignore_ascii_case(q)) {
            out.push((q.clone(), "need"));
        }
    }
    out
}

/// How much a page changed: the share of the control's rows still on it,
/// and the rows whose place moved.
pub fn page_change(a: &[String], b: &[String]) -> (f64, usize) {
    let kept = a.iter().filter(|x| b.contains(x)).count();
    let moved = (0..a.len().max(b.len())).filter(|&i| a.get(i) != b.get(i)).count();
    (if a.is_empty() { 1.0 } else { kept as f64 / a.len() as f64 }, moved)
}

pub fn side_summary(rows: &[Value]) -> Value {
    let n = rows.len();
    let changed = rows.iter().filter(|r| r["moved"].as_u64().unwrap_or(0) > 0).count();
    let overlap: Vec<f64> = rows.iter().filter_map(|r| r["overlap"].as_f64()).collect();
    let ams: Vec<f64> = rows.iter().filter_map(|r| r["a_ms"].as_f64()).collect();
    let bms: Vec<f64> = rows.iter().filter_map(|r| r["b_ms"].as_f64()).collect();
    let share = if n == 0 { 0.0 } else { changed as f64 / n as f64 };
    let sentence = if n == 0 {
        "No queries to compare: the site has no log yet and the experiment no personas.".to_string()
    } else if changed == 0 {
        format!("The treatment shows the same first page as the control on all {n} queries: there is nothing to test.")
    } else {
        format!("The treatment changes the first page of {changed} of {n} queries ({:.0}%); on average {:.0}% of the control's rows stay on it.", share * 100.0, mean(&overlap) * 100.0)
    };
    json!({
        "queries": n, "changed": changed, "share": share, "overlap": mean(&overlap),
        "a_p95": quantile(&ams, 0.95), "b_p95": quantile(&bms, 0.95), "sentence": sentence,
    })
}

// ------------------------------------------------------------ judged queries

/// One need of the judged step, on both arms.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NeedUnit {
    pub need: String,
    pub kind: String,
    pub persona: String,
    pub text: String,
    pub queries: Vec<String>,
    /// nDCG@10 on each arm (the mean of its queries').
    pub a: f64,
    pub b: f64,
    /// A known need: its row's best place on each arm's first pages.
    pub a_rank: Option<usize>,
    pub b_rank: Option<usize>,
    pub a_ms: f64,
    pub b_ms: f64,
}

impl NeedUnit {
    /// Whether the treatment lost what the control found: a known item
    /// off the first page, or a need's score down by 0.3 or more.
    pub fn regressed(&self) -> bool {
        (self.a_rank.is_some() && self.b_rank.is_none()) || self.b - self.a <= -0.3
    }
}

/// The queries of each need: those people typed for it in the persona
/// set's studies (the first few), or else its own words.
pub fn need_queries(set: &PersonaSet, sessions: &[Session]) -> Vec<(Need, String, Vec<String>)> {
    let mut typed: HashMap<&str, Vec<String>> = HashMap::new();
    for s in sessions {
        let qs = typed.entry(s.need.as_str()).or_default();
        for st in &s.steps {
            if st["kind"] == "search" {
                let q = st["text"].as_str().unwrap_or("").trim().to_string();
                if !q.is_empty() && !qs.iter().any(|x| x.eq_ignore_ascii_case(&q)) {
                    qs.push(q);
                }
            }
        }
    }
    let mut out = Vec::new();
    for p in &set.personas {
        for n in &p.needs {
            let mut qs = typed.get(n.id.as_str()).cloned().unwrap_or_default();
            if qs.is_empty() {
                qs.push(n.text.clone());
            }
            qs.truncate(QUERIES_PER_NEED);
            out.push((n.clone(), p.name.clone(), qs));
        }
    }
    out.truncate(MAX_NEEDS);
    out
}

pub fn judged_summary(units: &[NeedUnit], g: &Guardrails, seed: u64) -> Value {
    let a: Vec<f64> = units.iter().map(|u| u.a).collect();
    let b: Vec<f64> = units.iter().map(|u| u.b).collect();
    let mut proof = crate::tune::proof(&a, &b, seed);
    let d: Vec<f64> = units.iter().map(|u| u.b - u.a).collect();
    let s = sd(&d);
    let se = s / (d.len().max(1) as f64).sqrt();
    let delta = mean(&d);
    let regressions: Vec<&NeedUnit> = units.iter().filter(|u| u.regressed()).collect();
    let known_lost = units.iter().filter(|u| u.a_rank.is_some() && u.b_rank.is_none()).count();
    let ams: Vec<f64> = units.iter().map(|u| u.a_ms).collect();
    let bms: Vec<f64> = units.iter().map(|u| u.b_ms).collect();
    let (pa, pb) = (quantile(&ams, 0.95), quantile(&bms, 0.95));
    let can = detectable(if s > 0.0 { s } else { 0.25 }, units.len());
    let n = units.len();
    proof["sentence"] = json!(if n < 3 {
        format!("{n} needs are too few to say anything.")
    } else {
        format!(
            "nDCG@10 {:.2} → {:.2} ({:+.3}, 95% interval {:+.3} to {:+.3}); {} needs better, {} worse (sign test p = {:.3}). {n} needs can find a change of about {:.2} or more.",
            mean(&a), mean(&b), delta, proof["interval"][0].as_f64().unwrap_or(0.0), proof["interval"][1].as_f64().unwrap_or(0.0),
            proof["better"], proof["worse"], proof["p"].as_f64().unwrap_or(1.0), can
        )
    });
    proof["delta"] = json!(delta);
    proof["se"] = json!(se);
    proof["p_t"] = json!(if se > 0.0 { p_two(delta / se) } else { 1.0 });
    proof["detectable"] = json!(can);
    proof["regressions"] = json!(regressions.iter().map(|u| json!({"need": u.need, "text": u.text, "kind": u.kind, "a": u.a, "b": u.b, "a_rank": u.a_rank, "b_rank": u.b_rank})).collect::<Vec<_>>());
    proof["known_lost"] = json!(known_lost);
    proof["regressions_ok"] = json!(known_lost <= g.regressions);
    proof["a_p95"] = json!(pa);
    proof["b_p95"] = json!(pb);
    proof
}

/// A step's status in the experiment's record.
fn running_step(phase: &str) -> Value {
    json!({"status": "running", "phase": phase})
}

pub struct Trial {
    pub id: String,
    pub created: i64,
    pub doc: Mutex<Value>,
    stop: std::sync::atomic::AtomicBool,
}

impl Trial {
    pub fn new(id: &str, created: i64, spec: &TrialSpec) -> Trial {
        let doc = json!({"id": id, "created": created, "spec": spec, "steps": {}, "decision": null});
        Trial { id: id.to_string(), created, doc: Mutex::new(doc), stop: Default::default() }
    }

    pub fn of(doc: Value) -> Trial {
        Trial { id: doc["id"].as_str().unwrap_or("").to_string(), created: doc["created"].as_i64().unwrap_or(0), doc: Mutex::new(doc), stop: Default::default() }
    }

    pub fn spec(&self) -> TrialSpec {
        serde_json::from_value(self.doc.lock().unwrap_or_else(|e| e.into_inner())["spec"].clone()).expect("a kept spec")
    }

    pub fn view(&self) -> Value {
        self.doc.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn set<F: FnOnce(&mut Value)>(&self, f: F) {
        f(&mut self.doc.lock().unwrap_or_else(|e| e.into_inner()));
    }

    pub fn step(&self, name: &str) -> Value {
        self.doc.lock().unwrap_or_else(|e| e.into_inner())["steps"][name].clone()
    }

    pub fn stop(&self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    fn stopped(&self) -> bool {
        self.stop.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Step 1: the side by side.
    pub async fn side(self: Arc<Self>, source: Arc<Source>, needs: Vec<String>, save: impl Fn(&Trial)) {
        let spec = self.spec();
        self.set(|d| d["steps"]["side"] = running_step("searching"));
        save(&*self);
        let logged: Vec<String> = match source.log(now_s() - 30 * 86_400, 5000).await {
            Ok(v) => v["requests"].as_array().into_iter().flatten().filter_map(|r| r["q"].as_str().map(str::to_string)).collect(),
            Err(_) => Vec::new(),
        };
        let qs = side_queries(&logged, &needs, SIDE_QUERIES);
        let mut rows = Vec::new();
        for (q, from) in &qs {
            let (pa, pb) = tokio::join!(source.search(q, &spec.control, K), source.search(q, &spec.treatment, K));
            let (Ok(pa), Ok(pb)) = (pa, pb) else { continue };
            let ids = |p: &[Hit]| p.iter().map(|h| h.id.clone()).collect::<Vec<_>>();
            let (overlap, moved) = page_change(&ids(&pa.rows), &ids(&pb.rows));
            let rowv = |p: &[Hit]| p.iter().map(|h| json!({"id": h.id, "title": h.title})).collect::<Vec<_>>();
            rows.push(json!({"q": q, "from": from, "a": rowv(&pa.rows), "b": rowv(&pb.rows), "overlap": overlap, "moved": moved, "a_ms": pa.ms, "b_ms": pb.ms}));
        }
        let mut sum = side_summary(&rows);
        sum["status"] = json!("done");
        sum["rows"] = json!(rows);
        self.set(|d| d["steps"]["side"] = sum);
        save(&*self);
    }

    /// Step 2: the judged queries.
    pub async fn judged(self: Arc<Self>, source: Arc<Source>, set: PersonaSet, sessions: Vec<Session>, judge: Arc<Judge>, budget: f64, save: impl Fn(&Trial)) {
        let spec = self.spec();
        let work = need_queries(&set, &sessions);
        let total = work.len();
        self.set(|d| d["steps"]["judged"] = json!({"status": "running", "phase": "searching and grading", "done": 0, "total": total, "spent": 0.0, "judge": judge.model()}));
        save(&*self);
        let mut units: Vec<NeedUnit> = Vec::new();
        let mut spent = 0.0;
        let mut error = Value::Null;
        for (i, (need, persona, qs)) in work.iter().enumerate() {
            if self.stopped() {
                error = json!("stopped");
                break;
            }
            if spent >= budget {
                error = json!(format!("the judge spent its ${budget:.2}: {i} of {total} needs graded"));
                break;
            }
            let mut pages_a = Vec::new();
            let mut pages_b = Vec::new();
            let (mut ams, mut bms) = (Vec::new(), Vec::new());
            let mut all: Vec<Hit> = Vec::new();
            for q in qs {
                let (pa, pb) = tokio::join!(source.search(q, &spec.control, K), source.search(q, &spec.treatment, K));
                let (Ok(pa), Ok(pb)) = (pa, pb) else { continue };
                ams.push(pa.ms);
                bms.push(pb.ms);
                for h in pa.rows.iter().chain(pb.rows.iter()) {
                    if !all.iter().any(|x| x.id == h.id) {
                        all.push(h.clone());
                    }
                }
                pages_a.push(pa.rows.iter().map(|h| h.id.clone()).collect::<Vec<_>>());
                pages_b.push(pb.rows.iter().map(|h| h.id.clone()).collect::<Vec<_>>());
            }
            if pages_a.is_empty() {
                continue;
            }
            let grades = match judge.grade(need, &all, &HashMap::new()).await {
                Ok((_, c)) => {
                    spent += c;
                    judge.grades_of(need)
                }
                Err(e) => {
                    error = json!(e);
                    break;
                }
            };
            let rank = |pages: &[Vec<String>]| -> Option<usize> {
                let t = need.target.as_ref().filter(|_| need.kind == "known")?;
                pages.iter().filter_map(|p| p.iter().position(|x| x == t)).min().map(|r| r + 1)
            };
            units.push(NeedUnit {
                need: need.id.clone(), kind: need.kind.clone(), persona: persona.clone(), text: need.text.clone(), queries: qs.clone(),
                a: need_score(&grades, &pages_a, K), b: need_score(&grades, &pages_b, K), a_rank: rank(&pages_a), b_rank: rank(&pages_b),
                a_ms: mean(&ams), b_ms: mean(&bms),
            });
            let done = i + 1;
            self.set(|d| {
                d["steps"]["judged"]["done"] = json!(done);
                d["steps"]["judged"]["spent"] = json!(spent);
            });
            if done % 5 == 0 {
                save(&*self);
            }
        }
        let mut sum = judged_summary(&units, &spec.guardrails, 7);
        sum["status"] = json!(if error.is_null() || units.len() >= 3 { "done" } else { "failed" });
        sum["error"] = error;
        sum["units"] = json!(units);
        sum["spent"] = json!(spent);
        sum["judge"] = json!(judge.model());
        sum["total"] = json!(total);
        sum["done"] = json!(units.len());
        self.set(|d| d["steps"]["judged"] = sum);
        save(&*self);
    }
}

// ------------------------------------------------------------ real visitors

/// One interleaved search: when, what, on what screen, which arm each
/// story shown came from, and what was opened.
#[derive(Clone, Debug, Serialize)]
pub struct SearchUnit {
    pub ts: i64,
    pub q: String,
    pub dev: String,
    pub words: usize,
    /// Stories opened from each arm's picks.
    pub ca: usize,
    pub cb: usize,
    /// +1 the treatment won (more of its stories opened), −1 the control, 0 even.
    pub outcome: i8,
    /// The arm that picked first (the draft's first coin).
    pub b_first: bool,
    pub a_ms: Option<f64>,
    pub b_ms: Option<f64>,
}

pub fn search_units(log: &Value) -> Vec<SearchUnit> {
    let mut out: Vec<SearchUnit> = log["requests"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            // Ids as text: a site's ids are numbers or words.
            let key = |v: &Value| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string());
            let shown: Vec<String> = r["shown"].as_array()?.iter().map(key).collect();
            let teams: Vec<char> = r["teams"].as_str()?.chars().collect();
            if shown.is_empty() || teams.len() != shown.len() {
                return None;
            }
            let clicks: Vec<String> = r["clicks"].as_array().into_iter().flatten().map(key).collect();
            let (mut ca, mut cb) = (0, 0);
            for c in &clicks {
                match shown.iter().position(|s| s == c).map(|i| teams[i]) {
                    Some('a') => ca += 1,
                    Some('b') => cb += 1,
                    _ => {}
                }
            }
            let q = r["q"].as_str().unwrap_or("").to_string();
            Some(SearchUnit {
                ts: r["ts"].as_i64().unwrap_or(0), words: q.split_whitespace().count(), q, dev: r["dev"].as_str().unwrap_or("").to_string(),
                ca, cb, outcome: (cb as i64 - ca as i64).signum() as i8, b_first: teams[0] == 'b', a_ms: r["a_ms"].as_f64(), b_ms: r["b_ms"].as_f64(),
            })
        })
        .collect();
    out.sort_by_key(|u| u.ts);
    out
}

/// The preference of the treatment over the control after these searches:
/// (wins − losses) / searches, its standard error.
fn preference(units: &[SearchUnit]) -> (f64, f64) {
    let n = units.len();
    if n == 0 {
        return (0.0, 0.0);
    }
    let xs: Vec<f64> = units.iter().map(|u| u.outcome as f64).collect();
    (mean(&xs), sd(&xs) / (n as f64).sqrt())
}

/// The real visitors' step: the preference and its interval, the
/// always-valid p value, each day, the coin's balance, the latency
/// guardrail, and how long until it can tell.
pub fn online_summary(units: &[SearchUnit], spec: &TrialSpec, now: i64) -> Value {
    let n = units.len();
    let wins = units.iter().filter(|u| u.outcome > 0).count();
    let losses = units.iter().filter(|u| u.outcome < 0).count();
    let (delta, se) = preference(units);
    // The always-valid p value over the searches as they came.
    let mut seq = Vec::with_capacity(n);
    for i in 1..=n {
        let (d, s) = preference(&units[..i]);
        seq.push((d, if i >= 20 { s } else { f64::NAN }));
    }
    let av = always_valid(&seq, TAU);
    let p_av = av.last().copied().unwrap_or(1.0);
    // By day (UTC), cumulative.
    let mut days: Vec<Value> = Vec::new();
    let mut i = 0;
    while i < n {
        let day = units[i].ts.div_euclid(86_400);
        let mut j = i;
        while j < n && units[j].ts.div_euclid(86_400) == day {
            j += 1;
        }
        let (d, s) = preference(&units[..j]);
        let (dd, ds) = preference(&units[i..j]);
        days.push(json!({
            "day": day * 86_400, "searches": j - i, "total": j, "delta": d, "lo": d - Z975 * s, "hi": d + Z975 * s, "p_always_valid": av[j - 1],
            "day_delta": dd, "day_lo": dd - Z975 * ds, "day_hi": dd + Z975 * ds,
        }));
        i = j;
    }
    // The coin: the treatment picks first in half the searches.
    let first_b = units.iter().filter(|u| u.b_first).count();
    let coin_p = if n == 0 { 1.0 } else { 2.0 * crate::stats::binom_cdf(first_b.min(n - first_b) as u64, n as u64, 0.5).min(0.5) };
    let ams: Vec<f64> = units.iter().filter_map(|u| u.a_ms).collect();
    let bms: Vec<f64> = units.iter().filter_map(|u| u.b_ms).collect();
    let (pa, pb) = (quantile(&ams, 0.95), quantile(&bms, 0.95));
    let latency_ok = match (pa, pb) {
        (Some(a), Some(b)) if a > 0.0 => Some(b <= a * (1.0 + spec.guardrails.latency_p95)),
        _ => None,
    };
    let clicked = units.iter().filter(|u| u.ca + u.cb > 0).count();
    let spread = {
        let xs: Vec<f64> = units.iter().map(|u| u.outcome as f64).collect();
        if n >= 20 { sd(&xs) } else { 0.6 }
    };
    let per_day = match (units.first(), units.last()) {
        (Some(f), Some(_)) if n > 0 => n as f64 / (((now - f.ts) as f64 / 86_400.0).max(1.0)),
        _ => 0.0,
    };
    let need = needed(spread, spec.smallest);
    let days_left = if per_day > 0.0 { ((need - n as f64).max(0.0) / per_day).ceil() } else { f64::NAN };
    json!({
        "searches": n, "wins": wins, "losses": losses, "ties": n - wins - losses, "clicked": clicked,
        "delta": delta, "se": se, "lo": delta - Z975 * se, "hi": delta + Z975 * se, "p": if se > 0.0 { p_two(delta / se) } else { 1.0 }, "p_always_valid": p_av,
        "coin": {"b_first": first_b, "p": coin_p, "ok": coin_p >= 0.001},
        "a_p95": pa, "b_p95": pb, "latency_ok": latency_ok,
        "detectable": detectable(spread, n), "needed": need.ceil(), "per_day": per_day, "days_left": if days_left.is_finite() { json!(days_left) } else { Value::Null },
        "days": days,
    })
}

// ------------------------------------------------------------ the decision

/// The decision a rule declared in advance gives on the evidence so far:
/// the real visitors' preference when there is one, else the judged
/// queries'. Returns the call (`invalid`, `ship`, `not_yet`, `tradeoff`,
/// `flat`, `dont_ship`, `inconclusive`, `none`), a title and a sentence.
pub fn rule(spec: &TrialSpec, judged: &Value, online: &Value) -> Value {
    let g = &spec.guardrails;
    let mut guard: Vec<(String, Option<bool>)> = Vec::new();
    let known_lost = judged["known_lost"].as_u64();
    if let Some(k) = known_lost {
        guard.push((format!("known items lost from the first page: {k} (at most {})", g.regressions), Some(k as usize <= g.regressions)));
    }
    let lat = online["latency_ok"].as_bool().or(match (judged["a_p95"].as_f64(), judged["b_p95"].as_f64()) {
        (Some(a), Some(b)) if a > 0.0 => Some(b <= a * (1.0 + g.latency_p95)),
        _ => None,
    });
    let (la, lb) = if online["a_p95"].is_number() { (online["a_p95"].as_f64(), online["b_p95"].as_f64()) } else { (judged["a_p95"].as_f64(), judged["b_p95"].as_f64()) };
    if let (Some(a), Some(b)) = (la, lb) {
        guard.push((format!("p95 latency {a:.0} → {b:.0} ms (at most +{:.0}%)", g.latency_p95 * 100.0), lat));
    }
    let failed: Vec<&String> = guard.iter().filter(|(_, ok)| *ok == Some(false)).map(|(t, _)| t).collect();
    let out = |call: &str, title: &str, sentence: String| json!({"call": call, "title": title, "sentence": sentence, "guardrails": guard.iter().map(|(t, ok)| json!({"text": t, "ok": ok})).collect::<Vec<_>>()});
    let n = online["searches"].as_u64().unwrap_or(0);
    if n >= 20 {
        if online["coin"]["ok"] == json!(false) {
            return out("invalid", "Don't read this result", format!("The treatment picked first in {} of {n} searches, not about half (p = {:.4}): the draft is broken. Find the cause and start again.", online["coin"]["b_first"], online["coin"]["p"].as_f64().unwrap_or(0.0)));
        }
        let (d, lo, hi, pav) = (online["delta"].as_f64().unwrap_or(0.0), online["lo"].as_f64().unwrap_or(0.0), online["hi"].as_f64().unwrap_or(0.0), online["p_always_valid"].as_f64().unwrap_or(1.0));
        let pref = format!("preference {d:+.3} (95% interval {lo:+.3} to {hi:+.3}, always-valid p = {pav:.3}) over {n} searches");
        if pav < 0.05 && d > 0.0 {
            if !failed.is_empty() {
                return out("tradeoff", "Trade-off, your call", format!("Visitors prefer the treatment: {pref}. But a guardrail failed: {}. Write down why the gain is worth it, or don't ship.", failed.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("; ")));
            }
            if guard.iter().any(|(_, ok)| ok.is_none()) || guard.is_empty() {
                return out("not_yet", "Not yet", format!("Visitors prefer the treatment: {pref}. A guardrail is not measured yet: score it on judged queries first."));
            }
            return out("ship", "Ship", format!("Visitors prefer the treatment: {pref}; every guardrail holds."));
        }
        if pav < 0.05 && d < 0.0 {
            return out("dont_ship", "Don't ship", format!("Visitors prefer the control: {pref}."));
        }
        if hi < spec.smallest && lo > -spec.smallest {
            return out("flat", "No difference", format!("No preference worth having: {pref}, inside ±{:.3}. Ship only if the treatment is simpler or cheaper.", spec.smallest));
        }
        let left = online["days_left"].as_f64().map(|x| format!(" About {x:.0} more days at this traffic to find a preference of {:.2}.", spec.smallest)).unwrap_or_default();
        return out("inconclusive", "Not yet", format!("No call yet: {pref}.{left}"));
    }
    if judged["status"] == "done" {
        let verdict = judged["verdict"].as_str().unwrap_or("");
        let s = judged["sentence"].as_str().unwrap_or("").to_string();
        return match verdict {
            "worse" => out("dont_ship", "Don't ship", format!("Worse on the judged queries. {s}")),
            "better" if !failed.is_empty() => out("tradeoff", "Trade-off, your call", format!("Better on the judged queries, but a guardrail failed: {}. {s}", failed.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("; "))),
            "better" => out("not_yet", "Not yet: try it on real visitors", format!("Better on the judged queries; real visitors decide. {s}")),
            _ if !failed.is_empty() => out("dont_ship", "Don't ship", format!("No gain on the judged queries, and a guardrail failed: {}. {s}", failed.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("; "))),
            _ => out("inconclusive", "Inconclusive", format!("No clear difference on the judged queries. {s}")),
        };
    }
    out("none", "No evidence yet", "Score it on judged queries, or start it on real visitors.".into())
}

/// The studies whose sessions give a persona set's typed queries.
pub fn sessions_of_set(studies: &[(Value, Vec<Session>)], set: &str) -> Vec<Session> {
    studies.iter().filter(|(v, _)| v["spec"]["personas"] == json!(set) && v["status"] == "done").flat_map(|(_, s)| s.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> TrialSpec {
        serde_json::from_value(json!({"name": "x", "source": "s", "treatment_name": "B", "treatment": {"votes": 1}})).unwrap()
    }

    #[test]
    fn side_queries_take_the_log_first() {
        let log: Vec<String> = ["rust", "Rust", "go", "rust", "zig"].iter().map(|s| s.to_string()).collect();
        let q = side_queries(&log, &["go".into(), "a need".into()], 4);
        assert_eq!(q.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["rust", "go", "zig", "a need"]);
        assert_eq!(q[3].1, "need");
        let (overlap, moved) = page_change(&["a".into(), "b".into()], &["b".into(), "c".into()]);
        assert!((overlap - 0.5).abs() < 1e-9 && moved == 2);
    }

    #[test]
    fn a_spec_needs_two_arms_that_differ() {
        assert!(spec().check().is_ok());
        let mut s = spec();
        s.treatment = json!({});
        assert!(s.check().is_err());
        let mut s = spec();
        s.treatment_name = "As it is".into();
        assert!(s.check().is_err());
    }

    fn unit(ts: i64, outcome: i8, b_first: bool) -> SearchUnit {
        SearchUnit { ts, q: "q".into(), dev: "d".into(), words: 1, ca: (outcome < 0) as usize, cb: (outcome > 0) as usize, outcome, b_first, a_ms: Some(10.0), b_ms: Some(10.5) }
    }

    #[test]
    fn units_credit_clicks_to_the_arm_that_picked() {
        let log = json!({"requests": [
            {"ts": 5, "q": "two words", "shown": [1, 2, 3], "teams": "bab", "clicks": [1, 3], "dev": "m"},
            {"ts": 1, "q": "x", "shown": [1, 2], "teams": "ab", "clicks": [1]},
            {"ts": 2, "q": "x", "shown": [1, 2], "teams": "abb", "clicks": []},
        ]});
        let u = search_units(&log);
        assert_eq!(u.len(), 2, "a broken row is left out");
        assert_eq!((u[0].ca, u[0].cb, u[0].outcome), (1, 0, -1));
        assert_eq!((u[1].cb, u[1].outcome, u[1].b_first, u[1].words), (2, 1, true, 2));
    }

    #[test]
    fn the_rule_ships_a_clear_preference_and_holds_a_broken_coin() {
        let s = spec();
        let judged = json!({"status": "done", "verdict": "better", "known_lost": 0, "a_p95": 100.0, "b_p95": 105.0, "sentence": ""});
        // 600 searches, the treatment winning 30% and losing 15%.
        let units: Vec<SearchUnit> = (0..600).map(|i| unit(i * 600, if i % 20 < 6 { 1 } else if i % 20 < 9 { -1 } else { 0 }, i % 2 == 0)).collect();
        let on = online_summary(&units, &s, 600 * 600);
        assert!((on["delta"].as_f64().unwrap() - 0.15).abs() < 1e-9, "{on}");
        assert!(on["p_always_valid"].as_f64().unwrap() < 0.05, "{on}");
        assert_eq!(rule(&s, &judged, &on)["call"], "ship");
        // A guardrail past its margin: a trade-off.
        let lost = json!({"status": "done", "verdict": "better", "known_lost": 2, "sentence": ""});
        assert_eq!(rule(&s, &lost, &on)["call"], "tradeoff");
        // The treatment always first: the draft is broken.
        let bad: Vec<SearchUnit> = units.iter().map(|u| SearchUnit { b_first: true, ..u.clone() }).collect();
        assert_eq!(rule(&s, &judged, &online_summary(&bad, &s, 600 * 600))["call"], "invalid");
        // Nothing online: the judged queries say try it on visitors.
        assert_eq!(rule(&s, &judged, &json!({}))["call"], "not_yet");
        assert_eq!(rule(&s, &json!({}), &json!({}))["call"], "none");
        // Days are cumulative and end at the total.
        let days = on["days"].as_array().unwrap();
        assert_eq!(days.last().unwrap()["total"], 600);
    }

    #[test]
    fn judged_summary_lists_regressions() {
        let u = |a: f64, b: f64, ar: Option<usize>, br: Option<usize>| NeedUnit { need: "n".into(), kind: "known".into(), persona: "p".into(), text: "t".into(), queries: vec![], a, b, a_rank: ar, b_rank: br, a_ms: 10.0, b_ms: 10.0 };
        let units = vec![u(1.0, 0.0, Some(1), None), u(0.5, 0.6, Some(2), Some(1)), u(0.4, 0.5, None, None), u(0.3, 0.4, None, None)];
        let s = judged_summary(&units, &Guardrails::default(), 1);
        assert_eq!(s["known_lost"], 1);
        assert_eq!(s["regressions"].as_array().unwrap().len(), 1);
        assert_eq!(s["regressions_ok"], false);
        assert!(s["detectable"].as_f64().unwrap() > 0.0);
    }
}
