//! `chloedb-workbench mcp`: an MCP server over stdio (JSON-RPC lines) with
//! the tools that let an agent start and steer other agents. It is a client
//! of the HTTP API; its caller is `$CHLOEDB_WORKBENCH_AGENT`, the parent of
//! what it spawns.

use std::io::{BufRead, Write};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::hook::{agent as http_agent, Env};

const PROTOCOL: &str = "2025-06-18";

const INSTRUCTIONS: &str = "Workbench runs Claude Code sessions side by side. spawn_agent starts another agent: a real, interactive Claude Code session in its own terminal and (by default) its own git worktree and branch, which the user watches and can type into. Each one costs tokens like any session, so start one only when the work splits into parts that can go on at once, and give it a task that stands alone: it does not see this conversation, so say what to do, where, how to tell it is done, and what to report. Then wait_agent waits for its turn to finish (or for it to ask something), read_agent reads what it did, send_to_agent answers it, and stop_agent ends it. When only the user can decide something, ask_human asks them in Workbench (a question with options) and waits for the answer: the user does not watch your terminal. A goal's outcomes are kept with add_outcome and update_outcome.";

/// The longest an `ask_human` waits for the answer.
const ASK_WAIT_S: u64 = 30 * 60;

fn tools() -> Value {
    json!([
        {
            "name": "spawn_agent",
            "description": "Start another Claude Code agent on this repository, in its own terminal (and by default its own git worktree on a new branch wb/<name>). It is a real session that the user watches; it costs tokens. Give it a task that stands alone. Returns its id and name.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task": { "type": "string", "description": "The first prompt: everything the agent needs to know, since it does not see this conversation." },
                    "name": { "type": "string", "description": "A short name, a-z, 0-9 and -, at most 32; made from the task when left out." },
                    "worktree": { "type": "boolean", "description": "Run in a worktree of its own (default true); false runs it in this repository's checkout." },
                    "model": { "type": "string", "description": "The model, such as sonnet or opus; the default when left out." },
                    "base": { "type": "string", "description": "What its branch starts from (default: the repository's HEAD)." },
                    "outcome": { "type": "string", "description": "The id of the outcome (from add_outcome) it works on: it becomes that outcome's agent and is told so." }
                },
                "required": ["task"]
            }
        },
        {
            "name": "list_agents",
            "description": "The agents you started, with their state (starting, working, waiting, idle, exited), branch and last event; with all, every agent.",
            "inputSchema": { "type": "object", "properties": { "all": { "type": "boolean", "description": "Every agent, not only yours, the exited ones too." } } }
        },
        {
            "name": "read_agent",
            "description": "An agent's state and its events (prompts, tool calls, notifications, turns finished), one per line, after a sequence number.",
            "inputSchema": { "type": "object", "properties": { "id": { "type": "string" }, "after": { "type": "integer", "description": "Only events with a higher sequence number (the first number of each line)." } }, "required": ["id"] }
        },
        {
            "name": "wait_agent",
            "description": "Wait until an agent reaches a state: idle (its turn finished, the default), waiting (it asks the user something), exited, or any (any new event). Answers its state and its last events.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "until": { "type": "string", "enum": ["idle", "waiting", "exited", "any"] },
                    "timeout_s": { "type": "integer", "description": "At most this many seconds (default 300, at most 1800)." }
                },
                "required": ["id"]
            }
        },
        {
            "name": "send_to_agent",
            "description": "Type text and Enter into an agent's terminal: a prompt when it is idle, an answer when it waits.",
            "inputSchema": { "type": "object", "properties": { "id": { "type": "string" }, "text": { "type": "string" } }, "required": ["id", "text"] }
        },
        {
            "name": "stop_agent",
            "description": "Stop an agent: its session ends (its worktree and branch are kept).",
            "inputSchema": { "type": "object", "properties": { "id": { "type": "string" } }, "required": ["id"] }
        },
        {
            "name": "set_preview",
            "description": "Say where the app you are working on runs (a dev server you started), so that the user sees it in Workbench's Preview and can point at what to change. Only http://127.0.0.1:<port> or http://localhost:<port>; an empty url clears it.",
            "inputSchema": { "type": "object", "properties": { "url": { "type": "string", "description": "Such as http://127.0.0.1:5173" } }, "required": ["url"] }
        },
        {
            "name": "ask_human",
            "description": "Ask the user a question in Workbench and wait for the answer (up to 30 minutes). Use it when only the user can decide: a choice of behaviour, scope or risk. Give 2 to 4 options and say which you recommend; the user can also answer in words, and may make the answer a rule that every agent of this repository is told from then on. Returns the answer.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "question": { "type": "string", "description": "The question, in one or two sentences the user can answer without reading your terminal." },
                    "options": { "type": "array", "description": "The choices.", "items": { "type": "object", "properties": { "label": { "type": "string", "description": "A few words." }, "detail": { "type": "string", "description": "What choosing it means." } }, "required": ["label"] } },
                    "recommended": { "type": "integer", "description": "The index of the option you recommend, from 0." },
                    "context": { "type": "string", "description": "What the user needs to know to decide: what you found, what each choice costs." }
                },
                "required": ["question"]
            }
        },
        {
            "name": "add_outcome",
            "description": "Add an outcome to your goal: a result the user can check, worked on by one agent. Returns its id, which spawn_agent takes as outcome.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "What will be true when it is done, in under 10 words." },
                    "detail": { "type": "string", "description": "How to tell it is done." },
                    "after": { "type": "string", "description": "The id of an outcome it must wait for." }
                },
                "required": ["title"]
            }
        },
        {
            "name": "update_outcome",
            "description": "Update an outcome: its progress (0 to 100), its state (done when it checks out, dropped when it no longer makes sense), its title or detail. Without arguments but id, answers your goal's outcomes.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "progress": { "type": "integer", "minimum": 0, "maximum": 100 },
                    "state": { "type": "string", "enum": ["next", "active", "needs_you", "done", "dropped"] },
                    "title": { "type": "string" },
                    "detail": { "type": "string" }
                },
                "required": ["id"]
            }
        },
        {
            "name": "list_incidents",
            "description": "Health's incidents in your repository: alert rules that fired on agents (a test failing again and again, tokens over a budget, two agents editing one file, an agent idle while asked), grouped by the agents and files they share, with what fires now.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "state": { "type": "string", "enum": ["open", "resolved", "all"], "description": "open (the default), resolved or all." }
                }
            }
        },
        {
            "name": "test_history",
            "description": "The test radar of your repository: each test's runs by every agent, pass rate, p50 and p99 duration, and which are flaky (passed and failed on the same code), newly failing or slow. With test, that test's runs, newest first.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "test": { "type": "string", "description": "A test's name as the radar lists it, such as tests::totals::applies_coupon." },
                    "days": { "type": "integer", "minimum": 1, "maximum": 90, "description": "How far back, in days (default 7)." }
                }
            }
        }
    ])
}

struct Client {
    env: Env,
}

impl Client {
    fn call(&self, method: &str, path: &str, body: Option<Value>, timeout: Duration) -> Result<Value, String> {
        let url = format!("{}{path}", self.env.url);
        let agent = http_agent(timeout);
        let auth = format!("Bearer {}", self.env.token);
        // Who calls: the input it types is recorded as this agent's.
        let me = self.env.agent.as_str();
        let res = match body {
            Some(b) if method == "PUT" => agent.put(&url).header("authorization", &auth).send_json(b),
            Some(b) if method == "PATCH" => agent.patch(&url).header("authorization", &auth).header("x-workbench-agent", me).send_json(b),
            Some(b) if !me.is_empty() => agent.post(&url).header("authorization", &auth).header("x-workbench-agent", me).send_json(b),
            Some(b) => agent.post(&url).header("authorization", &auth).send_json(b),
            None if method == "POST" => agent.post(&url).header("authorization", &auth).send_empty(),
            None => agent.get(&url).header("authorization", &auth).call(),
        };
        let mut res = res.map_err(|e| format!("cannot reach Workbench at {}: {e}", self.env.url))?;
        let status = res.status().as_u16();
        let text = res.body_mut().read_to_string().map_err(|e| e.to_string())?;
        let v: Value = serde_json::from_str(&text).unwrap_or_else(|_| json!({ "error": text }));
        if status >= 400 {
            return Err(v.get("error").and_then(Value::as_str).unwrap_or("error").to_string());
        }
        Ok(v)
    }

    fn get(&self, path: &str) -> Result<Value, String> {
        self.call("GET", path, None, Duration::from_secs(15))
    }
}

/// `&repo=` with the caller's repository, or nothing (the server takes
/// the one most agents work in).
fn repo_param(c: &Client) -> String {
    if c.env.agent.is_empty() {
        return String::new();
    }
    match c.get(&format!("/api/agents/{}", enc(&c.env.agent))) {
        Ok(v) => v["agent"]["repo"].as_str().map(|r| format!("&repo={}", enc(r))).unwrap_or_default(),
        Err(_) => String::new(),
    }
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn agent_line(a: &Value) -> String {
    let mut line = format!("{} {} [{}]", s(a, "id"), s(a, "name"), s(a, "state"));
    if !s(a, "branch").is_empty() {
        line.push_str(&format!(" branch {}", s(a, "branch")));
    }
    if s(a, "state") == "waiting" && !s(a, "waiting_for").is_empty() {
        line.push_str(&format!(" waiting for: {}", s(a, "waiting_for")));
    }
    if !s(a, "last_event").is_empty() {
        line.push_str(&format!(" | last: {}", s(a, "last_event")));
    }
    line
}

fn event_lines(evs: &Value) -> String {
    let lines: Vec<String> = evs
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| format!("{} {} {}: {}", e.get("seq").and_then(Value::as_i64).unwrap_or(0), s(e, "ts"), s(e, "kind"), s(e, "summary")))
        .collect();
    if lines.is_empty() {
        "(no events)".into()
    } else {
        lines.join("\n")
    }
}

fn id_arg(args: &Value) -> Result<String, String> {
    let id = s(args, "id");
    if id.is_empty() {
        return Err("id is required".into());
    }
    Ok(enc(id))
}

/// The agent's state and its last events, as text.
fn status(c: &Client, id: &str, last: usize) -> Result<String, String> {
    let a = c.get(&format!("/api/agents/{id}"))?;
    let evs = c.get(&format!("/api/agents/{id}/events?limit=1000"))?;
    let all = evs["events"].as_array().cloned().unwrap_or_default();
    let tail: Vec<Value> = all[all.len().saturating_sub(last)..].to_vec();
    Ok(format!("{}\nlast events:\n{}", agent_line(&a["agent"]), event_lines(&Value::Array(tail))))
}

fn tool(c: &Client, name: &str, args: &Value) -> Result<String, String> {
    match name {
        "spawn_agent" => {
            let task = s(args, "task");
            if task.trim().is_empty() {
                return Err("task is required".into());
            }
            let repo = if c.env.agent.is_empty() {
                std::env::current_dir().map_err(|e| e.to_string())?.to_string_lossy().to_string()
            } else {
                let me = c.get(&format!("/api/agents/{}", enc(&c.env.agent)))?;
                s(&me["agent"], "repo").to_string()
            };
            let mut body = json!({ "repo": repo, "task": task, "worktree": args.get("worktree").and_then(Value::as_bool).unwrap_or(true) });
            for k in ["name", "model", "base", "outcome"] {
                if let Some(v) = args.get(k).and_then(Value::as_str).filter(|v| !v.is_empty()) {
                    body[k] = json!(v);
                }
            }
            if !c.env.agent.is_empty() {
                body["parent"] = json!(c.env.agent);
            }
            let v = c.call("POST", "/api/agents", Some(body), Duration::from_secs(60))?;
            let a = &v["agent"];
            Ok(format!(
                "Started agent {} (id {}) in {}{}. The user can watch it in Workbench. Use wait_agent with id {} to wait for its turn to finish.",
                s(a, "name"),
                s(a, "id"),
                s(a, "cwd"),
                if s(a, "branch").is_empty() { String::new() } else { format!(" on branch {}", s(a, "branch")) },
                s(a, "id")
            ))
        }
        "list_agents" => {
            let all = args.get("all").and_then(Value::as_bool).unwrap_or(false);
            let v = c.get("/api/agents?all=1")?;
            let lines: Vec<String> = v["agents"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|a| all || (!c.env.agent.is_empty() && s(a, "parent") == c.env.agent) || (c.env.agent.is_empty() && a.get("parent").is_none_or(Value::is_null)))
                .map(agent_line)
                .collect();
            Ok(if lines.is_empty() { "(no agents)".into() } else { lines.join("\n") })
        }
        "read_agent" => {
            let id = id_arg(args)?;
            let after = args.get("after").and_then(Value::as_i64).unwrap_or(0);
            let a = c.get(&format!("/api/agents/{id}"))?;
            let evs = c.get(&format!("/api/agents/{id}/events?after={after}&limit=1000"))?;
            Ok(format!("{}\n{}", agent_line(&a["agent"]), event_lines(&evs["events"])))
        }
        "wait_agent" => {
            let id = id_arg(args)?;
            let until = match s(args, "until") {
                "" => "idle",
                u @ ("idle" | "waiting" | "exited" | "any") => u,
                u => return Err(format!("until is one of idle, waiting, exited, any, not {u}")),
            };
            let total = args.get("timeout_s").and_then(Value::as_u64).unwrap_or(300).min(1800);
            let deadline = Instant::now() + Duration::from_secs(total);
            let reached = loop {
                let left = deadline.saturating_duration_since(Instant::now()).as_secs();
                let step = left.min(30);
                let v = c.call("GET", &format!("/api/wait?agent={id}&until={until}&timeout_s={step}"), None, Duration::from_secs(step + 15))?;
                let reached = v["reached"].as_bool().unwrap_or(false);
                let state = s(&v["agent"], "state");
                if reached || state == "exited" || Instant::now() >= deadline || step == 0 {
                    break reached;
                }
            };
            let head = if reached { format!("reached {until}") } else { format!("not {until} yet (waited up to {total} s)") };
            Ok(format!("{head}\n{}", status(c, &id, 10)?))
        }
        "send_to_agent" => {
            let id = id_arg(args)?;
            let text = s(args, "text");
            c.call("POST", &format!("/api/agents/{id}/input"), Some(json!({ "text": text, "enter": true })), Duration::from_secs(15))?;
            Ok(format!("typed into {} and pressed Enter", s(args, "id")))
        }
        "stop_agent" => {
            let id = id_arg(args)?;
            c.call("POST", &format!("/api/agents/{id}/stop"), None, Duration::from_secs(15))?;
            Ok(format!("stopping {} (SIGHUP, then SIGKILL after 5 s)", s(args, "id")))
        }
        "set_preview" => {
            if c.env.agent.is_empty() {
                return Err("set_preview is for an agent of Workbench: CHLOEDB_WORKBENCH_AGENT is not set".into());
            }
            let url = s(args, "url").trim();
            let v = c.call("PUT", &format!("/api/agents/{}/preview", enc(&c.env.agent)), Some(json!({ "url": url })), Duration::from_secs(15))?;
            Ok(match v["agent"]["preview_url"].as_str() {
                Some(u) => format!("The preview is {u}: the user sees it in Workbench's Preview, and can pick an element there to send you."),
                None => "The preview is cleared.".into(),
            })
        }
        "ask_human" => ask_human(c, args),
        "add_outcome" => {
            let title = s(args, "title");
            if title.trim().is_empty() {
                return Err("title is required".into());
            }
            let mut body = json!({ "title": title });
            for k in ["detail", "after"] {
                if let Some(v) = args.get(k).and_then(Value::as_str).filter(|v| !v.is_empty()) {
                    body[k] = json!(v);
                }
            }
            let v = c.call("POST", "/api/outcomes", Some(body), Duration::from_secs(15))?;
            let o = &v["outcome"];
            Ok(format!("Added outcome {} ({}) to goal {}. Start its agent with spawn_agent(outcome=\"{}\", task=...).", s(o, "id"), s(o, "title"), s(o, "goal"), s(o, "id")))
        }
        "update_outcome" => {
            let id = id_arg(args)?;
            let mut body = json!({});
            for k in ["state", "title", "detail"] {
                if let Some(v) = args.get(k).and_then(Value::as_str).filter(|v| !v.is_empty()) {
                    body[k] = json!(v);
                }
            }
            if let Some(p) = args.get("progress").and_then(Value::as_i64) {
                body["progress"] = json!(p);
            }
            let v = c.call("PATCH", &format!("/api/outcomes/{id}"), Some(body), Duration::from_secs(15))?;
            let o = &v["outcome"];
            let all = c.get(&format!("/api/outcomes?goal={}", enc(s(o, "goal"))))?;
            let lines: Vec<String> = all["outcomes"].as_array().into_iter().flatten().map(outcome_line).collect();
            Ok(format!("{}\ngoal {}'s outcomes:\n{}", outcome_line(o), s(o, "goal"), lines.join("\n")))
        }
        "list_incidents" => {
            let state = match s(args, "state") {
                "" => "open",
                x => x,
            };
            let v = c.get(&format!("/api/incidents?state={}{}", enc(state), repo_param(c)))?;
            Ok(crate::alerts::incidents_text(&v))
        }
        "test_history" => {
            let test = s(args, "test").trim();
            if test.is_empty() {
                let days = args.get("days").and_then(Value::as_i64).unwrap_or(7);
                let v = c.get(&format!("/api/tests?days={days}{}", repo_param(c)))?;
                Ok(crate::testruns::radar_text(&v))
            } else {
                let v = c.get(&format!("/api/tests/history?test={}{}", enc(test), repo_param(c)))?;
                Ok(crate::testruns::history_text(&v))
            }
        }
        other => Err(format!("no tool {other}")),
    }
}

fn outcome_line(o: &Value) -> String {
    let mut line = format!("{} [{}] {}", s(o, "id"), s(o, "state"), s(o, "title"));
    if let Some(p) = o["progress"].as_i64().filter(|p| *p > 0 && *p < 100) {
        line.push_str(&format!(" {p}%"));
    }
    if !s(o, "agent_name").is_empty() {
        line.push_str(&format!(" (agent {})", s(o, "agent_name")));
    }
    if !s(o, "after").is_empty() {
        line.push_str(&format!(" after {}", s(o, "after")));
    }
    line
}

/// What an answered question says to the agent that asked it.
fn answer_text(q: &Value) -> String {
    let by = s(q, "answered_by");
    let who = if by.starts_with("rule:") { format!("A rule the user set ({}) answers it", by.trim_start_matches("rule:")) } else { "The user answered".to_string() };
    let mut out = format!("{who}: {}", s(q, "answer"));
    if !s(q, "rule").is_empty() && !by.starts_with("rule:") {
        out.push_str("\n(The user made this a rule for the repository: follow it from now on.)");
    }
    out
}

/// `ask_human`: posts the question, then waits for its answer in 30 s steps.
fn ask_human(c: &Client, args: &Value) -> Result<String, String> {
    if c.env.agent.is_empty() {
        return Err("ask_human needs CHLOEDB_WORKBENCH_AGENT: it runs inside a Workbench session".into());
    }
    let question = s(args, "question");
    if question.trim().is_empty() {
        return Err("question is required".into());
    }
    let mut body = json!({ "question": question, "options": args.get("options").cloned().unwrap_or(json!([])) });
    for k in ["recommended", "context"] {
        if let Some(v) = args.get(k).filter(|v| !v.is_null()) {
            body[k] = v.clone();
        }
    }
    let v = c.call("POST", "/api/questions", Some(body), Duration::from_secs(30))?;
    let q = v["question"].clone();
    let id = s(&q, "id").to_string();
    if s(&q, "state") == "answered" {
        return Ok(answer_text(&q));
    }
    let deadline = Instant::now() + Duration::from_secs(ASK_WAIT_S);
    loop {
        let left = deadline.saturating_duration_since(Instant::now()).as_secs();
        if left == 0 {
            return Ok(format!("No answer yet after {} minutes (question {id}). Carry on with what does not depend on it, or ask again later.", ASK_WAIT_S / 60));
        }
        let step = left.min(30);
        let v = c.call("GET", &format!("/api/questions/{}/wait?timeout_s={step}", enc(&id)), None, Duration::from_secs(step + 15))?;
        let q = &v["question"];
        match s(q, "state") {
            "answered" => return Ok(answer_text(q)),
            "expired" => return Ok(format!("The question {id} expired without an answer.")),
            _ => {}
        }
    }
}

fn handle(c: Option<&Client>, msg: &Value) -> Option<Value> {
    let id = msg.get("id").cloned();
    let method = s(msg, "method");
    let id = id?; // a notification: no answer
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "workbench", "version": env!("CARGO_PKG_VERSION") },
            "instructions": INSTRUCTIONS,
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools() })),
        "tools/call" => {
            let params = msg.get("params").cloned().unwrap_or(json!({}));
            let name = s(&params, "name").to_string();
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            let out = match c {
                Some(c) => tool(c, &name, &args),
                None => Err("CHLOEDB_WORKBENCH_URL and CHLOEDB_WORKBENCH_TOKEN are not set: this server runs inside a Workbench session".into()),
            };
            Ok(match out {
                Ok(text) => json!({ "content": [{ "type": "text", "text": text }] }),
                Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
            })
        }
        "resources/list" => Ok(json!({ "resources": [] })),
        "prompts/list" => Ok(json!({ "prompts": [] })),
        _ => Err(json!({ "code": -32601, "message": format!("method not found: {method}") })),
    };
    Some(match result {
        Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
        Err(e) => json!({ "jsonrpc": "2.0", "id": id, "error": e }),
    })
}

pub fn run() {
    let client = Env::from_env().map(|env| Client { env });
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let answer = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => handle(client.as_ref(), &msg),
            Err(e) => Some(json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": format!("parse error: {e}") } })),
        };
        if let Some(a) = answer {
            if writeln!(out, "{a}").is_err() || out.flush().is_err() {
                break;
            }
        }
    }
}
