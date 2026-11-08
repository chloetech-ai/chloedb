//! The on-call agent (`POST /v1/admin/oncall`): a Claude model looks into
//! a firing alert, or a question about the cluster, with the read-only
//! views an operator has (health, alerts, events, queries, logs, load,
//! metrics, contain rules), and answers with what it found and what it
//! would do. It changes nothing: each action it would take is a
//! proposal, kept here, that an admin approves or rejects
//! (`/v1/admin/oncall/proposals`).
//!
//! Every tool call is a request to this server's own route, made with the
//! caller's credential through the same checks and audit as any other
//! (the router copy [`crate::mcp`] calls too), named `oncall` in the query
//! log. An approved proposal is a request to its route with the
//! approver's credential, its reason the proposal's, so the audit log names
//! the admin who approved it, not the agent.
//!
//! A proposal holds for [`PROPOSAL_FOR_MS`] (an hour): an action seen fit
//! then is not applied to a cluster that has moved on since.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::http::{HeaderMap, HeaderValue, Method};
use axum::Router;
use serde::Serialize;
use serde_json::{json, Value as Json};

use crate::mcp::{forward, Caller};
use crate::service::Service;
use crate::telemetry::now_ms;
use crate::ApiError;

/// Model calls at most, by default and at most.
pub const STEPS: usize = 12;
pub const MAX_STEPS: usize = 30;
/// Characters of one tool's answer the model sees.
const SHOWN: usize = 12_000;
/// How long a proposal can be approved.
pub const PROPOSAL_FOR_MS: u64 = 3_600_000;
/// Proposals kept, the oldest decided ones dropped first.
const KEPT: usize = 200;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// An action the agent would take, for an admin to approve.
#[derive(Clone, Serialize)]
pub struct Proposal {
    pub id: String,
    /// The run that proposed it.
    pub run: String,
    pub created_ms: u64,
    pub expires_ms: u64,
    /// Who asked the agent.
    pub asked_by: String,
    /// `contain`, `remove_contain`, `cancel_query` or `silence_alert`.
    pub action: String,
    /// What it does, in words.
    pub what: String,
    pub why: String,
    /// The request an approval makes.
    pub method: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<Json>,
    /// `pending`, `applying`, `applied`, `failed`, `rejected` or `expired`.
    pub state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decided_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decided_ms: Option<u64>,
    /// The route's answer when applied, its error when it failed, the
    /// reason when rejected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Json>,
}

impl Proposal {
    fn state_now(&self, t: u64) -> &'static str {
        if self.state == "pending" && t >= self.expires_ms {
            "expired"
        } else {
            self.state
        }
    }

    fn json(&self, t: u64) -> Json {
        let mut j = json!(self);
        j["state"] = json!(self.state_now(t));
        j
    }
}

/// The proposals of every run, newest last.
#[derive(Default)]
pub struct Desk {
    inner: Mutex<(u64, VecDeque<Proposal>)>,
}

impl Desk {
    fn add(&self, mut p: Proposal) -> String {
        let mut g = lock(&self.inner);
        g.0 += 1;
        p.id = format!("p{}", g.0);
        let id = p.id.clone();
        let t = now_ms();
        if g.1.len() >= KEPT {
            // The oldest decided (or expired) one goes; else the oldest.
            let at = g.1.iter().position(|p| p.state_now(t) != "pending" && p.state != "applying").unwrap_or(0);
            g.1.remove(at);
        }
        g.1.push_back(p);
        id
    }

    /// The proposals, newest first; of one state, or of one run.
    pub fn list(&self, state: Option<&str>, run: Option<&str>) -> Vec<Json> {
        let t = now_ms();
        let g = lock(&self.inner);
        g.1.iter().rev().filter(|p| state.is_none_or(|s| p.state_now(t) == s)).filter(|p| run.is_none_or(|r| p.run == r)).map(|p| p.json(t)).collect()
    }

    fn of_run(&self, run: &str) -> Vec<Json> {
        let mut v = self.list(None, Some(run));
        v.reverse();
        v
    }

    /// Marks proposal `id` as being applied, if it is pending: the request
    /// to make.
    fn take(&self, id: &str) -> Result<Proposal, ApiError> {
        let t = now_ms();
        let mut g = lock(&self.inner);
        let p = g.1.iter_mut().find(|p| p.id == id).ok_or_else(|| ApiError { status: 404, message: format!("no proposal {id}: the server keeps the latest {KEPT}") })?;
        match p.state_now(t) {
            "pending" => {
                p.state = "applying";
                Ok(p.clone())
            }
            s => Err(ApiError { status: 409, message: format!("proposal {id} is {s}, not pending") }),
        }
    }

    fn decide(&self, id: &str, state: &'static str, by: &str, result: Json) -> Json {
        let t = now_ms();
        let mut g = lock(&self.inner);
        match g.1.iter_mut().find(|p| p.id == id) {
            Some(p) => {
                p.state = state;
                p.decided_by = Some(by.to_string());
                p.decided_ms = Some(t);
                p.result = Some(result);
                p.json(t)
            }
            None => Json::Null,
        }
    }

    /// Rejects pending proposal `id`.
    pub fn reject(&self, id: &str, by: &str, reason: &str) -> Result<Json, ApiError> {
        let p = self.take(id)?;
        Ok(self.decide(&p.id, "rejected", by, json!({"reason": reason})))
    }
}

/// What a run is asked.
pub struct OncallReq {
    /// A question, or what to look into.
    pub question: Option<String>,
    /// A firing alert's rule, to look into.
    pub alert: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// Overrides the server's key for this run (never stored).
    pub api_key: Option<String>,
    pub max_steps: usize,
    /// Also writes the answer as a note on the events timeline.
    pub note: bool,
    /// Who asks, as the audit names them.
    pub by: String,
}

fn system_prompt(svc: &Service) -> String {
    let name = svc.identity.name.as_deref().map_or("this cluster".to_string(), |n| format!("the cluster {n}"));
    let env = svc.identity.environment.as_deref().map_or(String::new(), |e| format!(" ({e})"));
    format!(
        r#"You are the on-call engineer for {name}{env}, a Strata cluster (Strata is a search database with SQL; a cluster is coordinators over shards, each shard a primary and its replicas). You look into an alert or a question with read-only tools, then report.

Tools:
- health, cluster, load, system: whether nodes are up, the shards and their copies, each copy's load, the host.
- alerts: the alert rules and their states; with `rule`, one rule with its recent events.
- events: the timeline (copies down, primaries changed, deploys, contain rules, notes) of the last minutes.
- queries: running, slow or recent queries, or statistics by statement (fingerprint), with the keys and apps that sent them.
- logs: this server's recent log lines, by level or words.
- timeseries: the server's metrics over the last minutes (rates, latencies, queues, memory).
- contain_rules: the rules that limit or refuse statements or keys now.
- propose_contain, propose_remove_contain, propose_cancel_query, propose_silence_alert: an action you would take. These change nothing: each is a proposal that an admin approves or rejects. Propose only what the evidence supports, the smallest action that helps (limit before refuse, a short expiry), and say why in a sentence.

How to work:
- Look before you conclude: base every claim on what a tool showed, and name it (an event's time, a query's fingerprint, a metric's value). Never invent nodes, numbers or causes.
- Start broad (health, alerts, events), then narrow to what moved.
- If a tool fails, say so and go on with the others.
- Stop as soon as you know enough.
- Never say an action was taken: proposals wait for an admin.

Answer in this shape, in plain sentences:
Summary: one or two sentences.
Evidence: a short list of what you saw.
Likely cause: your best explanation, and how sure you are.
Proposed actions: each proposal's id and what it does, or "none".
Watch: what would show it is getting better or worse."#
    )
}

fn tools() -> Json {
    let none = json!({"type": "object", "properties": {}});
    let why = json!({"type": "string", "description": "Why, in a sentence: the evidence for it."});
    json!([
        {"name": "health", "description": "Whether this server and its shards are up, with their row counts and versions; with fleet, every coordinator's.", "input_schema": {"type": "object", "properties": {"fleet": {"type": "boolean"}}}},
        {"name": "cluster", "description": "The cluster's shards and the copies of each: primary or replica, up or down, lag, slots.", "input_schema": none},
        {"name": "load", "description": "Each copy's load now: queries a second, latency, queue, CPU.", "input_schema": none},
        {"name": "system", "description": "The host: CPU, memory, disk, open files, threads.", "input_schema": none},
        {"name": "alerts", "description": "The alert rules and their states (firing, pending, ok, silenced); with rule, that rule's state and recent events.", "input_schema": {"type": "object", "properties": {"rule": {"type": "string"}}}},
        {"name": "events", "description": "The events timeline of the last `minutes` (default 60), newest first; kind narrows it (a comma-separated list, e.g. copy_down,primary_changed).", "input_schema": {"type": "object", "properties": {"minutes": {"type": "integer"}, "kind": {"type": "string"}}}},
        {"name": "queries", "description": "Queries: `running` now, the `slow` ones, the `recent` ones, or `stats` by statement fingerprint (counts, latency, errors, who sent them).", "input_schema": {"type": "object", "properties": {"which": {"type": "string", "enum": ["running", "slow", "recent", "stats"]}, "limit": {"type": "integer"}}, "required": ["which"]}},
        {"name": "logs", "description": "This server's recent log lines, newest last: of a level (error, warn, info) and up, holding words.", "input_schema": {"type": "object", "properties": {"level": {"type": "string"}, "words": {"type": "string"}, "limit": {"type": "integer"}}}},
        {"name": "timeseries", "description": "The server's metrics, a point each interval, over the last `minutes` (default 15).", "input_schema": {"type": "object", "properties": {"minutes": {"type": "integer"}}}},
        {"name": "contain_rules", "description": "The contain rules in force: what each matches, limits or refuses, until when, and how often it applied.", "input_schema": none},
        {"name": "propose_contain", "description": "Proposes a contain rule: limit (to `concurrency` at once) or refuse the queries of a statement fingerprint, a key, or both, for `expires_s` seconds (default 900). An admin approves it.", "input_schema": {"type": "object", "properties": {"fingerprint": {"type": "string"}, "key": {"type": "string"}, "action": {"type": "string", "enum": ["limit", "refuse"]}, "concurrency": {"type": "integer"}, "expires_s": {"type": "integer"}, "why": why}, "required": ["action", "why"]}},
        {"name": "propose_remove_contain", "description": "Proposes removing a contain rule, by its id. An admin approves it.", "input_schema": {"type": "object", "properties": {"id": {"type": "string"}, "why": why}, "required": ["id", "why"]}},
        {"name": "propose_cancel_query", "description": "Proposes cancelling a running query, by its id. An admin approves it.", "input_schema": {"type": "object", "properties": {"id": {"type": "string"}, "why": why}, "required": ["id", "why"]}},
        {"name": "propose_silence_alert", "description": "Proposes silencing an alert rule for `minutes`, while its cause is known and being handled. An admin approves it.", "input_schema": {"type": "object", "properties": {"rule": {"type": "string"}, "minutes": {"type": "integer"}, "why": why}, "required": ["rule", "minutes", "why"]}},
    ])
}

/// A path segment from the model, which names an id or a rule: only the
/// characters ids and rules have.
fn segment(v: &Json, what: &str) -> Result<String, String> {
    let s = v.as_str().map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| format!("{what} is required"))?;
    if s.len() > 200 || !s.chars().all(|c| c.is_ascii_alphanumeric() || "._-@:".contains(c)) {
        return Err(format!("{what} {s:?} is not an id"));
    }
    Ok(s.to_string())
}

fn uint(v: &Json, default: u64, max: u64) -> u64 {
    v.as_u64().unwrap_or(default).clamp(1, max)
}

/// The query string of `pairs`, each value percent-encoded.
fn query_string(pairs: &[(&str, String)]) -> String {
    let enc = |s: &str| s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~,".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect::<String>();
    let q: Vec<String> = pairs.iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| format!("{k}={}", enc(v))).collect();
    if q.is_empty() {
        String::new()
    } else {
        format!("?{}", q.join("&"))
    }
}

/// The read a tool makes: its route.
fn read_path(name: &str, input: &Json) -> Result<Option<String>, String> {
    let t = now_ms();
    Ok(Some(match name {
        "health" if input["fleet"].as_bool() == Some(true) => "/v1/health/fleet".into(),
        "health" => "/v1/health".into(),
        "cluster" => "/v1/cluster".into(),
        "load" => "/v1/load".into(),
        "system" => "/v1/system".into(),
        "contain_rules" => "/v1/admin/contain".into(),
        "alerts" => match &input["rule"] {
            Json::Null => "/v1/alerts".into(),
            r => format!("/v1/alerts/{}", segment(r, "rule")?),
        },
        "events" => {
            let minutes = uint(&input["minutes"], 60, 7 * 24 * 60);
            let kind = input["kind"].as_str().unwrap_or_default().to_string();
            format!("/v1/events{}", query_string(&[("from", t.saturating_sub(minutes * 60_000).to_string()), ("limit", "200".into()), ("kind", kind)]))
        }
        "queries" => {
            let limit = uint(&input["limit"], 20, 100).to_string();
            match input["which"].as_str() {
                Some("running") => "/v1/queries".into(),
                Some("slow") => format!("/v1/queries/slow?limit={limit}"),
                Some("recent") => format!("/v1/queries/recent?limit={limit}"),
                Some("stats") => format!("/v1/queries/stats?limit={limit}"),
                other => return Err(format!("which is running, slow, recent or stats, not {other:?}")),
            }
        }
        "logs" => {
            let limit = uint(&input["limit"], 100, 500).to_string();
            let level = input["level"].as_str().unwrap_or_default().to_string();
            let words = input["words"].as_str().unwrap_or_default().to_string();
            format!("/v1/logs{}", query_string(&[("limit", limit), ("level", level), ("q", words)]))
        }
        "timeseries" => {
            let minutes = uint(&input["minutes"], 15, 24 * 60);
            format!("/v1/timeseries?since={}", t.saturating_sub(minutes * 60_000))
        }
        _ => return Ok(None),
    }))
}

/// The proposal a `propose_*` tool makes, without its id.
fn proposal(name: &str, input: &Json, run: &str, by: &str) -> Result<Option<Proposal>, String> {
    let why = input["why"].as_str().map(str::trim).filter(|s| !s.is_empty()).ok_or("say why: `why` is required")?.to_string();
    let (action, what, method, path, body) = match name {
        "propose_contain" => {
            let fingerprint = input["fingerprint"].as_str().map(str::trim).filter(|s| !s.is_empty());
            let key = input["key"].as_str().map(str::trim).filter(|s| !s.is_empty());
            if fingerprint.is_none() && key.is_none() {
                return Err("a contain rule matches a fingerprint, a key or both".into());
            }
            let act = match input["action"].as_str() {
                Some(a @ ("limit" | "refuse")) => a,
                other => return Err(format!("action is limit or refuse, not {other:?}")),
            };
            let expires_s = uint(&input["expires_s"], 900, crate::contain::MAX_EXPIRES_S);
            let mut m = json!({});
            if let Some(f) = fingerprint {
                m["fingerprint"] = json!(f);
            }
            if let Some(k) = key {
                m["key"] = json!(k);
            }
            let mut body = json!({"match": m, "action": act, "expires_s": expires_s, "reason": why});
            let whom = [fingerprint.map(|f| format!("statement {f}")), key.map(|k| format!("key {k}"))].into_iter().flatten().collect::<Vec<_>>().join(" from ");
            let what = if act == "limit" {
                let c = input["concurrency"].as_u64().ok_or("a limit needs `concurrency`: the queries that run at once")?.min(10_000);
                body["concurrency"] = json!(c);
                format!("limit {whom} to {c} at once for {} min", expires_s.div_ceil(60))
            } else {
                format!("refuse {whom} for {} min", expires_s.div_ceil(60))
            };
            ("contain", what, Method::POST, "/v1/admin/contain".to_string(), Some(body))
        }
        "propose_remove_contain" => {
            let id = segment(&input["id"], "id")?;
            ("remove_contain", format!("remove contain rule {id}"), Method::DELETE, format!("/v1/admin/contain/{id}"), None)
        }
        "propose_cancel_query" => {
            let id = segment(&input["id"], "id")?;
            ("cancel_query", format!("cancel query {id}"), Method::DELETE, format!("/v1/queries/{id}"), None)
        }
        "propose_silence_alert" => {
            let rule = segment(&input["rule"], "rule")?;
            let minutes = uint(&input["minutes"], 30, 24 * 60);
            ("silence_alert", format!("silence alert {rule} for {minutes} min"), Method::POST, format!("/v1/alerts/{rule}/silence"), Some(json!({"minutes": minutes, "reason": why})))
        }
        _ => return Ok(None),
    };
    let t = now_ms();
    Ok(Some(Proposal {
        id: String::new(),
        run: run.to_string(),
        created_ms: t,
        expires_ms: t + PROPOSAL_FOR_MS,
        asked_by: by.to_string(),
        action: action.into(),
        what,
        why,
        method: method.to_string(),
        path,
        body,
        state: "pending",
        decided_by: None,
        decided_ms: None,
        result: None,
    }))
}

/// What the model sees of a tool's answer: its JSON, cut to [`SHOWN`].
fn shown(j: &Json) -> String {
    let s = j.to_string();
    if s.len() <= SHOWN {
        return s;
    }
    format!("{}… (cut: {} characters in all; narrow the call to see more)", crate::obs::truncate(&s, SHOWN), s.len())
}

/// The caller's headers, its app named `oncall` unless it names one.
fn as_oncall(headers: &HeaderMap) -> HeaderMap {
    let mut h = headers.clone();
    if !h.contains_key("x-strata-app") {
        h.insert("x-strata-app", HeaderValue::from_static("oncall"));
    }
    h
}

/// One run: the model's tool loop, then its answer, steps and proposals.
pub async fn run(svc: &Arc<Service>, router: Option<Router>, headers: &HeaderMap, peer: Option<std::net::SocketAddr>, req: OncallReq) -> Result<Json, ApiError> {
    let router = router.ok_or_else(|| ApiError::internal("the on-call agent's routes are missing"))?;
    let key = req.api_key.clone().or_else(|| svc.agent.api_key.clone()).ok_or_else(|| ApiError {
        status: 400,
        message: "no Anthropic API key: start the server with ANTHROPIC_API_KEY set, or pass api_key".into(),
    })?;
    let question = match (&req.question, &req.alert) {
        (Some(q), _) => q.clone(),
        (None, Some(rule)) => format!("Alert {rule} is firing. Find out why, how bad it is, and what to do."),
        (None, None) => return Err(ApiError::bad("the body needs a \"question\" or an \"alert\" (a rule's name)")),
    };
    let question = match (&req.question, &req.alert) {
        (Some(_), Some(rule)) => format!("{question}\n\n(About alert {rule}.)"),
        _ => question,
    };
    let model = req.model.clone().unwrap_or_else(|| svc.agent.model.clone());
    let run = format!("oc{}", now_ms());
    let headers = as_oncall(headers);
    let caller = Caller { headers: &headers, peer };
    let client = crate::http_client().timeout(Duration::from_secs(300)).build().map_err(ApiError::internal)?;
    let t0 = Instant::now();
    let mut messages = vec![json!({"role": "user", "content": question})];
    let mut steps = Vec::new();
    let (mut input_tokens, mut output_tokens, mut cache_read) = (0u64, 0u64, 0u64);
    let (mut answer, mut stop) = (String::new(), String::new());
    for step in 0..req.max_steps {
        let mut body = json!({
            "model": model,
            "max_tokens": 16000,
            "system": system_prompt(svc),
            "tools": tools(),
            "messages": messages,
            "cache_control": {"type": "ephemeral"},
        });
        if step + 1 == req.max_steps {
            body["tool_choice"] = json!({"type": "none"});
        }
        if let Some(e) = &req.effort {
            body["output_config"] = json!({"effort": e});
        }
        let resp = crate::agent::call(&client, &svc.agent.base_url, &key, &body).await?;
        let u = &resp["usage"];
        input_tokens += u["input_tokens"].as_u64().unwrap_or(0) + u["cache_creation_input_tokens"].as_u64().unwrap_or(0);
        cache_read += u["cache_read_input_tokens"].as_u64().unwrap_or(0);
        output_tokens += u["output_tokens"].as_u64().unwrap_or(0);
        stop = resp["stop_reason"].as_str().unwrap_or("").to_string();
        let content = resp["content"].as_array().cloned().unwrap_or_default();
        messages.push(json!({"role": "assistant", "content": content}));
        match stop.as_str() {
            "tool_use" => {
                let mut results = Vec::new();
                for b in content.iter().filter(|b| b["type"] == "tool_use") {
                    let (id, name, input) = (b["id"].as_str().unwrap_or(""), b["name"].as_str().unwrap_or(""), &b["input"]);
                    let t = Instant::now();
                    let r: Result<Json, String> = match read_path(name, input) {
                        Ok(Some(path)) => forward(&router, &caller, Method::GET, &path, None).await,
                        Ok(None) => match proposal(name, input, &run, &req.by) {
                            Ok(Some(p)) => {
                                let what = p.what.clone();
                                let id = svc.oncall.add(p);
                                Ok(json!({"proposed": id, "what": what, "state": "pending: an admin approves or rejects it"}))
                            }
                            Ok(None) => Err(format!("no tool {name:?}")),
                            Err(e) => Err(e),
                        },
                        Err(e) => Err(e),
                    };
                    let ms = t.elapsed().as_secs_f64() * 1e3;
                    let mut ui = json!({"tool": name, "input": input, "ok": r.is_ok(), "took_ms": ms});
                    match &r {
                        Ok(j) if j.get("proposed").is_some() => ui["proposed"] = j["proposed"].clone(),
                        Err(e) => ui["error"] = json!(e),
                        _ => {}
                    }
                    steps.push(ui);
                    results.push(match r {
                        Ok(j) => json!({"type": "tool_result", "tool_use_id": id, "content": shown(&j)}),
                        Err(e) => json!({"type": "tool_result", "tool_use_id": id, "content": e, "is_error": true}),
                    });
                }
                if step + 2 == req.max_steps {
                    results.push(json!({"type": "text", "text": "That was the last tool call: report now from what you found."}));
                }
                messages.push(json!({"role": "user", "content": results}));
            }
            "pause_turn" => continue,
            "refusal" => {
                answer = "The model declined to look into this.".into();
                break;
            }
            _ => {
                let text: Vec<&str> = content.iter().filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str()).collect();
                answer = text.join("\n");
                if stop == "max_tokens" {
                    answer.push_str("\n\n(The report was cut off at the output limit.)");
                }
                break;
            }
        }
    }
    if answer.is_empty() {
        answer = "No report within the step limit; the tools the agent called are listed below.".into();
    }
    let proposals = svc.oncall.of_run(&run);
    let mut out = json!({
        "run": run,
        "question": question,
        "answer": answer,
        "proposals": proposals,
        "steps": steps,
        "model": model,
        "stop_reason": stop,
        "usage": {"input_tokens": input_tokens, "output_tokens": output_tokens, "cache_read_input_tokens": cache_read},
        "took_ms": t0.elapsed().as_secs_f64() * 1e3,
    });
    if req.note {
        let ids: Vec<&str> = proposals.iter().filter_map(|p| p["id"].as_str()).collect();
        let tail = if ids.is_empty() { String::new() } else { format!(" Proposals: {}.", ids.join(", ")) };
        let head = format!("On-call agent ({run}): ");
        let room = 2000 - head.chars().count() - tail.chars().count();
        let text = format!("{head}{}{tail}", answer.chars().take(room).collect::<String>());
        out["note"] = match forward(&router, &caller, Method::POST, "/v1/events/notes", Some(json!({"text": text}))).await {
            Ok(j) => j,
            Err(e) => json!({"error": e}),
        };
    }
    Ok(out)
}

/// Applies pending proposal `id`: its request, with the approver's
/// credential and the proposal's reason.
pub async fn approve(svc: &Arc<Service>, router: Option<Router>, headers: &HeaderMap, peer: Option<std::net::SocketAddr>, id: &str, by: &str) -> Result<Json, ApiError> {
    let router = router.ok_or_else(|| ApiError::internal("the on-call agent's routes are missing"))?;
    let p = svc.oncall.take(id)?;
    let mut h = headers.clone();
    let reason = format!("on-call proposal {}: {}", p.id, p.why);
    if let Ok(v) = HeaderValue::from_str(&crate::obs::truncate(&reason.replace(['\r', '\n'], " "), 500)) {
        h.insert("x-strata-reason", v);
    }
    let caller = Caller { headers: &h, peer };
    let method = Method::from_bytes(p.method.as_bytes()).unwrap_or(Method::POST);
    let (state, result) = match forward(&router, &caller, method, &p.path, p.body.clone()).await {
        Ok(j) => ("applied", j),
        Err(e) => ("failed", json!({"error": e})),
    };
    tracing::info!(proposal = %p.id, by, state, "on-call proposal {}: {}", if state == "applied" { "applied" } else { "failed" }, p.what);
    Ok(svc.oncall.decide(&p.id, state, by, result))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_read_their_routes() {
        assert_eq!(read_path("health", &json!({})).unwrap().unwrap(), "/v1/health");
        assert_eq!(read_path("health", &json!({"fleet": true})).unwrap().unwrap(), "/v1/health/fleet");
        assert_eq!(read_path("alerts", &json!({"rule": "p99_high"})).unwrap().unwrap(), "/v1/alerts/p99_high");
        assert!(read_path("alerts", &json!({"rule": "../admin/keys"})).is_err());
        assert_eq!(read_path("queries", &json!({"which": "slow", "limit": 500})).unwrap().unwrap(), "/v1/queries/slow?limit=100");
        assert!(read_path("queries", &json!({})).is_err());
        let logs = read_path("logs", &json!({"level": "warn", "words": "disk full&x=1"})).unwrap().unwrap();
        assert_eq!(logs, "/v1/logs?limit=100&level=warn&q=disk%20full%26x%3D1");
        let events = read_path("events", &json!({"kind": "copy_down,primary_changed"})).unwrap().unwrap();
        assert!(events.starts_with("/v1/events?from=") && events.ends_with("&limit=200&kind=copy_down,primary_changed"), "{events}");
        assert_eq!(read_path("propose_contain", &json!({})).unwrap(), None);
    }

    #[test]
    fn proposals_say_what_they_do() {
        let p = proposal("propose_contain", &json!({"fingerprint": "ab12", "action": "limit", "concurrency": 2, "why": "it holds every core"}), "oc1", "admin").unwrap().unwrap();
        assert_eq!((p.action.as_str(), p.what.as_str(), p.method.as_str(), p.path.as_str()), ("contain", "limit statement ab12 to 2 at once for 15 min", "POST", "/v1/admin/contain"));
        assert_eq!(p.body.unwrap(), json!({"match": {"fingerprint": "ab12"}, "action": "limit", "concurrency": 2, "expires_s": 900, "reason": "it holds every core"}));
        let e = |name: &str, input: Json| proposal(name, &input, "oc1", "admin").err().unwrap();
        assert!(e("propose_contain", json!({"action": "refuse", "why": "x"})).contains("fingerprint, a key"));
        assert!(e("propose_contain", json!({"key": "k1", "action": "limit", "why": "x"})).contains("concurrency"));
        assert!(e("propose_cancel_query", json!({"id": "7"})).contains("why"));
        assert!(e("propose_cancel_query", json!({"id": "7/../../x", "why": "y"})).contains("not an id"));
        let s = proposal("propose_silence_alert", &json!({"rule": "copy_down", "minutes": 20, "why": "a replica is being rebuilt"}), "oc1", "admin").unwrap().unwrap();
        assert_eq!((s.path.as_str(), s.body.unwrap()), ("/v1/alerts/copy_down/silence", json!({"minutes": 20, "reason": "a replica is being rebuilt"})));
    }

    #[test]
    fn a_proposal_is_decided_once_and_expires() {
        let desk = Desk::default();
        let p = || proposal("propose_cancel_query", &json!({"id": "9", "why": "it runs for minutes"}), "oc1", "admin").unwrap().unwrap();
        let a = desk.add(p());
        let b = desk.add(p());
        assert_eq!((a.as_str(), b.as_str()), ("p1", "p2"));
        assert_eq!(desk.list(Some("pending"), None).len(), 2);
        desk.reject(&a, "ops", "not now").unwrap();
        assert_eq!(desk.reject(&a, "ops", "again").err().unwrap().status, 409);
        assert_eq!(desk.list(Some("rejected"), None)[0]["result"]["reason"], "not now");
        // An hour later it can no longer be approved.
        lock(&desk.inner).1.iter_mut().find(|p| p.id == b).unwrap().expires_ms = now_ms() - 1;
        let e = desk.take(&b).err().unwrap();
        assert!(e.status == 409 && e.message.contains("expired"), "{}", e.message);
        assert_eq!(desk.take("p9").err().unwrap().status, 404);
        assert_eq!(desk.of_run("oc1").len(), 2);
    }
}
