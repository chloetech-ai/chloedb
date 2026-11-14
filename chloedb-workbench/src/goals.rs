//! Goals and outcomes (Mission): a goal of a repository, the 3 to 8
//! outcomes a lead agent breaks it into, the agent working on each, and
//! the outcome's state following its agent's.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chloedb::query::Params;
use chloedb::Value;
use serde::Deserialize;
use serde_json::{json, Value as Json};

use crate::db::{get_int, get_ms, get_str, iso, now_ms, opt_str, ts, Agent};
use crate::levels::{new_id, Levels};
use crate::supervisor::{ApiError, SpawnRequest, Supervisor};
use crate::trail::{self, Ran};

pub const GOAL_STATES: [&str; 3] = ["active", "done", "dropped"];
pub const OUTCOME_STATES: [&str; 5] = ["next", "active", "needs_you", "done", "dropped"];

#[derive(Clone, Debug, Default)]
pub struct Goal {
    pub id: String,
    pub repo: String,
    pub text: String,
    pub state: String,
    pub lead: Option<String>,
    pub created: i64,
    pub done: Option<i64>,
}

pub(crate) const GOAL_COLS: &str = "id, repo, text, state, lead, created, done";

impl Goal {
    pub(crate) fn of(r: &[Value]) -> Goal {
        Goal {
            id: get_str(&r[0]).unwrap_or_default(),
            repo: get_str(&r[1]).unwrap_or_default(),
            text: get_str(&r[2]).unwrap_or_default(),
            state: get_str(&r[3]).unwrap_or_default(),
            lead: get_str(&r[4]),
            created: get_ms(&r[5]).unwrap_or(0),
            done: get_ms(&r[6]),
        }
    }

    fn put(&self, sup: &Supervisor) -> Result<(), ApiError> {
        let p = Params::new()
            .set("id", Value::Str(self.id.clone()))
            .set("repo", Value::Str(self.repo.clone()))
            .set("text", Value::Str(self.text.clone()))
            .set("state", Value::Str(self.state.clone()))
            .set("lead", opt_str(&self.lead))
            .set("created", ts(self.created))
            .set("done", self.done.map_or(Value::Null, ts));
        sup.db().query(&format!("INSERT OR REPLACE INTO goals ({GOAL_COLS}) VALUES (:id, :repo, :text, :state, :lead, :created, :done)"), &p).map_err(ApiError::internal)?;
        Ok(())
    }

    /// The row as JSON, with its outcomes counted.
    pub fn to_json(&self, outcomes: &[Outcome]) -> Json {
        let live: Vec<&Outcome> = outcomes.iter().filter(|o| o.state != "dropped").collect();
        let done = live.iter().filter(|o| o.state == "done").count();
        json!({
            "id": self.id, "repo": self.repo, "text": self.text, "state": self.state, "lead": self.lead,
            "created": iso(self.created), "done": self.done.map(iso),
            "outcomes_total": live.len(), "outcomes_done": done,
            "needs_you": live.iter().filter(|o| o.state == "needs_you").count(),
            "progress": if live.is_empty() { 0 } else { live.iter().map(|o| o.progress.clamp(0, 100)).sum::<i64>() / live.len() as i64 },
        })
    }
}

#[derive(Clone, Debug, Default)]
pub struct Outcome {
    pub id: String,
    pub goal: String,
    pub title: String,
    pub detail: Option<String>,
    pub state: String,
    pub agent: Option<String>,
    pub ord: i64,
    pub after: Option<String>,
    pub progress: i64,
    pub created: i64,
    pub updated: i64,
}

pub(crate) const OUTCOME_COLS: &str = "id, goal, title, detail, state, agent, ord, after, progress, created, updated";

impl Outcome {
    pub(crate) fn of(r: &[Value]) -> Outcome {
        Outcome {
            id: get_str(&r[0]).unwrap_or_default(),
            goal: get_str(&r[1]).unwrap_or_default(),
            title: get_str(&r[2]).unwrap_or_default(),
            detail: get_str(&r[3]),
            state: get_str(&r[4]).unwrap_or_default(),
            agent: get_str(&r[5]),
            ord: get_int(&r[6]).unwrap_or(0),
            after: get_str(&r[7]),
            progress: get_int(&r[8]).unwrap_or(0),
            created: get_ms(&r[9]).unwrap_or(0),
            updated: get_ms(&r[10]).unwrap_or(0),
        }
    }

    fn put(&self, sup: &Supervisor) -> Result<(), ApiError> {
        let p = Params::new()
            .set("id", Value::Str(self.id.clone()))
            .set("goal", Value::Str(self.goal.clone()))
            .set("title", Value::Str(self.title.clone()))
            .set("detail", opt_str(&self.detail))
            .set("state", Value::Str(self.state.clone()))
            .set("agent", opt_str(&self.agent))
            .set("ord", Value::Int(self.ord))
            .set("after", opt_str(&self.after))
            .set("progress", Value::Int(self.progress))
            .set("created", ts(self.created))
            .set("updated", ts(self.updated));
        sup.db()
            .query(&format!("INSERT OR REPLACE INTO outcomes ({OUTCOME_COLS}) VALUES (:id, :goal, :title, :detail, :state, :agent, :ord, :after, :progress, :created, :updated)"), &p)
            .map_err(ApiError::internal)?;
        Ok(())
    }

    /// The row as JSON, with its agent's name and state.
    pub fn to_json(&self, sup: &Supervisor) -> Json {
        let a = self.agent.as_deref().and_then(|id| sup.agent_row(id));
        json!({
            "id": self.id, "goal": self.goal, "title": self.title, "detail": self.detail, "state": self.state,
            "agent": self.agent, "agent_name": a.as_ref().map(|a| a.name.clone()), "agent_state": a.as_ref().map(|a| a.state.clone()),
            "ord": self.ord, "after": self.after, "progress": self.progress,
            "created": iso(self.created), "updated": iso(self.updated), "updated_ms": self.updated,
        })
    }
}

pub fn goal(sup: &Supervisor, id: &str) -> Result<Goal, ApiError> {
    let p = Params::new().set("id", Value::Str(id.to_string()));
    let rows = sup.db().query(&format!("SELECT {GOAL_COLS} FROM goals WHERE id = :id LIMIT 1"), &p).map_err(ApiError::internal)?;
    rows.first().map(|r| Goal::of(r)).ok_or_else(|| ApiError::not_found(format!("no goal {id}")))
}

pub fn goals(sup: &Supervisor, repo: Option<&str>) -> Vec<Goal> {
    let rows = match repo {
        Some(r) => sup.db().query(&format!("SELECT {GOAL_COLS} FROM goals WHERE repo = :repo ORDER BY created DESC LIMIT 10000"), &Params::new().set("repo", Value::Str(r.to_string()))),
        None => sup.db().query(&format!("SELECT {GOAL_COLS} FROM goals ORDER BY created DESC LIMIT 10000"), &Params::new()),
    };
    rows.unwrap_or_default().iter().map(|r| Goal::of(r)).collect()
}

pub fn outcome(sup: &Supervisor, id: &str) -> Result<Outcome, ApiError> {
    let p = Params::new().set("id", Value::Str(id.to_string()));
    let rows = sup.db().query(&format!("SELECT {OUTCOME_COLS} FROM outcomes WHERE id = :id LIMIT 1"), &p).map_err(ApiError::internal)?;
    rows.first().map(|r| Outcome::of(r)).ok_or_else(|| ApiError::not_found(format!("no outcome {id}")))
}

/// The outcomes of a goal (or of all), in their order.
pub fn outcomes(sup: &Supervisor, goal: Option<&str>) -> Vec<Outcome> {
    let rows = match goal {
        Some(g) => sup.db().query(&format!("SELECT {OUTCOME_COLS} FROM outcomes WHERE goal = :goal LIMIT 10000"), &Params::new().set("goal", Value::Str(g.to_string()))),
        None => sup.db().query(&format!("SELECT {OUTCOME_COLS} FROM outcomes LIMIT 100000"), &Params::new()),
    };
    let mut v: Vec<Outcome> = rows.unwrap_or_default().iter().map(|r| Outcome::of(r)).collect();
    v.sort_by_key(|o| (o.goal.clone(), o.ord, o.created));
    v
}

/// The state an outcome's agent implies; `done` and `dropped` once set
/// stay. `asking`: the agents with an open question.
fn derived(o: &Outcome, a: Option<&Agent>, running: bool, asking: &HashSet<String>) -> String {
    if matches!(o.state.as_str(), "done" | "dropped") {
        return o.state.clone();
    }
    let Some(a) = a else {
        return if o.agent.is_none() { "next".into() } else { o.state.clone() };
    };
    if asking.contains(&a.id) || a.state == "waiting" {
        return "needs_you".into();
    }
    if a.state == "exited" || !running {
        // Ended: done when it said so (progress 100) and exited cleanly;
        // else someone has to look.
        return if a.exit_code == Some(0) && o.progress >= 100 { "done".into() } else { "needs_you".into() };
    }
    "active".into()
}

/// The outcomes of a goal (or of all) with their state brought up to
/// their agents', written back where it changed.
pub fn refresh(sup: &Supervisor, lv: &Levels, goal: Option<&str>) -> Vec<Outcome> {
    let _w = lv.write.lock().unwrap();
    let asking = crate::questions::asking(sup);
    let agents: HashMap<String, Agent> = sup.agent_rows().into_iter().map(|a| (a.id.clone(), a)).collect();
    let mut list = outcomes(sup, goal);
    for o in list.iter_mut() {
        let a = o.agent.as_deref().and_then(|id| agents.get(id));
        let running = a.is_some_and(|a| sup.is_running(&a.id));
        let s = derived(o, a, running, &asking);
        if s != o.state {
            o.state = s;
            if o.state == "done" {
                o.progress = 100;
            }
            o.updated = now_ms();
            let _ = o.put(sup);
        }
    }
    list
}

/// What `POST /api/goals` takes.
#[derive(Debug, Default, Deserialize)]
pub struct GoalBody {
    pub repo: Option<String>,
    pub text: Option<String>,
    pub plan: Option<bool>,
    pub model: Option<String>,
    pub worktree: Option<bool>,
}

/// The lead's first prompt.
pub fn lead_task(goal: &Goal) -> String {
    format!(
        "You lead the work on a goal the user set in Workbench, where the user watches every agent:\n\n\
GOAL ({id}): {text}\n\n\
You plan and coordinate; the agents you start do the building. Do this:\n\
1. Read enough of this repository to understand what the goal takes. Do not change code yet.\n\
2. Break the goal into 3 to 8 outcomes, each one a result the user can check, small enough for one agent, and as independent of the others as it can be. Add each with the workbench tool add_outcome: title (what will be true when it is done, under 10 words), detail (how to tell it is done), and after (the id of an outcome it must wait for, if any).\n\
3. For each outcome that waits for nothing, start an agent with spawn_agent, passing outcome (its id) and a task that stands alone: the agent does not see this conversation, so say what to do, where, how to tell it is done, and to report what it changed. Each agent works in its own git worktree and branch. Start the waiting outcomes when what they wait for is done.\n\
4. Follow the agents with wait_agent and read_agent. Answer their questions yourself with send_to_agent when you can. When only the user can decide, ask with ask_human (the question, 2 to 4 options, your recommendation): never ask in your terminal, the user is not watching it.\n\
5. Keep the outcomes true: update_outcome with progress (0 to 100) as they go, state \"done\" when an agent's work checks out, \"dropped\" when one no longer makes sense.\n\
6. When every outcome is done, say in a few sentences what was built and on which branches, and stop.",
        id = goal.id,
        text = goal.text.trim()
    )
}

/// A free name for a lead: `lead`, `lead-2`, ...
fn lead_name(sup: &Supervisor, repo: &std::path::Path) -> String {
    let running: HashSet<String> = sup.agent_rows().into_iter().filter(|a| sup.is_running(&a.id)).map(|a| a.name).collect();
    let mut i = 1;
    loop {
        let n = if i == 1 { "lead".to_string() } else { format!("lead-{i}") };
        if !running.contains(&n) && !crate::git::branch_exists(repo, &format!("wb/{n}")) {
            return n;
        }
        i += 1;
    }
}

/// `POST /api/goals`: a goal, and with `plan` its lead.
pub fn create(sup: &Arc<Supervisor>, lv: &Levels, b: GoalBody) -> Result<Json, ApiError> {
    let repo = trail::repo_dir(b.repo.as_deref())?;
    crate::git::checkout(&repo).map_err(ApiError::bad)?;
    let text = b.text.unwrap_or_default().trim().to_string();
    if text.is_empty() {
        return Err(ApiError::bad("text is empty: say what the goal is"));
    }
    let mut g = Goal { id: new_id("g"), repo: repo.to_string_lossy().to_string(), text, state: "active".into(), lead: None, created: now_ms(), done: None };
    g.put(sup)?;
    let mut lead = Json::Null;
    if b.plan.unwrap_or(false) {
        let req = SpawnRequest {
            repo: g.repo.clone(),
            task: Some(lead_task(&g)),
            name: Some(lead_name(sup, &repo)),
            worktree: b.worktree,
            model: b.model.clone(),
            goal: Some(g.id.clone()),
            ..SpawnRequest::default()
        };
        lead = spawn(sup, lv, req)?;
        g.lead = lead["id"].as_str().map(str::to_string);
        g.put(sup)?;
    }
    Ok(json!({ "goal": g.to_json(&[]), "lead": lead }))
}

/// `GET /api/goals`.
pub fn list(sup: &Supervisor, lv: &Levels, repo: Option<&str>) -> Result<Json, ApiError> {
    let repo = match repo.filter(|r| !r.is_empty()) {
        Some(r) => Some(trail::repo_dir(Some(r))?.to_string_lossy().to_string()),
        None => None,
    };
    let all = refresh(sup, lv, None);
    let list: Vec<Json> = goals(sup, repo.as_deref())
        .iter()
        .map(|g| {
            let os: Vec<Outcome> = all.iter().filter(|o| o.goal == g.id).cloned().collect();
            g.to_json(&os)
        })
        .collect();
    Ok(json!({ "goals": list }))
}

/// `GET /api/goals/:id`: the goal, its outcomes and agents, its open
/// questions, what ChloeCtx learned today, and its pulse.
pub fn detail(sup: &Supervisor, lv: &Levels, id: &str) -> Result<Json, ApiError> {
    let g = goal(sup, id)?;
    let os = refresh(sup, lv, Some(id));
    let mut agents: Vec<Agent> = sup.agent_rows().into_iter().filter(|a| a.goal.as_deref() == Some(id) || os.iter().any(|o| o.agent.as_deref() == Some(&a.id))).collect();
    agents.sort_by_key(|a| a.created);
    let ids: HashSet<String> = agents.iter().map(|a| a.id.clone()).collect();
    let open = crate::questions::all(sup).into_iter().filter(|q| q.state == "open" && q.agent.as_ref().is_some_and(|a| ids.contains(a))).count();
    let (learned, missing) = learned(sup, &g.repo, now_ms() / 1000 - 24 * 3600, 50);
    Ok(json!({
        "goal": g.to_json(&os),
        "outcomes": os.iter().map(|o| o.to_json(sup)).collect::<Vec<_>>(),
        "agents": agents.iter().filter_map(|a| sup.agent_json_of(&a.id)).collect::<Vec<_>>(),
        "open_questions": open,
        "learned": learned,
        // The map's collisions that involve this goal's agents.
        "heads_up": crate::map::collisions(sup, &g.repo).into_iter()
            .filter(|c| c["agents"].as_array().is_some_and(|xs| xs.iter().any(|x| x.as_str().is_some_and(|x| ids.contains(x)))))
            .collect::<Vec<_>>(),
        "pulse": pulse(sup, &ids, now_ms()),
        "missing": missing,
    }))
}

/// Events per minute of `agents` for the 60 minutes before `now`:
/// `[[minute_ms, count]]`, oldest first.
pub fn pulse(sup: &Supervisor, agents: &HashSet<String>, now: i64) -> Vec<[i64; 2]> {
    let end = now - now.rem_euclid(60_000) + 60_000;
    let start = end - 60 * 60_000;
    let mut counts = vec![0i64; 60];
    let p = Params::new().set("since", ts(start));
    for r in sup.db().query("SELECT agent, ts FROM events WHERE ts >= :since LIMIT 1000000", &p).unwrap_or_default() {
        if get_str(&r[0]).is_some_and(|a| agents.contains(&a)) {
            if let Some(t) = get_ms(&r[1]) {
                let i = (t - start) / 60_000;
                if (0..60).contains(&i) {
                    counts[i as usize] += 1;
                }
            }
        }
    }
    counts.iter().enumerate().map(|(i, c)| [start + i as i64 * 60_000, *c]).collect()
}

/// ChloeCtx's decision, preference, fact and error-fix records of a
/// repository since `since_s` (seconds), newest first, with their agents;
/// and what could not be found.
pub fn learned(sup: &Supervisor, repo: &str, since_s: i64, limit: usize) -> (Vec<Json>, Vec<String>) {
    let sql = format!(
        "SELECT id, kind, tag, ts, session, text, files FROM memory WHERE ts >= {since_s} AND (kind = 'decision' OR kind = 'preference' OR kind = 'fact' OR (kind = 'error' AND tag = 'fix')) ORDER BY ts DESC LIMIT {limit}"
    );
    let dir = PathBuf::from(repo);
    let out = match trail::run_chloectx(&sup.cfg.chloectx_bin, &dir, &["sql", "--json", &sql], Duration::from_secs(30)) {
        Ran::Out(v) => v,
        Ran::Missing => return (Vec::new(), vec![trail::not_installed(&sup.cfg.chloectx_bin)]),
        Ran::Failed(e) => return (Vec::new(), vec![e]),
    };
    let cols: Vec<String> = out["columns"].as_array().into_iter().flatten().map(|c| c.as_str().or_else(|| c["name"].as_str()).unwrap_or("").to_string()).collect();
    let col = |row: &Vec<Json>, name: &str| cols.iter().position(|c| c == name).and_then(|i| row.get(i).cloned()).unwrap_or(Json::Null);
    let s = trail::Sessions::load(sup);
    let mut list = Vec::new();
    for row in out["rows"].as_array().into_iter().flatten().filter_map(Json::as_array) {
        let kind = col(row, "kind");
        let tag = col(row, "tag");
        let kind = if kind == "error" && tag == "fix" { json!("fix") } else { kind };
        let session = col(row, "session");
        list.push(json!({
            "id": col(row, "id"), "kind": kind, "text": col(row, "text"),
            "ts": trail::ctx_time(&col(row, "ts")), "ts_ms": trail::ctx_ms(&col(row, "ts")),
            "session": session,
            "agent": session.as_str().and_then(|sid| s.agent(sid)).map(|a| trail::agent_json(sup, a)),
            "files": trail::files_of(&col(row, "files")),
        }));
    }
    (list, Vec::new())
}

/// What `PATCH /api/goals/:id` takes.
#[derive(Debug, Default, Deserialize)]
pub struct GoalPatch {
    pub text: Option<String>,
    pub state: Option<String>,
}

pub fn patch(sup: &Supervisor, lv: &Levels, id: &str, b: GoalPatch) -> Result<Json, ApiError> {
    let _w = lv.write.lock().unwrap();
    let mut g = goal(sup, id)?;
    if let Some(t) = b.text.filter(|t| !t.trim().is_empty()) {
        g.text = t.trim().to_string();
    }
    if let Some(s) = b.state {
        if !GOAL_STATES.contains(&s.as_str()) {
            return Err(ApiError::bad(format!("state is one of {}", GOAL_STATES.join(", "))));
        }
        g.done = if s == "active" { None } else { g.done.or(Some(now_ms())) };
        g.state = s;
    }
    g.put(sup)?;
    let os = outcomes(sup, Some(id));
    Ok(json!({ "goal": g.to_json(&os) }))
}

/// What `POST /api/outcomes` takes.
#[derive(Debug, Default, Deserialize)]
pub struct OutcomeBody {
    pub goal: Option<String>,
    pub title: Option<String>,
    pub detail: Option<String>,
    pub after: Option<String>,
}

/// `POST /api/outcomes`; `by` (the calling agent) stands for its goal when
/// none is given.
pub fn add_outcome(sup: &Supervisor, lv: &Levels, b: OutcomeBody, by: Option<&str>) -> Result<Json, ApiError> {
    let goal_id = match b.goal.filter(|g| !g.is_empty()) {
        Some(g) => g,
        None => by
            .and_then(|a| sup.agent_row(a))
            .and_then(|a| a.goal)
            .ok_or_else(|| ApiError::bad("goal is required (an agent's own goal stands for it when it has one)"))?,
    };
    let g = goal(sup, &goal_id)?;
    let title = b.title.unwrap_or_default().trim().to_string();
    if title.is_empty() || title.len() > 300 {
        return Err(ApiError::bad("title is required, at most 300 bytes"));
    }
    let _w = lv.write.lock().unwrap();
    let existing = outcomes(sup, Some(&g.id));
    if let Some(after) = b.after.as_deref().filter(|a| !a.is_empty()) {
        if !existing.iter().any(|o| o.id == after) {
            return Err(ApiError::bad(format!("after: goal {} has no outcome {after}", g.id)));
        }
    }
    let now = now_ms();
    let o = Outcome {
        id: new_id("o"),
        goal: g.id.clone(),
        title,
        detail: b.detail.filter(|d| !d.trim().is_empty()),
        state: "next".into(),
        agent: None,
        ord: existing.iter().map(|o| o.ord).max().unwrap_or(0) + 1,
        after: b.after.filter(|a| !a.is_empty()),
        progress: 0,
        created: now,
        updated: now,
    };
    o.put(sup)?;
    Ok(json!({ "outcome": o.to_json(sup) }))
}

/// What `PATCH /api/outcomes/:id` takes.
#[derive(Debug, Default, Deserialize)]
pub struct OutcomePatch {
    pub title: Option<String>,
    pub detail: Option<String>,
    pub state: Option<String>,
    pub progress: Option<i64>,
    pub agent: Option<String>,
}

pub fn patch_outcome(sup: &Supervisor, lv: &Levels, id: &str, b: OutcomePatch) -> Result<Json, ApiError> {
    {
        let _w = lv.write.lock().unwrap();
        let mut o = outcome(sup, id)?;
        if let Some(t) = b.title.filter(|t| !t.trim().is_empty()) {
            o.title = t.trim().to_string();
        }
        if let Some(d) = b.detail {
            o.detail = Some(d).filter(|d| !d.trim().is_empty());
        }
        if let Some(p) = b.progress {
            o.progress = p.clamp(0, 100);
        }
        if let Some(a) = b.agent.filter(|a| !a.is_empty()) {
            if sup.agent_row(&a).is_none() {
                return Err(ApiError::bad(format!("no agent {a}")));
            }
            o.agent = Some(a);
        }
        if let Some(s) = b.state {
            if !OUTCOME_STATES.contains(&s.as_str()) {
                return Err(ApiError::bad(format!("state is one of {}", OUTCOME_STATES.join(", "))));
            }
            if s == "done" {
                o.progress = 100;
            }
            o.state = s;
        }
        o.updated = now_ms();
        o.put(sup)?;
    }
    let goal_id = outcome(sup, id)?.goal;
    let o = refresh(sup, lv, Some(&goal_id)).into_iter().find(|o| o.id == id).ok_or_else(|| ApiError::not_found(format!("no outcome {id}")))?;
    Ok(json!({ "outcome": o.to_json(sup) }))
}

/// Starts an agent (`POST /api/agents`) with its goal and outcome: an
/// outcome's agent works toward its goal and the outcome becomes active;
/// an agent started by an agent with a goal works toward that goal too.
pub fn spawn(sup: &Arc<Supervisor>, lv: &Levels, mut req: SpawnRequest) -> Result<Json, ApiError> {
    req.outcome = req.outcome.filter(|o| !o.is_empty());
    req.goal = req.goal.filter(|g| !g.is_empty());
    let o = match req.outcome.as_deref() {
        Some(oid) => {
            let o = outcome(sup, oid).map_err(|_| ApiError::bad(format!("no outcome {oid}")))?;
            if let Some(a) = o.agent.as_deref().filter(|a| sup.is_running(a)) {
                return Err(ApiError::conflict(format!("outcome {oid} has a running agent, {a}")));
            }
            req.goal = Some(o.goal.clone());
            Some(o)
        }
        None => None,
    };
    if req.goal.is_none() {
        req.goal = req.parent.as_deref().and_then(|p| sup.agent_row(p)).and_then(|p| p.goal);
    }
    if let Some(g) = req.goal.as_deref() {
        goal(sup, g).map_err(|_| ApiError::bad(format!("no goal {g}")))?;
    }
    let agent = sup.spawn(req)?;
    if let Some(mut o) = o {
        let _w = lv.write.lock().unwrap();
        o.agent = agent["id"].as_str().map(str::to_string);
        if matches!(o.state.as_str(), "next" | "needs_you") {
            o.state = "active".into();
        }
        o.updated = now_ms();
        o.put(sup)?;
    }
    Ok(agent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_outcome_follows_its_agent() {
        let o = Outcome { agent: Some("a1".into()), state: "active".into(), ..Outcome::default() };
        let mut a = Agent { id: "a1".into(), state: "working".into(), ..Agent::default() };
        let none = HashSet::new();
        assert_eq!(derived(&o, Some(&a), true, &none), "active");
        a.state = "waiting".into();
        assert_eq!(derived(&o, Some(&a), true, &none), "needs_you");
        a.state = "idle".into();
        assert_eq!(derived(&o, Some(&a), true, &["a1".to_string()].into()), "needs_you");
        a.state = "exited".into();
        a.exit_code = Some(0);
        assert_eq!(derived(&o, Some(&a), false, &none), "needs_you");
        let o2 = Outcome { progress: 100, ..o.clone() };
        assert_eq!(derived(&o2, Some(&a), false, &none), "done");
        let o3 = Outcome { state: "dropped".into(), ..o.clone() };
        assert_eq!(derived(&o3, Some(&a), true, &none), "dropped");
        let o4 = Outcome { agent: None, state: "next".into(), ..Outcome::default() };
        assert_eq!(derived(&o4, None, false, &none), "next");
    }
}
