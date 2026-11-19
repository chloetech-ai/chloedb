//! Live runs: an A/B test you watch as it happens.
//!
//! Simulated users arrive at a rate you set (and change while it runs).
//! Each search is a real logged query to chloedb-server, ranked by the
//! user's arm, and the clicks the click model draws go back as events, so
//! the server's rank log fills as a site's would. Every second every
//! metric of every arm is computed again over the users so far: its value,
//! its difference from the control with the interval, the p value a peek
//! gives and the always-valid one (mSPRT), which may be read at any time,
//! and the truth the simulator knows.
//!
//! Beside them a few AI agents use the same search page: each is a person
//! of the population (age, gender, country, device, tenure) in the arm the
//! split gives them, with a need from the site's queries. They type their
//! own searches, read the results, open what looks right and say when
//! they found what they needed or would give up. Claude plays them
//! ([`Brain::Claude`], over the Messages API, with a spend cap), or a
//! scripted stand-in with no model ([`Brain::Scripted`]). Their searches
//! are scored apart from the simulated users': they are a handful, there
//! to be read, and the judges' grades say whether what they opened was
//! relevant. Anyone watching can search the page too ([`Live::try_search`]).
//!
//! When the run ends its users and their sums are an experiment like any
//! other ([`Live::run_data`]), with the full report: cohorts, every
//! metric's distribution, power and the truth.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::ab::{self, Arm, RunData, Spec, Sums, METRICS, N, SUMS, Z80, Z975};
use crate::click::Model;
use crate::policy::{Policy, Rng};
use crate::server::{lit, Result, Server};
use crate::sim::{key_text, World};
use crate::site::Site;
use crate::stats::{self, norm_cdf};
use crate::users::{Alias, Population, Shape, User, AGES, COUNTRIES, DEVICES, GENDERS, TENURES};

pub const MAX_RATE: f64 = 400.0;
pub const MAX_MINUTES: f64 = 60.0;
pub const MAX_USERS: usize = 100_000;
pub const MAX_AGENTS: usize = 6;
pub const MAX_BUDGET: f64 = 20.0;
/// An agent's actions in one task, at most.
pub const AGENT_STEPS: usize = 8;
/// Simulated searches in flight at once.
const IN_FLIGHT: usize = 48;
/// Points of the series kept; past it, every other one goes.
const POINTS: usize = 1200;
/// Feed entries kept.
const FEED: usize = 400;
/// Searches before a run may be called decided ([`Live::decided`]).
pub const MIN_DECIDE: u64 = 1000;
/// What the agents keep of their budget for the one call that sums up
/// why they gave up, at the end ([`Live::sum_up`]).
const SUM_UP_RESERVE: f64 = 0.05;
/// The Messages API's version this app speaks.
const API_VERSION: &str = "2026-10-21";

/// The models an agent can be: id, name, dollars per million input and
/// output tokens, and whether the API's server-side fallback serves it.
pub const AGENT_MODELS: [(&str, &str, f64, f64, bool); 3] = [
    ("claude-opus-5-5", "Claude Opus 5.5", 4.0, 20.0, true),
    ("claude-sonnet-5-5", "Claude Sonnet 5.5", 2.0, 10.0, true),
    ("claude-haiku-5-5", "Claude Haiku 5.5", 0.10, 0.50, false),
];

/// What a live run runs.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LiveSpec {
    pub name: String,
    pub site: String,
    /// The first is the control.
    pub arms: Vec<Arm>,
    #[serde(default = "dbn")]
    pub model: String,
    #[serde(default = "users")]
    pub users: usize,
    /// Simulated searches a second.
    #[serde(default = "rate")]
    pub rate: f64,
    /// How long it runs, at most.
    #[serde(default = "minutes")]
    pub minutes: f64,
    #[serde(default = "limit")]
    pub limit: usize,
    #[serde(default = "seed")]
    pub seed: u64,
    #[serde(default = "taste")]
    pub taste: f64,
    #[serde(default = "appetite")]
    pub appetite: f64,
    #[serde(default = "primary")]
    pub primary: String,
    /// AI agents on the search page (0 to [`MAX_AGENTS`]).
    #[serde(default)]
    pub agents: usize,
    /// `claude` or `scripted`.
    #[serde(default = "brain")]
    pub brain: String,
    #[serde(default = "agent_model")]
    pub agent_model: String,
    /// The most the agents may spend, in dollars.
    #[serde(default = "budget")]
    pub budget: f64,
    /// When it ends: `decided`, once every arm's primary metric is called
    /// by its always-valid p value and every guardrail is read
    /// ([`Live::decided`]), or at the latest when its minutes are up;
    /// `time`, when they are up.
    #[serde(default = "until")]
    pub until: String,
}

fn dbn() -> String {
    "dbn".into()
}
fn users() -> usize {
    20_000
}
fn rate() -> f64 {
    50.0
}
fn minutes() -> f64 {
    10.0
}
fn limit() -> usize {
    5
}
fn seed() -> u64 {
    ab::fresh_seed()
}
fn until() -> String {
    "decided".into()
}
fn taste() -> f64 {
    0.35
}
fn appetite() -> f64 {
    0.6
}
fn primary() -> String {
    "success".into()
}
fn brain() -> String {
    "claude".into()
}
fn agent_model() -> String {
    AGENT_MODELS[0].0.into()
}
fn budget() -> f64 {
    2.0
}

impl LiveSpec {
    /// The A/B spec of the same arms and users: what assigns users to arms
    /// and what the report is written from.
    pub fn ab(&self, searches: u64) -> Spec {
        Spec {
            name: self.name.clone(),
            site: self.site.clone(),
            arms: self.arms.clone(),
            model: self.model.clone(),
            users: self.users,
            impressions: searches,
            explore_share: 0.0,
            explore: Value::Null,
            limit: self.limit,
            seed: self.seed,
            salt: 0,
            taste: self.taste,
            appetite: self.appetite,
            mode: "live".into(),
            primary: self.primary.clone(),
            pre: 0.0,
        }
    }

    pub fn check(&self) -> Result<()> {
        let mut s = self.ab(1000);
        s.mode = "served".into();
        s.check()?;
        if self.users > MAX_USERS {
            return Err(format!("users: from 100 to {MAX_USERS} in a live run"));
        }
        if !(self.rate.is_finite() && self.rate >= 0.0 && self.rate <= MAX_RATE) {
            return Err(format!("searches a second: from 0 to {MAX_RATE}"));
        }
        if !(self.minutes.is_finite() && self.minutes >= 0.5 && self.minutes <= MAX_MINUTES) {
            return Err(format!("minutes: from 0.5 to {MAX_MINUTES}"));
        }
        if self.agents > MAX_AGENTS {
            return Err(format!("AI agents: from 0 to {MAX_AGENTS}"));
        }
        if self.brain != "claude" && self.brain != "scripted" {
            return Err("an agent's brain is claude or scripted".into());
        }
        if self.brain == "claude" && !AGENT_MODELS.iter().any(|m| m.0 == self.agent_model) {
            return Err(format!("agents are one of {}", AGENT_MODELS.iter().map(|m| m.0).collect::<Vec<_>>().join(", ")));
        }
        if !(self.budget.is_finite() && self.budget >= 0.0 && self.budget <= MAX_BUDGET) {
            return Err(format!("the agents' budget: from $0 to ${MAX_BUDGET}"));
        }
        if self.until != "decided" && self.until != "time" {
            return Err("until is decided (stop once the call is made and the guardrails are read) or time".into());
        }
        if self.rate == 0.0 && self.agents == 0 {
            return Err("a live run needs simulated users (a rate above 0) or AI agents".into());
        }
        Ok(())
    }
}

/// How an agent decides.
#[derive(Clone)]
pub enum Brain {
    /// No model: it searches the need's own words, opens the result whose
    /// title shares the most words with it and says it found it when two
    /// words or more do.
    Scripted,
    /// Claude over the Messages API at `base` (`https://api.anthropic.com`).
    Claude { key: String, model: String, base: String },
}

impl Brain {
    pub fn name(&self) -> &str {
        match self {
            Brain::Scripted => "scripted",
            Brain::Claude { model, .. } => model,
        }
    }
}

/// One row of a results page.
#[derive(Clone, Debug, Serialize)]
pub struct Row {
    pub key: String,
    pub title: String,
    pub snippet: String,
    /// Its grade for the agent's need (visitors have none).
    pub grade: Option<u8>,
    pub opened: bool,
}

/// What an agent did in its tasks, per arm.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Tally {
    pub tasks: u64,
    /// Tasks it said it finished with what it needed.
    pub found: u64,
    /// Of those, the ones whose result the judges grade relevant.
    pub found_relevant: u64,
    pub searches: u64,
    pub clicks: u64,
    /// Clicks on rows the judges grade relevant to the need.
    pub relevant_clicks: u64,
    /// nDCG of the first page of each task, summed.
    pub first_ndcg: f64,
    pub steps: u64,
    pub cost: f64,
}

/// What the page shows of an agent.
#[derive(Clone, Debug, Serialize)]
struct AgentView {
    n: usize,
    user: u32,
    persona: Value,
    arm: usize,
    status: String,
    need: Option<String>,
    /// What it typed last.
    typed: String,
    /// The page it is looking at.
    rows: Vec<Row>,
    /// The current task's actions.
    steps: Vec<Value>,
    /// The last task that ended.
    last: Option<Value>,
    tasks: u64,
    found: u64,
    cost: f64,
    error: Option<String>,
}

/// A visitor's page, to open its rows by number.
struct Visit {
    arm: usize,
    who: String,
    rows: Vec<Row>,
    opened: Vec<usize>,
}

/// What the ranking, the truth and the users of a run are, once ranked.
struct Prepared {
    site: Site,
    worlds: Vec<Arc<World>>,
    users: Vec<User>,
    assigned: Vec<Option<usize>>,
    share: Vec<f64>,
    /// Per user, per arm: the exact sums of one search.
    truth: Vec<Vec<Sums>>,
    /// Per metric (METRICS, then the judged quality), per arm: its exact value.
    truth_arm: Vec<Vec<Option<f64>>>,
    /// Per arm, per query: nDCG of the rows the arm shows.
    ndcg: Vec<Vec<f64>>,
    queries: Alias,
    /// Per query, each arm's exact values (its counts are the state's).
    by_query: ab::ByQuery,
}

struct State {
    status: &'static str,
    stage: String,
    error: Option<String>,
    searches: u64,
    errors: u64,
    last_error: Option<String>,
    sums: Vec<Sums>,
    /// Per user: [searches, nDCG summed].
    quality: Vec<Sums>,
    /// Per arm, every search's time at the server.
    latencies: Vec<Vec<f64>>,
    /// Per metric, per arm after the control: (difference, se) each second.
    series: Vec<Vec<Vec<(f64, f64)>>>,
    /// Per second: searches, and per metric of METRICS, per arm after the control, (difference, se).
    checkpoints: Vec<(u64, Vec<Vec<(f64, f64)>>)>,
    points: Vec<Value>,
    point_id: u64,
    every: u64,
    ticks: u64,
    feed: VecDeque<Value>,
    seq: u64,
    agents: Vec<AgentView>,
    agent_tally: Vec<Tally>,
    visitor_tally: Vec<Tally>,
    visits: HashMap<String, Visit>,
    visit_order: VecDeque<String>,
    cost: f64,
    snapshot: Value,
    recent: VecDeque<(f64, u64)>,
    ended: Option<f64>,
    /// When the run was decided and stopped ([`Live::decided`]).
    decided: Option<Value>,
    /// While it is not, what it waits on ([`decision`]).
    waiting: Option<String>,
    /// Per query and arm (`q * arms + a`): searches, successes, clicks,
    /// abandoned searches.
    by_query: Vec<[f64; 4]>,
    /// The agents' tasks that ended without what they needed: arm, need, note.
    gave_up: Vec<(usize, String, String)>,
    /// Per arm, one line on why the agents gave up, once the run ends.
    why: Option<Value>,
}

pub struct Live {
    pub id: String,
    pub spec: LiveSpec,
    pub created: i64,
    pub about: String,
    server: Server,
    brain: Brain,
    http: reqwest::Client,
    log: String,
    events: String,
    rate: AtomicU64,
    stop: AtomicBool,
    started: Instant,
    next: AtomicU64,
    in_flight: AtomicUsize,
    visits: AtomicU64,
    prep: OnceLock<Prepared>,
    state: Mutex<State>,
    pending: Mutex<Vec<Value>>,
}

/// `n` with thousands separated by commas.
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

/// Seconds as m:ss.
fn clock(s: f64) -> String {
    let s = s.max(0.0).round() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

fn now_us() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_micros() as i64)
}

/// A number for the page: null when not finite, else to 6 figures.
fn num(x: f64) -> Value {
    if !x.is_finite() {
        return Value::Null;
    }
    if x == 0.0 {
        return json!(0.0);
    }
    let mag = 10f64.powi(5 - x.abs().log10().floor() as i32);
    json!((x * mag).round() / mag)
}

fn opt(x: Option<f64>) -> Value {
    x.map_or(Value::Null, num)
}

fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3))
}

/// nDCG of `keys` (the first `limit`) by `grades` of 0 to `max`, against
/// the best order of every row judged.
pub fn ndcg(grades: &HashMap<String, u8>, max: u8, keys: &[String], limit: usize) -> f64 {
    let gain = |g: u8| ((1u32 << g) - 1) as f64 / ((1u32 << max.max(1)) - 1) as f64;
    let dcg: f64 = keys.iter().take(limit).enumerate().map(|(i, k)| gain(grades.get(k).copied().unwrap_or(0)) / ((i + 2) as f64).log2()).sum();
    let mut ideal: Vec<f64> = grades.values().map(|&g| gain(g)).collect();
    ideal.sort_by(|a, b| b.total_cmp(a));
    let idcg: f64 = ideal.iter().take(limit).enumerate().map(|(i, g)| g / ((i + 2) as f64).log2()).sum();
    if idcg > 0.0 {
        dcg / idcg
    } else {
        0.0
    }
}

/// A user, as the page names them.
pub fn persona(u: &User) -> Value {
    json!({
        "user": u.id, "age": AGES[u.age as usize], "gender": GENDERS[u.gender as usize], "country": COUNTRIES[u.country as usize],
        "device": DEVICES[u.device as usize], "tenure": TENURES[u.tenure as usize],
        "short": format!("{} · {} · {} · {} · {}", AGES[u.age as usize], GENDERS[u.gender as usize], COUNTRIES[u.country as usize], DEVICES[u.device as usize], TENURES[u.tenure as usize]),
    })
}

/// A user, in a sentence for the agent that plays them.
pub fn persona_text(u: &User) -> String {
    let age = AGES[u.age as usize].replace('-', " to ");
    let who = ["woman", "man", "person"][u.gender as usize];
    let place = ["the United States", "India", "the United Kingdom", "Germany", "Brazil", "Canada", "France", "Japan", "a country not on our list"][u.country as usize];
    let device = ["at a desktop computer", "on a phone", "on a tablet"][u.device as usize];
    let tenure = ["new to this site", "someone who uses this site now and then", "someone who has used this site for years"][u.tenure as usize];
    let age = if age.ends_with('+') { format!("{} or older", age.trim_end_matches('+')) } else { age };
    format!("a {who} aged {age}, living in {place}, searching {device}, {tenure}")
}

/// The words of `s`, lower-cased, of three letters or more.
pub(crate) fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric()).filter(|w| w.chars().count() >= 3).map(|w| w.to_lowercase()).collect()
}

impl Live {
    pub fn new(server: Server, id: &str, spec: LiveSpec, about: String, brain: Brain, created: i64) -> Live {
        let k = spec.arms.len();
        let rate = spec.rate;
        Live {
            id: id.to_string(),
            created,
            about,
            log: format!("exp_log_{id}"),
            events: format!("exp_events_{id}"),
            server,
            brain,
            http: reqwest::Client::builder().timeout(Duration::from_secs(180)).connect_timeout(Duration::from_secs(10)).build().expect("an HTTP client"),
            rate: AtomicU64::new(rate.to_bits()),
            stop: AtomicBool::new(false),
            started: Instant::now(),
            next: AtomicU64::new(0),
            in_flight: AtomicUsize::new(0),
            visits: AtomicU64::new(0),
            prep: OnceLock::new(),
            state: Mutex::new(State {
                status: "starting",
                stage: "ranking every query under each arm".into(),
                error: None,
                searches: 0,
                errors: 0,
                last_error: None,
                sums: Vec::new(),
                quality: Vec::new(),
                latencies: vec![Vec::new(); k],
                series: vec![vec![Vec::new(); k.saturating_sub(1)]; METRICS.len() + 1],
                checkpoints: Vec::new(),
                points: Vec::new(),
                point_id: 0,
                every: 1,
                ticks: 0,
                feed: VecDeque::new(),
                seq: 0,
                agents: Vec::new(),
                agent_tally: vec![Tally::default(); k],
                visitor_tally: vec![Tally::default(); k],
                visits: HashMap::new(),
                visit_order: VecDeque::new(),
                cost: 0.0,
                decided: None,
                waiting: None,
                by_query: Vec::new(),
                gave_up: Vec::new(),
                why: None,
                snapshot: Value::Null,
                recent: VecDeque::new(),
                ended: None,
            }),
            pending: Mutex::new(Vec::new()),
            spec,
        }
    }

    fn st(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn p(&self) -> &Prepared {
        self.prep.get().expect("a prepared run")
    }

    pub fn rate(&self) -> f64 {
        f64::from_bits(self.rate.load(Ordering::Relaxed))
    }

    pub fn set_rate(&self, r: f64) -> Result<f64> {
        if !(r.is_finite() && (0.0..=MAX_RATE).contains(&r)) {
            return Err(format!("searches a second: from 0 to {MAX_RATE}"));
        }
        self.rate.store(r.to_bits(), Ordering::Relaxed);
        Ok(r)
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    pub fn status(&self) -> &'static str {
        self.st().status
    }

    fn feed(&self, mut v: Value) {
        let mut st = self.st();
        st.seq += 1;
        v["seq"] = json!(st.seq);
        v["t"] = num(self.started.elapsed().as_secs_f64());
        st.feed.push_back(v);
        while st.feed.len() > FEED {
            st.feed.pop_front();
        }
    }

    fn fail(&self, e: String) {
        let mut st = self.st();
        st.status = "failed";
        st.error = Some(e);
        drop(st);
        self.stop();
    }

    /// Ranks every query under each arm, and works out every user's exact
    /// values under each.
    async fn prepare(&self, mut site: Site) -> Result<()> {
        let spec = self.spec.ab(0);
        let ranks = ab::rank(&self.server, &mut site, &spec, &Policy::None).await?;
        self.st().stage = "working out every user's exact values under each arm".into();
        let model = Model::parse(&spec.model)?;
        let shape = Shape { taste: spec.taste, appetite: spec.appetite, ..Shape::default() };
        let pop = || Population::new(spec.users, spec.seed, shape);
        let worlds: Vec<Arc<World>> = ranks.arms.iter().zip(&spec.arms).map(|(r, a)| Arc::new(World::new(site.clone(), pop(), model, Policy::None, spec.limit, a.rank.clone(), r.clone()).then(&a.then))).collect();
        let users = worlds[0].users.users.clone();
        let share = worlds[0].users.shares();
        let assigned: Vec<Option<usize>> = users.iter().map(|u| ab::assign(&spec, u.id)).collect();
        let qshares = site.shares();
        let lists: Vec<Vec<Vec<String>>> = ranks.arms.iter().map(|r| r.iter().map(|x| x.keys.clone()).collect()).collect();
        let k = worlds.len();
        let (w2, l2, u2, q2) = (worlds.clone(), lists.clone(), users.clone(), qshares.clone());
        let truth: Vec<Vec<Sums>> = tokio::task::spawn_blocking(move || ab::par(u2.len(), |r| r.map(|i| (0..k).map(|a| ab::exact(&w2[a], &u2[i], &l2[a], &q2)).collect::<Vec<Sums>>()).collect::<Vec<_>>()).into_iter().flatten().collect())
            .await
            .map_err(|e| e.to_string())?;
        let ndcg_of: Vec<Vec<f64>> = lists.iter().map(|l| l.iter().enumerate().map(|(q, keys)| ndcg(&site.grades[q], site.max_grade, keys, spec.limit)).collect()).collect();
        let mut truth_arm: Vec<Vec<Option<f64>>> = METRICS
            .iter()
            .map(|m| {
                (0..k)
                    .map(|a| {
                        if !m.exact {
                            return None;
                        }
                        let (mut num, mut den) = (0.0, 0.0);
                        for u in 0..users.len() {
                            num += share[u] * truth[u][a][m.num];
                            den += share[u] * truth[u][a][m.den];
                        }
                        (den > 0.0).then(|| num / den)
                    })
                    .collect()
            })
            .collect();
        truth_arm.push((0..k).map(|a| Some(ndcg_of[a].iter().zip(&qshares).map(|(v, s)| v * s).sum())).collect());
        let queries = Alias::new(&site.queries.iter().map(|q| q.weight).collect::<Vec<_>>());
        let nu = users.len();
        let (s2, w3, u3, sh3, l3) = (site.clone(), worlds.clone(), users.clone(), share.clone(), lists.clone());
        let by_query = tokio::task::spawn_blocking(move || ab::ByQuery::new(&s2, &w3, &u3, &sh3, &l3)).await.map_err(|e| e.to_string())?;
        {
            let mut st = self.st();
            st.sums = vec![[0.0; SUMS]; nu];
            st.quality = vec![[0.0; SUMS]; nu];
            st.by_query = vec![[0.0; 4]; by_query.measured.len()];
        }
        self.server.sql(&format!("CREATE TABLE IF NOT EXISTS {} (id KEYWORD PRIMARY KEY, usr KEYWORD NOT NULL, item KEYWORD, action KEYWORD, ts INT, dwell_ms FLOAT)", self.events), json!({})).await?;
        let _ = self.prep.set(Prepared { site, worlds, users, assigned, share, truth, truth_arm, ndcg: ndcg_of, queries, by_query });
        Ok(())
    }

    /// Runs it to its end: prepared, then the users, the agents and the
    /// count each second until the time is up or it is stopped.
    pub async fn run(self: Arc<Self>, site: Site) -> Result<()> {
        if let Err(e) = self.prepare(site).await {
            self.fail(e.clone());
            return Err(e);
        }
        {
            let mut st = self.st();
            st.status = "live";
            st.stage = String::new();
        }
        self.feed(json!({"kind": "note", "text": format!("Started: {} simulated searches a second{}", self.rate(), if self.spec.agents > 0 { format!(", {} AI agents ({})", self.spec.agents, self.brain.name()) } else { String::new() })}));
        let drive = tokio::spawn(self.clone().drive());
        let mut agents = Vec::new();
        for n in 0..self.spec.agents {
            agents.push(tokio::spawn(self.clone().agent(n)));
        }
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let limit = self.spec.minutes * 60.0;
        loop {
            tick.tick().await;
            self.flush().await;
            let elapsed = self.started.elapsed().as_secs_f64();
            self.count(elapsed);
            let (errors, searches, last) = {
                let st = self.st();
                (st.errors, st.searches, st.last_error.clone())
            };
            if errors >= 50 && errors as f64 > 0.2 * (errors + searches) as f64 {
                self.fail(format!("searches fail: {}", last.unwrap_or_default()));
                break;
            }
            if elapsed >= limit || self.stopped() {
                break;
            }
            if self.spec.until == "decided" {
                match self.decided(elapsed) {
                    Ok(d) => {
                        self.feed(json!({"kind": "note", "text": d["text"]}));
                        let mut st = self.st();
                        st.decided = Some(d);
                        st.waiting = None;
                        break;
                    }
                    Err(w) => self.st().waiting = Some(w),
                }
            }
        }
        self.stop();
        {
            let mut st = self.st();
            if st.status == "live" {
                st.status = "ending";
                st.stage = "waiting for the searches in flight".into();
            }
        }
        let _ = drive.await;
        for _ in 0..100 {
            if self.in_flight.load(Ordering::Relaxed) == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        // An agent waits for its model's answer: give it a little, then stop it.
        for mut a in agents {
            if tokio::time::timeout(Duration::from_secs(20), &mut a).await.is_err() {
                a.abort();
                eprintln!("chloedb-experiments: {}: an agent did not stop in time", self.id);
            }
        }
        if self.spec.agents > 0 {
            self.st().stage = "summing up why the agents gave up".into();
            let why = self.sum_up().await;
            self.st().why = Some(why);
        }
        self.flush().await;
        let elapsed = self.started.elapsed().as_secs_f64();
        self.count(elapsed);
        let mut st = self.st();
        st.ended = Some(elapsed);
        if let Some(e) = st.error.clone() {
            return Err(e);
        }
        Ok(())
    }

    /// Marks the run done, once its report is saved.
    pub fn done(&self) {
        let mut st = self.st();
        if st.status != "failed" {
            st.status = "done";
            st.stage = String::new();
        }
    }

    pub fn failed(&self, e: String) {
        let mut st = self.st();
        st.status = "failed";
        st.error = Some(e);
    }

    /// Starts simulated searches at the rate asked, as many in flight as
    /// the server keeps up with.
    async fn drive(self: Arc<Self>) {
        if self.p().users.is_empty() {
            return;
        }
        let mut tick = tokio::time::interval(Duration::from_millis(50));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut tokens = 0.0;
        let mut last = Instant::now();
        while !self.stopped() {
            tick.tick().await;
            let now = Instant::now();
            let dt = (now - last).as_secs_f64();
            last = now;
            let rate = self.rate();
            tokens = (tokens + rate * dt).min(rate.max(1.0));
            while tokens >= 1.0 && self.in_flight.load(Ordering::Relaxed) < IN_FLIGHT && !self.stopped() {
                tokens -= 1.0;
                let i = self.next.fetch_add(1, Ordering::Relaxed);
                self.in_flight.fetch_add(1, Ordering::Relaxed);
                let me = self.clone();
                tokio::spawn(async move {
                    me.search(i).await;
                    me.in_flight.fetch_sub(1, Ordering::Relaxed);
                });
            }
        }
    }

    /// Simulated search `i`: its user, their arm's logged query, the
    /// clicks the model draws, written back and counted.
    async fn search(&self, i: u64) {
        let p = self.p();
        let c = p.worlds[0].context(&self.id, i);
        let a = p.assigned[c.user as usize].unwrap_or(0);
        let w = &p.worlds[a];
        match w.served_as(&self.server, &self.id, &format!("arm{a}"), &self.log, &self.events, i).await {
            Ok((imp, ops)) => {
                let u = &p.users[imp.user as usize];
                let go = ab::goes_on(u, &imp, &c.request);
                let s = ab::sums_of(&imp, imp.took_ms, go);
                let q = p.ndcg[a][imp.query as usize];
                let rate = self.rate();
                let every = (rate / 5.0).round().max(1.0) as u64;
                let shown = i % every == 0;
                {
                    let mut st = self.st();
                    let us = imp.user as usize;
                    ab::add(&mut st.sums[us], &s);
                    let k = p.worlds.len();
                    ab::add_query(&mut st.by_query, k, &imp, a);
                    st.quality[us][0] += 1.0;
                    st.quality[us][1] += q;
                    st.latencies[a].push(imp.took_ms);
                    st.searches += 1;
                }
                if !ops.is_empty() {
                    self.pending.lock().unwrap_or_else(|e| e.into_inner()).extend(ops);
                }
                if shown {
                    let clicked: Vec<usize> = imp.click.iter().enumerate().filter(|(_, &c)| c).map(|(k, _)| k + 1).collect();
                    let sat: Vec<usize> = imp.satisfied.iter().enumerate().filter(|(_, &c)| c).map(|(k, _)| k + 1).collect();
                    let text: String = p.site.queries[imp.query as usize].text.chars().take(90).collect();
                    self.feed(json!({"kind": "user", "who": persona(u)["short"], "user": u.id, "arm": a, "query": text, "clicked": clicked, "satisfied": sat, "dwell": num(imp.dwell.iter().sum()), "took_ms": num(imp.took_ms), "goes_on": go}));
                }
            }
            Err(e) => {
                let mut st = self.st();
                st.errors += 1;
                st.last_error = Some(e);
            }
        }
    }

    /// Writes the clicks gathered since the last time.
    async fn flush(&self) {
        let ops = std::mem::take(&mut *self.pending.lock().unwrap_or_else(|e| e.into_inner()));
        for chunk in ops.chunks(2000) {
            if let Err(e) = self.server.write(chunk.to_vec()).await {
                let mut st = self.st();
                st.errors += 1;
                st.last_error = Some(format!("writing clicks: {e}"));
            }
        }
    }

    /// Metric `mi` (METRICS, then the judged quality) of arm `a`, over the users so far.
    fn stat(p: &Prepared, st: &State, a: usize, mi: usize) -> ab::RatioStat {
        let (data, num, den) = if mi < METRICS.len() { (&st.sums, METRICS[mi].num, METRICS[mi].den) } else { (&st.quality, 1, 0) };
        let g: Vec<&Sums> = (0..data.len()).filter(|&u| p.assigned[u] == Some(a) && st.sums[u][N] > 0.0).map(|u| &data[u]).collect();
        ab::ratio(&g, num, den)
    }

    /// The count of the second: every metric of every arm, every
    /// comparison with the control, the sample ratio, and a point of the
    /// series.
    fn count(&self, elapsed: f64) {
        let Some(p) = self.prep.get() else { return };
        let k = self.spec.arms.len();
        let mut st = self.st();
        st.ticks += 1;
        let searches = st.searches;
        st.recent.push_back((elapsed, searches));
        while st.recent.len() > 6 {
            st.recent.pop_front();
        }
        let achieved = match (st.recent.front(), st.recent.back()) {
            (Some(a), Some(b)) if b.0 > a.0 => (b.1 - a.1) as f64 / (b.0 - a.0),
            _ => 0.0,
        };
        let nm = METRICS.len() + 1;
        let stats: Vec<Vec<ab::RatioStat>> = (0..nm).map(|mi| (0..k).map(|a| Self::stat(p, &st, a, mi)).collect()).collect();
        let mut metrics = Vec::new();
        let mut point_v = Vec::new();
        let mut point_d = Vec::new();
        let mut deltas8 = Vec::new();
        for mi in 0..nm {
            let (key, title, unit, higher, guardrail, margin, exact, about) = if mi < METRICS.len() {
                let m = &METRICS[mi];
                (m.key, m.title, m.unit, m.higher, m.guardrail, m.margin, m.exact, m.about)
            } else {
                ("ndcg", "Judged quality (nDCG)", "score", true, false, 0.0, true, "nDCG of the rows shown, by the judges' grades for the query asked: what the site's own judgments say of each arm, beside what people did.")
            };
            let s0 = stats[mi][0].clone();
            let mut comps = Vec::new();
            let mut pd = Vec::new();
            let mut d8 = Vec::new();
            for a in 1..k {
                let s1 = &stats[mi][a];
                let delta = s1.value - s0.value;
                let se = (s0.se * s0.se + s1.se * s1.se).sqrt();
                st.series[mi][a - 1].push((delta, se));
                let tau = 0.05 * s0.value.abs().max(1e-9);
                let av = ab::always_valid(&st.series[mi][a - 1], tau).last().copied().unwrap_or(1.0);
                let pv = ab::p_two(delta / se);
                let lift = s1.value / s0.value - 1.0;
                let lift_se = (s1.value / s0.value) * ((s1.se / s1.value).powi(2) + (s0.se / s0.value).powi(2)).sqrt();
                let (t0, t1) = (p.truth_arm[mi][0], p.truth_arm[mi][a]);
                let true_delta = t0.zip(t1).map(|(x, y)| y - x);
                let mde = (Z975 + Z80) * se;
                let power = true_delta.map(|t| norm_cdf(t.abs() / se - Z975) + norm_cdf(-t.abs() / se - Z975));
                comps.push(json!({
                    "arm": self.spec.arms[a].name, "delta": num(delta), "se": num(se), "lo": num(delta - Z975 * se), "hi": num(delta + Z975 * se),
                    "p": num(pv), "p_always_valid": num(av), "lift": num(lift), "lift_lo": num(lift - Z975 * lift_se), "lift_hi": num(lift + Z975 * lift_se),
                    "true_delta": opt(true_delta), "true_lift": opt(t0.zip(t1).map(|(x, y)| y / x - 1.0)), "mde": num(mde), "mde_lift": num(mde / s0.value.abs()), "power": opt(power),
                }));
                pd.push(json!([num(delta), num(delta - Z975 * se), num(delta + Z975 * se), num(pv), num(av)]));
                if mi < METRICS.len() {
                    d8.push((delta, se));
                }
            }
            let arms: Vec<Value> = (0..k).map(|a| json!({"value": num(stats[mi][a].value), "se": num(stats[mi][a].se), "users": stats[mi][a].users, "truth": opt(p.truth_arm[mi][a])})).collect();
            point_v.push(json!((0..k).map(|a| num(stats[mi][a].value)).collect::<Vec<_>>()));
            point_d.push(json!(pd));
            if mi < METRICS.len() {
                deltas8.push(d8);
            }
            metrics.push(json!({"key": key, "title": title, "unit": unit, "higher": higher, "guardrail": guardrail, "margin": margin, "exact": exact, "about": about, "arms": arms, "comparisons": comps}));
        }
        st.checkpoints.push((searches, deltas8));
        // The sample ratio: users seen in each arm against the split.
        let mut seen = vec![0.0; k];
        for (u, a) in p.assigned.iter().enumerate() {
            if let Some(a) = a {
                if st.sums[u][N] > 0.0 {
                    seen[*a] += 1.0;
                }
            }
        }
        let total: f64 = seen.iter().sum();
        let shares: f64 = self.spec.arms.iter().map(|a| a.share).sum();
        let chi: f64 = self.spec.arms.iter().zip(&seen).map(|(a, s)| { let e = total * a.share / shares; if e > 0.0 { (s - e) * (s - e) / e } else { 0.0 } }).sum();
        let srm_p = if total > 0.0 { stats::chi2_upper(chi, (k - 1) as f64) } else { 1.0 };
        let verdicts: Vec<Value> = (1..k).map(|a| verdict(&self.spec, &metrics, a)).collect();
        let lat: Vec<Value> = st.latencies.iter().map(|l| {
            let tail: Vec<f64> = l.iter().rev().take(5000).copied().collect();
            let mut v = tail;
            v.sort_by(f64::total_cmp);
            let q = |p: f64| if v.is_empty() { Value::Null } else { num(v[((v.len() - 1) as f64 * p).round() as usize]) };
            json!({"p50": q(0.5), "p95": q(0.95), "p99": q(0.99), "n": l.len()})
        }).collect();
        st.snapshot = json!({
            "elapsed": num(elapsed), "searches": searches, "achieved": num(achieved), "errors": st.errors, "last_error": st.last_error,
            "users_seen": total, "arm_users": seen,
            "srm": {"users": seen, "planned": self.spec.arms.iter().map(|a| a.share / shares).collect::<Vec<_>>(), "chi2": num(chi), "p": num(srm_p), "ok": srm_p >= 0.001},
            "metrics": metrics, "verdicts": verdicts, "latency": lat,
        });
        // A point every `every` seconds; past POINTS, every other one goes.
        if st.ticks % st.every == 0 {
            st.point_id += 1;
            let id = st.point_id;
            st.points.push(json!({"id": id, "t": num(elapsed), "n": searches, "v": point_v, "d": point_d}));
            if st.points.len() > POINTS {
                let kept: Vec<Value> = st.points.iter().enumerate().filter(|(i, _)| i % 2 == 1).map(|(_, v)| v.clone()).collect();
                st.points = kept;
                st.every *= 2;
            }
        }
    }

    /// What the page shows: the count of the last second, the points after
    /// `after` (at most 300 when none), the feed after `since`, the agents.
    pub fn view(&self, after: Option<u64>, since: u64) -> Value {
        let st = self.st();
        let points: Vec<Value> = match after {
            Some(a) => st.points.iter().filter(|p| p["id"].as_u64().unwrap_or(0) > a).cloned().collect(),
            None => {
                let step = st.points.len().div_ceil(300).max(1);
                let n = st.points.len();
                st.points.iter().enumerate().filter(|(i, _)| i % step == 0 || *i + 1 == n).map(|(_, v)| v.clone()).collect()
            }
        };
        let feed: Vec<Value> = st.feed.iter().filter(|f| f["seq"].as_u64().unwrap_or(0) > since).cloned().collect();
        let tally = |t: &[Tally]| -> Vec<Value> {
            t.iter().map(|t| json!({"tasks": t.tasks, "found": t.found, "found_relevant": t.found_relevant, "searches": t.searches, "clicks": t.clicks, "relevant_clicks": t.relevant_clicks, "first_ndcg": num(if t.tasks > 0 { t.first_ndcg / t.tasks as f64 } else { f64::NAN }), "steps": t.steps, "cost": num(t.cost)})).collect()
        };
        let mut v = st.snapshot.clone();
        if v.is_null() {
            v = json!({});
        }
        v["id"] = json!(self.id);
        v["name"] = json!(self.spec.name);
        v["site"] = json!(self.spec.site);
        v["about"] = json!(self.about);
        v["spec"] = json!(self.spec);
        v["created"] = json!(self.created);
        v["status"] = json!(st.status);
        v["stage"] = json!(st.stage);
        v["error"] = json!(st.error);
        v["rate"] = num(self.rate());
        v["limit_s"] = num(self.spec.minutes * 60.0);
        v["elapsed"] = num(st.ended.unwrap_or_else(|| self.started.elapsed().as_secs_f64()));
        v["arms"] = json!(self.spec.arms);
        v["points"] = json!(points);
        v["points_last"] = json!(st.point_id);
        v["feed"] = json!(feed);
        v["seq"] = json!(st.seq);
        v["agents"] = json!(st.agents);
        v["agent_tally"] = json!(tally(&st.agent_tally[..]));
        v["visitor_tally"] = json!(tally(&st.visitor_tally[..]));
        v["cost"] = num(st.cost);
        v["budget"] = num(self.spec.budget);
        v["brain"] = json!(self.brain.name());
        v["until"] = json!(self.spec.until);
        v["decided"] = st.decided.clone().unwrap_or(Value::Null);
        v["waiting"] = json!(st.waiting);
        v["why"] = st.why.clone().unwrap_or(Value::Null);
        v
    }

    /// Everything the report is written from: each user's arm and sums,
    /// their exact values, the checkpoints and every search's time.
    pub fn run_data(&self) -> Option<RunData> {
        let p = self.prep.get()?;
        let st = self.st();
        let n = st.checkpoints.len();
        let step = n.div_ceil(20).max(1);
        let checkpoints: Vec<(u64, Vec<Vec<(f64, f64)>>)> = st.checkpoints.iter().enumerate().filter(|(i, c)| c.0 > 0 && ((i + 1) % step == 0 || i + 1 == n)).map(|(_, c)| c.clone()).collect();
        let nu = p.users.len();
        Some(RunData {
            spec: self.spec.ab(st.searches),
            users: p.users.clone(),
            arm: p.assigned.clone(),
            sums: st.sums.clone(),
            pre: vec![[0.0; SUMS]; nu],
            truth: p.truth.clone(),
            share: p.share.clone(),
            checkpoints,
            latencies: st.latencies.clone(),
            whatif: None,
            seconds: st.ended.unwrap_or_else(|| self.started.elapsed().as_secs_f64()),
            site: json!({"name": p.site.name, "table": p.site.table, "queries": p.site.queries.len()}),
            by_query: Some(ab::ByQuery { measured: st.by_query.clone(), ..p.by_query.clone() }),
        })
    }

    /// What the report keeps of the live run: the agents, the visitors,
    /// the cost.
    pub fn live_part(&self) -> Value {
        let v = self.view(Some(u64::MAX), u64::MAX);
        json!({"agents": v["agents"], "agent_tally": v["agent_tally"], "visitor_tally": v["visitor_tally"], "cost": v["cost"], "budget": v["budget"], "brain": v["brain"], "rate": v["rate"], "minutes": self.spec.minutes, "elapsed": v["elapsed"], "errors": v["errors"], "until": v["until"], "decided": v["decided"], "waiting": v["waiting"], "why": v["why"]})
    }

    // ------------------------------------------------------------ the search page

    /// The page arm `a` shows for `words`: a logged query to the server,
    /// its rows with their titles and the start of their text.
    async fn page(&self, a: usize, words: &str, context: &str, request: &str, need: Option<usize>) -> Result<Vec<Row>> {
        let p = self.p();
        let sql = format!("SELECT id FROM {} ORDER BY {} DESC LIMIT {}", p.site.table, self.spec.arms[a].rank, self.spec.limit);
        let l = json!({"request": request, "context": context, "ts": now_us(), "table": self.log, "variant": format!("arm{a}")});
        let ans = self.server.query(&sql, json!({"t": words}), Some(l)).await?;
        let mut keys: Vec<String> = ans["rows"].as_array().into_iter().flatten().map(|r| key_text(&r[0])).collect();
        ab::then_apply(&self.spec.arms[a].then, &mut keys);
        let texts = self.texts(&keys).await?;
        Ok(keys
            .into_iter()
            .map(|k| {
                let (title, body) = texts.get(&k).cloned().unwrap_or_default();
                let snippet: String = body.chars().take(240).collect();
                let grade = need.map(|q| p.site.grades[q].get(&k).copied().unwrap_or(0));
                Row { key: k, title, snippet, grade, opened: false }
            })
            .collect())
    }

    /// Rows' titles and text, by key: a column called title (or the first
    /// short text), and one called body or text (or the longest).
    async fn texts(&self, keys: &[String]) -> Result<HashMap<String, (String, String)>> {
        if keys.is_empty() {
            return Ok(HashMap::new());
        }
        let list = keys.iter().map(|k| lit(k)).collect::<Vec<_>>().join(", ");
        let a = self.server.query(&format!("SELECT * FROM {} WHERE id IN ({list})", self.p().site.table), json!({}), None).await?;
        let cols: Vec<String> = a["columns"].as_array().into_iter().flatten().filter_map(|c| c.as_str().map(str::to_string)).collect();
        let id = cols.iter().position(|c| c == "id").unwrap_or(0);
        let mut out = HashMap::new();
        for r in a["rows"].as_array().into_iter().flatten() {
            let texts: Vec<(&str, &str)> = cols.iter().enumerate().filter(|(i, _)| *i != id).filter_map(|(i, c)| r[i].as_str().map(|s| (c.as_str(), s))).filter(|(_, s)| !s.trim().is_empty()).collect();
            let body = texts.iter().find(|(c, _)| *c == "body" || *c == "text").or_else(|| texts.iter().max_by_key(|(_, s)| s.len())).map(|(_, s)| s.to_string()).unwrap_or_default();
            let title = texts.iter().find(|(c, _)| *c == "title").map(|(_, s)| s.to_string()).or_else(|| texts.iter().find(|(_, s)| *s != body).map(|(_, s)| s.chars().take(120).collect())).unwrap_or_else(|| body.chars().take(90).collect::<String>() + "…");
            out.insert(key_text(&r[id]), (title, body));
        }
        Ok(out)
    }

    fn click(&self, request: &str, k: usize, who: &str, key: &str, action: &str, dwell_s: f64) {
        let suffix = if action == "click" { String::new() } else { format!("-{action}") };
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).push(json!({"op": "upsert", "table": self.events, "doc": {"id": format!("{request}#{k}{suffix}"), "usr": who, "item": key, "action": action, "ts": now_us(), "dwell_ms": dwell_s * 1000.0}}));
    }

    // ------------------------------------------------------------ visitors

    /// A search by someone watching, `who` a name their page keeps: their
    /// arm by the split, the page it shows.
    pub async fn try_search(&self, who: &str, q: &str) -> Result<Value> {
        if self.prep.get().is_none() {
            return Err("the run is still starting".into());
        }
        let who: String = who.chars().filter(|c| c.is_ascii_alphanumeric()).take(40).collect();
        let user = (fnv(&who) as u32) | 0x8000_0000;
        let a = ab::assign(&self.spec.ab(0), user).unwrap_or(0);
        let n = self.visits.fetch_add(1, Ordering::Relaxed);
        let request = format!("{}-v{n}", self.id);
        let rows = self.page(a, q, &format!("you-{who}"), &request, None).await?;
        let out = json!({"request": request, "arm": a, "arm_name": self.spec.arms[a].name, "rows": rows});
        {
            let mut st = self.st();
            st.visitor_tally[a].searches += 1;
            st.visitor_tally[a].tasks += 1;
            st.visits.insert(request.clone(), Visit { arm: a, who: who.clone(), rows, opened: Vec::new() });
            st.visit_order.push_back(request.clone());
            while st.visit_order.len() > 200 {
                if let Some(old) = st.visit_order.pop_front() {
                    st.visits.remove(&old);
                }
            }
        }
        self.feed(json!({"kind": "you", "arm": a, "action": "search", "text": q.chars().take(90).collect::<String>()}));
        Ok(out)
    }

    /// A visitor opens result `n` (from 1) of their page `request`.
    pub async fn try_open(&self, request: &str, n: usize) -> Result<Value> {
        let (key, a, who) = {
            let mut st = self.st();
            let v = st.visits.get_mut(request).ok_or("that page is gone: search again")?;
            let r = v.rows.get_mut(n.wrapping_sub(1)).ok_or("no such result")?;
            r.opened = true;
            let key = r.key.clone();
            v.opened.push(n);
            let (a, who) = (v.arm, v.who.clone());
            st.visitor_tally[a].clicks += 1;
            (key, a, who)
        };
        self.click(request, n - 1, &format!("you-{who}"), &key, "click", 0.0);
        let texts = self.texts(std::slice::from_ref(&key)).await?;
        let (title, body) = texts.get(&key).cloned().unwrap_or_default();
        self.feed(json!({"kind": "you", "arm": a, "action": "open", "text": title.chars().take(90).collect::<String>(), "rank": n}));
        Ok(json!({"key": key, "title": title, "body": body.chars().take(4000).collect::<String>()}))
    }

    /// A visitor says result `n` of page `request` answered what they
    /// looked for (or, with 0, that nothing did).
    pub fn try_done(&self, request: &str, n: usize, found: bool) -> Result<Value> {
        let (key, a, who) = {
            let mut st = self.st();
            let v = st.visits.remove(request).ok_or("that page is gone: search again")?;
            let key = (found && n >= 1 && v.opened.contains(&n)).then(|| v.rows[n - 1].key.clone());
            if key.is_some() {
                st.visitor_tally[v.arm].found += 1;
            }
            (key, v.arm, v.who)
        };
        if let Some(k) = &key {
            self.click(request, n - 1, &format!("you-{who}"), k, "satisfied", 60.0);
        }
        self.feed(json!({"kind": "you", "arm": a, "action": "done", "found": key.is_some(), "rank": n}));
        Ok(json!({"found": key.is_some()}))
    }

    // ------------------------------------------------------------ agents

    fn agent_set(&self, n: usize, f: impl FnOnce(&mut AgentView)) {
        let mut st = self.st();
        if let Some(a) = st.agents.iter_mut().find(|a| a.n == n) {
            f(a);
        }
    }

    fn spent(&self) -> bool {
        let reserve = if matches!(self.brain, Brain::Claude { .. }) { SUM_UP_RESERVE.min(self.spec.budget * 0.1) } else { 0.0 };
        self.st().cost >= self.spec.budget - reserve
    }

    /// The run decided at `elapsed` seconds ([`decision`]), or what it
    /// still waits on.
    fn decided(&self, elapsed: f64) -> std::result::Result<Value, String> {
        let st = self.st();
        let read = decision(&st.snapshot, &self.spec.primary, self.spec.arms.len(), st.searches)?;
        let calls: Vec<String> = st.snapshot["verdicts"].as_array().into_iter().flatten().map(|v| format!("{}: {}", v["arm"].as_str().unwrap_or(""), v["call"].as_str().unwrap_or(""))).collect();
        let text = format!("Decided at {} searches, after {}: {}. The guardrails are read, so the run stops here.", fmt_n(st.searches), clock(elapsed), calls.join("; "));
        Ok(json!({"searches": st.searches, "elapsed": num(elapsed), "verdicts": st.snapshot["verdicts"], "guardrails": read, "text": text}))
    }

    /// One line per arm on why its agents gave up, once the run ends: from
    /// Claude in one call over their notes, when Claude plays them and some
    /// gave up, and from the counts otherwise (or when that call fails).
    async fn sum_up(&self) -> Value {
        let (gave, tallies) = {
            let st = self.st();
            (st.gave_up.clone(), st.agent_tally.clone())
        };
        let k = self.spec.arms.len();
        let mut per: Vec<Vec<(String, String)>> = vec![Vec::new(); k];
        for (a, need, note) in gave {
            per[a].push((need, note));
        }
        let mut arms: Vec<Value> = (0..k)
            .map(|a| {
                let (t, n) = (tallies[a].tasks, per[a].len());
                let text = if t == 0 {
                    "No agent finished a task here.".to_string()
                } else if n == 0 {
                    format!("Every one of its {t} task{} ended with what the agent needed.", if t == 1 { "" } else { "s" })
                } else {
                    let notes: Vec<String> = per[a].iter().take(3).map(|(_, note)| format!("\"{}\"", note.trim_end_matches('.'))).collect();
                    format!("{n} of {t} tasks ended without what the agent needed. They said: {}.", notes.join("; "))
                };
                json!({"arm": a, "name": self.spec.arms[a].name, "tasks": t, "gave_up": n, "text": text})
            })
            .collect();
        let mut by = "counts".to_string();
        if let Brain::Claude { key, model, base } = &self.brain {
            if per.iter().any(|p| !p.is_empty()) {
                let mut prompt = String::from("AI agents, each playing a person, used a search site to find something they needed. The site showed each of them one of several rankings (arms). Below, per arm, are the tasks they gave up on: what they looked for, and their own note.\n\nFor each arm with notes, write one plain sentence on why its agents gave up, as a pattern across the notes rather than a list of them. Answer with one line per arm: its number, a colon, and the sentence. Nothing else.\n");
                for (a, p) in per.iter().enumerate() {
                    if p.is_empty() {
                        continue;
                    }
                    prompt.push_str(&format!("\nArm {a} ({}), {} of {} tasks given up:\n", self.spec.arms[a].name, p.len(), tallies[a].tasks));
                    for (need, note) in p.iter().take(20) {
                        prompt.push_str(&format!("- looked for \"{}\": {}\n", need.chars().take(120).collect::<String>(), note.chars().take(240).collect::<String>()));
                    }
                }
                let (_, _, pin, pout, fallbacks) = AGENT_MODELS.iter().copied().find(|m| m.0 == model.as_str()).unwrap_or(AGENT_MODELS[0]);
                let mut body = json!({"model": model, "max_tokens": 600, "output_config": {"effort": "low"}, "messages": [{"role": "user", "content": prompt}]});
                if fallbacks {
                    body["fallbacks"] = json!("default");
                }
                match call(&self.http, base, key, fallbacks, &body).await {
                    Ok(r) => {
                        let u = &r["usage"];
                        let f = |k: &str| u[k].as_f64().unwrap_or(0.0);
                        let cost = (f("input_tokens") * pin + f("output_tokens") * pout) / 1e6;
                        self.st().cost += cost;
                        let text: String = r["content"].as_array().into_iter().flatten().filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n");
                        let mut any = false;
                        for line in text.lines() {
                            let Some((n, sentence)) = line.trim().trim_start_matches("Arm ").split_once(':') else { continue };
                            let Ok(a) = n.trim().parse::<usize>() else { continue };
                            if a < k && !per[a].is_empty() && !sentence.trim().is_empty() {
                                arms[a]["text"] = json!(sentence.trim().chars().take(400).collect::<String>());
                                arms[a]["summed_up"] = json!(true);
                                any = true;
                            }
                        }
                        if any {
                            by = model.clone();
                        }
                    }
                    Err(e) => eprintln!("chloedb-experiments: {}: summing up the agents: {e}", self.id),
                }
            }
        }
        json!({"by": by, "arms": arms})
    }

    /// Agent `n`: a person of the population doing one task after another
    /// on the search page, until the run ends or the budget is spent.
    async fn agent(self: Arc<Self>, n: usize) {
        let p = self.p();
        if p.users.is_empty() {
            return;
        }
        let mut rng = Rng::of(&format!("{}/agent/{n}", self.id));
        let taken: Vec<u32> = self.st().agents.iter().map(|a| a.user).collect();
        // The agents take the arms in turn, so each has as many: a person
        // of the population whom the split puts in arm n mod k.
        let want = n % p.worlds.len();
        let mut user = p.worlds[0].users.draw(&mut rng).id;
        for _ in 0..2000 {
            if !taken.contains(&user) && p.assigned[user as usize] == Some(want) {
                break;
            }
            user = p.worlds[0].users.draw(&mut rng).id;
        }
        let u = p.users[user as usize].clone();
        let arm = p.assigned[user as usize].unwrap_or(0);
        self.st().agents.push(AgentView { n, user, persona: persona(&u), arm, status: "starting".into(), need: None, typed: String::new(), rows: Vec::new(), steps: Vec::new(), last: None, tasks: 0, found: 0, cost: 0.0, error: None });
        let mut task = 0usize;
        while !self.stopped() {
            if self.spent() {
                self.agent_set(n, |a| a.status = "budget spent".into());
                self.feed(json!({"kind": "agent", "n": n, "arm": arm, "action": "note", "text": "stopped: the agents' budget is spent"}));
                return;
            }
            let q = p.queries.draw(&mut rng);
            match self.task(n, &u, arm, q, task).await {
                Ok(()) => {}
                Err(e) => {
                    self.agent_set(n, |a| {
                        a.status = "failed".into();
                        a.error = Some(e.clone());
                    });
                    self.feed(json!({"kind": "agent", "n": n, "arm": arm, "action": "note", "text": format!("failed: {e}")}));
                    return;
                }
            }
            task += 1;
            // A pause between tasks, as a person's.
            for _ in 0..10 {
                if self.stopped() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
        self.agent_set(n, |a| {
            if a.status != "failed" {
                a.status = "stopped".into();
            }
        });
    }

    /// One task of agent `n` (user `u`, in arm `arm`): find what query `q`
    /// asks, in at most [`AGENT_STEPS`] actions.
    async fn task(&self, n: usize, u: &User, arm: usize, q: usize, task: usize) -> Result<()> {
        let p = self.p();
        let need = p.site.queries[q].text.clone();
        let who = format!("agent{n}");
        self.agent_set(n, |a| {
            a.need = Some(need.clone());
            a.steps.clear();
            a.rows.clear();
            a.typed.clear();
            a.status = "thinking".into();
        });
        let mut mind = Mind::new(&self.brain, &self.http, persona_text(u), &self.about, &need);
        let mut rows: Vec<Row> = Vec::new();
        let mut page_request = String::new();
        let mut obs: Option<String> = None;
        let mut first_page = true;
        let mut outcome = json!({"found": false, "note": "ran out of actions"});
        let mut tally = Tally { tasks: 1, ..Tally::default() };
        let mut ended = false;
        // A task the run's end or the budget cuts short is not scored: it
        // would count as a need the site failed.
        let mut cut = false;
        for step in 0..AGENT_STEPS {
            if self.stopped() {
                outcome = json!({"found": false, "note": "the run ended"});
                cut = true;
                break;
            }
            if self.spent() {
                outcome = json!({"found": false, "note": "the budget is spent"});
                cut = true;
                break;
            }
            self.agent_set(n, |a| a.status = "thinking".into());
            let (action, cost) = mind.act(obs.take(), &rows).await?;
            tally.cost += cost;
            tally.steps += 1;
            {
                let mut st = self.st();
                st.cost += cost;
                if let Some(a) = st.agents.iter_mut().find(|a| a.n == n) {
                    a.cost += cost;
                }
            }
            match action {
                Action::Search { query, why } => {
                    self.agent_set(n, |a| {
                        a.status = "searching".into();
                        a.typed = query.clone();
                    });
                    page_request = format!("{}-a{n}-{task}-{step}", self.id);
                    rows = self.page(arm, &query, &who, &page_request, Some(q)).await?;
                    tally.searches += 1;
                    if first_page {
                        let keys: Vec<String> = rows.iter().map(|r| r.key.clone()).collect();
                        tally.first_ndcg += ndcg(&p.site.grades[q], p.site.max_grade, &keys, self.spec.limit);
                        first_page = false;
                    }
                    obs = Some(render_page(&query, &rows));
                    let shown = rows.clone();
                    self.agent_set(n, |a| {
                        a.rows = shown;
                        a.steps.push(json!({"kind": "search", "text": query, "why": why}));
                    });
                    self.feed(json!({"kind": "agent", "n": n, "arm": arm, "action": "search", "text": query.chars().take(90).collect::<String>(), "why": why}));
                }
                Action::Open { result, why } => {
                    let Some(r) = result.checked_sub(1).and_then(|i| rows.get(i)).cloned() else {
                        obs = Some(format!("There is no result {result} on the page you are looking at{}.", if rows.is_empty() { " (search first)" } else { "" }));
                        continue;
                    };
                    self.agent_set(n, |a| a.status = "reading".into());
                    rows[result - 1].opened = true;
                    tally.clicks += 1;
                    if r.grade.unwrap_or(0) > 0 {
                        tally.relevant_clicks += 1;
                    }
                    self.click(&page_request, result - 1, &who, &r.key, "click", 0.0);
                    let texts = self.texts(std::slice::from_ref(&r.key)).await?;
                    let (title, body) = texts.get(&r.key).cloned().unwrap_or_default();
                    obs = Some(format!("Result {result}: {title}\n\n{}", body.chars().take(1800).collect::<String>()));
                    let shown = rows.clone();
                    self.agent_set(n, |a| {
                        a.rows = shown;
                        a.steps.push(json!({"kind": "open", "rank": result, "text": title, "why": why, "grade": r.grade}));
                    });
                    self.feed(json!({"kind": "agent", "n": n, "arm": arm, "action": "open", "rank": result, "text": title.chars().take(90).collect::<String>(), "why": why, "grade": r.grade}));
                }
                Action::Done { found, result, note } => {
                    let row = result.checked_sub(1).and_then(|i| rows.get(i)).filter(|r| r.opened).cloned();
                    let found = found && row.is_some();
                    let relevant = row.as_ref().is_some_and(|r| r.grade.unwrap_or(0) > 0);
                    if found {
                        tally.found += 1;
                        if relevant {
                            tally.found_relevant += 1;
                        }
                        if let Some(r) = &row {
                            self.click(&page_request, result - 1, &who, &r.key, "satisfied", 60.0);
                        }
                    }
                    outcome = json!({"found": found, "relevant": relevant, "rank": result, "note": note, "title": row.as_ref().map(|r| r.title.clone()), "grade": row.as_ref().and_then(|r| r.grade)});
                    self.agent_set(n, |a| a.steps.push(json!({"kind": "done", "found": found, "relevant": relevant, "rank": result, "text": note})));
                    self.feed(json!({"kind": "agent", "n": n, "arm": arm, "action": "done", "found": found, "relevant": relevant, "text": note}));
                    ended = true;
                    break;
                }
                Action::Say(text) => {
                    self.agent_set(n, |a| a.steps.push(json!({"kind": "say", "text": text})));
                    obs = Some("Please act with one of the tools: search, open or done.".into());
                }
                Action::Quit(reason) => {
                    outcome = json!({"found": false, "note": reason});
                    break;
                }
            }
        }
        if !ended {
            let note = if cut { json!(format!("cut short: {}", outcome["note"].as_str().unwrap_or(""))) } else { outcome["note"].clone() };
            self.agent_set(n, |a| a.steps.push(json!({"kind": if cut { "cut" } else { "done" }, "found": false, "text": note})));
        }
        let mut st = self.st();
        let t = &mut st.agent_tally[arm];
        if cut {
            // What it cost still counts.
            t.cost += tally.cost;
            t.steps += tally.steps;
            if let Some(a) = st.agents.iter_mut().find(|a| a.n == n) {
                a.status = "between tasks".into();
            }
            return Ok(());
        }
        t.tasks += tally.tasks;
        t.found += tally.found;
        t.found_relevant += tally.found_relevant;
        t.searches += tally.searches;
        t.clicks += tally.clicks;
        t.relevant_clicks += tally.relevant_clicks;
        t.first_ndcg += tally.first_ndcg;
        t.steps += tally.steps;
        t.cost += tally.cost;
        if tally.found == 0 {
            let note = outcome["note"].as_str().unwrap_or("").to_string();
            st.gave_up.push((arm, need.clone(), note));
        }
        if let Some(a) = st.agents.iter_mut().find(|a| a.n == n) {
            a.tasks += 1;
            if tally.found > 0 {
                a.found += 1;
            }
            let mut last = outcome;
            last["need"] = json!(need);
            last["steps"] = json!(a.steps.len());
            a.last = Some(last);
            a.status = "between tasks".into();
        }
        Ok(())
    }
}

/// The verdict of a live run for arm `a`: its primary metric by the
/// always-valid p value, which may be read at any time.
fn verdict(spec: &LiveSpec, metrics: &[Value], a: usize) -> Value {
    let primary = metrics.iter().find(|m| m["key"] == json!(spec.primary)).unwrap_or(&metrics[0]);
    let c = &primary["comparisons"][a - 1];
    let higher = primary["higher"].as_bool().unwrap_or(true);
    let p = c["p_always_valid"].as_f64().unwrap_or(1.0);
    let delta = c["delta"].as_f64().unwrap_or(0.0);
    let better = (delta > 0.0) == higher;
    let worse_guards: Vec<String> = metrics
        .iter()
        .filter(|m| m["guardrail"].as_bool().unwrap_or(false))
        .filter(|m| {
            let c = &m["comparisons"][a - 1];
            let lift = c["lift"].as_f64().unwrap_or(0.0);
            let worse = (lift > 0.0) != m["higher"].as_bool().unwrap_or(true);
            worse && lift.abs() > m["margin"].as_f64().unwrap_or(0.0) && c["p_always_valid"].as_f64().unwrap_or(1.0) < 0.05
        })
        .map(|m| m["title"].as_str().unwrap_or("").to_string())
        .collect();
    let call = if p >= 0.05 {
        "no detectable difference"
    } else if better && worse_guards.is_empty() {
        "ship"
    } else if better {
        "better, but a guardrail got worse"
    } else {
        "worse"
    };
    let truth_call = c["true_delta"].as_f64().map(|t| if t.abs() < 1e-12 { "no true difference" } else if (t > 0.0) == higher { "truly better" } else { "truly worse" });
    json!({"arm": spec.arms[a].name, "metric": primary["key"], "call": call, "p_always_valid": c["p_always_valid"], "p": c["p"], "lift": c["lift"], "lift_lo": c["lift_lo"], "lift_hi": c["lift_hi"], "true_lift": c["true_lift"], "truth_call": truth_call, "guardrails_worse": worse_guards, "power": c["power"]})
}

/// A results page, as the agent reads it.
pub(crate) fn render_page(query: &str, rows: &[Row]) -> String {
    if rows.is_empty() {
        return format!("No results for \"{query}\".");
    }
    let mut s = format!("Results for \"{query}\":\n");
    for (i, r) in rows.iter().enumerate() {
        s.push_str(&format!("\n{}. {}\n   {}{}\n", i + 1, r.title, r.snippet, if r.snippet.chars().count() >= 240 { "…" } else { "" }));
    }
    s
}

// ------------------------------------------------------------ minds

/// What an agent does next.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Search { query: String, why: String },
    Open { result: usize, why: String },
    Done { found: bool, result: usize, note: String },
    /// It wrote instead of acting.
    Say(String),
    /// It cannot go on (the model declined, or its answer was cut off).
    Quit(String),
}

const SYSTEM: &str = "You are taking part in a study of a website's search. You play one person, described in the first message, who came to the site with a need. Act as that person would: search in the words they would type (short, as people type into a search box), judge the results by what the page shows, open one when it looks like it answers the need, and stop when it does or when you would give up. Use the tools for every step; give each one a short reason, one sentence, in the person's voice.";

fn tools() -> Value {
    json!([
        {"name": "search", "description": "Type a search into the site's search box and see the page of results.", "input_schema": {"type": "object", "properties": {"query": {"type": "string", "description": "What you type, in your own words"}, "why": {"type": "string", "description": "One short sentence: why you search this"}}, "required": ["query", "why"]}},
        {"name": "open", "description": "Open one result of the page you are looking at, and read it.", "input_schema": {"type": "object", "properties": {"result": {"type": "integer", "description": "The result's number on the page"}, "why": {"type": "string", "description": "One short sentence: why this one"}}, "required": ["result", "why"]}},
        {"name": "done", "description": "Stop: you found what you came for, or you would give up now.", "input_schema": {"type": "object", "properties": {"found": {"type": "boolean", "description": "Whether a result you opened answers your need"}, "result": {"type": "integer", "description": "The number of the result that answered it, one you opened; 0 if none did"}, "note": {"type": "string", "description": "One sentence on how it went"}}, "required": ["found", "result", "note"]}}
    ])
}

pub(crate) struct Mind<'a> {
    brain: &'a Brain,
    http: &'a reqwest::Client,
    need: String,
    messages: Vec<Value>,
    /// The tool call the next observation answers.
    pending: Option<String>,
    step: usize,
}

impl<'a> Mind<'a> {
    pub(crate) fn new(brain: &'a Brain, http: &'a reqwest::Client, who: String, about: &str, need: &str) -> Mind<'a> {
        let first = format!(
            "You are {who}.\n\nThe site: {about}\n\nYou came to it looking for this:\n\n\"{need}\"\n\nFind a page on the site that answers it, as this person would. You have at most {AGENT_STEPS} actions."
        );
        Mind { brain, http, need: need.to_string(), messages: vec![json!({"role": "user", "content": first})], pending: None, step: 0 }
    }

    /// The next action after `obs` (what the last one showed), and what
    /// deciding it cost, in dollars.
    pub(crate) async fn act(&mut self, obs: Option<String>, rows: &[Row]) -> Result<(Action, f64)> {
        self.step += 1;
        match self.brain {
            Brain::Scripted => Ok((self.scripted(rows), 0.0)),
            Brain::Claude { key, model, base } => {
                if let Some(o) = obs {
                    match self.pending.take() {
                        Some(id) => self.messages.push(json!({"role": "user", "content": [{"type": "tool_result", "tool_use_id": id, "content": o}]})),
                        None => self.messages.push(json!({"role": "user", "content": o})),
                    }
                }
                let (_, _, _, _, fallbacks) = model_of(model);
                let mut body = json!({
                    "model": model, "max_tokens": 4096, "system": SYSTEM, "tools": tools(),
                    "tool_choice": {"type": "auto", "disable_parallel_tool_use": true},
                    "output_config": {"effort": "low"}, "messages": self.messages,
                });
                if fallbacks {
                    body["fallbacks"] = json!("default");
                }
                let r = call(self.http, base, key, fallbacks, &body).await?;
                let cost = cost_of(model, &r["usage"]);
                let stop = r["stop_reason"].as_str().unwrap_or("");
                if stop == "refusal" {
                    return Ok((Action::Quit("the model declined to go on".into()), cost));
                }
                let content = r["content"].clone();
                self.messages.push(json!({"role": "assistant", "content": content}));
                let blocks = content.as_array().cloned().unwrap_or_default();
                let call = blocks.iter().find(|b| b["type"] == "tool_use");
                if stop == "max_tokens" {
                    return Ok((Action::Quit("the model's answer was cut off".into()), cost));
                }
                let Some(c) = call else {
                    let text: String = blocks.iter().filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join(" ");
                    return Ok((Action::Say(text.chars().take(400).collect()), cost));
                };
                self.pending = c["id"].as_str().map(str::to_string);
                let input = &c["input"];
                let text = |k: &str| input[k].as_str().unwrap_or("").chars().take(300).collect::<String>();
                let int = |k: &str| input[k].as_u64().or_else(|| input[k].as_f64().map(|x| x as u64)).or_else(|| input[k].as_str().and_then(|s| s.trim().parse().ok())).unwrap_or(0) as usize;
                let action = match c["name"].as_str().unwrap_or("") {
                    "search" => Action::Search { query: text("query"), why: text("why") },
                    "open" => Action::Open { result: int("result"), why: text("why") },
                    "done" => Action::Done { found: input["found"].as_bool().unwrap_or(false), result: int("result"), note: text("note") },
                    other => {
                        // An unknown tool: answer it, and ask again.
                        return Ok((Action::Say(format!("(asked for a tool {other:?})")), cost));
                    }
                };
                Ok((action, cost))
            }
        }
    }

    /// No model: search the need's words, open the result whose title
    /// shares the most words with it, and stop.
    fn scripted(&self, rows: &[Row]) -> Action {
        let need = words(&self.need);
        let overlap = |r: &Row| words(&format!("{} {}", r.title, r.snippet)).iter().filter(|w| need.contains(w)).count();
        match self.step {
            1 => Action::Search { query: need.iter().take(8).cloned().collect::<Vec<_>>().join(" "), why: "the need's own words".into() },
            2 => match rows.iter().enumerate().max_by_key(|(i, r)| (overlap(r), usize::MAX - i)) {
                Some((i, _)) => Action::Open { result: i + 1, why: "its title shares the most words with what I need".into() },
                None => Action::Done { found: false, result: 0, note: "no results".into() },
            },
            _ => {
                let best = rows.iter().enumerate().find(|(_, r)| r.opened);
                match best {
                    Some((i, r)) => {
                        let found = overlap(r) >= 2;
                        Action::Done { found, result: if found { i + 1 } else { 0 }, note: if found { "it covers what I asked".into() } else { "not what I was after".into() } }
                    }
                    None => Action::Done { found: false, result: 0, note: "nothing opened".into() },
                }
            }
        }
    }
}

/// A model of [`AGENT_MODELS`] by its id (the first when unknown).
pub(crate) fn model_of(model: &str) -> (&'static str, &'static str, f64, f64, bool) {
    AGENT_MODELS.iter().copied().find(|m| m.0 == model).unwrap_or(AGENT_MODELS[0])
}

/// What a call's usage cost, in dollars.
pub(crate) fn cost_of(model: &str, u: &Value) -> f64 {
    let (_, _, pin, pout, _) = model_of(model);
    let f = |k: &str| u[k].as_f64().unwrap_or(0.0);
    (f("input_tokens") * pin + f("cache_creation_input_tokens") * pin * 1.25 + f("cache_read_input_tokens") * pin * 0.1 + f("output_tokens") * pout) / 1e6
}

/// One call to the Messages API, again on overload or a lost connection.
pub(crate) async fn call(http: &reqwest::Client, base: &str, key: &str, fallbacks: bool, body: &Value) -> Result<Value> {
    let url = format!("{}/v1/messages", base.trim_end_matches('/'));
    let mut wait = Duration::from_secs(2);
    for attempt in 0.. {
        let mut req = http.post(&url).header("x-api-key", key).header("anthropic-version", API_VERSION).json(body);
        if fallbacks {
            req = req.header("anthropic-beta", "server-side-fallback-2026-07-01");
        }
        let r = match req.send().await {
            Ok(r) => r,
            Err(e) if attempt < 4 && (e.is_connect() || e.is_timeout()) => {
                tokio::time::sleep(wait).await;
                wait *= 2;
                continue;
            }
            Err(e) => return Err(format!("the Messages API: {e}")),
        };
        let status = r.status().as_u16();
        let after = r.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok());
        let text = r.text().await.map_err(|e| format!("the Messages API: {e}"))?;
        if status == 200 {
            return serde_json::from_str(&text).map_err(|e| format!("the Messages API's answer: {e}"));
        }
        if matches!(status, 429 | 500 | 502 | 503 | 504 | 529) && attempt < 4 {
            tokio::time::sleep(after.map(Duration::from_secs).unwrap_or(wait).min(Duration::from_secs(30))).await;
            wait *= 2;
            continue;
        }
        let j: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        let m = j["error"]["message"].as_str().map(str::to_string).unwrap_or_else(|| text.chars().take(300).collect());
        return Err(format!("the Messages API ({status}): {m}"));
    }
    unreachable!()
}

/// One answer by a tool: `tool`'s input, and what the call cost. The
/// newest models refuse a forced `tool_choice`, so the call is `auto` with
/// the tool named in the message, and made again (twice at most) when no
/// call of it comes back.
pub(crate) async fn tool_answer(http: &reqwest::Client, base: &str, key: &str, model: &str, system: &str, tool: &Value, message: &str, max_tokens: u32) -> Result<(Value, f64)> {
    let name = tool["name"].as_str().unwrap_or("answer");
    let (_, _, _, _, fallbacks) = model_of(model);
    let mut body = json!({
        "model": model, "max_tokens": max_tokens, "system": system, "tools": [tool],
        "tool_choice": {"type": "auto", "disable_parallel_tool_use": true},
        "messages": [{"role": "user", "content": format!("{message}\n\nAnswer only by calling the {name} tool, once.")}],
    });
    if fallbacks {
        body["fallbacks"] = json!("default");
    }
    let mut cost = 0.0;
    for _ in 0..3 {
        let r = call(http, base, key, fallbacks, &body).await?;
        cost += cost_of(model, &r["usage"]);
        if let Some(b) = r["content"].as_array().into_iter().flatten().find(|b| b["type"] == "tool_use" && b["name"] == name) {
            return Ok((b["input"].clone(), cost));
        }
        if r["stop_reason"] == "max_tokens" {
            return Err(format!("the answer was cut off at {max_tokens} tokens: ask for less"));
        }
    }
    Err(format!("the model did not call {name}"))
}

/// Whether a live run whose last second's numbers are `snapshot` is
/// decided, after `searches` of them: at least [`MIN_DECIDE`], every arm's
/// `primary` metric called by its always-valid p value (below 0.05), and
/// every guardrail read, either moved by its own always-valid p value or
/// held within its margin by its 95% interval. The guardrails as read, or
/// one line on what it waits on.
pub fn decision(snapshot: &Value, primary: &str, arms: usize, searches: u64) -> std::result::Result<Vec<Value>, String> {
    if searches < MIN_DECIDE {
        return Err(format!("at least {} searches", fmt_n(MIN_DECIDE)));
    }
    let metrics = snapshot["metrics"].as_array().ok_or("the first numbers")?;
    let p = metrics.iter().find(|m| m["key"] == json!(primary)).ok_or("the primary metric")?;
    let name = |a: usize| snapshot["verdicts"][a - 1]["arm"].as_str().map_or(format!("arm {a}"), str::to_string);
    let mut read = Vec::new();
    for a in 1..arms {
        if p["comparisons"][a - 1]["p_always_valid"].as_f64().unwrap_or(1.0) >= 0.05 {
            return Err(format!("{} for {}: its always-valid p value is not below 0.05 yet", p["title"].as_str().unwrap_or(primary), name(a)));
        }
        for m in metrics.iter().filter(|m| m["guardrail"] == json!(true)) {
            let c = &m["comparisons"][a - 1];
            let moved = c["p_always_valid"].as_f64().unwrap_or(1.0) < 0.05;
            let margin = m["margin"].as_f64().unwrap_or(0.0);
            let (lo, hi) = (c["lift_lo"].as_f64().unwrap_or(f64::NAN), c["lift_hi"].as_f64().unwrap_or(f64::NAN));
            let held = if m["higher"].as_bool().unwrap_or(true) { lo > -margin } else { hi < margin };
            if !moved && !held {
                return Err(format!(
                    "the guardrail {} for {}: neither moved nor held within its {:.0}% margin yet",
                    m["title"].as_str().unwrap_or(""),
                    name(a),
                    margin * 100.0
                ));
            }
            read.push(json!({"arm": a, "metric": m["key"], "moved": moved, "held": held && !moved}));
        }
    }
    Ok(read)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ndcg_is_one_for_the_best_order_and_less_otherwise() {
        let g: HashMap<String, u8> = [("a".to_string(), 2u8), ("b".to_string(), 1), ("c".to_string(), 0)].into_iter().collect();
        let k = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!((ndcg(&g, 2, &k(&["a", "b", "c"]), 3) - 1.0).abs() < 1e-12);
        let worse = ndcg(&g, 2, &k(&["b", "a", "c"]), 3);
        assert!(worse < 1.0 && worse > 0.0);
        assert_eq!(ndcg(&g, 2, &k(&["x", "y"]), 3), 0.0);
    }

    #[test]
    fn a_run_is_decided_once_the_call_is_made_and_the_guardrails_read() {
        let snap = |p_av: f64, lat_hi: f64, lat_p: f64| {
            json!({
                "verdicts": [{"arm": "B", "call": "worse"}],
                "metrics": [
                    {"key": "success", "title": "Success rate", "guardrail": false, "comparisons": [{"p_always_valid": p_av}]},
                    {"key": "abandonment", "title": "Abandonment", "guardrail": true, "higher": false, "margin": 0.01, "comparisons": [{"p_always_valid": 0.001, "lift_lo": 0.2, "lift_hi": 0.4}]},
                    {"key": "latency", "title": "Query time", "guardrail": true, "higher": false, "margin": 0.1, "comparisons": [{"p_always_valid": lat_p, "lift_lo": -0.05, "lift_hi": lat_hi}]},
                ],
            })
        };
        let read = decision(&snap(0.001, 0.04, 0.6), "success", 2, 1500).unwrap();
        assert_eq!(read, vec![json!({"arm": 1, "metric": "abandonment", "moved": true, "held": false}), json!({"arm": 1, "metric": "latency", "moved": false, "held": true})]);
        assert!(decision(&snap(0.001, 0.04, 0.6), "success", 2, 999).unwrap_err().contains("1,000 searches"));
        assert!(decision(&snap(0.2, 0.04, 0.6), "success", 2, 1500).unwrap_err().starts_with("Success rate for B"));
        // A query time that may still be 10% slower, and has not moved.
        let w = decision(&snap(0.001, 0.15, 0.6), "success", 2, 1500).unwrap_err();
        assert_eq!(w, "the guardrail Query time for B: neither moved nor held within its 10% margin yet");
        // Or that has moved: read.
        assert!(decision(&snap(0.001, 0.15, 0.01), "success", 2, 1500).is_ok());
    }

    #[test]
    fn specs_are_checked() {
        let spec: LiveSpec = serde_json::from_value(json!({"name": "x", "site": "s", "arms": [{"name": "A", "rank": "r"}, {"name": "B", "rank": "q"}]})).unwrap();
        spec.check().unwrap();
        assert_eq!(spec.brain, "claude");
        assert_eq!(spec.agent_model, "claude-opus-5-5");
        let mut s = spec.clone();
        s.rate = 0.0;
        assert!(s.check().is_err(), "no users and no agents");
        s.agents = 2;
        s.check().unwrap();
        s.agent_model = "gpt".into();
        assert!(s.check().is_err());
        s.brain = "scripted".into();
        s.check().unwrap();
        let mut s = spec;
        s.rate = 1e6;
        assert!(s.check().is_err());
    }

    #[test]
    fn the_scripted_mind_searches_opens_and_stops() {
        let b = Brain::Scripted;
        let http = reqwest::Client::new();
        let mut m = Mind::new(&b, &http, "someone".into(), "a site", "Do statins cause breast cancer?");
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let (a, cost) = rt.block_on(m.act(None, &[])).unwrap();
        assert_eq!(cost, 0.0);
        assert_eq!(a, Action::Search { query: "statins cause breast cancer".into(), why: "the need's own words".into() });
        let mut rows = vec![
            Row { key: "1".into(), title: "Vitamin D".into(), snippet: "sun".into(), grade: Some(0), opened: false },
            Row { key: "2".into(), title: "Statins and breast cancer risk".into(), snippet: "".into(), grade: Some(2), opened: false },
        ];
        let (a, _) = rt.block_on(m.act(Some(String::new()), &rows)).unwrap();
        assert_eq!(a, Action::Open { result: 2, why: "its title shares the most words with what I need".into() });
        rows[1].opened = true;
        let (a, _) = rt.block_on(m.act(Some(String::new()), &rows)).unwrap();
        assert!(matches!(a, Action::Done { found: true, result: 2, .. }), "{a:?}");
    }

    #[test]
    fn personas_read_as_people() {
        let pop = Population::new(50, 3, Shape::default());
        for u in &pop.users {
            let t = persona_text(u);
            assert!(t.starts_with("a ") && t.contains("aged") && t.contains("living in"), "{t}");
        }
        assert!(num(1.23456789).as_f64().unwrap() == 1.23457);
        assert!(num(f64::NAN).is_null());
    }
}
