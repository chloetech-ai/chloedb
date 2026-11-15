//! Converge (SPEC.md, "Seventh build: Converge"): personas play the product
//! around the clock, what they hit becomes findings, and each finding walks
//! eight stages to a pull request you approve. Every stage is left through
//! a gate Workbench checks itself and records as a row of `cv_events`:
//!
//! | stage | the gate to leave it |
//! |---|---|
//! | found | no open finding says the same thing (else it is merged into it) |
//! | reproduce | its `repro.sh` fails N of N times at the base branch (default 3); its hash is locked |
//! | scope | an area, a size and an acceptance check; a one-way door (CODEOWNERS `# door:`) is flagged |
//! | prioritize | a slot under the WIP limit, budget left today; a door waits for your Start |
//! | develop | the agent's branch is committed, the locked repro passes N of N, the test command passes, a test file changed, the diff stays in scope |
//! | verify | the same persona replays its journey on the branch: the problem is gone, its ease did not drop, nothing new of severity major or worse |
//! | review | a pull request, CI green, and your approval (only your token approves) |
//! | merged | the nightly guard: the repro, run on the base branch, still passes; else it is regressed and goes back to prioritize |
//!
//! Personas, runs, findings, events, metrics and settings are rows of the
//! state database (`cv_*`). The model work (a persona playing, a scoping
//! read, a new persona) goes through one runner: the program in
//! `CHLOEDB_WORKBENCH_CONVERGE_CMD` (called with the role as its argument,
//! the job as JSON on stdin, its answer as the last JSON line of stdout;
//! tests use a script) or, by default, `claude -p` with a prompt per role.
//! Pull requests go through `gh` (`CHLOEDB_WORKBENCH_CONVERGE_GH` replaces
//! it). The autopilot (off until you turn it on per repository) runs a tick
//! every `tick_s`; `POST /api/converge/tick` runs one by hand.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::{Path as AxPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json as AxJson, Router};
use chloedb::query::Params;
use chloedb::Value;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};

use crate::db::{get_str, iso, now_ms};
use crate::levels::Levels;
use crate::server::{blocking, parse_body, ApiResult, App};
use crate::supervisor::{one_line, ApiError, SpawnRequest, Supervisor};

/// The tables (created with the others in `db.rs`). A row's columns are
/// what the board filters and sorts on; the whole record is `doc` (JSON).
pub const TABLES: [&str; 6] = [
    "CREATE TABLE IF NOT EXISTS cv_personas (id KEYWORD PRIMARY KEY, repo KEYWORD, name TEXT, state KEYWORD, origin KEYWORD, created TIMESTAMP, doc TEXT OPTIONS(indexed = false))",
    "CREATE TABLE IF NOT EXISTS cv_runs (id KEYWORD PRIMARY KEY, repo KEYWORD, persona KEYWORD, purpose KEYWORD, finding KEYWORD, state KEYWORD, outcome KEYWORD, ease FLOAT, cost_usd FLOAT, created TIMESTAMP, ended TIMESTAMP, doc TEXT OPTIONS(indexed = false))",
    "CREATE TABLE IF NOT EXISTS cv_findings (id KEYWORD PRIMARY KEY, repo KEYWORD, title TEXT, stage KEYWORD, state KEYWORD, severity INT, score FLOAT, persona KEYWORD, created TIMESTAMP, updated TIMESTAMP, doc TEXT OPTIONS(indexed = false))",
    "CREATE TABLE IF NOT EXISTS cv_events (id KEYWORD PRIMARY KEY, finding KEYWORD, repo KEYWORD, ts TIMESTAMP, from_stage KEYWORD, to_stage KEYWORD, gate KEYWORD, passed BOOL, by_who KEYWORD, summary TEXT, evidence TEXT OPTIONS(indexed = false))",
    "CREATE TABLE IF NOT EXISTS cv_metrics (id KEYWORD PRIMARY KEY, repo KEYWORD, day KEYWORD, doc TEXT OPTIONS(indexed = false))",
    "CREATE TABLE IF NOT EXISTS cv_settings (repo KEYWORD PRIMARY KEY, doc TEXT OPTIONS(indexed = false), updated TIMESTAMP)",
];

/// The kinds of problem the board groups by.
pub const THEMES: [&str; 10] = ["onboarding", "reliability", "security", "scalability", "trust", "correctness", "usability", "performance", "observability", "docs"];

/// A theme the runner or a person gave, or the closest one from the words.
pub fn theme_of(given: Option<&str>, title: &str, detail: &str) -> String {
    if let Some(g) = given.map(|g| g.trim().to_lowercase()).filter(|g| THEMES.contains(&g.as_str())) {
        return g;
    }
    let text = format!("{title} {detail}").to_lowercase();
    let rules: [(&str, &[&str]); 9] = [
        ("security", &["key", "token", "auth", "leak", "forge", "permission denied", "scope", "secret", "403"]),
        ("reliability", &["crash", "repair", "failover", "rollback", "rolls back", "lost", "primary", "deadlock", "down"]),
        ("scalability", &["shard", "cluster", "replica", "coordinator", "slot", "scale", "load"]),
        ("performance", &["slow", "latency", "timeout", "memory use", "cpu"]),
        ("onboarding", &["install", "first-run", "first run", "setup", "getting started", "readme", "quickstart", "proxy", "ca "]),
        ("trust", &["silently", "without a word", "disagree", "accepted", "not recorded", "forget"]),
        ("observability", &["timeline", "alert", "dashboard", "home says", "metric", "log"]),
        ("docs", &["doc", "guide", "example", "misleading"]),
        ("correctness", &["wrong", "incorrect", "collide", "cannot use", "stops at"]),
    ];
    for (t, words) in rules {
        if words.iter().any(|w| text.contains(w)) {
            return t.to_string();
        }
    }
    "usability".into()
}

pub const STAGES: [&str; 8] = ["found", "reproduce", "scope", "prioritize", "develop", "verify", "review", "merged"];

/// What each stage's gate checks, for the board's column headers.
pub fn gates() -> Json {
    json!([
        { "stage": "found", "name": "Found", "gate": "No open finding says the same thing" },
        { "stage": "reproduce", "name": "Reproduce", "gate": "repro.sh fails N of N at the base branch" },
        { "stage": "scope", "name": "Scope", "gate": "Area, size and acceptance check set" },
        { "stage": "prioritize", "name": "Prioritize", "gate": "A WIP slot and budget; doors wait for you" },
        { "stage": "develop", "name": "Develop", "gate": "Repro green N of N, tests pass, a test added" },
        { "stage": "verify", "name": "Verify", "gate": "The same persona succeeds on the branch" },
        { "stage": "review", "name": "Review", "gate": "CI green and your approval", "you": true },
        { "stage": "merged", "name": "Merged", "gate": "Nightly guard on the base branch" }
    ])
}

// ------------------------------------------------------------------ records

/// A repository's settings (`cv_settings`); every field has a default.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The autopilot: ticks on its own.
    pub enabled: bool,
    pub tick_s: u64,
    /// Model spend per day (persona runs, scoping, new personas).
    pub budget_usd_day: f64,
    /// Persona runs at once, and findings in development at once.
    pub runs_at_once: usize,
    pub wip: usize,
    /// How often each persona replays (hours).
    pub replay_every_h: f64,
    pub generate: bool,
    pub retire: bool,
    /// The model of persona runs, scoping and the generator.
    pub model: String,
    /// The model of development agents (none: Claude Code's default).
    pub dev_model: Option<String>,
    /// Run in the agent's worktree by the develop gate (e.g. `cargo test --profile ci`).
    pub test_cmd: Option<String>,
    /// The branch findings are reproduced on and fixes merge into (none: the main checkout's).
    pub base: Option<String>,
    /// How many runs the repro must fail (reproduce) and pass (develop).
    pub repro_runs: u32,
    pub repro_timeout_s: u64,
    pub run_timeout_s: u64,
    /// Develop and verify rounds before it asks you.
    pub max_rounds: i64,
    /// Open a pull request at review (else you record its URL).
    pub open_pr: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            enabled: false,
            tick_s: 300,
            budget_usd_day: 20.0,
            runs_at_once: 2,
            wip: 2,
            replay_every_h: 6.0,
            generate: true,
            retire: true,
            model: "haiku".into(),
            dev_model: None,
            test_cmd: None,
            base: None,
            repro_runs: 3,
            repro_timeout_s: 300,
            run_timeout_s: 3600,
            max_rounds: 3,
            open_pr: true,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Scope {
    pub area: String,
    pub files: Vec<String>,
    pub root_cause: String,
    pub acceptance: String,
    /// S, M or L.
    pub size: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Finding {
    pub id: String,
    pub repo: String,
    pub title: String,
    pub detail: String,
    /// bug, gap, docs, ux, opportunity
    pub kind: String,
    /// The kind of problem, for grouping: one of [`THEMES`].
    pub theme: String,
    /// 4 blocker, 3 major, 2 minor, 1 papercut
    pub severity: i64,
    pub stage: String,
    /// active, parked, dismissed, duplicate, cant_reproduce
    pub state: String,
    pub persona: Option<String>,
    pub run: Option<String>,
    pub personas_hit: Vec<String>,
    pub occurrences: i64,
    pub step: Option<String>,
    pub repro: Option<String>,
    pub repro_hash: Option<String>,
    pub repro_attempts: i64,
    pub scope: Option<Scope>,
    pub door: Option<String>,
    pub ease_before: Option<f64>,
    pub ease_after: Option<f64>,
    pub score: f64,
    pub pinned: bool,
    pub agent: Option<String>,
    pub branch: Option<String>,
    pub worktree: Option<String>,
    pub rounds: i64,
    pub pr_url: Option<String>,
    /// pending, pass, fail
    pub ci: Option<String>,
    pub approved_by: Option<String>,
    pub guard_ok: Option<bool>,
    pub guard_last: Option<i64>,
    /// Why a person is needed now (shown as "Needs you").
    pub needs_you: Option<String>,
    pub dup_of: Option<String>,
    pub note: Option<String>,
    pub by: String,
    pub created: i64,
    pub updated: i64,
    pub stage_since: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Persona {
    pub id: String,
    pub repo: String,
    pub name: String,
    pub role: String,
    pub traits: Vec<String>,
    pub journey: String,
    pub surfaces: Vec<String>,
    /// seed, generated, you
    pub origin: String,
    /// active, probation, retired
    pub state: String,
    pub why: Option<String>,
    pub runs: i64,
    pub reported: i64,
    pub reproduced: i64,
    pub stale_runs: i64,
    pub ease: Option<f64>,
    pub ease_hist: Vec<f64>,
    pub last_run: Option<i64>,
    pub created: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Run {
    pub id: String,
    pub repo: String,
    pub persona: Option<String>,
    /// explore, verify, scope, generate
    pub purpose: String,
    pub finding: Option<String>,
    pub commit: Option<String>,
    /// running, done, failed
    pub state: String,
    pub outcome: Option<String>,
    pub ease: Option<f64>,
    pub answer: Option<String>,
    pub findings: Vec<Json>,
    pub fixed: Option<bool>,
    pub error: Option<String>,
    pub cost_usd: f64,
    pub created: i64,
    pub ended: Option<i64>,
}

fn s(v: &str) -> Value {
    Value::Str(v.to_string())
}

fn os(v: &Option<String>) -> Value {
    v.as_ref().map_or(Value::Null, |x| Value::Str(x.clone()))
}

fn t(ms: i64) -> Value {
    crate::db::ts(ms)
}

fn docs<T: serde::de::DeserializeOwned>(sup: &Supervisor, sql: &str, p: &Params) -> Vec<T> {
    sup.db().query(sql, p).unwrap_or_default().iter().filter_map(|r| r.last().and_then(get_str)).filter_map(|d| serde_json::from_str(&d).ok()).collect()
}

fn put_finding(sup: &Supervisor, f: &Finding) -> Result<(), ApiError> {
    let p = Params::new()
        .set("id", s(&f.id))
        .set("repo", s(&f.repo))
        .set("title", s(&f.title))
        .set("stage", s(&f.stage))
        .set("state", s(&f.state))
        .set("severity", Value::Int(f.severity))
        .set("score", Value::Float(f.score))
        .set("persona", os(&f.persona))
        .set("created", t(f.created))
        .set("updated", t(f.updated))
        .set("doc", s(&serde_json::to_string(f).unwrap()));
    sup.db()
        .query("INSERT OR REPLACE INTO cv_findings (id, repo, title, stage, state, severity, score, persona, created, updated, doc) VALUES (:id, :repo, :title, :stage, :state, :severity, :score, :persona, :created, :updated, :doc)", &p)
        .map(|_| ())
        .map_err(ApiError::internal)
}

fn put_persona(sup: &Supervisor, p: &Persona) -> Result<(), ApiError> {
    let q = Params::new()
        .set("id", s(&p.id))
        .set("repo", s(&p.repo))
        .set("name", s(&p.name))
        .set("state", s(&p.state))
        .set("origin", s(&p.origin))
        .set("created", t(p.created))
        .set("doc", s(&serde_json::to_string(p).unwrap()));
    sup.db().query("INSERT OR REPLACE INTO cv_personas (id, repo, name, state, origin, created, doc) VALUES (:id, :repo, :name, :state, :origin, :created, :doc)", &q).map(|_| ()).map_err(ApiError::internal)
}

fn put_run(sup: &Supervisor, r: &Run) -> Result<(), ApiError> {
    let q = Params::new()
        .set("id", s(&r.id))
        .set("repo", s(&r.repo))
        .set("persona", os(&r.persona))
        .set("purpose", s(&r.purpose))
        .set("finding", os(&r.finding))
        .set("state", s(&r.state))
        .set("outcome", os(&r.outcome))
        .set("ease", r.ease.map_or(Value::Null, Value::Float))
        .set("cost_usd", Value::Float(r.cost_usd))
        .set("created", t(r.created))
        .set("ended", r.ended.map_or(Value::Null, t))
        .set("doc", s(&serde_json::to_string(r).unwrap()));
    sup.db()
        .query("INSERT OR REPLACE INTO cv_runs (id, repo, persona, purpose, finding, state, outcome, ease, cost_usd, created, ended, doc) VALUES (:id, :repo, :persona, :purpose, :finding, :state, :outcome, :ease, :cost_usd, :created, :ended, :doc)", &q)
        .map(|_| ())
        .map_err(ApiError::internal)
}

fn get_finding(sup: &Supervisor, id: &str) -> Result<Finding, ApiError> {
    docs::<Finding>(sup, "SELECT doc FROM cv_findings WHERE id = :id LIMIT 1", &Params::new().set("id", s(id))).into_iter().next().ok_or_else(|| ApiError::not_found(format!("no finding {id}")))
}

fn get_persona(sup: &Supervisor, id: &str) -> Result<Persona, ApiError> {
    docs::<Persona>(sup, "SELECT doc FROM cv_personas WHERE id = :id LIMIT 1", &Params::new().set("id", s(id))).into_iter().next().ok_or_else(|| ApiError::not_found(format!("no persona {id}")))
}

fn get_run(sup: &Supervisor, id: &str) -> Result<Run, ApiError> {
    docs::<Run>(sup, "SELECT doc FROM cv_runs WHERE id = :id LIMIT 1", &Params::new().set("id", s(id))).into_iter().next().ok_or_else(|| ApiError::not_found(format!("no run {id}")))
}

fn findings_of(sup: &Supervisor, repo: &str) -> Vec<Finding> {
    docs(sup, "SELECT doc FROM cv_findings WHERE repo = :repo ORDER BY created ASC LIMIT 5000", &Params::new().set("repo", s(repo)))
}

fn personas_of(sup: &Supervisor, repo: &str) -> Vec<Persona> {
    docs(sup, "SELECT doc FROM cv_personas WHERE repo = :repo ORDER BY created ASC LIMIT 500", &Params::new().set("repo", s(repo)))
}

fn runs_of(sup: &Supervisor, repo: &str, limit: usize) -> Vec<Run> {
    docs(sup, &format!("SELECT doc FROM cv_runs WHERE repo = :repo ORDER BY created DESC LIMIT {limit}"), &Params::new().set("repo", s(repo)))
}

pub fn settings(sup: &Supervisor, repo: &str) -> Settings {
    docs::<Settings>(sup, "SELECT doc FROM cv_settings WHERE repo = :repo LIMIT 1", &Params::new().set("repo", s(repo))).into_iter().next().unwrap_or_default()
}

fn put_settings(sup: &Supervisor, repo: &str, st: &Settings) -> Result<(), ApiError> {
    let p = Params::new().set("repo", s(repo)).set("doc", s(&serde_json::to_string(st).unwrap())).set("updated", t(now_ms()));
    sup.db().query("INSERT OR REPLACE INTO cv_settings (repo, doc, updated) VALUES (:repo, :doc, :updated)", &p).map(|_| ()).map_err(ApiError::internal)
}

/// The repositories with settings: the ones the autopilot looks at.
fn repos(sup: &Supervisor) -> Vec<(String, Settings)> {
    sup.db()
        .query("SELECT repo, doc FROM cv_settings LIMIT 100", &Params::new())
        .unwrap_or_default()
        .iter()
        .filter_map(|r| Some((r.first().and_then(get_str)?, r.get(1).and_then(get_str).and_then(|d| serde_json::from_str(&d).ok()).unwrap_or_default())))
        .collect()
}

/// Records a stage change or a gate's result.
fn event(sup: &Supervisor, f: &Finding, from: &str, to: &str, gate: &str, passed: bool, by: &str, summary: &str, evidence: Json) {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let p = Params::new()
        .set("id", s(&format!("{}:{:013}:{n:04}", f.id, now_ms())))
        .set("finding", s(&f.id))
        .set("repo", s(&f.repo))
        .set("ts", t(now_ms()))
        .set("from_stage", s(from))
        .set("to_stage", s(to))
        .set("gate", s(gate))
        .set("passed", Value::Bool(passed))
        .set("by_who", s(by))
        .set("summary", s(&one_line(summary, 500)))
        .set("evidence", s(&evidence.to_string()));
    let _ = sup.db().query("INSERT INTO cv_events (id, finding, repo, ts, from_stage, to_stage, gate, passed, by_who, summary, evidence) VALUES (:id, :finding, :repo, :ts, :from_stage, :to_stage, :gate, :passed, :by_who, :summary, :evidence)", &p);
}

fn events_of(sup: &Supervisor, finding: &str) -> Vec<Json> {
    let rows = sup.db().query("SELECT ts, from_stage, to_stage, gate, passed, by_who, summary, evidence FROM cv_events WHERE finding = :f ORDER BY id ASC LIMIT 500", &Params::new().set("finding", s(finding)).set("f", s(finding))).unwrap_or_default();
    rows.iter()
        .map(|r| {
            let g = |i: usize| r.get(i).and_then(get_str);
            json!({
                "ts": r.first().and_then(crate::db::get_ms).map(iso), "ts_ms": r.first().and_then(crate::db::get_ms),
                "from": g(1), "to": g(2), "gate": g(3), "passed": matches!(r.get(4), Some(Value::Bool(true))),
                "by": g(5), "summary": g(6), "evidence": g(7).and_then(|e| serde_json::from_str::<Json>(&e).ok()).unwrap_or(Json::Null),
            })
        })
        .collect()
}

/// Moves a finding to `to`, recording the gate that let it.
fn advance_to(sup: &Supervisor, f: &mut Finding, to: &str, gate: &str, passed: bool, by: &str, summary: &str, evidence: Json) -> Result<(), ApiError> {
    let from = f.stage.clone();
    f.stage = to.to_string();
    f.updated = now_ms();
    if from != to {
        f.stage_since = f.updated;
    }
    put_finding(sup, f)?;
    event(sup, f, &from, to, gate, passed, by, summary, evidence);
    Ok(())
}

/// A gate that did not pass, with the stage kept.
fn failed(sup: &Supervisor, f: &mut Finding, gate: &str, by: &str, summary: &str, evidence: Json) -> Result<(), ApiError> {
    let st = f.stage.clone();
    f.updated = now_ms();
    put_finding(sup, f)?;
    event(sup, f, &st, &st, gate, false, by, summary, evidence);
    Ok(())
}

fn sev_name(n: i64) -> &'static str {
    match n {
        4.. => "blocker",
        3 => "major",
        2 => "minor",
        _ => "papercut",
    }
}

fn sev_of(v: &Json) -> i64 {
    match v {
        Json::Number(n) => n.as_i64().unwrap_or(2).clamp(1, 4),
        Json::String(x) => match x.to_ascii_lowercase().as_str() {
            "blocker" | "critical" => 4,
            "major" | "high" => 3,
            "minor" | "medium" => 2,
            _ => 1,
        },
        _ => 2,
    }
}

impl Finding {
    pub fn to_json(&self, sup: &Supervisor) -> Json {
        let mut v = serde_json::to_value(self).unwrap();
        v["severity_name"] = json!(sev_name(self.severity));
        v["stage_index"] = json!(STAGES.iter().position(|x| *x == self.stage).unwrap_or(0));
        v["busy"] = json!(busy_what(&self.id));
        v["created_iso"] = json!(iso(self.created));
        v["agent_state"] = json!(self.agent.as_deref().and_then(|a| sup.agent_row(a)).map(|a| a.state));
        v
    }
}

// ------------------------------------------------------------ work in flight

/// What is running now for a finding or a persona: one thing at a time.
static BUSY: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

fn busy_what(key: &str) -> Option<String> {
    BUSY.lock().unwrap().as_ref().and_then(|m| m.get(key).cloned())
}

fn take(key: &str, what: &str) -> bool {
    let mut g = BUSY.lock().unwrap();
    let m = g.get_or_insert_with(HashMap::new);
    if m.contains_key(key) {
        return false;
    }
    m.insert(key.to_string(), what.to_string());
    true
}

fn release(key: &str) {
    if let Some(m) = BUSY.lock().unwrap().as_mut() {
        m.remove(key);
    }
}

fn busy_count(prefix: &str) -> usize {
    BUSY.lock().unwrap().as_ref().map_or(0, |m| m.values().filter(|v| v.starts_with(prefix)).count())
}

/// Starts `f` on a thread holding `key`; none when the key is taken.
fn in_background(key: &str, what: &str, f: impl FnOnce() + Send + 'static) -> Option<std::thread::JoinHandle<()>> {
    if !take(key, what) {
        return None;
    }
    let key = key.to_string();
    Some(std::thread::spawn(move || {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        release(&key);
        if r.is_err() {
            eprintln!("converge: {key} panicked");
        }
    }))
}

// ------------------------------------------------------------------ programs

/// Runs a program with stdin, in `cwd`, with a deadline: (exit code, stdout, stderr).
fn run_cmd(program: &str, args: &[String], cwd: &Path, stdin: Option<&str>, envs: &[(&str, String)], timeout: Duration) -> Result<(i32, String, String), String> {
    let mut cmd = Command::new(program);
    cmd.args(args).current_dir(cwd).stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped()).env("GIT_TERMINAL_PROMPT", "0");
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().map_err(|e| format!("cannot run {program}: {e}"))?;
    if let (Some(text), Some(mut w)) = (stdin, child.stdin.take()) {
        let text = text.to_string();
        std::thread::spawn(move || {
            let _ = w.write_all(text.as_bytes());
        });
    }
    let mut out = child.stdout.take().unwrap();
    let mut err = child.stderr.take().unwrap();
    let ro = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = out.read_to_end(&mut b);
        String::from_utf8_lossy(&b).to_string()
    });
    let re = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = err.read_to_end(&mut b);
        String::from_utf8_lossy(&b).to_string()
    });
    let end = Instant::now() + timeout;
    let code = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st.code().unwrap_or(-1),
            Ok(None) if Instant::now() < end => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{program} did not finish in {} s", timeout.as_secs()));
            }
        }
    };
    Ok((code, ro.join().unwrap_or_default(), re.join().unwrap_or_default()))
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let a: Vec<String> = args.iter().map(|x| x.to_string()).collect();
    let (code, out, err) = run_cmd("git", &a, dir, None, &[], Duration::from_secs(120))?;
    if code == 0 {
        Ok(out.trim().to_string())
    } else {
        Err(format!("git {}: {}", args.join(" "), one_line(err.trim(), 300)))
    }
}

/// The tail of a long output, for evidence.
fn tail(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut i = s.len() - max;
    while !s.is_char_boundary(i) {
        i += 1;
    }
    format!("…{}", &s[i..])
}

fn base_of(repo: &Path, st: &Settings) -> String {
    st.base.clone().filter(|b| !b.is_empty()).or_else(|| crate::git::current_branch(repo)).unwrap_or_else(|| "HEAD".into())
}

/// A detached checkout of `rev` under the data dir, removed when dropped.
struct Scratch {
    repo: PathBuf,
    path: PathBuf,
}

impl Scratch {
    fn new(sup: &Supervisor, repo: &Path, rev: &str, name: &str) -> Result<Scratch, String> {
        let path = sup.cfg.data_dir.join("converge").join("checkouts").join(format!("{name}-{}", crate::supervisor::hex(4)));
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        git(repo, &["worktree", "add", "--detach", &path.to_string_lossy(), rev])?;
        Ok(Scratch { repo: repo.to_path_buf(), path })
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = git(&self.repo, &["worktree", "remove", "--force", &self.path.to_string_lossy()]);
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// The repro script's file: kept in the data dir, where an agent's
/// worktree cannot change it.
fn repro_file(sup: &Supervisor, f: &Finding) -> Result<PathBuf, String> {
    let dir = sup.cfg.data_dir.join("converge").join("findings").join(&f.id);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let p = dir.join("repro.sh");
    std::fs::write(&p, f.repro.as_deref().unwrap_or("")).map_err(|e| e.to_string())?;
    Ok(p)
}

fn repro_hash(text: &str) -> String {
    // FNV-1a over the bytes: the lock is against changes, not attacks
    // (the script lives where agents cannot write).
    let mut h: u64 = 0xcbf29ce484222325;
    for b in text.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

/// Runs the repro `n` times in `cwd`: each run's exit code and output tail.
fn run_repro(sup: &Supervisor, f: &Finding, cwd: &Path, n: u32, timeout: u64) -> Vec<(i32, String)> {
    let file = match repro_file(sup, f) {
        Ok(p) => p,
        Err(e) => return vec![(-1, e)],
    };
    (0..n)
        .map(|_| {
            let envs = [("CV_FINDING", f.id.clone()), ("CV_REPO", f.repo.clone())];
            match run_cmd("sh", &[file.to_string_lossy().to_string()], cwd, None, &envs, Duration::from_secs(timeout)) {
                Ok((code, out, err)) => (code, tail(&format!("{out}{err}"), 1500)),
                Err(e) => (-1, e),
            }
        })
        .collect()
}

// -------------------------------------------------------------- the runner

/// Asks the runner for `role` (play, scope, generate): its answer and cost.
fn runner(sup: &Supervisor, role: &str, job: &Json, cwd: &Path, model: &str, timeout: u64) -> Result<Json, String> {
    if let Some(cmd) = std::env::var_os("CHLOEDB_WORKBENCH_CONVERGE_CMD").filter(|c| !c.is_empty()) {
        let (code, out, err) = run_cmd(&cmd.to_string_lossy(), &[role.to_string()], cwd, Some(&job.to_string()), &[], Duration::from_secs(timeout))?;
        let last = out.lines().rev().find_map(|l| serde_json::from_str::<Json>(l.trim()).ok().filter(Json::is_object));
        return match last {
            Some(v) => Ok(v),
            None => Err(format!("the runner ({role}) printed no JSON (exit {code}): {}", one_line(&tail(&format!("{out}{err}"), 400), 400))),
        };
    }
    let prompt = prompt_for(role, job);
    let mut args: Vec<String> = vec!["-p".into(), "--output-format".into(), "json".into(), "--model".into(), model.to_string()];
    match role {
        "play" => args.extend(["--allowedTools".into(), "Bash Read Write Edit Glob Grep WebFetch".into()]),
        _ => args.extend(["--allowedTools".into(), "Read Glob Grep".into()]),
    }
    // The prompt on stdin: --allowedTools takes every word after it.
    let (code, out, err) = run_cmd(&sup.cfg.claude_bin, &args, cwd, Some(&prompt), &[("CHLOECTX_DISABLE", "1".to_string())], Duration::from_secs(timeout))?;
    let outer: Json = serde_json::from_str(out.trim()).map_err(|_| format!("claude -p (exit {code}) answered no JSON: {}", one_line(&tail(&format!("{out}{err}"), 400), 400)))?;
    let text = outer["result"].as_str().unwrap_or("");
    let mut inner = extract_json(text).ok_or_else(|| format!("the {role} answer held no JSON object: {}", one_line(text, 300)))?;
    inner["cost_usd"] = json!(outer["total_cost_usd"].as_f64().unwrap_or(0.0));
    Ok(inner)
}

/// The last balanced `{…}` of a model's text.
fn extract_json(text: &str) -> Option<Json> {
    let end = text.rfind('}')?;
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut i = end as i64;
    while i >= 0 {
        match bytes[i as usize] {
            b'}' => depth += 1,
            b'{' => {
                depth -= 1;
                if depth == 0 {
                    return serde_json::from_str(&text[i as usize..=end]).ok();
                }
            }
            _ => {}
        }
        i -= 1;
    }
    None
}

fn prompt_for(role: &str, job: &Json) -> String {
    let p = &job["persona"];
    match role {
        "play" => {
            let verify = match job["finding"].as_object() {
                Some(_) => format!(
                    "\n\nYou played this before and hit a problem: \"{}\" ({}). Replay the same journey on this checkout and say whether that problem is gone: set \"fixed\" to true only if what went wrong before now works.",
                    job["finding"]["title"].as_str().unwrap_or(""),
                    job["finding"]["detail"].as_str().unwrap_or("")
                ),
                None => String::new(),
            };
            format!(
                "You are {name}, {role}. {traits}\n\nYou are a user of the software in the current directory (a checkout of its repository). Your journey: {journey}{verify}\n\nDo it the way this person would: read the README and the docs, build or run what you need, try things. You are a user, not a developer of it: do not change its source to make things work; work around problems as a user would and remember each one.\n\nWhen you are done, or stuck, end your answer with one JSON object and nothing after it:\n{{\"outcome\": \"done\" or \"gave_up\", \"ease\": 1 to 5 (how easy it was for you, 5 easiest), \"answer\": \"what you achieved, in two sentences\", \"fixed\": true or false (only when replaying a problem), \"findings\": [{{\"title\": \"one line\", \"detail\": \"what you expected, what happened\", \"kind\": \"bug\" or \"gap\" or \"docs\" or \"ux\" or \"opportunity\", \"theme\": \"onboarding\" or \"reliability\" or \"security\" or \"scalability\" or \"trust\" or \"correctness\" or \"usability\" or \"performance\" or \"observability\" or \"docs\", \"severity\": \"blocker\" or \"major\" or \"minor\" or \"papercut\", \"step\": \"where in the journey\", \"repro\": \"a POSIX sh script, run from the checkout's root, that exits non-zero while this problem is there and 0 once it is fixed (empty when you cannot write one)\"}}]}}",
                name = p["name"].as_str().unwrap_or("a user"),
                role = p["role"].as_str().unwrap_or(""),
                traits = p["traits"].as_array().map(|a| a.iter().filter_map(Json::as_str).collect::<Vec<_>>().join(" ")).unwrap_or_default(),
                journey = p["journey"].as_str().unwrap_or("try the product's main use case"),
            )
        }
        "scope" => format!(
            "A user of the software in the current directory hit this problem:\n\nTitle: {}\nDetail: {}\nRepro script (fails while the problem is there):\n{}\n\nRead the code (do not change anything) and scope the fix. End your answer with one JSON object and nothing after it:\n{{\"area\": \"the component or folder\", \"files\": [\"paths a fix likely changes, including where its test goes\"], \"root_cause\": \"one or two sentences\", \"acceptance\": \"what must be true when it is fixed (the repro exits 0, plus ...)\", \"size\": \"S\" or \"M\" or \"L\"}}",
            job["finding"]["title"].as_str().unwrap_or(""),
            job["finding"]["detail"].as_str().unwrap_or(""),
            job["finding"]["repro"].as_str().unwrap_or("(none)")
        ),
        _ => format!(
            "You design test personas for the software in the current directory. These personas exist:\n{}\n\nHow often each part of the product was played lately (runs in 7 days):\n{}\n\nRead the README and docs (change nothing), then make ONE new persona aimed at the least-played part that real users would use. End your answer with one JSON object and nothing after it:\n{{\"name\": \"first name and role, e.g. Omar, data engineer\", \"role\": \"who they are and what they want, one sentence\", \"traits\": [\"two or three short traits\"], \"journey\": \"the task they try, end to end, in three sentences\", \"surfaces\": [\"the parts of the product it touches\"], \"why\": \"why this persona, from the coverage\"}}",
            job["personas"],
            job["coverage"]
        ),
    }
}

// ------------------------------------------------------------- the stages

/// Words of a title and detail, for the duplicate check.
fn words(x: &str) -> HashSet<String> {
    const STOP: [&str; 24] = ["the", "a", "an", "and", "or", "of", "to", "in", "on", "is", "it", "its", "for", "with", "not", "no", "be", "are", "that", "this", "from", "as", "by", "at"];
    x.split(|c: char| !c.is_alphanumeric() && c != '_').map(|w| w.to_lowercase()).filter(|w| w.len() > 1 && !STOP.contains(&w.as_str())).collect()
}

fn similarity(a: &Finding, b: &Finding) -> f64 {
    let (ta, tb) = (words(&a.title), words(&b.title));
    let (da, db) = (words(&format!("{} {}", a.title, a.detail)), words(&format!("{} {}", b.title, b.detail)));
    let j = |x: &HashSet<String>, y: &HashSet<String>| if x.is_empty() || y.is_empty() { 0.0 } else { x.intersection(y).count() as f64 / x.union(y).count() as f64 };
    j(&ta, &tb).max(j(&da, &db))
}

const DUP: f64 = 0.6;

/// Found: merged into an open finding that says the same thing, else on to reproduce.
fn gate_found(sup: &Supervisor, f: &mut Finding) -> Result<(), ApiError> {
    let others: Vec<Finding> = findings_of(sup, &f.repo).into_iter().filter(|o| o.id != f.id && o.state == "active" && o.created <= f.created).collect();
    let best = others.iter().map(|o| (similarity(f, o), o)).max_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    if let Some((sim, o)) = best.filter(|(sim, _)| *sim >= DUP) {
        let mut target = o.clone();
        target.occurrences += f.occurrences.max(1);
        for p in &f.personas_hit {
            if !target.personas_hit.contains(p) {
                target.personas_hit.push(p.clone());
            }
        }
        target.severity = target.severity.max(f.severity);
        target.updated = now_ms();
        put_finding(sup, &target)?;
        event(sup, &target, &target.stage, &target.stage, "dedup", true, "autopilot", &format!("{} was merged in (similarity {sim:.2})", f.id), json!({ "merged": f.id, "similarity": sim }));
        f.state = "duplicate".into();
        f.dup_of = Some(target.id.clone());
        return failed(sup, f, "dedup", "autopilot", &format!("The same as {} ({}, similarity {sim:.2})", target.id, target.title), json!({ "duplicate_of": target.id, "similarity": sim }));
    }
    let best_sim = best.map(|b| b.0).unwrap_or(0.0);
    advance_to(sup, f, "reproduce", "dedup", true, "autopilot", &format!("No open finding says the same (best match {best_sim:.2})"), json!({ "best_similarity": best_sim }))
}

/// Reproduce: the repro must fail every run at the base branch.
fn gate_reproduce(sup: &Arc<Supervisor>, id: &str) -> Result<(), ApiError> {
    let mut f = get_finding(sup, id)?;
    let repo = PathBuf::from(&f.repo);
    let st = settings(sup, &f.repo);
    let Some(text) = f.repro.clone().filter(|r| !r.trim().is_empty()) else {
        f.needs_you = Some("Needs a repro: a script that fails while the problem is there. Paste one, or ask an agent to write it.".into());
        f.updated = now_ms();
        return put_finding(sup, &f);
    };
    let base = base_of(&repo, &st);
    let rev = crate::git::rev_parse(&repo, &base).unwrap_or_else(|| base.clone());
    let scratch = Scratch::new(sup, &repo, &rev, &f.id).map_err(ApiError::internal)?;
    let runs = run_repro(sup, &f, &scratch.path, st.repro_runs.max(1), st.repro_timeout_s);
    drop(scratch);
    let fails = runs.iter().filter(|(c, _)| *c != 0).count();
    let n = runs.len();
    let evidence = json!({ "commit": rev, "base": base, "runs": runs.iter().map(|(c, o)| json!({ "exit": c, "output": o })).collect::<Vec<_>>() });
    f.repro_attempts += 1;
    if fails == n {
        f.repro_hash = Some(repro_hash(&text));
        f.needs_you = None;
        // The persona's report counts as reproduced.
        if let Some(p) = f.persona.clone() {
            if let Ok(mut pe) = get_persona(sup, &p) {
                pe.reproduced += 1;
                let _ = put_persona(sup, &pe);
            }
        }
        return advance_to(sup, &mut f, "scope", "repro_fails", true, "autopilot", &format!("repro.sh failed {fails} of {n} runs at {} ({}); its hash is locked", base, &rev[..rev.len().min(10)]), evidence);
    }
    if f.repro_attempts >= 2 {
        f.state = "cant_reproduce".into();
        f.needs_you = None;
        let msg = format!("Can't reproduce: repro.sh failed {fails} of {n} runs (attempt {}); out of the loop", f.repro_attempts);
        return failed(sup, &mut f, "repro_fails", "autopilot", &msg, evidence);
    }
    failed(sup, &mut f, "repro_fails", "autopilot", &format!("repro.sh failed only {fails} of {n} runs at {base}; one more attempt"), evidence)
}

/// The CODEOWNERS `# door:` sections' paths that the scope touches.
fn door_of(repo: &Path, scope: &Scope) -> Option<String> {
    let text = std::fs::read_to_string(repo.join(".github/CODEOWNERS")).ok()?;
    let mut door: Option<String> = None;
    let paths: Vec<String> = scope.files.iter().cloned().chain(std::iter::once(scope.area.clone())).filter(|p| !p.is_empty()).map(|p| p.trim_start_matches('/').to_string()).collect();
    for line in text.lines() {
        let l = line.trim();
        if let Some(name) = l.strip_prefix("# door:") {
            door = Some(name.trim().to_string());
            continue;
        }
        if l.is_empty() {
            door = None;
            continue;
        }
        if l.starts_with('#') {
            continue;
        }
        if let (Some(d), Some(pat)) = (door.as_ref(), l.split_whitespace().next()) {
            let pat = pat.trim_start_matches('/').trim_end_matches('*').trim_end_matches("**");
            if !pat.is_empty() && paths.iter().any(|p| p.starts_with(pat) || pat.starts_with(p.as_str())) {
                return Some(d.clone());
            }
        }
    }
    None
}

fn scope_ok(sc: &Scope) -> Result<(), String> {
    if sc.area.trim().is_empty() {
        return Err("the scope has no area".into());
    }
    if sc.acceptance.trim().is_empty() {
        return Err("the scope has no acceptance check".into());
    }
    if !matches!(sc.size.as_str(), "S" | "M" | "L") {
        return Err(format!("size is S, M or L, not {:?}", sc.size));
    }
    Ok(())
}

/// Scope: complete it with the runner when it is missing, then check it.
fn gate_scope(sup: &Arc<Supervisor>, id: &str) -> Result<(), ApiError> {
    let mut f = get_finding(sup, id)?;
    let st = settings(sup, &f.repo);
    let repo = PathBuf::from(&f.repo);
    if f.scope.as_ref().map(|s| scope_ok(s).is_err()).unwrap_or(true) {
        if spent_today(sup, &f.repo) >= st.budget_usd_day {
            return Ok(());
        }
        let mut run = Run { id: crate::levels::new_id("cr"), repo: f.repo.clone(), purpose: "scope".into(), finding: Some(f.id.clone()), state: "running".into(), created: now_ms(), ..Default::default() };
        put_run(sup, &run)?;
        let job = json!({ "finding": { "id": f.id, "title": f.title, "detail": f.detail, "repro": f.repro } });
        match runner(sup, "scope", &job, &repo, &st.model, 900) {
            Ok(v) => {
                run.cost_usd = v["cost_usd"].as_f64().unwrap_or(0.0);
                run.state = "done".into();
                let mut sc: Scope = serde_json::from_value(v.clone()).unwrap_or_default();
                sc.size = sc.size.trim().to_uppercase();
                if sc.acceptance.trim().is_empty() {
                    sc.acceptance = "The locked repro exits 0".into();
                }
                f.scope = Some(sc);
            }
            Err(e) => {
                run.state = "failed".into();
                run.error = Some(e.clone());
                run.ended = Some(now_ms());
                put_run(sup, &run)?;
                return failed(sup, &mut f, "scope", "autopilot", &format!("Scoping failed: {e}"), json!({ "error": e }));
            }
        }
        run.ended = Some(now_ms());
        put_run(sup, &run)?;
    }
    let sc = f.scope.clone().unwrap_or_default();
    if let Err(e) = scope_ok(&sc) {
        f.needs_you = Some(format!("Scope incomplete: {e}"));
        return failed(sup, &mut f, "scope", "autopilot", &e, json!({ "scope": sc }));
    }
    f.door = door_of(&repo, &sc);
    f.needs_you = None;
    f.score = score(&f);
    let door = f.door.clone();
    advance_to(sup, &mut f, "prioritize", "scope", true, "autopilot", &format!("Scoped: {} (size {}){}", sc.area, sc.size, door.as_ref().map(|d| format!("; one-way door: {d}")).unwrap_or_default()), json!({ "scope": sc, "door": door }))
}

/// severity weight × reach × (1 + ease lost) ÷ size.
pub fn score(f: &Finding) -> f64 {
    let sev = match f.severity {
        4.. => 8.0,
        3 => 4.0,
        2 => 2.0,
        _ => 1.0,
    };
    let distinct = f.personas_hit.len().max(1) as f64;
    let reach = distinct + (f.occurrences as f64 - distinct).max(0.0) / 2.0;
    let lost = f.ease_before.map(|e| (5.0 - e).max(0.0) / 4.0).unwrap_or(0.5);
    let size = match f.scope.as_ref().map(|s| s.size.as_str()) {
        Some("S") => 1.0,
        Some("L") => 4.0,
        _ => 2.0,
    };
    ((sev * reach * (1.0 + lost) / size) * 10.0).round() / 10.0
}

fn dev_task(f: &Finding) -> String {
    let sc = f.scope.clone().unwrap_or_default();
    format!(
        "Fix this problem a user hit (Workbench Converge, finding {id}, severity {sev}).\n\nProblem: {title}\n{detail}\n\nScope: {area}. Likely files: {files}. Root cause: {cause}\nAcceptance: {acc}\n\nThis script fails now and must exit 0 when you are done (run it from the repository root with sh; do not change it, Workbench keeps its own copy):\n{repro}\n\nAdd a regression test, keep the change to the scope, run the tests, and commit your work on this branch. Then stop: Workbench checks the repro, the tests and the diff, and the persona who found it replays the journey.",
        id = f.id,
        sev = sev_name(f.severity),
        title = f.title,
        detail = f.detail,
        area = sc.area,
        files = if sc.files.is_empty() { "(not named)".into() } else { sc.files.join(", ") },
        cause = sc.root_cause,
        acc = sc.acceptance,
        repro = f.repro.clone().unwrap_or_default()
    )
}

/// Prioritize → develop: an agent on its own branch.
fn start_develop(sup: &Arc<Supervisor>, lv: &Arc<Levels>, f: &mut Finding, by: &str) -> Result<(), ApiError> {
    let st = settings(sup, &f.repo);
    let repo = PathBuf::from(&f.repo);
    let short = f.id.trim_start_matches("cv").chars().take(8).collect::<String>();
    let name = format!("cv-{short}");
    let branch = format!("wb/cv-{short}");
    let req = SpawnRequest {
        repo: f.repo.clone(),
        task: Some(dev_task(f)),
        name: Some(name),
        branch: Some(branch.clone()),
        base: Some(base_of(&repo, &st)),
        worktree: Some(true),
        model: st.dev_model.clone(),
        ..Default::default()
    };
    let a = crate::goals::spawn(sup, lv, req)?;
    let aid = a["id"].as_str().unwrap_or("").to_string();
    f.agent = Some(aid.clone());
    f.branch = Some(branch.clone());
    f.worktree = sup.agent_row(&aid).map(|r| r.cwd);
    f.rounds = 0;
    f.needs_you = None;
    advance_to(sup, f, "develop", "slot", true, by, &format!("Agent {aid} started on {branch}"), json!({ "agent": aid, "branch": branch }))
}

/// Whether the agent's turn is over (idle or exited) since `since`.
fn agent_done(sup: &Supervisor, aid: &str) -> bool {
    sup.agent_row(aid).is_some_and(|a| matches!(a.state.as_str(), "idle" | "exited"))
}

fn steer_back(sup: &Arc<Supervisor>, lv: &Arc<Levels>, f: &mut Finding, text: &str) {
    f.rounds += 1;
    if let Some(a) = f.agent.clone() {
        if sup.is_running(&a) {
            let _ = crate::levels::steer(sup, lv, &a, text);
        } else {
            f.needs_you = Some("The agent has exited: restart it or take the branch over.".into());
        }
    }
    let st = settings(sup, &f.repo);
    if f.rounds >= st.max_rounds {
        f.needs_you = Some(format!("{} rounds without passing: look at the branch, steer the agent, or park it.", f.rounds));
    }
}

/// Documentation: Markdown, text, and what is under docs/.
fn is_doc(path: &str) -> bool {
    let l = path.to_lowercase();
    l.ends_with(".md") || l.ends_with(".txt") || l.starts_with("docs/") || l.contains("/docs/")
}

/// Develop's gate: committed, repro green N of N, tests, a test changed, in scope.
fn gate_develop(sup: &Arc<Supervisor>, lv: &Arc<Levels>, id: &str) -> Result<(), ApiError> {
    let mut f = get_finding(sup, id)?;
    let st = settings(sup, &f.repo);
    let repo = PathBuf::from(&f.repo);
    let Some(wt) = f.worktree.clone().map(PathBuf::from).filter(|p| p.is_dir()) else {
        f.needs_you = Some("The agent's worktree is gone.".into());
        return put_finding(sup, &f);
    };
    let base = base_of(&repo, &st);
    let mut problems: Vec<String> = Vec::new();
    let dirty = git(&wt, &["status", "--porcelain"]).unwrap_or_default();
    if !dirty.trim().is_empty() {
        problems.push(format!("uncommitted changes: commit your work ({})", one_line(&dirty.replace('\n', ", "), 200)));
    }
    let changed: Vec<String> = git(&wt, &["diff", "--name-only", &format!("{base}...HEAD")]).unwrap_or_default().lines().map(str::to_string).filter(|l| !l.is_empty()).collect();
    if changed.is_empty() {
        problems.push(format!("no commits on the branch past {base}"));
    }
    if f.repro_hash.as_deref() != f.repro.as_deref().map(repro_hash).as_deref() {
        problems.push("the repro changed after it was locked: it goes back to reproduce".into());
    }
    let runs = run_repro(sup, &f, &wt, st.repro_runs.max(1), st.repro_timeout_s);
    let green = runs.iter().filter(|(c, _)| *c == 0).count();
    if green != runs.len() {
        problems.push(format!("the repro passed {green} of {} runs: {}", runs.len(), one_line(&runs.iter().find(|(c, _)| *c != 0).map(|r| r.1.clone()).unwrap_or_default(), 300)));
    }
    let tests = match st.test_cmd.as_deref().filter(|c| !c.trim().is_empty()) {
        Some(cmd) => match run_cmd("sh", &["-c".into(), cmd.to_string()], &wt, None, &[], Duration::from_secs(st.run_timeout_s)) {
            Ok((0, out, err)) => json!({ "cmd": cmd, "exit": 0, "output": tail(&format!("{out}{err}"), 1200) }),
            Ok((code, out, err)) => {
                problems.push(format!("the tests failed (exit {code}): {}", one_line(&tail(&format!("{out}{err}"), 300), 300)));
                json!({ "cmd": cmd, "exit": code, "output": tail(&format!("{out}{err}"), 2000) })
            }
            Err(e) => {
                problems.push(e.clone());
                json!({ "cmd": cmd, "error": e })
            }
        },
        None => json!({ "cmd": null, "note": "no test command is set for this repository (Settings)" }),
    };
    // A change only to documentation has no test to add: its locked repro
    // is the regression check, and the guard keeps running it.
    let docs_only = !changed.is_empty() && changed.iter().all(|p| is_doc(p));
    if !changed.is_empty() && !docs_only && !changed.iter().any(|p| p.to_lowercase().contains("test")) {
        problems.push("no test file changed: add a regression test".into());
    }
    let files = f.scope.as_ref().map(|s| s.files.clone()).unwrap_or_default();
    let outside: Vec<&String> = if files.is_empty() { Vec::new() } else { changed.iter().filter(|c| !files.iter().any(|p| c.starts_with(p.trim_end_matches('*')) || c.to_lowercase().contains("test"))).collect() };
    if !outside.is_empty() {
        problems.push(format!("changed outside the scope: {}", outside.iter().map(|x| x.as_str()).collect::<Vec<_>>().join(", ")));
    }
    let evidence = json!({ "base": base, "changed": changed, "docs_only": docs_only, "repro": runs.iter().map(|(c, o)| json!({ "exit": c, "output": o })).collect::<Vec<_>>(), "tests": tests, "round": f.rounds + 1 });
    if f.repro_hash.as_deref() != f.repro.as_deref().map(repro_hash).as_deref() {
        f.repro_hash = None;
        f.repro_attempts = 0;
        return advance_to(sup, &mut f, "reproduce", "develop", false, "autopilot", "The repro changed after it was locked: reproduce it again", evidence);
    }
    if problems.is_empty() {
        f.needs_you = None;
        return advance_to(sup, &mut f, "verify", "develop", true, "autopilot", &format!("Repro green {green} of {green}; tests pass; {} files changed", changed.len()), evidence);
    }
    let text = format!("Workbench's check of your branch did not pass (round {}):\n- {}\nFix these, commit, and stop.", f.rounds + 1, problems.join("\n- "));
    steer_back(sup, lv, &mut f, &text);
    failed(sup, &mut f, "develop", "autopilot", &problems.join("; "), evidence)
}

/// Plays one persona: explore at the base branch, or verify a finding on its branch.
fn play(sup: &Arc<Supervisor>, lv: &Arc<Levels>, persona_id: &str, finding: Option<&str>) -> Result<Run, ApiError> {
    let mut pe = get_persona(sup, persona_id)?;
    let st = settings(sup, &pe.repo);
    let repo = PathBuf::from(&pe.repo);
    let f = finding.map(|id| get_finding(sup, id)).transpose()?;
    let rev = match f.as_ref().and_then(|f| f.worktree.clone()) {
        Some(wt) => git(Path::new(&wt), &["rev-parse", "HEAD"]).map_err(ApiError::internal)?,
        None => {
            let base = base_of(&repo, &st);
            crate::git::rev_parse(&repo, &base).unwrap_or(base)
        }
    };
    let mut run = Run {
        id: crate::levels::new_id("cr"),
        repo: pe.repo.clone(),
        persona: Some(pe.id.clone()),
        purpose: if f.is_some() { "verify".into() } else { "explore".into() },
        finding: f.as_ref().map(|f| f.id.clone()),
        commit: Some(rev.clone()),
        state: "running".into(),
        created: now_ms(),
        ..Default::default()
    };
    put_run(sup, &run)?;
    let scratch = Scratch::new(sup, &repo, &rev, &run.id).map_err(ApiError::internal)?;
    let job = json!({
        "persona": { "id": pe.id, "name": pe.name, "role": pe.role, "traits": pe.traits, "journey": pe.journey, "surfaces": pe.surfaces },
        "purpose": run.purpose, "commit": rev,
        "finding": f.as_ref().map(|f| json!({ "id": f.id, "title": f.title, "detail": f.detail, "step": f.step })),
    });
    let answer = runner(sup, "play", &job, &scratch.path, &st.model, st.run_timeout_s);
    drop(scratch);
    run.ended = Some(now_ms());
    pe.last_run = Some(now_ms());
    match answer {
        Err(e) => {
            run.state = "failed".into();
            run.error = Some(e);
            put_run(sup, &run)?;
            put_persona(sup, &pe)?;
            return Ok(run);
        }
        Ok(v) => {
            run.state = "done".into();
            run.outcome = v["outcome"].as_str().map(str::to_string);
            run.ease = v["ease"].as_f64().map(|e| e.clamp(1.0, 5.0));
            run.answer = v["answer"].as_str().map(|a| one_line(a, 2000));
            run.fixed = v["fixed"].as_bool();
            run.findings = v["findings"].as_array().cloned().unwrap_or_default().into_iter().take(20).collect();
            run.cost_usd = v["cost_usd"].as_f64().unwrap_or(0.0);
        }
    }
    put_run(sup, &run)?;
    pe.runs += 1;
    if let Some(e) = run.ease {
        pe.ease = Some(e);
        pe.ease_hist.push(e);
        if pe.ease_hist.len() > 60 {
            pe.ease_hist.remove(0);
        }
    }
    if run.purpose == "explore" {
        let mut new = 0;
        for x in &run.findings {
            let title = x["title"].as_str().unwrap_or("").trim().to_string();
            if title.is_empty() {
                continue;
            }
            pe.reported += 1;
            let mut nf = Finding {
                id: crate::levels::new_id("cv"),
                repo: pe.repo.clone(),
                title: one_line(&title, 200),
                detail: x["detail"].as_str().unwrap_or("").to_string(),
                kind: x["kind"].as_str().unwrap_or("bug").to_string(),
                theme: theme_of(x["theme"].as_str(), &title, x["detail"].as_str().unwrap_or("")),
                severity: sev_of(&x["severity"]),
                stage: "found".into(),
                state: "active".into(),
                persona: Some(pe.id.clone()),
                run: Some(run.id.clone()),
                personas_hit: vec![pe.id.clone()],
                occurrences: 1,
                step: x["step"].as_str().map(str::to_string),
                repro: x["repro"].as_str().filter(|r| !r.trim().is_empty()).map(str::to_string),
                ease_before: run.ease,
                by: pe.id.clone(),
                created: now_ms(),
                updated: now_ms(),
                stage_since: now_ms(),
                ..Default::default()
            };
            nf.score = score(&nf);
            put_finding(sup, &nf)?;
            event(sup, &nf, "", "found", "reported", true, &pe.id, &format!("Found by {} in run {}", pe.name, run.id), json!({ "run": run.id, "step": nf.step }));
            let dup = findings_of(sup, &pe.repo).iter().any(|o| o.id != nf.id && o.state == "active" && similarity(&nf, o) >= DUP);
            if !dup {
                new += 1;
            }
        }
        pe.stale_runs = if new == 0 { pe.stale_runs + 1 } else { 0 };
        review_persona(sup, &st, &mut pe);
    }
    put_persona(sup, &pe)?;
    if let Some(mut f) = f {
        finish_verify(sup, lv, &mut f, &run)?;
    }
    Ok(run)
}

/// Probation and retirement, after each explore run.
fn review_persona(sup: &Supervisor, st: &Settings, pe: &mut Persona) {
    // The repro rate counts only findings whose reproduce gate has decided.
    let cant = findings_of(sup, &pe.repo).iter().filter(|f| f.persona.as_deref() == Some(pe.id.as_str()) && f.state == "cant_reproduce").count() as i64;
    let decided = pe.reproduced as i64 + cant;
    if pe.state == "probation" && pe.runs >= 2 {
        pe.state = if pe.reproduced >= 1 { "active".into() } else { "retired".into() };
        pe.why = Some(match pe.state.as_str() {
            "active" => format!("{}; joined after {} runs with {} reproduced finding(s)", pe.why.clone().unwrap_or_default(), pe.runs, pe.reproduced),
            _ => format!("{}; retired after {} runs with no reproduced finding", pe.why.clone().unwrap_or_default(), pe.runs),
        });
    } else if pe.state == "active" && st.retire && (pe.stale_runs >= 3 || (decided >= 5 && (pe.reproduced as f64) / (decided as f64) < 0.4)) {
        pe.state = "retired".into();
        pe.why = Some(if pe.stale_runs >= 3 { format!("Retired: {} runs in a row with nothing new", pe.stale_runs) } else { format!("Retired: {} of {} findings reproduced", pe.reproduced, decided) });
    }
}

/// Verify's gate, on the persona's replay.
fn finish_verify(sup: &Arc<Supervisor>, lv: &Arc<Levels>, f: &mut Finding, run: &Run) -> Result<(), ApiError> {
    *f = get_finding(sup, &f.id)?;
    if f.stage != "verify" {
        return Ok(());
    }
    let mut problems = Vec::new();
    if run.state != "done" {
        problems.push(format!("the replay failed: {}", run.error.clone().unwrap_or_default()));
    }
    if run.outcome.as_deref() != Some("done") {
        problems.push(format!("the persona did not finish its journey ({})", run.outcome.clone().unwrap_or_else(|| "no outcome".into())));
    }
    if run.fixed != Some(true) {
        problems.push("the persona says the problem is still there".into());
    }
    if let (Some(b), Some(a)) = (f.ease_before, run.ease) {
        if a < b {
            problems.push(format!("its ease dropped from {b} to {a}"));
        }
    }
    let serious: Vec<String> = run.findings.iter().filter(|x| sev_of(&x["severity"]) >= 3).filter_map(|x| x["title"].as_str().map(str::to_string)).collect();
    if !serious.is_empty() {
        problems.push(format!("new problems of severity major or worse: {}", serious.join("; ")));
    }
    f.ease_after = run.ease;
    let evidence = json!({ "run": run.id, "outcome": run.outcome, "fixed": run.fixed, "ease_before": f.ease_before, "ease_after": run.ease, "answer": run.answer, "new_findings": run.findings });
    if problems.is_empty() {
        f.needs_you = None;
        return advance_to(sup, f, "review", "verify", true, run.persona.as_deref().unwrap_or("persona"), &format!("The persona succeeded on the branch; ease {} → {}", f.ease_before.map(|e| e.to_string()).unwrap_or("?".into()), run.ease.map(|e| e.to_string()).unwrap_or("?".into())), evidence);
    }
    let text = format!("The persona who found this replayed it on your branch and it did not pass:\n- {}\nTheir words: {}\nFix it, commit, and stop.", problems.join("\n- "), run.answer.clone().unwrap_or_default());
    steer_back(sup, lv, f, &text);
    advance_to(sup, f, "develop", "verify", false, run.persona.as_deref().unwrap_or("persona"), &problems.join("; "), evidence)
}

fn gh() -> String {
    std::env::var("CHLOEDB_WORKBENCH_CONVERGE_GH").ok().filter(|g| !g.is_empty()).unwrap_or_else(|| "gh".into())
}

/// `owner/name` of the repository's origin on GitHub: from an https or ssh
/// address, or any address whose path ends in the two (a proxy's).
fn gh_slug(repo: &Path) -> Option<String> {
    let url = git(repo, &["remote", "get-url", "origin"]).ok()?;
    let url = url.trim().trim_end_matches('/').trim_end_matches(".git");
    let path = url.rsplit_once(':').filter(|(h, _)| h.starts_with("git@")).map(|(_, p)| p.to_string()).unwrap_or_else(|| url.to_string());
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    (parts.len() >= 2).then(|| format!("{}/{}", parts[parts.len() - 2], parts[parts.len() - 1]))
}

/// `gh api` (REST, which every gh and every token can call): its JSON.
fn gh_api(cwd: &Path, args: &[String]) -> Result<Json, String> {
    let mut all = vec!["api".to_string()];
    all.extend(args.iter().cloned());
    match run_cmd(&gh(), &all, cwd, None, &[], Duration::from_secs(120))? {
        (0, out, _) => serde_json::from_str(out.trim()).map_err(|_| format!("gh api {} answered no JSON: {}", args.join(" "), one_line(&out, 200))),
        (code, out, err) => Err(format!("gh api {} failed (exit {code}): {}", args.first().cloned().unwrap_or_default(), one_line(&format!("{out}{err}"), 300))),
    }
}

/// The pull request's `owner/name` and number, from its address.
fn pr_of(url: &str) -> Option<(String, String)> {
    let rest = url.split("github.com/").nth(1)?;
    let parts: Vec<&str> = rest.split('/').collect();
    (parts.len() >= 4 && parts[2] == "pull").then(|| (format!("{}/{}", parts[0], parts[1]), parts[3].to_string()))
}

/// Review: push the branch and open its pull request.
fn open_pr(sup: &Arc<Supervisor>, id: &str) -> Result<(), ApiError> {
    let mut f = get_finding(sup, id)?;
    let st = settings(sup, &f.repo);
    let repo = PathBuf::from(&f.repo);
    let (Some(wt), Some(branch)) = (f.worktree.clone(), f.branch.clone()) else { return Ok(()) };
    let wt = PathBuf::from(wt);
    let mut notes = Vec::new();
    if git(&repo, &["remote", "get-url", "origin"]).is_ok() {
        if let Err(e) = git(&wt, &["push", "-u", "origin", &branch]) {
            f.needs_you = Some(format!("Could not push {branch}: {e}"));
            return failed(sup, &mut f, "pr", "autopilot", &format!("push failed: {e}"), json!({ "error": e }));
        }
    } else {
        notes.push("no remote named origin: nothing pushed".to_string());
    }
    let persona = f.persona.as_deref().and_then(|p| get_persona(sup, p).ok()).map(|p| format!("{} ({})", p.name, p.role)).unwrap_or_else(|| "you".into());
    let evs = events_of(sup, &f.id);
    let gates: Vec<String> = evs.iter().filter(|e| e["passed"] == true).map(|e| format!("- **{}**: {}", e["gate"].as_str().unwrap_or(""), e["summary"].as_str().unwrap_or(""))).collect();
    let body = format!(
        "Found by persona {persona} in Workbench Converge (finding `{}`, {}).\n\n{}\n\n### Gates passed\n{}\n\n### Repro (locked, now exits 0)\n```sh\n{}\n```",
        f.id,
        sev_name(f.severity),
        f.detail,
        gates.join("\n"),
        f.repro.clone().unwrap_or_default()
    );
    let Some(slug) = gh_slug(&repo) else {
        f.needs_you = Some("The repository has no origin on GitHub: open a pull request and record its URL.".into());
        return failed(sup, &mut f, "pr", "autopilot", "no GitHub origin", Json::Null);
    };
    let base = base_of(&repo, &st);
    let args: Vec<String> = vec!["-X".into(), "POST".into(), format!("repos/{slug}/pulls"), "-f".into(), format!("head={branch}"), "-f".into(), format!("base={base}"), "-f".into(), format!("title={}", f.title), "-f".into(), format!("body={body}")];
    match gh_api(&wt, &args) {
        Ok(v) if v["html_url"].is_string() => {
            let url = v["html_url"].as_str().map(str::to_string);
            f.pr_url = url.clone();
            f.ci = Some("pending".into());
            f.needs_you = None;
            f.updated = now_ms();
            put_finding(sup, &f)?;
            event(sup, &f, "review", "review", "pr", true, "autopilot", &format!("Opened {}", url.clone().unwrap_or_else(|| "a pull request".into())), json!({ "url": url, "notes": notes }));
            Ok(())
        }
        Ok(v) => {
            let e = one_line(&v.to_string(), 300);
            f.needs_you = Some(format!("Could not open a pull request: {e}. Open one and record its URL."));
            failed(sup, &mut f, "pr", "autopilot", &format!("opening the pull request failed: {e}"), json!({ "answer": v }))
        }
        Err(e) => {
            f.needs_you = Some(format!("{e}. Open a pull request and record its URL."));
            failed(sup, &mut f, "pr", "autopilot", &e, json!({ "error": e }))
        }
    }
}

/// Review: CI's state and whether it merged.
fn check_pr(sup: &Arc<Supervisor>, lv: &Arc<Levels>, id: &str) -> Result<(), ApiError> {
    let mut f = get_finding(sup, id)?;
    let Some(url) = f.pr_url.clone() else { return Ok(()) };
    let wt = f.worktree.clone().map(PathBuf::from).filter(|p| p.is_dir()).unwrap_or_else(|| PathBuf::from(&f.repo));
    let Some((slug, n)) = pr_of(&url) else { return Ok(()) };
    let Ok(pr) = gh_api(&wt, &[format!("repos/{slug}/pulls/{n}")]) else { return Ok(()) };
    let sha = pr["head"]["sha"].as_str().unwrap_or("").to_string();
    // The check runs of its head, as a rollup: name, status, conclusion.
    let runs = if sha.is_empty() { Json::Null } else { gh_api(&wt, &[format!("repos/{slug}/commits/{sha}/check-runs?per_page=100")]).unwrap_or(Json::Null) };
    let rollup: Vec<Json> = runs["check_runs"].as_array().cloned().unwrap_or_default().iter().map(|c| json!({ "name": c["name"], "status": c["status"], "conclusion": c["conclusion"].as_str().map(|x| x.to_ascii_uppercase()) })).collect();
    let rollup = Json::Array(rollup);
    let ci = ci_of(&rollup);
    let was = f.ci.clone();
    f.ci = Some(ci.to_string());
    let v = json!({ "statusCheckRollup": rollup });
    if pr["merged"] == true {
        f.approved_by = pr["merged_by"]["login"].as_str().map(str::to_string).or(f.approved_by.clone()).or(Some("you".into()));
        let by = f.approved_by.clone().unwrap_or_default();
        return advance_to(sup, &mut f, "merged", "approved", true, &by, &format!("Merged {url}"), json!({ "url": url, "ci": ci }));
    }
    if ci == "fail" && was.as_deref() != Some("fail") {
        let names: Vec<String> = v["statusCheckRollup"].as_array().cloned().unwrap_or_default().iter().filter(|c| check_state(c) == "fail").filter_map(|c| c["name"].as_str().or(c["context"].as_str()).map(str::to_string)).collect();
        let text = format!("CI failed on your pull request {url}: {}. Fix it, commit, and stop.", names.join(", "));
        steer_back(sup, lv, &mut f, &text);
        return advance_to(sup, &mut f, "develop", "ci", false, "autopilot", &format!("CI failed: {}", names.join(", ")), json!({ "url": url, "failed": names }));
    }
    if was.as_deref() != Some(ci) {
        f.updated = now_ms();
        put_finding(sup, &f)?;
        event(sup, &f, "review", "review", "ci", ci == "pass", "autopilot", &format!("CI is {ci}"), json!({ "url": url, "ci": ci }));
    } else {
        put_finding(sup, &f)?;
    }
    Ok(())
}

fn check_state(c: &Json) -> &'static str {
    let st = c["conclusion"].as_str().or(c["state"].as_str()).unwrap_or("").to_ascii_uppercase();
    match st.as_str() {
        "SUCCESS" | "NEUTRAL" | "SKIPPED" => "pass",
        "FAILURE" | "ERROR" | "CANCELLED" | "TIMED_OUT" | "ACTION_REQUIRED" | "STARTUP_FAILURE" => "fail",
        _ => "pending",
    }
}

/// `CI result` when the repository has that check (ChloeDB's required one), else all of them.
fn ci_of(rollup: &Json) -> &'static str {
    let checks = rollup.as_array().cloned().unwrap_or_default();
    if let Some(c) = checks.iter().find(|c| c["name"] == "CI result" || c["context"] == "CI result") {
        return check_state(c);
    }
    if checks.is_empty() {
        return "pending";
    }
    if checks.iter().any(|c| check_state(c) == "fail") {
        "fail"
    } else if checks.iter().all(|c| check_state(c) == "pass") {
        "pass"
    } else {
        "pending"
    }
}

/// Merged: the repro on the base branch, once a day; regressed when it fails.
fn guard(sup: &Arc<Supervisor>, id: &str) -> Result<(), ApiError> {
    let mut f = get_finding(sup, id)?;
    let st = settings(sup, &f.repo);
    let repo = PathBuf::from(&f.repo);
    let base = base_of(&repo, &st);
    // A pull request merges on the remote: guard what origin has, when there is one.
    let remote = git(&repo, &["fetch", "-q", "origin", &base]).ok().and_then(|_| crate::git::rev_parse(&repo, &format!("origin/{base}")));
    let rev = remote.or_else(|| crate::git::rev_parse(&repo, &base)).unwrap_or_else(|| base.clone());
    let scratch = Scratch::new(sup, &repo, &rev, &format!("guard-{}", f.id)).map_err(ApiError::internal)?;
    let runs = run_repro(sup, &f, &scratch.path, 1, st.repro_timeout_s);
    drop(scratch);
    f.guard_last = Some(now_ms());
    let ok = runs.iter().all(|(c, _)| *c == 0);
    f.guard_ok = Some(ok);
    let evidence = json!({ "commit": rev, "runs": runs.iter().map(|(c, o)| json!({ "exit": c, "output": o })).collect::<Vec<_>>() });
    if ok {
        f.updated = now_ms();
        put_finding(sup, &f)?;
        event(sup, &f, "merged", "merged", "guard", true, "autopilot", &format!("The repro passes on {base}"), evidence);
        return Ok(());
    }
    f.note = Some(format!("Regressed on {} at {}", base, &rev[..rev.len().min(10)]));
    f.agent = None;
    f.pr_url = None;
    f.ci = None;
    f.approved_by = None;
    f.score = score(&f) * 2.0;
    advance_to(sup, &mut f, "prioritize", "guard", false, "autopilot", &format!("Regressed: the repro fails again on {base}"), evidence)
}

// ---------------------------------------------------------- the autopilot

fn day_start(ms: i64) -> i64 {
    ms - ms.rem_euclid(86_400_000)
}

fn spent_today(sup: &Supervisor, repo: &str) -> f64 {
    let rows = sup
        .db()
        .query("SELECT SUM(cost_usd) FROM cv_runs WHERE repo = :repo AND created >= :since", &Params::new().set("repo", s(repo)).set("since", t(day_start(now_ms()))))
        .unwrap_or_default();
    match rows.first().and_then(|r| r.first()) {
        Some(Value::Float(f)) => *f,
        Some(Value::Int(i)) => *i as f64,
        _ => 0.0,
    }
}

/// One pass over a repository: every item whose gate can be checked now
/// gets its work started. Answers what it started; `wait` joins it.
pub fn tick(sup: &Arc<Supervisor>, lv: &Arc<Levels>, repo: &str, wait: bool) -> Result<Json, ApiError> {
    let st = settings(sup, repo);
    let mut started: Vec<String> = Vec::new();
    let mut handles = Vec::new();
    let budget_left = st.budget_usd_day - spent_today(sup, repo);
    let mut fs = findings_of(sup, repo);
    for f in fs.iter_mut().filter(|f| f.state == "active" && f.stage == "found") {
        if busy_what(&f.id).is_none() {
            gate_found(sup, f)?;
            started.push(format!("{} dedup", f.id));
        }
    }
    let fs = findings_of(sup, repo);
    let active: Vec<&Finding> = fs.iter().filter(|f| f.state == "active").collect();
    for f in &active {
        let id = f.id.clone();
        let (s2, l2) = (sup.clone(), lv.clone());
        let h = match f.stage.as_str() {
            "reproduce" if f.repro.is_some() => in_background(&id, "reproducing", {
                let id = id.clone();
                move || {
                    let _ = gate_reproduce(&s2, &id);
                }
            }),
            "reproduce" if f.needs_you.is_none() => {
                let _ = gate_reproduce(sup, &id);
                None
            }
            "scope" if budget_left > 0.0 || f.scope.as_ref().is_some_and(|s| scope_ok(s).is_ok()) => in_background(&id, "scoping", {
                let id = id.clone();
                move || {
                    let _ = gate_scope(&s2, &id);
                }
            }),
            "develop" if f.agent.as_deref().is_some_and(|a| agent_done(sup, a)) && f.needs_you.is_none() => in_background(&id, "checking the branch", {
                let id = id.clone();
                move || {
                    let _ = gate_develop(&s2, &l2, &id);
                }
            }),
            "verify" if budget_left > 0.0 => match f.persona.clone() {
                Some(p) => in_background(&id, "replaying", {
                    let id = id.clone();
                    move || {
                        let _ = play(&s2, &l2, &p, Some(&id));
                    }
                }),
                // A finding of yours: no persona to replay it, so verify is yours.
                None => None,
            },
            "review" if f.pr_url.is_none() && st.open_pr && f.needs_you.is_none() => in_background(&id, "opening the pull request", {
                let id = id.clone();
                move || {
                    let _ = open_pr(&s2, &id);
                }
            }),
            "review" if f.pr_url.is_some() => in_background(&id, "reading CI", {
                let id = id.clone();
                move || {
                    let _ = check_pr(&s2, &l2, &id);
                }
            }),
            "merged" if f.guard_last.is_none_or(|g| now_ms() - g > 86_400_000) => in_background(&id, "guarding", {
                let id = id.clone();
                move || {
                    let _ = guard(&s2, &id);
                }
            }),
            _ => None,
        };
        if let Some(h) = h {
            started.push(format!("{} {}", id, busy_what(&id).unwrap_or_default()));
            handles.push(h);
        }
    }
    // Prioritize → develop, under the WIP limit; doors wait for you.
    let mut in_dev = fs.iter().filter(|f| f.state == "active" && f.stage == "develop").count();
    let mut queue: Vec<Finding> = fs.iter().filter(|f| f.state == "active" && f.stage == "prioritize" && f.door.is_none()).cloned().collect();
    queue.sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal)));
    for mut f in queue {
        if in_dev >= st.wip || budget_left <= 0.0 {
            break;
        }
        match start_develop(sup, lv, &mut f, "autopilot") {
            Ok(()) => {
                in_dev += 1;
                started.push(format!("{} develop", f.id));
            }
            Err(e) => {
                f.needs_you = Some(format!("Could not start an agent: {}", e.message));
                put_finding(sup, &f)?;
            }
        }
    }
    // Personas due to replay.
    let due_ms = (st.replay_every_h * 3_600_000.0) as i64;
    let mut playing = busy_count("playing");
    for p in personas_of(sup, repo).into_iter().filter(|p| p.state != "retired") {
        if playing >= st.runs_at_once || budget_left <= 0.0 {
            break;
        }
        if p.last_run.is_some_and(|l| now_ms() - l < due_ms) {
            continue;
        }
        let (s2, l2, pid) = (sup.clone(), lv.clone(), p.id.clone());
        if let Some(h) = in_background(&format!("persona:{}", p.id), "playing", move || {
            let _ = play(&s2, &l2, &pid, None);
        }) {
            playing += 1;
            started.push(format!("{} plays", p.id));
            handles.push(h);
        }
    }
    // A new persona when the loop saturates: none on probation, and the last
    // six explore runs brought fewer than three serious findings.
    if st.generate && budget_left > 0.0 {
        let ps = personas_of(sup, repo);
        let recent: Vec<Run> = runs_of(sup, repo, 200).into_iter().filter(|r| r.purpose == "explore" && r.state == "done").take(6).collect();
        let serious: usize = recent.iter().map(|r| r.findings.iter().filter(|x| sev_of(&x["severity"]) >= 3).count()).sum();
        let last_gen = runs_of(sup, repo, 200).into_iter().filter(|r| r.purpose == "generate").map(|r| r.created).max();
        if !ps.is_empty() && !ps.iter().any(|p| p.state == "probation") && recent.len() >= 6 && serious < 3 && last_gen.is_none_or(|g| now_ms() - g > 86_400_000) {
            let (s2, r2) = (sup.clone(), repo.to_string());
            if let Some(h) = in_background(&format!("generate:{repo}"), "generating", move || {
                let _ = generate(&s2, &r2);
            }) {
                started.push("a new persona".into());
                handles.push(h);
            }
        }
    }
    snapshot(sup, repo);
    if wait {
        for h in handles {
            let _ = h.join();
        }
        snapshot(sup, repo);
    }
    Ok(json!({ "started": started, "spent_today": spent_today(sup, repo), "budget_usd_day": st.budget_usd_day }))
}

/// The autopilot: a thread that ticks each enabled repository.
pub fn start(sup: &Arc<Supervisor>, lv: &Arc<Levels>) {
    let (sup, lv) = (sup.clone(), lv.clone());
    std::thread::spawn(move || {
        let mut last: HashMap<String, Instant> = HashMap::new();
        loop {
            std::thread::sleep(Duration::from_secs(5));
            for (repo, st) in repos(&sup) {
                if !st.enabled {
                    continue;
                }
                if last.get(&repo).is_some_and(|l| l.elapsed() < Duration::from_secs(st.tick_s.max(10))) {
                    continue;
                }
                last.insert(repo.clone(), Instant::now());
                if let Err(e) = tick(&sup, &lv, &repo, false) {
                    eprintln!("converge: tick of {repo}: {}", e.message);
                }
            }
        }
    });
}

/// Surfaces: the repository's top folders with a README, and the personas' own.
fn coverage(sup: &Supervisor, repo: &str) -> Vec<Json> {
    let mut surfaces: Vec<String> = std::fs::read_dir(repo)
        .map(|d| d.filter_map(|e| e.ok()).filter(|e| e.path().join("README.md").is_file()).filter_map(|e| e.file_name().to_str().map(str::to_string)).filter(|n| !n.starts_with('.')).collect())
        .unwrap_or_default();
    let ps = personas_of(sup, repo);
    for p in &ps {
        for x in &p.surfaces {
            if !surfaces.contains(x) {
                surfaces.push(x.clone());
            }
        }
    }
    surfaces.sort();
    let week = now_ms() - 7 * 86_400_000;
    let runs: Vec<Run> = runs_of(sup, repo, 2000).into_iter().filter(|r| r.purpose == "explore" && r.created >= week).collect();
    surfaces
        .iter()
        .map(|x| {
            let who: Vec<&Persona> = ps.iter().filter(|p| p.state != "retired" && p.surfaces.iter().any(|y| y.eq_ignore_ascii_case(x))).collect();
            let n = runs.iter().filter(|r| r.persona.as_ref().is_some_and(|p| who.iter().any(|w| &w.id == p))).count();
            let open = findings_of(sup, repo).iter().filter(|f| f.state == "active" && f.stage != "merged" && f.persona.as_ref().is_some_and(|p| who.iter().any(|w| &w.id == p))).count();
            json!({ "surface": x, "personas": who.iter().map(|p| p.name.clone()).collect::<Vec<_>>(), "runs_7d": n, "open": open })
        })
        .collect()
}

/// Makes one persona aimed at the least-played surface: it starts on probation.
pub fn generate(sup: &Arc<Supervisor>, repo: &str) -> Result<Persona, ApiError> {
    let st = settings(sup, repo);
    let ps: Vec<Json> = personas_of(sup, repo).iter().map(|p| json!({ "name": p.name, "role": p.role, "surfaces": p.surfaces, "state": p.state })).collect();
    let mut cov = coverage(sup, repo);
    cov.sort_by_key(|c| c["runs_7d"].as_i64().unwrap_or(0));
    let mut run = Run { id: crate::levels::new_id("cr"), repo: repo.to_string(), purpose: "generate".into(), state: "running".into(), created: now_ms(), ..Default::default() };
    put_run(sup, &run)?;
    let v = runner(sup, "generate", &json!({ "personas": ps, "coverage": cov }), Path::new(repo), &st.model, 900);
    run.ended = Some(now_ms());
    let v = match v {
        Ok(v) => v,
        Err(e) => {
            run.state = "failed".into();
            run.error = Some(e.clone());
            put_run(sup, &run)?;
            return Err(ApiError::new(502, "runner_failed", e));
        }
    };
    run.state = "done".into();
    run.cost_usd = v["cost_usd"].as_f64().unwrap_or(0.0);
    put_run(sup, &run)?;
    let name = v["name"].as_str().unwrap_or("").trim().to_string();
    if name.is_empty() {
        return Err(ApiError::new(502, "runner_failed", "the generator answered a persona with no name"));
    }
    let p = Persona {
        id: crate::levels::new_id("cp"),
        repo: repo.to_string(),
        name: one_line(&name, 80),
        role: v["role"].as_str().unwrap_or("").to_string(),
        traits: v["traits"].as_array().map(|a| a.iter().filter_map(Json::as_str).map(str::to_string).collect()).unwrap_or_default(),
        journey: v["journey"].as_str().unwrap_or("").to_string(),
        surfaces: v["surfaces"].as_array().map(|a| a.iter().filter_map(Json::as_str).map(str::to_string).collect()).unwrap_or_default(),
        origin: "generated".into(),
        state: "probation".into(),
        why: v["why"].as_str().map(str::to_string),
        created: now_ms(),
        ..Default::default()
    };
    put_persona(sup, &p)?;
    Ok(p)
}

// ------------------------------------------------------------------ metrics

pub fn metrics(sup: &Supervisor, repo: &str) -> Json {
    let fs = findings_of(sup, repo);
    let ps = personas_of(sup, repo);
    let runs = runs_of(sup, repo, 5000);
    let now = now_ms();
    let active: Vec<&Finding> = fs.iter().filter(|f| f.state == "active" && f.stage != "merged").collect();
    let eases: Vec<f64> = ps.iter().filter(|p| p.state != "retired").filter_map(|p| p.ease).collect();
    let ease = if eases.is_empty() { None } else { Some((eases.iter().sum::<f64>() / eases.len() as f64 * 100.0).round() / 100.0) };
    let found = fs.iter().filter(|f| f.state != "duplicate").count();
    let reproduced = fs.iter().filter(|f| f.repro_hash.is_some() || STAGES.iter().position(|x| *x == f.stage).unwrap_or(0) >= 2).count();
    let merged = fs.iter().filter(|f| f.stage == "merged").count();
    let regressed = fs.iter().filter(|f| f.note.as_deref().is_some_and(|n| n.starts_with("Regressed"))).count();
    let week = now - 7 * 86_400_000;
    let explore: Vec<&Run> = runs.iter().filter(|r| r.purpose == "explore" && r.state == "done" && r.created >= week).collect();
    // A run counts at least five minutes, so a quick run does not make the rate explode.
    let hours: f64 = explore.iter().map(|r| ((r.ended.unwrap_or(r.created) - r.created).max(300_000)) as f64 / 3_600_000.0).sum();
    let serious: usize = explore.iter().map(|r| r.findings.iter().filter(|x| sev_of(&x["severity"]) >= 3).count()).sum();
    let cost_7d: f64 = runs.iter().filter(|r| r.created >= week).map(|r| r.cost_usd).sum();
    let merged_7d = fs.iter().filter(|f| f.stage == "merged" && f.stage_since >= week).count();
    let mut by_stage = serde_json::Map::new();
    for st in STAGES {
        by_stage.insert(st.into(), json!(fs.iter().filter(|f| f.state == "active" && f.stage == st).count()));
    }
    let mut by_theme = serde_json::Map::new();
    for th in THEMES {
        let of: Vec<&&Finding> = active.iter().filter(|f| f.theme == th).collect();
        let merged_of = fs.iter().filter(|f| f.theme == th && f.stage == "merged").count();
        if !of.is_empty() || merged_of > 0 {
            by_theme.insert(th.into(), json!({ "open": of.len(), "blocker": of.iter().filter(|f| f.severity >= 4).count(), "major": of.iter().filter(|f| f.severity == 3).count(), "merged": merged_of }));
        }
    }
    let history: Vec<Json> = docs::<Json>(sup, "SELECT doc FROM cv_metrics WHERE repo = :repo ORDER BY day ASC LIMIT 400", &Params::new().set("repo", s(repo)));
    json!({
        "ease": ease,
        "open": active.len(),
        "open_by_severity": { "blocker": active.iter().filter(|f| f.severity >= 4).count(), "major": active.iter().filter(|f| f.severity == 3).count(), "minor": active.iter().filter(|f| f.severity == 2).count(), "papercut": active.iter().filter(|f| f.severity <= 1).count() },
        "by_stage": by_stage,
        "by_theme": by_theme,
        "needs_you": fs.iter().filter(|f| f.state == "active" && (f.needs_you.is_some() || (f.stage == "review" && f.ci.as_deref() == Some("pass")) || (f.stage == "prioritize" && f.door.is_some()))).count(),
        "funnel": { "found": found, "reproduced": reproduced, "merged": merged, "cant_reproduce": fs.iter().filter(|f| f.state == "cant_reproduce").count(), "duplicates": fs.iter().filter(|f| f.state == "duplicate").count() },
        "repro_rate": if found == 0 { None } else { Some(reproduced as f64 / found as f64) },
        "regression_rate": if merged + regressed == 0 { None } else { Some(regressed as f64 / (merged + regressed) as f64) },
        "serious_per_persona_hour": if hours <= 0.0 { None } else { Some(serious as f64 / hours) },
        "cost_7d": cost_7d,
        "cost_per_merged_fix": if merged_7d == 0 { None } else { Some(cost_7d / merged_7d as f64) },
        "spent_today": spent_today(sup, repo),
        "history": history,
    })
}

/// Today's row of `cv_metrics`, rewritten on each tick.
fn snapshot(sup: &Supervisor, repo: &str) {
    let m = metrics(sup, repo);
    let day = iso(day_start(now_ms()))[..10].to_string();
    let doc = json!({ "day": day, "ease": m["ease"], "open": m["open"], "open_by_severity": m["open_by_severity"], "funnel": m["funnel"], "serious_per_persona_hour": m["serious_per_persona_hour"], "cost_7d": m["cost_7d"] });
    let p = Params::new().set("id", s(&format!("{repo}#{day}"))).set("repo", s(repo)).set("day", s(&day)).set("doc", s(&doc.to_string()));
    let _ = sup.db().query("INSERT OR REPLACE INTO cv_metrics (id, repo, day, doc) VALUES (:id, :repo, :day, :doc)", &p);
}

// --------------------------------------------------------------- the seed

/// The six personas of ChloeDB's first evaluation (2026-11-15) and the
/// problems their reports named, as a starting board.
const SEED: &str = include_str!("../converge/seed.json");

pub fn seed(sup: &Supervisor, repo: &str) -> Result<Json, ApiError> {
    let v: Json = serde_json::from_str(SEED).map_err(|e| ApiError::internal(format!("the seed is not JSON: {e}")))?;
    let existing: Vec<String> = personas_of(sup, repo).iter().map(|p| p.name.clone()).collect();
    let mut ids: HashMap<String, String> = HashMap::new();
    let mut np = 0;
    for p in v["personas"].as_array().cloned().unwrap_or_default() {
        let name = p["name"].as_str().unwrap_or("").to_string();
        if existing.contains(&name) {
            if let Some(e) = personas_of(sup, repo).into_iter().find(|e| e.name == name) {
                ids.insert(p["key"].as_str().unwrap_or("").into(), e.id);
            }
            continue;
        }
        let pe = Persona {
            id: crate::levels::new_id("cp"),
            repo: repo.to_string(),
            name,
            role: p["role"].as_str().unwrap_or("").into(),
            traits: serde_json::from_value(p["traits"].clone()).unwrap_or_default(),
            journey: p["journey"].as_str().unwrap_or("").into(),
            surfaces: serde_json::from_value(p["surfaces"].clone()).unwrap_or_default(),
            origin: "seed".into(),
            state: "active".into(),
            ease: p["ease"].as_f64(),
            ease_hist: p["ease"].as_f64().into_iter().collect(),
            // The six of the first evaluation were played once; the others not yet.
            runs: p["ease"].as_f64().map_or(0, |_| 1),
            created: now_ms(),
            ..Default::default()
        };
        ids.insert(p["key"].as_str().unwrap_or("").into(), pe.id.clone());
        put_persona(sup, &pe)?;
        np += 1;
    }
    let titles: HashSet<String> = findings_of(sup, repo).iter().map(|f| f.title.clone()).collect();
    let mut nf = 0;
    for x in v["findings"].as_array().cloned().unwrap_or_default() {
        let title = x["title"].as_str().unwrap_or("").to_string();
        if title.is_empty() || titles.contains(&title) {
            continue;
        }
        let who = x["persona"].as_str().and_then(|k| ids.get(k)).cloned();
        let ease = who.as_ref().and_then(|w| get_persona(sup, w).ok()).and_then(|p| p.ease);
        let mut f = Finding {
            id: crate::levels::new_id("cv"),
            repo: repo.to_string(),
            title: title.clone(),
            detail: x["detail"].as_str().unwrap_or("").into(),
            kind: x["kind"].as_str().unwrap_or("bug").into(),
            theme: theme_of(x["theme"].as_str(), &title, x["detail"].as_str().unwrap_or("")),
            severity: sev_of(&x["severity"]),
            stage: "found".into(),
            state: "active".into(),
            persona: who.clone(),
            personas_hit: who.clone().into_iter().collect(),
            occurrences: 1,
            step: x["step"].as_str().map(str::to_string),
            ease_before: ease,
            by: "seed".into(),
            created: now_ms(),
            updated: now_ms(),
            stage_since: now_ms(),
            ..Default::default()
        };
        f.score = score(&f);
        put_finding(sup, &f)?;
        event(sup, &f, "", "found", "reported", true, "seed", &format!("From the persona reports of 2026-11-15 ({})", x["source"].as_str().unwrap_or("")), json!({ "source": x["source"] }));
        nf += 1;
    }
    Ok(json!({ "personas": np, "findings": nf }))
}

// ------------------------------------------------------------------- routes

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/converge", get(overview_route))
        .route("/api/converge/settings", get(settings_get).put(settings_put))
        .route("/api/converge/tick", post(tick_route))
        .route("/api/converge/seed", post(seed_route))
        .route("/api/converge/metrics", get(metrics_route))
        .route("/api/converge/findings", post(finding_new))
        .route("/api/converge/findings/:id", get(finding_get).put(finding_put))
        .route("/api/converge/findings/:id/:verb", post(finding_verb))
        .route("/api/converge/personas", get(personas_get).post(persona_new))
        .route("/api/converge/personas/generate", post(generate_route))
        .route("/api/converge/personas/:id/:verb", post(persona_verb))
        .route("/api/converge/runs/:id", get(run_get))
}

#[derive(Deserialize, Default)]
struct RepoQ {
    repo: Option<String>,
}

fn repo_of(r: Option<String>) -> Result<String, ApiError> {
    let r = r.filter(|r| !r.trim().is_empty()).ok_or_else(|| ApiError::bad("repo is required: the repository's path"))?;
    let co = crate::git::checkout(Path::new(r.trim())).map_err(ApiError::bad)?;
    Ok(co.main.to_string_lossy().to_string())
}

/// Only the user's token: approving, starting a door, settings.
fn user_only(app: &App, headers: &HeaderMap) -> Result<(), ApiError> {
    let t = headers.get(axum::http::header::AUTHORIZATION).and_then(|h| h.to_str().ok()).and_then(|h| h.strip_prefix("Bearer ")).map(str::trim).unwrap_or("");
    if crate::server::same_bytes(t, &app.sup.cfg.token) {
        Ok(())
    } else {
        Err(ApiError::new(403, "forbidden", "only you decide this: an agent's key can report a finding, not approve, start or change the loop"))
    }
}

fn caller(headers: &HeaderMap) -> String {
    headers.get("x-workbench-agent").and_then(|h| h.to_str().ok()).filter(|s| !s.is_empty()).unwrap_or("you").to_string()
}

async fn overview_route(State(app): State<App>, Query(q): Query<RepoQ>) -> ApiResult {
    let sup = app.sup.clone();
    let v = blocking(move || {
        let repo = repo_of(q.repo)?;
        let fs: Vec<Json> = findings_of(&sup, &repo).iter().filter(|f| matches!(f.state.as_str(), "active" | "parked")).map(|f| f.to_json(&sup)).collect();
        let ps: Vec<Json> = personas_of(&sup, &repo).iter().map(|p| persona_json(&sup, p)).collect();
        let runs: Vec<Json> = runs_of(&sup, &repo, 30).iter().map(|r| serde_json::to_value(r).unwrap()).collect();
        Ok(json!({ "repo": repo, "settings": settings(&sup, &repo), "stages": gates(), "themes": THEMES, "findings": fs, "personas": ps, "runs": runs, "metrics": metrics(&sup, &repo), "busy": BUSY.lock().unwrap().clone().unwrap_or_default() }))
    })
    .await?;
    Ok(AxJson(v).into_response())
}

fn persona_json(sup: &Supervisor, p: &Persona) -> Json {
    let mut v = serde_json::to_value(p).unwrap();
    v["busy"] = json!(busy_what(&format!("persona:{}", p.id)));
    v["repro_rate"] = json!(if p.reported == 0 { None } else { Some(p.reproduced as f64 / p.reported as f64) });
    v["open"] = json!(findings_of(sup, &p.repo).iter().filter(|f| f.state == "active" && f.stage != "merged" && f.personas_hit.contains(&p.id)).count());
    v
}

async fn settings_get(State(app): State<App>, Query(q): Query<RepoQ>) -> ApiResult {
    let sup = app.sup.clone();
    let v = blocking(move || {
        let repo = repo_of(q.repo)?;
        Ok(json!({ "repo": repo, "settings": settings(&sup, &repo) }))
    })
    .await?;
    Ok(AxJson(v).into_response())
}

async fn settings_put(State(app): State<App>, headers: HeaderMap, body: Bytes) -> ApiResult {
    user_only(&app, &headers)?;
    let v: Json = parse_body(&body)?;
    let sup = app.sup.clone();
    let out = blocking(move || {
        let repo = repo_of(v["repo"].as_str().map(str::to_string))?;
        let mut cur = serde_json::to_value(settings(&sup, &repo)).unwrap();
        if let (Some(c), Some(n)) = (cur.as_object_mut(), v.get("settings").or(Some(&v)).and_then(Json::as_object)) {
            for (k, x) in n {
                if k != "repo" && c.contains_key(k) {
                    c.insert(k.clone(), x.clone());
                }
            }
        }
        let st: Settings = serde_json::from_value(cur).map_err(|e| ApiError::bad(format!("settings: {e}")))?;
        if st.budget_usd_day < 0.0 || st.wip > 20 || st.runs_at_once > 20 || st.repro_runs > 20 {
            return Err(ApiError::bad("settings out of range: budget ≥ 0, wip and runs_at_once ≤ 20, repro_runs ≤ 20"));
        }
        put_settings(&sup, &repo, &st)?;
        Ok(json!({ "repo": repo, "settings": st }))
    })
    .await?;
    Ok(AxJson(out).into_response())
}

#[derive(Deserialize, Default)]
struct TickB {
    repo: Option<String>,
    wait: Option<bool>,
}

async fn tick_route(State(app): State<App>, headers: HeaderMap, body: Bytes) -> ApiResult {
    user_only(&app, &headers)?;
    let b: TickB = parse_body(&body)?;
    let (sup, lv) = (app.sup.clone(), app.lv.clone());
    let v = blocking(move || {
        let repo = repo_of(b.repo)?;
        if docs::<Settings>(&sup, "SELECT doc FROM cv_settings WHERE repo = :repo LIMIT 1", &Params::new().set("repo", s(&repo))).is_empty() {
            put_settings(&sup, &repo, &Settings::default())?;
        }
        tick(&sup, &lv, &repo, b.wait.unwrap_or(false))
    })
    .await?;
    Ok(AxJson(v).into_response())
}

async fn seed_route(State(app): State<App>, headers: HeaderMap, body: Bytes) -> ApiResult {
    user_only(&app, &headers)?;
    let b: RepoQ = parse_body(&body)?;
    let sup = app.sup.clone();
    let v = blocking(move || {
        let repo = repo_of(b.repo)?;
        if docs::<Settings>(&sup, "SELECT doc FROM cv_settings WHERE repo = :repo LIMIT 1", &Params::new().set("repo", s(&repo))).is_empty() {
            put_settings(&sup, &repo, &Settings::default())?;
        }
        seed(&sup, &repo)
    })
    .await?;
    Ok((StatusCode::CREATED, AxJson(v)).into_response())
}

async fn metrics_route(State(app): State<App>, Query(q): Query<RepoQ>) -> ApiResult {
    let sup = app.sup.clone();
    let v = blocking(move || {
        let repo = repo_of(q.repo)?;
        Ok(metrics(&sup, &repo))
    })
    .await?;
    Ok(AxJson(v).into_response())
}

#[derive(Deserialize, Default)]
struct NewFinding {
    repo: Option<String>,
    title: Option<String>,
    detail: Option<String>,
    kind: Option<String>,
    theme: Option<String>,
    severity: Option<Json>,
    repro: Option<String>,
    persona: Option<String>,
    step: Option<String>,
}

async fn finding_new(State(app): State<App>, headers: HeaderMap, body: Bytes) -> ApiResult {
    let b: NewFinding = parse_body(&body)?;
    let by = caller(&headers);
    let sup = app.sup.clone();
    let v = blocking(move || {
        let repo = repo_of(b.repo)?;
        let title = b.title.as_deref().map(str::trim).filter(|x| !x.is_empty() && x.len() <= 200).ok_or_else(|| ApiError::bad("title is required, at most 200 bytes"))?.to_string();
        if let Some(p) = b.persona.as_deref() {
            get_persona(&sup, p)?;
        }
        let mut f = Finding {
            id: crate::levels::new_id("cv"),
            repo: repo.clone(),
            theme: theme_of(b.theme.as_deref(), &title, b.detail.as_deref().unwrap_or("")),
            title,
            detail: b.detail.unwrap_or_default(),
            kind: b.kind.unwrap_or_else(|| "bug".into()),
            severity: sev_of(b.severity.as_ref().unwrap_or(&json!("major"))),
            stage: "found".into(),
            state: "active".into(),
            persona: b.persona.clone(),
            personas_hit: b.persona.into_iter().collect(),
            occurrences: 1,
            step: b.step,
            repro: b.repro.filter(|r| !r.trim().is_empty()),
            by: by.clone(),
            created: now_ms(),
            updated: now_ms(),
            stage_since: now_ms(),
            ..Default::default()
        };
        f.score = score(&f);
        put_finding(&sup, &f)?;
        event(&sup, &f, "", "found", "reported", true, &by, &format!("Reported by {by}"), Json::Null);
        if docs::<Settings>(&sup, "SELECT doc FROM cv_settings WHERE repo = :repo LIMIT 1", &Params::new().set("repo", s(&repo))).is_empty() {
            put_settings(&sup, &repo, &Settings::default())?;
        }
        Ok(json!({ "finding": f.to_json(&sup) }))
    })
    .await?;
    Ok((StatusCode::CREATED, AxJson(v)).into_response())
}

async fn finding_get(State(app): State<App>, AxPath(id): AxPath<String>) -> ApiResult {
    let sup = app.sup.clone();
    let v = blocking(move || {
        let f = get_finding(&sup, &id)?;
        let runs: Vec<Json> = docs::<Run>(&sup, "SELECT doc FROM cv_runs WHERE finding = :f ORDER BY created DESC LIMIT 50", &Params::new().set("f", s(&id))).iter().map(|r| serde_json::to_value(r).unwrap()).collect();
        let found_run = f.run.as_deref().and_then(|r| get_run(&sup, r).ok()).map(|r| serde_json::to_value(r).unwrap());
        let persona = f.persona.as_deref().and_then(|p| get_persona(&sup, p).ok()).map(|p| persona_json(&sup, &p));
        Ok(json!({ "finding": f.to_json(&sup), "events": events_of(&sup, &id), "runs": runs, "found_run": found_run, "persona": persona }))
    })
    .await?;
    Ok(AxJson(v).into_response())
}

/// Edits: title, detail, severity, repro (a changed repro is unlocked and
/// reproduced again), scope.
async fn finding_put(State(app): State<App>, headers: HeaderMap, AxPath(id): AxPath<String>, body: Bytes) -> ApiResult {
    let v: Json = parse_body(&body)?;
    let by = caller(&headers);
    let sup = app.sup.clone();
    let out = blocking(move || {
        let mut f = get_finding(&sup, &id)?;
        if let Some(x) = v["title"].as_str().map(str::trim).filter(|x| !x.is_empty()) {
            f.title = one_line(x, 200);
        }
        if let Some(x) = v["detail"].as_str() {
            f.detail = x.to_string();
        }
        if !v["severity"].is_null() {
            f.severity = sev_of(&v["severity"]);
        }
        if let Some(th) = v["theme"].as_str() {
            if !THEMES.contains(&th) {
                return Err(ApiError::bad(format!("theme is one of {}", THEMES.join(", "))));
            }
            f.theme = th.to_string();
        }
        if let Some(sc) = v.get("scope").filter(|x| x.is_object()) {
            let mut sc: Scope = serde_json::from_value(sc.clone()).map_err(|e| ApiError::bad(format!("scope: {e}")))?;
            sc.size = sc.size.trim().to_uppercase();
            f.scope = Some(sc);
        }
        if let Some(r) = v["repro"].as_str() {
            let changed = f.repro.as_deref() != Some(r);
            f.repro = Some(r.to_string()).filter(|r| !r.trim().is_empty());
            if changed && f.repro_hash.is_some() {
                f.repro_hash = None;
                f.repro_attempts = 0;
                f.needs_you = None;
                f.score = score(&f);
                advance_to(&sup, &mut f, "reproduce", "repro_changed", false, &by, "The repro changed: it is reproduced again", Json::Null)?;
                return Ok(json!({ "finding": f.to_json(&sup) }));
            }
            if changed {
                f.repro_attempts = 0;
                f.needs_you = None;
            }
        }
        f.score = score(&f);
        f.updated = now_ms();
        put_finding(&sup, &f)?;
        Ok(json!({ "finding": f.to_json(&sup) }))
    })
    .await?;
    Ok(AxJson(out).into_response())
}

async fn finding_verb(State(app): State<App>, headers: HeaderMap, AxPath((id, verb)): AxPath<(String, String)>, body: Bytes) -> ApiResult {
    let v: Json = if body.is_empty() { json!({}) } else { parse_body(&body)? };
    if matches!(verb.as_str(), "approve" | "start" | "dismiss" | "back" | "park" | "unpark" | "pin" | "unpin" | "pr") {
        user_only(&app, &headers)?;
    }
    let (sup, lv) = (app.sup.clone(), app.lv.clone());
    let out = blocking(move || {
        let mut f = get_finding(&sup, &id)?;
        match verb.as_str() {
            "pin" | "unpin" => {
                f.pinned = verb == "pin";
                f.updated = now_ms();
                put_finding(&sup, &f)?;
            }
            "park" => {
                f.state = "parked".into();
                failed(&sup, &mut f, "parked", "you", v["reason"].as_str().unwrap_or("Parked"), Json::Null)?;
            }
            "unpark" => {
                f.state = "active".into();
                failed(&sup, &mut f, "unparked", "you", "Back in the loop", Json::Null)?;
            }
            "dismiss" => {
                let reason = v["reason"].as_str().map(str::trim).filter(|r| !r.is_empty()).ok_or_else(|| ApiError::bad("reason is required: why it is not a problem (it teaches the persona)"))?;
                f.state = "dismissed".into();
                f.note = Some(reason.to_string());
                failed(&sup, &mut f, "dismissed", "you", reason, Json::Null)?;
            }
            "start" => {
                if f.stage != "prioritize" {
                    return Err(ApiError::conflict(format!("{} is in {}, not prioritize", f.id, f.stage)));
                }
                start_develop(&sup, &lv, &mut f, "you")?;
            }
            "back" => {
                if !matches!(f.stage.as_str(), "verify" | "review") {
                    return Err(ApiError::conflict(format!("{} is in {}: only verify and review go back to develop", f.id, f.stage)));
                }
                let text = v["text"].as_str().unwrap_or("Your reviewer sent this back.").to_string();
                steer_back(&sup, &lv, &mut f, &format!("Sent back by your reviewer: {text}"));
                f.ci = None;
                advance_to(&sup, &mut f, "develop", "review", false, "you", &text, Json::Null)?;
            }
            "pr" => {
                let url = v["url"].as_str().map(str::trim).filter(|u| u.starts_with("http")).ok_or_else(|| ApiError::bad("url is required: the pull request's address"))?;
                f.pr_url = Some(url.to_string());
                f.ci = Some("pending".into());
                f.needs_you = None;
                failed(&sup, &mut f, "pr", "you", &format!("Pull request {url} recorded"), json!({ "url": url }))?;
            }
            "approve" => {
                if f.stage != "review" {
                    return Err(ApiError::conflict(format!("{} is in {}, not review", f.id, f.stage)));
                }
                if f.ci.as_deref() != Some("pass") {
                    return Err(ApiError::conflict(format!("CI is {} on {}: only a green pull request is approved", f.ci.clone().unwrap_or_else(|| "unknown".into()), f.pr_url.clone().unwrap_or_else(|| "no pull request".into()))));
                }
                let url = f.pr_url.clone().unwrap_or_default();
                let wt = f.worktree.clone().map(PathBuf::from).filter(|p| p.is_dir()).unwrap_or_else(|| PathBuf::from(&f.repo));
                let (slug, n) = pr_of(&url).ok_or_else(|| ApiError::conflict(format!("{url} is not a GitHub pull request's address")))?;
                let args: Vec<String> = vec!["-X".into(), "PUT".into(), format!("repos/{slug}/pulls/{n}/merge"), "-f".into(), "merge_method=merge".into()];
                match gh_api(&wt, &args) {
                    Ok(v) if v["merged"] == true => {
                        f.approved_by = Some("you".into());
                        advance_to(&sup, &mut f, "merged", "approved", true, "you", &format!("Approved and merged {url}"), json!({ "url": url, "sha": v["sha"] }))?;
                    }
                    Ok(v) => return Err(ApiError::new(502, "merge_failed", format!("GitHub did not merge it: {}", one_line(&v.to_string(), 300)))),
                    Err(e) => return Err(ApiError::new(502, "merge_failed", e)),
                }
            }
            "repro-agent" => {
                if f.stage != "reproduce" {
                    return Err(ApiError::conflict(format!("{} is in {}, not reproduce", f.id, f.stage)));
                }
                let short = f.id.trim_start_matches("cv").chars().take(8).collect::<String>();
                let task = format!(
                    "Write a reproduction script for this problem a user hit, without fixing it.\n\nProblem: {}\n{}\n\nWrite .converge/repro.sh at the repository root: a POSIX sh script, run from the root, that exits non-zero while the problem is there and 0 once it is fixed. Keep it under a few minutes. Run it to check that it fails now, then stop.",
                    f.title, f.detail
                );
                let a = crate::goals::spawn(&sup, &lv, SpawnRequest { repo: f.repo.clone(), task: Some(task), name: Some(format!("cvr-{short}")), worktree: Some(true), ..Default::default() })?;
                let aid = a["id"].as_str().unwrap_or("").to_string();
                f.agent = Some(aid.clone());
                f.worktree = sup.agent_row(&aid).map(|r| r.cwd);
                f.needs_you = None;
                failed(&sup, &mut f, "repro_agent", "you", &format!("Agent {aid} is writing the repro"), json!({ "agent": aid }))?;
            }
            "take-repro" => {
                let wt = f.worktree.clone().ok_or_else(|| ApiError::conflict("no repro agent's worktree"))?;
                let text = std::fs::read_to_string(Path::new(&wt).join(".converge/repro.sh")).map_err(|_| ApiError::conflict("the agent has not written .converge/repro.sh yet"))?;
                f.repro = Some(text);
                f.repro_attempts = 0;
                f.agent = None;
                f.worktree = None;
                failed(&sup, &mut f, "repro_agent", "you", "The agent's repro was taken", Json::Null)?;
            }
            _ => return Err(ApiError::not_found(format!("no action {verb}"))),
        }
        Ok(json!({ "finding": get_finding(&sup, &f.id)?.to_json(&sup) }))
    })
    .await?;
    Ok(AxJson(out).into_response())
}

async fn personas_get(State(app): State<App>, Query(q): Query<RepoQ>) -> ApiResult {
    let sup = app.sup.clone();
    let v = blocking(move || {
        let repo = repo_of(q.repo)?;
        let ps: Vec<Json> = personas_of(&sup, &repo).iter().map(|p| persona_json(&sup, p)).collect();
        Ok(json!({ "personas": ps, "coverage": coverage(&sup, &repo) }))
    })
    .await?;
    Ok(AxJson(v).into_response())
}

async fn persona_new(State(app): State<App>, headers: HeaderMap, body: Bytes) -> ApiResult {
    user_only(&app, &headers)?;
    let v: Json = parse_body(&body)?;
    let sup = app.sup.clone();
    let out = blocking(move || {
        let repo = repo_of(v["repo"].as_str().map(str::to_string))?;
        let name = v["name"].as_str().map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| ApiError::bad("name is required"))?;
        let journey = v["journey"].as_str().map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| ApiError::bad("journey is required: the task the persona tries, end to end"))?;
        let p = Persona {
            id: crate::levels::new_id("cp"),
            repo: repo.clone(),
            name: one_line(name, 80),
            role: v["role"].as_str().unwrap_or("").into(),
            traits: serde_json::from_value(v["traits"].clone()).unwrap_or_default(),
            journey: journey.into(),
            surfaces: serde_json::from_value(v["surfaces"].clone()).unwrap_or_default(),
            origin: "you".into(),
            state: "active".into(),
            created: now_ms(),
            ..Default::default()
        };
        put_persona(&sup, &p)?;
        if docs::<Settings>(&sup, "SELECT doc FROM cv_settings WHERE repo = :repo LIMIT 1", &Params::new().set("repo", s(&repo))).is_empty() {
            put_settings(&sup, &repo, &Settings::default())?;
        }
        Ok(json!({ "persona": persona_json(&sup, &p) }))
    })
    .await?;
    Ok((StatusCode::CREATED, AxJson(out)).into_response())
}

async fn generate_route(State(app): State<App>, headers: HeaderMap, body: Bytes) -> ApiResult {
    user_only(&app, &headers)?;
    let b: RepoQ = parse_body(&body)?;
    let sup = app.sup.clone();
    let v = blocking(move || {
        let repo = repo_of(b.repo)?;
        let p = generate(&sup, &repo)?;
        Ok(json!({ "persona": persona_json(&sup, &p) }))
    })
    .await?;
    Ok((StatusCode::CREATED, AxJson(v)).into_response())
}

async fn persona_verb(State(app): State<App>, headers: HeaderMap, AxPath((id, verb)): AxPath<(String, String)>, body: Bytes) -> ApiResult {
    user_only(&app, &headers)?;
    let v: Json = if body.is_empty() { json!({}) } else { parse_body(&body)? };
    let (sup, lv) = (app.sup.clone(), app.lv.clone());
    let out = blocking(move || {
        let mut p = get_persona(&sup, &id)?;
        match verb.as_str() {
            "play" => {
                let (s2, l2, pid) = (sup.clone(), lv.clone(), p.id.clone());
                let wait = v["wait"].as_bool().unwrap_or(false);
                let h = in_background(&format!("persona:{}", p.id), "playing", move || {
                    let _ = play(&s2, &l2, &pid, None);
                })
                .ok_or_else(|| ApiError::conflict(format!("{} is already playing", p.name)))?;
                if wait {
                    let _ = h.join();
                }
                let run = runs_of(&sup, &p.repo, 50).into_iter().find(|r| r.persona.as_deref() == Some(p.id.as_str()) && r.purpose == "explore");
                return Ok(json!({ "persona": persona_json(&sup, &get_persona(&sup, &id)?), "run": run }));
            }
            "retire" => {
                p.state = "retired".into();
                p.why = Some(v["reason"].as_str().unwrap_or("Retired by you").to_string());
            }
            "activate" => p.state = "active".into(),
            _ => return Err(ApiError::not_found(format!("no action {verb}"))),
        }
        put_persona(&sup, &p)?;
        Ok(json!({ "persona": persona_json(&sup, &p) }))
    })
    .await?;
    Ok(AxJson(out).into_response())
}

async fn run_get(State(app): State<App>, AxPath(id): AxPath<String>) -> ApiResult {
    let sup = app.sup.clone();
    let v = blocking(move || Ok(json!({ "run": get_run(&sup, &id)? }))).await?;
    Ok(AxJson(v).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pull_request_addresses_are_read() {
        assert_eq!(pr_of("https://github.com/chloetech-ai/chloedb/pull/560"), Some(("chloetech-ai/chloedb".into(), "560".into())));
        assert_eq!(pr_of("https://example.com/x"), None);
    }

    #[test]
    fn the_origin_names_its_github_repository() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        git(r, &["init", "-q"]).unwrap();
        for (url, want) in [
            ("https://github.com/chloetech-ai/chloedb.git", "chloetech-ai/chloedb"),
            ("git@github.com:chloetech-ai/chloedb.git", "chloetech-ai/chloedb"),
            ("http://proxy@127.0.0.1:4000/git/chloetech-ai/chloedb", "chloetech-ai/chloedb"),
        ] {
            let _ = git(r, &["remote", "remove", "origin"]);
            git(r, &["remote", "add", "origin", url]).unwrap();
            assert_eq!(gh_slug(r).as_deref(), Some(want), "{url}");
        }
    }

    #[test]
    fn similar_titles_are_duplicates() {
        let a = Finding { title: "Plan the repair leaves a 2-copy shard with no reads".into(), ..Default::default() };
        let b = Finding { title: "Plan the repair leaves the shard with no reads".into(), ..Default::default() };
        let c = Finding { title: "Sheets Summarize default range stops at row 64".into(), ..Default::default() };
        assert!(similarity(&a, &b) >= DUP, "{}", similarity(&a, &b));
        assert!(similarity(&a, &c) < DUP);
    }

    #[test]
    fn the_score_weighs_severity_reach_and_size() {
        let mut f = Finding { severity: 4, personas_hit: vec!["a".into(), "b".into()], occurrences: 4, ease_before: Some(1.0), scope: Some(Scope { size: "S".into(), ..Default::default() }), ..Default::default() };
        assert_eq!(score(&f), 8.0 * 3.0 * 2.0);
        f.scope.as_mut().unwrap().size = "L".into();
        assert_eq!(score(&f), 12.0);
    }

    #[test]
    fn ci_reads_ci_result_first() {
        let r = json!([{ "name": "lint", "conclusion": "FAILURE" }, { "name": "CI result", "conclusion": "SUCCESS" }]);
        assert_eq!(ci_of(&r), "pass");
        assert_eq!(ci_of(&json!([{ "name": "a", "conclusion": "SUCCESS" }, { "name": "b", "status": "IN_PROGRESS" }])), "pending");
        assert_eq!(ci_of(&json!([{ "name": "a", "conclusion": "SUCCESS" }, { "context": "b", "state": "ERROR" }])), "fail");
    }

    #[test]
    fn the_last_json_object_of_a_text() {
        let v = extract_json("I tried it.\n```json\n{\"outcome\": \"done\", \"findings\": [{\"title\": \"x {y}\"}]}\n```").unwrap();
        assert_eq!(v["outcome"], "done");
        assert_eq!(v["findings"][0]["title"], "x {y}");
    }

    #[test]
    fn doors_come_from_codeowners() {
        let d = tempfile_dir();
        std::fs::create_dir_all(d.join(".github")).unwrap();
        std::fs::write(d.join(".github/CODEOWNERS"), "* @team\n\n# door: auth\n/chloedb-server/src/auth/ @owner\n\n/docs/ @docs\n").unwrap();
        let sc = |f: &str| Scope { area: "x".into(), files: vec![f.into()], ..Default::default() };
        assert_eq!(door_of(&d, &sc("chloedb-server/src/auth/keys.rs")).as_deref(), Some("auth"));
        assert_eq!(door_of(&d, &sc("docs/x.md")), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    fn tempfile_dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("cv-test-{}", crate::supervisor::hex(6)));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn themes_are_kept_or_guessed() {
        assert_eq!(theme_of(Some("Security"), "x", ""), "security");
        assert_eq!(theme_of(Some("nonsense"), "Ops-scope keys read transcripts", ""), "security");
        assert_eq!(theme_of(None, "A new shard answers reads before it holds the tables", ""), "scalability");
        assert_eq!(theme_of(None, "The button is hard to find", ""), "usability");
    }

    #[test]
    fn the_seed_parses() {
        let v: Json = serde_json::from_str(SEED).unwrap();
        assert_eq!(v["personas"].as_array().unwrap().len(), 16);
        assert!(v["findings"].as_array().unwrap().len() >= 20);
    }
}
