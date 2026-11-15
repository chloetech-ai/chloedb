//! Make this a test (HARNESS.md, "Evals as rows"): a case is a stretch of a
//! session Workbench recorded, made reproducible.
//!
//! - **Where it starts.** The code at the start of its first turn: Workbench
//!   writes a commit of each worktree as it is when a prompt arrives (its
//!   uncommitted changes included, through a private index; nothing of the
//!   agent's changes), as a row of `turn_trees`. Turn 1 falls back on the
//!   commit the worktree started from. A case keeps its commit alive with a
//!   ref of its own, `refs/workbench/cases/<id>`.
//! - **What came before.** A case that starts after turn 1 keeps a copy of
//!   the session's transcript up to that turn, in Workbench's data folder,
//!   and a replay resumes it (`--resume --fork-session`, as a restart does).
//! - **What it plays.** The prompts of its turns, in order, and what the user
//!   answered to each permission prompt and question in them: the scripted
//!   user of a replay (src/harness/replay.rs).
//! - **How it is judged.** Its checks, proposed from what happened: tests the
//!   session made pass (fail→pass), tests that passed and still did
//!   (pass→pass), never `git push`, and a limit on prompts; the user ticks
//!   them, and can add a command of their own.
//! - **Its set.** One case in three is held out, chosen by session, so a
//!   change is judged on sessions it was not made from.
//!
//! No model is called here.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use axum::body::Bytes;
use axum::extract::{Path as AxPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json as AxJson, Router};
use chloedb::query::Params;
use chloedb::Value;
use serde::Deserialize;
use serde_json::{json, Value as Json};

use super::{hash, sv, user_only};
use crate::db::{get_int, get_ms, get_str, iso, now_ms, ts, Agent, Event};
use crate::server::{blocking, parse_body, ApiResult, App};
use crate::supervisor::{one_line, ApiError, Supervisor};

pub const TABLES: [&str; 3] = [
    "CREATE TABLE IF NOT EXISTS turn_trees (id KEYWORD PRIMARY KEY, agent KEYWORD, turn INT, \"commit\" KEYWORD, head KEYWORD, ts TIMESTAMP)",
    "CREATE TABLE IF NOT EXISTS eval_suites (id KEYWORD PRIMARY KEY, repo KEYWORD, name KEYWORD, note TEXT, created TIMESTAMP)",
    "CREATE TABLE IF NOT EXISTS eval_cases (id KEYWORD PRIMARY KEY, suite KEYWORD, repo KEYWORD, name KEYWORD, agent KEYWORD, \"commit\" KEYWORD, tree KEYWORD, wall_ms INT, start_from KEYWORD, turns_from INT, turns_to INT, turns_all INT, script TEXT OPTIONS(indexed = false), checks TEXT OPTIONS(indexed = false), \"set\" KEYWORD, context KEYWORD, setup TEXT, model KEYWORD, permission_mode KEYWORD, cost_usd FLOAT, made_by KEYWORD, created TIMESTAMP)",
];

/// The most checks a case keeps of each test kind.
const MAX_TEST_CHECKS: usize = 40;
/// The words a prompt keeps in a case's name.
const NAME_WORDS: usize = 5;

/// Writes a row: `cols` are its columns and values (quoted, so that
/// `commit`, `set` and `case` can be names).
pub(super) fn put_row(sup: &Supervisor, table: &str, cols: &[(&str, Value)]) -> Result<(), ApiError> {
    let names: Vec<String> = cols.iter().map(|(c, _)| format!("\"{c}\"")).collect();
    let marks: Vec<String> = cols.iter().map(|(c, _)| format!(":{c}")).collect();
    let mut p = Params::new();
    for (c, v) in cols {
        p = p.set(c, v.clone());
    }
    sup.db().query(&format!("INSERT OR REPLACE INTO {table} ({}) VALUES ({})", names.join(", "), marks.join(", ")), &p).map_err(ApiError::internal)?;
    Ok(())
}

pub(super) fn ov(v: &Option<String>) -> Value {
    v.as_ref().map_or(Value::Null, |s| sv(s))
}

// ------------------------------------------------------------ turn trees

/// After each hook: on a prompt, the code of the agent's worktree as the
/// turn starts, written off the hook's thread (a commit no branch names).
pub fn after_hook(sup: &std::sync::Arc<Supervisor>, id: &str, hook: &Json) {
    if hook.get("hook_event_name").and_then(Json::as_str) != Some("UserPromptSubmit") || sup.is_replay(id) {
        return;
    }
    let Some(a) = sup.agent_row(id) else { return };
    let turn = sup.all_events(id).map(|e| e.iter().filter(|e| e.kind == "UserPromptSubmit").count()).unwrap_or(0) as i64;
    if turn == 0 {
        return;
    }
    let sup = sup.clone();
    let _ = std::thread::Builder::new().name(format!("turn-tree-{id}")).spawn(move || {
        let dir = Path::new(&a.cwd);
        let Some(head) = crate::git::rev_parse(dir, "HEAD") else { return };
        let commit = if crate::review::uncommitted(dir).is_empty() { Some(head.clone()) } else { crate::review::worktree_commit(dir) };
        let Some(commit) = commit else { return };
        let _ = put_row(&sup, "turn_trees", &[("id", sv(&format!("{}:{turn}", a.id))), ("agent", sv(&a.id)), ("turn", Value::Int(turn)), ("commit", sv(&commit)), ("head", sv(&head)), ("ts", ts(now_ms()))]);
    });
}

/// The code at a turn's start: (its commit, the HEAD it was made on); the
/// two are one commit when the worktree had nothing uncommitted.
fn turn_tree(sup: &Supervisor, agent: &str, turn: i64) -> Option<(String, String)> {
    let rows = sup.db().query("SELECT \"commit\", head FROM turn_trees WHERE id = :id LIMIT 1", &Params::new().set("id", sv(&format!("{agent}:{turn}")))).ok()?;
    let r = rows.first()?;
    let c = get_str(&r[0])?;
    Some((c.clone(), get_str(&r[1]).unwrap_or(c)))
}

// --------------------------------------------------------------- the log

/// One turn of a session: a prompt and what followed until the next.
#[derive(Clone, Debug)]
pub(super) struct Turn {
    pub n: i64,
    pub ts: i64,
    pub end: i64,
    pub prompt: String,
}

pub(super) fn prompt_of(e: &Event) -> Option<String> {
    let d: Json = serde_json::from_str(e.data.as_deref()?).ok()?;
    d["prompt"].as_str().map(str::to_string)
}

pub(super) fn turns_of(a: &Agent, evs: &[Event]) -> Vec<Turn> {
    let starts: Vec<&Event> = evs.iter().filter(|e| e.kind == "UserPromptSubmit").collect();
    let last = evs.last().map(|e| e.ts).unwrap_or(a.created).max(a.ended.unwrap_or(0));
    starts
        .iter()
        .enumerate()
        .map(|(i, e)| Turn { n: i as i64 + 1, ts: e.ts, end: starts.get(i + 1).map(|x| x.ts).unwrap_or(last + 1), prompt: prompt_of(e).unwrap_or_default() })
        .collect()
}

/// What a permission prompt asked about, as a replay matches it: a Bash
/// command's simple form (`PYTHONPATH=. python3 -m pytest`), an edit's
/// tool and file, or the tool.
pub(super) fn perm_key(detail: &Json) -> String {
    let tool = detail["tool"].as_str().unwrap_or("");
    match tool {
        "Bash" => detail["command"].as_str().and_then(super::storm_key).map(|k| format!("Bash: {k}")).unwrap_or_else(|| "Bash".into()),
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => format!("{tool}: {}", detail["rel"].as_str().or(detail["file"].as_str()).unwrap_or("")),
        t => t.to_string(),
    }
}

/// What an answer to a permission prompt was: `yes`, `yes_more` (it also
/// stopped the asking for such calls, for the session) or `no`. Switching
/// to auto mode is a plain yes here: a mode is a part of the harness.
pub(super) fn answer_kind(label: &str) -> &'static str {
    let l = label.trim().to_lowercase();
    if l.starts_with("yes") {
        match crate::screen::grants_more(label) {
            Some(g) if !g.contains("auto mode") => "yes_more",
            _ => "yes",
        }
    } else {
        "no"
    }
}

/// The user's side of a stretch of a session, in order: its prompts, and
/// the answers to its permission prompts and questions.
fn script_of(sup: &Supervisor, a: &Agent, turns: &[Turn]) -> Json {
    let mut out: Vec<Json> = turns.iter().map(|t| json!({ "kind": "prompt", "turn": t.n, "text": t.prompt })).collect();
    let (from, to) = (turns.first().map(|t| t.ts).unwrap_or(0), turns.last().map(|t| t.end).unwrap_or(i64::MAX));
    let turn_at = |at: i64| turns.iter().find(|t| at >= t.ts && at < t.end).map(|t| t.n);
    let mut qs: Vec<crate::questions::Question> = crate::questions::all(sup).into_iter().filter(|q| q.agent.as_deref() == Some(a.id.as_str()) && q.created >= from && q.created < to && q.state == "answered").collect();
    qs.sort_by_key(|q| q.created);
    for q in qs {
        let answer = q.answer.clone().unwrap_or_default();
        if q.kind == "permission" {
            let d = q.detail.clone().unwrap_or(Json::Null);
            if d["menu"]["kind"] == "trust" {
                continue;
            }
            out.push(json!({ "kind": "permission", "turn": turn_at(q.created), "key": perm_key(&d), "what": d["summary"].as_str().unwrap_or(&q.text), "label": answer, "answer": answer_kind(&answer), "by": q.answered_by }));
        } else {
            out.push(json!({ "kind": "answer", "turn": turn_at(q.created), "question": q.text, "options": q.labels(), "answer": answer, "by": q.answered_by }));
        }
    }
    Json::Array(out)
}

/// A test's runs in a stretch: (outcome first, outcome last, runner, command).
fn tests_in(sup: &Supervisor, agent: &str, from: i64, to: i64) -> BTreeMap<String, (String, String, String, String)> {
    let p = Params::new().set("agent", sv(agent)).set("from", ts(from)).set("to", ts(to));
    let rows = sup.db().query("SELECT test, outcome, runner, command FROM test_runs WHERE agent = :agent AND ts >= :from AND ts < :to AND runner != 'other' ORDER BY ts ASC LIMIT 100000", &p).unwrap_or_default();
    let mut out: BTreeMap<String, (String, String, String, String)> = BTreeMap::new();
    for r in rows {
        let (Some(t), Some(o)) = (get_str(&r[0]), get_str(&r[1])) else { continue };
        if o == "skipped" {
            continue;
        }
        let runner = get_str(&r[2]).unwrap_or_default();
        let cmd = get_str(&r[3]).unwrap_or_default();
        out.entry(t).and_modify(|e| {
            e.1 = o.clone();
            e.2 = runner.clone();
            e.3 = cmd.clone();
        }).or_insert((o.clone(), o, runner, cmd));
    }
    out
}

/// Test commands of a runner Workbench cannot aim at one test, that failed
/// and then passed in a stretch: each a `command` check.
fn commands_fixed(sup: &Supervisor, agent: &str, from: i64, to: i64) -> Vec<String> {
    let p = Params::new().set("agent", sv(agent)).set("from", ts(from)).set("to", ts(to));
    let rows = sup.db().query("SELECT command, exit_code, runner FROM test_summaries WHERE agent = :agent AND ts >= :from AND ts < :to ORDER BY ts ASC LIMIT 10000", &p).unwrap_or_default();
    let mut state: Vec<(String, bool, bool)> = Vec::new();
    for r in rows {
        if get_str(&r[2]).as_deref() != Some("other") {
            continue;
        }
        let Some(c) = get_str(&r[0]) else { continue };
        let ok = get_int(&r[1]) == Some(0);
        match state.iter_mut().find(|(x, _, _)| *x == c) {
            Some(e) => {
                e.1 |= !ok;
                e.2 = ok;
            }
            None => state.push((c, !ok, ok)),
        }
    }
    state.into_iter().filter(|(_, failed, last_ok)| *failed && *last_ok).map(|(c, _, _)| c).collect()
}

/// The tokens and dollars of a stretch, from its transcript's usage.
fn cost_in(sup: &Supervisor, a: &Agent, from: i64, to: i64) -> (i64, f64) {
    let p = Params::new().set("agent", sv(&a.id)).set("from", ts(from)).set("to", ts(to));
    let rows = sup.db().query("SELECT input, output, cache_read FROM token_use WHERE agent = :agent AND ts >= :from AND ts < :to LIMIT 100000", &p).unwrap_or_default();
    let (mut i, mut o, mut c) = (0, 0, 0);
    for r in rows {
        i += get_int(&r[0]).unwrap_or(0);
        o += get_int(&r[1]).unwrap_or(0);
        c += get_int(&r[2]).unwrap_or(0);
    }
    (i + o, crate::routing::cost_usd(&crate::routing::model_of(a.model.as_deref()), i, o, c))
}

/// Held out or train, by the session: one in three held out.
fn set_of(agent: &str) -> &'static str {
    if u64::from_str_radix(&hash(agent)[..8], 16).unwrap_or(0) % 3 == 0 {
        "held_out"
    } else {
        "train"
    }
}

fn name_of(prompt: &str) -> String {
    let words: Vec<String> = prompt.split_whitespace().take(NAME_WORDS).map(|w| w.chars().filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_').collect::<String>().to_lowercase()).filter(|w| !w.is_empty()).collect();
    let n = words.join("-");
    if n.is_empty() {
        "case".into()
    } else {
        n.chars().take(48).collect()
    }
}

// -------------------------------------------------------------- timeline

/// `GET /api/harness/timeline?agent=`: a session's turns, to pick a
/// stretch of: each with its prompt, its time, what it did, and whether
/// Workbench has the code at its start.
pub fn timeline(sup: &Supervisor, agent: &str) -> Result<Json, ApiError> {
    let a = sup.agent_row(agent).ok_or_else(|| ApiError::not_found(format!("no agent {agent}")))?;
    let evs = sup.all_events(agent)?;
    let turns = turns_of(&a, &evs);
    let qs: Vec<crate::questions::Question> = crate::questions::all(sup).into_iter().filter(|q| q.agent.as_deref() == Some(agent)).collect();
    let list: Vec<Json> = turns
        .iter()
        .map(|t| {
            let inside = |at: i64| at >= t.ts && at < t.end;
            let tools = evs.iter().filter(|e| e.kind == "PreToolUse" && inside(e.ts)).count();
            let edits = evs.iter().filter(|e| e.kind == "PreToolUse" && inside(e.ts) && matches!(e.tool.as_deref(), Some("Edit" | "Write" | "MultiEdit" | "NotebookEdit"))).count();
            let tests = tests_in(sup, agent, t.ts, t.end);
            let failed = tests.values().filter(|v| v.1 == "failed").count();
            let prompts = qs.iter().filter(|q| q.kind == "permission" && inside(q.created) && q.detail.as_ref().is_none_or(|d| d["menu"]["kind"] != "trust")).count();
            let asks = qs.iter().filter(|q| q.kind != "permission" && inside(q.created)).count();
            let code = if t.n == 1 { turn_tree(sup, agent, 1).is_some() || a.base.is_some() } else { turn_tree(sup, agent, t.n).is_some() };
            let (tokens, cost) = cost_in(sup, &a, t.ts, t.end);
            json!({
                "n": t.n, "ts": iso(t.ts), "ms": (t.end - t.ts).max(0), "prompt": one_line(&t.prompt, 400), "tools": tools, "edits": edits,
                "tests": { "ran": tests.len(), "failed": failed }, "prompts": prompts, "asks": asks, "code": code, "tokens": tokens, "cost_usd": (cost * 10_000.0).round() / 10_000.0,
            })
        })
        .collect();
    Ok(json!({ "agent": { "id": a.id, "name": a.name, "repo": a.repo, "model": a.model, "running": sup.is_running(agent), "created": iso(a.created), "ended": a.ended.map(iso), "base": a.base, "harness": a.harness }, "turns": list }))
}

// ------------------------------------------------------------- proposal

#[derive(Deserialize, Default)]
pub struct ProposeQ {
    pub agent: String,
    pub from: Option<i64>,
    pub to: Option<i64>,
}

/// The case a stretch of a session would make, with its proposed checks
/// (`on` the ones ticked by default) and what it cannot reproduce.
pub fn propose(sup: &Supervisor, q: &ProposeQ) -> Result<Json, ApiError> {
    let a = sup.agent_row(&q.agent).ok_or_else(|| ApiError::not_found(format!("no agent {}", q.agent)))?;
    if sup.is_replay(&a.id) {
        return Err(ApiError::bad("that agent is a replay: a case is made from a session you ran"));
    }
    let evs = sup.all_events(&a.id)?;
    let all = turns_of(&a, &evs);
    if all.is_empty() {
        return Err(ApiError::conflict(format!("{} has no prompt yet: there is nothing to play", a.name)));
    }
    let last = all.len() as i64;
    let from = q.from.unwrap_or(1).clamp(1, last);
    let to = q.to.unwrap_or(last).clamp(from, last);
    let turns: Vec<Turn> = all.iter().filter(|t| t.n >= from && t.n <= to).cloned().collect();
    let (t0, t1) = (turns.first().unwrap().ts, turns.last().unwrap().end);
    let mut warnings: Vec<String> = Vec::new();

    // Where it starts.
    // The commit to check out, and the uncommitted changes on it (a commit
    // whose tree a replay puts in the worktree, uncommitted again).
    let (commit, tree, start_from) = match (from, turn_tree(sup, &a.id, from)) {
        (_, Some((c, h))) => {
            let tree = (c != h).then_some(c);
            let what = if tree.is_some() { format!("the code as turn {from} started, its uncommitted changes too") } else { format!("the code as turn {from} started") };
            (Some(h), tree, what)
        }
        (1, None) => (a.base.clone(), None, "the commit the session started from".to_string()),
        (n, None) => (None, None, format!("no record of the code at turn {n}")),
    };
    if commit.is_none() {
        warnings.push(if from == 1 {
            "Workbench has no commit for where this session started, so it cannot be replayed".into()
        } else {
            format!("Workbench has no record of the code at turn {from} (it is written as each prompt arrives, from this build on): start at turn 1, or pick a later session")
        });
    }
    if from == 1 && turn_tree(sup, &a.id, 1).is_none() && a.base.is_some() {
        warnings.push("If the worktree had uncommitted changes when the session started, they are not in the case: Workbench recorded only its commit".into());
    }
    let transcript = sup.transcript_of(&a.id);
    let context = if from == 1 {
        "fresh"
    } else if transcript.is_some() {
        "transcript"
    } else {
        warnings.push(format!("The conversation before turn {from} is not on disk, so a replay would start from the code only"));
        "code_only"
    };
    if sup.is_running(&a.id) {
        warnings.push(format!("{} is still running: the case holds its turns so far", a.name));
    }

    // How it is judged.
    let mut checks: Vec<Json> = Vec::new();
    let tests = tests_in(sup, &a.id, t0, t1);
    let mut f2p = 0;
    let mut p2p = 0;
    let mut unaimed = 0;
    for (test, (first, last, runner, cmd)) in &tests {
        let kind = match (first.as_str(), last.as_str()) {
            ("failed", "passed") => "fail_to_pass",
            ("passed", "passed") => "pass_to_pass",
            _ => continue,
        };
        let Some(words) = crate::testruns::rerun_command(runner, cmd, test) else {
            unaimed += 1;
            continue;
        };
        let n = if kind == "fail_to_pass" { &mut f2p } else { &mut p2p };
        *n += 1;
        if *n > MAX_TEST_CHECKS {
            continue;
        }
        checks.push(json!({ "kind": kind, "test": test, "run": words, "on": true }));
    }
    if unaimed > 0 {
        warnings.push(format!("{unaimed} test{} ran through a command Workbench cannot aim at one test: add a command check if it matters", if unaimed == 1 { "" } else { "s" }));
    }
    for c in commands_fixed(sup, &a.id, t0, t1) {
        checks.push(json!({ "kind": "command", "run": c, "on": true, "why": "it failed, then passed, in this stretch" }));
    }
    let script = script_of(sup, &a, &turns);
    let prompts = script.as_array().map(|s| s.iter().filter(|x| x["kind"] == "permission").count()).unwrap_or(0);
    checks.push(json!({ "kind": "never", "pattern": "git push", "on": true }));
    checks.push(json!({ "kind": "limit", "metric": "prompts", "max": prompts, "on": false, "why": format!("the original needed {prompts}") }));
    if f2p + p2p == 0 && !checks.iter().any(|c| c["kind"] == "command") {
        warnings.push("No test passed in this stretch, so no check says whether a replay worked: add a command that does (its exit code decides)".into());
    }
    for (i, c) in checks.iter_mut().enumerate() {
        c["id"] = json!(format!("k{}", i + 1));
    }

    let (tokens, cost) = cost_in(sup, &a, t0, t1);
    let wall_ms = evs.iter().filter(|e| e.kind == "Stop" && e.ts >= t0 && e.ts < t1).map(|e| e.ts).max().unwrap_or(t1) - t0;
    let first = turns.first().map(|t| t.prompt.clone()).unwrap_or_default();
    let pq = script.as_array().map(|s| s.iter().filter(|x| x["kind"] == "prompt").count()).unwrap_or(0);
    Ok(json!({
        "agent": { "id": a.id, "name": a.name, "repo": a.repo, "model": a.model, "harness": a.harness },
        "name": name_of(&first), "repo": a.repo, "from": from, "to": to, "turns_all": last,
        "commit": commit, "tree": tree, "start_from": start_from, "context": context, "wall_ms": wall_ms.max(0),
        "script": script, "prompts": pq, "permissions": prompts,
        "checks": checks, "set": set_of(&a.id),
        "tokens": tokens, "cost_usd": (cost * 10_000.0).round() / 10_000.0, "model": a.model,
        "permission_mode": sup.extra_of(&a.id)["permission_mode"],
        "warnings": warnings, "replayable": commit.is_some(),
    }))
}

// ---------------------------------------------------------------- cases

/// A case's row.
#[derive(Clone, Debug, Default)]
pub struct Case {
    pub id: String,
    pub suite: String,
    pub repo: String,
    pub name: String,
    pub agent: Option<String>,
    pub commit: Option<String>,
    pub tree: Option<String>,
    pub wall_ms: i64,
    pub start_from: String,
    pub from: i64,
    pub to: i64,
    pub turns_all: i64,
    pub script: Json,
    pub checks: Json,
    pub set: String,
    pub context: String,
    pub setup: Option<String>,
    pub model: Option<String>,
    /// The session's permission mode, so that a replay asks what it asked.
    pub permission_mode: Option<String>,
    pub cost_usd: f64,
    pub made_by: String,
    pub created: i64,
}

const C_COLS: &str = "id, suite, repo, name, agent, \"commit\", tree, wall_ms, start_from, turns_from, turns_to, turns_all, script, checks, \"set\", context, setup, model, permission_mode, cost_usd, made_by, created";

impl Case {
    fn of(r: &[Value]) -> Case {
        let js = |v: &Value| get_str(v).and_then(|s| serde_json::from_str::<Json>(&s).ok()).unwrap_or(json!([]));
        Case {
            id: get_str(&r[0]).unwrap_or_default(),
            suite: get_str(&r[1]).unwrap_or_default(),
            repo: get_str(&r[2]).unwrap_or_default(),
            name: get_str(&r[3]).unwrap_or_default(),
            agent: get_str(&r[4]),
            commit: get_str(&r[5]),
            tree: get_str(&r[6]),
            wall_ms: get_int(&r[7]).unwrap_or(0),
            start_from: get_str(&r[8]).unwrap_or_default(),
            from: get_int(&r[9]).unwrap_or(1),
            to: get_int(&r[10]).unwrap_or(1),
            turns_all: get_int(&r[11]).unwrap_or(1),
            script: js(&r[12]),
            checks: js(&r[13]),
            set: get_str(&r[14]).unwrap_or_else(|| "train".into()),
            context: get_str(&r[15]).unwrap_or_else(|| "fresh".into()),
            setup: get_str(&r[16]),
            model: get_str(&r[17]),
            permission_mode: get_str(&r[18]),
            cost_usd: match &r[19] {
                Value::Float(f) => *f,
                Value::Int(i) => *i as f64,
                _ => 0.0,
            },
            made_by: get_str(&r[20]).unwrap_or_default(),
            created: get_ms(&r[21]).unwrap_or(0),
        }
    }

    fn put(&self, sup: &Supervisor) -> Result<(), ApiError> {
        put_row(
            sup,
            "eval_cases",
            &[
                ("id", sv(&self.id)),
                ("suite", sv(&self.suite)),
                ("repo", sv(&self.repo)),
                ("name", sv(&self.name)),
                ("agent", ov(&self.agent)),
                ("commit", ov(&self.commit)),
                ("tree", ov(&self.tree)),
                ("wall_ms", Value::Int(self.wall_ms)),
                ("start_from", sv(&self.start_from)),
                ("turns_from", Value::Int(self.from)),
                ("turns_to", Value::Int(self.to)),
                ("turns_all", Value::Int(self.turns_all)),
                ("script", sv(&self.script.to_string())),
                ("checks", sv(&self.checks.to_string())),
                ("set", sv(&self.set)),
                ("context", sv(&self.context)),
                ("setup", ov(&self.setup)),
                ("model", ov(&self.model)),
                ("permission_mode", ov(&self.permission_mode)),
                ("cost_usd", Value::Float(self.cost_usd)),
                ("made_by", sv(&self.made_by)),
                ("created", ts(self.created)),
            ],
        )
    }

    /// The prompts it plays, in order.
    pub fn prompts(&self) -> Vec<String> {
        self.script.as_array().into_iter().flatten().filter(|x| x["kind"] == "prompt").filter_map(|x| x["text"].as_str().map(str::to_string)).collect()
    }

    pub fn to_json(&self) -> Json {
        let n = |k: &str| self.script.as_array().map(|s| s.iter().filter(|x| x["kind"] == k).count()).unwrap_or(0);
        json!({
            "id": self.id, "suite": self.suite, "repo": self.repo, "name": self.name, "agent": self.agent, "commit": self.commit, "tree": self.tree, "wall_ms": self.wall_ms,
            "start_from": self.start_from, "from": self.from, "to": self.to, "turns_all": self.turns_all,
            "script": self.script, "checks": self.checks, "set": self.set, "context": self.context, "setup": self.setup,
            "model": self.model, "permission_mode": self.permission_mode, "cost_usd": self.cost_usd, "made_by": self.made_by, "created": iso(self.created),
            "prompts": n("prompt"), "permissions": n("permission"), "answers": n("answer"),
        })
    }
}

pub fn case(sup: &Supervisor, id: &str) -> Option<Case> {
    let rows = sup.db().query(&format!("SELECT {C_COLS} FROM eval_cases WHERE id = :id LIMIT 1"), &Params::new().set("id", sv(id))).ok()?;
    rows.first().map(|r| Case::of(r))
}

pub fn cases_of(sup: &Supervisor, repo: &str) -> Vec<Case> {
    let rows = sup.db().query(&format!("SELECT {C_COLS} FROM eval_cases WHERE repo = :repo ORDER BY created ASC LIMIT 10000"), &Params::new().set("repo", sv(repo))).unwrap_or_default();
    rows.iter().map(|r| Case::of(r)).collect()
}

/// Where a case keeps the conversation before its first turn.
pub fn before_file(sup: &Supervisor, case: &str) -> PathBuf {
    sup.cfg.data_dir.join("cases").join(case).join("before.jsonl")
}

/// The ref that keeps a case's commit from git's garbage collection.
fn case_ref(id: &str) -> String {
    format!("refs/workbench/cases/{id}")
}

fn git_in(dir: &Path, args: &[&str]) -> bool {
    std::process::Command::new("git").arg("-C").arg(dir).args(args).env("GIT_TERMINAL_PROMPT", "0").output().is_ok_and(|o| o.status.success())
}

/// A user's prompt line of a transcript: its text, or `None` for every
/// other line (the assistant's, a tool's result, Claude Code's own notes).
pub(super) fn transcript_prompt(line: &Json) -> Option<String> {
    if line["type"] != "user" || line["isMeta"] == true {
        return None;
    }
    let c = &line["message"]["content"];
    let text = match c {
        Json::String(s) => s.clone(),
        Json::Array(items) => {
            if items.iter().any(|i| i["type"] == "tool_result") {
                return None;
            }
            items.iter().filter(|i| i["type"] == "text").filter_map(|i| i["text"].as_str()).collect::<Vec<_>>().join("\n")
        }
        _ => return None,
    };
    let t = text.trim();
    if t.is_empty() || t.starts_with("<command-") || t.starts_with("<local-command") {
        return None;
    }
    Some(t.to_string())
}

/// The lines of a transcript before the user's prompt of turn `from`
/// (whose words are `prompt`): the conversation a replay resumes.
pub(super) fn cut_transcript(text: &str, from: i64, prompt: &str) -> Option<String> {
    let mut seen = 0;
    let want = prompt.trim();
    let mut by_count = None;
    for (i, line) in text.lines().enumerate() {
        let Ok(v) = serde_json::from_str::<Json>(line) else { continue };
        if let Some(p) = transcript_prompt(&v) {
            seen += 1;
            if seen >= from && p == want {
                return Some(text.lines().take(i).collect::<Vec<_>>().join("\n") + "\n");
            }
            if seen == from && by_count.is_none() {
                by_count = Some(i);
            }
        }
    }
    by_count.map(|i| text.lines().take(i).collect::<Vec<_>>().join("\n") + "\n")
}

#[derive(Deserialize, Default)]
pub struct CreateB {
    pub agent: String,
    #[serde(alias = "turns_from")]
    pub from: Option<i64>,
    #[serde(alias = "turns_to")]
    pub to: Option<i64>,
    pub suite: Option<String>,
    pub name: Option<String>,
    /// The checks to keep: the proposal's (all of their fields), or new
    /// ones; without it, the proposal's ticked ones.
    pub checks: Option<Vec<Json>>,
    pub set: Option<String>,
    pub setup: Option<String>,
}

/// A check as a case keeps it, or why it cannot be one.
fn check_of(c: &Json, i: usize) -> Result<Json, String> {
    let kind = c["kind"].as_str().unwrap_or("");
    let id = c["id"].as_str().map(str::to_string).unwrap_or_else(|| format!("k{}", i + 1));
    match kind {
        "fail_to_pass" | "pass_to_pass" => {
            let words: Vec<String> = c["run"].as_array().map(|a| a.iter().filter_map(|w| w.as_str().map(str::to_string)).collect()).unwrap_or_default();
            let test = c["test"].as_str().unwrap_or("");
            if words.is_empty() || words.len() > 64 || test.is_empty() {
                return Err(format!("{kind} needs its test and the words that run it (run)"));
            }
            Ok(json!({ "id": id, "kind": kind, "test": test, "run": words }))
        }
        "command" => {
            let run = c["run"].as_str().map(str::trim).unwrap_or("");
            if run.is_empty() || run.len() > 2000 || run.contains('\n') {
                return Err("a command check is one line of shell, its exit code 0 when the replay worked".into());
            }
            Ok(json!({ "id": id, "kind": kind, "run": run, "expect": c["expect"].as_i64().unwrap_or(0) }))
        }
        "limit" => {
            let metric = c["metric"].as_str().unwrap_or("");
            if !matches!(metric, "prompts" | "interventions" | "turns") {
                return Err("a limit is on prompts, interventions or turns".into());
            }
            Ok(json!({ "id": id, "kind": kind, "metric": metric, "max": c["max"].as_i64().unwrap_or(0).max(0) }))
        }
        "never" => {
            let pattern = c["pattern"].as_str().map(str::trim).unwrap_or("");
            if pattern.is_empty() || pattern.len() > 200 {
                return Err("never needs the words no Bash command of a replay may contain (pattern)".into());
            }
            Ok(json!({ "id": id, "kind": kind, "pattern": pattern }))
        }
        k => Err(format!("{k:?} is not a check: fail_to_pass, pass_to_pass, command, limit or never")),
    }
}

fn suite_for(sup: &Supervisor, repo: &str, name: Option<&str>) -> Result<String, ApiError> {
    let name = name.map(str::trim).filter(|n| !n.is_empty()).map(|n| one_line(n, 64)).unwrap_or_else(|| format!("{} core", Path::new(repo).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "repo".into())));
    let rows = sup.db().query("SELECT id FROM eval_suites WHERE repo = :repo AND name = :name LIMIT 1", &Params::new().set("repo", sv(repo)).set("name", sv(&name))).unwrap_or_default();
    if let Some(id) = rows.first().and_then(|r| get_str(&r[0])) {
        return Ok(id);
    }
    let id = format!("s{}", &hash(&format!("{repo}\n{name}"))[..10]);
    put_row(sup, "eval_suites", &[("id", sv(&id)), ("repo", sv(repo)), ("name", sv(&name)), ("note", Value::Null), ("created", ts(now_ms()))])?;
    Ok(id)
}

/// `POST /api/harness/cases`: a case from a stretch of a session.
pub fn create(sup: &Supervisor, b: CreateB) -> Result<Json, ApiError> {
    let p = propose(sup, &ProposeQ { agent: b.agent.clone(), from: b.from, to: b.to })?;
    let Some(commit) = p["commit"].as_str().map(str::to_string) else {
        return Err(ApiError::conflict(p["warnings"][0].as_str().unwrap_or("this stretch cannot be replayed").to_string()));
    };
    let repo = p["repo"].as_str().unwrap_or("").to_string();
    let checks: Vec<Json> = match &b.checks {
        Some(cs) => cs.iter().enumerate().map(|(i, c)| check_of(c, i)).collect::<Result<_, _>>().map_err(ApiError::bad)?,
        None => p["checks"].as_array().into_iter().flatten().filter(|c| c["on"] == true).enumerate().map(|(i, c)| check_of(c, i)).collect::<Result<_, _>>().map_err(ApiError::bad)?,
    };
    if checks.is_empty() {
        return Err(ApiError::bad("a case needs at least one check: what says whether a replay worked"));
    }
    let set = match b.set.as_deref() {
        None => p["set"].as_str().unwrap_or("train").to_string(),
        Some(s @ ("train" | "held_out")) => s.to_string(),
        Some(s) => return Err(ApiError::bad(format!("set is train or held_out, not {s}"))),
    };
    let setup = b.setup.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    if setup.as_ref().is_some_and(|s| s.len() > 2000 || s.contains('\n')) {
        return Err(ApiError::bad("setup is one line of shell, run in the sandbox before the agent starts"));
    }
    let (from, to) = (p["from"].as_i64().unwrap_or(1), p["to"].as_i64().unwrap_or(1));
    let id = format!("c{}", &hash(&format!("{}:{from}:{to}:{}", b.agent, now_ms()))[..10]);
    let mut context = p["context"].as_str().unwrap_or("fresh").to_string();
    // The conversation before the stretch, kept with the case.
    if context == "transcript" {
        let first = p["script"].as_array().and_then(|s| s.iter().find(|x| x["kind"] == "prompt")).and_then(|x| x["text"].as_str()).unwrap_or("").to_string();
        let kept = sup.transcript_of(&b.agent).and_then(|t| std::fs::read_to_string(t).ok()).and_then(|t| cut_transcript(&t, from, &first)).and_then(|cut| {
            let f = before_file(sup, &id);
            std::fs::create_dir_all(f.parent()?).ok()?;
            std::fs::write(&f, cut).ok()
        });
        if kept.is_none() {
            context = "code_only".into();
        }
    }
    let tree = p["tree"].as_str().map(str::to_string);
    let dir = PathBuf::from(&repo);
    git_in(&dir, &["update-ref", &case_ref(&id), tree.as_deref().unwrap_or(&commit)]);
    let c = Case {
        id: id.clone(),
        suite: suite_for(sup, &repo, b.suite.as_deref())?,
        repo,
        name: b.name.map(|n| one_line(n.trim(), 80)).filter(|n| !n.is_empty()).unwrap_or_else(|| p["name"].as_str().unwrap_or("case").to_string()),
        agent: Some(b.agent.clone()),
        commit: Some(commit),
        tree,
        wall_ms: p["wall_ms"].as_i64().unwrap_or(0),
        start_from: p["start_from"].as_str().unwrap_or("").to_string(),
        from,
        to,
        turns_all: p["turns_all"].as_i64().unwrap_or(to),
        script: p["script"].clone(),
        checks: Json::Array(checks),
        set,
        context,
        setup,
        model: p["model"].as_str().map(str::to_string),
        permission_mode: p["permission_mode"].as_str().map(str::to_string),
        cost_usd: p["cost_usd"].as_f64().unwrap_or(0.0),
        made_by: "you".into(),
        created: now_ms(),
    };
    c.put(sup)?;
    Ok(json!({ "case": c.to_json(), "warnings": p["warnings"] }))
}

#[derive(Deserialize, Default)]
pub struct PatchB {
    pub name: Option<String>,
    pub set: Option<String>,
    pub checks: Option<Vec<Json>>,
    pub setup: Option<String>,
}

pub fn patch(sup: &Supervisor, id: &str, b: PatchB) -> Result<Json, ApiError> {
    let mut c = case(sup, id).ok_or_else(|| ApiError::not_found(format!("no case {id}")))?;
    if let Some(n) = b.name.map(|n| one_line(n.trim(), 80)).filter(|n| !n.is_empty()) {
        c.name = n;
    }
    match b.set.as_deref() {
        None => {}
        Some(s @ ("train" | "held_out")) => c.set = s.into(),
        Some(s) => return Err(ApiError::bad(format!("set is train or held_out, not {s}"))),
    }
    if let Some(cs) = &b.checks {
        let checks: Vec<Json> = cs.iter().enumerate().map(|(i, c)| check_of(c, i)).collect::<Result<_, _>>().map_err(ApiError::bad)?;
        if checks.is_empty() {
            return Err(ApiError::bad("a case needs at least one check"));
        }
        c.checks = Json::Array(checks);
    }
    if let Some(s) = b.setup {
        let s = s.trim().to_string();
        if s.len() > 2000 || s.contains('\n') {
            return Err(ApiError::bad("setup is one line of shell"));
        }
        c.setup = Some(s).filter(|s| !s.is_empty());
    }
    c.put(sup)?;
    Ok(json!({ "case": c.to_json() }))
}

/// Deletes a case and its ref; its runs stay, as history.
pub fn delete(sup: &Supervisor, id: &str) -> Result<Json, ApiError> {
    let c = case(sup, id).ok_or_else(|| ApiError::not_found(format!("no case {id}")))?;
    sup.db().query("DELETE FROM eval_cases WHERE id = :id", &Params::new().set("id", sv(id))).map_err(ApiError::internal)?;
    git_in(Path::new(&c.repo), &["update-ref", "-d", &case_ref(id)]);
    let _ = std::fs::remove_dir_all(before_file(sup, id).parent().unwrap_or(Path::new("/nonexistent")));
    Ok(json!({ "deleted": id }))
}

/// `GET /api/harness/suites?repo=`: the repository's suites and cases,
/// each case's latest result under each version it ran, and the sessions
/// a case can be made from.
pub fn suites(sup: &Supervisor, repo: &str) -> Result<Json, ApiError> {
    let rows = sup.db().query("SELECT id, name, created FROM eval_suites WHERE repo = :repo ORDER BY created ASC LIMIT 1000", &Params::new().set("repo", sv(repo))).unwrap_or_default();
    let cases = cases_of(sup, repo);
    let results = super::replay::results_by_case(sup, repo);
    let suites: Vec<Json> = rows
        .iter()
        .map(|r| {
            let id = get_str(&r[0]).unwrap_or_default();
            let cs: Vec<Json> = cases
                .iter()
                .filter(|c| c.suite == id)
                .map(|c| {
                    let mut j = c.to_json();
                    j["results"] = results.get(&c.id).cloned().unwrap_or(json!([]));
                    j
                })
                .collect();
            json!({ "id": id, "name": get_str(&r[1]), "created": get_ms(&r[2]).map(iso), "cases": cs })
        })
        .collect();
    let used: HashMap<String, usize> = cases.iter().filter_map(|c| c.agent.clone()).fold(HashMap::new(), |mut m, a| {
        *m.entry(a).or_default() += 1;
        m
    });
    let mut agents: Vec<Agent> = sup.agent_rows().into_iter().filter(|a| a.repo == repo).collect();
    agents.sort_by_key(|a| std::cmp::Reverse(a.created));
    let sessions: Vec<Json> = agents
        .iter()
        .take(40)
        .map(|a| {
            let turns = sup.all_events(&a.id).map(|e| e.iter().filter(|e| e.kind == "UserPromptSubmit").count()).unwrap_or(0);
            json!({ "id": a.id, "name": a.name, "task": a.task.as_deref().map(|t| one_line(t, 160)), "turns": turns, "running": sup.is_running(&a.id), "created": iso(a.created), "ended": a.ended.map(iso), "outcome": a.outcome, "cases": used.get(&a.id).copied().unwrap_or(0) })
        })
        .collect();
    Ok(json!({ "repo": repo, "suites": suites, "sessions": sessions, "cases": cases.len() }))
}

// ---------------------------------------------------------------- routes

#[derive(Deserialize)]
struct AgentQ {
    agent: String,
}

async fn timeline_route(State(app): State<App>, Query(q): Query<AgentQ>) -> ApiResult {
    let sup = app.sup.clone();
    Ok(AxJson(blocking(move || timeline(&sup, &q.agent)).await?).into_response())
}

async fn propose_route(State(app): State<App>, Query(q): Query<ProposeQ>) -> ApiResult {
    let sup = app.sup.clone();
    Ok(AxJson(blocking(move || propose(&sup, &q)).await?).into_response())
}

async fn create_route(State(app): State<App>, headers: HeaderMap, body: Bytes) -> ApiResult {
    user_only(&app, &headers)?;
    let b: CreateB = parse_body(&body)?;
    let sup = app.sup.clone();
    Ok((StatusCode::CREATED, AxJson(blocking(move || create(&sup, b)).await?)).into_response())
}

async fn suites_route(State(app): State<App>, Query(q): Query<super::RepoQ>) -> ApiResult {
    let sup = app.sup.clone();
    Ok(AxJson(blocking(move || {
        let repo = super::repo_or_default(&sup, q.repo)?;
        suites(&sup, &repo)
    })
    .await?)
    .into_response())
}

async fn case_route(State(app): State<App>, AxPath(id): AxPath<String>) -> ApiResult {
    let sup = app.sup.clone();
    Ok(AxJson(blocking(move || {
        let c = case(&sup, &id).ok_or_else(|| ApiError::not_found(format!("no case {id}")))?;
        let results = super::replay::results_by_case(&sup, &c.repo);
        Ok::<Json, ApiError>(json!({ "case": c.to_json(), "results": results.get(&c.id).cloned().unwrap_or(json!([])) }))
    })
    .await?)
    .into_response())
}

async fn patch_route(State(app): State<App>, headers: HeaderMap, AxPath(id): AxPath<String>, body: Bytes) -> ApiResult {
    user_only(&app, &headers)?;
    let b: PatchB = parse_body(&body)?;
    let sup = app.sup.clone();
    Ok(AxJson(blocking(move || patch(&sup, &id, b)).await?).into_response())
}

async fn delete_route(State(app): State<App>, headers: HeaderMap, AxPath(id): AxPath<String>) -> ApiResult {
    user_only(&app, &headers)?;
    let sup = app.sup.clone();
    Ok(AxJson(blocking(move || delete(&sup, &id)).await?).into_response())
}

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/harness/timeline", get(timeline_route))
        .route("/api/harness/cases/propose", get(propose_route))
        .route("/api/harness/cases", axum::routing::post(create_route))
        .route("/api/harness/cases/:id", get(case_route).post(patch_route).delete(delete_route))
        .route("/api/harness/suites", get(suites_route))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_are_read_as_kinds() {
        assert_eq!(answer_kind("Yes"), "yes");
        assert_eq!(answer_kind("Yes, and don't ask again for echo commands in /x"), "yes_more");
        assert_eq!(answer_kind("Yes, and switch to auto mode · auto mode handles these prompts for you"), "yes");
        assert_eq!(answer_kind("No"), "no");
        assert_eq!(answer_kind("Cancelled (Esc)"), "no");
        assert_eq!(perm_key(&json!({ "tool": "Bash", "command": "PYTHONPATH=. python3 -m pytest -q tests/" })), "Bash: PYTHONPATH=. python3 -m pytest");
        assert_eq!(perm_key(&json!({ "tool": "Edit", "file": "/w/a.py", "rel": "a.py" })), "Edit: a.py");
    }

    #[test]
    fn a_transcript_is_cut_before_the_turn_it_starts() {
        let lines = [
            json!({ "type": "user", "message": { "role": "user", "content": "first ask" } }),
            json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "id": "t1" }] } }),
            json!({ "type": "user", "message": { "content": [{ "type": "tool_result", "tool_use_id": "t1", "content": "hi" }] } }),
            json!({ "type": "user", "isMeta": true, "message": { "content": "Caveat" } }),
            json!({ "type": "user", "message": { "content": [{ "type": "text", "text": "second ask" }] } }),
            json!({ "type": "assistant", "message": { "content": [{ "type": "text", "text": "done" }] } }),
        ];
        let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
        let cut = cut_transcript(&text, 2, "second ask").unwrap();
        assert_eq!(cut.lines().count(), 4, "{cut}");
        assert!(!cut.contains("second ask"));
        // By its count when the words differ (Claude Code may expand them).
        assert_eq!(cut_transcript(&text, 2, "other words").unwrap().lines().count(), 4);
        assert!(cut_transcript(&text, 3, "x").is_none());
        assert_eq!(name_of("Fix the rounding in money.py please now"), "fix-the-rounding-in-moneypy");
    }

    #[test]
    fn checks_are_checked() {
        assert!(check_of(&json!({ "kind": "command", "run": "grep -q fixed notes.txt" }), 0).is_ok());
        assert!(check_of(&json!({ "kind": "command", "run": "a\nb" }), 0).is_err());
        assert!(check_of(&json!({ "kind": "fail_to_pass", "test": "t", "run": [] }), 0).is_err());
        assert!(check_of(&json!({ "kind": "limit", "metric": "dollars", "max": 1 }), 0).is_err());
        assert_eq!(check_of(&json!({ "kind": "never", "pattern": " git push " }), 2).unwrap()["id"], "k3");
        assert!(check_of(&json!({ "kind": "judge" }), 0).is_err());
    }
}
