//! Away: when you last looked (`seen`), and the digest of what happened
//! since: outcomes done, questions decided (by you, a rule or the
//! terminal) and still open, what ChloeCtx learned, a story line per
//! agent, and the moments of decisions to replay.

use std::collections::{BTreeMap, HashSet};

use chloedb::query::Params;
use chloedb::Value;
use serde_json::{json, Value as Json};

use crate::db::{get_ms, get_str, iso, now_ms, ts, Agent, Event};
use crate::levels::Levels;
use crate::supervisor::{one_line, ApiError, Supervisor};
use crate::{goals, questions};

/// When there is no `last_seen` yet: the last day.
const DEFAULT_WINDOW_MS: i64 = 24 * 3600 * 1000;
/// A replay's window before and after its moment.
const BEFORE_MS: i64 = 20_000;
const AFTER_MS: i64 = 3_000;

/// The value of a key of `seen`, and when it was set.
pub fn get_seen(sup: &Supervisor, key: &str) -> Option<(String, i64)> {
    let p = Params::new().set("key", Value::Str(key.to_string()));
    let rows = sup.db().query("SELECT value, ts FROM seen WHERE id = :key LIMIT 1", &p).ok()?;
    let r = rows.first()?;
    Some((get_str(&r[0])?, get_ms(&r[1]).unwrap_or(0)))
}

pub fn set_seen(sup: &Supervisor, key: &str, value: &str) -> Result<(), ApiError> {
    let p = Params::new().set("key", Value::Str(key.to_string())).set("value", Value::Str(value.to_string())).set("ts", ts(now_ms()));
    sup.db().query("INSERT OR REPLACE INTO seen (id, value, ts) VALUES (:key, :value, :ts)", &p).map_err(ApiError::internal)?;
    Ok(())
}

fn last_seen(sup: &Supervisor) -> Option<i64> {
    get_seen(sup, "last_seen").and_then(|(v, _)| v.parse().ok())
}

/// `POST /api/seen`: you looked now (or at `ts_ms`).
pub fn seen(sup: &Supervisor, at: Option<i64>) -> Result<Json, ApiError> {
    let before = last_seen(sup);
    let t = at.filter(|t| *t > 0).unwrap_or_else(now_ms);
    set_seen(sup, "last_seen", &t.to_string())?;
    Ok(json!({ "ok": true, "last_seen_ms": t, "previous_ms": before }))
}

/// Events since `since` (ms), oldest first.
fn events_since(sup: &Supervisor, since: i64) -> Vec<Event> {
    let p = Params::new().set("since", ts(since));
    let rows = sup.db().query("SELECT id, seq, ts, agent, kind, tool, summary FROM events WHERE ts >= :since ORDER BY seq ASC LIMIT 200000", &p).unwrap_or_default();
    rows.iter()
        .map(|r| Event {
            id: get_str(&r[0]).unwrap_or_default(),
            seq: crate::db::get_int(&r[1]).unwrap_or(0),
            ts: get_ms(&r[2]).unwrap_or(0),
            agent: get_str(&r[3]).unwrap_or_default(),
            kind: get_str(&r[4]).unwrap_or_default(),
            tool: get_str(&r[5]),
            summary: get_str(&r[6]),
            data: None,
        })
        .collect()
}

/// A replay window around `at` (ms since the epoch) in an agent's
/// recording, when there is one.
fn replay_of(sup: &Supervisor, a: &Agent, at: i64, label: String) -> Option<Json> {
    let st = a.started_ms?;
    if !sup.session_dir(&a.id).join("term.cast").is_file() || at < st {
        return None;
    }
    let rel = at - st;
    Some(json!({ "agent": a.id, "from_ms": (rel - BEFORE_MS).max(0), "to_ms": rel + AFTER_MS, "at_ms": at, "label": label }))
}

/// `GET /api/digest?since=<ms>&repo=`.
pub fn digest(sup: &Supervisor, lv: &Levels, since: Option<i64>, repo: Option<&str>) -> Result<Json, ApiError> {
    let now = now_ms();
    let seen_at = last_seen(sup);
    let since = since.or(seen_at).unwrap_or(now - DEFAULT_WINDOW_MS);
    let repo = match repo.filter(|r| !r.is_empty()) {
        Some(r) => Some(crate::trail::repo_dir(Some(r))?.to_string_lossy().to_string()),
        None => None,
    };
    let in_repo = |r: Option<&str>| repo.as_deref().is_none_or(|want| r == Some(want));
    questions::expire_stale(sup, lv);
    let agents: Vec<Agent> = sup.agent_rows().into_iter().filter(|a| in_repo(Some(&a.repo))).collect();
    let by_id = |id: &str| agents.iter().find(|a| a.id == id);
    let ids: HashSet<String> = agents.iter().map(|a| a.id.clone()).collect();

    // Outcomes done since.
    let goal_repo: BTreeMap<String, String> = goals::goals(sup, None).into_iter().map(|g| (g.id, g.repo)).collect();
    let done: Vec<goals::Outcome> = goals::refresh(sup, lv, None).into_iter().filter(|o| o.state == "done" && o.updated >= since && in_repo(goal_repo.get(&o.goal).map(String::as_str))).collect();

    // Questions answered since, and open ones.
    let qs: Vec<questions::Question> = questions::all(sup).into_iter().filter(|q| in_repo(q.repo.as_deref())).collect();
    let decided: Vec<&questions::Question> = qs.iter().filter(|q| q.state == "answered" && q.answered.is_some_and(|t| t >= since)).collect();
    let open: Vec<&questions::Question> = qs.iter().filter(|q| q.state == "open").collect();
    let by = |p: &str| decided.iter().filter(|q| q.answered_by.as_deref().unwrap_or("").starts_with(p)).count();
    let rules: Vec<questions::Rule> = questions::rules(sup, repo.as_deref(), false).into_iter().filter(|r| r.created >= since).collect();

    // What ChloeCtx learned since, in the repositories that saw work.
    let evs: Vec<Event> = events_since(sup, since).into_iter().filter(|e| ids.contains(&e.agent)).collect();
    let mut repos: Vec<String> = Vec::new();
    for e in &evs {
        if let Some(a) = by_id(&e.agent) {
            if !repos.contains(&a.repo) {
                repos.push(a.repo.clone());
            }
        }
    }
    if let Some(r) = &repo {
        if !repos.contains(r) {
            repos.insert(0, r.clone());
        }
    }
    let mut learned = Vec::new();
    let mut missing = Vec::new();
    for r in repos.iter().take(5) {
        let (l, m) = goals::learned(sup, r, since / 1000, 100);
        learned.extend(l);
        for x in m {
            if !missing.contains(&x) {
                missing.push(x);
            }
        }
    }

    // A story line per agent that did something since.
    let mut active: Vec<&Agent> = agents.iter().filter(|a| evs.iter().any(|e| e.agent == a.id)).collect();
    active.sort_by_key(|a| evs.iter().filter(|e| e.agent == a.id).map(|e| e.ts).max().unwrap_or(0));
    let mut story = Vec::new();
    for a in active {
        let mine: Vec<&Event> = evs.iter().filter(|e| e.agent == a.id).collect();
        let last = mine.last().map(|e| e.ts).unwrap_or(since);
        let turns: Vec<String> = mine.iter().filter(|e| e.kind == "UserPromptSubmit").filter_map(|e| e.summary.as_deref()).map(|s| one_line(s.trim_start_matches("prompt: "), 80)).collect();
        let mut text = match turns.len() {
            0 => String::new(),
            1 => format!("worked on “{}”", turns[0]),
            n => format!("worked on {n} turns, the last “{}”", turns[n - 1]),
        };
        let now_line = match a.state.as_str() {
            "waiting" => a.waiting_for.as_deref().map(|w| format!("is waiting on you: {}", one_line(w, 160))),
            "exited" => Some(format!("ended{}", a.exit_code.map(|c| format!(" (exit {c})")).unwrap_or_default())),
            _ => a.narration.as_deref().map(|n| format!("now: {}", one_line(n, 160))),
        };
        if let Some(l) = now_line {
            if !text.is_empty() {
                text.push_str("; ");
            }
            text.push_str(&l);
        }
        let mut chips = Vec::new();
        let edits: HashSet<String> = mine.iter().filter(|e| e.kind == "PostToolUse" && matches!(e.tool.as_deref(), Some("Edit" | "MultiEdit" | "Write" | "NotebookEdit"))).filter_map(|e| e.summary.clone()).collect();
        if !edits.is_empty() {
            chips.push(json!({ "kind": "edits", "text": format!("{} file{} edited", edits.len(), if edits.len() == 1 { "" } else { "s" }) }));
        }
        for o in done.iter().filter(|o| o.agent.as_deref() == Some(&a.id)) {
            chips.push(json!({ "kind": "done", "text": o.title }));
        }
        for q in decided.iter().filter(|q| q.agent.as_deref() == Some(&a.id)) {
            let who = q.answered_by.as_deref().unwrap_or("you");
            let kind = if who.starts_with("rule:") { "rule" } else { "decided" };
            chips.push(json!({ "kind": kind, "text": one_line(q.answer.as_deref().unwrap_or(""), 80), "question": q.id, "by": who }));
        }
        for q in open.iter().filter(|q| q.agent.as_deref() == Some(&a.id)) {
            chips.push(json!({ "kind": "waiting", "text": one_line(&q.text, 80), "question": q.id }));
        }
        story.push(json!({ "ts": iso(last), "ts_ms": last, "agent": sup.agent_json_of(&a.id), "text": text, "chips": chips }));
    }

    // The moments of decisions: answered questions, and decision records.
    let mut replay = Vec::new();
    for q in &decided {
        if let (Some(a), Some(t)) = (q.agent.as_deref().and_then(by_id), q.answered) {
            if let Some(r) = replay_of(sup, a, t, format!("{}: {}", a.name, one_line(&q.text, 80))) {
                replay.push(r);
            }
        }
    }
    let s = crate::trail::Sessions::load(sup);
    for rec in learned.iter().filter(|r| r["kind"] == "decision") {
        let a = rec["session"].as_str().and_then(|sid| s.agent(sid)).filter(|a| ids.contains(&a.id));
        if let (Some(a), Some(t)) = (a, rec["ts_ms"].as_i64()) {
            if let Some(r) = replay_of(sup, a, t, format!("{}: {}", a.name, one_line(rec["text"].as_str().unwrap_or(""), 80))) {
                replay.push(r);
            }
        }
    }
    replay.sort_by_key(|r| r["at_ms"].as_i64().unwrap_or(0));

    let mut qlist: Vec<Json> = open.iter().map(|q| q.to_json(sup)).collect();
    qlist.extend(decided.iter().map(|q| q.to_json(sup)));
    Ok(json!({
        "since_ms": since,
        "until_ms": now,
        "last_seen_ms": seen_at,
        "away_ms": now - since,
        "tiles": { "done": done.len(), "decided": decided.len(), "waiting": open.len(), "learned": learned.len() },
        "decided_by": { "you": by("you"), "rule": by("rule:"), "terminal": by("terminal") },
        "done": done.iter().map(|o| o.to_json(sup)).collect::<Vec<_>>(),
        "story": story,
        "questions": qlist,
        "rules": rules.iter().map(|r| r.to_json()).collect::<Vec<_>>(),
        "learned": learned,
        "replay": replay,
        "happened": !evs.is_empty() || !done.is_empty() || !decided.is_empty(),
        "missing": missing,
    }))
}
