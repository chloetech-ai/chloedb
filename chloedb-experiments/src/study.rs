//! Studies on real sites: AI agents, each playing a persona written from
//! the site's own rows ([`crate::personas`]), look for what the persona
//! came for on every arm, where an arm is a real site's search with knobs
//! (query understanding, retrieval, ranking, blending, the index) or
//! another engine over the same rows ([`crate::source`]). Every search is
//! the real site's, on its real cluster, and nothing is written to it.
//!
//! The design is paired: each need is tried on every arm, by the same
//! persona in a fresh session, so the arms are compared need by need (a
//! difference of each need's outcome, with its interval, and McNemar's
//! exact test for finding it), which takes far fewer sessions than people
//! split between arms. The arms' order is turned for each need.
//!
//! What a session counts: whether the agent said it found the need, and
//! whether it did (a known need: it named its row; a topical one: the
//! judge grades the row it named as answering it), the nDCG of its first
//! page against every grade of that need (the rows of all arms, judged
//! alike), where a known need's row was on the first page (its reciprocal
//! rank), its searches, steps, time and cost.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::judge::Judge;
use crate::live::{self, Action, Brain, Mind, Row, AGENT_STEPS};
use crate::personas::{Need, Persona, PersonaSet};
use crate::server::Result;
use crate::source::{clip, Hit, Source};
use crate::stats;

pub const MAX_ARMS: usize = 4;
pub const MAX_WORKERS: usize = 4;
pub const MAX_SESSIONS: usize = 240;
pub const MAX_BUDGET: f64 = 20.0;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StudyArm {
    pub name: String,
    /// The source the arm searches.
    pub source: String,
    /// The knobs it sets (none: the site as it is).
    #[serde(default)]
    pub knobs: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StudySpec {
    pub name: String,
    /// The persona set whose needs it tries.
    pub personas: String,
    /// The first is the control.
    pub arms: Vec<StudyArm>,
    /// Rows a page shows.
    #[serde(default = "per")]
    pub per: usize,
    /// At most this many needs (0: all of the set's).
    #[serde(default)]
    pub needs: usize,
    /// `claude` or `scripted`.
    #[serde(default = "brain")]
    pub brain: String,
    #[serde(default = "agent_model")]
    pub agent_model: String,
    #[serde(default = "judge_model")]
    pub judge_model: String,
    /// The most the agents and the judge may spend, in dollars.
    #[serde(default = "budget")]
    pub budget: f64,
    /// Sessions at once.
    #[serde(default = "workers")]
    pub workers: usize,
}

fn per() -> usize {
    10
}
fn brain() -> String {
    "claude".into()
}
fn agent_model() -> String {
    "claude-sonnet-5-5".into()
}
fn judge_model() -> String {
    "claude-sonnet-5-5".into()
}
fn budget() -> f64 {
    5.0
}
fn workers() -> usize {
    3
}

impl StudySpec {
    pub fn check(&self, set: &PersonaSet) -> std::result::Result<(), String> {
        if self.arms.len() < 2 || self.arms.len() > MAX_ARMS {
            return Err(format!("a study has 2 to {MAX_ARMS} arms"));
        }
        for a in &self.arms {
            if a.name.trim().is_empty() {
                return Err("every arm has a name".into());
            }
        }
        if !(3..=20).contains(&self.per) {
            return Err("a page shows 3 to 20 rows".into());
        }
        if !(1..=MAX_WORKERS).contains(&self.workers) {
            return Err(format!("1 to {MAX_WORKERS} sessions at once"));
        }
        if !(self.budget > 0.0 && self.budget <= MAX_BUDGET) {
            return Err(format!("the budget is more than $0 and at most ${MAX_BUDGET}"));
        }
        if self.brain != "claude" && self.brain != "scripted" {
            return Err("agents are claude or scripted".into());
        }
        if self.brain == "claude" {
            for m in [&self.agent_model, &self.judge_model] {
                if !live::AGENT_MODELS.iter().any(|x| x.0 == m.as_str()) {
                    return Err(format!("models are one of {}", live::AGENT_MODELS.iter().map(|x| x.0).collect::<Vec<_>>().join(", ")));
                }
            }
        }
        let n = self.need_count(set);
        if n == 0 {
            return Err("the personas have no needs".into());
        }
        if n * self.arms.len() > MAX_SESSIONS {
            return Err(format!("{n} needs on {} arms are {} sessions: at most {MAX_SESSIONS}", self.arms.len(), n * self.arms.len()));
        }
        Ok(())
    }

    pub fn need_count(&self, set: &PersonaSet) -> usize {
        let all = set.needs();
        if self.needs == 0 {
            all
        } else {
            self.needs.min(all)
        }
    }
}

/// One persona's try at one need on one arm.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Session {
    pub need: String,
    pub persona: String,
    pub arm: usize,
    pub steps: Vec<Value>,
    /// The first page's rows.
    pub first: Vec<String>,
    /// Every row it was shown, and those it opened.
    pub shown: Vec<String>,
    pub opened: Vec<String>,
    /// It said it found the need, in this row.
    pub said: bool,
    pub result: Option<String>,
    /// The row it named answers the need (graded 2), or is the known row.
    pub success: bool,
    /// The grade of the row it named.
    pub grade: Option<u8>,
    /// A known need: its row on the first page (its place) and on any page.
    pub target_rank: Option<usize>,
    pub target_seen: bool,
    pub searches: u32,
    pub actions: u32,
    pub seconds: f64,
    /// The site's time to answer its searches, in milliseconds, summed.
    pub search_ms: f64,
    pub cost: f64,
    pub note: String,
    /// Cut short by the budget or a stop: not counted.
    pub cut: bool,
    pub error: Option<String>,
}

/// What a worker is doing now, for the page.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Worker {
    pub n: usize,
    pub status: String,
    pub persona: String,
    pub need: String,
    pub arm: usize,
    pub typed: String,
    pub rows: Vec<Row>,
    pub steps: Vec<Value>,
}

struct State {
    status: String,
    sessions: Vec<Session>,
    workers: Vec<Worker>,
    spent: f64,
    judge_spent: f64,
    error: Option<String>,
    ended: Option<i64>,
}

pub struct Study {
    pub id: String,
    pub spec: StudySpec,
    pub set: PersonaSet,
    pub created: i64,
    sources: Vec<Arc<Source>>,
    about: String,
    brain: Brain,
    judge: Arc<Judge>,
    http: reqwest::Client,
    st: Mutex<State>,
    stop: AtomicBool,
    queue: Mutex<VecDeque<(usize, usize)>>,
}

/// The needs of a set a study tries, in order, with their persona.
fn needs_of(set: &PersonaSet, n: usize) -> Vec<(&Persona, &Need)> {
    // One need of each persona in turn, so a cut list keeps every persona.
    let most = set.personas.iter().map(|p| p.needs.len()).max().unwrap_or(0);
    let mut out = Vec::new();
    for i in 0..most {
        for p in &set.personas {
            if let Some(need) = p.needs.get(i) {
                out.push((p, need));
            }
        }
    }
    out.truncate(n);
    out
}

impl Study {
    #[allow(clippy::too_many_arguments)]
    pub fn new(id: &str, spec: StudySpec, set: PersonaSet, sources: Vec<Arc<Source>>, about: String, brain: Brain, judge: Arc<Judge>, created: i64) -> Study {
        let n = spec.need_count(&set);
        let k = spec.arms.len();
        let mut queue = VecDeque::new();
        for need in 0..n {
            for j in 0..k {
                queue.push_back((need, (need + j) % k));
            }
        }
        let workers = (0..spec.workers).map(|n| Worker { n, status: "starting".into(), ..Worker::default() }).collect();
        Study {
            id: id.into(),
            spec,
            set,
            created,
            sources,
            about,
            brain,
            judge,
            http: reqwest::Client::new(),
            st: Mutex::new(State { status: "running".into(), sessions: Vec::new(), workers, spent: 0.0, judge_spent: 0.0, error: None, ended: None }),
            stop: AtomicBool::new(false),
            queue: Mutex::new(queue),
        }
    }

    fn st(&self) -> std::sync::MutexGuard<'_, State> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    pub fn running(&self) -> bool {
        self.st().ended.is_none()
    }

    fn spent(&self) -> bool {
        self.st().spent >= self.spec.budget
    }

    fn worker(&self, n: usize, f: impl FnOnce(&mut Worker)) {
        if let Some(w) = self.st().workers.get_mut(n) {
            f(w);
        }
    }

    /// Every session, `workers` at a time, until the needs are tried, the
    /// budget is spent or it is stopped; `save` is called after each.
    pub async fn run(self: Arc<Self>, save: impl Fn(Arc<Study>) + Send + Sync + 'static) {
        let save = Arc::new(save);
        let mut handles = Vec::new();
        for n in 0..self.spec.workers {
            let me = self.clone();
            let save = save.clone();
            handles.push(tokio::spawn(async move {
                loop {
                    if me.stop.load(Ordering::SeqCst) || me.spent() {
                        break;
                    }
                    let Some((need, arm)) = me.queue.lock().unwrap_or_else(|e| e.into_inner()).pop_front() else { break };
                    let s = me.session(n, need, arm).await;
                    me.st().sessions.push(s);
                    save(me.clone());
                }
                me.worker(n, |w| {
                    w.status = "done".into();
                    w.rows.clear();
                });
            }));
        }
        for h in handles {
            let _ = h.await;
        }
        {
            let mut st = self.st();
            st.status = if self.stop.load(Ordering::SeqCst) {
                "stopped".into()
            } else if st.spent >= self.spec.budget {
                "budget spent".into()
            } else {
                "done".into()
            };
            st.ended = Some(crate::app::now_ms());
        }
        save(self.clone());
    }

    async fn session(&self, n: usize, ni: usize, arm: usize) -> Session {
        let list = needs_of(&self.set, self.spec.need_count(&self.set));
        let (persona, need) = list[ni];
        let t0 = Instant::now();
        let mut s = Session { need: need.id.clone(), persona: persona.id.clone(), arm, ..Session::default() };
        self.worker(n, |w| {
            w.status = "thinking".into();
            w.persona = persona.name.clone();
            w.need = need.text.clone();
            w.arm = arm;
            w.typed.clear();
            w.rows.clear();
            w.steps.clear();
        });
        let r = self.try_need(n, persona, need, arm, &mut s).await;
        if let Err(e) = r {
            s.error = Some(e.clone());
            s.note = e;
            s.cut = true;
        }
        s.seconds = (t0.elapsed().as_secs_f64() * 10.0).round() / 10.0;
        self.st().spent += s.cost;
        s
    }

    async fn try_need(&self, n: usize, persona: &Persona, need: &Need, arm: usize, s: &mut Session) -> Result<()> {
        let src = self.sources[arm].clone();
        let knobs = self.spec.arms[arm].knobs.clone();
        let who = format!("{}. {}", persona.name, persona.about);
        let mut mind = Mind::new(&self.brain, &self.http, who, &self.about, &need.text);
        let mut rows: Vec<Row> = Vec::new();
        let mut hits: Vec<Hit> = Vec::new();
        let mut all: Vec<Hit> = Vec::new();
        let mut texts: HashMap<String, String> = HashMap::new();
        let mut obs: Option<String> = None;
        let mut ended = false;
        for _ in 0..AGENT_STEPS {
            if self.stop.load(Ordering::SeqCst) || self.spent() {
                s.cut = true;
                s.note = if self.spent() { "the budget is spent".into() } else { "the study was stopped".into() };
                return Ok(());
            }
            self.worker(n, |w| w.status = "thinking".into());
            let (action, cost) = mind.act(obs.take(), &rows).await?;
            s.cost += cost;
            s.actions += 1;
            self.st().spent += cost;
            match action {
                Action::Search { query, why } => {
                    self.worker(n, |w| {
                        w.status = "searching".into();
                        w.typed = query.clone();
                    });
                    let page = src.search(&query, &knobs, self.spec.per).await?;
                    s.searches += 1;
                    s.search_ms += page.ms;
                    hits = page.rows;
                    for h in &hits {
                        if !s.shown.contains(&h.id) {
                            s.shown.push(h.id.clone());
                            all.push(h.clone());
                        }
                    }
                    if s.first.is_empty() && s.searches == 1 {
                        s.first = hits.iter().map(|h| h.id.clone()).collect();
                    }
                    if let Some(t) = &need.target {
                        if let Some(i) = hits.iter().position(|h| &h.id == t) {
                            s.target_seen = true;
                            if s.searches == 1 {
                                s.target_rank = Some(i + 1);
                            }
                        }
                    }
                    rows = hits.iter().map(|h| Row { key: h.id.clone(), title: h.title.clone(), snippet: clip(&h.snippet, 240), grade: None, opened: false }).collect();
                    obs = Some(live::render_page(&query, &rows));
                    let shown = rows.clone();
                    let step = json!({"kind": "search", "text": query, "why": why, "rows": hits.len(), "ms": page.ms, "understood": page.understood});
                    s.steps.push(step.clone());
                    self.worker(n, |w| {
                        w.rows = shown;
                        w.steps.push(step);
                    });
                }
                Action::Open { result, why } => {
                    let Some(h) = result.checked_sub(1).and_then(|i| hits.get(i)).cloned() else {
                        obs = Some(format!("There is no result {result} on the page you are looking at{}.", if hits.is_empty() { " (search first)" } else { "" }));
                        continue;
                    };
                    self.worker(n, |w| w.status = "reading".into());
                    rows[result - 1].opened = true;
                    if !s.opened.contains(&h.id) {
                        s.opened.push(h.id.clone());
                    }
                    let item = src.item(&h.id).await;
                    let text = match item {
                        Ok(i) => i.text,
                        Err(_) => format!("{}\n{}", h.title, h.snippet),
                    };
                    texts.insert(h.id.clone(), text.clone());
                    obs = Some(format!("Result {result}: {}\n\n{}", h.title, clip(&text, 2400)));
                    let shown = rows.clone();
                    let step = json!({"kind": "open", "rank": result, "id": h.id, "text": h.title, "why": why});
                    s.steps.push(step.clone());
                    self.worker(n, |w| {
                        w.rows = shown;
                        w.steps.push(step);
                    });
                }
                Action::Done { found, result, note } => {
                    let h = result.checked_sub(1).and_then(|i| hits.get(i)).filter(|h| s.opened.contains(&h.id)).cloned();
                    s.said = found && h.is_some();
                    s.result = if s.said { h.as_ref().map(|h| h.id.clone()) } else { None };
                    s.note = note.clone();
                    s.steps.push(json!({"kind": "done", "found": s.said, "rank": result, "id": s.result, "text": note}));
                    ended = true;
                    break;
                }
                Action::Say(text) => {
                    s.steps.push(json!({"kind": "say", "text": text}));
                    obs = Some("Please act with one of the tools: search, open or done.".into());
                }
                Action::Quit(reason) => {
                    s.note = reason;
                    break;
                }
            }
        }
        if !ended && s.note.is_empty() {
            s.note = "ran out of actions".into();
        }
        // The judge grades every row it was shown, once for the need.
        self.worker(n, |w| w.status = "judged".into());
        let (grades, cost) = self.judge.grade(need, &all, &texts).await?;
        s.cost += cost;
        {
            let mut st = self.st();
            st.spent += cost;
            st.judge_spent += cost;
        }
        s.grade = s.result.as_ref().map(|r| grades.get(r).copied().unwrap_or(0));
        s.success = match (&need.target, need.kind.as_str()) {
            (Some(t), "known") => s.result.as_ref() == Some(t),
            _ => s.grade == Some(2),
        };
        Ok(())
    }

    /// The study as the page shows it: its state, the workers, and the
    /// report over the sessions so far.
    pub fn view(&self) -> Value {
        let st = self.st();
        let total = self.spec.need_count(&self.set) * self.spec.arms.len();
        json!({
            "id": self.id, "name": self.spec.name, "created": self.created, "status": st.status, "ended": st.ended,
            "spec": self.spec, "personas": self.set.summary(), "done": st.sessions.len(), "total": total,
            "spent": st.spent, "judge_spent": st.judge_spent, "error": st.error,
            "workers": if st.ended.is_none() { json!(st.workers) } else { json!([]) },
            "report": report(&self.spec, &self.set, &st.sessions, |n| self.judge.grades_of(n)),
        })
    }

    /// What is kept of it: the view without the workers, and the sessions.
    pub fn kept(&self) -> (Value, Vec<Session>) {
        let mut v = self.view();
        v["workers"] = json!([]);
        (v, self.st().sessions.clone())
    }
}

// ------------------------------------------------------------ report

/// t's 97.5% quantile with `df` degrees of freedom (Cornish–Fisher).
fn t975(df: f64) -> f64 {
    let z = 1.959_964f64;
    if df < 1.0 {
        return f64::NAN;
    }
    z + (z.powi(3) + z) / (4.0 * df) + (5.0 * z.powi(5) + 16.0 * z.powi(3) + 3.0 * z) / (96.0 * df * df)
}

/// A mean with its 95% interval (t).
fn mean_ci(xs: &[f64]) -> Value {
    if xs.is_empty() {
        return json!({"n": 0});
    }
    let (m, sd) = stats::mean_sd(xs);
    let n = xs.len() as f64;
    let h = if xs.len() > 1 && sd.is_finite() { t975(n - 1.0) * sd / n.sqrt() } else { f64::NAN };
    json!({"n": xs.len(), "mean": m, "lo": if h.is_finite() { json!(m - h) } else { Value::Null }, "hi": if h.is_finite() { json!(m + h) } else { Value::Null }})
}

/// A share with its 95% Wilson interval.
fn share(k: usize, n: usize) -> Value {
    if n == 0 {
        return json!({"n": 0});
    }
    let (p, nf, z) = (k as f64 / n as f64, n as f64, 1.959_964f64);
    let d = 1.0 + z * z / nf;
    let c = (p + z * z / (2.0 * nf)) / d;
    let h = z * (p * (1.0 - p) / nf + z * z / (4.0 * nf * nf)).sqrt() / d;
    json!({"n": n, "k": k, "mean": p, "lo": (c - h).max(0.0), "hi": (c + h).min(1.0)})
}

/// McNemar's exact two-sided p value: `b` pairs where only the control
/// found it, `c` where only the arm did.
pub fn mcnemar(b: usize, c: usize) -> f64 {
    let n = (b + c) as u64;
    if n == 0 {
        return 1.0;
    }
    (2.0 * stats::binom_cdf(b.min(c) as u64, n, 0.5)).min(1.0)
}

/// The report over `sessions`: every arm's outcomes, and each arm against
/// the control need by need.
pub fn report(spec: &StudySpec, set: &PersonaSet, sessions: &[Session], grades: impl Fn(&Need) -> HashMap<String, u8>) -> Value {
    let needs: HashMap<&str, (&Persona, &Need)> = set.personas.iter().flat_map(|p| p.needs.iter().map(move |n| (n.id.as_str(), (p, n)))).collect();
    let counted: Vec<&Session> = sessions.iter().filter(|s| !s.cut).collect();
    let pool: HashMap<&str, HashMap<String, u8>> = needs.iter().map(|(id, (_, n))| (*id, grades(n))).collect();
    let ndcg = |s: &Session| pool.get(s.need.as_str()).map(|g| live::ndcg(g, 2, &s.first, spec.per)).unwrap_or(0.0);
    let k = spec.arms.len();
    let mut arms = Vec::new();
    for a in 0..k {
        let ss: Vec<&&Session> = counted.iter().filter(|s| s.arm == a).collect();
        let n = ss.len();
        let of = |f: &dyn Fn(&Session) -> bool| ss.iter().filter(|s| f(s)).count();
        let known: Vec<&&&Session> = ss.iter().filter(|s| needs.get(s.need.as_str()).is_some_and(|(_, n)| n.kind == "known")).collect();
        let topical: Vec<&&&Session> = ss.iter().filter(|s| needs.get(s.need.as_str()).is_some_and(|(_, n)| n.kind != "known")).collect();
        let said = of(&|s| s.said);
        let rr: Vec<f64> = known.iter().map(|s| s.target_rank.map_or(0.0, |r| 1.0 / r as f64)).collect();
        arms.push(json!({
            "name": spec.arms[a].name, "source": spec.arms[a].source, "knobs": spec.arms[a].knobs,
            "sessions": n,
            "success": share(of(&|s| s.success), n),
            "said": share(said, n),
            // Of those it said it found, the share it did not.
            "overclaim": share(of(&|s| s.said && !s.success), said),
            "known_success": share(known.iter().filter(|s| s.success).count(), known.len()),
            "topical_success": share(topical.iter().filter(|s| s.success).count(), topical.len()),
            "known_on_first_page": share(known.iter().filter(|s| s.target_rank.is_some()).count(), known.len()),
            "known_seen": share(known.iter().filter(|s| s.target_seen).count(), known.len()),
            "mrr": mean_ci(&rr),
            "ndcg": mean_ci(&ss.iter().map(|s| ndcg(s)).collect::<Vec<_>>()),
            "searches": mean_ci(&ss.iter().map(|s| s.searches as f64).collect::<Vec<_>>()),
            "actions": mean_ci(&ss.iter().map(|s| s.actions as f64).collect::<Vec<_>>()),
            "seconds": mean_ci(&ss.iter().map(|s| s.seconds).collect::<Vec<_>>()),
            "search_ms": mean_ci(&ss.iter().filter(|s| s.searches > 0).map(|s| s.search_ms / s.searches as f64).collect::<Vec<_>>()),
            "cost": ss.iter().map(|s| s.cost).sum::<f64>(),
        }));
    }
    // Each arm against the control, over the needs both tried.
    let by: HashMap<(&str, usize), &Session> = counted.iter().map(|s| ((s.need.as_str(), s.arm), *s)).collect();
    let mut comparisons = Vec::new();
    for a in 1..k {
        let pairs: Vec<(&Session, &Session)> = needs_of(set, spec.need_count(set)).iter().filter_map(|(_, n)| Some((*by.get(&(n.id.as_str(), 0))?, *by.get(&(n.id.as_str(), a))?))).collect();
        let (b, c) = (pairs.iter().filter(|(x, y)| x.success && !y.success).count(), pairs.iter().filter(|(x, y)| !x.success && y.success).count());
        let d = |f: &dyn Fn(&Session) -> f64| mean_ci(&pairs.iter().map(|(x, y)| f(y) - f(x)).collect::<Vec<_>>());
        let succ = d(&|s| f64::from(u8::from(s.success)));
        let p = mcnemar(b, c);
        let lift = succ["mean"].as_f64().unwrap_or(0.0);
        let (name, control) = (spec.arms[a].name.clone(), spec.arms[0].name.clone());
        let call = if pairs.len() < 2 {
            "too few needs yet".to_string()
        } else if p < 0.05 && lift > 0.0 {
            "finds more".into()
        } else if p < 0.05 && lift < 0.0 {
            "finds less".into()
        } else {
            "no clear difference".into()
        };
        let pct = |x: Option<f64>| x.map_or("–".to_string(), |x| format!("{:+.0}", x * 100.0));
        let line = if pairs.len() < 2 {
            format!("{name} against {control}: too few needs tried on both yet.")
        } else {
            format!(
                "{name} found what people came for in {} of {} needs, {control} in {} ({} points, 95% interval {} to {}); {} {} only {name} found, {} only {control} (McNemar p {:.3}).",
                pairs.iter().filter(|(_, y)| y.success).count(),
                pairs.len(),
                pairs.iter().filter(|(x, _)| x.success).count(),
                pct(succ["mean"].as_f64()),
                pct(succ["lo"].as_f64()),
                pct(succ["hi"].as_f64()),
                c,
                if c == 1 { "need" } else { "needs" },
                b,
                p
            )
        };
        comparisons.push(json!({
            "arm": a, "name": name, "pairs": pairs.len(), "only_control": b, "only_arm": c, "mcnemar_p": p, "call": call, "line": line,
            "success": succ,
            "ndcg": d(&|s| ndcg(s)),
            "searches": d(&|s| s.searches as f64),
            "seconds": d(&|s| s.seconds),
        }));
    }
    // Each need, as each arm did.
    let rows: Vec<Value> = needs_of(set, spec.need_count(set))
        .iter()
        .map(|(p, n)| {
            let tries: Vec<Value> = (0..k)
                .map(|a| match sessions.iter().find(|s| s.need == n.id && s.arm == a) {
                    Some(s) => json!({"success": s.success, "said": s.said, "cut": s.cut, "searches": s.searches, "rank": s.target_rank, "note": s.note, "error": s.error}),
                    None => Value::Null,
                })
                .collect();
            json!({"id": n.id, "kind": n.kind, "text": n.text, "target": n.target, "target_title": n.target_title, "persona": p.name, "tries": tries})
        })
        .collect();
    let mut personas = Vec::new();
    for p in &set.personas {
        let per_arm: Vec<Value> = (0..k).map(|a| {
            let ss: Vec<&&Session> = counted.iter().filter(|s| s.arm == a && s.persona == p.id).collect();
            share(ss.iter().filter(|s| s.success).count(), ss.len())
        }).collect();
        personas.push(json!({"id": p.id, "name": p.name, "arms": per_arm}));
    }
    json!({"arms": arms, "comparisons": comparisons, "needs": rows, "personas": personas, "cut": sessions.len() - counted.len()})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set() -> PersonaSet {
        let need = |id: &str, kind: &str, target: Option<&str>| Need { id: id.into(), kind: kind.into(), text: format!("need {id}"), target: target.map(str::to_string), target_title: None };
        PersonaSet {
            id: "s".into(),
            source: "hn".into(),
            title: "HN".into(),
            created: 0,
            model: "scripted".into(),
            seed: 1,
            rows: 2,
            cost: 0.0,
            personas: vec![
                Persona { id: "p1".into(), name: "Ana".into(), about: "".into(), needs: vec![need("p1-n1", "known", Some("7")), need("p1-n2", "topical", None)] },
                Persona { id: "p2".into(), name: "Ben".into(), about: "".into(), needs: vec![need("p2-n1", "known", Some("8"))] },
            ],
        }
    }

    #[test]
    fn needs_take_turns_between_personas() {
        let s = set();
        let ids: Vec<&str> = needs_of(&s, 3).iter().map(|(_, n)| n.id.as_str()).collect();
        assert_eq!(ids, ["p1-n1", "p2-n1", "p1-n2"]);
        assert_eq!(needs_of(&s, 2).len(), 2);
    }

    #[test]
    fn mcnemar_is_exact() {
        assert_eq!(mcnemar(0, 0), 1.0);
        // 0 against 6 discordant pairs: 2 / 64.
        assert!((mcnemar(0, 6) - 2.0 / 64.0).abs() < 1e-12);
        assert!((mcnemar(3, 3) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn the_report_pairs_needs_and_pools_their_grades() {
        let s = set();
        let spec = StudySpec { name: "x".into(), personas: "s".into(), arms: vec![StudyArm { name: "A".into(), source: "hn".into(), knobs: json!({}) }, StudyArm { name: "B".into(), source: "hn".into(), knobs: json!({"meaning": false}) }], per: 10, needs: 0, brain: "scripted".into(), agent_model: agent_model(), judge_model: judge_model(), budget: 1.0, workers: 2 };
        assert!(spec.check(&s).is_ok());
        let sess = |need: &str, arm: usize, success: bool, first: &[&str]| Session { need: need.into(), persona: need[..2].into(), arm, success, said: success, first: first.iter().map(|x| x.to_string()).collect(), searches: 1, ..Session::default() };
        let all = vec![
            sess("p1-n1", 0, false, &["1", "7"]), sess("p1-n1", 1, true, &["7"]),
            sess("p2-n1", 0, false, &["2"]), sess("p2-n1", 1, true, &["8", "2"]),
            sess("p1-n2", 0, true, &["5"]), Session { cut: true, ..sess("p1-n2", 1, false, &[]) },
        ];
        let grades = |n: &Need| -> HashMap<String, u8> { match n.target.clone() { Some(t) => [(t, 2u8)].into_iter().collect(), None => [("5".to_string(), 2u8)].into_iter().collect() } };
        let r = report(&spec, &s, &all, grades);
        assert_eq!(r["cut"], 1);
        assert_eq!(r["arms"][1]["success"]["k"], 2);
        assert_eq!(r["arms"][0]["known_on_first_page"]["k"], 0);
        let c = &r["comparisons"][0];
        assert_eq!((c["pairs"].as_u64(), c["only_arm"].as_u64(), c["only_control"].as_u64()), (Some(2), Some(2), Some(0)));
        // The first page of A for p1-n1 has the known row second.
        let nd = r["arms"][0]["ndcg"]["mean"].as_f64().unwrap();
        assert!((nd - (1.0 / 3f64.log2() + 0.0 + 1.0) / 3.0).abs() < 1e-9, "{nd}");
        assert!(c["line"].as_str().unwrap().starts_with("B found what people came for in 2 of 2 needs, A in 0"), "{c}");
        let bad = StudySpec { arms: vec![spec.arms[0].clone()], ..spec.clone() };
        assert!(bad.check(&s).is_err());
    }
}
