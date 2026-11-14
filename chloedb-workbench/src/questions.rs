//! Questions and rules (Decide): what agents ask you (`ask_human`, and
//! permission prompts), what ChloeCtx found about each when it was asked
//! (precedent), your answers, and the rules made from them, which every
//! agent of the repository is then told.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chloedb::query::Params;
use chloedb::Value;
use serde::Deserialize;
use serde_json::{json, Value as Json};

use crate::db::{get_int, get_ms, get_str, iso, now_ms, opt_int, opt_str, ts};
use crate::levels::{new_id, Levels};
use crate::supervisor::{one_line, ApiError, Supervisor};
use crate::trail::{self, Ran};

/// The options of a permission prompt, as Claude Code numbers them.
pub const PERMISSION_OPTIONS: [&str; 3] = ["Yes", "Yes, and don't ask again", "No"];

#[derive(Clone, Debug, Default)]
pub struct Question {
    pub id: String,
    pub agent: Option<String>,
    pub outcome: Option<String>,
    pub repo: Option<String>,
    /// `ask` (from ask_human) or `permission`.
    pub kind: String,
    pub text: String,
    pub context: Option<String>,
    /// `[{label, detail?, matches_precedent}]`.
    pub options: Json,
    pub recommended: Option<i64>,
    pub precedent: Option<Json>,
    /// `open`, `answered` or `expired`.
    pub state: String,
    pub answer: Option<String>,
    /// `you`, `rule:<id>` or `terminal`.
    pub answered_by: Option<String>,
    pub rule: Option<String>,
    pub created: i64,
    pub answered: Option<i64>,
}

const Q_COLS: &str = "id, agent, outcome, repo, kind, text, context, options, recommended, precedent, state, answer, answered_by, rule, created, answered";

impl Question {
    fn of(r: &[Value]) -> Question {
        let js = |v: &Value| get_str(v).and_then(|s| serde_json::from_str::<Json>(&s).ok());
        Question {
            id: get_str(&r[0]).unwrap_or_default(),
            agent: get_str(&r[1]),
            outcome: get_str(&r[2]),
            repo: get_str(&r[3]),
            kind: get_str(&r[4]).unwrap_or_default(),
            text: get_str(&r[5]).unwrap_or_default(),
            context: get_str(&r[6]),
            options: js(&r[7]).unwrap_or(json!([])),
            recommended: get_int(&r[8]),
            precedent: js(&r[9]),
            state: get_str(&r[10]).unwrap_or_default(),
            answer: get_str(&r[11]),
            answered_by: get_str(&r[12]),
            rule: get_str(&r[13]),
            created: get_ms(&r[14]).unwrap_or(0),
            answered: get_ms(&r[15]),
        }
    }

    pub fn put(&self, sup: &Supervisor) -> Result<(), ApiError> {
        let mut precedent = self.precedent.as_ref().map(|p| p.to_string());
        if precedent.as_ref().is_some_and(|p| p.len() > 64 * 1024) {
            precedent = Some(json!({ "text": "", "ids": [], "agents": {}, "missing": ["what ChloeCtx found was too long to keep"] }).to_string());
        }
        let p = Params::new()
            .set("id", Value::Str(self.id.clone()))
            .set("agent", opt_str(&self.agent))
            .set("outcome", opt_str(&self.outcome))
            .set("repo", opt_str(&self.repo))
            .set("kind", Value::Str(self.kind.clone()))
            .set("text", Value::Str(self.text.clone()))
            .set("context", opt_str(&self.context))
            .set("options", Value::Str(self.options.to_string()))
            .set("recommended", opt_int(self.recommended))
            .set("precedent", opt_str(&precedent))
            .set("state", Value::Str(self.state.clone()))
            .set("answer", opt_str(&self.answer))
            .set("answered_by", opt_str(&self.answered_by))
            .set("rule", opt_str(&self.rule))
            .set("created", ts(self.created))
            .set("answered", self.answered.map_or(Value::Null, ts));
        let names: Vec<String> = Q_COLS.split(", ").map(|c| format!(":{c}")).collect();
        sup.db().query(&format!("INSERT OR REPLACE INTO questions ({Q_COLS}) VALUES ({})", names.join(", ")), &p).map_err(ApiError::internal)?;
        Ok(())
    }

    /// The row as JSON, with its agent's name.
    pub fn to_json(&self, sup: &Supervisor) -> Json {
        let a = self.agent.as_deref().and_then(|id| sup.agent_row(id));
        json!({
            "id": self.id, "agent": self.agent, "agent_name": a.map(|a| a.name), "outcome": self.outcome, "repo": self.repo,
            "kind": self.kind, "text": self.text, "context": self.context, "options": self.options, "recommended": self.recommended,
            "precedent": self.precedent, "state": self.state, "answer": self.answer, "answered_by": self.answered_by, "rule": self.rule,
            "created": iso(self.created), "created_ms": self.created, "answered": self.answered.map(iso), "answered_ms": self.answered,
        })
    }

    fn labels(&self) -> Vec<String> {
        self.options.as_array().into_iter().flatten().map(|o| o["label"].as_str().unwrap_or("").to_string()).collect()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Rule {
    pub id: String,
    pub repo: String,
    pub text: String,
    pub question: Option<String>,
    pub ctx_id: Option<String>,
    pub active: bool,
    pub created: i64,
}

const R_COLS: &str = "id, repo, text, question, ctx_id, active, created";

impl Rule {
    fn of(r: &[Value]) -> Rule {
        Rule {
            id: get_str(&r[0]).unwrap_or_default(),
            repo: get_str(&r[1]).unwrap_or_default(),
            text: get_str(&r[2]).unwrap_or_default(),
            question: get_str(&r[3]),
            ctx_id: get_str(&r[4]),
            active: get_int(&r[5]).unwrap_or(0) != 0,
            created: get_ms(&r[6]).unwrap_or(0),
        }
    }

    fn put(&self, sup: &Supervisor) -> Result<(), ApiError> {
        let p = Params::new()
            .set("id", Value::Str(self.id.clone()))
            .set("repo", Value::Str(self.repo.clone()))
            .set("text", Value::Str(self.text.clone()))
            .set("question", opt_str(&self.question))
            .set("ctx_id", opt_str(&self.ctx_id))
            .set("active", Value::Bool(self.active))
            .set("created", ts(self.created));
        sup.db().query(&format!("INSERT OR REPLACE INTO rules ({R_COLS}) VALUES (:id, :repo, :text, :question, :ctx_id, :active, :created)"), &p).map_err(ApiError::internal)?;
        Ok(())
    }

    pub fn to_json(&self) -> Json {
        json!({ "id": self.id, "repo": self.repo, "text": self.text, "question": self.question, "ctx_id": self.ctx_id, "active": self.active, "created": iso(self.created), "created_ms": self.created })
    }
}

pub fn get(sup: &Supervisor, id: &str) -> Result<Question, ApiError> {
    let p = Params::new().set("id", Value::Str(id.to_string()));
    let rows = sup.db().query(&format!("SELECT {Q_COLS} FROM questions WHERE id = :id LIMIT 1"), &p).map_err(ApiError::internal)?;
    rows.first().map(|r| Question::of(r)).ok_or_else(|| ApiError::not_found(format!("no question {id}")))
}

/// Every question, newest first.
pub fn all(sup: &Supervisor) -> Vec<Question> {
    let rows = sup.db().query(&format!("SELECT {Q_COLS} FROM questions ORDER BY created DESC LIMIT 100000"), &Params::new()).unwrap_or_default();
    rows.iter().map(|r| Question::of(r)).collect()
}

fn open_of(sup: &Supervisor, agent: &str) -> Vec<Question> {
    let p = Params::new().set("agent", Value::Str(agent.to_string()));
    let rows = sup.db().query(&format!("SELECT {Q_COLS} FROM questions WHERE agent = :agent AND state = 'open' LIMIT 1000"), &p).unwrap_or_default();
    rows.iter().map(|r| Question::of(r)).collect()
}

/// The agents with an open question.
pub fn asking(sup: &Supervisor) -> HashSet<String> {
    let rows = sup.db().query("SELECT agent FROM questions WHERE state = 'open' LIMIT 100000", &Params::new()).unwrap_or_default();
    rows.iter().filter_map(|r| get_str(&r[0])).collect()
}

/// The rules of a repository (or all), oldest first; only active ones
/// with `active_only`.
pub fn rules(sup: &Supervisor, repo: Option<&str>, active_only: bool) -> Vec<Rule> {
    let rows = match repo {
        Some(r) => sup.db().query(&format!("SELECT {R_COLS} FROM rules WHERE repo = :repo ORDER BY created ASC LIMIT 10000"), &Params::new().set("repo", Value::Str(r.to_string()))),
        None => sup.db().query(&format!("SELECT {R_COLS} FROM rules ORDER BY created ASC LIMIT 10000"), &Params::new()),
    };
    rows.unwrap_or_default().iter().map(|r| Rule::of(r)).filter(|r| !active_only || r.active).collect()
}

/// Open questions whose agent no longer runs expire.
pub fn expire_stale(sup: &Supervisor, lv: &Levels) {
    let _w = lv.write.lock().unwrap();
    let rows = sup.db().query(&format!("SELECT {Q_COLS} FROM questions WHERE state = 'open' LIMIT 100000"), &Params::new()).unwrap_or_default();
    for mut q in rows.iter().map(|r| Question::of(r)) {
        if q.agent.as_deref().is_some_and(|a| !sup.is_running(a)) {
            q.state = "expired".into();
            q.answered = Some(now_ms());
            if q.put(sup).is_ok() {
                if let Some(a) = &q.agent {
                    let mut open = lv.open_perm.lock().unwrap();
                    if open.get(a) == Some(&q.id) {
                        open.remove(a);
                    }
                }
                lv.changed(&q.id);
            }
        }
    }
}

/// `GET /api/questions`.
pub fn list(sup: &Supervisor, lv: &Levels, repo: Option<&str>, state: Option<&str>, agent: Option<&str>) -> Result<Json, ApiError> {
    expire_stale(sup, lv);
    let repo = match repo.filter(|r| !r.is_empty()) {
        Some(r) => Some(trail::repo_dir(Some(r))?.to_string_lossy().to_string()),
        None => None,
    };
    let qs: Vec<Json> = all(sup)
        .into_iter()
        .filter(|q| repo.as_ref().is_none_or(|r| q.repo.as_ref() == Some(r)))
        .filter(|q| state.filter(|s| !s.is_empty()).is_none_or(|s| q.state == s))
        .filter(|q| agent.filter(|s| !s.is_empty()).is_none_or(|a| q.agent.as_deref() == Some(a)))
        .map(|q| q.to_json(sup))
        .collect();
    Ok(json!({ "questions": qs }))
}

/// Words of a label or text: lower case, three letters or more, not the
/// commonest words.
fn words(s: &str) -> Vec<String> {
    const COMMON: [&str; 24] = ["the", "and", "for", "with", "not", "but", "you", "your", "this", "that", "use", "one", "all", "any", "are", "was", "yes", "its", "it's", "from", "into", "then", "than", "per"];
    s.to_lowercase().split(|c: char| !c.is_alphanumeric() && c != '\'').filter(|w| w.chars().count() >= 3 && !COMMON.contains(w)).map(str::to_string).collect()
}

/// Whether `text` holds every word of `label` (at least one).
pub fn label_matches(label: &str, text: &str) -> bool {
    let want = words(label);
    let have: HashSet<String> = words(text).into_iter().collect();
    !want.is_empty() && want.iter().all(|w| have.contains(w))
}

/// A question's text for comparing: lower case, words only.
fn norm(s: &str) -> String {
    s.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")
}

/// What `POST /api/questions` (ask_human) takes.
#[derive(Debug, Default, Deserialize)]
pub struct AskBody {
    pub agent: Option<String>,
    #[serde(alias = "text")]
    pub question: Option<String>,
    #[serde(default)]
    pub options: Vec<Json>,
    pub recommended: Option<i64>,
    pub context: Option<String>,
}

fn option_of(v: &Json) -> Option<Json> {
    match v {
        Json::String(s) if !s.trim().is_empty() => Some(json!({ "label": s.trim(), "matches_precedent": false })),
        Json::Object(o) => {
            let label = o.get("label").and_then(Json::as_str).map(str::trim).filter(|l| !l.is_empty())?;
            let mut out = json!({ "label": label, "matches_precedent": false });
            if let Some(d) = o.get("detail").and_then(Json::as_str).filter(|d| !d.trim().is_empty()) {
                out["detail"] = json!(d.trim());
            }
            Some(out)
        }
        _ => None,
    }
}

/// `POST /api/questions`: an agent asks you. Its state is `waiting` until
/// you answer; when a rule made from the same question answers it, it is
/// answered at once.
pub fn create_ask(sup: &Arc<Supervisor>, lv: &Arc<Levels>, agent: &str, b: AskBody) -> Result<Json, ApiError> {
    let a = sup.agent_row(agent).ok_or_else(|| ApiError::not_found(format!("no agent {agent}")))?;
    let text = b.question.unwrap_or_default().trim().to_string();
    if text.is_empty() || text.len() > 4000 {
        return Err(ApiError::bad("question is required, at most 4000 bytes"));
    }
    if b.options.len() > 8 {
        return Err(ApiError::bad("at most 8 options"));
    }
    let options: Vec<Json> = b.options.iter().map(|o| option_of(o).ok_or_else(|| ApiError::bad("each option is a label, or {label, detail?}"))).collect::<Result<_, _>>()?;
    if let Some(r) = b.recommended {
        if r < 0 || r as usize >= options.len() {
            return Err(ApiError::bad("recommended is the index of an option, from 0"));
        }
    }
    let mut q = Question {
        id: new_id("q"),
        agent: Some(a.id.clone()),
        outcome: a.outcome.clone(),
        repo: Some(a.repo.clone()),
        kind: "ask".into(),
        text,
        context: b.context.filter(|c| !c.trim().is_empty()),
        options: Json::Array(options),
        recommended: b.recommended,
        precedent: None,
        state: "open".into(),
        created: now_ms(),
        ..Question::default()
    };
    // A rule made from your answer to the same question answers it.
    if let Some((rule, answer)) = rule_answer(sup, &q) {
        q.state = "answered".into();
        q.answer = Some(answer.clone());
        q.answered_by = Some(format!("rule:{}", rule.id));
        q.rule = Some(rule.id.clone());
        q.answered = Some(q.created);
        q.put(sup)?;
        let _ = sup.note(&a.id, "answered", format!("answered by a rule: {}", one_line(&answer, 200)), Some(json!({ "question": q.id, "by": format!("rule:{}", rule.id) }).to_string()), |_| {});
        lv.changed(&q.id);
        return Ok(json!({ "question": q.to_json(sup) }));
    }
    q.put(sup)?;
    let waiting = one_line(&q.text, 500);
    sup.note(&a.id, "question", format!("question: {}", one_line(&q.text, 200)), Some(json!({ "question": q.id }).to_string()), |ag| {
        Supervisor::set_agent_state(ag, "waiting");
        ag.waiting_for = Some(waiting.clone());
        ag.narration = Some(one_line(&waiting, crate::narrate::MAX_CHARS));
        ag.narration_ts = Some(now_ms());
    })?;
    lv.changed(&q.id);
    find_precedent_later(sup, lv, &q.id);
    Ok(json!({ "question": q.to_json(sup) }))
}

/// The active rule made from an answer to the same question, and that
/// answer, when it fits the new question's options.
fn rule_answer(sup: &Supervisor, q: &Question) -> Option<(Rule, String)> {
    let repo = q.repo.as_deref()?;
    let want = norm(&q.text);
    let labels = q.labels();
    for r in rules(sup, Some(repo), true).into_iter().rev() {
        let Some(from) = r.question.as_deref().and_then(|id| get(sup, id).ok()) else { continue };
        if from.kind != "ask" || norm(&from.text) != want {
            continue;
        }
        let Some(ans) = from.answer.clone() else { continue };
        let first = ans.lines().next().unwrap_or("").to_string();
        if labels.is_empty() || labels.iter().any(|l| l == &first) {
            return Some((r, ans));
        }
    }
    None
}

/// Whether a Notification asks permission to use a tool.
pub fn is_permission(hook: &Json) -> bool {
    match hook.get("notification_type").and_then(Json::as_str) {
        Some(t) => t == "permission_prompt",
        None => hook.get("message").and_then(Json::as_str).is_some_and(|m| m.to_lowercase().contains("permission")),
    }
}

/// A permission prompt becomes a question (one open at a time per agent).
pub fn create_permission(sup: &Arc<Supervisor>, lv: &Arc<Levels>, agent: &str, hook: &Json, context: Option<String>) {
    if lv.open_perm.lock().unwrap().contains_key(agent) {
        return;
    }
    let Some(a) = sup.agent_row(agent) else { return };
    let text = one_line(hook.get("message").and_then(Json::as_str).unwrap_or("Claude needs your permission"), 500);
    let q = Question {
        id: new_id("q"),
        agent: Some(a.id.clone()),
        outcome: a.outcome.clone(),
        repo: Some(a.repo.clone()),
        kind: "permission".into(),
        text,
        context,
        options: Json::Array(PERMISSION_OPTIONS.iter().map(|l| json!({ "label": l, "matches_precedent": false })).collect()),
        recommended: None,
        state: "open".into(),
        created: now_ms(),
        ..Question::default()
    };
    if q.put(sup).is_err() {
        return;
    }
    lv.open_perm.lock().unwrap().insert(a.id.clone(), q.id.clone());
    // Its Notification event stands for it: no `question` event.
    let _ = sup.update_agent(&a.id, |ag| {
        ag.narration = Some(one_line(&q.text, crate::narrate::MAX_CHARS));
        ag.narration_ts = Some(now_ms());
    });
    lv.changed(&q.id);
    find_precedent_later(sup, lv, &q.id);
}

/// The agent did something after its permission prompt: it was answered
/// in its terminal.
pub fn answered_in_terminal(sup: &Supervisor, lv: &Levels, agent: &str) {
    let Some(qid) = lv.open_perm.lock().unwrap().remove(agent) else { return };
    let _w = lv.write.lock().unwrap();
    if let Ok(mut q) = get(sup, &qid) {
        if q.state == "open" {
            q.state = "answered".into();
            q.answered_by = Some("terminal".into());
            q.answer = Some("answered in the terminal".into());
            q.answered = Some(now_ms());
            let _ = q.put(sup);
            lv.changed(&q.id);
        }
    }
}

/// `ask_human` returned without an answer (it timed out): its question
/// expires.
pub fn expire_asks_of(sup: &Supervisor, lv: &Levels, agent: &str) {
    let _w = lv.write.lock().unwrap();
    for mut q in open_of(sup, agent).into_iter().filter(|q| q.kind == "ask") {
        q.state = "expired".into();
        q.answered = Some(now_ms());
        let _ = q.put(sup);
        lv.changed(&q.id);
    }
}

/// Asks ChloeCtx about the question off the request path, and marks the
/// options its answer or a rule of the repository speaks for.
fn find_precedent_later(sup: &Arc<Supervisor>, lv: &Arc<Levels>, qid: &str) {
    let (sup, lv, qid) = (sup.clone(), lv.clone(), qid.to_string());
    let _ = std::thread::Builder::new().name(format!("precedent-{qid}")).spawn(move || {
        let Ok(q) = get(&sup, &qid) else { return };
        let Some(repo) = q.repo.clone() else { return };
        let ask = if q.kind == "permission" { format!("{} {}", q.text, q.context.as_deref().unwrap_or("")) } else { q.text.clone() };
        let mut p = trail::ask(&sup, Some(&repo), &ask).unwrap_or_else(|e| json!({ "text": "", "ids": [], "agents": {}, "missing": [e.message] }));
        let rs = rules(&sup, Some(&repo), true);
        let _w = lv.write.lock().unwrap();
        let Ok(mut q) = get(&sup, &qid) else { return };
        let text = p["text"].as_str().unwrap_or("").to_string();
        let mut matched_rules = Vec::new();
        if q.kind == "ask" {
            if let Some(opts) = q.options.as_array_mut() {
                for o in opts.iter_mut() {
                    let label = o["label"].as_str().unwrap_or("").to_string();
                    let by_rule: Vec<&Rule> = rs.iter().filter(|r| label_matches(&label, &r.text)).collect();
                    let by_record = text.lines().any(|l| label_matches(&label, l));
                    o["matches_precedent"] = json!(!by_rule.is_empty() || by_record);
                    matched_rules.extend(by_rule.into_iter().map(|r| json!({ "id": r.id, "text": r.text, "option": label })));
                }
            }
        }
        p["rules"] = Json::Array(matched_rules);
        q.precedent = Some(p);
        if q.put(&sup).is_ok() {
            lv.changed(&q.id);
        }
    });
}

/// What `POST /api/questions/:id/answer` takes: `option` (from 0), `text`
/// (said with it, or instead), `rule` (a rule made from the answer).
#[derive(Debug, Default, Deserialize)]
pub struct AnswerBody {
    pub option: Option<usize>,
    pub text: Option<String>,
    pub rule: Option<String>,
}

/// `POST /api/questions/:id/answer`.
pub fn answer(sup: &Arc<Supervisor>, lv: &Arc<Levels>, id: &str, b: AnswerBody) -> Result<Json, ApiError> {
    let text = b.text.as_deref().map(str::trim).filter(|t| !t.is_empty()).map(str::to_string);
    let q = {
        let _w = lv.write.lock().unwrap();
        let mut q = get(sup, id)?;
        if q.state != "open" {
            return Err(ApiError::conflict(format!("question {id} is {}{}", q.state, q.answered_by.as_deref().map(|b| format!(" (by {b})")).unwrap_or_default())));
        }
        let labels = q.labels();
        let label = match b.option {
            Some(n) => Some(labels.get(n).cloned().ok_or_else(|| ApiError::bad(format!("option is an index from 0 to {}", labels.len().saturating_sub(1))))?),
            None => None,
        };
        if q.kind == "permission" && label.is_none() {
            return Err(ApiError::bad("a permission question is answered with an option: 0 yes, 1 yes and don't ask again, 2 no"));
        }
        let answer = match (&label, &text) {
            (Some(l), Some(t)) => format!("{l}\n{t}"),
            (Some(l), None) => l.clone(),
            (None, Some(t)) => t.clone(),
            (None, None) => return Err(ApiError::bad("option or text is required")),
        };
        q.state = "answered".into();
        q.answer = Some(answer);
        q.answered_by = Some("you".into());
        q.answered = Some(now_ms());
        q.put(sup)?;
        q
    };
    let agent = q.agent.clone().unwrap_or_default();
    if q.kind == "permission" {
        lv.open_perm.lock().unwrap().remove(&agent);
        let key = (b.option.unwrap_or(0) + 1).to_string();
        if let Err(e) = sup.input(&agent, &key, true, None) {
            // Not typed: the question stays open.
            let _w = lv.write.lock().unwrap();
            let mut back = q.clone();
            back.state = "open".into();
            back.answer = None;
            back.answered_by = None;
            back.answered = None;
            let _ = back.put(sup);
            lv.open_perm.lock().unwrap().insert(agent.clone(), back.id.clone());
            return Err(e);
        }
    } else {
        let _ = sup.note(&agent, "answered", format!("answered: {}", one_line(q.answer.as_deref().unwrap_or(""), 200)), Some(json!({ "question": q.id, "by": "you" }).to_string()), |a| {
            if a.state == "waiting" {
                Supervisor::set_agent_state(a, "working");
            }
        });
    }
    let mut q = q;
    let mut missing = Vec::new();
    let mut rule_json = Json::Null;
    if let Some(r) = b.rule.as_deref().map(str::trim).filter(|r| !r.is_empty()) {
        let repo = q.repo.clone().unwrap_or_default();
        let (rule, m) = add_rule(sup, &repo, r, Some(&q.id));
        missing = m;
        rule_json = rule.to_json();
        let _w = lv.write.lock().unwrap();
        q.rule = Some(rule.id.clone());
        q.put(sup)?;
    }
    lv.changed(&q.id);
    Ok(json!({ "question": q.to_json(sup), "rule": rule_json, "missing": missing }))
}

/// A rule of a repository, kept in ChloeCtx too (`chloectx remember
/// decision`) when it is installed; what could not be done is said.
pub fn add_rule(sup: &Supervisor, repo: &str, text: &str, question: Option<&str>) -> (Rule, Vec<String>) {
    let mut missing = Vec::new();
    let dir = PathBuf::from(repo);
    let ctx_id = match trail::run_raw(&sup.cfg.chloectx_bin, &dir, &["remember", "decision", text], Duration::from_secs(30)) {
        Ran::Out(Json::String(out)) => stored_id(&out),
        Ran::Out(_) => None,
        Ran::Missing => {
            missing.push(trail::not_installed(&sup.cfg.chloectx_bin) + "; the rule is kept in Workbench only.");
            None
        }
        Ran::Failed(e) => {
            missing.push(format!("{e}; the rule is kept in Workbench only."));
            None
        }
    };
    let r = Rule { id: new_id("r"), repo: repo.to_string(), text: text.to_string(), question: question.map(str::to_string), ctx_id, active: true, created: now_ms() };
    if let Err(e) = r.put(sup) {
        missing.push(e.message);
    }
    (r, missing)
}

/// The record id in what `chloectx remember` prints: "Stored decision as <id>."
fn stored_id(out: &str) -> Option<String> {
    let line = out.lines().find(|l| l.contains(" as "))?;
    let id = line.rsplit_once(" as ")?.1.trim().trim_end_matches('.');
    (!id.is_empty() && !id.contains(' ')).then(|| id.to_string())
}

/// What `PATCH /api/rules/:id` takes.
#[derive(Debug, Default, Deserialize)]
pub struct RulePatch {
    pub active: Option<bool>,
    pub text: Option<String>,
}

pub fn patch_rule(sup: &Supervisor, lv: &Levels, id: &str, b: RulePatch) -> Result<Json, ApiError> {
    let _w = lv.write.lock().unwrap();
    let p = Params::new().set("id", Value::Str(id.to_string()));
    let rows = sup.db().query(&format!("SELECT {R_COLS} FROM rules WHERE id = :id LIMIT 1"), &p).map_err(ApiError::internal)?;
    let mut r = rows.first().map(|r| Rule::of(r)).ok_or_else(|| ApiError::not_found(format!("no rule {id}")))?;
    if let Some(a) = b.active {
        r.active = a;
    }
    if let Some(t) = b.text.filter(|t| !t.trim().is_empty()) {
        r.text = t.trim().to_string();
    }
    r.put(sup)?;
    Ok(json!({ "rule": r.to_json() }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_match_words_and_ids_are_read() {
        assert!(label_matches("Round half up", "Totals round half up, always."));
        assert!(!label_matches("Round half even", "Totals round half up"));
        assert!(!label_matches("Yes", "yes we do"), "too common a word to match on");
        assert_eq!(stored_id("Stored decision as 1c2e9a0b:58.\n"), Some("1c2e9a0b:58".into()));
        assert_eq!(stored_id("nothing"), None);
        assert_eq!(norm("Round half UP?"), "round half up");
        assert!(is_permission(&json!({ "notification_type": "permission_prompt", "message": "x" })));
        assert!(!is_permission(&json!({ "notification_type": "idle_prompt", "message": "permission" })));
        assert!(is_permission(&json!({ "message": "Claude needs your permission to use Bash" })));
    }
}
