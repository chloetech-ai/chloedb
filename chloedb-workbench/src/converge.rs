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
pub const TABLES: [&str; 9] = [
    "CREATE TABLE IF NOT EXISTS cv_personas (id KEYWORD PRIMARY KEY, repo KEYWORD, name TEXT, state KEYWORD, origin KEYWORD, created TIMESTAMP, doc TEXT OPTIONS(indexed = false))",
    "CREATE TABLE IF NOT EXISTS cv_runs (id KEYWORD PRIMARY KEY, repo KEYWORD, persona KEYWORD, purpose KEYWORD, finding KEYWORD, state KEYWORD, outcome KEYWORD, ease FLOAT, cost_usd FLOAT, created TIMESTAMP, ended TIMESTAMP, doc TEXT OPTIONS(indexed = false))",
    "CREATE TABLE IF NOT EXISTS cv_findings (id KEYWORD PRIMARY KEY, repo KEYWORD, title TEXT, stage KEYWORD, state KEYWORD, severity INT, score FLOAT, persona KEYWORD, created TIMESTAMP, updated TIMESTAMP, doc TEXT OPTIONS(indexed = false))",
    "CREATE TABLE IF NOT EXISTS cv_events (id KEYWORD PRIMARY KEY, finding KEYWORD, repo KEYWORD, ts TIMESTAMP, from_stage KEYWORD, to_stage KEYWORD, gate KEYWORD, passed BOOL, by_who KEYWORD, summary TEXT, evidence TEXT OPTIONS(indexed = false))",
    "CREATE TABLE IF NOT EXISTS cv_metrics (id KEYWORD PRIMARY KEY, repo KEYWORD, day KEYWORD, doc TEXT OPTIONS(indexed = false))",
    "CREATE TABLE IF NOT EXISTS cv_settings (repo KEYWORD PRIMARY KEY, doc TEXT OPTIONS(indexed = false), updated TIMESTAMP)",
    // What the autopilot, the personas, the agents and you did, newest last (`log`).
    "CREATE TABLE IF NOT EXISTS cv_log (id KEYWORD PRIMARY KEY, repo KEYWORD, ts TIMESTAMP, kind KEYWORD, doc TEXT OPTIONS(indexed = false))",
    // The part of the project a finding is about (`component_of`).
    "ALTER TABLE cv_findings ADD COLUMN IF NOT EXISTS component KEYWORD",
    // What you said on a card (`talk`): your words as you wrote them.
    "CREATE TABLE IF NOT EXISTS cv_talk (id KEYWORD PRIMARY KEY, finding KEYWORD, repo KEYWORD, ts TIMESTAMP, who KEYWORD, text TEXT, agent KEYWORD, doc TEXT OPTIONS(indexed = false))",
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

/// The parts of a project its findings are filed under: each top-level
/// folder that builds on its own (a Cargo.toml, package.json,
/// pyproject.toml, pom.xml or go.mod), the language packages (`*-py`,
/// `*-js`, `*-java`, …) together as `sdks`, then `docs`, `packaging` (an
/// installer or a packaging folder) and `other`. On ChloeDB: chloedb,
/// chloedb-server, chloectx, chloedb-apps, …, sdks, docs, packaging, other.
pub fn components_of(repo: &Path) -> Vec<String> {
    let mut dirs: Vec<String> = std::fs::read_dir(repo)
        .map(|d| {
            d.filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir() && BUILD_FILES.iter().any(|b| e.path().join(b).is_file()))
                .filter_map(|e| e.file_name().to_str().map(str::to_string))
                .filter(|n| !n.starts_with('.') && !matches!(n.as_str(), "docs" | "packaging" | "target" | "node_modules"))
                .collect()
        })
        .unwrap_or_default();
    dirs.sort();
    let mut out: Vec<String> = dirs.iter().filter(|d| sdk_dir(d).is_none()).cloned().collect();
    if dirs.iter().any(|d| sdk_dir(d).is_some()) {
        out.push("sdks".into());
    }
    if repo.join("docs").is_dir() {
        out.push("docs".into());
    }
    if repo.join("packaging").is_dir() || repo.join("install.sh").is_file() {
        out.push("packaging".into());
    }
    out.push("other".into());
    out
}

const BUILD_FILES: [&str; 5] = ["Cargo.toml", "package.json", "pyproject.toml", "pom.xml", "go.mod"];

/// A language package's folder (`chloedb-py`), as `sdks`.
fn sdk_dir(name: &str) -> Option<&'static str> {
    const ENDS: [&str; 9] = ["-py", "-python", "-js", "-node", "-java", "-go", "-ruby", "-sdk", "-dotnet"];
    (ENDS.iter().any(|e| name.ends_with(e)) || matches!(name, "sdk" | "sdks" | "bindings")).then_some("sdks")
}

/// Words that point at a part of a project, by the end of its folder's name
/// (`server` is chloedb-server's, `sheets` chloedb-sheets'): they count
/// only when the project has such a part.
const COMPONENT_WORDS: [(&str, &[&str]); 12] = [
    ("server", &["server", "operator", "command center", "kubernetes", "k8s", "helm", "coordinator", "shard", "replica", "grpc", "http api", "auth", "api key", "cluster", "failover", "primary"]),
    ("chloectx", &["chloectx", "engram", "memory", "recall", "distill", "hooks", "mcp server", "inspector"]),
    ("apps", &["app engine", "apps", "quickjs", "sandboxed"]),
    ("workbench", &["workbench", "converge", "worktree", "spawn_agent"]),
    ("sheets", &["sheets", "spreadsheet", "workbook", "xlsx", "formula", "cell"]),
    ("crawl", &["crawl", "crawler", "robots.txt"]),
    ("netops", &["netops", "incident", "topology", "carrier", "alert rule"]),
    ("swarm", &["swarm", "simulated users", "playwright"]),
    ("sdks", &["python", "pip", "pypi", "langchain", "llama_index", "dbapi", "javascript", "node", "npm", "bun", "deno", "java", "jdbc", "maven", "sdk"]),
    ("docs", &["docs", "documentation", "guide", "readme", "quickstart", "tutorial"]),
    ("packaging", &["install.sh", "installer", "install", "homebrew", "brew", "release", "prebuilt", "crates.io", "packag"]),
    ("chloedb", &["sql", "bm25", "posting", "index", "query", "ranking", "analyzer", "embedded", "on-disk", "compaction"]),
];

fn tok(c: char) -> bool {
    c.is_alphanumeric() || c == '-' || c == '_'
}

/// Whether `w` starts a word of `text` (both lower case); a word of six
/// letters or more is a stem (`packag`, `distill`), a shorter one whole.
fn has_word(text: &str, w: &str) -> bool {
    text.match_indices(w).any(|(i, _)| text[..i].chars().next_back().is_none_or(|c| !tok(c)) && (w.len() >= 6 || text[i + w.len()..].chars().next().is_none_or(|c| !tok(c))))
}

/// Whether `w` is a whole word (or a path's part) of `text`.
fn whole(text: &str, w: &str) -> bool {
    text.match_indices(w).any(|(i, _)| text[..i].chars().next_back().is_none_or(|c| !tok(c)) && text[i + w.len()..].chars().next().is_none_or(|c| !tok(c)))
}

/// The component that names itself in `text`: its folder's name or path
/// (the longest first, so chloedb-server before chloedb), a language
/// package's folder (sdks), a path under docs/.
fn named_in(comps: &[String], text: &str) -> Option<String> {
    let mut by_len: Vec<&String> = comps.iter().filter(|c| !matches!(c.as_str(), "other" | "sdks" | "docs" | "packaging")).collect();
    by_len.sort_by_key(|c| std::cmp::Reverse(c.len()));
    let exact = |w: &str| whole(text, w);
    if let Some(c) = by_len.iter().find(|c| exact(c)) {
        return Some(c.to_string());
    }
    if comps.iter().any(|c| c == "sdks") {
        for w in text.split(|c: char| !tok(c)) {
            if sdk_dir(w).is_some() && w.contains('-') {
                return Some("sdks".into());
            }
        }
    }
    if comps.iter().any(|c| c == "docs") && (text.starts_with("docs/") || text.contains(" docs/") || text.contains("\"docs/") || text.contains("/docs/")) {
        return Some("docs".into());
    }
    if comps.iter().any(|c| c == "packaging") && (exact("install.sh") || text.contains("packaging/")) {
        return Some("packaging".into());
    }
    None
}

/// The component of the words a finding (or a persona's surface) uses.
fn guessed_in(comps: &[String], text: &str) -> Option<String> {
    let mut best: Option<(usize, &String)> = None;
    for c in comps.iter().filter(|c| c.as_str() != "other") {
        let n: usize = COMPONENT_WORDS.iter().filter(|(end, _)| c == end || c.ends_with(&format!("-{end}"))).flat_map(|(_, ws)| ws.iter()).filter(|w| has_word(text, w)).count();
        // The part's own short name (`sheets` for chloedb-sheets).
        let short = c.rsplit('-').next().unwrap_or(c);
        let n = n + usize::from(short != c && has_word(text, short)) * 2;
        if n > 0 && best.is_none_or(|(b, _)| n > b) {
            best = Some((n, c));
        }
    }
    best.map(|(_, c)| c.clone())
}

/// A finding's component: the one given (when the project has it), else
/// the one its words or paths name, else its persona's first surface that
/// names one, else the one its words point at most, else docs for a docs
/// finding, else other.
pub fn component_of(comps: &[String], given: Option<&str>, text: &str, surfaces: &[String], theme: &str) -> String {
    if let Some(g) = given.map(|g| g.trim().to_lowercase()).filter(|g| comps.contains(g)) {
        return g;
    }
    let text = text.to_lowercase();
    if let Some(c) = named_in(comps, &text) {
        return c;
    }
    for x in surfaces {
        let x = x.to_lowercase();
        if let Some(c) = named_in(comps, &x).or_else(|| comps.contains(&x).then(|| x.clone())) {
            return c;
        }
    }
    if let Some(c) = guessed_in(comps, &text) {
        return c;
    }
    if theme == "docs" && comps.iter().any(|c| c == "docs") {
        return "docs".into();
    }
    "other".into()
}

/// What a finding says about itself, for [`component_of`].
fn finding_text(f: &Finding) -> String {
    let files = f.scope.as_ref().map(|s| format!("{} {}", s.area, s.files.join(" "))).unwrap_or_default();
    format!("{} {} {} {}", f.title, f.detail, f.step.clone().unwrap_or_default(), files)
}

/// A finding's component from its own words and its persona's surfaces.
fn infer_component(sup: &Supervisor, f: &Finding, given: Option<&str>) -> String {
    let comps = components_of(Path::new(&f.repo));
    let surfaces = f.persona.as_deref().and_then(|p| get_persona(sup, p).ok()).map(|p| p.surfaces).unwrap_or_default();
    component_of(&comps, given, &finding_text(f), &surfaces, &f.theme)
}

pub const STAGES: [&str; 8] =["found", "reproduce", "scope", "prioritize", "develop", "verify", "review", "merged"];

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
    /// What a fix agent may do without asking, set once here so a run does
    /// not stop at every new command: Claude Code rules (`Bash(cargo:*)`).
    pub dev_allow: Vec<String>,
    /// What it may never do: pushing and pull requests are Converge's own.
    pub dev_deny: Vec<String>,
    /// Claude Code's permission mode for fix agents (`auto`: its classifier
    /// answers what the list does not; `default`: those wait for you).
    pub dev_permission_mode: Option<String>,
    /// The autopilot asks an agent for a missing repro before asking you,
    /// this many at once (0: a missing repro waits for you).
    pub repro_agents: usize,
    /// Findings leave Found on their own once they are not duplicates
    /// (else they wait for you to choose Reproduce or Dismiss).
    pub found_auto: bool,
    /// Findings leave Prioritize for development on their own, under the
    /// WIP limit (else each waits for you to choose Start development).
    pub develop_auto: bool,
    /// What you pick (Reproduce, Start development) carries on to your next
    /// decision even while the autopilot is off; personas play only when on.
    pub follow_picks: bool,
    /// Scoping asks the project's ChloeCtx memory (read only) for earlier
    /// decisions and rules (off: scoping runs with ChloeCtx off).
    pub scope_memory: bool,
}

/// A fix agent's default allow-list: reading, editing, building and testing
/// in its own worktree, and git there.
pub fn dev_allow_default() -> Vec<String> {
    let mut v: Vec<String> = ["Read", "Edit", "Write", "MultiEdit", "Glob", "Grep", "NotebookEdit", "TodoWrite"].iter().map(|x| x.to_string()).collect();
    for c in [
        "git status", "git diff", "git log", "git show", "git add", "git commit", "git checkout", "git restore", "git stash", "git rev-parse", "git branch", "git grep", "git ls-files", "cargo", "rustc", "python3", "python", "pytest", "pip install", "node", "npm test", "npm run", "npm ci", "npx", "mvn", "sh", "bash", "make", "ls", "cat", "head", "tail", "wc", "grep", "rg", "find", "sed", "awk", "sort", "uniq", "diff", "test", "echo", "printf", "mkdir", "touch", "cp", "mv", "pwd", "which", "env", "true",
    ] {
        v.push(format!("Bash({c}:*)"));
    }
    v
}

pub fn dev_deny_default() -> Vec<String> {
    ["git push", "gh", "sudo", "curl", "wget", "ssh", "scp", "docker", "kubectl", "rm -rf /"].iter().map(|c| format!("Bash({c}:*)")).collect()
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
            repro_timeout_s: 900,
            run_timeout_s: 3600,
            max_rounds: 3,
            open_pr: true,
            dev_allow: dev_allow_default(),
            dev_deny: dev_deny_default(),
            dev_permission_mode: Some("auto".into()),
            repro_agents: 2,
            found_auto: false,
            develop_auto: false,
            follow_picks: true,
            scope_memory: true,
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

/// An agent's (or scope's) report that the work needs a person.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Blocked {
    /// credentials, publish, account, external, decision, other
    pub kind: String,
    /// Why the agent cannot do it, one sentence.
    pub what: String,
    /// The exact step for the person.
    pub you_do: String,
    pub by: String,
    pub at: i64,
}

pub const BLOCKED_KINDS: [&str; 6] = ["credentials", "publish", "account", "external", "decision", "other"];

/// What every agent is told about steps it cannot take.
const BLOCKED_HELP: &str = "If the work needs something only a person can do or reach from outside this checkout (publishing a package or a release, credentials or tokens, account or repository settings, an outside service, or a decision that is the owner's), do not work around it: write .converge/blocked.json, one JSON object {\"kind\": \"credentials\" or \"publish\" or \"account\" or \"external\" or \"decision\", \"what\": \"one sentence: why you cannot do it\", \"you_do\": \"the exact step for the person, with the command if there is one\"}, do not commit it, and stop.";

/// Reads `.converge/blocked.json` from an agent's worktree (a plain line
/// counts too: it is the step).
fn read_blocked(wt: &Path, by: &str) -> Option<Blocked> {
    let text = std::fs::read_to_string(wt.join(".converge/blocked.json")).ok()?;
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let v: Json = serde_json::from_str(text).or_else(|_| extract_json(text).ok_or(())).unwrap_or(Json::Null);
    let mut b = if v.is_object() { blocked_of(&v, by)? } else { Blocked { kind: "other".into(), what: String::new(), you_do: one_line(text, 400), by: by.into(), at: now_ms() } };
    if b.you_do.trim().is_empty() {
        b.you_do = one_line(text, 400);
    }
    Some(b)
}

/// A `{kind, what, you_do}` object as a Blocked, if it names a step.
fn blocked_of(v: &Json, by: &str) -> Option<Blocked> {
    let you_do = v["you_do"].as_str().map(str::trim).filter(|t| !t.is_empty())?;
    let kind = v["kind"].as_str().map(|k| k.trim().to_lowercase()).filter(|k| BLOCKED_KINDS.contains(&k.as_str())).unwrap_or_else(|| "other".into());
    Some(Blocked { kind, what: one_line(v["what"].as_str().unwrap_or(""), 400), you_do: one_line(you_do, 600), by: by.into(), at: now_ms() })
}

/// The finding waits for a person: the autopilot stops on it.
fn set_blocked(sup: &Supervisor, f: &mut Finding, b: Blocked) -> Result<(), ApiError> {
    f.needs_you = Some(format!("Blocked: {}", b.you_do));
    f.queued = None;
    let st = f.stage.clone();
    let (repo, title, by) = (f.repo.clone(), f.title.clone(), b.by.clone());
    let detail = json!({ "kind": b.kind, "what": b.what, "you_do": b.you_do });
    f.blocked = Some(b.clone());
    f.updated = now_ms();
    put_finding(sup, f)?;
    event(sup, f, &st, &st, "blocked", false, &by, &format!("Blocked on you ({}): {}", b.kind, b.you_do), detail.clone());
    log(sup, &repo, "decide", "autopilot", &format!("{}: blocked on you ({}): {}", short(&title, 60), b.kind, short(&b.you_do, 120)), json!({ "finding": f.id, "agent": f.agent }), detail);
    Ok(())
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
    /// The part of the project it is about: one of [`components_of`] the repository.
    pub component: String,
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
    /// What only a person can do (publish, credentials, an outside
    /// service, a decision): the autopilot leaves it alone until you
    /// say it is done (`unblock`).
    pub blocked: Option<Blocked>,
    /// Waits for capacity (an agent slot, WIP, budget): not a need of yours.
    pub queued: Option<String>,
    /// Why the last develop check failed, and what the agent said.
    pub last_check: Option<Json>,
    /// How many times you talked to this card's agent (`talk`).
    pub talk_count: i64,
    /// The read-only agent started to answer you on this card, and its
    /// kind: `card` (questions) or `scope` (the plan).
    pub chat_agent: Option<String>,
    pub chat_kind: Option<String>,
    /// The time of the scope agent's last answer whose plan was read.
    pub chat_taken: Option<i64>,
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
    /// Its ease in runs without memory (each visit its first), and its history.
    pub ease: Option<f64>,
    pub ease_hist: Vec<f64>,
    /// The same, of runs with its memory: the two are never compared.
    pub ease_memory: Option<f64>,
    pub ease_hist_memory: Vec<f64>,
    pub last_run: Option<i64>,
    pub created: i64,
    /// Plays with a ChloeCtx memory of its own earlier visits (off: each
    /// visit is its first).
    pub use_memory: bool,
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
    /// What the persona did, step by step (a persona run's stream): `{ts, kind, text}`.
    pub steps: Vec<Json>,
    /// The journey step it said it was on: `{n, of, name}`.
    pub at: Option<Json>,
    /// The persona played with its memory (ChloeCtx wired in).
    pub use_memory: bool,
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
        .set("component", s(&f.component))
        .set("created", t(f.created))
        .set("updated", t(f.updated))
        .set("doc", s(&serde_json::to_string(f).unwrap()));
    sup.db()
        .query("INSERT OR REPLACE INTO cv_findings (id, repo, title, stage, state, severity, score, persona, component, created, updated, doc) VALUES (:id, :repo, :title, :stage, :state, :severity, :score, :persona, :component, :created, :updated, :doc)", &p)
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
    // The activity feed mirrors it, but for what is logged where it starts
    // (a finding reported, asked for), and a merge's notice on the target.
    let notice = gate == "dedup" && passed && from == to;
    if let Some(kind) = log_kind(gate, by).filter(|_| !notice) {
        let agent = evidence["agent"].as_str().map(str::to_string).or_else(|| (kind == "agent").then(|| f.agent.clone()).flatten());
        let refs = json!({ "finding": f.id, "persona": f.persona, "run": evidence["run"].as_str(), "agent": agent });
        let detail = json!({ "gate": gate, "passed": passed, "from": from, "to": to, "title": f.title, "component": f.component, "evidence": evidence });
        log(sup, &f.repo, kind, &who(sup, by), &format!("{}: {}", short(&f.title, 70), summary), refs, detail);
    }
}

// ------------------------------------------------------------ the activity log

/// The kind of activity a gate's event is: `you` for your picks, `agent`
/// for a repro or fix agent, `pr` for the pull request, `gate` otherwise;
/// none for a finding's creation (logged by what made it).
pub fn log_kind(gate: &str, by: &str) -> Option<&'static str> {
    match gate {
        "reported" | "asked" => None,
        _ if by == "you" => Some("you"),
        "repro_agent" | "slot" => Some("agent"),
        "pr" | "ci" | "approved" => Some("pr"),
        _ => Some("gate"),
    }
}

/// A title cut to `max` characters, for a phone's line.
fn short(text: &str, max: usize) -> String {
    format!("“{}”", one_line(text, max))
}

/// Who did it, by name: a persona's, an agent's, or as given (autopilot, you).
fn who(sup: &Supervisor, by: &str) -> String {
    if by.starts_with("cp") {
        if let Ok(p) = get_persona(sup, by) {
            return p.name;
        }
    }
    sup.agent_row(by).map(|a| a.name).filter(|n| !n.is_empty()).unwrap_or_else(|| by.to_string())
}

/// One line of the activity feed (`cv_log`): `refs` names the finding,
/// persona, run or agent it is about; `detail` holds the evidence.
pub fn log(sup: &Supervisor, repo: &str, kind: &str, actor: &str, summary: &str, refs: Json, detail: Json) {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst) % 1_000_000;
    let ts = now_ms();
    let id = format!("lg{ts:013}{n:06}");
    let mut doc = json!({ "id": id, "ts": ts, "kind": kind, "actor": actor, "summary": one_line(summary, 400), "detail": detail });
    for (k, v) in refs.as_object().into_iter().flatten() {
        if !v.is_null() {
            doc[k] = v.clone();
        }
    }
    let p = Params::new().set("id", s(&id)).set("repo", s(repo)).set("ts", t(ts)).set("kind", s(kind)).set("doc", s(&doc.to_string()));
    let _ = sup.db().query("INSERT INTO cv_log (id, repo, ts, kind, doc) VALUES (:id, :repo, :ts, :kind, :doc)", &p);
}

/// The feed, newest first: items since `since` (ms), at most `limit`.
pub fn activity(sup: &Supervisor, repo: &str, since: i64, limit: usize) -> Vec<Json> {
    let limit = limit.clamp(1, 500);
    docs(sup, &format!("SELECT doc FROM cv_log WHERE repo = :repo AND ts >= :since ORDER BY id DESC LIMIT {limit}"), &Params::new().set("repo", s(repo)).set("since", t(since)))
}

/// The last logged tick: `{ts, started, decisions}`.
fn last_tick(sup: &Supervisor, repo: &str) -> Json {
    let v: Option<Json> = docs::<Json>(sup, "SELECT doc FROM cv_log WHERE repo = :repo AND kind = 'tick' ORDER BY id DESC LIMIT 1", &Params::new().set("repo", s(repo))).into_iter().next();
    match v {
        Some(v) => json!({ "ts": v["ts"], "summary": v["summary"], "started": v["detail"]["started"], "decisions": v["detail"]["decisions"] }),
        None => Json::Null,
    }
}

/// One thing the autopilot decided in a tick. `key` says what it is about
/// (the same decision on the same thing keeps its key while numbers like
/// the money spent move), `text` says it to you.
#[derive(Clone, Debug, PartialEq)]
pub struct Decision {
    pub key: String,
    pub text: String,
}

fn decide(out: &mut Vec<Decision>, key: impl Into<String>, text: impl Into<String>) {
    let key = key.into();
    if !out.iter().any(|d| d.key == key) {
        out.push(Decision { key, text: text.into() });
    }
}

/// The decisions of the last logged tick, by repository: a tick is logged
/// only when it started something or its decisions changed.
static LAST_DECISIONS: Mutex<Option<HashMap<String, Vec<String>>>> = Mutex::new(None);

/// What of a tick goes in the feed: whether the tick itself does, and the
/// decisions that were not in the last logged one.
pub fn tick_news<'a>(prev: Option<&[String]>, started: &[String], now: &'a [Decision]) -> (bool, Vec<&'a Decision>) {
    let prev = prev.unwrap_or(&[]);
    let fresh: Vec<&Decision> = now.iter().filter(|d| !prev.contains(&d.key)).collect();
    let gone = prev.iter().any(|k| !now.iter().any(|d| &d.key == k));
    (!started.is_empty() || !fresh.is_empty() || gone, fresh)
}

/// The tick's line: what it started, else why nothing.
pub fn tick_summary(started: &[String], decisions: &[Decision]) -> String {
    match (started.len(), decisions.first()) {
        (0, Some(d)) => format!("Nothing to start: {}", d.text),
        (0, None) => "Nothing to do".into(),
        (1, _) => format!("Started {}", started[0]),
        (n, _) => format!("Started {n}: {}", started.join("; ")),
    }
}

fn log_tick(sup: &Supervisor, repo: &str, started: &[String], decisions: &[Decision], detail: Json) {
    let mut g = LAST_DECISIONS.lock().unwrap();
    let m = g.get_or_insert_with(HashMap::new);
    let (log_it, fresh) = tick_news(m.get(repo).map(Vec::as_slice), started, decisions);
    if !log_it {
        return;
    }
    m.insert(repo.to_string(), decisions.iter().map(|d| d.key.clone()).collect());
    drop(g);
    let mut detail = detail;
    detail["started"] = json!(started);
    detail["decisions"] = json!(decisions.iter().map(|d| d.text.clone()).collect::<Vec<_>>());
    detail["keys"] = json!(decisions.iter().map(|d| d.key.clone()).collect::<Vec<_>>());
    log(sup, repo, "tick", "autopilot", &tick_summary(started, decisions), Json::Null, detail);
    for d in fresh {
        log(sup, repo, "decide", "autopilot", &d.text, Json::Null, json!({ "key": d.key }));
    }
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
        if let Some(a) = self.agent.as_deref().and_then(|a| sup.agent_json_of(a)) {
            v["agent_doing"] = a["last_event"].clone();
            v["agent_created"] = a["created"].clone();
            v["agent_cost"] = a["cost_usd"].clone();
        }
        v
    }
}

// ------------------------------------------------------------ work in flight

/// The steps of runs in flight, by run id, and each playing persona's run.
static LIVE: Mutex<Option<HashMap<String, (Vec<Json>, Option<Json>)>>> = Mutex::new(None);
static PLAYING: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

const MAX_STEPS: usize = 400;

fn live_push(run: &str, step: Json, at: Option<Json>) {
    let mut g = LIVE.lock().unwrap();
    let e = g.get_or_insert_with(HashMap::new).entry(run.to_string()).or_default();
    if !step.is_null() {
        e.0.push(step);
        if e.0.len() > MAX_STEPS {
            e.0.remove(0);
        }
    }
    if at.is_some() {
        e.1 = at;
    }
}

fn live_of(run: &str) -> Option<(Vec<Json>, Option<Json>)> {
    LIVE.lock().unwrap().as_ref().and_then(|m| m.get(run).cloned())
}

fn live_end(run: &str) -> Option<(Vec<Json>, Option<Json>)> {
    LIVE.lock().unwrap().as_mut().and_then(|m| m.remove(run))
}

/// A persona's run in flight.
fn playing_run(persona: &str) -> Option<String> {
    PLAYING.lock().unwrap().as_ref().and_then(|m| m.get(persona).cloned())
}

/// What one line of `claude -p --output-format stream-json` says the
/// persona did: what it said, and each tool it used, in a few words.
pub fn steps_of_line(line: &str) -> Vec<(Json, Option<Json>)> {
    let Ok(v) = serde_json::from_str::<Json>(line.trim()) else { return Vec::new() };
    if v["type"] != "assistant" {
        return Vec::new();
    }
    let mut out = Vec::new();
    for c in v["message"]["content"].as_array().into_iter().flatten() {
        match c["type"].as_str() {
            Some("text") => {
                let text = c["text"].as_str().unwrap_or("").trim();
                if text.is_empty() {
                    continue;
                }
                let at = journey_step(text);
                // The final answer's JSON is the result, not a step.
                let shown = match text.find("{\"outcome\"") {
                    Some(i) => text[..i].trim(),
                    None => text,
                };
                let shown = spoken(shown, 1500);
                // A new step of the journey, as a marker of its own.
                if let Some(a) = &at {
                    out.push((json!({ "ts": now_ms(), "kind": "step", "text": format!("Step {} of {}{}", a["n"], a["of"], a["name"].as_str().filter(|n| !n.is_empty()).map(|n| format!(": {n}")).unwrap_or_default()) }), at.clone()));
                }
                if !shown.is_empty() {
                    out.push((json!({ "ts": now_ms(), "kind": "say", "text": shown }), None));
                }
            }
            // What it thinks before it acts: the persona's inner voice.
            Some("thinking") => {
                let text = c["thinking"].as_str().unwrap_or("").trim();
                if !text.is_empty() {
                    out.push((json!({ "ts": now_ms(), "kind": "think", "text": spoken(text, 1500) }), journey_step(text)));
                }
            }
            Some("tool_use") => {
                let name = c["name"].as_str().unwrap_or("tool");
                let i = &c["input"];
                let what = ["command", "file_path", "path", "pattern", "url", "query", "description"].iter().find_map(|k| i[*k].as_str()).unwrap_or("");
                out.push((json!({ "ts": now_ms(), "kind": "tool", "tool": name, "text": one_line(what, 300) }), None));
            }
            _ => {}
        }
    }
    out
}

/// What a persona said, whole (its lines kept, the step markers left out),
/// up to `max` characters.
fn spoken(text: &str, max: usize) -> String {
    let kept: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty() && journey_step(l).is_none()).collect();
    let all = kept.join("\n");
    let mut out: String = all.chars().take(max).collect();
    if all.chars().count() > max {
        out.push('…');
    }
    out
}

/// "Step 3 of 6: Accept a teammate's rules" in what the persona says.
fn journey_step(text: &str) -> Option<Json> {
    for line in text.lines() {
        let l = line.trim().trim_start_matches(['*', '#', '-', ' ']);
        let Some(rest) = l.strip_prefix("Step ").or_else(|| l.strip_prefix("STEP ")) else { continue };
        let (n, rest) = rest.split_once(' ')?;
        let rest = rest.strip_prefix("of ")?;
        let (of, name) = rest.split_once(':').map(|(a, b)| (a, b.trim())).unwrap_or((rest, ""));
        let (Ok(n), Ok(of)) = (n.parse::<u32>(), of.trim().trim_end_matches(['*', '.']).parse::<u32>()) else { continue };
        return Some(json!({ "n": n, "of": of, "name": one_line(name.trim_end_matches('*'), 120) }));
    }
    None
}

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

/// The process groups of runs in flight (a persona's `claude -p` and the
/// builds it starts), ended with the server or a timeout.
static GROUPS: Mutex<Option<HashSet<i32>>> = Mutex::new(None);

fn kill_group(pgid: i32) {
    // The whole group: the run and every build it started.
    let _ = Command::new("kill").args(["-TERM", &format!("-{pgid}")]).stdout(Stdio::null()).stderr(Stdio::null()).status();
}

/// Ends every Converge run still going (the server is stopping).
pub fn stop_runs() {
    let groups: Vec<i32> = GROUPS.lock().unwrap().as_ref().map(|g| g.iter().copied().collect()).unwrap_or_default();
    for g in groups {
        kill_group(g);
    }
}

/// `run_cmd` that hands each line of stdout to `on_line` as it comes; the
/// command runs in a process group of its own, ended with it.
fn run_cmd_lines(program: &str, args: &[String], cwd: &Path, stdin: Option<&str>, envs: &[(&str, String)], timeout: Duration, mut on_line: impl FnMut(&str)) -> Result<(i32, String, String), String> {
    use std::os::unix::process::CommandExt;
    let mut cmd = Command::new(program);
    cmd.args(args).current_dir(cwd).stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped()).env("GIT_TERMINAL_PROMPT", "0").process_group(0);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().map_err(|e| format!("cannot run {program}: {e}"))?;
    let pgid = child.id() as i32;
    GROUPS.lock().unwrap().get_or_insert_with(HashSet::new).insert(pgid);
    struct Ungroup(i32);
    impl Drop for Ungroup {
        fn drop(&mut self) {
            if let Some(g) = GROUPS.lock().unwrap().as_mut() {
                g.remove(&self.0);
            }
        }
    }
    let _ungroup = Ungroup(pgid);
    if let (Some(text), Some(mut w)) = (stdin, child.stdin.take()) {
        let text = text.to_string();
        std::thread::spawn(move || {
            let _ = w.write_all(text.as_bytes());
        });
    }
    let out = child.stdout.take().unwrap();
    let mut err = child.stderr.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        use std::io::BufRead;
        for l in std::io::BufReader::new(out).lines().map_while(Result::ok) {
            if tx.send(l).is_err() {
                break;
            }
        }
    });
    let re = std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = err.read_to_end(&mut b);
        String::from_utf8_lossy(&b).to_string()
    });
    let end = Instant::now() + timeout;
    let mut all = String::new();
    let mut take_line = |l: String, all: &mut String| {
        on_line(&l);
        all.push_str(&l);
        all.push('\n');
    };
    let code = loop {
        while let Ok(l) = rx.try_recv() {
            take_line(l, &mut all);
        }
        match child.try_wait() {
            Ok(Some(st)) => break st.code().unwrap_or(-1),
            Ok(None) if Instant::now() < end => {
                if let Ok(l) = rx.recv_timeout(Duration::from_millis(100)) {
                    take_line(l, &mut all);
                }
            }
            _ => {
                kill_group(pgid);
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{program} did not finish in {} s", timeout.as_secs()));
            }
        }
    };
    // What it printed before it exited.
    while let Ok(l) = rx.recv_timeout(Duration::from_millis(500)) {
        take_line(l, &mut all);
    }
    Ok((code, all, re.join().unwrap_or_default()))
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
            let target = shared_env(sup, &f.repo).remove("CARGO_TARGET_DIR").unwrap_or_default();
            let envs = [("CV_FINDING", f.id.clone()), ("CV_REPO", f.repo.clone()), ("CARGO_TARGET_DIR", target)];
            match run_cmd_lines("sh", &[file.to_string_lossy().to_string()], cwd, None, &envs, Duration::from_secs(timeout), |_| {}) {
                Ok((code, out, err)) => (code, tail(&format!("{out}{err}"), 1500)),
                Err(e) => (-1, e),
            }
        })
        .collect()
}

// -------------------------------------------------------------- the runner

/// Asks the runner for `role` (play, scope, generate): its answer and cost.
fn runner(sup: &Supervisor, role: &str, job: &Json, cwd: &Path, model: &str, timeout: u64) -> Result<Json, String> {
    runner_live(sup, role, job, cwd, model, timeout, None)
}

/// `runner`, keeping each step of a persona's run under `run` as it plays.
fn runner_live(sup: &Supervisor, role: &str, job: &Json, cwd: &Path, model: &str, timeout: u64, run: Option<&str>) -> Result<Json, String> {
    runner_with(sup, role, job, cwd, model, timeout, run, None)
}

/// A persona's memory for one run: its own ChloeCtx store (`home`), the
/// user's models (not downloaded again), and the run's `--settings` and
/// `--mcp-config` files with ChloeCtx's hooks and server.
#[derive(Clone, Debug)]
pub struct Memory {
    /// A store of its own (a persona's); none: the project's usual one (scope).
    pub home: Option<PathBuf>,
    pub models: Option<PathBuf>,
    pub settings: PathBuf,
    pub mcp: PathBuf,
    /// Where the run notes that its prompt is Workbench's, not the user's.
    pub prompt_log: PathBuf,
    /// `CHLOECTX_AGENT_NAME`, `CHLOECTX_PROMPT_SOURCE`, `CHLOECTX_PROMPT_LOG`.
    pub who: Json,
}

/// ChloeCtx's models folder: `$CHLOECTX_MODELS`, else `chloectx/models`
/// under `$XDG_DATA_HOME` (or `~/.local/share`), else the `engram/models`
/// of before the rename when only that one is there.
pub fn models_dir(env_models: Option<String>, xdg: Option<String>, home: Option<String>, exists: impl Fn(&Path) -> bool) -> PathBuf {
    if let Some(m) = env_models.filter(|m| !m.is_empty()) {
        return PathBuf::from(m);
    }
    let base = xdg.filter(|x| !x.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(home.unwrap_or_default()).join(".local/share"));
    let new = base.join("chloectx/models");
    let old = base.join("engram/models");
    if !exists(&new) && exists(&old) {
        old
    } else {
        new
    }
}

/// ChloeCtx's tools that only read: what scoping may ask the project's memory.
pub const MEMORY_READ_TOOLS: [&str; 3] = ["mcp__chloectx__memory_search", "mcp__chloectx__memory_ask", "mcp__chloectx__memory_timeline"];

/// The `claude -p` arguments and environment of a runner call: a persona
/// that plays with its memory gets ChloeCtx's hooks, server and tools, its
/// own store and the user's models; scoping gets the project's own store
/// and only its read tools; every other call runs with ChloeCtx off. Both
/// tell ChloeCtx the prompt is Workbench's, not the user's.
pub fn claude_call(role: &str, model: &str, stream: bool, mem: Option<&Memory>) -> (Vec<String>, Vec<(String, String)>) {
    let mem = mem.filter(|_| matches!(role, "play" | "scope"));
    let mut args: Vec<String> = vec!["-p".into(), "--output-format".into(), if stream { "stream-json" } else { "json" }.into(), "--model".into(), model.to_string()];
    if stream {
        args.push("--verbose".into());
    }
    let mut envs: Vec<(String, String)> = Vec::new();
    match mem {
        Some(m) => {
            args.extend(["--settings".into(), m.settings.to_string_lossy().to_string(), "--mcp-config".into(), m.mcp.to_string_lossy().to_string()]);
            if let Some(h) = &m.home {
                envs.push(("CHLOECTX_HOME".into(), h.to_string_lossy().to_string()));
            }
            if let Some(x) = &m.models {
                envs.push(("CHLOECTX_MODELS".into(), x.to_string_lossy().to_string()));
            }
            for (k, v) in m.who.as_object().into_iter().flatten() {
                envs.push((k.clone(), v.as_str().unwrap_or("").to_string()));
            }
        }
        None => envs.push(("CHLOECTX_DISABLE".into(), "1".into())),
    }
    // Last: --allowedTools takes every word after it (the prompt is on stdin).
    let tools = match (role, mem) {
        ("play", Some(_)) => "Bash Read Write Edit Glob Grep WebFetch mcp__chloectx__*".to_string(),
        ("play", None) => "Bash Read Write Edit Glob Grep WebFetch".to_string(),
        ("scope", Some(_)) => format!("Read Glob Grep {}", MEMORY_READ_TOOLS.join(" ")),
        _ => "Read Glob Grep".to_string(),
    };
    args.extend(["--allowedTools".into(), tools]);
    (args, envs)
}

/// Wires ChloeCtx into a persona's run (`dir`, the run's own folder), or says why it cannot.
fn memory_for(sup: &Supervisor, pe: &Persona, dir: &Path, cwd: &Path) -> Result<Memory, String> {
    let home = sup.cfg.data_dir.join("converge").join("memory").join(&pe.id);
    std::fs::create_dir_all(&home).map_err(|e| format!("cannot make {}: {e}", home.display()))?;
    let models = models_dir(std::env::var("CHLOECTX_MODELS").ok(), std::env::var("XDG_DATA_HOME").ok(), std::env::var("HOME").ok(), |p| p.is_dir());
    wire_memory(sup, &pe.name, dir, cwd, Some(home), Some(models))
}

/// ChloeCtx's hooks and server for a runner call in `cwd`, in files under
/// `dir`: a store of its own (`home`) or the project's usual one. Its
/// prompt is Workbench's words, so ChloeCtx does not take them for the user's.
fn wire_memory(sup: &Supervisor, name: &str, dir: &Path, cwd: &Path, home: Option<PathBuf>, models: Option<PathBuf>) -> Result<Memory, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
    let who = crate::ctxwire::Who::new(name, dir, None, true, None);
    let base = match &home {
        Some(h) => json!({ "env": { "CHLOECTX_HOME": h.to_string_lossy() } }),
        None => json!({}),
    };
    let (settings, mcp) = crate::ctxwire::wire_as(&sup.cfg.chloectx_bin, true, cwd, base, json!({ "mcpServers": {} }), &who).map_err(|e| e.message)?;
    let (sf, mf) = (dir.join("settings.json"), dir.join("mcp.json"));
    std::fs::write(&sf, serde_json::to_string_pretty(&settings).unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&mf, serde_json::to_string_pretty(&mcp).unwrap()).map_err(|e| e.to_string())?;
    Ok(Memory { home, models, settings: sf, mcp: mf, prompt_log: who.log.clone(), who: who.env() })
}

/// The version a `--version` line names (`chloectx 0.3.4` → `0.3.4`).
pub fn version_in(text: &str) -> Option<String> {
    text.split_whitespace().map(|w| w.trim_start_matches('v')).find(|w| w.split('.').count() >= 2 && w.split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())))
        .map(str::to_string)
}

/// The `[package]` version of a Cargo.toml.
pub fn cargo_version(toml: &str) -> Option<String> {
    let mut in_pkg = false;
    for l in toml.lines().map(str::trim) {
        if l.starts_with('[') {
            in_pkg = l == "[package]";
            continue;
        }
        if let Some(v) = l.strip_prefix("version").map(str::trim).and_then(|r| r.strip_prefix('=')).filter(|_| in_pkg) {
            return Some(v.trim().trim_matches('"').to_string());
        }
    }
    None
}

/// Whether the installed ChloeCtx is not this checkout's (`bin` none: it is
/// missing), and how to say so; nothing to say when the checkout has none.
pub fn chloectx_stale(bin: Option<&str>, repo: Option<&str>) -> Option<String> {
    let repo = repo?;
    let n = |v: &str| v.split('.').map(|p| p.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
    match bin {
        None => Some(format!("ChloeCtx is not installed (this checkout has {repo}): scope runs without the project's memory; run ./install.sh")),
        Some(b) if b == repo => None,
        Some(b) => {
            let side = if n(b) < n(repo) { "older" } else { "newer" };
            Some(format!("ChloeCtx {b} is {side} than this checkout's {repo}: scope runs with it anyway; run `chloectx upgrade`"))
        }
    }
}

/// The installed ChloeCtx against the checkout's, checked every ten minutes
/// at most: `{bin, bin_version, repo_version, stale, why}`. Never upgrades.
fn chloectx_check(sup: &Supervisor, repo: &str) -> Json {
    static CACHE: Mutex<Option<HashMap<String, (Instant, Json)>>> = Mutex::new(None);
    if let Some((at, v)) = CACHE.lock().unwrap().as_ref().and_then(|m| m.get(repo).cloned()) {
        if at.elapsed() < Duration::from_secs(600) {
            return v;
        }
    }
    let bin = &sup.cfg.chloectx_bin;
    let bin_version = match crate::trail::run_raw(bin, Path::new(repo), &["--version"], Duration::from_secs(10)) {
        crate::trail::Ran::Out(Json::String(v)) => version_in(&v),
        _ => None,
    };
    let repo_version = std::fs::read_to_string(Path::new(repo).join("chloectx/Cargo.toml")).ok().and_then(|t| cargo_version(&t));
    let why = chloectx_stale(bin_version.as_deref(), repo_version.as_deref());
    let v = json!({ "bin": bin, "bin_version": bin_version, "repo_version": repo_version, "stale": why.is_some(), "why": why });
    if let Some(w) = &why {
        log(sup, repo, "decide", "autopilot", w, Json::Null, v.clone());
    }
    CACHE.lock().unwrap().get_or_insert_with(HashMap::new).insert(repo.to_string(), (Instant::now(), v.clone()));
    v
}

/// Whether `key` was said in the last `secs`; else it is now.
fn said_lately(key: &str, secs: u64) -> bool {
    static SAID: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);
    let mut g = SAID.lock().unwrap();
    let m = g.get_or_insert_with(HashMap::new);
    if m.get(key).is_some_and(|t| t.elapsed() < Duration::from_secs(secs)) {
        return true;
    }
    m.insert(key.to_string(), Instant::now());
    false
}

/// `runner_live`, with a persona's memory when it plays with one.
#[allow(clippy::too_many_arguments)]
fn runner_with(sup: &Supervisor, role: &str, job: &Json, cwd: &Path, model: &str, timeout: u64, run: Option<&str>, mem: Option<&Memory>) -> Result<Json, String> {
    let on_line = |l: &str| {
        if let Some(r) = run {
            for (step, at) in steps_of_line(l) {
                live_push(r, step, at);
            }
        }
    };
    // A persona's run streams its steps; the other roles answer once.
    let (args, call_env) = claude_call(role, model, run.is_some(), mem);
    let call_env: Vec<(&str, String)> = call_env.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
    if let Some(cmd) = std::env::var_os("CHLOEDB_WORKBENCH_CONVERGE_CMD").filter(|c| !c.is_empty()) {
        let (code, out, err) = run_cmd_lines(&cmd.to_string_lossy(), &[role.to_string()], cwd, Some(&job.to_string()), &call_env, Duration::from_secs(timeout), on_line)?;
        let last = out.lines().rev().find_map(|l| serde_json::from_str::<Json>(l.trim()).ok().filter(Json::is_object));
        return match last {
            Some(v) => Ok(v),
            None => Err(format!("the runner ({role}) printed no JSON (exit {code}): {}", one_line(&tail(&format!("{out}{err}"), 400), 400))),
        };
    }
    let prompt = prompt_for(role, job);
    if let Some(m) = mem {
        crate::ctxwire::note_prompt(&m.prompt_log, &prompt, "workbench");
    }
    // Its builds share the repository's target directory (see shared_env).
    let target = job["repo"].as_str().map(|r| shared_env(sup, r).remove("CARGO_TARGET_DIR").unwrap_or_default()).unwrap_or_default();
    let mut envs = call_env;
    if !target.is_empty() {
        envs.push(("CARGO_TARGET_DIR", target));
    }
    let (code, out, err) = run_cmd_lines(&sup.cfg.claude_bin, &args, cwd, Some(&prompt), &envs, Duration::from_secs(timeout), on_line)?;
    // stream-json ends with the result line; json is that line alone.
    let last = out.lines().rev().find(|l| l.contains("\"type\":\"result\"")).unwrap_or(out.trim());
    let outer: Json = serde_json::from_str(last.trim()).map_err(|_| format!("claude -p (exit {code}) answered no JSON: {}", one_line(&tail(&format!("{out}{err}"), 400), 400)))?;
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
    let comps = job["components"].as_array().map(|a| a.iter().filter_map(Json::as_str).collect::<Vec<_>>().join(", ")).filter(|c| !c.is_empty()).unwrap_or_else(|| "other".into());
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
                "You are {name}, {role}. {traits}\n\nYou are a user of the software in the current directory (a checkout of its repository). Your journey: {journey}{verify}\n\nThink aloud in the first person as you go, as in a user interview: before each thing you try, say in a sentence or two what you expect and why, and after it, what you saw and how it felt. As you start each step of your journey, say so on a line of its own, as \"Step N of M: <the step, in a few words>\". Do it the way this person would: read the README and the docs, build or run what you need, try things. You are a user, not a developer of it: do not change its source to make things work; work around problems as a user would and remember each one.\n\nWhen you are done, or stuck, end your answer with one JSON object and nothing after it:\n{{\"outcome\": \"done\" or \"gave_up\", \"ease\": 1 to 5 (how easy it was for you, 5 easiest), \"answer\": \"what you achieved, in two sentences\", \"fixed\": true or false (only when replaying a problem), \"findings\": [{{\"title\": \"one line\", \"detail\": \"what you expected, what happened\", \"kind\": \"bug\" or \"gap\" or \"docs\" or \"ux\" or \"opportunity\", \"theme\": \"onboarding\" or \"reliability\" or \"security\" or \"scalability\" or \"trust\" or \"correctness\" or \"usability\" or \"performance\" or \"observability\" or \"docs\", \"component\": \"the part of the product it is in, one of: {components}\", \"severity\": \"blocker\" or \"major\" or \"minor\" or \"papercut\", \"step\": \"where in the journey\", \"repro\": \"a POSIX sh script, run from the checkout's root, that exits 1 while this problem is there and 0 once it is fixed (empty when you cannot write one)\"}}]}}",
                name = p["name"].as_str().unwrap_or("a user"),
                role = p["role"].as_str().unwrap_or(""),
                traits = p["traits"].as_array().map(|a| a.iter().filter_map(Json::as_str).collect::<Vec<_>>().join(" ")).unwrap_or_default(),
                journey = p["journey"].as_str().unwrap_or("try the product's main use case"),
                components = comps,
            ) + if job["memory"] == true { "\n\nYou have memory of your earlier visits (memory_* tools); use it like a returning user would." } else { "" }
        }
        "scope" => format!(
            "A user of the software in the current directory hit this problem:\n\nTitle: {}\nDetail: {}\nRepro script (fails while the problem is there):\n{}\n\nRead the code (do not change anything) and scope the fix. End your answer with one JSON object and nothing after it:\n{{\"area\": \"the component or folder\", \"files\": [\"paths a fix likely changes, including where its test goes\"], \"root_cause\": \"one or two sentences\", \"acceptance\": \"what must be true when it is fixed (the repro exits 0, plus ...)\", \"size\": \"S\" or \"M\" or \"L\"}}\n\nIf the fix needs a step only a person can take (publishing a package or a release, credentials, account or repository settings, an outside service, the owner's decision), add to that object \"blocked\": {{\"kind\": \"credentials\" or \"publish\" or \"account\" or \"external\" or \"decision\", \"what\": \"why an agent cannot do it\", \"you_do\": \"the exact step for the person\"}}.",
            job["finding"]["title"].as_str().unwrap_or(""),
            job["finding"]["detail"].as_str().unwrap_or(""),
            job["finding"]["repro"].as_str().unwrap_or("(none)")
        ) + if job["memory"] == true { "\n\nBefore planning, ask the project's memory (memory_ask) for earlier decisions and rules about these files." } else { "" },
        "card" => format!(
            "You keep the board of work on the software in the current directory. Its owner asked for this, in their words:\n\n{}\n\nRead what you need of the code and docs to understand it (change nothing), then write it as one card. End your answer with one JSON object and nothing after it:\n{{\"title\": \"one line, what is wrong or wanted\", \"detail\": \"what is expected, what happens now, where (two to four sentences)\", \"kind\": \"bug\" or \"gap\" or \"docs\" or \"ux\" or \"opportunity\" or \"feature\", \"theme\": one of {}, \"component\": the part of the product it is in, one of {}, \"severity\": \"blocker\" or \"major\" or \"minor\" or \"papercut\", \"acceptance\": \"what must be true when it is done\"}}",
            job["text"].as_str().unwrap_or(""),
            THEMES.join(", "),
            comps
        ),
        "personas" => format!(
            "You design test personas for the software in the current directory: people who would really use it, each trying one task end to end. These personas exist already:\n{}\n\nRead the project (the README, the docs, the code's entry points; change nothing) to learn who it is for and what they do with it. Say briefly what you read and what you learn as you go. Then make {} new personas, different from the ones that exist and from each other, covering the parts of the product real users touch most. End your answer with one JSON object and nothing after it:\n{{\"personas\": [{{\"name\": \"first name and role, e.g. Omar, data engineer\", \"role\": \"who they are and what they want, one sentence\", \"traits\": [\"two or three short traits\"], \"journey\": \"the task they try, end to end, in three sentences\", \"surfaces\": [\"the parts of the product it touches\"], \"component\": \"the part it touches most, one of {}\", \"why\": \"why this persona, from what you read\"}}]}}",
            job["personas"],
            job["count"].as_u64().unwrap_or(3),
            comps
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
fn gate_found(sup: &Supervisor, st: &Settings, f: &mut Finding) -> Result<(), ApiError> {
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
    // You choose what is worth reproducing, unless the autopilot may.
    if !st.found_auto {
        if f.needs_you.is_none() {
            f.needs_you = Some(TRIAGE.into());
            f.updated = now_ms();
            put_finding(sup, f)?;
        }
        return Ok(());
    }
    advance_to(sup, f, "reproduce", "dedup", true, "autopilot", &format!("No open finding says the same (best match {best_sim:.2})"), json!({ "best_similarity": best_sim }))
}

const READY: &str = "Ready for development: choose Start development to hand it to a fix agent.";
const TRIAGE: &str = "New finding: choose Reproduce to chase it, or Dismiss.";

/// One cargo target directory per repository, shared by Converge's agents
/// and repro runs: each new worktree reuses what was built, and cargo's lock
/// on it lets one build run at a time instead of a dozen at once.
fn shared_env(sup: &Supervisor, repo: &str) -> std::collections::BTreeMap<String, String> {
    let key = repo.bytes().fold(0u64, |h, b| h.wrapping_mul(31).wrapping_add(b as u64));
    let dir = sup.cfg.data_dir.join("cargo-target").join(format!("{key:016x}"));
    let _ = std::fs::create_dir_all(&dir);
    [("CARGO_TARGET_DIR".to_string(), dir.to_string_lossy().to_string())].into_iter().collect()
}

/// An agent in its own worktree writes `.converge/repro.sh`, with the fix
/// agents' allow-list so it does not stop at every command.
/// `note`: what you said on the card (`talk`), told to the agent.
fn start_repro_agent(sup: &Arc<Supervisor>, lv: &Arc<Levels>, f: &mut Finding, by: &str, note: Option<&str>) -> Result<(), ApiError> {
    let st = settings(sup, &f.repo);
    let short = f.id.trim_start_matches("cv").chars().take(8).collect::<String>();
    let mut task = format!(
        "Write a reproduction script for this problem a user hit, without fixing it.\n\nProblem: {}\n{}\n\nWrite .converge/repro.sh at the repository root: a POSIX sh script, run from the root, that exits 1 while the problem is there and 0 once it is fixed (any other exit, or running past a few minutes, counts as a broken repro, so do not build more than it needs). Run it to check that it exits 1 now, then stop. If the problem cannot be checked by a script, write none and say why.\n\n{}",
        f.title, f.detail, BLOCKED_HELP
    );
    if let Some(n) = note {
        task.push_str(&format!("\n\nThe person who owns this project says about it: {n}"));
    }
    let req = SpawnRequest {
        repo: f.repo.clone(),
        task: Some(task),
        name: Some(format!("cvr-{short}")),
        worktree: Some(true),
        model: st.dev_model.clone(),
        permissions: Some(crate::supervisor::PermissionProfile { allow: st.dev_allow.clone(), ask: Vec::new(), deny: st.dev_deny.clone() }),
        permission_mode: st.dev_permission_mode.clone().filter(|m| !m.is_empty()),
        env: Some(shared_env(sup, &f.repo)),
        ..Default::default()
    };
    let a = crate::goals::spawn(sup, lv, req)?;
    let aid = a["id"].as_str().unwrap_or("").to_string();
    f.agent = Some(aid.clone());
    f.worktree = sup.agent_row(&aid).map(|r| r.cwd);
    // A worktree an earlier repro agent of this name left is reused: what
    // that one wrote is not this one's.
    if let Some(wt) = f.worktree.as_deref() {
        for x in [".converge/repro.sh", ".converge/blocked.json"] {
            let _ = std::fs::remove_file(Path::new(wt).join(x));
        }
    }
    f.queued = None;
    f.needs_you = None;
    failed(sup, f, "repro_agent", by, &format!("Agent {aid} is writing the repro"), json!({ "agent": aid, "note": note }))
}

/// The repro agent's `.converge/repro.sh` becomes the finding's repro.
fn take_repro(sup: &Arc<Supervisor>, f: &mut Finding, by: &str) -> Result<(), ApiError> {
    let wt = f.worktree.clone().ok_or_else(|| ApiError::conflict("no repro agent's worktree"))?;
    let text = std::fs::read_to_string(Path::new(&wt).join(".converge/repro.sh")).map_err(|_| ApiError::conflict("the agent has not written .converge/repro.sh yet"))?;
    f.repro = Some(text);
    f.agent = None;
    f.worktree = None;
    f.needs_you = None;
    failed(sup, f, "repro_agent", by, "The agent's repro was taken", Json::Null)
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
    // Exit 1 says the problem is there, 0 that it is not; anything else
    // (a timeout, a build that broke, a missing tool) says the repro is broken.
    let fails = runs.iter().filter(|(c, _)| *c == 1).count();
    let n = runs.len();
    let evidence = json!({ "commit": rev, "base": base, "runs": runs.iter().map(|(c, o)| json!({ "exit": c, "output": o })).collect::<Vec<_>>() });
    f.repro_attempts += 1;
    if let Some((code, _)) = runs.iter().find(|(c, _)| *c != 0 && *c != 1) {
        let why = if *code < 0 { "did not finish in time".to_string() } else { format!("exited {code}") };
        f.repro = None;
        f.needs_you = (f.repro_attempts >= 2).then(|| format!("The repro is broken: it {why}, not 1 (problem there) or 0 (fixed). Paste one that works, or park it."));
        let msg = format!("repro.sh {why} at {base}: it must exit 1 while the problem is there{}", if f.repro_attempts < 2 { "; an agent writes another" } else { "" });
        return failed(sup, &mut f, "repro_broken", "autopilot", &msg, evidence);
    }
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
        // The project's memory (its own store, read only): earlier decisions and rules.
        if st.scope_memory {
            let _ = chloectx_check(sup, &f.repo);
        }
        let dir = sup.cfg.data_dir.join("converge").join("runs").join(&run.id);
        let wired = st.scope_memory.then(|| wire_memory(sup, "Converge scope", &dir, &repo, None, None));
        let mem = match wired {
            None => None,
            Some(Ok(m)) => Some(m),
            Some(Err(why)) => {
                if !said_lately(&format!("scope-memory:{}", f.repo), 600) {
                    log(sup, &f.repo, "decide", "autopilot", &format!("Scoping runs without the project's memory: {}", one_line(&why, 200)), json!({ "finding": f.id, "run": run.id }), json!({ "why": why }));
                }
                None
            }
        };
        run.use_memory = mem.is_some();
        let job = json!({ "finding": { "id": f.id, "title": f.title, "detail": f.detail, "repro": f.repro, "acceptance": f.scope.as_ref().map(|s| s.acceptance.clone()) }, "memory": mem.is_some() });
        let answer = runner_with(sup, "scope", &job, &repo, &st.model, 900, None, mem.as_ref());
        let _ = std::fs::remove_dir_all(&dir);
        match answer {
            Ok(v) => {
                run.cost_usd = v["cost_usd"].as_f64().unwrap_or(0.0);
                run.state = "done".into();
                let mut sc: Scope = serde_json::from_value(v.clone()).unwrap_or_default();
                if let Some(b) = blocked_of(&v["blocked"], "scope") {
                    sc.size = sc.size.trim().to_uppercase();
                    f.scope = Some(sc);
                    run.ended = Some(now_ms());
                    put_run(sup, &run)?;
                    return set_blocked(sup, &mut f, b);
                }
                sc.size = sc.size.trim().to_uppercase();
                // What you asked for keeps the acceptance you were given.
                let before = f.scope.as_ref().map(|s| s.acceptance.clone()).filter(|a| !a.trim().is_empty());
                if sc.acceptance.trim().is_empty() {
                    sc.acceptance = before.unwrap_or_else(|| "The locked repro exits 0".into());
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
    // The files the scope touches say where it is better than its words.
    let comps = components_of(&repo);
    let paths = format!("{} {}", sc.area, sc.files.join(" ")).to_lowercase();
    if let Some(c) = named_in(&comps, &paths).filter(|c| *c != f.component) {
        let was = std::mem::replace(&mut f.component, c.clone());
        let st = f.stage.clone();
        event(sup, &f, &st, &st, "component", true, "autopilot", &format!("Component {} → {c}, from the files the scope touches", if was.is_empty() { "unset" } else { &was }), json!({ "from": was, "to": c, "files": sc.files, "area": sc.area }));
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
        "Fix this problem a user hit (Workbench Converge, finding {id}, severity {sev}).\n\nProblem: {title}\n{detail}\n\nScope: {area}. Likely files: {files}. Root cause: {cause}\nAcceptance: {acc}\n\nThis script fails now and must exit 0 when you are done (run it from the repository root with sh; do not change it, Workbench keeps its own copy):\n{repro}\n\nAdd a regression test, keep the change to the scope, run the tests, and commit your work on this branch. Then stop: Workbench checks the repro, the tests and the diff, and the persona who found it replays the journey.\n\n{help}",
        id = f.id,
        sev = sev_name(f.severity),
        title = f.title,
        detail = f.detail,
        area = sc.area,
        files = if sc.files.is_empty() { "(not named)".into() } else { sc.files.join(", ") },
        cause = sc.root_cause,
        acc = sc.acceptance,
        repro = f.repro.clone().unwrap_or_default(),
        help = BLOCKED_HELP
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
        permissions: Some(crate::supervisor::PermissionProfile { allow: st.dev_allow.clone(), ask: Vec::new(), deny: st.dev_deny.clone() }),
        permission_mode: st.dev_permission_mode.clone().filter(|m| !m.is_empty()),
        env: Some(shared_env(sup, &f.repo)),
        ..Default::default()
    };
    let a = crate::goals::spawn(sup, lv, req)?;
    let aid = a["id"].as_str().unwrap_or("").to_string();
    f.agent = Some(aid.clone());
    f.branch = Some(branch.clone());
    f.worktree = sup.agent_row(&aid).map(|r| r.cwd);
    f.rounds = 0;
    f.queued = None;
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
        let first = f.last_check.as_ref().and_then(|c| c["problems"][0].as_str().map(|p| one_line(p, 200)));
        f.needs_you = Some(match first {
            Some(p) => format!("{} rounds without passing: {p}. Look at the branch, steer the agent, or park it.", f.rounds),
            None => format!("{} rounds without passing: look at the branch, steer the agent, or park it.", f.rounds),
        });
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
    let agent_name = f.agent.as_deref().map(|a| who(sup, a)).unwrap_or_else(|| "The fix agent".into());
    if let Some(b) = read_blocked(&wt, &agent_name) {
        return set_blocked(sup, &mut f, b);
    }
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
    let text = format!("Workbench's check of your branch did not pass (round {}):\n- {}\nFix these, commit, and stop. {BLOCKED_HELP}", f.rounds + 1, problems.join("\n- "));
    let said = f.agent.as_deref().and_then(|a| crate::capsule::last_summary(sup, a)).map(|t| crate::narrate::sentences(&t, 2, 400));
    f.last_check = Some(json!({ "problems": problems, "agent_said": said, "at": now_ms(), "round": f.rounds + 1 }));
    steer_back(sup, lv, &mut f, &text);
    failed(sup, &mut f, "develop", "autopilot", &problems.join("; "), evidence)
}

/// Plays one persona: explore at the base branch, or verify a finding on its branch.
/// `memory` overrides the persona's `use_memory` for this run.
fn play(sup: &Arc<Supervisor>, lv: &Arc<Levels>, persona_id: &str, finding: Option<&str>, memory: Option<bool>) -> Result<Run, ApiError> {
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
    let refs = json!({ "persona": pe.id, "run": run.id, "finding": run.finding });
    let what = match f.as_ref() {
        Some(f) => format!("{} replays {} on the fix branch", pe.name, short(&f.title, 60)),
        None => format!("{} starts playing: {}", pe.name, one_line(&pe.journey, 100)),
    };
    log(sup, &pe.repo, "persona", &pe.name, &what, refs.clone(), json!({ "purpose": run.purpose, "commit": rev, "journey": pe.journey }));
    let scratch = match Scratch::new(sup, &repo, &rev, &run.id) {
        Ok(x) => x,
        Err(e) => {
            log(sup, &pe.repo, "persona", &pe.name, &format!("{}'s run could not start: {}", pe.name, one_line(&e, 200)), refs, json!({ "error": e }));
            return Err(ApiError::internal(e));
        }
    };
    // Its memory of earlier visits, when it plays with one; else none, and why.
    let run_dir = sup.cfg.data_dir.join("converge").join("runs").join(&run.id);
    let mem = match memory.unwrap_or(pe.use_memory) {
        false => None,
        true => match memory_for(sup, &pe, &run_dir, &scratch.path) {
            Ok(m) => Some(m),
            Err(why) => {
                log(sup, &pe.repo, "decide", "autopilot", &format!("{} plays without memory: {}", pe.name, one_line(&why, 200)), refs.clone(), json!({ "why": why, "chloectx": sup.cfg.chloectx_bin }));
                None
            }
        },
    };
    run.use_memory = mem.is_some();
    if run.use_memory {
        put_run(sup, &run)?;
        log(sup, &pe.repo, "persona", &pe.name, &format!("{} remembers its earlier visits (its own memory)", pe.name), refs.clone(), json!({ "use_memory": true, "store": mem.as_ref().and_then(|m| m.home.as_ref()).map(|h| h.to_string_lossy().to_string()) }));
    }
    let job = json!({
        "persona": { "id": pe.id, "name": pe.name, "role": pe.role, "traits": pe.traits, "journey": pe.journey, "surfaces": pe.surfaces },
        "purpose": run.purpose, "commit": rev, "repo": pe.repo, "components": components_of(&repo), "memory": mem.is_some(),
        "finding": f.as_ref().map(|f| json!({ "id": f.id, "title": f.title, "detail": f.detail, "step": f.step })),
    });
    PLAYING.lock().unwrap().get_or_insert_with(HashMap::new).insert(pe.id.clone(), run.id.clone());
    live_push(&run.id, Json::Null, None);
    let answer = runner_with(sup, "play", &job, &scratch.path, &st.model, st.run_timeout_s, Some(&run.id), mem.as_ref());
    drop(scratch);
    let _ = std::fs::remove_dir_all(&run_dir);
    if let Some(m) = PLAYING.lock().unwrap().as_mut() {
        m.remove(&pe.id);
    }
    if let Some((steps, at)) = live_end(&run.id) {
        run.steps = steps;
        run.at = at;
    }
    run.ended = Some(now_ms());
    pe.last_run = Some(now_ms());
    match answer {
        Err(e) => {
            log(sup, &pe.repo, "persona", &pe.name, &format!("{}'s run failed: {}", pe.name, one_line(&e, 200)), refs, json!({ "error": e, "secs": (now_ms() - run.created) / 1000 }));
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
    let mut end = format!("{} finished: {}", pe.name, match run.outcome.as_deref() { Some("done") => "done", Some("gave_up") => "gave up", Some(o) => o, None => "no outcome" });
    if let Some(e) = run.ease {
        end.push_str(&format!(", ease {e}"));
    }
    if run.purpose == "verify" {
        end.push_str(if run.fixed == Some(true) { ", says it is fixed" } else { ", says it is not fixed" });
    } else {
        end.push_str(&format!(", {} problem{}", run.findings.len(), if run.findings.len() == 1 { "" } else { "s" }));
    }
    end.push_str(&format!(", ${:.2}, {} min", run.cost_usd, (now_ms() - run.created) / 60_000));
    log(sup, &pe.repo, "persona", &pe.name, &end, refs, json!({ "use_memory": run.use_memory, "outcome": run.outcome, "ease": run.ease, "fixed": run.fixed, "findings": run.findings.iter().map(|x| x["title"].clone()).collect::<Vec<_>>(), "cost_usd": run.cost_usd, "answer": run.answer, "secs": (now_ms() - run.created) / 1000 }));
    pe.runs += 1;
    if let Some(e) = run.ease {
        let (last, hist) = if run.use_memory { (&mut pe.ease_memory, &mut pe.ease_hist_memory) } else { (&mut pe.ease, &mut pe.ease_hist) };
        *last = Some(e);
        hist.push(e);
        if hist.len() > 60 {
            hist.remove(0);
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
            nf.component = component_of(&components_of(&repo), x["component"].as_str(), &finding_text(&nf), &pe.surfaces, &nf.theme);
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

/// Whether the run that found a finding played with memory (none: it did not).
fn found_with_memory(sup: &Supervisor, f: &Finding) -> bool {
    f.run.as_deref().and_then(|r| get_run(sup, r).ok()).is_some_and(|r| r.purpose == "explore" && r.use_memory)
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
    // Eases compare only between runs of the same mode (with memory or without).
    let same_mode = found_with_memory(sup, f) == run.use_memory;
    if let (Some(b), Some(a), true) = (f.ease_before, run.ease, same_mode) {
        if a < b {
            problems.push(format!("its ease dropped from {b} to {a}"));
        }
    }
    let serious: Vec<String> = run.findings.iter().filter(|x| sev_of(&x["severity"]) >= 3).filter_map(|x| x["title"].as_str().map(str::to_string)).collect();
    if !serious.is_empty() {
        problems.push(format!("new problems of severity major or worse: {}", serious.join("; ")));
    }
    f.ease_after = run.ease;
    let evidence = json!({ "run": run.id, "outcome": run.outcome, "fixed": run.fixed, "ease_before": f.ease_before, "ease_after": run.ease, "use_memory": run.use_memory, "ease_compared": same_mode, "answer": run.answer, "new_findings": run.findings });
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
    tick_with(sup, lv, repo, wait, true)
}

/// A tick; `personas` false moves findings only (what you picked, while
/// the autopilot is off), and plays and generates no persona.
fn tick_with(sup: &Arc<Supervisor>, lv: &Arc<Levels>, repo: &str, wait: bool, personas: bool) -> Result<Json, ApiError> {
    let st = settings(sup, repo);
    let mut started: Vec<String> = Vec::new();
    // The same, in words, for the activity feed; and what it decided.
    let mut did: Vec<String> = Vec::new();
    let mut dec: Vec<Decision> = Vec::new();
    let mut handles = Vec::new();
    let spent = spent_today(sup, repo);
    let budget_left = st.budget_usd_day - spent;
    if budget_left <= 0.0 {
        decide(&mut dec, "budget", format!("Budget spent (${spent:.2} of ${:.2} today): personas, scoping and new fixes wait", st.budget_usd_day));
    }
    if !personas {
        decide(&mut dec, "off", "Autopilot is off: only what you picked moves; personas do not play");
    }
    let mut fs = findings_of(sup, repo);
    for f in fs.iter_mut().filter(|f| f.state == "active" && f.stage == "found") {
        if busy_what(&f.id).is_none() && f.needs_you.as_deref() != Some(TRIAGE) {
            gate_found(sup, &st, f)?;
            started.push(format!("{} dedup", f.id));
            did.push(format!("the duplicate check of {}", short(&f.title, 50)));
        }
    }
    // What you agreed with a scope agent becomes the plan.
    for f in fs.iter_mut().filter(|f| f.state == "active" && f.chat_kind.as_deref() == Some("scope")) {
        if busy_what(&f.id).is_none() {
            take_chat_scope(sup, f)?;
        }
    }
    let fs = findings_of(sup, repo);
    let active: Vec<&Finding> = fs.iter().filter(|f| f.state == "active").collect();
    let mut repro_agents = active.iter().filter(|f| f.stage == "reproduce" && f.repro.is_none() && f.agent.is_some()).count();
    for f in &active {
        let id = f.id.clone();
        let (s2, l2) = (sup.clone(), lv.clone());
        // Blocked on you: nothing to do until you say it is done.
        if f.blocked.is_some() {
            continue;
        }
        let h = match f.stage.as_str() {
            "reproduce" if f.repro.is_some() => in_background(&id, "reproducing", {
                let id = id.clone();
                move || {
                    let _ = gate_reproduce(&s2, &id);
                }
            }),
            // A repro agent finished: take its script, or say it wrote none.
            "reproduce" if f.agent.as_deref().is_some_and(|a| agent_done(sup, a)) => {
                if let Ok(mut g) = get_finding(sup, &id) {
                    let name = g.agent.as_deref().map(|a| who(sup, a)).unwrap_or_else(|| "The repro agent".into());
                    if let Some(b) = g.worktree.as_deref().and_then(|w| read_blocked(Path::new(w), &name)) {
                        let _ = set_blocked(sup, &mut g, b);
                    } else if take_repro(sup, &mut g, "autopilot").is_err() {
                        let aid = g.agent.take();
                        g.needs_you = Some("The repro agent finished without writing .converge/repro.sh: paste a repro, ask again, or park it.".into());
                        g.repro_attempts += 1;
                        g.updated = now_ms();
                        let _ = put_finding(sup, &g);
                        let name = aid.as_deref().map(|a| who(sup, a)).unwrap_or_else(|| "The repro agent".into());
                        log(sup, repo, "agent", &name, &format!("{}: finished without writing a repro; it waits for you", short(&g.title, 70)), json!({ "finding": g.id, "agent": aid }), json!({ "worktree": g.worktree }));
                    }
                }
                None
            }
            "reproduce" if f.agent.is_some() => None,
            // No repro yet: an agent writes one before you are asked (once per finding).
            "reproduce" if f.repro_attempts < 2 && !f.needs_you.as_deref().is_some_and(|n| n.contains("repro agent")) => {
                if repro_agents >= st.repro_agents {
                    if st.repro_agents > 0 {
                        decide(&mut dec, format!("repro_full:{repro_agents}"), format!("Repro agents busy ({repro_agents} of {}): {} waits for one", st.repro_agents, short(&f.title, 50)));
                    }
                    if f.needs_you.is_none() {
                        let _ = gate_reproduce(sup, &id);
                    }
                } else if let Ok(mut g) = get_finding(sup, &id) {
                    match start_repro_agent(sup, lv, &mut g, "autopilot", None) {
                        Ok(()) => {
                            repro_agents += 1;
                            started.push(format!("{id} repro agent"));
                            did.push(format!("a repro agent for {}", short(&f.title, 50)));
                        }
                        Err(e) if e.status == 429 => {
                            let q = format!("waiting for a free agent slot ({})", one_line(&e.message, 120));
                            if g.queued.as_deref() != Some(q.as_str()) {
                                g.queued = Some(q);
                                let _ = put_finding(sup, &g);
                            }
                            decide(&mut dec, "agent_slots".to_string(), format!("Agent slots full: {}", one_line(&e.message, 120)));
                        }
                        Err(e) => {
                            g.needs_you = Some(format!("Could not start a repro agent: {}", e.message));
                            let _ = put_finding(sup, &g);
                        }
                    }
                }
                None
            }
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
            "develop" if f.agent.as_deref().is_some_and(|a| agent_done(sup, a)) && f.needs_you.is_none() => {
                let h = in_background(&id, "checking the branch", {
                    let id = id.clone();
                    move || {
                        let _ = gate_develop(&s2, &l2, &id);
                    }
                });
                if h.is_some() {
                    let aid = f.agent.clone().unwrap_or_default();
                    log(sup, repo, "agent", &who(sup, &aid), &format!("{}: the fix agent finished its turn; checking its branch", short(&f.title, 70)), json!({ "finding": id, "agent": aid }), json!({ "branch": f.branch, "round": f.rounds + 1 }));
                }
                h
            }
            "verify" if budget_left > 0.0 => match f.persona.clone() {
                // The replay plays in the mode of the run that found it, so their eases compare.
                Some(p) => in_background(&id, "replaying", {
                    let id = id.clone();
                    let memory = Some(found_with_memory(sup, f));
                    move || {
                        let _ = play(&s2, &l2, &p, Some(&id), memory);
                    }
                }),
                // A finding of yours: no persona to replay it, so verify is yours.
                None => {
                    decide(&mut dec, format!("verify_yours:{id}"), format!("Waits for you: {} has no persona to replay it, so Verify is yours", short(&f.title, 50)));
                    None
                }
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
            let what = busy_what(&id).unwrap_or_default();
            did.push(format!("{what} {}", short(&f.title, 50)));
            started.push(format!("{id} {what}"));
            handles.push(h);
        }
    }
    // Prioritize waits for you unless the autopilot may start development.
    if !st.develop_auto {
        for f in fs.iter().filter(|f| f.state == "active" && f.stage == "prioritize" && f.needs_you.is_none()) {
            if let Ok(mut g) = get_finding(sup, &f.id) {
                g.needs_you = Some(READY.into());
                g.updated = now_ms();
                let _ = put_finding(sup, &g);
            }
        }
    }
    // Prioritize → develop, under the WIP limit; doors wait for you.
    let mut in_dev = fs.iter().filter(|f| f.state == "active" && f.stage == "develop").count();
    let mut queue: Vec<Finding> = fs.iter().filter(|f| st.develop_auto && f.state == "active" && f.stage == "prioritize" && f.door.is_none()).cloned().collect();
    queue.sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal)));
    for (i, mut f) in queue.clone().into_iter().enumerate() {
        if in_dev >= st.wip {
            let more = queue.len() - i - 1;
            decide(&mut dec, format!("wip:{}", f.id), format!("WIP full ({in_dev} of {}): {} waits{}", st.wip, short(&f.title, 50), if more > 0 { format!(", and {more} more") } else { String::new() }));
            break;
        }
        if budget_left <= 0.0 {
            break;
        }
        match start_develop(sup, lv, &mut f, "autopilot") {
            Ok(()) => {
                in_dev += 1;
                started.push(format!("{} develop", f.id));
                did.push(format!("development of {}", short(&f.title, 50)));
            }
            Err(e) if e.status == 429 => {
                let q = format!("waiting for a free agent slot ({})", one_line(&e.message, 120));
                if f.queued.as_deref() != Some(q.as_str()) {
                    f.queued = Some(q);
                    put_finding(sup, &f)?;
                }
                decide(&mut dec, "agent_slots".to_string(), format!("Agent slots full: {}", one_line(&e.message, 120)));
                break;
            }
            Err(e) => {
                f.needs_you = Some(format!("Could not start an agent: {}", e.message));
                put_finding(sup, &f)?;
            }
        }
    }
    // What waits for you, by the kind of pick.
    let fs = findings_of(sup, repo);
    let open: Vec<&Finding> = fs.iter().filter(|f| f.state == "active").collect();
    let waits = |pick: &str, list: Vec<&&Finding>, dec: &mut Vec<Decision>| {
        if let Some(first) = list.first() {
            let text = match list.len() {
                1 => format!("Waits for you: {pick} on {}", short(&first.title, 50)),
                n => format!("Waits for you: {pick} on {n} cards (first {})", short(&first.title, 40)),
            };
            decide(dec, format!("wait:{pick}:{}", list.len()), text);
        }
    };
    waits("Reproduce or Dismiss", open.iter().filter(|f| f.stage == "found" && f.needs_you.as_deref() == Some(TRIAGE)).collect(), &mut dec);
    waits("Start development", open.iter().filter(|f| f.stage == "prioritize" && (f.needs_you.as_deref() == Some(READY) || (st.develop_auto && f.door.is_some()))).collect(), &mut dec);
    waits("the merge", open.iter().filter(|f| f.stage == "review" && f.ci.as_deref() == Some("pass")).collect(), &mut dec);
    for f in open.iter().filter(|f| f.stage == "prioritize" && f.door.is_some()) {
        decide(&mut dec, format!("door:{}", f.id), format!("One-way door ({}): {} waits for your Start", f.door.clone().unwrap_or_default(), short(&f.title, 50)));
    }
    let other: Vec<&&Finding> = open.iter().filter(|f| f.needs_you.as_deref().is_some_and(|n| n != TRIAGE && n != READY)).collect();
    if let Some(f) = other.first() {
        decide(&mut dec, format!("wait:other:{}", other.len()), format!("Needs you: {} ({}){}", short(&f.title, 40), one_line(f.needs_you.as_deref().unwrap_or(""), 80), if other.len() > 1 { format!(", and {} more", other.len() - 1) } else { String::new() }));
    }
    // Personas due to replay (not in a tick that only follows your picks).
    let budget_left = if personas { budget_left } else { 0.0 };
    let due_ms = (st.replay_every_h * 3_600_000.0) as i64;
    let mut playing = busy_count("playing");
    let mut next_due: Option<(i64, String, String)> = None;
    for p in personas_of(sup, repo).into_iter().filter(|p| p.state != "retired") {
        if !personas || budget_left <= 0.0 {
            break;
        }
        if busy_what(&format!("persona:{}", p.id)).is_some() {
            continue;
        }
        if let Some(l) = p.last_run.filter(|l| now_ms() - l < due_ms) {
            let at = l + due_ms;
            if next_due.as_ref().is_none_or(|(n, _, _)| at < *n) {
                next_due = Some((at, p.id.clone(), p.name.clone()));
            }
            continue;
        }
        if playing >= st.runs_at_once {
            decide(&mut dec, format!("full:{}", p.id), format!("{} is due, but {playing} of {} persona runs are going: waits", p.name, st.runs_at_once));
            continue;
        }
        let (s2, l2, pid) = (sup.clone(), lv.clone(), p.id.clone());
        if let Some(h) = in_background(&format!("persona:{}", p.id), "playing", move || {
            let _ = play(&s2, &l2, &pid, None, None);
        }) {
            playing += 1;
            started.push(format!("{} plays", p.id));
            did.push(format!("{} plays", p.name));
            decide(&mut dec, format!("due:{}", p.id), format!("{} is due{}: plays now", p.name, p.last_run.map(|l| format!(" (last played {} h ago)", (now_ms() - l) / 3_600_000)).unwrap_or_else(|| " (never played)".into())));
            handles.push(h);
        }
    }
    if let Some((at, pid, name)) = next_due.filter(|_| playing == 0) {
        decide(&mut dec, format!("next:{pid}"), format!("No persona is due: {name} plays next, in {} min", ((at - now_ms()) / 60_000).max(1)));
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
                did.push("a new persona".into());
                decide(&mut dec, "generate", format!("The last 6 runs found {serious} serious problems: a new persona is made"));
                handles.push(h);
            }
        }
    }
    let mut by_stage = serde_json::Map::new();
    for x in STAGES {
        by_stage.insert(x.into(), json!(open.iter().filter(|f| f.stage == x).count()));
    }
    let detail = json!({
        "budget": { "spent": spent, "per_day": st.budget_usd_day, "left": (st.budget_usd_day - spent).max(0.0) },
        "wip": { "in_development": in_dev, "limit": st.wip, "auto": st.develop_auto },
        "runs": { "playing": playing, "at_once": st.runs_at_once },
        "repro_agents": { "running": repro_agents, "limit": st.repro_agents },
        "by_stage": by_stage, "autopilot": st.enabled, "personas_play": personas,
    });
    log_tick(sup, repo, &did, &dec, detail);
    snapshot(sup, repo);
    if wait {
        for h in handles {
            let _ = h.join();
        }
        snapshot(sup, repo);
    }
    Ok(json!({ "started": started, "decisions": dec.iter().map(|d| d.text.clone()).collect::<Vec<_>>(), "spent_today": spent_today(sup, repo), "budget_usd_day": st.budget_usd_day }))
}

/// Whether a finding can move now: an agent's turn ended, a repro waits to
/// run or to be written, a scope to be set.
fn follow_up(sup: &Supervisor, repo: &str) -> bool {
    findings_of(sup, repo).iter().filter(|f| f.state == "active" && busy_what(&f.id).is_none()).any(|f| match f.stage.as_str() {
        "reproduce" => f.agent.as_deref().map_or(f.needs_you.is_none() || f.repro.is_some(), |a| agent_done(sup, a)),
        "develop" => f.agent.as_deref().is_some_and(|a| agent_done(sup, a)) && f.needs_you.is_none(),
        "scope" | "verify" => true,
        "review" => f.pr_url.is_none() && f.needs_you.is_none(),
        _ => false,
    })
}

/// A tick now, in the background, after you chose something, when the
/// autopilot is on: what you picked moves without waiting for the tick.
fn kick(sup: &Arc<Supervisor>, lv: &Arc<Levels>, repo: &str) {
    let st = settings(sup, repo);
    if !st.enabled && !st.follow_picks {
        return;
    }
    let (s2, l2, r) = (sup.clone(), lv.clone(), repo.to_string());
    in_background(&format!("kick:{repo}"), "ticking", move || {
        if let Err(e) = tick_with(&s2, &l2, &r, false, st.enabled) {
            eprintln!("converge: tick of {r}: {}", e.message);
        }
    });
}

/// The autopilot: a thread that ticks each enabled repository.
pub fn start(sup: &Arc<Supervisor>, lv: &Arc<Levels>) {
    let (sup, lv) = (sup.clone(), lv.clone());
    std::thread::spawn(move || {
        backfill_components(&sup);
        let mut last: HashMap<String, Instant> = HashMap::new();
        loop {
            std::thread::sleep(Duration::from_secs(5));
            for (repo, st) in repos(&sup) {
                // Off, it still carries on what you picked (follow_picks).
                if !st.enabled && !(st.follow_picks && follow_up(&sup, &repo)) {
                    continue;
                }
                // Work waiting on a step that just finished goes on within
                // seconds; the rest waits for the tick.
                let every = if follow_up(&sup, &repo) { st.tick_s.clamp(10, 20) } else { st.tick_s.max(10) };
                if last.get(&repo).is_some_and(|l| l.elapsed() < Duration::from_secs(every)) {
                    continue;
                }
                last.insert(repo.clone(), Instant::now());
                if let Err(e) = tick_with(&sup, &lv, &repo, false, st.enabled) {
                    eprintln!("converge: tick of {repo}: {}", e.message);
                }
            }
        }
    });
}

/// Findings from before components (none set) get theirs, once.
fn backfill_components(sup: &Supervisor) {
    let all: Vec<Finding> = docs(sup, "SELECT doc FROM cv_findings LIMIT 20000", &Params::new());
    for mut f in all.into_iter().filter(|f| f.component.is_empty()) {
        f.component = infer_component(sup, &f, None);
        let _ = put_finding(sup, &f);
    }
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
    let least = cov.first().and_then(|c| c["surface"].as_str()).map(str::to_string);
    log(sup, repo, "generate", "autopilot", &format!("Making a new persona{}", least.as_ref().map(|l| format!(" for the least-played part, {l}")).unwrap_or_default()), json!({ "run": run.id }), json!({ "coverage": cov }));
    let v = runner(sup, "generate", &json!({ "personas": ps, "coverage": cov }), Path::new(repo), &st.model, 900);
    run.ended = Some(now_ms());
    let v = match v {
        Ok(v) => v,
        Err(e) => {
            run.state = "failed".into();
            run.error = Some(e.clone());
            put_run(sup, &run)?;
            log(sup, repo, "generate", "autopilot", &format!("Making a persona failed: {}", one_line(&e, 200)), json!({ "run": run.id }), json!({ "error": e }));
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
    log(sup, repo, "generate", "autopilot", &format!("New persona on probation: {}", p.name), json!({ "run": run.id, "persona": p.id }), json!({ "why": p.why, "surfaces": p.surfaces, "cost_usd": run.cost_usd }));
    Ok(p)
}

/// A persona of the generator's answer, on probation; none without a name.
fn persona_of_answer(repo: &str, comps: &[String], v: &Json, origin: &str) -> Option<Persona> {
    let name = v["name"].as_str().map(str::trim).filter(|n| !n.is_empty())?;
    let list = |k: &str| v[k].as_array().map(|a| a.iter().filter_map(Json::as_str).map(|x| one_line(x, 80)).filter(|x| !x.is_empty()).take(8).collect::<Vec<_>>()).unwrap_or_default();
    let mut surfaces = list("surfaces");
    // The component it touches most leads its surfaces, when the project has it.
    if let Some(c) = v["component"].as_str().map(|c| c.trim().to_lowercase()).filter(|c| comps.contains(c) && c != "other") {
        surfaces.retain(|x| x.to_lowercase() != c);
        surfaces.insert(0, c);
    }
    Some(Persona {
        id: crate::levels::new_id("cp"),
        repo: repo.to_string(),
        name: one_line(name, 80),
        role: one_line(v["role"].as_str().unwrap_or(""), 300),
        traits: list("traits"),
        journey: v["journey"].as_str().unwrap_or("").trim().to_string(),
        surfaces,
        origin: origin.into(),
        state: "probation".into(),
        why: v["why"].as_str().map(|w| one_line(w, 400)),
        created: now_ms(),
        ..Default::default()
    })
}

/// Reads the project and makes `count` personas from it, on probation
/// (`origin: codebase`), as run `run_id`: its steps stream like a persona
/// run's, and its record lists the personas made (`findings`: `{persona, name}`).
fn generate_from_codebase(sup: &Arc<Supervisor>, repo: &str, run_id: &str, count: usize) {
    let st = settings(sup, repo);
    let Ok(mut run) = get_run(sup, run_id) else { return };
    let existing = personas_of(sup, repo);
    let ps: Vec<Json> = existing.iter().map(|p| json!({ "name": p.name, "role": p.role, "surfaces": p.surfaces, "state": p.state })).collect();
    let comps = components_of(Path::new(repo));
    log(sup, repo, "generate", "you", &format!("Reading the project to make {count} persona{}", if count == 1 { "" } else { "s" }), json!({ "run": run.id }), json!({ "count": count, "existing": existing.len(), "components": comps }));
    live_push(&run.id, json!({ "ts": now_ms(), "kind": "step", "text": "Reading the project: README, docs and code" }), None);
    let job = json!({ "repo": repo, "count": count, "personas": ps, "components": comps });
    let answer = runner_live(sup, "personas", &job, Path::new(repo), &st.model, 1200, Some(&run.id));
    if let Some((steps, at)) = live_end(&run.id) {
        run.steps = steps;
        run.at = at;
    }
    run.ended = Some(now_ms());
    let v = match answer {
        Ok(v) => v,
        Err(e) => {
            run.state = "failed".into();
            run.error = Some(e.clone());
            let _ = put_run(sup, &run);
            log(sup, repo, "generate", "autopilot", &format!("Reading the project failed: {}", one_line(&e, 200)), json!({ "run": run.id }), json!({ "error": e }));
            return;
        }
    };
    run.cost_usd = v["cost_usd"].as_f64().unwrap_or(0.0);
    let mut names: Vec<String> = existing.iter().map(|p| p.name.to_lowercase()).collect();
    let mut made: Vec<Persona> = Vec::new();
    for x in v["personas"].as_array().cloned().unwrap_or_default() {
        if made.len() >= count {
            break;
        }
        let Some(p) = persona_of_answer(repo, &comps, &x, "codebase") else { continue };
        if names.contains(&p.name.to_lowercase()) || p.journey.is_empty() {
            continue;
        }
        names.push(p.name.to_lowercase());
        if put_persona(sup, &p).is_ok() {
            made.push(p);
        }
    }
    run.findings = made.iter().map(|p| json!({ "persona": p.id, "name": p.name })).collect();
    if made.is_empty() {
        run.state = "failed".into();
        run.error = Some(format!("the answer held no new persona: {}", one_line(&v.to_string(), 300)));
    } else {
        run.state = "done".into();
        run.answer = Some(format!("Made {}", made.iter().map(|p| p.name.clone()).collect::<Vec<_>>().join(", ")));
    }
    let _ = put_run(sup, &run);
    let summary = match made.len() {
        0 => "Reading the project made no new persona".to_string(),
        n => format!("Made {n} persona{} on probation: {} (${:.2})", if n == 1 { "" } else { "s" }, made.iter().map(|p| p.name.clone()).collect::<Vec<_>>().join("; "), run.cost_usd),
    };
    log(sup, repo, "generate", "autopilot", &summary, json!({ "run": run.id }), json!({ "personas": run.findings, "error": run.error, "cost_usd": run.cost_usd, "secs": (now_ms() - run.created) / 1000 }));
}

// ------------------------------------------------------------------ metrics

/// Findings of one component, or all of them.
fn of_component(fs: Vec<Finding>, component: Option<&str>) -> Vec<Finding> {
    match component.map(str::trim).filter(|c| !c.is_empty()) {
        Some(c) => fs.into_iter().filter(|f| f.component.eq_ignore_ascii_case(c)).collect(),
        None => fs,
    }
}

/// Open findings by component: every part of the project, with the ones a
/// finding names that the project no longer has.
fn components_open(repo: &str, fs: &[Finding]) -> Vec<Json> {
    let mut names = components_of(Path::new(repo));
    for f in fs {
        if !f.component.is_empty() && !names.contains(&f.component) {
            names.push(f.component.clone());
        }
    }
    names.iter().map(|n| json!({ "name": n, "open": fs.iter().filter(|f| f.state == "active" && f.stage != "merged" && &f.component == n).count() })).collect()
}

pub fn metrics(sup: &Supervisor, repo: &str) -> Json {
    metrics_of(sup, repo, None)
}

/// The metrics; with a component, its findings' only (the persona runs' are the project's).
pub fn metrics_of(sup: &Supervisor, repo: &str, component: Option<&str>) -> Json {
    let fs = of_component(findings_of(sup, repo), component);
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
        "blocked": {
            "total": active.iter().filter(|f| f.blocked.is_some()).count(),
            "by_kind": BLOCKED_KINDS.iter().map(|k| (k.to_string(), json!(active.iter().filter(|f| f.blocked.as_ref().is_some_and(|b| b.kind == *k)).count()))).filter(|(_, n)| n != &json!(0)).collect::<serde_json::Map<_, _>>(),
            "queued": active.iter().filter(|f| f.queued.is_some()).count(),
        },
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
        f.component = infer_component(sup, &f, x["component"].as_str());
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
        .route("/api/converge/timeline", get(timeline_route))
        .route("/api/converge/activity", get(activity_route))
        .route("/api/converge/ask", post(ask_route))
        .route("/api/converge/findings", post(finding_new))
        .route("/api/converge/findings/bulk", post(bulk_route))
        .route("/api/converge/findings/:id", get(finding_get).put(finding_put).patch(finding_put))
        .route("/api/converge/findings/:id/talk", get(talk_get).post(talk_post))
        .route("/api/converge/findings/:id/:verb", post(finding_verb))
        .route("/api/converge/personas", get(personas_get).post(persona_new))
        .route("/api/converge/personas/generate", post(generate_route))
        .route("/api/converge/personas/:id", axum::routing::patch(persona_patch).put(persona_patch))
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
        let all = findings_of(&sup, &repo);
        let moving = |f: &Finding| busy_what(&f.id).is_some() || f.agent.as_deref().and_then(|a| sup.agent_row(a)).is_some_and(|a| matches!(a.state.as_str(), "starting" | "working"));
        let steps = steps_of(&all, &moving);
        Ok(json!({ "repo": repo, "settings": settings(&sup, &repo), "stages": gates(), "themes": THEMES, "components": components_open(&repo, &all), "findings": fs, "personas": ps, "runs": runs, "metrics": metrics(&sup, &repo), "busy": BUSY.lock().unwrap().clone().unwrap_or_default(), "last_tick": last_tick(&sup, &repo), "chloectx": chloectx_check(&sup, &repo), "steps": steps, "merged_week": merged_week(&all, now_ms()) }))
    })
    .await?;
    Ok(AxJson(v).into_response())
}

/// The findings that wait for you, for the inbox (`/api/needs`): one item
/// per reason, so twenty findings missing a repro are one card.
pub fn needs_items(sup: &Supervisor, keep: &dyn Fn(&str) -> bool) -> Vec<(i64, Json)> {
    let all: Vec<Finding> = docs(sup, "SELECT doc FROM cv_findings WHERE state = 'active' LIMIT 5000", &Params::new());
    let mut groups: Vec<(String, String, Vec<&Finding>)> = Vec::new();
    for f in all.iter().filter(|f| keep(&f.repo)) {
        let why = if let Some(b) = &f.blocked {
            format!("Blocked on you ({}): {}", b.kind, b.you_do)
        } else if let Some(n) = &f.needs_you {
            n.clone()
        } else if f.stage == "review" && f.ci.as_deref() == Some("pass") {
            "CI passed: approve the merge.".into()
        } else if f.stage == "prioritize" && f.door.is_some() {
            "Touches a one-way door: approve it for development.".into()
        } else {
            continue;
        };
        // Reasons that differ only in their details group together; what
        // only you can do groups only with the same step.
        let key = match &f.blocked {
            Some(b) => format!("{}\u{0}blocked\u{0}{}\u{0}{}", f.repo, b.kind, b.you_do.trim()),
            None => format!("{}\u{0}{}", f.repo, why.split([':', '(']).next().unwrap_or(&why).trim()),
        };
        match groups.iter_mut().find(|g| g.0 == key) {
            Some(g) => g.2.push(f),
            None => groups.push((key, why, vec![f])),
        }
    }
    // What only you can do comes first.
    groups.sort_by_key(|g| !g.1.starts_with("Blocked on you"));
    groups
        .into_iter()
        .map(|(key, why, fs)| {
            let since = fs.iter().map(|f| f.updated).min().unwrap_or(0);
            let list: Vec<Json> = fs.iter().take(50).map(|f| json!({ "id": f.id, "title": f.title, "stage": f.stage, "severity": f.severity, "severity_name": sev_name(f.severity), "why": f.needs_you, "blocked": f.blocked.as_ref().map(|b| json!({ "kind": b.kind, "what": b.what, "you_do": b.you_do })) })).collect();
            let repo = fs[0].repo.clone();
            let hash = format!("cv-{:x}", key.bytes().fold(0u64, |h, b| h.wrapping_mul(31).wrapping_add(b as u64)));
            (since, json!({ "kind": "converge", "id": hash, "repo": repo, "since_ms": since, "text": why, "count": fs.len(), "findings": list, "stage": fs[0].stage, "blocked": fs[0].blocked.as_ref().map(|b| json!({ "kind": b.kind, "what": b.what, "you_do": b.you_do })) }))
        })
        .collect()
}

// ------------------------------------------------------------------- talk

/// The longest thing you can say on a card at once.
const TALK_MAX: usize = 8000;

/// A chat agent's permissions: it reads the code and changes nothing.
fn chat_profile() -> crate::supervisor::PermissionProfile {
    crate::supervisor::PermissionProfile {
        allow: ["Read", "Glob", "Grep"].iter().map(|x| x.to_string()).collect(),
        ask: Vec::new(),
        deny: ["Edit", "Write", "MultiEdit", "NotebookEdit", "Bash"].iter().map(|x| x.to_string()).collect(),
    }
}

/// Who answers you on a card, by its stage: the repro agent, the fix
/// agent, a scope agent (the plan) or a card agent (questions).
pub fn talk_kind(state: &str, stage: &str) -> &'static str {
    match (state, stage) {
        ("active" | "cant_reproduce", "reproduce") => "repro",
        ("active", "develop" | "verify" | "review") => "fix",
        ("active", "scope" | "prioritize") => "scope",
        _ => "card",
    }
}

/// Your words, framed for an agent that works on the card.
fn told(text: &str) -> String {
    format!("The person who owns this project says, about this card: {text}")
}

fn talk_rows(sup: &Supervisor, finding: &str) -> Vec<Json> {
    docs(sup, "SELECT doc FROM cv_talk WHERE finding = :f ORDER BY id ASC LIMIT 2000", &Params::new().set("f", s(finding)))
}

fn put_talk(sup: &Supervisor, f: &Finding, who: &str, text: &str, agent: Option<&str>) -> Result<Json, ApiError> {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst) % 1_000_000;
    let ts = now_ms();
    let id = format!("tk{ts:013}{n:06}");
    let doc = json!({ "id": id, "ts": ts, "who": who, "text": text, "agent": agent });
    let p = Params::new()
        .set("id", s(&id))
        .set("finding", s(&f.id))
        .set("repo", s(&f.repo))
        .set("ts", t(ts))
        .set("who", s(who))
        .set("text", s(text))
        .set("agent", agent.map_or(Value::Null, s))
        .set("doc", s(&doc.to_string()));
    sup.db().query("INSERT INTO cv_talk (id, finding, repo, ts, who, text, agent, doc) VALUES (:id, :finding, :repo, :ts, :who, :text, :agent, :doc)", &p).map_err(ApiError::internal)?;
    Ok(doc)
}

/// The finding, the way a chat agent is told it.
fn card_context(f: &Finding) -> String {
    let mut c = format!("Problem: {}\n{}\n\nStage: {}. Component: {}. Severity: {}. Kind: {}.", f.title, f.detail, f.stage, if f.component.is_empty() { "unset" } else { &f.component }, sev_name(f.severity), f.kind);
    if let Some(st) = f.step.as_deref() {
        c.push_str(&format!(" The user was trying to: {st}."));
    }
    if let Some(r) = f.repro.as_deref().filter(|r| !r.trim().is_empty()) {
        c.push_str(&format!("\n\nIts repro script (exits 1 while the problem is there):\n{r}"));
    }
    c
}

/// A read-only agent in the main checkout that answers you: `card`
/// (questions) or `scope` (the plan; its answer may end with a new one).
fn start_chat(sup: &Arc<Supervisor>, lv: &Arc<Levels>, f: &Finding, kind: &str, text: &str) -> Result<String, ApiError> {
    let st = settings(sup, &f.repo);
    let short = f.id.trim_start_matches("cv").chars().take(8).collect::<String>();
    let head = format!("Workbench Converge, finding {}. You may read the code (Read, Glob, Grep); you cannot change anything or run commands.", f.id);
    let task = if kind == "scope" {
        let plan = f.scope.as_ref().map(|s| serde_json::to_string(s).unwrap()).unwrap_or_else(|| "(none yet)".into());
        format!(
            "You talk with the person who owns this project about the plan to fix a problem a user hit ({head})\n\n{}\n\nThe plan now: {plan}\n\nThe person says: {text}\n\nAnswer them in a few sentences. If what they say changes the plan, end your answer with the whole revised plan as one JSON object and nothing after it: {{\"area\": \"the component or folder\", \"files\": [\"paths a fix likely changes\"], \"root_cause\": \"one or two sentences\", \"acceptance\": \"what must be true when it is fixed\", \"size\": \"S\" or \"M\" or \"L\"}}. Workbench takes it as the new plan. If the plan stays, end without JSON. Their next messages come as your next prompts.",
            card_context(f)
        )
    } else {
        format!("You answer the person who owns this project about a problem a user hit ({head})\n\n{}\n\nThe person says: {text}\n\nAnswer in a few sentences, then stop. Their next messages come as your next prompts.", card_context(f))
    };
    let req = SpawnRequest {
        repo: f.repo.clone(),
        task: Some(task),
        name: Some(format!("{}-{short}", if kind == "scope" { "cvs" } else { "cvt" })),
        worktree: Some(false),
        model: Some(st.model.clone()).filter(|m| !m.trim().is_empty()),
        read_only: Some(true),
        permissions: Some(chat_profile()),
        permission_mode: Some("default".into()),
        ..Default::default()
    };
    let a = crate::goals::spawn(sup, lv, req)?;
    Ok(a["id"].as_str().unwrap_or("").to_string())
}

/// The fix agent again, on the finding's worktree and branch, told what
/// you said: its session continues when it has one.
fn restart_fix(sup: &Arc<Supervisor>, lv: &Arc<Levels>, f: &mut Finding, text: &str) -> Result<String, ApiError> {
    let st = settings(sup, &f.repo);
    let words = format!("{}\n\nAnswer them. If it asks for a change, make it on this branch, commit it, and stop. {BLOCKED_HELP}", told(text));
    let profile = crate::supervisor::PermissionProfile { allow: st.dev_allow.clone(), ask: Vec::new(), deny: st.dev_deny.clone() };
    let req = match f.agent.as_deref().filter(|a| sup.agent_row(a).is_some()) {
        Some(old) => {
            let mut req = sup.restart_request(old, Some(words.clone()))?;
            // Nothing to continue: it starts over on its task, with your words.
            if let Some(r) = req.restart.as_mut().filter(|r| r.resume.is_none()) {
                r.prompt = Some(format!("{}\n\n{words}", dev_task(f)));
            }
            req.permissions = Some(profile);
            req.env = Some(shared_env(sup, &f.repo));
            if req.permission_mode.is_none() {
                req.permission_mode = st.dev_permission_mode.clone().filter(|m| !m.is_empty());
            }
            req
        }
        None => {
            let repo = PathBuf::from(&f.repo);
            let short = f.id.trim_start_matches("cv").chars().take(8).collect::<String>();
            SpawnRequest {
                repo: f.repo.clone(),
                task: Some(format!("{}\n\n{words}", dev_task(f))),
                name: Some(format!("cv-{short}")),
                branch: Some(f.branch.clone().unwrap_or_else(|| format!("wb/cv-{short}"))),
                base: Some(base_of(&repo, &st)),
                worktree: Some(true),
                model: st.dev_model.clone(),
                permissions: Some(profile),
                permission_mode: st.dev_permission_mode.clone().filter(|m| !m.is_empty()),
                env: Some(shared_env(sup, &f.repo)),
                ..Default::default()
            }
        }
    };
    let a = crate::goals::spawn(sup, lv, req)?;
    let aid = a["id"].as_str().unwrap_or("").to_string();
    f.agent = Some(aid.clone());
    if let Some(cwd) = sup.agent_row(&aid).map(|r| r.cwd) {
        f.branch = f.branch.clone().or_else(|| git(Path::new(&cwd), &["rev-parse", "--abbrev-ref", "HEAD"]).ok());
        f.worktree = Some(cwd);
    }
    Ok(aid)
}

/// `POST /api/converge/findings/:id/talk {text}`: your words go to the
/// card's agent, started for them when none runs.
pub fn talk(sup: &Arc<Supervisor>, lv: &Arc<Levels>, id: &str, text: &str) -> Result<Json, ApiError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(ApiError::bad("text is empty"));
    }
    if text.len() > TALK_MAX {
        return Err(ApiError::bad(format!("text is longer than {TALK_MAX} bytes")));
    }
    let mut f = get_finding(sup, id)?;
    let kind = talk_kind(&f.state, &f.stage);
    let running = |a: &Option<String>| a.clone().filter(|a| sup.is_running(a));
    let (aid, started) = match kind {
        "repro" | "fix" => match running(&f.agent) {
            Some(a) => {
                crate::levels::steer(sup, lv, &a, &told(text))?;
                (a, false)
            }
            None if kind == "repro" => {
                if busy_what(&f.id).is_some() {
                    return Err(ApiError::conflict("its repro is running now: say it again when it is done"));
                }
                // A hint for a card that could not be reproduced brings it back.
                if f.state == "cant_reproduce" {
                    f.state = "active".into();
                    f.repro_attempts = 0;
                }
                f.repro = None;
                f.repro_hash = None;
                start_repro_agent(sup, lv, &mut f, "you", Some(text))?;
                (f.agent.clone().unwrap_or_default(), true)
            }
            None => (restart_fix(sup, lv, &mut f, text)?, true),
        },
        _ => match running(&f.chat_agent).filter(|_| f.chat_kind.as_deref() == Some(kind)) {
            Some(a) => {
                crate::levels::steer(sup, lv, &a, text)?;
                (a, false)
            }
            None => {
                // The other kind's agent (the card moved on) is not the one to ask.
                if let Some(old) = running(&f.chat_agent) {
                    let _ = sup.stop_with(&old, Some(true));
                }
                let a = start_chat(sup, lv, &f, kind, text)?;
                f.chat_agent = Some(a.clone());
                f.chat_kind = Some(kind.to_string());
                f.chat_taken = None;
                (a, true)
            }
        },
    };
    // Talking to the fix agent is how you take over a stuck round.
    if kind == "fix" && f.stage == "develop" {
        f.needs_you = None;
    }
    f.queued = None;
    let message = put_talk(sup, &f, "you", text, Some(&aid))?;
    f.talk_count += 1;
    f.updated = now_ms();
    put_finding(sup, &f)?;
    let name = who(sup, &aid);
    log(sup, &f.repo, "you", "you", &format!("{}: you said to {name}: “{}”", short(&f.title, 60), one_line(text, 160)), json!({ "finding": f.id, "agent": aid }), json!({ "text": text, "stage": f.stage, "to": kind, "started": started }));
    Ok(json!({ "message": message, "agent": talk_agent(sup, &f), "started": started }))
}

/// The agent your next words go to, if there is one.
fn talk_agent(sup: &Supervisor, f: &Finding) -> Json {
    let kind = talk_kind(&f.state, &f.stage);
    let a = match kind {
        "repro" | "fix" => f.agent.clone(),
        _ => f.chat_agent.clone().filter(|_| f.chat_kind.as_deref() == Some(kind)),
    };
    let Some(a) = a else { return Json::Null };
    match sup.agent_row(&a) {
        Some(r) => json!({ "id": a, "name": r.name, "state": r.state, "doing": sup.agent_json_of(&a).map(|j| j["last_event"].clone()).unwrap_or(Json::Null), "kind": kind }),
        None => Json::Null,
    }
}

/// What an agent said at the end of each of its turns since `since`:
/// `(event id, ts, text)`.
fn replies_of(sup: &Supervisor, agent: &str, since: i64) -> Vec<(String, i64, String)> {
    sup.all_events(agent)
        .unwrap_or_default()
        .iter()
        .filter(|e| e.kind == "Stop" && e.ts >= since)
        .filter_map(|e| {
            let d = crate::trail::data_of(e)?;
            let t = d["last_assistant_message"].as_str().map(str::trim).filter(|t| !t.is_empty())?;
            Some((e.id.clone(), e.ts, t.to_string()))
        })
        .collect()
}

/// `GET /api/converge/findings/:id/talk`: your words and the replies of
/// the agents they went to (and the card's own), by time.
pub fn talk_of(sup: &Supervisor, f: &Finding) -> Json {
    let mine = talk_rows(sup, &f.id);
    let mut msgs = mine.clone();
    if let Some(first) = mine.first().and_then(|r| r["ts"].as_i64()) {
        let mut agents: Vec<String> = Vec::new();
        for a in mine.iter().filter_map(|r| r["agent"].as_str().map(str::to_string)).chain(f.chat_agent.clone()).chain(f.agent.clone()) {
            if !agents.contains(&a) {
                agents.push(a);
            }
        }
        for a in agents {
            let name = who(sup, &a);
            for (id, ts, text) in replies_of(sup, &a, first) {
                msgs.push(json!({ "id": id, "ts": ts, "who": name, "text": text, "agent": a }));
            }
        }
    }
    msgs.sort_by_key(|m| m["ts"].as_i64().unwrap_or(0));
    json!({ "messages": msgs, "agent": talk_agent(sup, f) })
}

/// A plan in a chat answer: its last JSON object, if it is a complete scope.
pub fn scope_of_reply(text: &str, acceptance: Option<&str>) -> Option<Scope> {
    let v = extract_json(text)?;
    v.get("area")?;
    let mut sc: Scope = serde_json::from_value(v).ok()?;
    sc.size = sc.size.trim().to_uppercase();
    if sc.acceptance.trim().is_empty() {
        sc.acceptance = acceptance.unwrap_or("").to_string();
    }
    scope_ok(&sc).ok()?;
    Some(sc)
}

/// The scope agent's latest answer: a new plan in it becomes the
/// finding's scope (scored again). Whether the finding changed.
fn take_chat_scope(sup: &Supervisor, f: &mut Finding) -> Result<bool, ApiError> {
    if f.state != "active" || f.chat_kind.as_deref() != Some("scope") || !matches!(f.stage.as_str(), "scope" | "prioritize") {
        return Ok(false);
    }
    let Some(a) = f.chat_agent.clone() else { return Ok(false) };
    let since = f.chat_taken.map(|t| t + 1).unwrap_or(0);
    let Some((_, ts, text)) = replies_of(sup, &a, since).pop() else { return Ok(false) };
    f.chat_taken = Some(ts);
    let before = f.scope.as_ref().map(|s| s.acceptance.clone());
    match scope_of_reply(&text, before.as_deref()) {
        Some(sc) => {
            let repo = PathBuf::from(&f.repo);
            f.door = door_of(&repo, &sc);
            f.scope = Some(sc.clone());
            f.score = score(f);
            f.updated = now_ms();
            put_finding(sup, f)?;
            let st = f.stage.clone();
            event(sup, f, &st, &st, "scope_talk", true, &a, &format!("The plan changed after you talked: {} (size {}), score {}", sc.area, sc.size, f.score), json!({ "scope": sc, "agent": a, "door": f.door }));
        }
        None => put_finding(sup, f)?,
    }
    Ok(true)
}

// ------------------------------------------------------------------- bulk

/// The verbs `POST /api/converge/findings/bulk` takes.
pub const BULK_VERBS: [&str; 5] = ["reproduce", "dismiss", "park", "start", "pin"];

/// `POST /api/converge/findings/bulk {repo, ids, verb, reason?}`: the verb on
/// each, `{done: [ids], failed: [{id, error}]}`.
fn bulk(sup: &Arc<Supervisor>, lv: &Arc<Levels>, repo: &str, ids: &[String], verb: &str, v: &Json) -> Result<Json, ApiError> {
    if !BULK_VERBS.contains(&verb) {
        return Err(ApiError::bad(format!("verb is one of {}", BULK_VERBS.join(", "))));
    }
    if ids.is_empty() || ids.len() > 500 {
        return Err(ApiError::bad("ids: 1 to 500 finding ids"));
    }
    if verb == "dismiss" && v["reason"].as_str().is_none_or(|r| r.trim().is_empty()) {
        return Err(ApiError::bad("reason is required: why they are not problems (it teaches the personas)"));
    }
    let (mut done, mut bad) = (Vec::new(), Vec::new());
    for id in ids {
        let r = get_finding(sup, id).and_then(|mut f| {
            if f.repo != repo {
                return Err(ApiError::bad(format!("{id} is not a finding of {repo}")));
            }
            do_verb(sup, lv, &mut f, verb, v)
        });
        match r {
            Ok(()) => done.push(id.clone()),
            Err(e) => bad.push(json!({ "id": id, "error": e.message })),
        }
    }
    if !done.is_empty() && matches!(verb, "reproduce" | "start") {
        kick(sup, lv, repo);
    }
    Ok(json!({ "done": done, "failed": bad }))
}

// ---------------------------------------------------- steps and run summary

/// The five steps of the pipeline you see, by the stages each holds.
pub const STEPS: [(&str, &[&str]); 5] = [("problems", &["found"]), ("reproducing", &["reproduce"]), ("ready", &["scope", "prioritize"]), ("building", &["develop", "verify"]), ("merge", &["review"])];

/// The overview's `steps`: each step's active findings, how many have work
/// in flight (a gate, or an agent in its turn), are blocked on you, or queued.
pub fn steps_of(fs: &[Finding], moving: &dyn Fn(&Finding) -> bool) -> Json {
    let list: Vec<Json> = STEPS
        .iter()
        .map(|(step, stages)| {
            let mine: Vec<&Finding> = fs.iter().filter(|f| f.state == "active" && stages.contains(&f.stage.as_str())).collect();
            json!({
                "step": step, "stages": stages, "count": mine.len(),
                "moving": mine.iter().filter(|f| f.blocked.is_none() && moving(f)).count(),
                "blocked": mine.iter().filter(|f| f.blocked.is_some()).count(),
                "queued": mine.iter().filter(|f| f.queued.is_some()).count(),
            })
        })
        .collect();
    Json::Array(list)
}

/// Findings merged in the last seven days.
pub fn merged_week(fs: &[Finding], now: i64) -> usize {
    fs.iter().filter(|f| f.stage == "merged" && !matches!(f.state.as_str(), "duplicate" | "dismissed") && f.stage_since >= now - 7 * 86_400_000).count()
}

/// What a persona's run found, once it ended: `{found, new, known,
/// findings: [{id, title, new, dup_of}]}`. A finding is known when it was
/// merged into an open one, or (not checked yet) one says the same.
pub fn run_summary(run: &Run, fs: &[Finding]) -> Json {
    let mine: Vec<&Finding> = fs.iter().filter(|f| f.run.as_deref() == Some(run.id.as_str())).collect();
    let list: Vec<Json> = mine
        .iter()
        .map(|f| {
            let like = || fs.iter().find(|o| o.id != f.id && o.state == "active" && o.created <= f.created && o.run.as_deref() != Some(run.id.as_str()) && similarity(f, o) >= DUP).map(|o| o.id.clone());
            let dup = match f.state.as_str() {
                "duplicate" => f.dup_of.clone().or_else(|| Some(String::new())),
                _ if f.stage == "found" && f.needs_you.is_none() => like(),
                _ => None,
            };
            json!({ "id": f.id, "title": f.title, "new": dup.is_none(), "dup_of": dup.filter(|d| !d.is_empty()) })
        })
        .collect();
    let new = list.iter().filter(|x| x["new"] == true).count();
    json!({ "found": list.len(), "new": new, "known": list.len() - new, "findings": list })
}

fn persona_json(sup: &Supervisor, p: &Persona) -> Json {
    let mut v = serde_json::to_value(p).unwrap();
    v["busy"] = json!(busy_what(&format!("persona:{}", p.id)));
    let live = playing_run(&p.id);
    v["live_since"] = json!(live.as_deref().and_then(|r| get_run(sup, r).ok()).map(|r| r.created));
    v["live_at"] = json!(live.as_deref().and_then(live_of).and_then(|(_, at)| at));
    v["live_run"] = json!(live);
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
        if let Some(m) = st.dev_permission_mode.as_deref().filter(|m| !m.is_empty()) {
            if !crate::ownperms::MODES.contains(&m) {
                return Err(ApiError::bad(format!("dev_permission_mode is one of {}, not {m:?}", crate::ownperms::MODES.join(", "))));
            }
        }
        crate::supervisor::PermissionProfile { allow: st.dev_allow.clone(), ask: Vec::new(), deny: st.dev_deny.clone() }.check().map_err(ApiError::bad)?;
        if st.budget_usd_day < 0.0 || st.wip > 20 || st.runs_at_once > 20 || st.repro_runs > 20 {
            return Err(ApiError::bad("settings out of range: budget ≥ 0, wip and runs_at_once ≤ 20, repro_runs ≤ 20"));
        }
        let before = serde_json::to_value(settings(&sup, &repo)).unwrap();
        put_settings(&sup, &repo, &st)?;
        let after = serde_json::to_value(&st).unwrap();
        let changed: Vec<String> = after.as_object().into_iter().flatten().filter(|(k, x)| before.get(k.as_str()) != Some(x)).map(|(k, _)| k.clone()).collect();
        if !changed.is_empty() {
            let what = match (before["enabled"] != after["enabled"], st.enabled) {
                (true, true) => "You turned the autopilot on".to_string(),
                (true, false) => "You turned the autopilot off".to_string(),
                _ => format!("You changed the settings: {}", changed.join(", ")),
            };
            log(&sup, &repo, "you", "you", &what, Json::Null, json!({ "changed": changed.iter().map(|k| json!({ "name": k, "from": before[k], "to": after[k] })).collect::<Vec<_>>() }));
        }
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

#[derive(Deserialize, Default)]
struct TimelineQ {
    repo: Option<String>,
    /// 24h, 7d or 30d
    range: Option<String>,
    component: Option<String>,
}

/// When a finding left the open set: merged, or out of the loop (a
/// duplicate, can't reproduce, parked); none while it is open.
fn closed_at(f: &Finding) -> Option<(i64, bool)> {
    if f.stage == "merged" {
        Some((f.stage_since.max(f.created), true))
    } else if f.state != "active" {
        Some((f.updated.max(f.created), false))
    } else {
        None
    }
}

/// Open findings over time, by severity, with what was found and merged in
/// each bucket and the persona runs' mean ease: rebuilt from the findings'
/// own times, so it reaches back to the first one.
pub fn timeline(sup: &Supervisor, repo: &str, range: &str, now: i64, component: Option<&str>) -> Json {
    let (span, n) = match range {
        "7d" => (7 * 86_400_000i64, 28i64),
        "30d" => (30 * 86_400_000i64, 30i64),
        _ => (86_400_000i64, 24i64),
    };
    let step = span / n;
    let end = now - now % step + step;
    let start = end - span;
    let fs = of_component(findings_of(sup, repo), component);
    let runs: Vec<Run> = docs(sup, "SELECT doc FROM cv_runs WHERE repo = :repo AND created >= :since ORDER BY created ASC LIMIT 5000", &Params::new().set("repo", s(repo)).set("since", t(start)));
    // With a component, only the work on its findings counts.
    let ids: HashSet<&str> = fs.iter().map(|f| f.id.as_str()).collect();
    let mine = |finding: Option<&str>| component.is_none_or(|c| c.trim().is_empty()) || finding.is_some_and(|f| ids.contains(f));
    let runs: Vec<Run> = runs.into_iter().filter(|r| mine(r.finding.as_deref())).collect();
    // The agents started: repro agents and fix agents, from the gates' events.
    let agent_starts: Vec<(i64, &'static str)> = sup
        .db()
        .query("SELECT ts, gate, finding, summary FROM cv_events WHERE repo = :repo AND ts >= :since LIMIT 20000", &Params::new().set("repo", s(repo)).set("since", t(start)))
        .unwrap_or_default()
        .iter()
        .filter_map(|r| {
            let ts = r.first().and_then(crate::db::get_ms)?;
            let gate = r.get(1).and_then(get_str)?;
            let finding = r.get(2).and_then(get_str);
            if !mine(finding.as_deref()) {
                return None;
            }
            match gate.as_str() {
                "repro_agent" if r.get(3).and_then(get_str).is_some_and(|x| x.contains("is writing")) => Some((ts, "repro")),
                "slot" => Some((ts, "fix")),
                _ => None,
            }
        })
        .collect();
    let buckets: Vec<Json> = (0..n)
        .map(|i| {
            let (b0, b1) = (start + i * step, start + (i + 1) * step);
            // Open at the bucket's end (now, for the last).
            let at = b1.min(now);
            let mut open = [0i64; 4];
            let (mut found, mut merged, mut dropped) = (0, 0, 0);
            for f in &fs {
                let c = closed_at(f);
                if f.created <= at && c.is_none_or(|(ts, _)| ts > at) {
                    open[(4 - f.severity.clamp(1, 4)) as usize] += 1;
                }
                if f.created >= b0 && f.created < b1 {
                    found += 1;
                }
                if let Some((ts, m)) = c {
                    if ts >= b0 && ts < b1 {
                        if m {
                            merged += 1;
                        } else {
                            dropped += 1;
                        }
                    }
                }
            }
            // The trend of ease is of runs without memory (each visit a first one).
            let eases: Vec<f64> = runs.iter().filter(|r| r.created >= b0 && r.created < b1 && r.purpose == "explore" && !r.use_memory).filter_map(|r| r.ease).collect();
            let ease = (!eases.is_empty()).then(|| eases.iter().sum::<f64>() / eases.len() as f64);
            let inside: Vec<&Run> = runs.iter().filter(|r| r.created >= b0 && r.created < b1).collect();
            let spend: f64 = inside.iter().map(|r| r.cost_usd).sum();
            let starts = |k: &str| agent_starts.iter().filter(|(ts, x)| *ts >= b0 && *ts < b1 && *x == k).count();
            let agents = json!({
                "repro": starts("repro"),
                "fix": starts("fix"),
                "persona": inside.iter().filter(|r| matches!(r.purpose.as_str(), "explore" | "verify")).count(),
                "generate": inside.iter().filter(|r| r.purpose == "generate").count(),
            });
            json!({ "t": b0, "end": b1, "open": { "blocker": open[0], "major": open[1], "minor": open[2], "papercut": open[3] }, "open_total": open.iter().sum::<i64>(), "found": found, "merged": merged, "dropped": dropped, "ease": ease, "runs": eases.len(), "spend_usd": (spend * 10000.0).round() / 10000.0, "agents": agents })
        })
        .collect();
    // The findings merged in the range: found and merged times, for the median.
    let merged: Vec<Json> = fs.iter().filter(|f| f.stage == "merged").filter_map(|f| closed_at(f).map(|(ts, _)| (f, ts))).filter(|(_, ts)| *ts >= start && *ts < end).map(|(f, ts)| json!({ "id": f.id, "title": f.title, "component": f.component, "found_ts": f.created, "merged_ts": ts })).collect();
    json!({ "repo": repo, "range": range, "component": component, "step_ms": step, "start": start, "end": end, "now": now, "buckets": buckets, "merged": merged })
}

async fn timeline_route(State(app): State<App>, Query(q): Query<TimelineQ>) -> ApiResult {
    let sup = app.sup.clone();
    let v = blocking(move || {
        let repo = repo_of(q.repo)?;
        Ok(timeline(&sup, &repo, q.range.as_deref().unwrap_or("24h"), now_ms(), q.component.as_deref()))
    })
    .await?;
    Ok(AxJson(v).into_response())
}

#[derive(Deserialize, Default)]
struct MetricsQ {
    repo: Option<String>,
    component: Option<String>,
}

async fn metrics_route(State(app): State<App>, Query(q): Query<MetricsQ>) -> ApiResult {
    let sup = app.sup.clone();
    let v = blocking(move || {
        let repo = repo_of(q.repo)?;
        let mut m = metrics_of(&sup, &repo, q.component.as_deref());
        m["component"] = json!(q.component);
        Ok(m)
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
    component: Option<String>,
    /// Yours, picked at once: straight to Reproduce.
    pick: Option<bool>,
}

/// A settings row for a repository the board has not seen.
fn ensure_settings(sup: &Supervisor, repo: &str) -> Result<(), ApiError> {
    if docs::<Settings>(sup, "SELECT doc FROM cv_settings WHERE repo = :repo LIMIT 1", &Params::new().set("repo", s(repo))).is_empty() {
        put_settings(sup, repo, &Settings::default())?;
    }
    Ok(())
}

/// A component you gave, checked against the project's.
fn component_given(repo: &str, c: Option<&str>) -> Result<Option<String>, ApiError> {
    let Some(c) = c.map(|c| c.trim().to_lowercase()).filter(|c| !c.is_empty()) else { return Ok(None) };
    let comps = components_of(Path::new(repo));
    if comps.contains(&c) {
        Ok(Some(c))
    } else {
        Err(ApiError::bad(format!("component is one of {}, not {c:?}", comps.join(", "))))
    }
}

async fn finding_new(State(app): State<App>, headers: HeaderMap, body: Bytes) -> ApiResult {
    let b: NewFinding = parse_body(&body)?;
    let pick = b.pick == Some(true);
    if pick {
        user_only(&app, &headers)?;
    }
    let by = caller(&headers);
    let (sup, lv) = (app.sup.clone(), app.lv.clone());
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
        let given = component_given(&repo, b.component.as_deref())?;
        f.component = infer_component(&sup, &f, given.as_deref());
        put_finding(&sup, &f)?;
        event(&sup, &f, "", "found", "reported", true, &by, &format!("Reported by {by}"), Json::Null);
        ensure_settings(&sup, &repo)?;
        let refs = json!({ "finding": f.id, "persona": f.persona });
        if pick {
            advance_to(&sup, &mut f, "reproduce", "asked", true, "you", "You added it and picked it: on to Reproduce", Json::Null)?;
            log(&sup, &repo, "you", "you", &format!("You added {} by hand and sent it to Reproduce", short(&f.title, 70)), refs, json!({ "component": f.component, "severity": sev_name(f.severity) }));
            kick(&sup, &lv, &repo);
        } else if by == "you" {
            log(&sup, &repo, "you", "you", &format!("You added {}", short(&f.title, 70)), refs, json!({ "component": f.component, "severity": sev_name(f.severity) }));
        } else {
            log(&sup, &repo, "agent", &who(&sup, &by), &format!("Reported {}", short(&f.title, 70)), json!({ "finding": f.id, "agent": by }), json!({ "component": f.component, "severity": sev_name(f.severity) }));
        }
        Ok(json!({ "finding": get_finding(&sup, &f.id)?.to_json(&sup) }))
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
    let app_lv = app.lv.clone();
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
        if let Some(c) = v["component"].as_str() {
            let c = component_given(&f.repo, Some(c))?.ok_or_else(|| ApiError::bad("component is empty"))?;
            if c != f.component {
                let was = std::mem::replace(&mut f.component, c.clone());
                let st = f.stage.clone();
                event(&sup, &f, &st, &st, "component", true, &by, &format!("Component {} → {c}", if was.is_empty() { "unset" } else { &was }), json!({ "from": was, "to": c }));
            }
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
        if v["repro"].is_string() {
            kick(&sup, &app_lv, &f.repo);
        }
        Ok(json!({ "finding": f.to_json(&sup) }))
    })
    .await?;
    Ok(AxJson(out).into_response())
}

async fn finding_verb(State(app): State<App>, headers: HeaderMap, AxPath((id, verb)): AxPath<(String, String)>, body: Bytes) -> ApiResult {
    let v: Json = if body.is_empty() { json!({}) } else { parse_body(&body)? };
    if matches!(verb.as_str(), "approve" | "start" | "reproduce" | "dismiss" | "back" | "park" | "unpark" | "pin" | "unpin" | "pr" | "unblock") {
        user_only(&app, &headers)?;
    }
    let (sup, lv) = (app.sup.clone(), app.lv.clone());
    let out = blocking(move || {
        let mut f = get_finding(&sup, &id)?;
        do_verb(&sup, &lv, &mut f, &verb, &v)?;
        if matches!(verb.as_str(), "reproduce" | "start" | "take-repro" | "repro-agent" | "unpark") {
            kick(&sup, &lv, &f.repo);
        }
        Ok(json!({ "finding": get_finding(&sup, &f.id)?.to_json(&sup) }))
    })
    .await?;
    Ok(AxJson(out).into_response())
}

/// One of a finding's verbs (`POST /api/converge/findings/:id/<verb>`),
/// without the kick that follows a pick.
fn do_verb(sup: &Arc<Supervisor>, lv: &Arc<Levels>, f: &mut Finding, verb: &str, v: &Json) -> Result<(), ApiError> {
    {
        match verb {
            "pin" | "unpin" => {
                f.pinned = verb == "pin";
                f.updated = now_ms();
                put_finding(sup, f)?;
            }
            "park" => {
                f.state = "parked".into();
                failed(sup, f, "parked", "you", v["reason"].as_str().unwrap_or("Parked"), Json::Null)?;
            }
            "unblock" => {
                let Some(b) = f.blocked.take() else { return Err(ApiError::conflict("this finding is not blocked")) };
                let note = v["note"].as_str().map(str::trim).filter(|n| !n.is_empty()).map(str::to_string);
                f.needs_you = None;
                if let Some(wt) = f.worktree.as_deref() {
                    let _ = std::fs::remove_file(Path::new(wt).join(".converge/blocked.json"));
                }
                let said = format!("The person did it: {}{}. Carry on from where you stopped.", b.you_do, note.as_deref().map(|n| format!(" ({n})")).unwrap_or_default());
                match f.stage.as_str() {
                    // A finished repro agent: a new one starts on the next tick.
                    "reproduce" if f.agent.as_deref().is_some_and(|a| !sup.is_running(a)) || f.agent.is_none() => {
                        f.agent = None;
                    }
                    _ => {
                        if let Some(a) = f.agent.clone().filter(|a| sup.is_running(a)) {
                            let _ = crate::levels::steer(sup, lv, &a, &said);
                        }
                    }
                }
                let st = f.stage.clone();
                f.updated = now_ms();
                put_finding(sup, f)?;
                event(sup, f, &st, &st, "unblocked", true, "you", &format!("You did it: {}{}", b.you_do, note.as_deref().map(|n| format!(" ({n})")).unwrap_or_default()), json!({ "blocked": b, "note": note }));
                kick(sup, lv, &f.repo);
            }
            "unpark" => {
                f.state = "active".into();
                failed(sup, f, "unparked", "you", "Back in the loop", Json::Null)?;
            }
            "dismiss" => {
                let reason = v["reason"].as_str().map(str::trim).filter(|r| !r.is_empty()).ok_or_else(|| ApiError::bad("reason is required: why it is not a problem (it teaches the persona)"))?;
                f.state = "dismissed".into();
                f.note = Some(reason.to_string());
                failed(sup, f, "dismissed", "you", reason, Json::Null)?;
            }
            "reproduce" => {
                if f.stage != "found" {
                    return Err(ApiError::conflict(format!("{} is in {}, not found", f.id, f.stage)));
                }
                f.needs_you = None;
                advance_to(sup, f, "reproduce", "triage", true, "you", "You chose to reproduce it", Json::Null)?;
            }
            "start" => {
                if f.stage != "prioritize" {
                    return Err(ApiError::conflict(format!("{} is in {}, not prioritize", f.id, f.stage)));
                }
                start_develop(sup, lv, f, "you")?;
            }
            "back" => {
                if !matches!(f.stage.as_str(), "verify" | "review") {
                    return Err(ApiError::conflict(format!("{} is in {}: only verify and review go back to develop", f.id, f.stage)));
                }
                let text = v["text"].as_str().unwrap_or("Your reviewer sent this back.").to_string();
                steer_back(sup, lv, f, &format!("Sent back by your reviewer: {text}"));
                f.ci = None;
                advance_to(sup, f, "develop", "review", false, "you", &text, Json::Null)?;
            }
            "pr" => {
                let url = v["url"].as_str().map(str::trim).filter(|u| u.starts_with("http")).ok_or_else(|| ApiError::bad("url is required: the pull request's address"))?;
                f.pr_url = Some(url.to_string());
                f.ci = Some("pending".into());
                f.needs_you = None;
                failed(sup, f, "pr", "you", &format!("Pull request {url} recorded"), json!({ "url": url }))?;
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
                        advance_to(sup, f, "merged", "approved", true, "you", &format!("Approved and merged {url}"), json!({ "url": url, "sha": v["sha"] }))?;
                    }
                    Ok(v) => return Err(ApiError::new(502, "merge_failed", format!("GitHub did not merge it: {}", one_line(&v.to_string(), 300)))),
                    Err(e) => return Err(ApiError::new(502, "merge_failed", e)),
                }
            }
            "repro-agent" => {
                if f.stage != "reproduce" {
                    return Err(ApiError::conflict(format!("{} is in {}, not reproduce", f.id, f.stage)));
                }
                start_repro_agent(sup, lv, f, "you", None)?;
            }
            "take-repro" => take_repro(sup, f, "you")?,
            _ => return Err(ApiError::not_found(format!("no action {verb}"))),
        }
    }
    Ok(())
}

#[derive(Deserialize, Default)]
struct TalkB {
    text: Option<String>,
}

async fn talk_post(State(app): State<App>, headers: HeaderMap, AxPath(id): AxPath<String>, body: Bytes) -> ApiResult {
    // Only you talk to a card: an agent's key reports, it does not steer.
    user_only(&app, &headers)?;
    let b: TalkB = if body.is_empty() { TalkB::default() } else { parse_body(&body)? };
    let (sup, lv) = (app.sup.clone(), app.lv.clone());
    let v = blocking(move || talk(&sup, &lv, &id, b.text.as_deref().unwrap_or(""))).await?;
    Ok(AxJson(v).into_response())
}

async fn talk_get(State(app): State<App>, AxPath(id): AxPath<String>) -> ApiResult {
    let sup = app.sup.clone();
    let v = blocking(move || {
        let mut f = get_finding(&sup, &id)?;
        take_chat_scope(&sup, &mut f)?;
        Ok(talk_of(&sup, &f))
    })
    .await?;
    Ok(AxJson(v).into_response())
}

#[derive(Deserialize, Default)]
struct BulkB {
    repo: Option<String>,
    #[serde(default)]
    ids: Vec<String>,
    verb: Option<String>,
}

async fn bulk_route(State(app): State<App>, headers: HeaderMap, body: Bytes) -> ApiResult {
    user_only(&app, &headers)?;
    let v: Json = parse_body(&body)?;
    let b: BulkB = serde_json::from_value(v.clone()).map_err(|e| ApiError::bad(e.to_string()))?;
    let (sup, lv) = (app.sup.clone(), app.lv.clone());
    let out = blocking(move || {
        let repo = repo_of(b.repo)?;
        bulk(&sup, &lv, &repo, &b.ids, b.verb.as_deref().unwrap_or(""), &v)
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

#[derive(Deserialize, Default)]
struct GenerateB {
    repo: Option<String>,
    /// `codebase`: read the project and make `count` personas, in the background.
    from: Option<String>,
    count: Option<i64>,
}

async fn generate_route(State(app): State<App>, headers: HeaderMap, body: Bytes) -> ApiResult {
    user_only(&app, &headers)?;
    let b: GenerateB = parse_body(&body)?;
    let sup = app.sup.clone();
    match b.from.as_deref().map(str::trim).filter(|f| !f.is_empty()) {
        None => {
            let v = blocking(move || {
                let repo = repo_of(b.repo)?;
                let p = generate(&sup, &repo)?;
                Ok(json!({ "persona": persona_json(&sup, &p) }))
            })
            .await?;
            Ok((StatusCode::CREATED, AxJson(v)).into_response())
        }
        Some("codebase") => {
            let count = b.count.unwrap_or(3);
            if !(1..=6).contains(&count) {
                return Err(ApiError::bad("count is 1 to 6"));
            }
            let v = blocking(move || {
                let repo = repo_of(b.repo)?;
                ensure_settings(&sup, &repo)?;
                let key = format!("generate:{repo}");
                if busy_what(&key).is_some() {
                    return Err(ApiError::conflict("personas are being made for this project already: wait for that run"));
                }
                let run = Run { id: crate::levels::new_id("cr"), repo: repo.clone(), purpose: "generate".into(), state: "running".into(), created: now_ms(), ..Default::default() };
                put_run(&sup, &run)?;
                live_push(&run.id, Json::Null, None);
                let (s2, r2, id) = (sup.clone(), repo.clone(), run.id.clone());
                if in_background(&key, "generating", move || generate_from_codebase(&s2, &r2, &id, count as usize)).is_none() {
                    let mut run = run;
                    live_end(&run.id);
                    run.state = "failed".into();
                    run.error = Some("another generator started first".into());
                    run.ended = Some(now_ms());
                    put_run(&sup, &run)?;
                    return Err(ApiError::conflict("personas are being made for this project already: wait for that run"));
                }
                Ok(json!({ "run": run }))
            })
            .await?;
            Ok((StatusCode::ACCEPTED, AxJson(v)).into_response())
        }
        Some(x) => Err(ApiError::bad(format!("from is \"codebase\" (or left out for one persona aimed at the least-played part), not {x:?}"))),
    }
}

#[derive(Deserialize, Default)]
struct AskB {
    repo: Option<String>,
    text: Option<String>,
}

const KINDS: [&str; 6] = ["bug", "gap", "docs", "ux", "opportunity", "feature"];

/// A card of the runner's answer to your words: yours, so already picked.
pub fn card_of(repo: &str, comps: &[String], text: &str, v: &Json) -> Option<Finding> {
    let title = v["title"].as_str().map(str::trim).filter(|t| !t.is_empty())?;
    let title = one_line(title, 200);
    let detail = v["detail"].as_str().map(str::trim).filter(|d| !d.is_empty()).map(str::to_string).unwrap_or_else(|| text.trim().to_string());
    let kind = v["kind"].as_str().map(|k| k.trim().to_lowercase()).filter(|k| KINDS.contains(&k.as_str())).unwrap_or_else(|| "feature".into());
    let theme = theme_of(v["theme"].as_str(), &title, &detail);
    let acceptance = v["acceptance"].as_str().map(str::trim).unwrap_or("").to_string();
    let mut f = Finding {
        id: crate::levels::new_id("cv"),
        repo: repo.to_string(),
        component: component_of(comps, v["component"].as_str(), &format!("{title} {detail} {text}"), &[], &theme),
        title,
        detail,
        kind,
        theme,
        severity: sev_of(v.get("severity").unwrap_or(&json!("major"))),
        stage: "found".into(),
        state: "active".into(),
        occurrences: 1,
        scope: (!acceptance.is_empty()).then(|| Scope { acceptance, ..Default::default() }),
        note: Some(format!("You asked: {}", one_line(text, 300))),
        by: "you".into(),
        created: now_ms(),
        updated: now_ms(),
        stage_since: now_ms(),
        ..Default::default()
    };
    f.score = score(&f);
    Some(f)
}

/// Your words become a card in Reproduce: the runner (`card`) writes it.
async fn ask_route(State(app): State<App>, headers: HeaderMap, body: Bytes) -> ApiResult {
    user_only(&app, &headers)?;
    let b: AskB = parse_body(&body)?;
    let text = b.text.as_deref().map(str::trim).filter(|t| !t.is_empty()).ok_or_else(|| ApiError::bad("text is required: what you want, in your words"))?.to_string();
    if text.len() > 8000 {
        return Err(ApiError::bad("text is at most 8000 bytes"));
    }
    let (sup, lv) = (app.sup.clone(), app.lv.clone());
    let v = blocking(move || {
        let repo = repo_of(b.repo)?;
        ensure_settings(&sup, &repo)?;
        let st = settings(&sup, &repo);
        let comps = components_of(Path::new(&repo));
        let mut run = Run { id: crate::levels::new_id("cr"), repo: repo.clone(), purpose: "ask".into(), state: "running".into(), created: now_ms(), ..Default::default() };
        let job = json!({ "repo": repo, "text": text, "themes": THEMES, "components": comps });
        let answer = runner(&sup, "card", &job, Path::new(&repo), &st.model, 300);
        let card = answer.as_ref().ok().and_then(|v| card_of(&repo, &comps, &text, v));
        let (Ok(v), Some(mut f)) = (answer.clone(), card) else {
            let e = match answer {
                Err(e) => e,
                Ok(v) => format!("the answer held no card with a title: {}", one_line(&v.to_string(), 300)),
            };
            log(&sup, &repo, "ask", "you", &format!("Your ask could not become a card: {}", one_line(&e, 200)), Json::Null, json!({ "text": text, "error": e }));
            return Err(ApiError::new(502, "runner_failed", e));
        };
        run.state = "done".into();
        run.ended = Some(now_ms());
        run.cost_usd = v["cost_usd"].as_f64().unwrap_or(0.0);
        run.finding = Some(f.id.clone());
        run.answer = Some(f.title.clone());
        put_run(&sup, &run)?;
        f.run = Some(run.id.clone());
        put_finding(&sup, &f)?;
        event(&sup, &f, "", "found", "reported", true, "you", &format!("You asked: {}", one_line(&text, 300)), json!({ "run": run.id, "text": text }));
        advance_to(&sup, &mut f, "reproduce", "asked", true, "you", "Yours, so already picked: on to Reproduce", json!({ "run": run.id }))?;
        log(&sup, &repo, "ask", "you", &format!("You asked: {} → card {} in Reproduce", short(&text, 60), short(&f.title, 60)), json!({ "finding": f.id, "run": run.id }), json!({ "text": text, "kind": f.kind, "theme": f.theme, "component": f.component, "severity": sev_name(f.severity), "acceptance": f.scope.as_ref().map(|s| s.acceptance.clone()), "cost_usd": run.cost_usd }));
        kick(&sup, &lv, &repo);
        Ok(json!({ "finding": get_finding(&sup, &f.id)?.to_json(&sup), "run": run }))
    })
    .await?;
    Ok((StatusCode::CREATED, AxJson(v)).into_response())
}

#[derive(Deserialize, Default)]
struct ActivityQ {
    repo: Option<String>,
    since: Option<i64>,
    limit: Option<usize>,
}

async fn activity_route(State(app): State<App>, Query(q): Query<ActivityQ>) -> ApiResult {
    let sup = app.sup.clone();
    let v = blocking(move || {
        let repo = repo_of(q.repo)?;
        Ok(json!({ "items": activity(&sup, &repo, q.since.unwrap_or(0), q.limit.unwrap_or(100)), "now": now_ms() }))
    })
    .await?;
    Ok(AxJson(v).into_response())
}

/// Edits a persona: `use_memory`, and its name, role, traits, journey, surfaces.
async fn persona_patch(State(app): State<App>, headers: HeaderMap, AxPath(id): AxPath<String>, body: Bytes) -> ApiResult {
    user_only(&app, &headers)?;
    let v: Json = parse_body(&body)?;
    let sup = app.sup.clone();
    let out = blocking(move || {
        let mut p = get_persona(&sup, &id)?;
        let mut changed: Vec<&str> = Vec::new();
        if let Some(m) = v.get("use_memory").filter(|x| !x.is_null()) {
            let m = m.as_bool().ok_or_else(|| ApiError::bad("use_memory is true or false"))?;
            if m != p.use_memory {
                p.use_memory = m;
                changed.push("use_memory");
            }
        }
        for (k, field) in [("name", &mut p.name), ("role", &mut p.role), ("journey", &mut p.journey)] {
            if let Some(x) = v[k].as_str().map(str::trim) {
                if x.is_empty() && k != "role" {
                    return Err(ApiError::bad(format!("{k} cannot be empty")));
                }
                if x != field.as_str() {
                    *field = if k == "name" { one_line(x, 80) } else { x.to_string() };
                    changed.push(k);
                }
            }
        }
        for (k, field) in [("traits", &mut p.traits), ("surfaces", &mut p.surfaces)] {
            if !v[k].is_null() {
                let list: Vec<String> = serde_json::from_value(v[k].clone()).map_err(|_| ApiError::bad(format!("{k} is a list of strings")))?;
                if &list != field {
                    *field = list;
                    changed.push(k);
                }
            }
        }
        put_persona(&sup, &p)?;
        if !changed.is_empty() {
            let what = match (changed.as_slice(), p.use_memory) {
                (["use_memory"], true) => format!("You let {} remember its earlier visits", p.name),
                (["use_memory"], false) => format!("{} now plays each visit as its first", p.name),
                _ => format!("You changed {}: {}", p.name, changed.join(", ")),
            };
            log(&sup, &p.repo, "you", "you", &what, json!({ "persona": p.id }), json!({ "changed": changed }));
        }
        Ok(json!({ "persona": persona_json(&sup, &p) }))
    })
    .await?;
    Ok(AxJson(out).into_response())
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
                let memory = v["use_memory"].as_bool();
                let h = in_background(&format!("persona:{}", p.id), "playing", move || {
                    let _ = play(&s2, &l2, &pid, None, memory);
                })
                .ok_or_else(|| ApiError::conflict(format!("{} is already playing", p.name)))?;
                log(&sup, &p.repo, "you", "you", &format!("You asked {} to play now", p.name), json!({ "persona": p.id }), Json::Null);
                if wait {
                    let _ = h.join();
                }
                let run = runs_of(&sup, &p.repo, 50).into_iter().find(|r| r.persona.as_deref() == Some(p.id.as_str()) && r.purpose == "explore");
                return Ok(json!({ "persona": persona_json(&sup, &get_persona(&sup, &id)?), "run": run }));
            }
            "retire" => {
                p.state = "retired".into();
                p.why = Some(v["reason"].as_str().unwrap_or("Retired by you").to_string());
                log(&sup, &p.repo, "you", "you", &format!("You retired {}", p.name), json!({ "persona": p.id }), json!({ "reason": p.why }));
            }
            "activate" => {
                p.state = "active".into();
                log(&sup, &p.repo, "you", "you", &format!("You brought {} back", p.name), json!({ "persona": p.id }), Json::Null);
            }
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
    let v = blocking(move || {
        let mut r = get_run(&sup, &id)?;
        if let Some((steps, at)) = live_of(&id) {
            r.steps = steps;
            r.at = at;
        }
        let persona = r.persona.as_deref().and_then(|p| get_persona(&sup, p).ok()).map(|p| json!({ "id": p.id, "name": p.name, "journey": p.journey }));
        let summary = (r.state != "running" && r.purpose == "explore").then(|| run_summary(&r, &findings_of(&sup, &r.repo)));
        Ok(json!({ "run": r, "live": r.state == "running", "persona": persona, "now": now_ms(), "summary": summary }))
    })
    .await?;
    Ok(AxJson(v).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_agents_blocked_file_names_the_step_for_you() {
        let d = tempfile::tempdir().unwrap();
        let wt = d.path();
        assert!(read_blocked(wt, "cv-1").is_none(), "no file");
        std::fs::create_dir_all(wt.join(".converge")).unwrap();
        std::fs::write(wt.join(".converge/blocked.json"), "  \n").unwrap();
        assert!(read_blocked(wt, "cv-1").is_none(), "an empty file");
        std::fs::write(wt.join(".converge/blocked.json"), r#"Here it is: {"kind": "Publish", "what": "No PyPI token here.", "you_do": "Run twine upload dist/*"}"#).unwrap();
        let b = read_blocked(wt, "cv-1").unwrap();
        assert_eq!((b.kind.as_str(), b.what.as_str(), b.you_do.as_str(), b.by.as_str()), ("publish", "No PyPI token here.", "Run twine upload dist/*", "cv-1"));
        std::fs::write(wt.join(".converge/blocked.json"), r#"{"kind": "magic", "you_do": "Ask the owner"}"#).unwrap();
        assert_eq!(read_blocked(wt, "x").unwrap().kind, "other");
        std::fs::write(wt.join(".converge/blocked.json"), "Set the DEPLOY_KEY secret in the repository settings").unwrap();
        let b = read_blocked(wt, "x").unwrap();
        assert_eq!((b.kind.as_str(), b.you_do.as_str()), ("other", "Set the DEPLOY_KEY secret in the repository settings"));
        // A scope's answer names it in its object, or not at all.
        assert!(blocked_of(&json!({ "kind": "credentials" }), "scope").is_none());
        assert_eq!(blocked_of(&json!({ "kind": "credentials", "you_do": "Add the token" }), "scope").unwrap().kind, "credentials");
    }

    #[test]
    fn talk_goes_to_the_agent_of_the_cards_step() {
        assert_eq!(talk_kind("active", "found"), "card");
        assert_eq!(talk_kind("active", "reproduce"), "repro");
        assert_eq!(talk_kind("cant_reproduce", "reproduce"), "repro");
        assert_eq!(talk_kind("active", "scope"), "scope");
        assert_eq!(talk_kind("active", "prioritize"), "scope");
        for st in ["develop", "verify", "review"] {
            assert_eq!(talk_kind("active", st), "fix", "{st}");
        }
        assert_eq!(talk_kind("active", "merged"), "card");
        assert_eq!(talk_kind("parked", "develop"), "card");
    }

    #[test]
    fn a_scope_agents_answer_can_carry_a_new_plan() {
        let text = "Yes, the exporter is in it too.\n\n{\"area\": \"src\", \"files\": [\"src/\", \"exporter/\"], \"root_cause\": \"Not wired.\", \"acceptance\": \"\", \"size\": \"l\"}";
        let sc = scope_of_reply(text, Some("The repro exits 0")).unwrap();
        assert_eq!((sc.size.as_str(), sc.acceptance.as_str(), sc.files.len()), ("L", "The repro exits 0", 2));
        assert!(scope_of_reply(text, None).is_none(), "no acceptance at all");
        assert!(scope_of_reply("The plan stays as it is.", Some("x")).is_none());
        assert!(scope_of_reply("{\"answer\": 1}", Some("x")).is_none(), "not a plan");
        assert!(scope_of_reply("{\"area\": \"src\", \"acceptance\": \"a\", \"size\": \"XL\"}", None).is_none(), "a size that is not S, M or L");
    }

    #[test]
    fn the_steps_count_their_cards_and_the_merges_of_the_week() {
        let now = 10 * 86_400_000;
        let f = |id: &str, stage: &str, state: &str| Finding { id: id.into(), stage: stage.into(), state: state.into(), stage_since: now - 1000, ..Default::default() };
        let mut fs = vec![f("a", "found", "active"), f("b", "found", "parked"), f("c", "scope", "active"), f("d", "prioritize", "active"), f("e", "develop", "active"), f("g", "verify", "active"), f("h", "review", "active"), f("m", "merged", "active"), f("o", "merged", "active")];
        fs[4].blocked = Some(Blocked { kind: "publish".into(), you_do: "x".into(), ..Default::default() });
        fs[5].queued = Some("waiting for a free agent slot".into());
        fs[8].stage_since = now - 8 * 86_400_000;
        let st = steps_of(&fs, &|f: &Finding| f.id == "e" || f.id == "g");
        let by: Vec<(String, u64, u64, u64, u64)> = st.as_array().unwrap().iter().map(|x| (x["step"].as_str().unwrap().to_string(), x["count"].as_u64().unwrap(), x["moving"].as_u64().unwrap(), x["blocked"].as_u64().unwrap(), x["queued"].as_u64().unwrap())).collect();
        assert_eq!(by, vec![("problems".into(), 1, 0, 0, 0), ("reproducing".into(), 0, 0, 0, 0), ("ready".into(), 2, 0, 0, 0), ("building".into(), 2, 1, 1, 1), ("merge".into(), 1, 0, 0, 0)]);
        assert_eq!(st[2]["stages"], json!(["scope", "prioritize"]));
        assert_eq!(merged_week(&fs, now), 1);
    }

    #[test]
    fn a_runs_summary_says_what_was_new() {
        let run = Run { id: "cr1".into(), state: "done".into(), purpose: "explore".into(), ..Default::default() };
        let mk = |id: &str, title: &str, run: &str, created: i64| Finding { id: id.into(), title: title.into(), run: Some(run.into()), state: "active".into(), stage: "found".into(), created, ..Default::default() };
        let old = mk("old", "The export button does nothing on the orders page", "cr0", 1);
        let mut checked = mk("n1", "Totals in the footer are rounded oddly", "cr1", 5);
        checked.needs_you = Some(TRIAGE.into());
        let mut merged = mk("d1", "Export button does nothing on orders page", "cr1", 6);
        merged.state = "duplicate".into();
        merged.dup_of = Some("old".into());
        let unchecked = mk("d2", "The export button does nothing on the orders page today", "cr1", 7);
        let v = run_summary(&run, &[old, checked, merged, unchecked]);
        assert_eq!((v["found"].as_u64(), v["new"].as_u64(), v["known"].as_u64()), (Some(3), Some(1), Some(2)), "{v}");
        assert_eq!(v["findings"][0], json!({ "id": "n1", "title": "Totals in the footer are rounded oddly", "new": true, "dup_of": null }));
        assert_eq!(v["findings"][1]["dup_of"], "old");
        assert_eq!(v["findings"][2]["dup_of"], "old");
    }

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
    fn a_persona_stream_becomes_steps() {
        let l = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Step 2 of 5: Load a CSV\nReading the quickstart."},{"type":"tool_use","name":"Bash","input":{"command":"cargo build --release"}}]}}"#;
        let st = steps_of_line(l);
        assert_eq!(st.len(), 3, "{st:?}");
        assert_eq!(st[0].0["kind"], "step");
        assert_eq!(st[0].0["text"], "Step 2 of 5: Load a CSV");
        assert_eq!(st[0].1, Some(json!({ "n": 2, "of": 5, "name": "Load a CSV" })));
        assert_eq!(st[1].0["kind"], "say");
        assert_eq!(st[1].0["text"], "Reading the quickstart.");
        assert_eq!(st[2].0["tool"], "Bash");
        assert_eq!(st[2].0["text"], "cargo build --release");
        // The final answer is the result, not a step; other lines are nothing.
        assert!(steps_of_line(r#"{"type":"assistant","message":{"content":[{"type":"text","text":"{\"outcome\":\"done\"}"}]}}"#).is_empty());
        assert!(steps_of_line(r#"{"type":"user","message":{}}"#).is_empty());
        let th = steps_of_line(r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"I expect the quickstart to work as written."}]}}"#);
        assert_eq!(th[0].0["kind"], "think");
        assert!(steps_of_line("not json").is_empty());
        assert_eq!(journey_step("**Step 3 of 6: Accept a teammate's rules**"), Some(json!({ "n": 3, "of": 6, "name": "Accept a teammate's rules" })));
    }

    /// A project laid out like ChloeDB's top level.
    fn chloedb_like() -> PathBuf {
        let d = tempfile_dir();
        for (dir, file) in [("chloedb", "Cargo.toml"), ("chloedb-server", "Cargo.toml"), ("chloectx", "Cargo.toml"), ("chloedb-sheets", "Cargo.toml"), ("chloedb-apps", "package.json"), ("chloedb-workbench", "Cargo.toml"), ("chloedb-py", "pyproject.toml"), ("chloedb-js", "package.json"), ("chloedb-java", "pom.xml"), ("docs", "index.html"), ("scripts", "x.sh"), ("packaging", "README.md")] {
            std::fs::create_dir_all(d.join(dir)).unwrap();
            std::fs::write(d.join(dir).join(file), "").unwrap();
        }
        std::fs::write(d.join("install.sh"), "").unwrap();
        d
    }

    #[test]
    fn components_come_from_the_projects_folders() {
        let d = chloedb_like();
        let comps = components_of(&d);
        assert_eq!(comps, ["chloectx", "chloedb", "chloedb-apps", "chloedb-server", "chloedb-sheets", "chloedb-workbench", "sdks", "docs", "packaging", "other"]);
        let c = |text: &str| component_of(&comps, None, text, &[], "usability");
        // Its name or path, the longest first.
        assert_eq!(c("chloedb-server/src/auth/keys.rs leaks a key"), "chloedb-server");
        assert_eq!(c("A crash in chloedb when the index is merged"), "chloedb");
        assert_eq!(c("chloedb-py's client drops the token"), "sdks");
        assert_eq!(c("docs/guide/quickstart.md names an old flag"), "docs");
        assert_eq!(c("install.sh stops behind a proxy"), "packaging");
        // Else the words that point at one.
        assert_eq!(c("The operator's StrataCluster loses a replica on failover"), "chloedb-server");
        assert_eq!(c("Recall forgets the decision the distiller wrote"), "chloectx");
        assert_eq!(c("Summarize in a workbook stops at row 64"), "chloedb-sheets");
        assert_eq!(c("The JDBC driver reports the wrong column type"), "sdks");
        assert_eq!(c("BM25 ranking puts the exact match second"), "chloedb");
        // Given (when the project has it), else the persona's surfaces, else other.
        assert_eq!(component_of(&comps, Some("ChloeCtx"), "anything", &[], "usability"), "chloectx");
        assert_eq!(component_of(&comps, Some("nonsense"), "sheets look odd", &[], "usability"), "chloedb-sheets");
        assert_eq!(component_of(&comps, None, "The button is hard to find", &["chloedb-workbench".into(), "chloectx".into()], "usability"), "chloedb-workbench");
        assert_eq!(component_of(&comps, None, "The button is hard to find", &[], "docs"), "docs");
        assert_eq!(c("The button is hard to find"), "other");
        let _ = std::fs::remove_dir_all(&d);
        // Another project has its own list.
        let e = tempfile_dir();
        std::fs::create_dir_all(e.join("web")).unwrap();
        std::fs::write(e.join("web/package.json"), "{}").unwrap();
        assert_eq!(components_of(&e), ["web", "other"]);
        assert_eq!(component_of(&components_of(&e), None, "web/src/cart.js rounds the total", &[], "correctness"), "web");
        let _ = std::fs::remove_dir_all(&e);
    }

    #[test]
    fn gates_log_as_the_kind_of_activity_they_are() {
        assert_eq!(log_kind("triage", "you"), Some("you"));
        assert_eq!(log_kind("repro_agent", "autopilot"), Some("agent"));
        assert_eq!(log_kind("slot", "autopilot"), Some("agent"));
        assert_eq!(log_kind("ci", "autopilot"), Some("pr"));
        assert_eq!(log_kind("repro_fails", "autopilot"), Some("gate"));
        assert_eq!(log_kind("reported", "cp1"), None);
        assert_eq!(log_kind("asked", "you"), None);
    }

    #[test]
    fn a_tick_is_logged_when_it_starts_something_or_its_decisions_change() {
        let d = |k: &str, t: &str| Decision { key: k.into(), text: t.into() };
        let now = vec![d("budget", "Budget spent ($20.10 of $20.00 today)"), d("wait:Start development:1", "Waits for you: Start development on “x”")];
        // The first: logged, with every decision.
        let (log_it, fresh) = tick_news(None, &[], &now);
        assert!(log_it);
        assert_eq!(fresh.len(), 2);
        let keys: Vec<String> = now.iter().map(|x| x.key.clone()).collect();
        // The same decisions (the money moved, the key did not): not logged.
        let again = vec![d("budget", "Budget spent ($20.30 of $20.00 today)"), now[1].clone()];
        assert_eq!(tick_news(Some(&keys), &[], &again), (false, vec![]));
        // It started something: logged, with no decision repeated.
        let (log_it, fresh) = tick_news(Some(&keys), &["Maya plays".into()], &again);
        assert!(log_it && fresh.is_empty());
        // One went away: logged.
        assert!(tick_news(Some(&keys), &[], &again[..1]).0);
        assert_eq!(tick_summary(&[], &again), "Nothing to start: Budget spent ($20.30 of $20.00 today)");
        assert_eq!(tick_summary(&["Maya plays".into()], &[]), "Started Maya plays");
        assert_eq!(tick_summary(&["a".into(), "b".into()], &[]), "Started 2: a; b");
        assert_eq!(tick_summary(&[], &[]), "Nothing to do");
        let mut v = Vec::new();
        decide(&mut v, "k", "one");
        decide(&mut v, "k", "two");
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn an_ask_becomes_a_card_of_yours() {
        let comps: Vec<String> = ["chloedb", "chloedb-sheets", "docs", "other"].iter().map(|x| x.to_string()).collect();
        let v = json!({ "title": "Sheets: freeze the first row", "detail": "Scrolling hides the header.", "kind": "Feature", "theme": "usability", "component": "chloedb-sheets", "severity": "minor", "acceptance": "The first row stays on screen" });
        let f = card_of("/r", &comps, "freeze the header row in sheets please", &v).unwrap();
        assert_eq!((f.kind.as_str(), f.theme.as_str(), f.component.as_str(), f.severity, f.by.as_str(), f.stage.as_str()), ("feature", "usability", "chloedb-sheets", 2, "you", "found"));
        assert_eq!(f.scope.unwrap().acceptance, "The first row stays on screen");
        // A kind or component it does not know falls back; no title, no card.
        let f = card_of("/r", &comps, "x", &json!({ "title": "Docs: the guide is stale", "kind": "chore", "component": "web" })).unwrap();
        assert_eq!((f.kind.as_str(), f.component.as_str()), ("feature", "docs"));
        assert!(card_of("/r", &comps, "x", &json!({ "detail": "no title" })).is_none());
    }

    #[test]
    fn a_generated_persona_leads_with_its_component() {
        let comps: Vec<String> = ["chloedb", "chloectx", "other"].iter().map(|x| x.to_string()).collect();
        let p = persona_of_answer("/r", &comps, &json!({ "name": "Ana, ML engineer", "role": "r", "traits": ["calm"], "journey": "j", "surfaces": ["recall", "chloectx"], "component": "chloectx", "why": "w" }), "codebase").unwrap();
        assert_eq!((p.state.as_str(), p.origin.as_str()), ("probation", "codebase"));
        assert_eq!(p.surfaces, ["chloectx", "recall"]);
        assert!(persona_of_answer("/r", &comps, &json!({ "name": " " }), "codebase").is_none());
    }

    #[test]
    fn a_persona_with_memory_gets_chloectx_and_others_do_not() {
        let m = Memory { home: Some("/d/converge/memory/cp1".into()), models: Some("/h/.local/share/chloectx/models".into()), settings: "/d/run/settings.json".into(), mcp: "/d/run/mcp.json".into(), prompt_log: "/d/run/p.jsonl".into(), who: json!({ "CHLOECTX_AGENT_NAME": "Maya", "CHLOECTX_PROMPT_SOURCE": "workbench" }) };
        let env = |e: &[(String, String)], k: &str| e.iter().find(|(x, _)| x == k).map(|(_, v)| v.clone());
        // Off (the default), and every role but play: ChloeCtx is off.
        for (role, mem) in [("play", None), ("scope", None), ("card", Some(&m)), ("personas", Some(&m))] {
            let (args, e) = claude_call(role, "haiku", role == "play", mem);
            assert_eq!(env(&e, "CHLOECTX_DISABLE").as_deref(), Some("1"), "{role}");
            assert!(env(&e, "CHLOECTX_HOME").is_none() && !args.contains(&"--settings".to_string()) && !args.join(" ").contains("mcp__chloectx"), "{role} {args:?}");
        }
        let (args, e) = claude_call("play", "haiku", true, Some(&m));
        assert!(env(&e, "CHLOECTX_DISABLE").is_none(), "{e:?}");
        assert_eq!(env(&e, "CHLOECTX_HOME").as_deref(), Some("/d/converge/memory/cp1"));
        assert_eq!(env(&e, "CHLOECTX_MODELS").as_deref(), Some("/h/.local/share/chloectx/models"));
        assert_eq!(env(&e, "CHLOECTX_PROMPT_SOURCE").as_deref(), Some("workbench"));
        let a = args.join(" ");
        assert!(a.contains("--settings /d/run/settings.json --mcp-config /d/run/mcp.json"), "{a}");
        // --allowedTools is last: it takes every word after it.
        assert_eq!(args[args.len() - 2], "--allowedTools");
        assert!(args.last().unwrap().ends_with("WebFetch mcp__chloectx__*"), "{a}");
        // The models: the variable, else XDG's or ~/.local/share's, else engram's when only that is there.
        assert_eq!(models_dir(Some("/m".into()), None, Some("/h".into()), |_| false), PathBuf::from("/m"));
        assert_eq!(models_dir(None, Some("/x".into()), Some("/h".into()), |_| false), PathBuf::from("/x/chloectx/models"));
        assert_eq!(models_dir(None, None, Some("/h".into()), |p| p.ends_with("engram/models")), PathBuf::from("/h/.local/share/engram/models"));
        assert_eq!(models_dir(None, None, Some("/h".into()), |_| true), PathBuf::from("/h/.local/share/chloectx/models"));
        // Scoping: the project's own store (no home, no models of its own), and only the read tools.
        let sm = Memory { home: None, models: None, ..m.clone() };
        let (args, e) = claude_call("scope", "haiku", false, Some(&sm));
        assert!(env(&e, "CHLOECTX_DISABLE").is_none() && env(&e, "CHLOECTX_HOME").is_none() && env(&e, "CHLOECTX_MODELS").is_none(), "{e:?}");
        assert_eq!(env(&e, "CHLOECTX_PROMPT_SOURCE").as_deref(), Some("workbench"));
        assert!(args.join(" ").contains("--settings /d/run/settings.json --mcp-config /d/run/mcp.json"));
        let tools = args.last().unwrap();
        assert_eq!(tools, "Read Glob Grep mcp__chloectx__memory_search mcp__chloectx__memory_ask mcp__chloectx__memory_timeline");
        assert!(!tools.contains("remember") && !tools.contains("forget") && !tools.contains("chloectx__*"));
    }

    #[test]
    fn chloectx_is_checked_against_the_checkout() {
        assert_eq!(version_in("chloectx 0.3.4\n").as_deref(), Some("0.3.4"));
        assert_eq!(version_in("chloectx v1.2"), Some("1.2".into()));
        assert_eq!(version_in("no version here"), None);
        assert_eq!(cargo_version("[package]\nname = \"chloectx\"\nversion = \"0.3.5\"\n\n[dependencies]\nversion = \"9\"\n").as_deref(), Some("0.3.5"));
        assert_eq!(cargo_version("[dependencies]\nversion = \"9\"\n"), None);
        assert_eq!(chloectx_stale(Some("0.3.5"), Some("0.3.5")), None);
        assert_eq!(chloectx_stale(Some("0.3.3"), None), None);
        assert_eq!(chloectx_stale(Some("0.3.3"), Some("0.3.5")).unwrap(), "ChloeCtx 0.3.3 is older than this checkout's 0.3.5: scope runs with it anyway; run `chloectx upgrade`");
        assert!(chloectx_stale(Some("0.10.0"), Some("0.9.1")).unwrap().contains("is newer than"));
        assert!(chloectx_stale(None, Some("0.3.5")).unwrap().starts_with("ChloeCtx is not installed"));
    }

    #[test]
    fn the_seed_parses() {
        let v: Json = serde_json::from_str(SEED).unwrap();
        assert_eq!(v["personas"].as_array().unwrap().len(), 16);
        assert!(v["findings"].as_array().unwrap().len() >= 20);
    }
}
