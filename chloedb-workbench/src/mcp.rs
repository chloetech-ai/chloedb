//! `chloedb-workbench mcp`: an MCP server over stdio (JSON-RPC lines) with
//! the tools that let an agent start and steer other agents. It is a client
//! of the HTTP API; its caller is `$CHLOEDB_WORKBENCH_AGENT`, the parent of
//! what it spawns.

use std::io::{BufRead, Write};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::hook::{agent as http_agent, Env};

const PROTOCOL: &str = "2025-06-18";

const INSTRUCTIONS: &str = "Workbench runs Claude Code sessions side by side. spawn_agent starts another agent: a real, interactive Claude Code session in its own terminal and (by default) its own git worktree and branch, which the user watches and can type into. Each one costs tokens like any session, so start one only when the work splits into parts that can go on at once, and give it a task that stands alone: it does not see this conversation, so say what to do, where, how to tell it is done, and what to report. Then wait_agent waits for its turn to finish (or for it to ask something), read_agent reads what it did, send_to_agent answers it, and stop_agent ends it.";

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
                    "base": { "type": "string", "description": "What its branch starts from (default: the repository's HEAD)." }
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
        let res = match body {
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
            for k in ["name", "model", "base"] {
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
        other => Err(format!("no tool {other}")),
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
