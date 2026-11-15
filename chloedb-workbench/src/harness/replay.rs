//! Replay A against B (HARNESS.md, "Reproduce and replay"), and Optimize
//! this: a stretch of a session played again under a harness version, many
//! times, and scored.
//!
//! - **The sandbox.** Each replay gets a clone of the repository at the
//!   case's commit (`git clone --shared --no-checkout`, its remote removed,
//!   so nothing can be pushed anywhere), with the case's uncommitted changes
//!   put back as uncommitted. Its agent runs there with the version's
//!   permissions, hooks and memory settings, and the edit and test defaults
//!   of a worktree. Its memory is a ChloeCtx store of its own, empty, with
//!   the distiller off, so a replay never writes the user's store and does
//!   not know what the user learned after the session. A case that starts
//!   after turn 1 resumes a copy of the conversation before it.
//! - **The user.** Scripted from the log: the prompts at the same points
//!   (the first at the start, each next one when the agent is idle), each
//!   permission prompt answered as the user answered the same command in
//!   the stretch, else as they answered such commands in the last 30 days,
//!   else no; each question answered as in the stretch when its words
//!   match. Whatever was not in the log is answered by precedent, counted
//!   as an intervention and listed. Never by a model playing the user.
//! - **The budget.** A run has a cap ($8 by default) and each replay one of
//!   its own ($0.60), read from the transcript's usage as it goes; a replay
//!   over either is stopped and counts as failed. A replay has a time limit
//!   too, from the original's wall time.
//! - **The score.** The case's checks run in the sandbox after the replay,
//!   then the scorecard (src/harness/score.rs): over cases for a suite,
//!   over runs for one stretch.
//! - **Optimize this.** On one stretch: A's runs first, then one change at
//!   a time (an allow rule for a command its replays were asked about, or a
//!   pin the week's misses call for), each on top of the best so far, kept
//!   only by the keep rule. It stops when every target the user ticked is
//!   met, when the budget is spent, when the changes run out, or when the
//!   user says Enough. A change that widens what agents may do waits for
//!   the user before it is tried. What it keeps is a candidate version: it
//!   is promoted, as any version, by the user.
//!
//! One run plays at a time, one replay at a time, in the order they were
//! started.

use std::collections::HashSet;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{Path as AxPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json as AxJson, Router};
use chloedb::query::Params;
use chloedb::Value;
use serde::Deserialize;
use serde_json::{json, Value as Json};

use super::evals::{self, ov, put_row, Case};
use super::score::{self, CaseRuns, RunM, Target};
use super::{edits_of, hash, sv, user_only, Edits, Version};
use crate::db::{get_int, get_ms, get_str, iso, now_ms, ts};
use crate::levels::Levels;
use crate::questions::{AnswerBody, Question};
use crate::server::{blocking, parse_body, ApiResult, App};
use crate::supervisor::{one_line, ApiError, SpawnRequest, Supervisor};

pub const TABLES: [&str; 2] = [
    "CREATE TABLE IF NOT EXISTS optimize_runs (id KEYWORD PRIMARY KEY, repo KEYWORD, kind KEYWORD, a KEYWORD, b KEYWORD, cases TEXT, n INT, rounds INT, cap_usd FLOAT, cap_run_usd FLOAT, est_usd FLOAT, spent_usd FLOAT, state KEYWORD, why TEXT, targets TEXT, log TEXT OPTIONS(indexed = false), best KEYWORD, approved TEXT, waiting TEXT OPTIONS(indexed = false), made_by KEYWORD, created TIMESTAMP, started TIMESTAMP, ended TIMESTAMP)",
    "CREATE TABLE IF NOT EXISTS eval_runs (id KEYWORD PRIMARY KEY, run KEYWORD, \"case\" KEYWORD, side KEYWORD, version KEYWORD, k INT, agent KEYWORD, state KEYWORD, passed BOOL, diverged BOOL, why TEXT, checks TEXT OPTIONS(indexed = false), not_in_log TEXT OPTIONS(indexed = false), interventions INT, prompts INT, turns INT, wall_ms INT, tokens INT, cost_usd FLOAT, started TIMESTAMP, ended TIMESTAMP, wait_ms INT, model KEYWORD)",
];

/// Runs a side, by default, and the most.
const DEFAULT_N: usize = 3;
const MAX_N: usize = 10;
/// The budget of a run, and of one replay, in dollars.
pub const CAP_USD: f64 = 8.0;
pub const CAP_RUN_USD: f64 = 0.60;
/// What a replay of a case with no cost recorded is estimated at.
const UNKNOWN_COST: f64 = 0.10;
/// Changes Optimize this tries, by default and at most.
const DEFAULT_ROUNDS: usize = 6;
const MAX_ROUNDS: usize = 12;
/// A replay's time limit: three times the original's, within these.
const MIN_LIMIT_MS: i64 = 10 * 60_000;
const MAX_LIMIT_MS: i64 = 60 * 60_000;
/// Waiting this long with nothing Workbench can read to answer: stuck.
const STUCK_MS: i64 = 120_000;
/// How long a turn has been idle before the next prompt is played.
const IDLE_MS: i64 = 1_500;
/// A turn ends with a Stop or, after a No or an Esc (Claude Code runs no
/// more hooks then), with "Interrupted" on its screen; one that shows
/// neither is over when nothing happened for this long.
const QUIET_MS: i64 = 20_000;
/// A check's time limit.
const CHECK_LIMIT: Duration = Duration::from_secs(300);
/// How far back a precedent for a permission prompt is looked for.
const PRECEDENT_MS: i64 = 30 * 86_400_000;

/// What the supervisor needs to start a replay's agent (SpawnRequest).
#[derive(Clone, Debug, Default)]
pub struct ReplayTag {
    /// The repository whose harness and rules it plays under.
    pub of_repo: String,
    /// The config version it plays.
    pub config: String,
    /// Its process's environment: its own ChloeCtx store.
    pub env: Vec<(String, String)>,
    /// The conversation it continues (`--resume`), for a case that starts
    /// after turn 1.
    pub resume: Option<String>,
    /// What its agent's row says it is: its run, case, side and number.
    pub tag: Json,
}

fn fv(v: &Value) -> f64 {
    match v {
        Value::Float(f) => *f,
        Value::Int(i) => *i as f64,
        _ => 0.0,
    }
}

fn money(x: f64) -> f64 {
    (x * 10_000.0).round() / 10_000.0
}

// ----------------------------------------------------------------- rows

/// An optimize run's row: `compare` (A against B on cases) or `optimize`
/// (Optimize this, on one stretch).
#[derive(Clone, Debug, Default)]
pub struct Run {
    pub id: String,
    pub repo: String,
    pub kind: String,
    pub a: String,
    pub b: Option<String>,
    pub cases: Vec<String>,
    pub n: usize,
    pub rounds: usize,
    pub cap_usd: f64,
    pub cap_run_usd: f64,
    pub est_usd: f64,
    pub spent_usd: f64,
    /// `queued`, `running`, `needs_you`, `done`, `stopped` or `failed`.
    pub state: String,
    pub why: Option<String>,
    pub targets: Vec<Target>,
    /// Optimize this's rounds: each change tried and what came of it.
    pub log: Vec<Json>,
    /// The best version so far (Optimize this).
    pub best: Option<String>,
    /// The widening patterns the user approved trying.
    pub approved: Vec<String>,
    /// The change that waits for the user (`needs_you`).
    pub waiting: Option<Json>,
    pub made_by: String,
    pub created: i64,
    pub started: Option<i64>,
    pub ended: Option<i64>,
}

const R_COLS: &str = "id, repo, kind, a, b, cases, n, rounds, cap_usd, cap_run_usd, est_usd, spent_usd, state, why, targets, log, best, approved, waiting, made_by, created, started, ended";

impl Run {
    fn of(r: &[Value]) -> Run {
        let js = |v: &Value| get_str(v).and_then(|s| serde_json::from_str::<Json>(&s).ok());
        Run {
            id: get_str(&r[0]).unwrap_or_default(),
            repo: get_str(&r[1]).unwrap_or_default(),
            kind: get_str(&r[2]).unwrap_or_default(),
            a: get_str(&r[3]).unwrap_or_default(),
            b: get_str(&r[4]),
            cases: js(&r[5]).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default(),
            n: get_int(&r[6]).unwrap_or(DEFAULT_N as i64).max(1) as usize,
            rounds: get_int(&r[7]).unwrap_or(DEFAULT_ROUNDS as i64).max(0) as usize,
            cap_usd: fv(&r[8]),
            cap_run_usd: fv(&r[9]),
            est_usd: fv(&r[10]),
            spent_usd: fv(&r[11]),
            state: get_str(&r[12]).unwrap_or_default(),
            why: get_str(&r[13]),
            targets: js(&r[14]).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default(),
            log: js(&r[15]).and_then(|v| v.as_array().cloned()).unwrap_or_default(),
            best: get_str(&r[16]),
            approved: js(&r[17]).and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default(),
            waiting: js(&r[18]).filter(|v| !v.is_null()),
            made_by: get_str(&r[19]).unwrap_or_default(),
            created: get_ms(&r[20]).unwrap_or(0),
            started: get_ms(&r[21]),
            ended: get_ms(&r[22]),
        }
    }

    fn put(&self, sup: &Supervisor) -> Result<(), ApiError> {
        put_row(
            sup,
            "optimize_runs",
            &[
                ("id", sv(&self.id)),
                ("repo", sv(&self.repo)),
                ("kind", sv(&self.kind)),
                ("a", sv(&self.a)),
                ("b", ov(&self.b)),
                ("cases", sv(&json!(self.cases).to_string())),
                ("n", Value::Int(self.n as i64)),
                ("rounds", Value::Int(self.rounds as i64)),
                ("cap_usd", Value::Float(self.cap_usd)),
                ("cap_run_usd", Value::Float(self.cap_run_usd)),
                ("est_usd", Value::Float(self.est_usd)),
                ("spent_usd", Value::Float(money(self.spent_usd))),
                ("state", sv(&self.state)),
                ("why", ov(&self.why)),
                ("targets", sv(&json!(self.targets).to_string())),
                ("log", sv(&Json::Array(self.log.clone()).to_string())),
                ("best", ov(&self.best)),
                ("approved", sv(&json!(self.approved).to_string())),
                ("waiting", self.waiting.as_ref().map_or(Value::Null, |w| sv(&w.to_string()))),
                ("made_by", sv(&self.made_by)),
                ("created", ts(self.created)),
                ("started", self.started.map_or(Value::Null, ts)),
                ("ended", self.ended.map_or(Value::Null, ts)),
            ],
        )
    }

    fn to_json(&self, sup: &Supervisor) -> Json {
        let name = |id: &str| super::version(sup, id).map(|v| v.name).unwrap_or_else(|| super::short(id));
        json!({
            "id": self.id, "repo": self.repo, "kind": self.kind,
            "a": { "id": self.a, "name": name(&self.a) }, "b": self.b.as_deref().map(|b| json!({ "id": b, "name": name(b) })),
            "cases": self.cases, "n": self.n, "rounds": self.rounds, "cap_usd": self.cap_usd, "cap_run_usd": self.cap_run_usd,
            "est_usd": money(self.est_usd), "spent_usd": money(self.spent_usd), "state": self.state, "why": self.why,
            "targets": self.targets, "log": self.log, "best": self.best.as_deref().map(|b| json!({ "id": b, "name": name(b) })),
            "approved": self.approved, "waiting": self.waiting, "made_by": self.made_by,
            "created": iso(self.created), "started": self.started.map(iso), "ended": self.ended.map(iso),
            "live": live().lock().unwrap().active.as_deref() == Some(self.id.as_str()),
        })
    }
}

pub fn run_of(sup: &Supervisor, id: &str) -> Option<Run> {
    let rows = sup.db().query(&format!("SELECT {R_COLS} FROM optimize_runs WHERE id = :id LIMIT 1"), &Params::new().set("id", sv(id))).ok()?;
    rows.first().map(|r| Run::of(r))
}

fn runs_in(sup: &Supervisor, where_: &str, p: &Params) -> Vec<Run> {
    let rows = sup.db().query(&format!("SELECT {R_COLS} FROM optimize_runs WHERE {where_} ORDER BY created ASC LIMIT 10000"), p).unwrap_or_default();
    rows.iter().map(|r| Run::of(r)).collect()
}

/// One replay's row.
#[derive(Clone, Debug, Default)]
pub struct Res {
    pub id: String,
    pub run: String,
    pub case: String,
    pub side: String,
    pub version: String,
    pub k: i64,
    pub agent: Option<String>,
    /// `running`, `passed`, `failed`, `stopped` or `error` (it could not be
    /// played: not counted).
    pub state: String,
    pub passed: bool,
    pub diverged: bool,
    pub why: Option<String>,
    pub checks: Json,
    pub not_in_log: Json,
    pub interventions: i64,
    pub prompts: i64,
    pub turns: i64,
    /// Its wall time, your answers included (`wait_ms` of it).
    pub wall_ms: i64,
    pub tokens: i64,
    pub cost_usd: f64,
    pub started: i64,
    pub ended: Option<i64>,
    /// The time its interventions would have taken you, at the pace you
    /// answered in the session (the scripted user answers at once).
    pub wait_ms: i64,
    /// The model it ran.
    pub model: Option<String>,
}

const E_COLS: &str = "id, run, \"case\", side, version, k, agent, state, passed, diverged, why, checks, not_in_log, interventions, prompts, turns, wall_ms, tokens, cost_usd, started, ended, wait_ms, model";

impl Res {
    fn of(r: &[Value]) -> Res {
        let js = |v: &Value| get_str(v).and_then(|s| serde_json::from_str::<Json>(&s).ok()).unwrap_or(json!([]));
        Res {
            id: get_str(&r[0]).unwrap_or_default(),
            run: get_str(&r[1]).unwrap_or_default(),
            case: get_str(&r[2]).unwrap_or_default(),
            side: get_str(&r[3]).unwrap_or_default(),
            version: get_str(&r[4]).unwrap_or_default(),
            k: get_int(&r[5]).unwrap_or(0),
            agent: get_str(&r[6]),
            state: get_str(&r[7]).unwrap_or_default(),
            passed: matches!(r[8], Value::Bool(true)),
            diverged: matches!(r[9], Value::Bool(true)),
            why: get_str(&r[10]),
            checks: js(&r[11]),
            not_in_log: js(&r[12]),
            interventions: get_int(&r[13]).unwrap_or(0),
            prompts: get_int(&r[14]).unwrap_or(0),
            turns: get_int(&r[15]).unwrap_or(0),
            wall_ms: get_int(&r[16]).unwrap_or(0),
            tokens: get_int(&r[17]).unwrap_or(0),
            cost_usd: fv(&r[18]),
            started: get_ms(&r[19]).unwrap_or(0),
            ended: get_ms(&r[20]),
            wait_ms: get_int(&r[21]).unwrap_or(0),
            model: get_str(&r[22]),
        }
    }

    fn put(&self, sup: &Supervisor) -> Result<(), ApiError> {
        put_row(
            sup,
            "eval_runs",
            &[
                ("id", sv(&self.id)),
                ("run", sv(&self.run)),
                ("case", sv(&self.case)),
                ("side", sv(&self.side)),
                ("version", sv(&self.version)),
                ("k", Value::Int(self.k)),
                ("agent", ov(&self.agent)),
                ("state", sv(&self.state)),
                ("passed", Value::Bool(self.passed)),
                ("diverged", Value::Bool(self.diverged)),
                ("why", ov(&self.why)),
                ("checks", sv(&self.checks.to_string())),
                ("not_in_log", sv(&self.not_in_log.to_string())),
                ("interventions", Value::Int(self.interventions)),
                ("prompts", Value::Int(self.prompts)),
                ("turns", Value::Int(self.turns)),
                ("wall_ms", Value::Int(self.wall_ms)),
                ("tokens", Value::Int(self.tokens)),
                ("cost_usd", Value::Float(money(self.cost_usd))),
                ("started", ts(self.started)),
                ("ended", self.ended.map_or(Value::Null, ts)),
                ("wait_ms", Value::Int(self.wait_ms)),
                ("model", ov(&self.model)),
            ],
        )
    }

    /// Counted in the scorecard: played to an end that says something.
    fn counts(&self) -> bool {
        matches!(self.state.as_str(), "passed" | "failed")
    }

    fn m(&self) -> RunM {
        RunM { passed: self.passed, diverged: self.diverged, m: [self.interventions as f64, self.prompts as f64, self.turns as f64, self.wall_ms as f64 / 1000.0, self.tokens as f64, self.cost_usd] }
    }

    fn to_json(&self) -> Json {
        json!({
            "id": self.id, "case": self.case, "side": self.side, "version": self.version, "k": self.k, "agent": self.agent, "state": self.state,
            "passed": self.passed, "diverged": self.diverged, "why": self.why, "checks": self.checks, "not_in_log": self.not_in_log,
            "interventions": self.interventions, "prompts": self.prompts, "turns": self.turns, "wall_s": self.wall_ms as f64 / 1000.0,
            "wait_s": self.wait_ms as f64 / 1000.0, "model": self.model, "tokens": self.tokens, "cost_usd": money(self.cost_usd), "started": iso(self.started), "ended": self.ended.map(iso),
        })
    }
}

fn results(sup: &Supervisor, run: &str) -> Vec<Res> {
    let rows = sup.db().query(&format!("SELECT {E_COLS} FROM eval_runs WHERE run = :run ORDER BY started ASC LIMIT 100000"), &Params::new().set("run", sv(run))).unwrap_or_default();
    rows.iter().map(|r| Res::of(r)).collect()
}

/// Each case's latest result under each version it ran, for the Evals
/// page: `{case: [{version, name, runs, passed, pass_k, run, when}]}`.
pub fn results_by_case(sup: &Supervisor, repo: &str) -> std::collections::HashMap<String, Json> {
    let runs = runs_in(sup, "repo = :repo", &Params::new().set("repo", sv(repo)));
    let mut by: std::collections::BTreeMap<(String, String), (String, i64, usize, usize)> = std::collections::BTreeMap::new();
    for r in &runs {
        for x in results(sup, &r.id).into_iter().filter(Res::counts) {
            let e = by.entry((x.case.clone(), x.version.clone())).or_insert((r.id.clone(), 0, 0, 0));
            if e.0 != r.id && r.created < e.1 {
                continue;
            }
            if e.0 != r.id {
                *e = (r.id.clone(), r.created, 0, 0);
            }
            e.1 = r.created;
            e.2 += 1;
            e.3 += usize::from(x.passed);
        }
    }
    let mut out: std::collections::HashMap<String, Vec<Json>> = std::collections::HashMap::new();
    for ((case, version), (run, when, n, passed)) in by {
        let name = super::version(sup, &version).map(|v| v.name).unwrap_or_else(|| super::short(&version));
        out.entry(case).or_default().push(json!({ "version": version, "name": name, "runs": n, "passed": passed, "pass_k": n > 0 && passed == n, "run": run, "when": iso(when) }));
    }
    out.into_iter().map(|(k, v)| (k, Json::Array(v))).collect()
}

// ------------------------------------------------------------- the runner

#[derive(Default)]
struct Live {
    /// The run playing now.
    active: Option<String>,
    /// Runs the user stopped (Enough).
    stop: HashSet<String>,
    /// The replay playing now: its agent.
    agent: Option<String>,
}

fn live() -> &'static Mutex<Live> {
    static L: OnceLock<Mutex<Live>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(Live::default()))
}

fn stopping(run: &str) -> bool {
    live().lock().unwrap().stop.contains(run)
}

/// Starts the runner unless it runs: it plays the queued runs, oldest
/// first, one at a time.
fn kick(sup: &Arc<Supervisor>, lv: &Arc<Levels>) {
    {
        let mut l = live().lock().unwrap();
        if l.active.is_some() {
            return;
        }
        l.active = Some(String::new());
    }
    // Runs this process did not finish (Workbench stopped while they ran).
    for mut r in runs_in(sup, "state = 'running'", &Params::new()) {
        r.state = "stopped".into();
        r.why = Some("Workbench stopped while it ran".into());
        r.ended = Some(now_ms());
        let _ = r.put(sup);
    }
    let (sup, lv) = (sup.clone(), lv.clone());
    let _ = std::thread::Builder::new().name("harness-runs".into()).spawn(move || loop {
        let next = runs_in(&sup, "state = 'queued'", &Params::new()).into_iter().next();
        let Some(mut run) = next else {
            live().lock().unwrap().active = None;
            return;
        };
        live().lock().unwrap().active = Some(run.id.clone());
        run.state = "running".into();
        run.started.get_or_insert(now_ms());
        let _ = run.put(&sup);
        let out = if run.kind == "optimize" { optimize(&sup, &lv, &mut run) } else { compare(&sup, &lv, &mut run) };
        if let Some(fresh) = run_of(&sup, &run.id) {
            run.spent_usd = fresh.spent_usd.max(run.spent_usd);
        }
        match out {
            Ok(Some(why)) => {
                run.state = if stopping(&run.id) { "stopped".into() } else { "done".into() };
                run.why = Some(why);
                run.ended = Some(now_ms());
            }
            // Waits for the user (needs_you), and is queued again by them.
            Ok(None) => {}
            Err(e) => {
                run.state = if stopping(&run.id) { "stopped".into() } else { "failed".into() };
                run.why = Some(e);
                run.ended = Some(now_ms());
            }
        }
        live().lock().unwrap().stop.remove(&run.id);
        let _ = run.put(&sup);
    });
}

/// The cost of one replay of a case, estimated from its original.
fn est_one(c: &Case) -> f64 {
    if c.cost_usd > 0.0 {
        c.cost_usd
    } else {
        UNKNOWN_COST
    }
}

/// Adds a replay's cost to its run; answers the run's total.
fn spend(sup: &Supervisor, run: &str, usd: f64) -> f64 {
    let Some(mut r) = run_of(sup, run) else { return usd };
    r.spent_usd += usd;
    let _ = r.put(sup);
    r.spent_usd
}

/// A versus B on every case: run k of A then of B, the order turning each
/// time, so that a slow hour lands on neither.
fn compare(sup: &Arc<Supervisor>, lv: &Arc<Levels>, run: &mut Run) -> Result<Option<String>, String> {
    let b = run.b.clone().ok_or("a comparison needs its B")?;
    let cases: Vec<Case> = run.cases.iter().filter_map(|c| evals::case(sup, c)).collect();
    if cases.is_empty() {
        return Err("its cases were deleted".into());
    }
    for k in 0..run.n {
        for c in &cases {
            let sides = if k % 2 == 0 { [("a", run.a.clone()), ("b", b.clone())] } else { [("b", b.clone()), ("a", run.a.clone())] };
            for (side, v) in sides {
                ensure(sup, lv, run, c, &v, side, k + 1)?;
            }
        }
    }
    Ok(Some(format!("{} replay{} of {} case{}", results(sup, &run.id).iter().filter(|r| r.counts()).count(), if run.n * cases.len() * 2 == 1 { "" } else { "s" }, cases.len(), if cases.len() == 1 { "" } else { "s" })))
}

/// Plays a version on a case, as a side, until that side has `want`
/// counted runs (an error is tried again, twice); by side, so that A
/// against itself plays both. Stops on Enough and on the budget.
fn ensure(sup: &Arc<Supervisor>, lv: &Arc<Levels>, run: &Run, c: &Case, version: &str, side: &str, want: usize) -> Result<(), String> {
    let mut errors = 0;
    loop {
        let mine: Vec<Res> = results(sup, &run.id).into_iter().filter(|r| r.case == c.id && r.version == version && r.side == side).collect();
        if mine.iter().filter(|r| r.counts()).count() >= want {
            return Ok(());
        }
        if stopping(&run.id) {
            return Err("you stopped it".into());
        }
        let spent = run_of(sup, &run.id).map_or(0.0, |r| r.spent_usd);
        if spent + est_one(c).min(run.cap_run_usd) > run.cap_usd + 1e-9 {
            return Err(format!("its budget of ${:.2} is spent (${spent:.2})", run.cap_usd));
        }
        let r = play(sup, lv, run, c, version, side, mine.len() as i64 + 1);
        spend(sup, &run.id, r.cost_usd);
        match r.state.as_str() {
            "stopped" => return Err("you stopped it".into()),
            "error" => {
                errors += 1;
                if errors >= 3 {
                    return Err(format!("{} could not be played: {}", c.name, r.why.unwrap_or_default()));
                }
            }
            _ => {}
        }
        if r.why.as_deref().is_some_and(|w| w.contains("budget of the run")) {
            return Err(format!("its budget of ${:.2} is spent", run.cap_usd));
        }
    }
}

// --------------------------------------------------------------- one replay

/// ChloeCtx's models folder, for a replay's own store.
fn models_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("CHLOECTX_MODELS").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d));
    }
    for k in ["CHLOECTX_HOME", "ENGRAM_HOME"] {
        if let Some(h) = std::env::var_os(k).filter(|d| !d.is_empty()) {
            let p = PathBuf::from(h).join("models");
            if p.is_dir() {
                return Some(p);
            }
        }
    }
    let home = PathBuf::from(std::env::var_os("HOME")?);
    ["chloectx", "engram"].iter().map(|n| home.join(".local/share").join(n).join("models")).find(|p| p.is_dir())
}

/// Stops the ChloeCtx daemons of a replay's store (each listens at its
/// store's `engram.sock`, or at a short path for a long one).
fn stop_daemons(home: &Path) {
    let Ok(rd) = std::fs::read_dir(home.join("projects")) else { return };
    for e in rd.flatten() {
        let dir = e.path();
        let name = format!("engram-{:016x}.sock", chloedb::embed::fnv(&dir.to_string_lossy()));
        let uid = unsafe { libc::getuid() };
        let mut socks = vec![dir.join("engram.sock"), PathBuf::from(format!("/tmp/engram-{uid}")).join(&name)];
        if let Some(r) = std::env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty()) {
            socks.push(PathBuf::from(r).join(&name));
        }
        if let Some(h) = std::env::var_os("HOME").filter(|d| !d.is_empty()) {
            socks.push(PathBuf::from(h).join(".cache/chloectx").join(&name));
        }
        for s in socks.iter().filter(|s| s.exists()) {
            if let Ok(mut c) = std::os::unix::net::UnixStream::connect(s) {
                let _ = c.set_write_timeout(Some(Duration::from_secs(1)));
                let _ = c.write_all(b"{\"op\":\"shutdown\"}\n");
            }
        }
    }
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let o = std::process::Command::new("git").arg("-C").arg(dir).args(args).env("GIT_TERMINAL_PROMPT", "0").output().map_err(|e| e.to_string())?;
    if o.status.success() {
        Ok(String::from_utf8_lossy(&o.stdout).trim().to_string())
    } else {
        Err(one_line(String::from_utf8_lossy(&o.stderr).trim(), 300))
    }
}

/// A clone of the case's repository at its start, with no remote.
fn sandbox(c: &Case, dir: &Path) -> Result<(), String> {
    let commit = c.commit.as_deref().ok_or("the case has no commit to start from")?;
    std::fs::create_dir_all(dir.parent().unwrap_or(dir)).map_err(|e| e.to_string())?;
    let o = std::process::Command::new("git").args(["clone", "--shared", "--no-checkout", "-q"]).arg(&c.repo).arg(dir).env("GIT_TERMINAL_PROMPT", "0").output().map_err(|e| e.to_string())?;
    if !o.status.success() {
        return Err(format!("git clone: {}", one_line(String::from_utf8_lossy(&o.stderr).trim(), 300)));
    }
    git(dir, &["remote", "remove", "origin"])?;
    git(dir, &["-c", "advice.detachedHead=false", "checkout", "-q", "--detach", commit])?;
    if let Some(tree) = &c.tree {
        // Its uncommitted changes, uncommitted again (new files untracked).
        git(dir, &["read-tree", "-u", "--reset", tree])?;
        git(dir, &["reset", "-q"])?;
    }
    if !git(dir, &["remote"])?.is_empty() {
        return Err("the sandbox still has a remote".into());
    }
    Ok(())
}

/// Claude Code's folder of a project's transcripts, for a path.
fn claude_slug(p: &Path) -> String {
    p.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

/// The conversation before a case, where Claude Code resumes it for the
/// sandbox: `(session id, the folder written)`.
fn place_before(sup: &Supervisor, c: &Case, sandbox: &Path) -> Option<(String, PathBuf)> {
    let text = std::fs::read_to_string(evals::before_file(sup, &c.id)).ok()?;
    // Claude Code's projects folder: where the original's transcript was.
    let orig = sup.transcript_of(c.agent.as_deref()?)?;
    let projects = orig.parent()?.parent()?;
    if projects.file_name()? != "projects" {
        return None;
    }
    let sid = crate::supervisor::uuid4();
    let dir = projects.join(claude_slug(&sandbox.canonicalize().unwrap_or(sandbox.to_path_buf())));
    std::fs::create_dir_all(&dir).ok()?;
    let lines: Vec<String> = text
        .lines()
        .map(|l| match serde_json::from_str::<Json>(l) {
            Ok(mut v) if v.is_object() => {
                if v.get("sessionId").is_some() {
                    v["sessionId"] = json!(sid);
                }
                if v.get("cwd").is_some() {
                    v["cwd"] = json!(sandbox.to_string_lossy());
                }
                v.to_string()
            }
            _ => l.to_string(),
        })
        .collect();
    std::fs::write(dir.join(format!("{sid}.jsonl")), lines.join("\n") + "\n").ok()?;
    Some((sid, dir))
}

/// The user's side of a case, as a replay plays it.
struct Script {
    prompts: Vec<String>,
    /// (what was asked, the answer's kind, used).
    perms: Vec<(String, String, bool)>,
    /// (question, answer, options).
    answers: Vec<(String, String)>,
}

impl Script {
    fn of(c: &Case) -> Script {
        let items = c.script.as_array().cloned().unwrap_or_default();
        Script {
            prompts: c.prompts(),
            perms: items.iter().filter(|x| x["kind"] == "permission").map(|x| (x["key"].as_str().unwrap_or("").to_string(), x["answer"].as_str().unwrap_or("no").to_string(), false)).collect(),
            answers: items.iter().filter(|x| x["kind"] == "answer").map(|x| (x["question"].as_str().unwrap_or("").to_string(), x["answer"].as_str().unwrap_or("").to_string())).collect(),
        }
    }

    /// The answer the stretch gave the same prompt: the next unused one,
    /// else the last.
    fn perm(&mut self, key: &str) -> Option<String> {
        if let Some(p) = self.perms.iter_mut().find(|p| p.0 == key && !p.2) {
            p.2 = true;
            return Some(p.1.clone());
        }
        self.perms.iter().rev().find(|p| p.0 == key).map(|p| p.1.clone())
    }

    /// The answer to a permission prompt for `key` (a Bash command's
    /// `cmd`): the stretch's to the same key, else, as a chain asks once
    /// for all its commands, its answers to the chain's commands (`grep -n
    /// x f; ./check.sh` was asked as ./check.sh), No if any was No.
    fn perm_of(&mut self, key: &str, cmd: Option<&str>) -> Option<String> {
        if let Some(k) = self.perm(key) {
            return Some(k);
        }
        let keys: Vec<String> = cmd.map(super::perms::parts).unwrap_or_default().iter().filter_map(|p| super::storm_key(p)).map(|k| format!("Bash: {k}")).filter(|k| k != key).collect();
        let found: Vec<String> = keys.iter().filter_map(|k| self.perm(k)).collect();
        found.iter().find(|k| *k == "no").or(found.first()).cloned()
    }
}

fn words(s: &str) -> HashSet<String> {
    s.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() > 2).map(str::to_string).collect()
}

/// How alike two questions are: their words in common.
fn alike(a: &str, b: &str) -> f64 {
    let (x, y) = (words(a), words(b));
    if x.is_empty() || y.is_empty() {
        return 0.0;
    }
    x.intersection(&y).count() as f64 / x.union(&y).count() as f64
}

/// How the user answered such a command lately, in their own sessions.
fn precedent(sup: &Supervisor, repo: &str, key: &str) -> Option<String> {
    let since = now_ms() - PRECEDENT_MS;
    let mut qs: Vec<Question> = crate::questions::all(sup)
        .into_iter()
        .filter(|q| q.kind == "permission" && q.state == "answered" && q.created >= since && q.repo.as_deref() == Some(repo) && q.answered_by.as_deref() == Some("you"))
        .filter(|q| q.agent.as_deref().is_some_and(|a| !sup.is_replay(a)))
        .filter(|q| q.detail.as_ref().is_some_and(|d| evals::perm_key(d) == key))
        .collect();
    qs.sort_by_key(|q| std::cmp::Reverse(q.created));
    qs.first().and_then(|q| q.answer.as_deref()).map(|a| evals::answer_kind(a).to_string())
}

/// The option of a permission prompt that gives an answer of this kind.
fn option_for(labels: &[String], kind: &str) -> Option<usize> {
    let yes = |l: &String| l.trim().to_lowercase().starts_with("yes");
    let plain = labels.iter().position(|l| yes(l) && crate::screen::grants_more(l).is_none());
    match kind {
        "yes_more" => labels.iter().position(|l| yes(l) && crate::screen::grants_more(l).is_some_and(|g| !g.contains("auto mode"))).or(plain),
        "yes" => plain,
        _ => labels.iter().position(|l| l.trim().to_lowercase().starts_with("no")),
    }
}

/// What a replay counted as it went.
#[derive(Default)]
struct Counts {
    interventions: i64,
    prompts: i64,
    not_in_log: Vec<Json>,
}

/// Answers one question of a replay as the script says, else by precedent.
fn answer_one(sup: &Arc<Supervisor>, lv: &Arc<Levels>, c: &Case, script: &mut Script, q: &Question, n: &mut Counts) {
    let labels = q.labels();
    if q.kind == "permission" {
        let d = q.detail.clone().unwrap_or(Json::Null);
        if d["menu"]["kind"] == "trust" {
            // Workbench answers the trust prompt of its own sandboxes.
            return;
        }
        if d["menu"]["kind"] == "offer" {
            // Claude Code's own offer (install a plugin): not now, and not
            // the user's intervention.
            let i = labels.iter().position(|l| l.to_lowercase().starts_with("no"));
            let body = AnswerBody { option: i, label: i.map(|i| labels[i].clone()), cancel: i.is_none().then_some(true), ..AnswerBody::default() };
            let _ = crate::questions::answer(sup, lv, &q.id, body);
            return;
        }
        let key = evals::perm_key(&d);
        let (kind, by) = match script.perm_of(&key, d["command"].as_str()) {
            Some(k) => (k, "the log"),
            None => match precedent(sup, &c.repo, &key) {
                Some(k) => (k, "precedent"),
                None => ("no".to_string(), "no precedent"),
            },
        };
        let i = option_for(&labels, &kind);
        let body = AnswerBody { option: i, label: i.map(|i| labels[i].clone()), cancel: i.is_none().then_some(true), ..AnswerBody::default() };
        let res = crate::questions::answer(sup, lv, &q.id, body);
        n.prompts += 1;
        n.interventions += 1;
        if by != "the log" || res.is_err() {
            n.not_in_log.push(json!({ "kind": "permission", "what": d["summary"].as_str().unwrap_or(&q.text), "key": key, "answer": i.map(|i| labels[i].clone()).unwrap_or_else(|| "Esc".into()), "by": by, "error": res.err().map(|e| e.message) }));
        }
        return;
    }
    // A question: the stretch's answer to the question most like it.
    let best = script.answers.iter().map(|(t, a)| (alike(t, &q.text), a.clone())).filter(|(s, _)| *s >= 0.35).max_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
    let precedent_opt = q.options.as_array().and_then(|o| o.iter().position(|x| x["matches_precedent"] == true));
    let (body, by, said) = match best {
        Some((_, a)) => match labels.iter().position(|l| l == &a) {
            Some(i) => (AnswerBody { option: Some(i), label: Some(a.clone()), ..AnswerBody::default() }, "the log", a),
            None => (AnswerBody { text: Some(a.clone()), ..AnswerBody::default() }, "the log", a),
        },
        None => match precedent_opt {
            Some(i) => (AnswerBody { option: Some(i), label: labels.get(i).cloned(), ..AnswerBody::default() }, "precedent", labels.get(i).cloned().unwrap_or_default()),
            None => {
                let t = "There is no answer to this in the session being replayed: decide as you would without one.".to_string();
                (AnswerBody { text: Some(t.clone()), ..AnswerBody::default() }, "no precedent", t)
            }
        },
    };
    let res = crate::questions::answer(sup, lv, &q.id, body);
    n.interventions += 1;
    if by != "the log" || res.is_err() {
        n.not_in_log.push(json!({ "kind": "question", "what": one_line(&q.text, 200), "answer": one_line(&said, 200), "by": by, "error": res.err().map(|e| e.message) }));
    }
}

/// A replay's tokens and dollars so far (its transcript's usage since it
/// started: a resumed conversation's earlier turns are not its cost), at
/// the price of the model its agent runs.
fn cost_so_far(sup: &Supervisor, agent: &str, since: i64) -> (i64, f64) {
    // Its transcript read up to now: Workbench's own reading runs every
    // 30 s, and a replay's last turns would be left out of its cost.
    crate::alerts::ingest_tokens_of(sup, Some(agent));
    let model = sup.agent_row(agent).and_then(|a| a.model);
    let p = Params::new().set("agent", sv(agent)).set("from", ts(since));
    let rows = sup.db().query("SELECT input, output, cache_read FROM token_use WHERE agent = :agent AND ts >= :from LIMIT 100000", &p).unwrap_or_default();
    let (mut i, mut o, mut c) = (0, 0, 0);
    for r in rows {
        i += get_int(&r[0]).unwrap_or(0);
        o += get_int(&r[1]).unwrap_or(0);
        c += get_int(&r[2]).unwrap_or(0);
    }
    (i + o, crate::routing::cost_usd(&crate::routing::model_of(model.as_deref()), i, o, c))
}

/// The case's checks, in the sandbox after the replay.
fn checks(c: &Case, dir: &Path, bash: &[String], counts: &Counts, turns: i64) -> (bool, Json) {
    let mut out = Vec::new();
    for k in c.checks.as_array().into_iter().flatten() {
        let kind = k["kind"].as_str().unwrap_or("");
        let (passed, detail) = match kind {
            "fail_to_pass" | "pass_to_pass" | "command" => {
                let words: Vec<String> = if kind == "command" {
                    vec!["sh".into(), "-c".into(), k["run"].as_str().unwrap_or("false").to_string()]
                } else {
                    k["run"].as_array().map(|a| a.iter().filter_map(|w| w.as_str().map(str::to_string)).collect()).unwrap_or_default()
                };
                if words.is_empty() {
                    (false, "nothing to run".to_string())
                } else {
                    match crate::testruns::run_limited(&words, &[], dir, CHECK_LIMIT) {
                        Ok((text, code)) => {
                            let want = if kind == "command" { k["expect"].as_i64().unwrap_or(0) } else { 0 };
                            let tail: Vec<&str> = text.trim().lines().rev().take(3).collect();
                            (code == want, format!("exit {code}{}", if tail.is_empty() { String::new() } else { format!(": {}", one_line(&tail.into_iter().rev().collect::<Vec<_>>().join(" / "), 240)) }))
                        }
                        Err(e) => (false, e),
                    }
                }
            }
            "limit" => {
                let max = k["max"].as_i64().unwrap_or(0);
                let v = match k["metric"].as_str() {
                    Some("prompts") => counts.prompts,
                    Some("turns") => turns,
                    _ => counts.interventions,
                };
                (v <= max, format!("{v} of at most {max}"))
            }
            "never" => {
                let pat = k["pattern"].as_str().unwrap_or("").to_lowercase();
                let hit = bash.iter().find(|b| b.to_lowercase().contains(&pat));
                (hit.is_none(), hit.map(|h| format!("ran {}", one_line(h, 200))).unwrap_or_else(|| "never ran".into()))
            }
            _ => (false, format!("{kind} is not a check this build runs")),
        };
        out.push(json!({ "id": k["id"], "kind": kind, "test": k["test"], "run": k["run"], "passed": passed, "detail": detail }));
    }
    (!out.is_empty() && out.iter().all(|x| x["passed"] == true), Json::Array(out))
}

/// Plays one replay of a case under a version, start to scored end.
fn play(sup: &Arc<Supervisor>, lv: &Arc<Levels>, run: &Run, c: &Case, version: &str, side: &str, k: i64) -> Res {
    let started = now_ms();
    let id = format!("e{}", &hash(&format!("{}:{}:{version}:{side}:{k}:{started}", run.id, c.id))[..12]);
    let mut res = Res { id: id.clone(), run: run.id.clone(), case: c.id.clone(), side: side.into(), version: version.into(), k, state: "running".into(), checks: json!([]), not_in_log: json!([]), started, ..Res::default() };
    let _ = res.put(sup);
    let root = sup.cfg.data_dir.join("replays").join(&run.id).join(&id);
    let dir = root.join("w");
    // Short, so that ChloeCtx's socket path fits (src/paths.rs there).
    let ctx = sup.cfg.data_dir.join("rx").join(&id[1..9]);
    let mut placed: Option<PathBuf> = None;
    let out = (|| -> Result<(), String> {
        sandbox(c, &dir)?;
        if let Some(s) = &c.setup {
            let (text, code) = crate::testruns::run_limited(&["sh".into(), "-c".into(), s.clone()], &[], &dir, CHECK_LIMIT)?;
            if code != 0 {
                return Err(format!("its setup failed (exit {code}): {}", one_line(text.trim(), 240)));
            }
        }
        std::fs::create_dir_all(&ctx).map_err(|e| e.to_string())?;
        let mut env = vec![("CHLOECTX_HOME".to_string(), ctx.to_string_lossy().to_string()), ("CHLOECTX_DISTILL".to_string(), "0".to_string())];
        if let Some(m) = models_dir() {
            env.push(("CHLOECTX_MODELS".into(), m.to_string_lossy().to_string()));
        }
        let resume = if c.context == "transcript" {
            place_before(sup, c, &dir).map(|(sid, d)| {
                placed = Some(d);
                sid
            })
        } else {
            None
        };
        let script = Script::of(c);
        let first = script.prompts.first().cloned().ok_or("the case has no prompt to play")?;
        let name = format!("rp-{}-{k}{side}", &run.id[1..7]);
        // The version's model and effort, else the session's.
        let how = edits_of(sup, version).agent;
        let req = SpawnRequest {
            repo: dir.to_string_lossy().to_string(),
            task: Some(first),
            name: Some(name),
            worktree: Some(false),
            model: how.model.clone().or_else(|| c.model.clone()),
            effort: how.effort.clone(),
            // The session's own mode: without it Claude Code's default
            // decides what is asked, and the replay measures another thing.
            permission_mode: c.permission_mode.clone(),
            replay: Some(ReplayTag { of_repo: c.repo.clone(), config: version.to_string(), env, resume, tag: json!({ "run": run.id, "case": c.id, "side": side, "k": k, "version": version, "result": id }) }),
            ..SpawnRequest::default()
        };
        let a = sup.spawn(req).map_err(|e| format!("cannot start its agent: {}", e.message))?;
        let agent = a["id"].as_str().unwrap_or("").to_string();
        res.agent = Some(agent.clone());
        let _ = res.put(sup);
        live().lock().unwrap().agent = Some(agent.clone());
        drive(sup, lv, run, c, script, &agent, &dir, &mut res);
        Ok(())
    })();
    if let Err(e) = out {
        res.state = "error".into();
        res.why = Some(e);
    }
    if let Some(a) = res.agent.clone() {
        let _ = sup.stop_with(&a, Some(true));
        for _ in 0..80 {
            if !sup.is_running(&a) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    live().lock().unwrap().agent = None;
    stop_daemons(&ctx);
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&ctx);
    if let Some(d) = placed {
        let _ = std::fs::remove_dir_all(d);
    }
    res.ended = Some(now_ms());
    let _ = res.put(sup);
    res
}

/// Plays the script into a replay's agent until it ends, then scores it.
fn drive(sup: &Arc<Supervisor>, lv: &Arc<Levels>, run: &Run, c: &Case, mut script: Script, agent: &str, dir: &Path, res: &mut Res) {
    let started = res.started;
    let limit = (c.wall_ms * 3 + 5 * 60_000).clamp(MIN_LIMIT_MS, MAX_LIMIT_MS);
    let spent_before = run_of(sup, &run.id).map_or(0.0, |r| r.spent_usd);
    let mut counts = Counts::default();
    let mut sent = 1usize;
    let mut answered: HashSet<String> = HashSet::new();
    let mut waiting_since: Option<i64> = None;
    let mut end: Result<(), String> = Ok(());
    loop {
        std::thread::sleep(Duration::from_millis(300));
        let now = now_ms();
        if stopping(&run.id) {
            res.state = "stopped".into();
            res.why = Some("you stopped the run".into());
            return finish_counts(sup, c, agent, res, counts, started);
        }
        if now - started > limit {
            end = Err(format!("it ran over its time limit of {} min", limit / 60_000));
            break;
        }
        let (_, cost) = cost_so_far(sup, agent, started);
        if cost > run.cap_run_usd {
            end = Err(format!("it cost over its cap of ${:.2}", run.cap_run_usd));
            break;
        }
        if spent_before + cost > run.cap_usd {
            end = Err(format!("the budget of the run (${:.2}) ran out", run.cap_usd));
            break;
        }
        let Some(a) = sup.agent_row(agent) else {
            end = Err("its agent is gone".into());
            break;
        };
        if !sup.is_running(agent) {
            if sent < script.prompts.len() {
                end = Err(format!("it exited after {sent} of {} prompts", script.prompts.len()));
            }
            break;
        }
        let open = crate::questions::open_of(sup, agent);
        for q in &open {
            if answered.insert(q.id.clone()) {
                answer_one(sup, lv, c, &mut script, q, &mut counts);
            }
        }
        // The turn of the last prompt played is over: a Stop after its
        // prompt arrived (idle alone is not enough: an agent that has just
        // been given a prompt reads as idle until it starts, and a prompt
        // played then joins the turn before it), or a long quiet.
        let evs = sup.all_events(agent).unwrap_or_default();
        let submits: Vec<i64> = evs.iter().filter(|e| e.kind == "UserPromptSubmit").map(|e| e.ts).collect();
        let ended = |e: &crate::db::Event| e.kind == "Stop" || (e.kind == "screen" && e.summary.as_deref().is_some_and(|s| s.starts_with("interrupted")));
        let over = submits.get(sent.saturating_sub(1)).is_some_and(|t| evs.iter().any(|e| e.ts >= *t && ended(e)) || evs.last().is_some_and(|l| now - l.ts >= QUIET_MS));
        if a.state == "idle" && now - a.state_since >= IDLE_MS && submits.len() >= sent && over {
            if sent < script.prompts.len() {
                if let Err(e) = sup.input(agent, &script.prompts[sent], true, None) {
                    end = Err(format!("cannot play prompt {}: {}", sent + 1, e.message));
                    break;
                }
                sent += 1;
            } else {
                break;
            }
        }
        if a.state == "waiting" && open.is_empty() {
            let since = *waiting_since.get_or_insert(now);
            if now - since > STUCK_MS {
                counts.interventions += 1;
                counts.not_in_log.push(json!({ "kind": "stuck", "what": a.waiting_for.clone().unwrap_or_else(|| "something on its screen".into()), "by": "nothing to answer" }));
                end = Err("it waited for something Workbench could not read or answer".into());
                break;
            }
        } else {
            waiting_since = None;
        }
    }
    // Ended: stopped, then checked in its sandbox.
    let _ = sup.stop_with(agent, Some(true));
    for _ in 0..80 {
        if !sup.is_running(agent) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let evs = sup.all_events(agent).unwrap_or_default();
    let bash: Vec<String> = evs
        .iter()
        .filter(|e| e.kind == "PreToolUse" && e.tool.as_deref() == Some("Bash"))
        .filter_map(|e| serde_json::from_str::<Json>(e.data.as_deref()?).ok()?["tool_input"]["command"].as_str().map(str::to_string))
        .collect();
    let turns = evs.iter().filter(|e| e.kind == "UserPromptSubmit").count() as i64;
    match end {
        Ok(()) => {
            let (passed, cj) = checks(c, dir, &bash, &counts, turns);
            res.passed = passed;
            res.checks = cj;
            res.state = if passed { "passed".into() } else { "failed".into() };
            if !passed {
                let failed: Vec<String> = res.checks.as_array().into_iter().flatten().filter(|x| x["passed"] != true).map(|x| x["test"].as_str().or(x["run"].as_str()).or(x["kind"].as_str()).unwrap_or("").to_string()).collect();
                res.why = Some(format!("failed: {}", one_line(&failed.join(", "), 300)));
            }
        }
        Err(e) => {
            res.state = "failed".into();
            res.why = Some(e);
        }
    }
    finish_counts(sup, c, agent, res, counts, started);
}

fn finish_counts(sup: &Supervisor, c: &Case, agent: &str, res: &mut Res, counts: Counts, started: i64) {
    let evs = sup.all_events(agent).unwrap_or_default();
    res.turns = evs.iter().filter(|e| e.kind == "UserPromptSubmit").count() as i64;
    let last = evs.iter().filter(|e| e.kind == "Stop").map(|e| e.ts).max().unwrap_or_else(now_ms);
    res.wall_ms = (last - started).max(0);
    let (tokens, cost) = cost_so_far(sup, agent, started);
    res.tokens = tokens;
    res.cost_usd = cost;
    res.model = sup.agent_row(agent).and_then(|a| a.model);
    // Your answers, at your pace in the session: the scripted user answers
    // at once, and a prompt costs you the time you take to answer it.
    res.wait_ms = counts.interventions * c.your_wait_ms().unwrap_or(0);
    res.wall_ms += res.wait_ms;
    res.interventions = counts.interventions;
    res.prompts = counts.prompts;
    res.diverged = !counts.not_in_log.is_empty();
    res.not_in_log = Json::Array(counts.not_in_log);
}

// ----------------------------------------------------------- Optimize this

/// The runs of a version on a case in a run, counted ones.
fn runs_of(sup: &Supervisor, run: &str, case: &str, version: &str) -> Vec<RunM> {
    results(sup, run).into_iter().filter(|r| r.case == case && r.version == version && r.counts()).map(|r| r.m()).collect()
}

fn card(sup: &Supervisor, run: &Run, c: &Case, a: &str, b: &str, targets: &[Target], finished: bool) -> Json {
    let cr = CaseRuns { case: c.id.clone(), name: c.name.clone(), held_out: c.set == "held_out", a: runs_of(sup, &run.id, &c.id, a), b: runs_of(sup, &run.id, &c.id, b) };
    score::stretch(&cr, run.n, finished, targets)
}

/// A version with `e`'s edits, made by the run, on top of `parent`.
fn version_with(sup: &Supervisor, run: &Run, parent: &Version, e: &Edits, note: &str, round: usize) -> Result<Version, String> {
    let id = format!("{}{}", &e.id()[..12], &hash(&run.repo)[..4]);
    if let Some(v) = super::version(sup, &id) {
        return Ok(v);
    }
    let v = Version {
        id: id.clone(),
        repo: run.repo.clone(),
        name: format!("{}-opt{round}", parent.name.trim_end_matches("-draft")),
        kind: "config".into(),
        parent: Some(parent.id.clone()),
        state: "tried".into(),
        made_by: format!("optimize:{}", run.id),
        note: Some(note.to_string()),
        created: now_ms(),
        ..Version::default()
    };
    v.put(sup).map_err(|e| e.message)?;
    for (k, t) in e.texts() {
        super::put_part(sup, &id, k, &t).map_err(|e| e.message)?;
    }
    Ok(v)
}

/// The changes Optimize this can try on top of `best`, those aimed at the
/// targets not met yet first: an allow rule for each command the best
/// version's replays were asked about (most asked first), a faster model,
/// less effort, instructions a model wrote from what the replays did, and
/// the pins the week's missed memory calls for. Each says what it `aims`
/// at.
fn changes(sup: &Supervisor, run: &Run, best: &Version, c: &Case) -> Vec<Json> {
    let e = edits_of(sup, &best.id);
    let mut out = permission_and_pin_changes(sup, run, best, &e);
    out.extend(how_changes(run, &e, c));
    out.extend(brief_changes(sup, run, best, &e, c));
    by_aims(out, &unmet_targets(sup, run, best, c))
}

/// Changes, those aimed at more of the unmet targets first; stable, so
/// that within as many the order they came in holds.
fn by_aims(mut out: Vec<Json>, unmet: &[String]) -> Vec<Json> {
    out.sort_by_key(|ch| std::cmp::Reverse(ch["aims"].as_array().into_iter().flatten().filter(|a| a.as_str().is_some_and(|a| unmet.iter().any(|u| u == a))).count()));
    out
}

/// The targets the best so far does not meet against A (all of them before
/// anything is kept), by what a change aims at.
fn unmet_targets(sup: &Supervisor, run: &Run, best: &Version, c: &Case) -> Vec<String> {
    let aim = |m: &str| if score::target_metric(m) && matches!(m, "pass_rate" | "success") { "pass_rate".to_string() } else { m.to_string() };
    if best.id == run.a {
        return run.targets.iter().map(|t| aim(&t.metric)).collect();
    }
    let vs_a = card(sup, run, c, &run.a, &best.id, &run.targets, true);
    vs_a["targets"].as_array().into_iter().flatten().filter(|t| t["met"] != true).filter_map(|t| t["metric"].as_str().map(aim)).collect()
}

/// Allow rules and pins.
fn permission_and_pin_changes(sup: &Supervisor, run: &Run, best: &Version, e: &Edits) -> Vec<Json> {
    let tried: HashSet<String> = run.log.iter().filter_map(|l| l["change"]["key"].as_str().map(str::to_string)).collect();
    let mut out: Vec<Json> = Vec::new();
    // Pins, from the week's fixes.
    let (from, to) = super::window_of(None);
    let w = super::Win::load(sup, &run.repo, from, to);
    let rules = super::rules_under(sup, &run.repo, best, &e);
    let found = super::find(sup, &w, &rules, true);
    let fx = super::fixes::build(sup, &w, best, &e, &found);
    for f in fx["fixes"].as_array().into_iter().flatten().filter(|f| f["part"] == "memory").take(3) {
        let key = format!("fix\n{}", f["id"].as_str().unwrap_or(""));
        if tried.contains(&key) {
            continue;
        }
        out.push(json!({ "key": key, "title": f["title"], "part": "memory", "edit": f["edit"], "why": "The week's sessions left this record out where it was needed", "saves": 0, "widens": false, "patterns": [], "aims": ["pass_rate", "turns"] }));
    }
    // Allow rules, for the commands the replays were asked about: each
    // prompt keyed under the rules its own replay ran with, over every
    // version tried, so that what one rule left is found (under a version
    // that allows ./check.sh, `python -m x && ./check.sh` asks for python
    // -m x).
    let res = results(sup, &run.id);
    let version_of: std::collections::HashMap<String, String> = res.iter().filter_map(|r| r.agent.clone().map(|a| (a, r.version.clone()))).collect();
    let mut rules_of: std::collections::HashMap<String, Vec<super::perms::Rule>> = std::collections::HashMap::new();
    let mut asked: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for q in crate::questions::all(sup).into_iter().filter(|q| q.kind == "permission") {
        let Some(v) = q.agent.as_ref().and_then(|a| version_of.get(a)) else { continue };
        let vrules = rules_of.entry(v.clone()).or_insert_with(|| if *v == best.id { rules.clone() } else { super::version(sup, v).map(|vv| super::rules_under(sup, &run.repo, &vv, &edits_of(sup, v))).unwrap_or_default() });
        let cwd = q.agent.as_deref().and_then(|a| sup.agent_row(a)).map(|a| a.cwd).unwrap_or_default();
        if let Some(k) = q.detail.as_ref().and_then(|d| d["command"].as_str()).and_then(|c| asked_key(vrules, c, &cwd)) {
            *asked.entry(k).or_default() += 1;
        }
    }
    let mut calls = Vec::new();
    for a in version_of.keys().filter_map(|a| sup.agent_row(a)) {
        calls.extend(super::perms::calls_of(&a.id, &a.name, &a.cwd, &sup.all_events(&a.id).unwrap_or_default()));
    }
    calls.extend(w.calls.iter().cloned());
    let allow_change = |ks: &[&String], n: usize| -> Json {
        let rules_new: Vec<String> = ks.iter().map(|k| format!("Bash({k}:*)")).collect();
        let mut after_e = e.permissions.clone();
        after_e.allow.extend(rules_new.iter().cloned());
        let after = super::perms::effective(&super::baseline(sup, &run.repo), &after_e, "draft");
        let rep = super::widen_report(&rules, &after, &calls, &w.cwd());
        let patterns = rep["patterns"].clone();
        let widens = patterns.as_array().is_some_and(|p| !p.is_empty());
        let names: Vec<String> = ks.iter().map(|k| format!("`{k}`")).collect();
        let title = match names.len() {
            1 => format!("Allow {} without asking", names[0]),
            _ => format!("Allow {} and {} without asking", names[..names.len() - 1].join(", "), names[names.len() - 1]),
        };
        let why = if ks.len() == 1 { format!("Its replays were asked about it {n} time{}", if n == 1 { "" } else { "s" }) } else { format!("Its replays were asked about these {n} times: one round for all of them, as one alone leaves the others asking") };
        json!({ "key": format!("allow\n{}", rules_new.join("\n")), "title": title, "part": "permissions", "edit": { "permissions": { "allow": rules_new } }, "why": why, "saves": n, "widens": widens, "patterns": patterns, "aims": ["interventions", "wall_s"] })
    };
    let mut rules_out: Vec<(usize, Json)> = Vec::new();
    // A rule asked about less than once a replay cannot be better beyond
    // the noise of its replays: not worth a round.
    let often: Vec<(&String, usize)> = asked.iter().map(|(k, n)| (k, *n)).filter(|(k, n)| *n >= run.n && !e.permissions.allow.contains(&format!("Bash({k}:*)"))).collect();
    for (k, n) in &often {
        let ch = allow_change(&[*k], *n);
        if !tried.contains(ch["key"].as_str().unwrap_or("")) {
            rules_out.push((*n, ch));
        }
    }
    // All of them at once, before any alone: a chain asks for each of its
    // commands no rule allows, so one rule alone can leave every prompt.
    let group: Vec<(&String, usize)> = often.iter().copied().filter(|(k, _)| !failed_in_log(run, &format!("allow\nBash({k}:*)"))).take(5).collect();
    if group.len() >= 2 {
        let ks: Vec<&String> = group.iter().map(|(k, _)| *k).collect();
        let n = group.iter().map(|(_, n)| n).sum();
        let ch = allow_change(&ks, n);
        if !tried.contains(ch["key"].as_str().unwrap_or("")) {
            rules_out.push((n, ch));
        }
    }
    // The most asked first, a widening one too (it waits for you): a
    // round spent on a rule that saves little is a round, and replays,
    // not spent on the one that saves most. Then the pins.
    rules_out.sort_by_key(|(n, j)| (std::cmp::Reverse(*n), j["widens"] == true));
    let pins = std::mem::take(&mut out);
    out.extend(rules_out.into_iter().map(|(_, j)| j));
    out.extend(pins);
    out
}

/// What a prompt for `cmd` was about: the first of its simple commands the
/// rules do not allow (`git status; ./check.sh` asks for `./check.sh`), as
/// the prefix a rule would name.
fn asked_key(rules: &[super::perms::Rule], cmd: &str, cwd: &str) -> Option<String> {
    let probe = |part: &str| super::perms::Probe { tool: "Bash".into(), command: Some(part.to_string()), path: None, cwd: cwd.to_string() };
    let parts = super::perms::parts(cmd);
    let part = parts.iter().filter(|p| !p.starts_with("cd ") && p.as_str() != "cd").find(|p| super::perms::decide(rules, &probe(p)) != "allow").or(parts.first())?;
    super::storm_key(part)
}

// ------------------------------------------- changes for time and cost

/// Claude Code's models, from the most able: each step down is faster and
/// cheaper, and is kept only when it passes what the one above passed.
const LADDER: [&str; 3] = ["opus", "sonnet", "haiku"];
/// The model that writes instructions from the replays.
const BRIEF_MODEL: &str = "opus";
/// How long it may take.
const BRIEF_LIMIT: Duration = Duration::from_secs(180);
/// The most of a replay's tool calls shown to it.
const BRIEF_CALLS: usize = 90;

/// What a result in the log says about its change's replays: they failed
/// what the version before passed (a step down that far is too far).
fn failed_in_log(run: &Run, key: &str) -> bool {
    run.log.iter().filter(|l| l["change"]["key"] == key && l["result"] != "kept").any(|l| l["vs_best"]["success"]["verdict"] == "worse" || l["vs_best"]["success"]["lost"].as_u64().unwrap_or(0) > 0)
}

fn tried_in_log(run: &Run, key: &str) -> bool {
    run.log.iter().any(|l| l["change"]["key"] == key)
}

fn model_name(m: &str) -> String {
    let mut c = m.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// A faster, cheaper model, one step down from the best's, and less
/// effort: each kept only when it passes all the best passed.
fn how_changes(run: &Run, e: &Edits, c: &Case) -> Vec<Json> {
    let mut out = Vec::new();
    let now = crate::routing::model_of(e.agent.model.as_deref().or(c.model.as_deref()));
    let at = LADDER.iter().position(|m| *m == now).unwrap_or(0);
    // A typical turn's mix of tokens, to say what a model saves.
    let price = |m: &str| crate::routing::cost_usd(m, 20_000, 3_000, 200_000);
    for m in LADDER.iter().skip(at + 1) {
        let key = format!("model\n{m}");
        if tried_in_log(run, &key) {
            if failed_in_log(run, &key) {
                break;
            }
            continue;
        }
        let saves = 1.0 - price(m) / price(LADDER[at]).max(1e-9);
        out.push(json!({
            "key": key, "title": format!("Run on {} instead of {}", model_name(m), model_name(LADDER[at])), "part": "agent", "edit": { "agent": { "model": m } },
            "why": format!("A faster model whose tokens cost {:.0}% less: kept only if it passes all {} passed", saves * 100.0, model_name(LADDER[at])),
            "saves": (saves * 100.0).round(), "widens": false, "patterns": [], "aims": ["cost_usd", "wall_s"],
        }));
        break;
    }
    // Less thinking, on the models that take an effort. Only low: Claude
    // Code's default is medium, so medium would be a round spent on nothing.
    let key = "effort\nlow".to_string();
    if LADDER[at] != "haiku" && e.agent.effort.as_deref() != Some("low") && !tried_in_log(run, &key) {
        out.push(json!({
            "key": key, "title": "Think less: effort low", "part": "agent", "edit": { "agent": { "effort": "low" } },
            "why": format!("{} at effort low thinks less before it acts: fewer output tokens and less waiting", model_name(LADDER[at])),
            "saves": 0, "widens": false, "patterns": [], "aims": ["wall_s", "cost_usd"],
        }));
    }
    out
}

/// What the instructions' writer said for a (run, best version): asked
/// once.
fn briefs() -> &'static Mutex<std::collections::HashMap<String, Vec<Json>>> {
    static B: OnceLock<Mutex<std::collections::HashMap<String, Vec<Json>>>> = OnceLock::new();
    B.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

const BRIEF_SYSTEM: &str = "You tune the harness of a coding agent (Claude Code) for one repository. You are shown replays of one stretch of work: the same code and the same scripted user prompts each time, under the harness as it is now, with every tool call the agent made, how long each took, the permission prompts it raised, and what each replay cost. Propose up to 3 instructions to give the agent as its session starts, each of which would make these replays reach the targets without failing a check: for example fewer turns and tool calls, no work done twice, a slow command run no more often than needed, files read together rather than one by one, nothing asked of the user that the repository already answers. Each instruction must be true of this repository's work in general, not of this one task; must never tell the agent to skip, fake or weaken a check, a test or a step the repository's own instructions require; and is at most 300 characters. Answer with JSON only, no prose: {\"changes\":[{\"title\":\"what it changes, imperative, at most 70 characters\",\"instruction\":\"the words the agent gets\",\"aims\":[one or more of \"wall_s\", \"cost_usd\", \"interventions\", \"turns\", \"pass_rate\"],\"why\":\"what in these replays it would save, with their numbers, at most 200 characters\"}]}. Answer {\"changes\":[]} rather than propose an instruction you do not expect to help.";

/// Instructions a model wrote from the best version's replays: each a
/// change of its own.
fn brief_changes(sup: &Supervisor, run: &Run, best: &Version, e: &Edits, c: &Case) -> Vec<Json> {
    let key = format!("{}:{}", run.id, best.id);
    if let Some(v) = briefs().lock().unwrap().get(&key) {
        return v.iter().filter(|ch| !tried_in_log(run, ch["key"].as_str().unwrap_or(""))).cloned().collect();
    }
    let mine: Vec<Res> = results(sup, &run.id).into_iter().filter(|r| r.version == best.id && r.counts()).collect();
    if mine.is_empty() {
        return Vec::new();
    }
    let input = brief_input(sup, run, best, e, c, &mine);
    let dir = sup.cfg.data_dir.join("harness");
    let _ = std::fs::create_dir_all(&dir);
    let args: Vec<String> = ["-p", "--output-format", "json", "--tools", "", "--strict-mcp-config", "--settings", "{\"disableAllHooks\":true}", "--no-session-persistence", "--model", BRIEF_MODEL, "--system-prompt", BRIEF_SYSTEM]
        .iter()
        .map(|s| s.to_string())
        .chain(std::iter::once(input))
        .collect();
    let out = match crate::goals::run_print(&sup.cfg.claude_bin, &args, &dir, BRIEF_LIMIT) {
        Ok(raw) => {
            let (list, cost) = briefs_of_answer(&raw);
            if cost > 0.0 {
                spend(sup, &run.id, cost);
            }
            list
        }
        Err(_) => Vec::new(),
    };
    briefs().lock().unwrap().insert(key, out.clone());
    out.into_iter().filter(|ch| !tried_in_log(run, ch["key"].as_str().unwrap_or(""))).collect()
}

/// The changes in the writer's answer, and what the call cost.
fn briefs_of_answer(raw: &str) -> (Vec<Json>, f64) {
    let Some(res) = raw.lines().rev().find_map(|l| serde_json::from_str::<Json>(l.trim()).ok().filter(|v| v.get("result").is_some())).or_else(|| serde_json::from_str::<Json>(raw.trim()).ok()) else {
        return (Vec::new(), 0.0);
    };
    let cost = res["total_cost_usd"].as_f64().unwrap_or(0.0);
    let text = res["result"].as_str().unwrap_or("").trim();
    let text = text.find('{').and_then(|i| text.rfind('}').map(|j| &text[i..=j])).unwrap_or("");
    let Ok(v) = serde_json::from_str::<Json>(text) else { return (Vec::new(), cost) };
    const AIMS: [&str; 5] = ["wall_s", "cost_usd", "interventions", "turns", "pass_rate"];
    let list = v["changes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|ch| {
            let text = ch["instruction"].as_str().map(str::trim).filter(|t| !t.is_empty())?;
            let text = one_line(text, 400);
            let title = ch["title"].as_str().map(|t| one_line(t.trim(), 90)).filter(|t| !t.is_empty()).unwrap_or_else(|| one_line(&text, 70));
            let aims: Vec<&str> = ch["aims"].as_array().into_iter().flatten().filter_map(Json::as_str).filter(|a| AIMS.contains(a)).collect();
            Some(json!({
                "key": format!("brief\n{}", hash(&text)), "title": format!("Tell it: {title}"), "part": "agent", "edit": { "agent": { "brief": [text] } },
                "why": ch["why"].as_str().map(|w| one_line(w, 240)).unwrap_or_else(|| "Written from what its replays did".into()),
                "saves": 0, "widens": false, "patterns": [], "aims": if aims.is_empty() { vec!["wall_s", "cost_usd"] } else { aims }, "by": BRIEF_MODEL,
            }))
        })
        .take(3)
        .collect();
    (list, cost)
}

/// What the writer reads: the repository's instructions, the case, the
/// targets, what was tried, and the best version's replays call by call.
fn brief_input(sup: &Supervisor, run: &Run, best: &Version, e: &Edits, c: &Case, mine: &[Res]) -> String {
    let mut out = String::new();
    if let Some(commit) = &c.commit {
        if let Ok(t) = git(Path::new(&c.repo), &["show", &format!("{commit}:CLAUDE.md")]) {
            out += &format!("The repository's CLAUDE.md:\n{}\n\n", t.chars().take(3000).collect::<String>());
        }
    }
    out += &format!("The stretch: {}, {} prompt{} from the user.\n", c.name, c.prompts().len(), if c.prompts().len() == 1 { "" } else { "s" });
    let checks: Vec<String> = c.checks.as_array().into_iter().flatten().filter(|k| k["on"] != false).map(|k| match k["kind"].as_str().unwrap_or("") {
        "fail_to_pass" | "pass_to_pass" => format!("the test {} passes", k["test"].as_str().unwrap_or("")),
        "command" => format!("`{}` exits 0", k["run"].as_str().unwrap_or("")),
        "never" => format!("no command contains `{}`", k["pattern"].as_str().unwrap_or("")),
        "limit" => format!("at most {} {}", k["max"], k["metric"].as_str().unwrap_or("")),
        k => k.to_string(),
    }).collect();
    out += &format!("A replay passes when: {}.\n", checks.join("; "));
    let targets: Vec<String> = run.targets.iter().map(|t| format!("{} {}% better", t.metric, t.pct)).collect();
    out += &format!("Targets, against the harness the run started from: {}.\n", targets.join(", "));
    if let Some(w) = c.your_wait_ms() {
        out += &format!("Each intervention (a permission prompt or a question) costs the user about {:.0} s, counted in wall time.\n", w as f64 / 1000.0);
    }
    let how = &e.agent;
    out += &format!("The harness now: model {}, effort {}, allow rules added: {}, instructions: {}.\n",
        how.model.clone().or_else(|| c.model.clone()).unwrap_or_else(|| "the default".into()),
        how.effort.clone().unwrap_or_else(|| "the default".into()),
        if e.permissions.allow.is_empty() { "none".to_string() } else { e.permissions.allow.join(", ") },
        if how.brief.is_empty() { "none".to_string() } else { how.brief.iter().map(|b| format!("“{b}”")).collect::<Vec<_>>().join(" ") });
    let tried: Vec<String> = run.log.iter().filter(|l| l["version"].is_string()).map(|l| format!("{} ({})", l["change"]["title"].as_str().unwrap_or(""), l["result"].as_str().unwrap_or(""))).collect();
    if !tried.is_empty() {
        out += &format!("Tried already: {}.\n", tried.join("; "));
    }
    out += &format!("These are the replays of {}.\n\n", best.name);
    let mut shown: Vec<&Res> = mine.iter().filter(|r| r.passed).take(2).collect();
    if shown.is_empty() {
        shown = mine.iter().take(2).collect();
    }
    for r in shown {
        out += &trace_of(sup, r);
        out += "\n";
    }
    out
}

/// One replay, call by call, as the writer reads it.
fn trace_of(sup: &Supervisor, r: &Res) -> String {
    let Some(agent) = &r.agent else { return String::new() };
    let evs = sup.all_events(agent).unwrap_or_default();
    let data = |e: &crate::db::Event| e.data.as_deref().and_then(|d| serde_json::from_str::<Json>(d).ok()).unwrap_or(Json::Null);
    let mut ends: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for e in evs.iter().filter(|e| e.kind.starts_with("PostToolUse")) {
        if let Some(id) = data(e)["tool_use_id"].as_str() {
            ends.insert(id.to_string(), e.ts);
        }
    }
    let asked: Vec<String> = crate::questions::all(sup).into_iter().filter(|q| q.agent.as_deref() == Some(agent.as_str())).map(|q| q.detail.as_ref().and_then(|d| d["command"].as_str().map(str::to_string)).unwrap_or_else(|| one_line(&q.text, 120))).collect();
    let mut out = format!(
        "Replay {} ({}{}): {} interventions, {} turns, {:.0} s of the agent's time, {} tokens, ${:.3}.\n",
        r.k,
        if r.passed { "passed" } else { "failed" },
        r.model.as_deref().map(|m| format!(", model {m}")).unwrap_or_default(),
        r.interventions,
        r.turns,
        (r.wall_ms - r.wait_ms) as f64 / 1000.0,
        r.tokens,
        r.cost_usd
    );
    if !asked.is_empty() {
        out += &format!("It asked the user {} time{}: {}.\n", asked.len(), if asked.len() == 1 { "" } else { "s" }, asked.iter().map(|a| format!("`{}`", one_line(a, 120))).collect::<Vec<_>>().join(", "));
    }
    let t0 = r.started;
    let mut calls = 0usize;
    let mut turn = 0;
    for e in &evs {
        match e.kind.as_str() {
            "UserPromptSubmit" => {
                turn += 1;
                out += &format!("Turn {turn} at +{:.0} s, the user: {}\n", (e.ts - t0) as f64 / 1000.0, one_line(&evals::prompt_of(e).unwrap_or_default(), 400));
            }
            "PreToolUse" => {
                calls += 1;
                if calls > BRIEF_CALLS {
                    continue;
                }
                let d = data(e);
                let i = &d["tool_input"];
                let what = i["command"].as_str().or(i["file_path"].as_str()).or(i["pattern"].as_str()).or(i["description"].as_str()).or(i["url"].as_str()).unwrap_or("");
                let took = d["tool_use_id"].as_str().and_then(|id| ends.get(id)).map(|t| format!(" ({:.1} s)", (t - e.ts) as f64 / 1000.0)).unwrap_or_default();
                out += &format!("  {} {}{took}\n", e.tool.as_deref().unwrap_or("tool"), one_line(what, 200));
            }
            "Stop" => out += &format!("  (the turn ended at +{:.0} s)\n", (e.ts - t0) as f64 / 1000.0),
            _ => {}
        }
    }
    if calls > BRIEF_CALLS {
        out += &format!("  ... and {} more tool calls\n", calls - BRIEF_CALLS);
    }
    out
}

/// `best`'s edits with a change on top.
fn with_change(sup: &Supervisor, best: &Version, ch: &Json) -> Edits {
    let mut e = edits_of(sup, &best.id);
    for r in ch["edit"]["permissions"]["allow"].as_array().into_iter().flatten().filter_map(Json::as_str) {
        if !e.permissions.allow.iter().any(|x| x == r) {
            e.permissions.allow.push(r.to_string());
        }
    }
    let how = &ch["edit"]["agent"];
    if let Some(m) = how["model"].as_str() {
        e.agent.model = Some(m.to_string());
    }
    if let Some(x) = how["effort"].as_str() {
        e.agent.effort = Some(x.to_string());
    }
    for b in how["brief"].as_array().into_iter().flatten().filter_map(Json::as_str) {
        if !e.agent.brief.iter().any(|x| x == b) {
            e.agent.brief.push(b.to_string());
        }
    }
    for p in ch["edit"]["memory"]["pins"].as_array().into_iter().flatten() {
        if let Ok(pin) = serde_json::from_value::<super::Pin>(p.clone()) {
            if !e.memory.pins.iter().any(|x| x.record == pin.record) {
                e.memory.pins.push(pin);
            }
        }
    }
    e
}

/// Optimize this: A first, then a change at a time on the best so far.
/// Resumable from its rows: after the user answers a change that waited,
/// it carries on from where it was.
fn optimize(sup: &Arc<Supervisor>, lv: &Arc<Levels>, run: &mut Run) -> Result<Option<String>, String> {
    let c = run.cases.first().and_then(|id| evals::case(sup, id)).ok_or("its case was deleted")?;
    let a0 = super::version(sup, &run.a).ok_or("its version A is gone")?;
    ensure(sup, lv, run, &c, &a0.id, "a", run.n)?;
    loop {
        let fresh = run_of(sup, &run.id).ok_or("the run is gone")?;
        run.log = fresh.log;
        run.approved = fresh.approved;
        run.best = fresh.best;
        let best = run.best.as_deref().and_then(|b| super::version(sup, b)).unwrap_or_else(|| a0.clone());
        // Reached already?
        if run.best.is_some() {
            let vs_a = card(sup, run, &c, &a0.id, &best.id, &run.targets, true);
            if vs_a["targets_met"] == true {
                return Ok(Some(format!("every target is met by {}", best.name)));
            }
        }
        let tried = run.log.iter().filter(|l| l["version"].is_string()).count();
        if tried >= run.rounds {
            return Ok(Some(format!("it tried {tried} change{} without meeting every target", if tried == 1 { "" } else { "s" })));
        }
        let Some(ch) = changes(sup, run, &best, &c).into_iter().find(|ch| !run.log.iter().any(|l| l["change"]["key"] == ch["key"])) else {
            return Ok(Some(if run.best.is_some() { "no more changes to try: the best so far is kept as a candidate".into() } else { "no change to try: its replays were asked nothing, and the week calls for no pin".into() }));
        };
        // A change that widens waits for the user.
        let patterns: Vec<String> = ch["patterns"].as_array().into_iter().flatten().filter_map(|p| p.as_str().map(str::to_string)).collect();
        if ch["widens"] == true && !patterns.iter().all(|p| run.approved.contains(p)) {
            let mut r = run_of(sup, &run.id).ok_or("the run is gone")?;
            r.state = "needs_you".into();
            r.waiting = Some(ch.clone());
            r.why = Some(format!("“{}” widens what agents may do: try it, or skip it", ch["title"].as_str().unwrap_or("")));
            r.put(sup).map_err(|e| e.message)?;
            *run = r;
            return Ok(None);
        }
        let spent = run_of(sup, &run.id).map_or(0.0, |r| r.spent_usd);
        if spent + est_one(&c).min(run.cap_run_usd) * run.n as f64 > run.cap_usd + 1e-9 {
            return Ok(Some(format!("its budget of ${:.2} would not cover another {} replays (${spent:.2} spent)", run.cap_usd, run.n)));
        }
        let round = tried + 1;
        let e = with_change(sup, &best, &ch);
        let v = version_with(sup, run, &best, &e, ch["title"].as_str().unwrap_or(""), round)?;
        // The entry first, so that the page shows what is being tried.
        let mut r = run_of(sup, &run.id).ok_or("the run is gone")?;
        r.log.push(json!({ "round": round, "version": v.id, "name": v.name, "base": best.id, "base_name": best.name, "change": ch, "result": "running", "started": iso(now_ms()) }));
        r.put(sup).map_err(|e| e.message)?;
        let played = ensure(sup, lv, run, &c, &v.id, "b", run.n);
        let vs_best = card(sup, run, &c, &best.id, &v.id, &[], true);
        let vs_a = card(sup, run, &c, &a0.id, &v.id, &run.targets, true);
        let result = match (&played, vs_best["decision"]["result"].as_str()) {
            (Err(_), _) => "stopped",
            (Ok(()), Some("keep")) => "kept",
            (Ok(()), _) => "not_kept",
        };
        let mut r = run_of(sup, &run.id).ok_or("the run is gone")?;
        if let Some(l) = r.log.iter_mut().rev().find(|l| l["version"] == v.id.as_str()) {
            l["result"] = json!(result);
            l["why"] = vs_best["decision"]["why"].clone();
            l["vs_best"] = json!({ "decision": vs_best["decision"], "metrics": vs_best["metrics"], "success": vs_best["success"] });
            l["vs_a"] = json!({ "targets": vs_a["targets"], "targets_met": vs_a["targets_met"], "metrics": vs_a["metrics"] });
        }
        if result == "kept" {
            r.best = Some(v.id.clone());
            let mut kept = v.clone();
            kept.state = "candidate".into();
            let _ = kept.put(sup);
            // The one it replaces goes back to tried.
            if let Some(mut old) = super::version(sup, &best.id).filter(|o| o.state == "candidate" && o.made_by == format!("optimize:{}", run.id)) {
                old.state = "tried".into();
                let _ = old.put(sup);
            }
        }
        r.put(sup).map_err(|e| e.message)?;
        *run = r;
        played?;
        if result == "kept" && vs_a["targets_met"] == true {
            return Ok(Some(format!("every target is met by {}", v.name)));
        }
    }
}

// ------------------------------------------------------------------ start

#[derive(Deserialize, Default, Clone)]
pub struct StartB {
    pub repo: Option<String>,
    /// `compare` (default) or `optimize`.
    pub kind: Option<String>,
    pub a: Option<String>,
    pub b: Option<String>,
    pub suite: Option<String>,
    pub cases: Option<Vec<String>>,
    /// Optimize this on a stretch not yet a case: its session and turns.
    pub agent: Option<String>,
    #[serde(alias = "turns_from")]
    pub from: Option<i64>,
    #[serde(alias = "turns_to")]
    pub to: Option<i64>,
    /// Optimize this on a stretch: the checks its case keeps (the page's
    /// ticks and commands); without it, the proposal's ticked ones.
    pub checks: Option<Vec<Json>>,
    pub name: Option<String>,
    pub n: Option<usize>,
    pub rounds: Option<usize>,
    pub cap_usd: Option<f64>,
    pub cap_run_usd: Option<f64>,
    pub targets: Option<Vec<Target>>,
    /// The price the user saw (the estimate, or the cap of Optimize this).
    pub confirm_usd: Option<f64>,
    pub approve_widening: Option<Json>,
}

/// What a run would be: its versions, cases and price, or why not.
fn plan(sup: &Supervisor, b: &StartB) -> Result<(Run, Vec<Case>, Json), ApiError> {
    let kind = b.kind.clone().unwrap_or_else(|| "compare".into());
    if !matches!(kind.as_str(), "compare" | "optimize") {
        return Err(ApiError::bad("kind is compare or optimize"));
    }
    let mut cases: Vec<Case> = match (&b.cases, &b.suite) {
        (Some(ids), _) => ids.iter().map(|id| evals::case(sup, id).ok_or_else(|| ApiError::not_found(format!("no case {id}")))).collect::<Result<_, _>>()?,
        (None, Some(s)) => {
            let repo = super::repo_or_default(sup, b.repo.clone())?;
            evals::cases_of(sup, &repo).into_iter().filter(|c| &c.suite == s).collect()
        }
        (None, None) => match &b.repo {
            Some(_) if b.agent.is_none() => {
                let repo = super::repo_or_default(sup, b.repo.clone())?;
                evals::cases_of(sup, &repo)
            }
            _ => Vec::new(),
        },
    };
    let repo = match cases.first() {
        Some(c) => c.repo.clone(),
        None => match &b.agent {
            Some(a) => sup.agent_row(a).map(|a| a.repo).ok_or_else(|| ApiError::not_found(format!("no agent {a}")))?,
            None => return Err(ApiError::bad("name the cases (cases, suite or repo), or the session and its turns (agent, from, to)")),
        },
    };
    if cases.iter().any(|c| c.repo != repo) {
        return Err(ApiError::bad("the cases of a run are of one repository"));
    }
    cases.retain(|c| c.commit.is_some());
    let n = b.n.unwrap_or(DEFAULT_N);
    if n == 0 || n > MAX_N {
        return Err(ApiError::bad(format!("n is 1 to {MAX_N} runs a side")));
    }
    let cap = b.cap_usd.unwrap_or(CAP_USD);
    let cap_run = b.cap_run_usd.unwrap_or(CAP_RUN_USD);
    if !(cap > 0.0 && cap <= 500.0) || !(cap_run > 0.0 && cap_run <= cap) {
        return Err(ApiError::bad("cap_usd is above 0 and at most 500, cap_run_usd above 0 and at most cap_usd"));
    }
    let cfg = |id: &str| super::version(sup, id).filter(|v| v.kind == "config" && v.repo == repo).ok_or_else(|| ApiError::not_found(format!("no version {id} of {repo}")));
    let a = match &b.a {
        Some(id) => cfg(id)?,
        None => super::current(sup, &repo),
    };
    let bv = match (&b.b, kind.as_str()) {
        (Some(id), _) => Some(cfg(id)?),
        (None, "compare") => return Err(ApiError::bad("a comparison needs b: the version to try against a")),
        (None, _) => None,
    };
    if bv.as_ref().is_some_and(|v| v.id == a.id) && kind == "compare" && b.n.unwrap_or(DEFAULT_N) < 2 {
        return Err(ApiError::bad("A against itself measures the noise: it needs at least 2 runs a side"));
    }
    let targets = b.targets.clone().unwrap_or_else(|| if kind == "optimize" { vec![Target { metric: "interventions".into(), pct: 30.0 }] } else { Vec::new() });
    if let Some(t) = targets.iter().find(|t| !score::target_metric(&t.metric) || !(0.0..=100.0).contains(&t.pct)) {
        return Err(ApiError::bad(format!("{} is not a target: interventions, wall_s, cost_usd, pass_rate (or prompts, turns, tokens), each 0 to 100%", t.metric)));
    }
    if kind == "optimize" && targets.is_empty() {
        return Err(ApiError::bad("Optimize this needs at least one target: what it should improve"));
    }
    let rounds = b.rounds.unwrap_or(DEFAULT_ROUNDS).min(MAX_ROUNDS);
    // The price.
    let skipped: Vec<Json> = cases.iter().filter(|c| est_one(c) > cap_run).map(|c| json!({ "case": c.id, "name": c.name, "why": format!("its original cost ${:.2}, over the cap of a replay (${cap_run:.2}): make a case of fewer turns", c.cost_usd) })).collect();
    let playable: Vec<&Case> = cases.iter().filter(|c| est_one(c) <= cap_run).collect();
    let per_round: f64 = playable.iter().map(|c| est_one(c)).sum::<f64>() * n as f64;
    let (est, most) = if kind == "optimize" {
        // A's runs, then each change's: at most the cap.
        (per_round * 2.0, (per_round * (1 + rounds) as f64).min(cap))
    } else {
        (per_round * 2.0, (per_round * 2.0).min(cap))
    };
    let run = Run {
        id: format!("r{}", &hash(&format!("{repo}:{}:{}", a.id, now_ms()))[..10]),
        repo: repo.clone(),
        kind: kind.clone(),
        a: a.id.clone(),
        b: bv.as_ref().map(|v| v.id.clone()),
        cases: playable.iter().map(|c| c.id.clone()).collect(),
        n,
        rounds,
        cap_usd: cap,
        cap_run_usd: cap_run,
        est_usd: est,
        state: "queued".into(),
        targets,
        made_by: "you".into(),
        created: now_ms(),
        ..Run::default()
    };
    let unknown = playable.iter().filter(|c| c.cost_usd <= 0.0).count();
    let estimate = json!({
        "kind": kind, "a": { "id": a.id, "name": a.name }, "b": bv.as_ref().map(|v| json!({ "id": v.id, "name": v.name })),
        "cases": playable.len(), "skipped": skipped, "n": n, "rounds": rounds,
        "replays": if kind == "optimize" { n * (1 + rounds) } else { n * 2 * playable.len() },
        "est_usd": money(est), "most_usd": money(most), "confirm_usd": money(most), "cap_usd": cap, "cap_run_usd": cap_run,
        "per_replay_usd": money(if playable.is_empty() { 0.0 } else { playable.iter().map(|c| est_one(c)).sum::<f64>() / playable.len() as f64 }),
        "estimated_from": if unknown > 0 { format!("the originals' cost; {unknown} case{} had none, counted at ${UNKNOWN_COST:.2}", if unknown == 1 { "" } else { "s" }) } else { "the originals' cost".to_string() },
        "targets": run.targets, "over_cap": est > cap + 1e-9 && kind == "compare",
        "honest": if kind == "optimize" { "One stretch says whether a change helps this stretch, not whether it generalizes: test what it keeps on the suite before you promote it." } else { "Fewer than 6 cases can say no more than “too few to tell”." },
    });
    Ok((run, cases, estimate))
}

/// The widening of B (or of nothing yet, for Optimize this) against A.
fn widening(sup: &Supervisor, run: &Run) -> Vec<String> {
    let Some(b) = run.b.as_deref().and_then(|b| super::version(sup, b)) else { return Vec::new() };
    let Some(a) = super::version(sup, &run.a) else { return Vec::new() };
    let (from, to) = super::window_of(None);
    let w = super::Win::load(sup, &run.repo, from, to);
    let before = super::rules_under(sup, &run.repo, &a, &edits_of(sup, &a.id));
    let after = super::rules_under(sup, &run.repo, &b, &edits_of(sup, &b.id));
    let rep = super::widen_report(&before, &after, &w.calls, &w.cwd());
    rep["patterns"].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(str::to_string)).collect()
}

fn named(v: &Option<Json>) -> Vec<String> {
    match v {
        Some(Json::Array(a)) => a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
        Some(Json::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    }
}

/// `POST /api/harness/runs`: a run queued, once its price was seen and any
/// widening approved.
pub fn start(sup: &Arc<Supervisor>, lv: &Arc<Levels>, mut b: StartB) -> Result<Json, ApiError> {
    // Optimize this on a stretch: its case first.
    let mut made_case = None;
    if b.kind.as_deref() == Some("optimize") && b.cases.is_none() {
        let agent = b.agent.clone().ok_or_else(|| ApiError::bad("Optimize this needs a case, or the session and its turns (agent, from, to)"))?;
        let p = evals::propose(sup, &evals::ProposeQ { agent: agent.clone(), from: b.from, to: b.to })?;
        if p["commit"].is_null() {
            return Err(ApiError::conflict(p["warnings"][0].as_str().unwrap_or("this stretch cannot be replayed").to_string()));
        }
        // Its runs, caps, targets and price are checked before the case is kept.
        plan(sup, &StartB { cases: None, agent: Some(agent.clone()), ..b.clone() })?;
        let cost_one = p["cost_usd"].as_f64().filter(|c| *c > 0.0).unwrap_or(UNKNOWN_COST);
        let n = b.n.unwrap_or(DEFAULT_N) as f64;
        let rounds = b.rounds.unwrap_or(DEFAULT_ROUNDS).min(MAX_ROUNDS) as f64;
        let cap = b.cap_usd.unwrap_or(CAP_USD);
        let cap_run = b.cap_run_usd.unwrap_or(CAP_RUN_USD);
        if cost_one > cap_run {
            return Err(ApiError::new(409, "over_cap", format!("this stretch cost ${cost_one:.2} when it ran, over the cap of a replay (${cap_run:.2}): pick fewer turns, or raise cap_run_usd")));
        }
        let most = money((cost_one * n * (1.0 + rounds)).min(cap));
        if b.confirm_usd.is_none_or(|c| c + 0.005 < most) {
            return Err(ApiError::new(409, "confirm_cost", format!("Optimize this costs at most ${most:.2} (about ${:.2} a replay, {} replays a round): start it again with confirm_usd: {most}", money(cost_one), n as usize)));
        }
        let mk = evals::create(sup, evals::CreateB { agent, from: b.from, to: b.to, suite: Some("Optimize this".into()), checks: b.checks.clone(), name: b.name.clone().filter(|n| !n.trim().is_empty()), ..evals::CreateB::default() })?;
        let id = mk["case"]["id"].as_str().unwrap_or("").to_string();
        made_case = Some(id.clone());
        b.cases = Some(vec![id]);
    }
    let (mut run, cases, est) = plan(sup, &b)?;
    if run.cases.is_empty() {
        return Err(ApiError::conflict(if cases.is_empty() { "no case to play: make one from a session first (Make this a test)".to_string() } else { format!("no case can be played under the cap of a replay: {}", est["skipped"][0]["why"].as_str().unwrap_or("")) }));
    }
    if run.kind == "compare" && est["over_cap"] == true {
        return Err(ApiError::new(409, "over_cap", format!("this run is estimated at ${:.2}, over its cap of ${:.2}: fewer runs (n) or cases, or a higher cap_usd", run.est_usd, run.cap_usd)));
    }
    let confirm = est["confirm_usd"].as_f64().unwrap_or(0.0);
    if made_case.is_none() && b.confirm_usd.is_none_or(|c| c + 0.005 < confirm) {
        return Err(ApiError::new(409, "confirm_cost", format!("this run costs up to ${confirm:.2} ({} replays): start it again with confirm_usd: {confirm}", est["replays"])));
    }
    let patterns = widening(sup, &run);
    let ok = named(&b.approve_widening);
    let unnamed: Vec<&String> = patterns.iter().filter(|p| !ok.contains(p)).collect();
    if !unnamed.is_empty() {
        return Err(ApiError::new(409, "widens", format!("B widens what agents may do, and a replay runs its commands for real: {}. To test it, confirm by naming {}: approve_widening: {}", unnamed.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "), if unnamed.len() == 1 { "the pattern" } else { "each pattern" }, json!(patterns))));
    }
    run.approved = patterns;
    run.put(sup)?;
    kick(sup, lv);
    Ok(json!({ "run": run.to_json(sup), "estimate": est, "case": made_case }))
}

// ------------------------------------------------------------------ views

/// `GET /api/harness/runs/:id`: the run, its cases with each replay, and
/// its scorecard (or, for Optimize this, each round's).
pub fn view(sup: &Supervisor, id: &str) -> Result<Json, ApiError> {
    let run = run_of(sup, id).ok_or_else(|| ApiError::not_found(format!("no run {id}")))?;
    let rs = results(sup, id);
    let cases: Vec<Case> = run.cases.iter().filter_map(|c| evals::case(sup, c)).collect();
    let finished = !matches!(run.state.as_str(), "queued" | "running");
    let side_of = |r: &Res| -> &'static str {
        if run.kind == "optimize" {
            if r.version == run.a {
                "a"
            } else {
                "b"
            }
        } else if r.side == "a" {
            "a"
        } else {
            "b"
        }
    };
    let case_rows: Vec<Json> = cases
        .iter()
        .map(|c| {
            let mine: Vec<&Res> = rs.iter().filter(|r| r.case == c.id).collect();
            json!({
                "case": c.to_json(),
                "a": mine.iter().filter(|r| side_of(r) == "a").map(|r| r.to_json()).collect::<Vec<_>>(),
                "b": mine.iter().filter(|r| side_of(r) == "b").map(|r| r.to_json()).collect::<Vec<_>>(),
            })
        })
        .collect();
    let scores = if run.kind == "optimize" {
        let best = run.best.clone();
        match (cases.first(), best) {
            (Some(c), Some(b)) => card(sup, &run, c, &run.a, &b, &run.targets, finished),
            (Some(c), None) => {
                let cr = CaseRuns { case: c.id.clone(), name: c.name.clone(), held_out: c.set == "held_out", a: runs_of(sup, &run.id, &c.id, &run.a), b: Vec::new() };
                score::stretch(&cr, run.n, finished, &run.targets)
            }
            _ => Json::Null,
        }
    } else {
        let b = run.b.clone().unwrap_or_default();
        let crs: Vec<CaseRuns> = cases
            .iter()
            .map(|c| CaseRuns {
                case: c.id.clone(),
                name: c.name.clone(),
                held_out: c.set == "held_out",
                a: rs.iter().filter(|r| r.case == c.id && r.side == "a" && r.counts()).map(Res::m).collect(),
                b: rs.iter().filter(|r| r.case == c.id && r.side == "b" && r.version == b && r.counts()).map(Res::m).collect(),
            })
            .collect();
        let sc = if crs.len() == 1 { score::stretch(&crs[0], run.n, finished, &run.targets) } else { score::scorecard(&crs, run.n, finished, &run.targets) };
        if b == run.a {
            as_noise(sc)
        } else {
            sc
        }
    };
    let replays = rs.iter().filter(|r| r.counts()).count();
    let now_playing = rs.iter().find(|r| r.state == "running").map(|r| {
        let evs = r.agent.as_deref().map(|a| sup.all_events(a).unwrap_or_default()).unwrap_or_default();
        let log: Vec<Json> = evs.iter().rev().take(12).rev().map(|e| json!({ "ts": iso(e.ts), "kind": e.kind, "tool": e.tool, "summary": e.summary.as_deref().map(|s| one_line(s, 160)) })).collect();
        json!({ "result": r.id, "agent": r.agent, "case": r.case, "side": side_of(r), "k": r.k, "log": log })
    });
    Ok(json!({
        "run": run.to_json(sup), "cases": case_rows, "scores": scores, "replays": replays,
        "errors": rs.iter().filter(|r| r.state == "error").map(|r| json!({ "case": r.case, "side": r.side, "why": r.why })).collect::<Vec<_>>(),
        "now": now_playing,
        "sandbox": ["A clone of the repository at the case's commit, with no remote: nothing can be pushed", "Edits and tests allowed in the clone only, as in an agent's worktree", "Its own ChloeCtx store, empty, the distiller off: your memory is not read or written", "Each replay capped at its own budget, and the run at its own"],
    }))
}

/// A against itself: the same version on both sides, so whatever differs
/// is noise by construction, whatever an interval of a few runs says. A
/// case that passed on one side and failed on the other is flaky.
fn as_noise(mut sc: Json) -> Json {
    let result = sc["decision"]["result"].as_str().unwrap_or("").to_string();
    for m in sc["metrics"].as_array_mut().into_iter().flatten() {
        if matches!(m["verdict"].as_str(), Some("better" | "worse")) {
            m["verdict"] = json!("noise");
        }
    }
    if matches!(result.as_str(), "running" | "too_few") {
        return sc;
    }
    let mut why = vec![json!("A against itself: the same version on both sides, so every difference here is noise, and its spread is the floor a change has to clear")];
    let flaky: Vec<String> = sc["regressions"].as_array().into_iter().flatten().filter_map(|r| r["name"].as_str().map(str::to_string)).collect();
    if !flaky.is_empty() {
        why.push(json!(format!("Flaky: {} passed in some runs and failed in others, so a comparison on {} says little", flaky.join(", "), if flaky.len() == 1 { "it" } else { "them" })));
    }
    sc["decision"] = json!({ "result": "no_detectable_difference", "why": why });
    sc["flaky"] = sc["regressions"].take();
    sc["regressions"] = json!([]);
    sc["targets_met"] = json!(false);
    sc
}

pub fn list(sup: &Supervisor, repo: &str) -> Vec<Json> {
    let mut rs = runs_in(sup, "repo = :repo", &Params::new().set("repo", sv(repo)));
    rs.sort_by_key(|r| std::cmp::Reverse(r.created));
    rs.iter().take(50).map(|r| r.to_json(sup)).collect()
}

/// `POST /api/harness/runs/:id/stop`: Enough. A queued run never starts;
/// a playing one stops its replay now; Optimize this keeps its best.
pub fn stop(sup: &Supervisor, id: &str) -> Result<Json, ApiError> {
    let mut run = run_of(sup, id).ok_or_else(|| ApiError::not_found(format!("no run {id}")))?;
    match run.state.as_str() {
        "queued" | "needs_you" => {
            run.state = "stopped".into();
            run.why = Some(if run.best.is_some() { "you said enough: the best so far is kept".into() } else { "you stopped it".into() });
            run.waiting = None;
            run.ended = Some(now_ms());
            run.put(sup)?;
        }
        "running" => {
            live().lock().unwrap().stop.insert(id.to_string());
        }
        s => return Err(ApiError::conflict(format!("the run is {s} already"))),
    }
    Ok(json!({ "run": run.to_json(sup) }))
}

#[derive(Deserialize, Default)]
pub struct DecideB {
    /// Try the change that waits (its patterns named), or skip it.
    pub approve_widening: Option<Json>,
    pub skip: Option<bool>,
}

/// `POST /api/harness/runs/:id/decide`: the user's answer to a change
/// that widens; the run carries on.
pub fn decide(sup: &Arc<Supervisor>, lv: &Arc<Levels>, id: &str, b: DecideB) -> Result<Json, ApiError> {
    let mut run = run_of(sup, id).ok_or_else(|| ApiError::not_found(format!("no run {id}")))?;
    if run.state != "needs_you" {
        return Err(ApiError::conflict(format!("the run is {}: nothing waits for you", run.state)));
    }
    let ch = run.waiting.clone().unwrap_or(Json::Null);
    if b.skip == Some(true) {
        run.log.push(json!({ "round": Json::Null, "change": ch, "result": "skipped", "why": ["you skipped it"] }));
    } else {
        let patterns: Vec<String> = ch["patterns"].as_array().into_iter().flatten().filter_map(|p| p.as_str().map(str::to_string)).collect();
        let ok = named(&b.approve_widening);
        let unnamed: Vec<&String> = patterns.iter().filter(|p| !ok.contains(p)).collect();
        if !unnamed.is_empty() {
            return Err(ApiError::new(409, "widens", format!("to try it, name {}: approve_widening: {}", if unnamed.len() == 1 { "the pattern" } else { "each pattern" }, json!(patterns))));
        }
        for p in patterns {
            if !run.approved.contains(&p) {
                run.approved.push(p);
            }
        }
    }
    run.waiting = None;
    run.state = "queued".into();
    run.why = None;
    run.put(sup)?;
    kick(sup, lv);
    Ok(json!({ "run": run.to_json(sup) }))
}

// ----------------------------------------------------------------- routes

async fn estimate_route(State(app): State<App>, body: Bytes) -> ApiResult {
    let b: StartB = parse_body(&body)?;
    let sup = app.sup.clone();
    Ok(AxJson(blocking(move || {
        if b.kind.as_deref() == Some("optimize") && b.cases.is_none() {
            let agent = b.agent.clone().ok_or_else(|| ApiError::bad("Optimize this needs the session and its turns (agent, from, to)"))?;
            let p = evals::propose(&sup, &evals::ProposeQ { agent, from: b.from, to: b.to })?;
            let cost_one = p["cost_usd"].as_f64().filter(|c| *c > 0.0).unwrap_or(UNKNOWN_COST);
            let n = b.n.unwrap_or(DEFAULT_N);
            let rounds = b.rounds.unwrap_or(DEFAULT_ROUNDS).min(MAX_ROUNDS);
            let cap = b.cap_usd.unwrap_or(CAP_USD);
            let most = money((cost_one * n as f64 * (1 + rounds) as f64).min(cap));
            return Ok::<Json, ApiError>(json!({
                "kind": "optimize", "n": n, "rounds": rounds, "replays": n * (1 + rounds), "per_replay_usd": money(cost_one),
                "est_usd": money(cost_one * n as f64 * 2.0), "most_usd": most, "confirm_usd": most, "cap_usd": cap, "cap_run_usd": b.cap_run_usd.unwrap_or(CAP_RUN_USD),
                "estimated_from": if p["cost_usd"].as_f64().unwrap_or(0.0) > 0.0 { "the original's cost".to_string() } else { format!("no cost was recorded: ${UNKNOWN_COST:.2} a replay") },
                "over_cap": cost_one > b.cap_run_usd.unwrap_or(CAP_RUN_USD), "replayable": p["replayable"], "warnings": p["warnings"],
                "honest": "One stretch says whether a change helps this stretch, not whether it generalizes: test what it keeps on the suite before you promote it.",
            }));
        }
        plan(&sup, &b).map(|(_, _, e)| e)
    })
    .await?)
    .into_response())
}

async fn start_route(State(app): State<App>, headers: HeaderMap, body: Bytes) -> ApiResult {
    user_only(&app, &headers)?;
    let b: StartB = parse_body(&body)?;
    let (sup, lv) = (app.sup.clone(), app.lv.clone());
    Ok((StatusCode::CREATED, AxJson(blocking(move || start(&sup, &lv, b)).await?)).into_response())
}

async fn list_route(State(app): State<App>, Query(q): Query<super::RepoQ>) -> ApiResult {
    let sup = app.sup.clone();
    Ok(AxJson(blocking(move || {
        let repo = super::repo_or_default(&sup, q.repo)?;
        Ok::<Json, ApiError>(json!({ "repo": repo, "runs": list(&sup, &repo) }))
    })
    .await?)
    .into_response())
}

async fn view_route(State(app): State<App>, AxPath(id): AxPath<String>) -> ApiResult {
    let sup = app.sup.clone();
    Ok(AxJson(blocking(move || view(&sup, &id)).await?).into_response())
}

async fn stop_route(State(app): State<App>, headers: HeaderMap, AxPath(id): AxPath<String>) -> ApiResult {
    user_only(&app, &headers)?;
    let sup = app.sup.clone();
    Ok(AxJson(blocking(move || stop(&sup, &id)).await?).into_response())
}

async fn decide_route(State(app): State<App>, headers: HeaderMap, AxPath(id): AxPath<String>, body: Bytes) -> ApiResult {
    user_only(&app, &headers)?;
    let b: DecideB = parse_body(&body)?;
    let (sup, lv) = (app.sup.clone(), app.lv.clone());
    Ok(AxJson(blocking(move || decide(&sup, &lv, &id, b)).await?).into_response())
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/harness/runs", get(list_route).post(start_route))
        .route("/api/harness/runs/estimate", post(estimate_route))
        .route("/api/harness/runs/:id", get(view_route))
        .route("/api/harness/runs/:id/stop", post(stop_route))
        .route("/api/harness/runs/:id/decide", post(decide_route))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_are_found_by_answer() {
        let l: Vec<String> = ["Yes", "Yes, and don't ask again for echo commands in /x", "Yes, and switch to auto mode · auto mode handles these prompts for you", "No"].iter().map(|s| s.to_string()).collect();
        assert_eq!(option_for(&l, "yes"), Some(0));
        assert_eq!(option_for(&l, "yes_more"), Some(1));
        assert_eq!(option_for(&l, "no"), Some(3));
        let three: Vec<String> = ["Yes", "Yes, and switch to auto mode", "No"].iter().map(|s| s.to_string()).collect();
        // No "don't ask again" here: a plain yes, never auto mode.
        assert_eq!(option_for(&three, "yes_more"), Some(0));
        assert!(alike("Should totals round half up?", "Do totals round half up or down?") >= 0.35);
        assert!(alike("Which database?", "Should totals round half up?") < 0.35);
        assert_eq!(claude_slug(Path::new("/Users/x/.local/share/w b/r1")), "-Users-x--local-share-w-b-r1");
    }

    #[test]
    fn a_chain_is_answered_as_the_stretch_answered_its_commands() {
        let perm = |k: &str, a: &str| (k.to_string(), a.to_string(), false);
        let mut s = Script { prompts: vec![], perms: vec![perm("Bash: ./scripts/check.sh", "yes"), perm("Bash: rm -rf", "no")], answers: vec![] };
        assert_eq!(s.perm_of("Bash: ./scripts/check.sh", None).as_deref(), Some("yes"));
        // Asked as its first command, which the stretch never ran alone.
        assert_eq!(s.perm_of("Bash: grep -n", Some("grep -n total src/a.py; ./scripts/check.sh")).as_deref(), Some("yes"));
        assert_eq!(s.perm_of("Bash: ./scripts/check.sh", Some("./scripts/check.sh && rm -rf out")).as_deref(), Some("yes"), "its own key first");
        assert_eq!(s.perm_of("Bash: ls", Some("ls && rm -rf out && ./scripts/check.sh")).as_deref(), Some("no"), "a No in the chain wins");
        assert_eq!(s.perm_of("Bash: grep -n", Some("grep -n x a; cat b")), None);
    }

    #[test]
    fn a_prompt_is_keyed_on_the_command_the_rules_do_not_allow() {
        let rule = |t: &str| super::super::perms::Rule { decision: "allow".into(), text: t.into(), from: "Workbench".into() };
        let rules = vec![rule("Bash(git status:*)"), rule("Bash(git stash:*)")];
        // Seen in a real replay: the prompt was for the check script, not
        // for git, which Workbench already allows.
        assert_eq!(asked_key(&rules, "git status --short; git stash list; ./scripts/check.sh", "/w").as_deref(), Some("./scripts/check.sh"));
        assert_eq!(asked_key(&rules, "git stash -q && ./scripts/check.sh; git stash pop", "/w").as_deref(), Some("./scripts/check.sh"));
        assert_eq!(asked_key(&rules, "cd sub && make lint", "/w").as_deref(), Some("make lint"));
        assert_eq!(asked_key(&[], "head -5 a.py; ./scripts/check.sh", "/w").as_deref(), Some("head -5"));
    }

    fn keys(v: &[Json]) -> Vec<String> {
        v.iter().map(|c| c["key"].as_str().unwrap_or("").replace('\n', " ")).collect()
    }

    #[test]
    fn a_faster_model_is_tried_a_step_at_a_time_and_less_effort_beside_it() {
        let c = Case { model: Some("opus".into()), ..Case::default() };
        let mut run = Run::default();
        assert_eq!(keys(&how_changes(&run, &Edits::default(), &c)), ["model sonnet", "effort low"]);
        // The session named none: the default is Opus.
        assert_eq!(keys(&how_changes(&run, &Edits::default(), &Case::default())), ["model sonnet", "effort low"]);
        // Sonnet not better beyond the noise, but it passed: Haiku next.
        run.log.push(json!({ "change": { "key": "model\nsonnet" }, "version": "v1", "result": "not_kept", "vs_best": { "success": { "verdict": "noise", "lost": 0 } } }));
        assert_eq!(keys(&how_changes(&run, &Edits::default(), &c)), ["model haiku", "effort low"]);
        // Sonnet failed what Opus passed: no model below it either.
        run.log[0]["vs_best"]["success"] = json!({ "verdict": "worse", "lost": 1 });
        assert_eq!(keys(&how_changes(&run, &Edits::default(), &c)), ["effort low"]);
        // Low effort tried: nothing more (medium is Claude Code's default);
        // once on Haiku, no effort at all.
        run.log.push(json!({ "change": { "key": "effort\nlow" }, "version": "v2", "result": "not_kept", "vs_best": { "success": { "verdict": "worse", "lost": 1 } } }));
        assert!(how_changes(&run, &Edits::default(), &c).is_empty());
        let on_haiku = Edits { agent: super::super::AgentEdits { model: Some("haiku".into()), ..Default::default() }, ..Edits::default() };
        assert!(how_changes(&Run::default(), &on_haiku, &c).is_empty());
        let ch = &how_changes(&Run::default(), &Edits::default(), &c)[0];
        assert_eq!((ch["title"].as_str(), ch["edit"]["agent"]["model"].as_str()), (Some("Run on Sonnet instead of Opus"), Some("sonnet")), "{ch}");
    }

    #[test]
    fn changes_aimed_at_the_targets_not_met_go_first() {
        let ch = |k: &str, aims: Json| json!({ "key": k, "aims": aims });
        let all = vec![ch("allow", json!(["interventions", "wall_s"])), ch("model", json!(["cost_usd", "wall_s"])), ch("brief", json!(["turns"])), ch("pin", json!(["pass_rate", "turns"]))];
        assert_eq!(keys(&by_aims(all.clone(), &["interventions".into()])), ["allow", "model", "brief", "pin"]);
        assert_eq!(keys(&by_aims(all.clone(), &["cost_usd".into()])), ["model", "allow", "brief", "pin"]);
        assert_eq!(keys(&by_aims(all.clone(), &["cost_usd".into(), "wall_s".into()])), ["model", "allow", "brief", "pin"]);
        assert_eq!(keys(&by_aims(all, &["turns".into(), "pass_rate".into()])), ["pin", "brief", "allow", "model"]);
    }

    #[test]
    fn the_writers_answer_becomes_changes() {
        let raw = r#"{"type":"result","is_error":false,"total_cost_usd":0.042,"result":"Here:\n```json\n{\"changes\":[{\"title\":\"Run the checks once\",\"instruction\":\"Run ./scripts/check.sh once, after your last edit.\",\"aims\":[\"wall_s\",\"bogus\"],\"why\":\"it ran 3 times\"},{\"title\":\"x\",\"instruction\":\"\"}]}\n```"}"#;
        let (list, cost) = briefs_of_answer(raw);
        assert!((cost - 0.042).abs() < 1e-9);
        assert_eq!(list.len(), 1, "{list:?}");
        assert_eq!(list[0]["title"], "Tell it: Run the checks once");
        assert_eq!(list[0]["edit"]["agent"]["brief"][0], "Run ./scripts/check.sh once, after your last edit.");
        assert_eq!(list[0]["aims"], json!(["wall_s"]));
        assert!(list[0]["key"].as_str().unwrap().starts_with("brief\n"));
        assert!(briefs_of_answer("not json").0.is_empty());
        assert!(briefs_of_answer(r#"{"result":"{\"changes\":[]}"}"#).0.is_empty());
    }
}
