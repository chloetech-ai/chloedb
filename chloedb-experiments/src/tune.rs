//! Tune: the relevance autopilot's method (`chloedb/src/autopilot`) on a
//! real site's knobs. A finished study gives the workload: every need, the
//! searches its agents typed for it on any arm, and the grades the study
//! kept (a known need's row; the judge's grades of a topical need's rows).
//!
//! ```text
//! study ──► workload: each need with the queries typed for it, and its grades
//!       ──► split: a third of the needs held out, by a hash of the need
//!       ──► search: coordinate ascent over the site's numeric knobs, each
//!           candidate's pages fetched from the site's experiment search and
//!           scored by nDCG on the training needs
//!       ──► holes: rows the control or the proposal shows on a held-out
//!           need that no one graded go to the judge (within the budget)
//!       ──► proof: held-out needs before and after, the needs made better
//!           and worse, the sign test, a bootstrap interval, the site's time
//!           per search; and a verdict
//! ```
//!
//! The held-out needs were not seen by the search, so the gain claimed is
//! not the gain fitted. A proposal is a set of knobs, so it becomes an arm:
//! the page offers to prove it with agents, on people the search never saw.
//! Nothing is written to the site.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::judge::{need_key, Judge};
use crate::live;
use crate::personas::{Need, PersonaSet};
use crate::source::{Hit, Source};
use crate::study::{self, Session};

/// The most searches one tune sends the site.
pub const MAX_SEARCHES: usize = 4000;
/// The most the judge may spend on holes, in dollars.
pub const MAX_BUDGET: f64 = 5.0;
/// Queries of one need that count, at most.
const QUERIES_PER_NEED: usize = 4;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct TuneSpec {
    #[serde(default)]
    pub name: String,
    /// The finished study whose needs, queries and grades are the workload.
    pub study: String,
    /// The source tuned: an app that takes experiments.
    pub source: String,
    /// The knobs tuned from (none: the site as it is).
    #[serde(default)]
    pub base: Value,
    /// The knobs searched (none: every number the site offers).
    #[serde(default)]
    pub keys: Vec<String>,
    /// nDCG of the first `k` rows.
    #[serde(default = "k")]
    pub k: usize,
    /// Rounds of coordinate ascent.
    #[serde(default = "rounds")]
    pub rounds: usize,
    #[serde(default = "seed")]
    pub seed: u64,
    /// Grade the held-out holes with the judge.
    #[serde(default = "yes")]
    pub judge: bool,
    /// `claude` or `scripted`, for the judge.
    #[serde(default = "brain")]
    pub brain: String,
    #[serde(default = "judge_model")]
    pub judge_model: String,
    /// The most the judge may spend on holes, in dollars.
    #[serde(default = "budget")]
    pub budget: f64,
}

fn k() -> usize {
    10
}
fn rounds() -> usize {
    2
}
fn seed() -> u64 {
    7
}
fn yes() -> bool {
    true
}
fn brain() -> String {
    "claude".into()
}
fn judge_model() -> String {
    "claude-sonnet-5-5".into()
}
fn budget() -> f64 {
    1.0
}

impl TuneSpec {
    pub fn check(&self) -> Result<(), String> {
        if !(3..=20).contains(&self.k) {
            return Err("nDCG of 3 to 20 rows".into());
        }
        if !(1..=4).contains(&self.rounds) {
            return Err("1 to 4 rounds".into());
        }
        if !(self.budget >= 0.0 && self.budget <= MAX_BUDGET) {
            return Err(format!("the judge's budget is $0 to ${MAX_BUDGET}"));
        }
        if self.brain != "claude" && self.brain != "scripted" {
            return Err("the judge is claude or scripted".into());
        }
        if !self.base.is_null() && !self.base.is_object() {
            return Err("base is an object of knobs".into());
        }
        Ok(())
    }
}

/// A knob the search may move.
#[derive(Clone, Debug, PartialEq)]
pub struct Knob {
    pub key: String,
    pub title: String,
    pub int: bool,
    pub min: f64,
    pub max: f64,
    pub step: f64,
}

/// The numeric knobs of a site, as `describe` lists them.
pub fn numeric(described: &Value) -> Vec<Knob> {
    described["knobs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|k| matches!(k["kind"].as_str(), Some("number" | "int")))
        .filter_map(|k| {
            let int = k["kind"] == "int";
            Some(Knob {
                key: k["key"].as_str()?.to_string(),
                title: k["title"].as_str().unwrap_or("").to_string(),
                int,
                min: k["min"].as_f64()?,
                max: k["max"].as_f64()?,
                step: k["step"].as_f64().unwrap_or(if int { 1.0 } else { 0.01 }),
            })
        })
        .collect()
}

/// The site's defaults, with `base` over them.
pub fn knobs_of(described: &Value, base: &Value) -> Map<String, Value> {
    let mut m = Map::new();
    for k in described["knobs"].as_array().into_iter().flatten() {
        if let Some(key) = k["key"].as_str() {
            // The ranking expression is left out: set, it replaces the
            // numbers the search moves.
            if k["kind"] != "sql" && !k["default"].is_null() {
                m.insert(key.to_string(), k["default"].clone());
            }
        }
    }
    if let Some(b) = base.as_object() {
        for (k, v) in b {
            m.insert(k.clone(), v.clone());
        }
    }
    m
}

/// The values to try for a knob now at `v`: halved and doubled (a number
/// at 0 tries a tenth and a third of its range), one less and one more
/// (an integer), within its range and rounded to its step.
pub fn moves(k: &Knob, v: f64) -> Vec<f64> {
    let raw = if k.int {
        vec![v - 1.0, v + 1.0]
    } else if v.abs() < 1e-12 {
        vec![k.min + (k.max - k.min) * 0.1, k.min + (k.max - k.min) / 3.0]
    } else {
        vec![v * 0.5, v * 2.0]
    };
    let mut out: Vec<f64> = Vec::new();
    for x in raw {
        let x = x.clamp(k.min, k.max);
        let x = if k.step > 0.0 { (x / k.step).round() * k.step } else { x };
        let x = (x * 1e6).round() / 1e6;
        if (x - v).abs() > 1e-9 && !out.iter().any(|y| (y - x).abs() < 1e-9) {
            out.push(x);
        }
    }
    out
}

/// One need of the workload.
#[derive(Clone, Debug)]
pub struct Work {
    pub need: Need,
    pub queries: Vec<String>,
    pub held_out: bool,
}

/// The workload of a study: each need with the queries typed for it (the
/// first few, in the order they were typed), held out by a hash of it.
pub fn workload(set: &PersonaSet, sessions: &[Session], seed: u64) -> Vec<Work> {
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
            let mut queries = typed.get(n.id.as_str()).cloned().unwrap_or_default();
            queries.truncate(QUERIES_PER_NEED);
            if queries.is_empty() {
                continue;
            }
            let h = u64::from_str_radix(&need_key(n), 16).unwrap_or(0) ^ seed.wrapping_mul(0x9e37_79b9_7f4a_7c15);
            out.push(Work { need: n.clone(), queries, held_out: mix(h) % 3 == 0 });
        }
    }
    // Both sides need needs: with every need on one side, move one.
    if out.len() >= 2 {
        if out.iter().all(|w| w.held_out) {
            out[0].held_out = false;
        } else if !out.iter().any(|w| w.held_out) {
            let last = out.len() - 1;
            out[last].held_out = true;
        }
    }
    out
}

fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// A need's score: the mean nDCG@k of its queries' pages.
pub fn need_score(grades: &HashMap<String, u8>, pages: &[Vec<String>], k: usize) -> f64 {
    if pages.is_empty() {
        return 0.0;
    }
    pages.iter().map(|p| live::ndcg(grades, 2, p, k)).sum::<f64>() / pages.len() as f64
}

/// The proof over the held-out needs' scores before and after.
pub fn proof(before: &[f64], after: &[f64], seed: u64) -> Value {
    let n = before.len().min(after.len());
    let d: Vec<f64> = (0..n).map(|i| after[i] - before[i]).collect();
    let better = d.iter().filter(|x| **x > 1e-9).count();
    let worse = d.iter().filter(|x| **x < -1e-9).count();
    let p = study::mcnemar(worse, better);
    let mean = |xs: &[f64]| if xs.is_empty() { 0.0 } else { xs.iter().sum::<f64>() / xs.len() as f64 };
    let gain = mean(&d);
    // A bootstrap interval of the mean difference, 2,000 resamples.
    let mut z = seed ^ 0x5eed;
    let mut means: Vec<f64> = (0..2000)
        .map(|_| {
            if n == 0 {
                return 0.0;
            }
            (0..n)
                .map(|_| {
                    z = mix(z);
                    d[(z % n as u64) as usize]
                })
                .sum::<f64>()
                / n as f64
        })
        .collect();
    means.sort_by(|a, b| a.total_cmp(b));
    let (lo, hi) = if n == 0 { (0.0, 0.0) } else { (means[50], means[1949]) };
    let verdict = if n < 3 {
        "too few"
    } else if better > worse && p < 0.05 && lo > 0.0 {
        "better"
    } else if worse > better && p < 0.05 && hi < 0.0 {
        "worse"
    } else {
        "no clear difference"
    };
    let sentence = match verdict {
        "too few" => format!("{n} held-out needs are too few to say: run a study with more needs, then tune again."),
        "better" => format!("Better on needs the search never saw: nDCG@k {:.2} → {:.2}, {better} needs better and {worse} worse (sign test p = {p:.3}). Prove it with people next.", mean(before), mean(after)),
        "worse" => format!("Worse on the held-out needs: nDCG@k {:.2} → {:.2}, {worse} needs worse and {better} better (p = {p:.3}). The gain on the training needs was fitted, not real.", mean(before), mean(after)),
        _ => format!("No clear difference on the held-out needs: nDCG@k {:.2} → {:.2}, {better} better and {worse} worse (p = {p:.3}). More needs would tell.", mean(before), mean(after)),
    };
    json!({
        "needs": n, "before": mean(before), "after": mean(after), "gain": gain, "interval": [lo, hi],
        "better": better, "worse": worse, "same": n - better - worse, "p": p, "verdict": verdict, "sentence": sentence,
    })
}

fn p95(xs: &mut [f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    xs.sort_by(|a, b| a.total_cmp(b));
    xs[((xs.len() as f64 * 0.95).ceil() as usize).saturating_sub(1).min(xs.len() - 1)]
}

struct State {
    status: String,
    phase: String,
    searches: usize,
    spent: f64,
    steps: Vec<Value>,
    report: Value,
    error: Option<String>,
    ended: Option<i64>,
}

pub struct Tune {
    pub id: String,
    pub spec: TuneSpec,
    pub created: i64,
    /// The persona set of the study tuned on.
    pub personas: String,
    source: Arc<Source>,
    judge: Arc<Judge>,
    work: Vec<Work>,
    st: Mutex<State>,
    stop: AtomicBool,
    /// Pages fetched, by query and knobs: rows and the site's time.
    pages: tokio::sync::Mutex<HashMap<(String, String), (Vec<Hit>, f64)>>,
}

impl Tune {
    pub fn new(id: &str, spec: TuneSpec, set: &PersonaSet, sessions: &[Session], source: Arc<Source>, judge: Arc<Judge>, created: i64) -> Tune {
        let work = workload(set, sessions, spec.seed);
        Tune {
            id: id.into(),
            spec,
            created,
            personas: set.id.clone(),
            source,
            judge,
            work,
            st: Mutex::new(State { status: "running".into(), phase: "starting".into(), searches: 0, spent: 0.0, steps: Vec::new(), report: Value::Null, error: None, ended: None }),
            stop: AtomicBool::new(false),
            pages: tokio::sync::Mutex::new(HashMap::new()),
        }
    }

    pub fn needs(&self) -> usize {
        self.work.len()
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    pub fn running(&self) -> bool {
        self.st.lock().unwrap_or_else(|e| e.into_inner()).status == "running"
    }

    fn set<F: FnOnce(&mut State)>(&self, f: F) {
        f(&mut self.st.lock().unwrap_or_else(|e| e.into_inner()));
    }

    pub fn view(&self) -> Value {
        let st = self.st.lock().unwrap_or_else(|e| e.into_inner());
        json!({
            "id": self.id, "name": self.spec.name, "created": self.created, "spec": self.spec, "personas": self.personas,
            "needs": self.work.len(), "held_out": self.work.iter().filter(|w| w.held_out).count(),
            "status": st.status, "phase": st.phase, "searches": st.searches, "spent": st.spent,
            "steps": st.steps, "report": st.report, "error": st.error, "ended": st.ended,
        })
    }

    /// A page of the site for a query under `knobs`, fetched once.
    async fn page(&self, q: &str, knobs: &Map<String, Value>) -> Result<(Vec<Hit>, f64), String> {
        let key = (q.to_string(), Value::Object(knobs.clone()).to_string());
        if let Some(p) = self.pages.lock().await.get(&key) {
            return Ok(p.clone());
        }
        if self.st.lock().unwrap_or_else(|e| e.into_inner()).searches >= MAX_SEARCHES {
            return Err(format!("the tune reached its {MAX_SEARCHES} searches"));
        }
        let got = self.source.search(q, &Value::Object(knobs.clone()), self.spec.k).await?;
        self.set(|s| s.searches += 1);
        let p = (got.rows, got.ms);
        self.pages.lock().await.insert(key, p.clone());
        Ok(p)
    }

    /// Each need's score under `knobs`, of the needs on one side.
    async fn scores(&self, knobs: &Map<String, Value>, held_out: bool, grades: &HashMap<String, HashMap<String, u8>>) -> Result<Vec<f64>, String> {
        let mut out = Vec::new();
        for w in self.work.iter().filter(|w| w.held_out == held_out) {
            let mut pages = Vec::new();
            // Two at a time: the site sees a person or two, not load.
            for pair in w.queries.chunks(2) {
                let got = futures_all(pair.iter().map(|q| self.page(q, knobs))).await;
                for g in got {
                    pages.push(g?.0.into_iter().map(|h| h.id).collect::<Vec<_>>());
                }
            }
            out.push(need_score(grades.get(&w.need.id).unwrap_or(&HashMap::new()), &pages, self.spec.k));
        }
        Ok(out)
    }

    pub async fn run(self: Arc<Self>, described: Value, save: impl Fn(Arc<Tune>) + Send + Sync + 'static) {
        let r = self.clone().go(&described, &save).await;
        self.set(|s| {
            match r {
                Ok(()) => s.status = if self.stop.load(Ordering::SeqCst) { "stopped".into() } else { "done".into() },
                Err(e) => {
                    s.status = "failed".into();
                    s.error = Some(e);
                }
            }
            s.phase = String::new();
            s.ended = Some(crate::app::now_ms());
        });
        save(self.clone());
    }

    async fn go(self: Arc<Self>, described: &Value, save: &(impl Fn(Arc<Tune>) + Send + Sync)) -> Result<(), String> {
        let mean = |xs: &[f64]| if xs.is_empty() { 0.0 } else { xs.iter().sum::<f64>() / xs.len() as f64 };
        if self.work.len() < 4 {
            return Err(format!("the study has {} needs with searches: a tune needs at least 4", self.work.len()));
        }
        let all = numeric(described);
        let knobs: Vec<Knob> = if self.spec.keys.is_empty() { all } else { all.into_iter().filter(|k| self.spec.keys.contains(&k.key)).collect() };
        if knobs.is_empty() {
            return Err("the site offers no numbers to tune".into());
        }
        self.set(|s| s.phase = "reading the study's grades".into());
        let mut grades: HashMap<String, HashMap<String, u8>> = HashMap::new();
        for w in &self.work {
            grades.insert(w.need.id.clone(), self.judge.load(&w.need).await);
        }
        let base = knobs_of(described, &self.spec.base);
        self.set(|s| s.phase = "the site as it is".into());
        let start = mean(&self.scores(&base, false, &grades).await?);
        self.set(|s| s.steps.push(json!({"key": null, "title": "the site as it is", "value": null, "score": start, "kept": true})));
        let mut cur = base.clone();
        let mut best = start;
        'rounds: for round in 0..self.spec.rounds {
            let mut moved = false;
            for k in &knobs {
                let v = cur.get(&k.key).and_then(Value::as_f64).unwrap_or(k.min);
                for x in moves(k, v) {
                    if self.stop.load(Ordering::SeqCst) {
                        break 'rounds;
                    }
                    let mut cand = cur.clone();
                    cand.insert(k.key.clone(), if k.int { json!(x as i64) } else { json!(x) });
                    self.set(|s| s.phase = format!("round {}: {} = {x}", round + 1, k.title));
                    let score = mean(&self.scores(&cand, false, &grades).await?);
                    let kept = score > best + 1e-4;
                    self.set(|s| s.steps.push(json!({"round": round + 1, "key": k.key, "title": k.title, "value": cand[&k.key], "score": score, "kept": kept})));
                    if kept {
                        best = score;
                        cur = cand;
                        moved = true;
                    }
                }
                save(self.clone());
            }
            if !moved {
                break;
            }
        }
        // Holes: rows of the held-out pages no one graded, to the judge.
        let mut holes = 0usize;
        let mut graded = 0usize;
        let mut cost = 0.0;
        for w in self.work.iter().filter(|w| w.held_out && w.need.kind != "known") {
            let mut rows: Vec<Hit> = Vec::new();
            for side in [&base, &cur] {
                for q in &w.queries {
                    for h in self.page(q, side).await?.0 {
                        if !grades[&w.need.id].contains_key(&h.id) && !rows.iter().any(|r| r.id == h.id) {
                            rows.push(h);
                        }
                    }
                }
            }
            holes += rows.len();
            if self.spec.judge && !rows.is_empty() && cost < self.spec.budget && !self.stop.load(Ordering::SeqCst) {
                self.set(|s| s.phase = "grading the rows no one graded".into());
                let (_, c) = self.judge.grade(&w.need, &rows, &HashMap::new()).await?;
                cost += c;
                graded += rows.len();
                grades.insert(w.need.id.clone(), self.judge.grades_of(&w.need));
                self.set(|s| s.spent = cost);
            }
        }
        self.set(|s| s.phase = "the proof on the held-out needs".into());
        let before = self.scores(&base, true, &grades).await?;
        let after = self.scores(&cur, true, &grades).await?;
        let train_after = mean(&self.scores(&cur, false, &grades).await?);
        let mut ms_before = Vec::new();
        let mut ms_after = Vec::new();
        {
            let pages = self.pages.lock().await;
            for w in self.work.iter().filter(|w| w.held_out) {
                for q in &w.queries {
                    for (side, out) in [(&base, &mut ms_before), (&cur, &mut ms_after)] {
                        if let Some(p) = pages.get(&(q.clone(), Value::Object(side.clone()).to_string())) {
                            out.push(p.1);
                        }
                    }
                }
            }
        }
        let held: Vec<&Work> = self.work.iter().filter(|w| w.held_out).collect();
        let per_need: Vec<Value> = held.iter().enumerate().map(|(i, w)| json!({"need": w.need.id, "kind": w.need.kind, "text": w.need.text, "queries": w.queries, "before": before[i], "after": after[i]})).collect();
        let changed: Vec<Value> = knobs
            .iter()
            .filter(|k| base.get(&k.key) != cur.get(&k.key))
            .map(|k| json!({"key": k.key, "title": k.title, "from": base.get(&k.key), "to": cur.get(&k.key)}))
            .collect();
        let tuned: Map<String, Value> = knobs.iter().filter_map(|k| cur.get(&k.key).map(|v| (k.key.clone(), v.clone()))).collect();
        let mut proposal = self.spec.base.as_object().cloned().unwrap_or_default();
        proposal.extend(tuned);
        let report = json!({
            "base": Value::Object(self.spec.base.as_object().cloned().unwrap_or_default()),
            "proposal": Value::Object(proposal),
            "changed": changed,
            "train": {"needs": self.work.len() - held.len(), "before": start, "after": train_after},
            "held_out": proof(&before, &after, self.spec.seed),
            "per_need": per_need,
            "holes": {"found": holes, "graded": graded, "cost": cost},
            "ms_p95": {"before": p95(&mut ms_before), "after": p95(&mut ms_after)},
            "k": self.spec.k,
        });
        self.set(|s| s.report = report);
        Ok(())
    }
}

/// Every future, run at once, their results in order.
async fn futures_all<F: std::future::Future>(fs: impl Iterator<Item = F>) -> Vec<F::Output> {
    let hs: Vec<_> = fs.collect();
    let mut out = Vec::with_capacity(hs.len());
    // Few at a time (two), so joining in order is enough.
    let mut pinned: Vec<std::pin::Pin<Box<F>>> = hs.into_iter().map(Box::pin).collect();
    let mut done: Vec<Option<F::Output>> = (0..pinned.len()).map(|_| None).collect();
    std::future::poll_fn(|cx| {
        let mut all = true;
        for (i, f) in pinned.iter_mut().enumerate() {
            if done[i].is_none() {
                match f.as_mut().poll(cx) {
                    std::task::Poll::Ready(v) => done[i] = Some(v),
                    std::task::Poll::Pending => all = false,
                }
            }
        }
        if all {
            std::task::Poll::Ready(())
        } else {
            std::task::Poll::Pending
        }
    })
    .await;
    for d in done {
        out.extend(d);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn knob(int: bool) -> Knob {
        Knob { key: "votes".into(), title: "Votes".into(), int, min: 0.0, max: 2.0, step: 0.05 }
    }

    #[test]
    fn moves_halve_double_and_stay_in_range() {
        assert_eq!(moves(&knob(false), 0.4), vec![0.2, 0.8]);
        assert_eq!(moves(&knob(false), 1.5), vec![0.75, 2.0]);
        assert_eq!(moves(&knob(false), 0.0), vec![0.2, 0.65]);
        let i = Knob { int: true, min: 0.0, max: 10.0, step: 1.0, ..knob(true) };
        assert_eq!(moves(&i, 0.0), vec![1.0]);
        assert_eq!(moves(&i, 3.0), vec![2.0, 4.0]);
    }

    #[test]
    fn proof_says_better_only_with_a_clear_gain() {
        let before = vec![0.2; 12];
        let after = vec![0.5; 12];
        assert_eq!(proof(&before, &after, 1)["verdict"], "better");
        assert_eq!(proof(&after, &before, 1)["verdict"], "worse");
        let mixed: Vec<f64> = (0..12).map(|i| if i % 2 == 0 { 0.3 } else { 0.1 }).collect();
        assert_eq!(proof(&before, &mixed, 1)["verdict"], "no clear difference");
        assert_eq!(proof(&before[..2], &after[..2], 1)["verdict"], "too few");
    }

    #[test]
    fn workload_takes_typed_queries_and_holds_out_some() {
        let mut set: PersonaSet = serde_json::from_value(json!({"id": "ps", "source": "hn", "title": "", "created": 0, "model": "", "seed": 0, "rows": 0, "cost": 0.0, "personas": []})).unwrap();
        let needs: Vec<Value> = (0..12).map(|i| json!({"id": format!("n{i}"), "kind": "topical", "text": format!("need number {i}")})).collect();
        set.personas = vec![serde_json::from_value(json!({"id": "p", "name": "P", "about": "", "needs": needs})).unwrap()];
        let sessions: Vec<Session> = (0..12)
            .flat_map(|i| {
                (0..2).map(move |a| Session { need: format!("n{i}"), arm: a, steps: vec![json!({"kind": "search", "text": format!("q{i}")}), json!({"kind": "search", "text": format!("Q{i}")}), json!({"kind": "search", "text": format!("q{i} more")})], ..Session::default() })
            })
            .collect();
        let w = workload(&set, &sessions, 7);
        assert_eq!(w.len(), 12);
        assert!(w.iter().all(|w| w.queries.len() == 2), "the same query typed twice counts once");
        let held = w.iter().filter(|w| w.held_out).count();
        assert!(held > 0 && held < 12, "{held} held out");
        assert_eq!(w.iter().map(|x| x.held_out).collect::<Vec<_>>(), workload(&set, &sessions, 7).iter().map(|x| x.held_out).collect::<Vec<_>>());
    }

    #[test]
    fn knobs_of_takes_defaults_and_leaves_out_the_expression() {
        let d = json!({"knobs": [{"key": "votes", "kind": "number", "default": 0.3, "min": 0, "max": 2, "step": 0.05}, {"key": "rank", "kind": "sql", "default": "bm25(...)"}, {"key": "fresh", "kind": "int", "default": 0, "min": 0, "max": 5}]});
        let m = knobs_of(&d, &json!({"fresh": 2}));
        assert_eq!(m.get("votes"), Some(&json!(0.3)));
        assert_eq!(m.get("fresh"), Some(&json!(2)));
        assert!(!m.contains_key("rank"));
        assert_eq!(numeric(&d).len(), 2);
    }
}
