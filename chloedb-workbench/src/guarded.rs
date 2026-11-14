//! Guarded actions (SPEC.md, "Fourth build", row 9): commands an agent may
//! not run itself (a migration, a deploy script, a push to the default
//! branch, a recursive rm outside its worktree, or one that starts with
//! words the repository's guarded list names). Workbench's PreToolUse hook
//! denies such a Bash call with a reason that points to the MCP tool
//! `propose_change`; the proposal is a row of `change_requests`, with the
//! agent's diff, the risk and what ChloeCtx remembers of such changes.
//! Approve runs the command in the agent's worktree (no terminal, a
//! timeout, its output kept) and tells the agent the result as a steer
//! does (`levels::steer`); Decline tells it why.
//!
//! Agents' keys: each agent's hooks and MCP server get a token of their
//! own ([`agent_token`]), which every route takes but the ones that decide
//! (approve, decline, the guarded list's PUT): those want the token
//! Workbench printed.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::{Path as AxPath, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json as AxJson, Router};
use chloedb::query::Params;
use chloedb::Value;
use serde::Deserialize;
use serde_json::{json, Value as Json};

use crate::db::{get_int, get_ms, get_str, iso, now_ms, opt_int, opt_str, ts};
use crate::levels::{new_id, Levels};
use crate::server::{blocking, parse_body, same_bytes, ApiResult, App};
use crate::supervisor::{hex, one_line, ApiError, Supervisor};
use crate::trail;

/// The longest an approved command runs.
const RUN_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// The most of its output kept (the end).
const OUTPUT_MAX: usize = 64 * 1024;
/// How long Approve waits for the command before answering `running`.
const APPROVE_WAIT: Duration = Duration::from_secs(8);

// Agents' keys.

fn tokens() -> &'static Mutex<HashMap<String, String>> {
    static T: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(HashMap::new()))
}

/// A new token for agent `id`'s hooks and MCP server: it may call the API
/// but not decide a change request.
pub fn agent_token(id: &str) -> String {
    let t = format!("wba{}", hex(32));
    tokens().lock().unwrap().insert(t.clone(), id.to_string());
    t
}

/// Whether `t` is an agent's token (for the server's guard).
pub fn is_agent_token(t: &str) -> bool {
    tokens().lock().unwrap().keys().any(|k| same_bytes(k, t))
}

fn bearer(headers: &HeaderMap) -> Option<String> {
    headers.get(header::AUTHORIZATION).and_then(|h| h.to_str().ok()).and_then(|h| h.strip_prefix("Bearer ")).map(|t| t.trim().to_string())
}

/// The agent whose token the request carries.
fn token_agent(headers: &HeaderMap) -> Option<String> {
    let t = bearer(headers)?;
    tokens().lock().unwrap().iter().find(|(k, _)| same_bytes(k, &t)).map(|(_, a)| a.clone())
}

/// Only the user's token decides.
fn user_only(app: &App, headers: &HeaderMap) -> Result<(), ApiError> {
    match bearer(headers) {
        Some(t) if same_bytes(&t, &app.sup.cfg.token) => Ok(()),
        _ => Err(ApiError::new(403, "forbidden", "only the user decides this: an agent's key can propose a change, not approve or decline one")),
    }
}

// The guarded list.

const BUILTINS: [(&str, &str, &str); 4] = [
    ("migrations", "Database migrations", "prisma migrate deploy, sqlx migrate run, alembic upgrade, rails db:migrate, …"),
    ("deploy", "Deploy scripts", "scripts/deploy*, kubectl apply, terraform apply, fly deploy, npm publish, …"),
    ("push-default", "git push to the default branch", "a push to {default} (this repo's default), or a force push"),
    ("rm-outside", "rm -rf outside the worktree", "a recursive rm of anything not under the agent's own worktree (temporary folders aside)"),
];

#[derive(Clone, Debug, PartialEq)]
pub struct GuardRule {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub on: bool,
}

/// The repository's default branch: origin's HEAD, else main or master,
/// else the main checkout's branch.
fn default_branch(main: &Path) -> String {
    let git = |args: &[&str]| -> Option<String> {
        let out = Command::new("git").arg("-C").arg(main).args(args).env("GIT_TERMINAL_PROMPT", "0").output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|s| !s.is_empty())
    };
    if let Some(r) = git(&["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"]) {
        return r.trim_start_matches("origin/").to_string();
    }
    for b in ["main", "master"] {
        if crate::git::branch_exists(main, b) {
            return b.to_string();
        }
    }
    crate::git::current_branch(main).unwrap_or_else(|| "main".into())
}

fn mark_key(repo: &str) -> String {
    format!("guarded:{repo}")
}

/// The repository's guarded list: the built-in rules (on unless turned
/// off) and its prefix rules.
pub fn rules_of(sup: &Supervisor, repo: &str) -> Vec<GuardRule> {
    let saved: Vec<GuardRule> = sup
        .mark(&mark_key(repo))
        .and_then(|v| serde_json::from_str::<Json>(&v).ok())
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .map(|r| GuardRule {
            id: r["id"].as_str().unwrap_or("").to_string(),
            kind: r["kind"].as_str().unwrap_or("prefix").to_string(),
            label: r["label"].as_str().unwrap_or("").to_string(),
            on: r["on"].as_bool().unwrap_or(true),
        })
        .collect();
    let mut out: Vec<GuardRule> = BUILTINS
        .iter()
        .map(|(id, label, _)| GuardRule { id: id.to_string(), kind: "builtin".into(), label: label.to_string(), on: saved.iter().find(|s| s.id == *id).is_none_or(|s| s.on) })
        .collect();
    out.extend(saved.into_iter().filter(|s| s.kind == "prefix" && !s.label.trim().is_empty()));
    out
}

fn rules_json(sup: &Supervisor, repo: &str) -> Json {
    let main = PathBuf::from(repo);
    let def = default_branch(&main);
    let list: Vec<Json> = rules_of(sup, repo)
        .iter()
        .map(|r| {
            let detail = match BUILTINS.iter().find(|b| b.0 == r.id && r.kind == "builtin") {
                Some(b) => b.2.replace("{default}", &def),
                None => "commands that start with these words".into(),
            };
            json!({ "id": r.id, "kind": r.kind, "label": r.label, "detail": detail, "on": r.on })
        })
        .collect();
    json!({ "repo": repo, "default_branch": def, "rules": list })
}

// Matching a command.

/// A command's words and the separators between its commands: quotes and
/// backslashes as a shell reads them; `$(...)` and backticks are left as
/// words.
fn shell_split(s: &str) -> Vec<Vec<String>> {
    let mut cmds: Vec<Vec<String>> = vec![Vec::new()];
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = s.chars().peekable();
    let push_word = |cmds: &mut Vec<Vec<String>>, word: &mut String, in_word: &mut bool| {
        if *in_word {
            cmds.last_mut().unwrap().push(std::mem::take(word));
            *in_word = false;
        }
    };
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_word = true;
                for d in chars.by_ref() {
                    if d == '\'' {
                        break;
                    }
                    word.push(d);
                }
            }
            '"' => {
                in_word = true;
                while let Some(d) = chars.next() {
                    match d {
                        '"' => break,
                        '\\' => {
                            if let Some(e) = chars.next() {
                                word.push(e);
                            }
                        }
                        _ => word.push(d),
                    }
                }
            }
            '\\' => {
                in_word = true;
                if let Some(e) = chars.next() {
                    if e != '\n' {
                        word.push(e);
                    }
                }
            }
            ';' | '|' | '&' | '\n' | '(' | ')' => {
                push_word(&mut cmds, &mut word, &mut in_word);
                if !cmds.last().unwrap().is_empty() {
                    cmds.push(Vec::new());
                }
            }
            c if c.is_whitespace() => push_word(&mut cmds, &mut word, &mut in_word),
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    push_word(&mut cmds, &mut word, &mut in_word);
    cmds.retain(|c| !c.is_empty());
    cmds
}

/// A command's words without what only runs it (`sudo`, `env`, `FOO=1`,
/// `time`, `nohup`, `exec`, `command`, `npx`, `bunx`, `pnpm exec`, …).
fn bare(words: &[String]) -> &[String] {
    let mut w = words;
    loop {
        match w.first().map(String::as_str) {
            Some("sudo" | "env" | "time" | "nohup" | "exec" | "command" | "npx" | "bunx" | "nice" | "xargs") => w = &w[1..],
            Some(x) if x.contains('=') && !x.starts_with('-') && x.split('=').next().is_some_and(|k| !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')) => w = &w[1..],
            Some("pnpm" | "yarn" | "bundle" | "poetry" | "uv" | "pipenv") if matches!(w.get(1).map(String::as_str), Some("exec" | "dlx" | "run")) && w.len() > 2 && !is_script_runner(w) => w = &w[2..],
            Some(x) if x.starts_with('-') && words.first().is_some_and(|f| f == "sudo" || f == "env") => w = &w[1..],
            _ => return w,
        }
    }
}

/// `npm run x`, `pnpm run x`, `yarn run x`: a package script, kept whole.
fn is_script_runner(w: &[String]) -> bool {
    matches!(w.first().map(String::as_str), Some("npm" | "pnpm" | "yarn" | "bun")) && w.get(1).is_some_and(|x| x == "run" || x == "run-script")
}

fn base_name(w: &str) -> &str {
    w.rsplit('/').next().unwrap_or(w)
}

/// Whether `w` holds `seq` in a row, its first word compared by file name.
fn has_seq(w: &[String], seq: &[&str]) -> bool {
    if w.len() < seq.len() {
        return false;
    }
    (0..=w.len() - seq.len()).any(|i| {
        let first = base_name(&w[i]);
        (first == seq[0] || first.strip_suffix(".py") == Some(seq[0]) || first.strip_suffix(".sh") == Some(seq[0])) && seq[1..].iter().enumerate().all(|(j, s)| w[i + 1 + j] == *s)
    })
}

/// The package script `npm run x` (or `yarn x`) runs.
fn script_of(w: &[String]) -> Option<&str> {
    match w.first().map(String::as_str) {
        Some("npm" | "pnpm" | "bun") if w.get(1).is_some_and(|x| x == "run" || x == "run-script") => w.get(2).map(String::as_str),
        Some("yarn") if w.get(1).is_some_and(|x| x == "run") => w.get(2).map(String::as_str),
        Some("yarn") => w.get(1).map(String::as_str).filter(|x| !x.starts_with('-')),
        Some("make" | "just" | "task") => w.get(1).map(String::as_str),
        _ => None,
    }
}

const MIGRATIONS: [&[&str]; 22] = [
    &["prisma", "migrate", "deploy"],
    &["prisma", "migrate", "reset"],
    &["prisma", "migrate", "dev"],
    &["prisma", "db", "push"],
    &["sqlx", "migrate", "run"],
    &["sqlx", "migrate", "revert"],
    &["diesel", "migration", "run"],
    &["diesel", "migration", "redo"],
    &["alembic", "upgrade"],
    &["alembic", "downgrade"],
    &["rails", "db:migrate"],
    &["rake", "db:migrate"],
    &["knex", "migrate:latest"],
    &["flyway", "migrate"],
    &["manage", "migrate"],
    &["goose", "up"],
    &["dbmate", "up"],
    &["sequelize", "db:migrate"],
    &["typeorm", "migration:run"],
    &["drizzle-kit", "push"],
    &["drizzle-kit", "migrate"],
    &["liquibase", "update"],
];

const DEPLOYS: [&[&str]; 20] = [
    &["kubectl", "apply"],
    &["kubectl", "delete"],
    &["terraform", "apply"],
    &["terraform", "destroy"],
    &["tofu", "apply"],
    &["fly", "deploy"],
    &["flyctl", "deploy"],
    &["helm", "upgrade"],
    &["helm", "install"],
    &["npm", "publish"],
    &["pnpm", "publish"],
    &["yarn", "publish"],
    &["cargo", "publish"],
    &["twine", "upload"],
    &["docker", "push"],
    &["serverless", "deploy"],
    &["sls", "deploy"],
    &["cdk", "deploy"],
    &["wrangler", "deploy"],
    &["firebase", "deploy"],
];

fn is_migration(w: &[String]) -> bool {
    MIGRATIONS.iter().any(|s| has_seq(w, s)) || script_of(w).is_some_and(|s| s.to_ascii_lowercase().contains("migrat"))
}

fn is_deploy(w: &[String]) -> bool {
    if DEPLOYS.iter().any(|s| has_seq(w, s)) || script_of(w).is_some_and(|s| s.to_ascii_lowercase().starts_with("deploy") || s.contains(":deploy") || s == "release") {
        return true;
    }
    let tool = w.first().map(|f| base_name(f)).unwrap_or("");
    if matches!(tool, "vercel" | "netlify") && w.iter().any(|x| x == "--prod" || x == "--production") {
        return true;
    }
    if tool == "gcloud" && w.iter().any(|x| x == "deploy") {
        return true;
    }
    // A deploy script: run itself, or by a shell.
    let script = |x: &String| {
        let b = base_name(x).to_ascii_lowercase();
        b.starts_with("deploy") && (x.contains('/') || b.ends_with(".sh") || b.ends_with(".py"))
    };
    w.first().is_some_and(script) || (matches!(tool, "bash" | "sh" | "zsh" | "python" | "python3" | "node") && w.get(1).is_some_and(script))
}

/// A `git push` that changes the default branch or forces.
fn is_push_default(w: &[String], default: &str, current: Option<&str>) -> bool {
    if w.first().map(|f| base_name(f)) != Some("git") {
        return false;
    }
    // Past git's own options (`-C dir`, `-c k=v`).
    let mut i = 1;
    while i < w.len() && w[i].starts_with('-') {
        i += if matches!(w[i].as_str(), "-C" | "-c" | "--git-dir" | "--work-tree") { 2 } else { 1 };
    }
    if w.get(i).map(String::as_str) != Some("push") {
        return false;
    }
    let args = &w[i + 1..];
    if args.iter().any(|a| matches!(a.as_str(), "-f" | "--force" | "--mirror" | "--all" | "--prune") || a.starts_with("--force-with-lease") || a.starts_with("--force-if-includes")) {
        return true;
    }
    let pos: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    let names = |r: &str| {
        let r = r.trim_start_matches('+');
        let dst = r.rsplit(':').next().unwrap_or(r);
        let dst = dst.strip_prefix("refs/heads/").unwrap_or(dst);
        (dst == default) || (dst == "HEAD" && current == Some(default))
    };
    if pos.len() >= 2 {
        pos[1..].iter().any(|r| r.starts_with('+') || names(r))
    } else {
        // `git push` or `git push origin`: the current branch.
        current == Some(default)
    }
}

/// Lexically: `p` from `cwd`, `..` and `.` resolved.
fn resolve(cwd: &Path, p: &str) -> PathBuf {
    let full = if Path::new(p).is_absolute() { PathBuf::from(p) } else { cwd.join(p) };
    let mut out = PathBuf::new();
    for c in full.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            c => out.push(c),
        }
    }
    out
}

/// A recursive `rm` of a path not under `root` (the agent's worktree;
/// temporary folders aside). `cwd` is where it runs.
fn is_rm_outside(w: &[String], cwd: &Path, root: &Path) -> bool {
    if w.first().map(|f| base_name(f)) != Some("rm") {
        return false;
    }
    let mut recursive = false;
    let mut paths = Vec::new();
    let mut opts = true;
    for a in &w[1..] {
        if opts && a == "--" {
            opts = false;
        } else if opts && a.starts_with("--") {
            recursive |= a == "--recursive";
        } else if opts && a.starts_with('-') && a.len() > 1 {
            recursive |= a.contains('r') || a.contains('R');
        } else {
            paths.push(a.as_str());
        }
    }
    if !recursive {
        return false;
    }
    let tmp: Vec<PathBuf> = [std::env::temp_dir(), PathBuf::from("/tmp"), PathBuf::from("/var/tmp")].into_iter().collect();
    paths.iter().any(|p| {
        if p.starts_with('~') || p.contains('$') || p.contains('`') {
            return true;
        }
        let r = resolve(cwd, p);
        let under = |d: &Path| r.starts_with(d) && r != d;
        !(under(root) || tmp.iter().any(|t| under(t)))
    })
}

/// What a command matches on the guarded list: the rule's id and label.
pub fn matches(cmd: &str, cwd: &Path, root: &Path, rules: &[GuardRule], default: &str, current: Option<&str>) -> Option<(String, String)> {
    let on = |id: &str| rules.iter().any(|r| r.id == id && r.on);
    let label = |id: &str| rules.iter().find(|r| r.id == id).map(|r| r.label.clone()).unwrap_or_default();
    let mut here = cwd.to_path_buf();
    for words in shell_split(cmd) {
        let w = bare(&words);
        if w.is_empty() {
            continue;
        }
        if w[0] == "cd" {
            here = w.get(1).map(|d| resolve(&here, d)).unwrap_or_else(|| PathBuf::from("/"));
            continue;
        }
        // A shell's -c runs a command of its own.
        if matches!(base_name(&w[0]), "bash" | "sh" | "zsh" | "eval") {
            let inner = if w[0] == "eval" { Some(w[1..].join(" ")) } else { w.iter().position(|x| x == "-c" || x == "-lc").and_then(|i| w.get(i + 1).cloned()) };
            if let Some(inner) = inner {
                if let Some(hit) = matches(&inner, &here, root, rules, default, current) {
                    return Some(hit);
                }
            }
        }
        for r in rules.iter().filter(|r| r.on && r.kind == "prefix") {
            let pre: Vec<String> = shell_split(&r.label).into_iter().next().unwrap_or_default();
            if !pre.is_empty() && (w.len() >= pre.len() && w[..pre.len()] == pre[..] || words.len() >= pre.len() && words[..pre.len()] == pre[..]) {
                return Some((r.id.clone(), r.label.clone()));
            }
        }
        let hits = [
            ("migrations", is_migration(w)),
            ("deploy", is_deploy(w)),
            ("push-default", is_push_default(w, default, current)),
            ("rm-outside", is_rm_outside(w, &here, root)),
        ];
        if let Some((id, _)) = hits.iter().find(|(id, hit)| *hit && on(id)) {
            return Some((id.to_string(), label(id)));
        }
    }
    None
}

/// The PreToolUse hook's answer for a guarded Bash command: denied, with
/// the reason Claude Code tells the agent; `None` for anything else.
pub fn check(sup: &Supervisor, id: &str, hook: &Json) -> Option<Json> {
    if hook.get("hook_event_name").and_then(Json::as_str) != Some("PreToolUse") || hook.get("tool_name").and_then(Json::as_str) != Some("Bash") {
        return None;
    }
    let cmd = hook["tool_input"]["command"].as_str()?;
    let a = sup.agent_row(id)?;
    let cwd = PathBuf::from(&a.cwd);
    let main = PathBuf::from(&a.repo);
    let rules = rules_of(sup, &a.repo);
    let (rule, label) = matches(cmd, &cwd, &cwd, &rules, &default_branch(&main), crate::git::current_branch(&cwd).as_deref())?;
    let _ = sup.note(id, "guarded", format!("guarded: {label}: {}", one_line(cmd, 160)), Some(json!({ "command": cmd, "rule": rule }).to_string()), |_| {});
    let reason = format!(
        "Workbench guards this command in this repository ({label}): only the user runs it. Propose it with the workbench tool propose_change (the command, and why it is needed), then carry on with what does not need it; you will be told the result when the user approves or declines. Do not try to run it another way."
    );
    Some(json!({ "hookSpecificOutput": { "hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": reason } }))
}

// Change requests.

#[derive(Clone, Debug, Default)]
pub struct Request {
    pub id: String,
    pub agent: String,
    pub repo: String,
    pub cwd: String,
    pub branch: Option<String>,
    pub command: String,
    pub why: Option<String>,
    pub rule: Option<String>,
    pub risk: String,
    pub risk_why: Vec<String>,
    pub diff: Json,
    pub precedent: Json,
    pub state: String,
    pub output: Option<String>,
    pub exit_code: Option<i64>,
    pub ms: Option<i64>,
    pub decided_by: Option<String>,
    pub reason: Option<String>,
    pub told: Option<String>,
    pub created: i64,
    pub decided: Option<i64>,
}

const COLS: &str = "id, agent, repo, cwd, branch, command, why, rule, risk, risk_why, diff, precedent, state, output, exit_code, ms, decided_by, reason, told, created, decided";

fn json_of(v: &Value) -> Json {
    get_str(v).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Json::Null)
}

impl Request {
    fn of(r: &[Value]) -> Request {
        Request {
            id: get_str(&r[0]).unwrap_or_default(),
            agent: get_str(&r[1]).unwrap_or_default(),
            repo: get_str(&r[2]).unwrap_or_default(),
            cwd: get_str(&r[3]).unwrap_or_default(),
            branch: get_str(&r[4]),
            command: get_str(&r[5]).unwrap_or_default(),
            why: get_str(&r[6]),
            rule: get_str(&r[7]),
            risk: get_str(&r[8]).unwrap_or_else(|| "medium".into()),
            risk_why: json_of(&r[9]).as_array().into_iter().flatten().filter_map(|x| x.as_str().map(str::to_string)).collect(),
            diff: json_of(&r[10]),
            precedent: json_of(&r[11]),
            state: get_str(&r[12]).unwrap_or_default(),
            output: get_str(&r[13]),
            exit_code: get_int(&r[14]),
            ms: get_int(&r[15]),
            decided_by: get_str(&r[16]),
            reason: get_str(&r[17]),
            told: get_str(&r[18]),
            created: get_ms(&r[19]).unwrap_or(0),
            decided: get_ms(&r[20]),
        }
    }

    fn put(&self, sup: &Supervisor) -> Result<(), ApiError> {
        let p = Params::new()
            .set("id", Value::Str(self.id.clone()))
            .set("agent", Value::Str(self.agent.clone()))
            .set("repo", Value::Str(self.repo.clone()))
            .set("cwd", Value::Str(self.cwd.clone()))
            .set("branch", opt_str(&self.branch))
            .set("command", Value::Str(self.command.clone()))
            .set("why", opt_str(&self.why))
            .set("rule", opt_str(&self.rule))
            .set("risk", Value::Str(self.risk.clone()))
            .set("risk_why", Value::Str(json!(self.risk_why).to_string()))
            .set("diff", Value::Str(self.diff.to_string()))
            .set("precedent", Value::Str(self.precedent.to_string()))
            .set("state", Value::Str(self.state.clone()))
            .set("output", opt_str(&self.output))
            .set("exit_code", opt_int(self.exit_code))
            .set("ms", opt_int(self.ms))
            .set("decided_by", opt_str(&self.decided_by))
            .set("reason", opt_str(&self.reason))
            .set("told", opt_str(&self.told))
            .set("created", ts(self.created))
            .set("decided", self.decided.map_or(Value::Null, ts));
        let names: Vec<String> = COLS.split(", ").map(|c| format!(":{c}")).collect();
        sup.db().query(&format!("INSERT OR REPLACE INTO change_requests ({COLS}) VALUES ({})", names.join(", ")), &p).map_err(ApiError::internal)?;
        Ok(())
    }

    pub fn to_json(&self, sup: &Supervisor) -> Json {
        let name = sup.agent_row(&self.agent).map(|a| a.name);
        let label = self.rule.as_deref().and_then(|r| rules_of(sup, &self.repo).into_iter().find(|x| x.id == r)).map(|r| r.label);
        json!({
            "id": self.id, "agent": self.agent, "agent_name": name, "repo": self.repo, "cwd": self.cwd, "branch": self.branch,
            "command": self.command, "why": self.why, "rule": self.rule, "rule_label": label,
            "risk": self.risk, "risk_why": self.risk_why, "diff": self.diff,
            "precedent": if self.precedent.is_null() { Json::Null } else { self.precedent.clone() },
            "state": self.state, "output": self.output, "exit_code": self.exit_code, "ms": self.ms,
            "decided_by": self.decided_by, "reason": self.reason, "told": self.told, "timeout_s": RUN_TIMEOUT.as_secs(),
            "created": iso(self.created), "created_ms": self.created, "decided": self.decided.map(iso), "decided_ms": self.decided,
        })
    }
}

fn get_req(sup: &Supervisor, id: &str) -> Result<Request, ApiError> {
    let rows = sup.db().query(&format!("SELECT {COLS} FROM change_requests WHERE id = :id LIMIT 1"), &Params::new().set("id", Value::Str(id.to_string()))).map_err(ApiError::internal)?;
    rows.first().map(|r| Request::of(r)).ok_or_else(|| ApiError::not_found(format!("no change request {id}")))
}

fn all_reqs(sup: &Supervisor) -> Vec<Request> {
    let rows = sup.db().query(&format!("SELECT {COLS} FROM change_requests ORDER BY created DESC LIMIT 10000"), &Params::new()).unwrap_or_default();
    rows.iter().map(|r| Request::of(r)).collect()
}

/// One decision at a time.
fn deciding() -> &'static Mutex<()> {
    static L: OnceLock<Mutex<()>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(()))
}

/// The agent's work so far, for the request: its files from where it
/// started, with their lines (migrations and schemas first).
fn diff_of(cwd: &Path, base: Option<&str>) -> Json {
    let Some(base) = base else { return Json::Null };
    let Ok(files) = crate::git::diff(cwd, base) else { return Json::Null };
    let first = |p: &str| {
        let l = p.to_ascii_lowercase();
        !(l.contains("migrat") || l.ends_with(".sql") || l.ends_with("schema.prisma"))
    };
    let mut files = files;
    files.sort_by_key(|f| first(f["path"].as_str().unwrap_or("")));
    let (mut added, mut removed) = (0, 0);
    let mut list = Vec::new();
    for f in &files {
        let (mut a, mut d) = (0, 0);
        let mut lines = Vec::new();
        for l in f["hunks"].as_array().into_iter().flatten().flat_map(|h| h["lines"].as_array().into_iter().flatten()) {
            let k = l["kind"].as_str().unwrap_or(" ");
            match k {
                "+" => a += 1,
                "-" => d += 1,
                _ => {}
            }
            if lines.len() < 60 {
                lines.push(format!("{k}{}", l["text"].as_str().unwrap_or("")));
            }
        }
        added += a;
        removed += d;
        if list.len() < 12 {
            list.push(json!({ "path": f["path"], "status": f["status"], "added": a, "removed": d, "lines": lines }));
        }
    }
    json!({ "files": files.len(), "added": added, "removed": removed, "list": list })
}

/// The risk of a request, and why.
fn risk_of(rule: Option<&str>, cmd: &str, diff: &Json, said: Option<&str>) -> (String, Vec<String>) {
    let mut why = Vec::new();
    let mut risk = match rule {
        Some("push-default") => {
            why.push("it changes the default branch everyone builds from".to_string());
            "high"
        }
        Some("deploy") => {
            why.push("it deploys or publishes: others see it at once".to_string());
            "high"
        }
        Some("rm-outside") => {
            why.push("it deletes files outside the agent's worktree".to_string());
            "high"
        }
        Some("migrations") => {
            why.push("a migration: it changes the database's schema".to_string());
            "medium"
        }
        Some(_) => {
            why.push("its words are on this repository's guarded list".to_string());
            "medium"
        }
        None => "low",
    };
    let text: String = diff["list"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|f| f["lines"].as_array().cloned().unwrap_or_default())
        .filter_map(|l| l.as_str().filter(|l| l.starts_with('+')).map(str::to_ascii_uppercase))
        .collect::<Vec<_>>()
        .join("\n");
    let lower = cmd.to_ascii_lowercase();
    for (k, w) in [("DROP TABLE", "it drops a table"), ("DROP COLUMN", "it drops a column"), ("TRUNCATE", "it truncates a table"), ("DELETE FROM", "it deletes rows")] {
        if text.contains(k) {
            why.insert(0, w.to_string());
            risk = "high";
        }
    }
    if lower.contains("--accept-data-loss") || lower.contains("migrate reset") {
        why.insert(0, "it may lose data".to_string());
        risk = "high";
    }
    if let Some(s) = said.map(str::to_ascii_lowercase) {
        let rank = |r: &str| ["low", "medium", "high"].iter().position(|x| *x == r).unwrap_or(1);
        if ["low", "medium", "high"].contains(&s.as_str()) && rank(&s) > rank(risk) {
            why.push(format!("the agent calls it {s} risk"));
            risk = match s.as_str() {
                "high" => "high",
                _ => "medium",
            };
        }
    }
    (risk.to_string(), why)
}

#[derive(Debug, Default, Deserialize)]
pub struct ProposeBody {
    pub command: Option<String>,
    pub why: Option<String>,
    pub risk: Option<String>,
}

/// `POST /api/changes`: an agent proposes a command.
fn propose(sup: &Arc<Supervisor>, agent: &str, b: ProposeBody) -> Result<Json, ApiError> {
    let cmd = b.command.as_deref().map(str::trim).filter(|c| !c.is_empty()).ok_or_else(|| ApiError::bad("command is required"))?;
    if cmd.len() > 8000 {
        return Err(ApiError::bad("command is longer than 8000 bytes"));
    }
    let a = sup.agent_row(agent).ok_or_else(|| ApiError::not_found(format!("no agent {agent}")))?;
    let cwd = PathBuf::from(&a.cwd);
    let rules = rules_of(sup, &a.repo);
    let hit = matches(cmd, &cwd, &cwd, &rules, &default_branch(Path::new(&a.repo)), crate::git::current_branch(&cwd).as_deref());
    let diff = diff_of(&cwd, a.base.as_deref());
    let (risk, risk_why) = risk_of(hit.as_ref().map(|h| h.0.as_str()), cmd, &diff, b.risk.as_deref());
    let r = Request {
        id: new_id("cr"),
        agent: a.id.clone(),
        repo: a.repo.clone(),
        cwd: a.cwd.clone(),
        branch: a.branch.clone(),
        command: cmd.to_string(),
        why: b.why.map(|w| w.trim().to_string()).filter(|w| !w.is_empty()),
        rule: hit.map(|h| h.0),
        risk,
        risk_why,
        diff,
        precedent: Json::Null,
        state: "open".into(),
        created: now_ms(),
        ..Request::default()
    };
    r.put(sup)?;
    let _ = sup.note(&a.id, "change_request", format!("proposed: {}", one_line(cmd, 160)), Some(json!({ "id": r.id, "command": cmd }).to_string()), |_| {});
    precedent_later(sup, &r.id);
    Ok(json!({ "request": r.to_json(sup) }))
}

/// Asks ChloeCtx about such changes off the request path.
fn precedent_later(sup: &Arc<Supervisor>, id: &str) {
    let (sup, id) = (sup.clone(), id.to_string());
    let _ = std::thread::Builder::new().name(format!("precedent-{id}")).spawn(move || {
        let Ok(r) = get_req(&sup, &id) else { return };
        let q = format!("Has a change like this been approved, declined or gone wrong before: {}? {}", r.command, r.why.as_deref().unwrap_or(""));
        let Ok(p) = trail::ask(&sup, Some(&r.repo), &q) else { return };
        let text = p["text"].as_str().unwrap_or("").trim().to_string();
        if text.is_empty() {
            return;
        }
        let _g = deciding().lock().unwrap();
        if let Ok(mut r) = get_req(&sup, &id) {
            r.precedent = json!({ "text": text, "ids": p["ids"] });
            let _ = r.put(&sup);
        }
    });
}

/// Runs `cmd` in `cwd` with no terminal: its exit code (-1 when it could
/// not start or ran out of time), its output (the end of it) and how long.
fn run(cmd: &str, cwd: &Path) -> (i64, String, i64) {
    let t0 = Instant::now();
    let child = Command::new("bash")
        .arg("-c")
        .arg(cmd)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("CI", "1")
        .env_remove("CHLOEDB_WORKBENCH_TOKEN")
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return (-1, format!("cannot start bash in {}: {e}", cwd.display()), 0),
    };
    let out = Arc::new(Mutex::new(Vec::<u8>::new()));
    let readers: Vec<_> = [child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>), child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>)]
        .into_iter()
        .flatten()
        .map(|mut r| {
            let out = out.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 8192];
                while let Ok(n) = r.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    let mut o = out.lock().unwrap();
                    o.extend_from_slice(&buf[..n]);
                    if o.len() > OUTPUT_MAX * 2 {
                        let cut = o.len() - OUTPUT_MAX;
                        o.drain(..cut);
                    }
                }
            })
        })
        .collect();
    let code = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s.code().unwrap_or(-1) as i64,
            Ok(None) if t0.elapsed() > RUN_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                out.lock().unwrap().extend_from_slice(format!("\n[Workbench stopped it after {} minutes]\n", RUN_TIMEOUT.as_secs() / 60).as_bytes());
                break -1;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => break -1,
        }
    };
    for r in readers {
        let _ = r.join();
    }
    let o = out.lock().unwrap();
    let tail = &o[o.len().saturating_sub(OUTPUT_MAX)..];
    (code, String::from_utf8_lossy(tail).to_string(), t0.elapsed().as_millis() as i64)
}

/// Tells the agent, as a steer does: typed when it is idle, else at its
/// next hook. `None` when it no longer runs.
fn tell(sup: &Arc<Supervisor>, lv: &Levels, agent: &str, text: &str) -> Option<String> {
    let v = crate::levels::steer(sup, lv, agent, text).ok()?;
    v["delivered"].as_str().map(str::to_string)
}

fn last_lines(s: &str, max: usize) -> String {
    let s = s.trim_end();
    if s.len() <= max {
        return s.to_string();
    }
    let mut i = s.len() - max;
    while !s.is_char_boundary(i) {
        i += 1;
    }
    format!("…{}", &s[i..])
}

/// `POST /api/changes/:id/approve`: runs it, records and tells.
fn approve(sup: &Arc<Supervisor>, lv: &Arc<Levels>, id: &str) -> Result<Json, ApiError> {
    let mut r = {
        let _g = deciding().lock().unwrap();
        let mut r = get_req(sup, id)?;
        if r.state != "open" {
            return Err(ApiError::conflict(format!("change request {id} is {}, not open", r.state)));
        }
        r.state = "running".into();
        r.decided = Some(now_ms());
        r.decided_by = Some("user".into());
        r.put(sup)?;
        r
    };
    let _ = sup.note(&r.agent, "change_approved", format!("approved: {}", one_line(&r.command, 160)), Some(json!({ "id": r.id }).to_string()), |_| {});
    let (tx, rx) = std::sync::mpsc::channel();
    let (sup2, lv2) = (sup.clone(), lv.clone());
    let r2 = r.clone();
    std::thread::Builder::new()
        .name(format!("change-{id}"))
        .spawn(move || {
            let mut r = r2;
            let cwd = PathBuf::from(&r.cwd);
            let (code, output, ms) = if cwd.is_dir() { run(&r.command, &cwd) } else { (-1, format!("{} is gone: nothing was run", r.cwd), 0) };
            r.exit_code = Some(code);
            r.output = Some(output);
            r.ms = Some(ms);
            r.state = if code == 0 { "done".into() } else { "failed".into() };
            let out = last_lines(r.output.as_deref().unwrap_or(""), 2500);
            let text = format!(
                "The user approved your change request {}, and Workbench ran `{}` in {}: it {} after {:.1} s.{}",
                r.id,
                one_line(&r.command, 300),
                r.cwd,
                if code == 0 { "succeeded (exit 0)".to_string() } else { format!("failed (exit {code})") },
                ms as f64 / 1000.0,
                if out.is_empty() { " It printed nothing.".to_string() } else { format!(" Its output:\n```\n{out}\n```") }
            );
            r.told = tell(&sup2, &lv2, &r.agent, &text);
            let _g = deciding().lock().unwrap();
            // The precedent may have come in meanwhile.
            if let Ok(now) = get_req(&sup2, &r.id) {
                r.precedent = now.precedent;
            }
            let _ = r.put(&sup2);
            let _ = tx.send(());
        })
        .map_err(|e| ApiError::internal(e.to_string()))?;
    if rx.recv_timeout(APPROVE_WAIT).is_ok() {
        r = get_req(sup, id)?;
    }
    Ok(json!({ "request": r.to_json(sup) }))
}

#[derive(Debug, Default, Deserialize)]
pub struct DeclineBody {
    pub reason: Option<String>,
}

/// `POST /api/changes/:id/decline`.
fn decline(sup: &Arc<Supervisor>, lv: &Levels, id: &str, b: DeclineBody) -> Result<Json, ApiError> {
    let _g = deciding().lock().unwrap();
    let mut r = get_req(sup, id)?;
    if r.state != "open" {
        return Err(ApiError::conflict(format!("change request {id} is {}, not open", r.state)));
    }
    r.state = "declined".into();
    r.decided = Some(now_ms());
    r.decided_by = Some("user".into());
    r.reason = b.reason.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let text = format!(
        "The user declined your change request {} (`{}`){}. Do not run it another way: carry on without it, or propose a different change with propose_change.",
        r.id,
        one_line(&r.command, 300),
        match &r.reason {
            Some(why) => format!(": {why}"),
            None => String::new(),
        }
    );
    let _ = sup.note(&r.agent, "change_declined", format!("declined: {}", one_line(&r.command, 160)), Some(json!({ "id": r.id, "reason": r.reason }).to_string()), |_| {});
    r.told = tell(sup, lv, &r.agent, &text);
    r.put(sup)?;
    Ok(json!({ "request": r.to_json(sup) }))
}

// Routes.

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/guarded", get(guarded_get).put(guarded_put))
        .route("/api/changes", get(changes_list).post(changes_create))
        .route("/api/changes/:id", get(change_get))
        .route("/api/changes/:id/approve", post(change_approve))
        .route("/api/changes/:id/decline", post(change_decline))
}

#[derive(Deserialize)]
struct RepoQ {
    repo: Option<String>,
    state: Option<String>,
}

async fn guarded_get(State(app): State<App>, Query(q): Query<RepoQ>) -> ApiResult {
    let sup = app.sup.clone();
    let v = blocking(move || {
        let main = trail::repo_dir(q.repo.as_deref())?;
        Ok(rules_json(&sup, &main.to_string_lossy()))
    })
    .await?;
    Ok(AxJson(v).into_response())
}

#[derive(Deserialize)]
struct GuardedB {
    repo: Option<String>,
    rules: Vec<Json>,
}

async fn guarded_put(State(app): State<App>, headers: HeaderMap, body: Bytes) -> ApiResult {
    user_only(&app, &headers)?;
    let b: GuardedB = parse_body(&body)?;
    let sup = app.sup.clone();
    let v = blocking(move || {
        let main = trail::repo_dir(b.repo.as_deref())?;
        let repo = main.to_string_lossy().to_string();
        let mut saved = Vec::new();
        for r in &b.rules {
            let id = r["id"].as_str().unwrap_or("");
            let on = r["on"].as_bool().unwrap_or(true);
            if BUILTINS.iter().any(|x| x.0 == id) {
                saved.push(json!({ "id": id, "kind": "builtin", "on": on }));
            } else {
                let label = r["label"].as_str().unwrap_or("").trim().to_string();
                if label.is_empty() {
                    continue;
                }
                if label.len() > 200 {
                    return Err(ApiError::bad("a guarded prefix is at most 200 bytes"));
                }
                let id = if id.is_empty() || id.len() > 40 { format!("p{}", hex(4)) } else { id.to_string() };
                saved.push(json!({ "id": id, "kind": "prefix", "label": label, "on": on }));
            }
        }
        sup.set_mark(&mark_key(&repo), &Json::Array(saved).to_string())?;
        Ok(rules_json(&sup, &repo))
    })
    .await?;
    Ok(AxJson(v).into_response())
}

async fn changes_list(State(app): State<App>, Query(q): Query<RepoQ>) -> ApiResult {
    let sup = app.sup.clone();
    let v = blocking(move || {
        let repo = match q.repo.as_deref().filter(|r| !r.is_empty()) {
            Some(r) => Some(trail::repo_dir(Some(r))?.to_string_lossy().to_string()),
            None => None,
        };
        let list: Vec<Json> = all_reqs(&sup)
            .iter()
            .filter(|r| repo.as_ref().is_none_or(|x| &r.repo == x))
            .filter(|r| q.state.as_deref().filter(|s| !s.is_empty()).is_none_or(|s| r.state == s))
            .map(|r| r.to_json(&sup))
            .collect();
        Ok(json!({ "requests": list }))
    })
    .await?;
    Ok(AxJson(v).into_response())
}

async fn changes_create(State(app): State<App>, headers: HeaderMap, body: Bytes) -> ApiResult {
    let b: ProposeBody = parse_body(&body)?;
    // The agent: its key's, else the one the header names.
    let agent = token_agent(&headers)
        .or_else(|| headers.get("x-workbench-agent").and_then(|h| h.to_str().ok()).map(str::to_string))
        .ok_or_else(|| ApiError::bad("a change request comes from an agent: the header x-workbench-agent is required"))?;
    let sup = app.sup.clone();
    let v = blocking(move || propose(&sup, &agent, b)).await?;
    Ok((StatusCode::CREATED, AxJson(v)).into_response())
}

async fn change_get(State(app): State<App>, AxPath(id): AxPath<String>) -> ApiResult {
    let sup = app.sup.clone();
    let v = blocking(move || Ok(json!({ "request": get_req(&sup, &id)?.to_json(&sup) }))).await?;
    Ok(AxJson(v).into_response())
}

async fn change_approve(State(app): State<App>, headers: HeaderMap, AxPath(id): AxPath<String>) -> ApiResult {
    user_only(&app, &headers)?;
    let (sup, lv) = (app.sup.clone(), app.lv.clone());
    let v = blocking(move || approve(&sup, &lv, &id)).await?;
    Ok(AxJson(v).into_response())
}

async fn change_decline(State(app): State<App>, headers: HeaderMap, AxPath(id): AxPath<String>, body: Bytes) -> ApiResult {
    user_only(&app, &headers)?;
    let b: DeclineBody = if body.is_empty() { DeclineBody::default() } else { parse_body(&body)? };
    let (sup, lv) = (app.sup.clone(), app.lv.clone());
    let v = blocking(move || decline(&sup, &lv, &id, b)).await?;
    Ok(AxJson(v).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> Vec<GuardRule> {
        let mut r: Vec<GuardRule> = BUILTINS.iter().map(|b| GuardRule { id: b.0.into(), kind: "builtin".into(), label: b.1.into(), on: true }).collect();
        r.push(GuardRule { id: "p1".into(), kind: "prefix".into(), label: "npm run seed:prod".into(), on: true });
        r
    }

    fn hit(cmd: &str) -> Option<String> {
        let wt = Path::new("/w/shop-cart");
        matches(cmd, wt, wt, &rules(), "main", Some("wb/cart")).map(|h| h.0)
    }

    #[test]
    fn guarded_commands_match() {
        for (cmd, want) in [
            ("npx prisma migrate deploy", Some("migrations")),
            ("cd app && DATABASE_URL=x pnpm exec prisma migrate deploy", Some("migrations")),
            ("python manage.py migrate", Some("migrations")),
            ("bin/rails db:migrate", Some("migrations")),
            ("npm run db:migrate", Some("migrations")),
            ("sqlx migrate run --source db", Some("migrations")),
            ("./scripts/deploy-prod.sh", Some("deploy")),
            ("bash scripts/deploy.sh staging", Some("deploy")),
            ("kubectl apply -f k8s/", Some("deploy")),
            ("vercel --prod", Some("deploy")),
            ("cargo publish -p calc", Some("deploy")),
            ("git push origin main", Some("push-default")),
            ("git push origin HEAD:main", Some("push-default")),
            ("git push -f origin wb/cart", Some("push-default")),
            ("git push origin +wb/cart", Some("push-default")),
            ("git -C /w/shop-cart push origin refs/heads/main", Some("push-default")),
            ("rm -rf ../shop", Some("rm-outside")),
            ("rm -rf ~/.cache/x", Some("rm-outside")),
            ("rm -r /home/u/src", Some("rm-outside")),
            ("cd .. && rm -rf shop-cart/node_modules", None),
            ("cd / && rm -rf w", Some("rm-outside")),
            ("bash -c 'git push origin main'", Some("push-default")),
            ("npm run seed:prod -- --force", Some("p1")),
            ("echo ok; prisma migrate deploy", Some("migrations")),
            ("git push", None),
            ("git push origin wb/cart", None),
            ("git push -u origin wb/cart", None),
            ("rm -rf node_modules dist", None),
            ("rm -rf /tmp/build-1", None),
            ("rm -f ../x", None),
            ("npx prisma generate", None),
            ("echo 'git push origin main'", None),
            ("grep -r deploy scripts/", None),
            ("cargo test", None),
            ("npm run seed:dev", None),
        ] {
            assert_eq!(hit(cmd).as_deref(), want, "{cmd}");
        }
        // On the default branch, a plain push pushes it.
        let wt = Path::new("/w/shop");
        assert_eq!(matches("git push", wt, wt, &rules(), "main", Some("main")).map(|h| h.0).as_deref(), Some("push-default"));
        // A rule turned off.
        let mut off = rules();
        off[0].on = false;
        assert_eq!(matches("prisma migrate deploy", wt, wt, &off, "main", None), None);
    }

    #[test]
    fn risk_reads_the_diff() {
        let diff = json!({ "list": [{ "lines": ["+ALTER TABLE carts DROP COLUMN legacy;"] }] });
        let (r, why) = risk_of(Some("migrations"), "prisma migrate deploy", &diff, None);
        assert_eq!(r, "high");
        assert_eq!(why[0], "it drops a column");
        let (r, _) = risk_of(Some("migrations"), "prisma migrate deploy", &json!(null), None);
        assert_eq!(r, "medium");
        let (r, why) = risk_of(None, "make thing", &json!(null), Some("high"));
        assert_eq!((r.as_str(), why.len()), ("high", 1));
        assert_eq!(last_lines("abcdef", 3), "…def");
    }

    #[test]
    fn words_as_a_shell_reads_them() {
        assert_eq!(shell_split("a 'b c' \"d\\\"e\" f\\ g && h | i;j"), vec![vec!["a", "b c", "d\"e", "f g"], vec!["h"], vec!["i"], vec!["j"]]);
    }
}
