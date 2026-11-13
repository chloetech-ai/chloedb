//! The blackboard's tools ([`chloedb::blackboard`]) for agents, over HTTP
//! and over the Model Context Protocol. **Experimental**, as tenants are
//! ([`crate::tenants`]).
//!
//! A blackboard is a tenant made from the template `blackboard`
//! (`POST /v1/admin/tenants` with `"template": "blackboard"`). Its tools:
//!
//! | tool | does |
//! |---|---|
//! | `add_tasks` | takes tasks in, each id once |
//! | `claim` | gives the caller open tasks, and claimed ones whose lease has passed |
//! | `heartbeat` | extends the caller's lease on a task |
//! | `complete`, `fail` | end the caller's claim |
//! | `post` | records a finding, as a duplicate of one that says the same |
//! | `search`, `timeline`, `status`, `get_task` | read |
//!
//! `POST /v1/admin/tenants/:id/board/:tool` calls one, its arguments and
//! the caller's name (`"agent"`) in the body. `POST
//! /v1/admin/tenants/:id/mcp?agent=NAME` is the same tools as an MCP
//! server (Streamable HTTP, answering JSON, no session), for an agent
//! that is given the URL: the agent's name is the URL's, so that it does
//! not change within a run. Both need an admin key until tenants have
//! keys of their own.
//!
//! A call that another server beat to the tenant (`NotWriter`: nothing of
//! it was written) is tried again here, twice. A call whose commit is in
//! doubt answers 503, and is safe to send again with the same `key`.

use chloedb::blackboard::{Board, BoardError, NewTask};
use chloedb::tenant::{Tenant, TenantError};
use serde_json::{json, Value as Json};

/// A tool: its name, what a model reads about it, and its arguments as
/// a JSON schema's properties and required names.
pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    properties: fn() -> Json,
    required: &'static [&'static str],
}

const KEY: &str = "A name of your own for this call (letters, digits, dashes). Send the same key when you send the call again after an error or a timeout: it is then applied once, and answers what it answered the first time.";
const LATEST: &str = "true looks at the store first, to see what other servers committed (slower).";

pub const TOOLS: [Tool; 10] = [
    Tool {
        name: "claim",
        description: "Claims work: gives you up to n tasks nobody is working on (open ones, and claimed ones whose lease has passed), oldest first. Each is yours for lease_s seconds: call heartbeat before that if you need longer, and complete or fail when you are through. An empty list means there is nothing to do now.",
        properties: || json!({"n": {"type": "integer", "minimum": 1, "maximum": 100, "description": "How many tasks at most (1 unless given)."}, "lease_s": {"type": "integer", "minimum": 1, "maximum": 86400, "description": "Seconds each task is yours (300 unless given)."}, "key": {"type": "string", "description": KEY}}),
        required: &[],
    },
    Tool {
        name: "heartbeat",
        description: "Extends your lease on a task you claimed to lease_s seconds from now. Refused when the lease passed and another agent claimed the task: stop working on it then.",
        properties: || json!({"task": {"type": "string"}, "lease_s": {"type": "integer", "minimum": 1, "maximum": 86400, "description": "Seconds from now (300 unless given)."}}),
        required: &["task"],
    },
    Tool {
        name: "complete",
        description: "Ends your claim on a task: it is done, with your result. Refused when the task is not yours any more (your lease passed and another agent claimed it), so a task is done once.",
        properties: || json!({"task": {"type": "string"}, "result": {"type": "string", "description": "What you did, for the others to read."}, "key": {"type": "string", "description": KEY}}),
        required: &["task", "result"],
    },
    Tool {
        name: "fail",
        description: "Ends your claim on a task: it failed, with the reason. Nobody is given it again.",
        properties: || json!({"task": {"type": "string"}, "why": {"type": "string"}, "key": {"type": "string", "description": KEY}}),
        required: &["task", "why"],
    },
    Tool {
        name: "post",
        description: "Records a finding for the other agents: something you learned that they should not have to learn again. Name the task it is about if there is one. When an earlier finding says the same in nearly the same words, yours is kept as its duplicate (dup_of names it).",
        properties: || json!({"body": {"type": "string"}, "task": {"type": "string"}, "key": {"type": "string", "description": KEY}}),
        required: &["body"],
    },
    Tool {
        name: "search",
        description: "Finds findings and tasks by their words, best first. Search before you start on something: another agent may have posted what you need.",
        properties: || json!({"q": {"type": "string", "description": "Words to look for; a row with any of them matches, those with more of them first."}, "limit": {"type": "integer", "minimum": 1, "maximum": 500, "description": "Of each, at most (10 unless given)."}, "latest": {"type": "boolean", "description": LATEST}}),
        required: &["q"],
    },
    Tool {
        name: "timeline",
        description: "What happened on the board, in order: tasks taken in, claims, tasks done or failed, findings. Pass the id of the last event you read as since to read on from there.",
        properties: || json!({"since": {"type": "integer", "minimum": 0, "description": "Events after this id (0, from the first, unless given)."}, "limit": {"type": "integer", "minimum": 1, "maximum": 500, "description": "At most (100 unless given)."}, "latest": {"type": "boolean", "description": LATEST}}),
        required: &[],
    },
    Tool {
        name: "status",
        description: "The board in numbers: tasks by state (open, claimed, done, failed), claimed tasks whose lease has passed, findings and duplicates, and the last event's id.",
        properties: || json!({"latest": {"type": "boolean", "description": LATEST}}),
        required: &[],
    },
    Tool {
        name: "get_task",
        description: "One task by its id: its title, spec, state, owner, lease, attempts and result.",
        properties: || json!({"id": {"type": "string"}, "latest": {"type": "boolean", "description": LATEST}}),
        required: &["id"],
    },
    Tool {
        name: "add_tasks",
        description: "Takes tasks in, for agents to claim. A task whose id the board has already is left as it is, so sending a list twice makes each task with an id once; a task without an id is given one (t1, t2, ...).",
        properties: || json!({"tasks": {"type": "array", "minItems": 1, "maxItems": 100, "items": {"type": "object", "properties": {"id": {"type": "string"}, "title": {"type": "string"}, "spec": {"type": "string", "description": "What to do, in full."}}, "required": ["title"], "additionalProperties": false}}, "key": {"type": "string", "description": KEY}}),
        required: &["tasks"],
    },
];

const INSTRUCTIONS: &str = "This is a blackboard: a shared store for the agents working one job. \
Call claim to be given tasks, do them, and call complete (or fail) for each; call heartbeat on a task that takes longer than its lease. \
Call search before you start, and post what you learn that others would otherwise have to find out again. \
When a call fails or times out, send it again with the same key: it is applied once.";

/// `tools/list`'s answer.
pub fn tools_json() -> Json {
    json!({"tools": TOOLS.iter().map(|t| json!({
        "name": t.name,
        "description": t.description,
        "inputSchema": {"type": "object", "properties": (t.properties)(), "required": t.required, "additionalProperties": false},
    })).collect::<Vec<_>>()})
}

/// Why a tool's call failed.
#[derive(Debug)]
pub enum Failed {
    /// No tool has the name.
    NoTool(String),
    /// Arguments the tool does not take.
    Arguments(String),
    /// The board's answer: refused, or the tenant's error.
    Board(BoardError),
    /// The tenant is not a blackboard.
    NotABoard,
}

impl std::fmt::Display for Failed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failed::NoTool(name) => write!(f, "no tool {name:?}: {}", TOOLS.map(|t| t.name).join(", ")),
            Failed::Arguments(why) => f.write_str(why),
            Failed::Board(e) => e.fmt(f),
            Failed::NotABoard => f.write_str("this tenant is not a blackboard: create one with \"template\": \"blackboard\""),
        }
    }
}

impl From<BoardError> for Failed {
    fn from(e: BoardError) -> Failed {
        Failed::Board(e)
    }
}

impl From<Failed> for crate::ApiError {
    fn from(e: Failed) -> crate::ApiError {
        let status = match e {
            Failed::Board(BoardError::Tenant(e)) => return e.into(),
            Failed::NoTool(_) => 404,
            Failed::Arguments(_) | Failed::NotABoard => 400,
            // Nothing was written, and the same call would be refused again.
            Failed::Board(BoardError::Refused(_)) => 409,
        };
        crate::ApiError { code: None, status, message: e.to_string() }
    }
}

fn text<'a>(args: &'a Json, k: &str) -> Result<Option<&'a str>, Failed> {
    match args.get(k) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::String(s)) => Ok(Some(s)),
        Some(_) => Err(Failed::Arguments(format!("{k} is text"))),
    }
}

fn need<'a>(args: &'a Json, k: &str) -> Result<&'a str, Failed> {
    text(args, k)?.ok_or_else(|| Failed::Arguments(format!("{k} is missing")))
}

fn number(args: &Json, k: &str, default: u64) -> Result<u64, Failed> {
    match args.get(k) {
        None | Some(Json::Null) => Ok(default),
        Some(v) => v.as_u64().ok_or_else(|| Failed::Arguments(format!("{k} is a whole number, 0 or more"))),
    }
}

fn flag(args: &Json, k: &str) -> Result<bool, Failed> {
    match args.get(k) {
        None | Some(Json::Null) => Ok(false),
        Some(Json::Bool(b)) => Ok(*b),
        Some(_) => Err(Failed::Arguments(format!("{k} is true or false"))),
    }
}

fn answer(v: impl serde::Serialize) -> Result<Json, Failed> {
    serde_json::to_value(v).map_err(|e| Failed::Board(BoardError::Tenant(TenantError::Db(chloedb::Error::Schema(e.to_string())))))
}

/// Calls `tool` on the blackboard of `tenant` for `agent`.
pub fn call(tenant: &Tenant, agent: &str, tool: &str, args: &Json) -> Result<Json, Failed> {
    let spec = TOOLS.iter().find(|t| t.name == tool).ok_or_else(|| Failed::NoTool(tool.to_string()))?;
    let Some(given) = args.as_object() else { return Err(Failed::Arguments("arguments are an object".into())) };
    let takes = (spec.properties)();
    if let Some(unknown) = given.keys().find(|k| takes.get(k.as_str()).is_none()) {
        let names: Vec<&str> = takes.as_object().map(|o| o.keys().map(String::as_str).collect()).unwrap_or_default();
        return Err(Failed::Arguments(format!("{tool} takes no argument {unknown:?}: it takes {}", if names.is_empty() { "none".into() } else { names.join(", ") })));
    }
    if !tenant.read(|db| Ok(chloedb::blackboard::is_board(db))).map_err(BoardError::Tenant)? {
        return Err(Failed::NotABoard);
    }
    let key = text(args, "key")?;
    let board = Board::new(tenant, agent)?.latest(flag(args, "latest")?);
    let once = || -> Result<Json, Failed> {
        match tool {
            "add_tasks" => {
                let tasks: Vec<NewTask> = serde_json::from_value(args.get("tasks").cloned().unwrap_or(Json::Null)).map_err(|e| Failed::Arguments(format!("tasks is a list of {{id, title, spec}}: {e}")))?;
                answer(board.add_tasks(&tasks, key)?)
            }
            "claim" => answer(json!({"tasks": board.claim(number(args, "n", 1)? as usize, number(args, "lease_s", 300)?, key)?})),
            "heartbeat" => answer(board.heartbeat(need(args, "task")?, number(args, "lease_s", 300)?)?),
            "complete" => answer(board.complete(need(args, "task")?, need(args, "result")?, key)?),
            "fail" => answer(board.fail(need(args, "task")?, need(args, "why")?, key)?),
            "post" => answer(board.post(need(args, "body")?, text(args, "task")?, key)?),
            "search" => answer(board.search(need(args, "q")?, number(args, "limit", 10)? as usize)?),
            "timeline" => answer(json!({"events": board.timeline(number(args, "since", 0)?.min(i64::MAX as u64) as i64, number(args, "limit", 100)? as usize)?})),
            "status" => answer(board.status()?),
            "get_task" => {
                let id = need(args, "id")?;
                answer(board.task(id)?.ok_or_else(|| BoardError::Refused(format!("no task {id:?}")))?)
            }
            other => Err(Failed::NoTool(other.to_string())),
        }
    };
    let mut tries = 0;
    loop {
        match once() {
            // Another server committed first, and nothing of this call
            // was written: the tenant is loaded again, and the call too.
            Err(Failed::Board(BoardError::Tenant(TenantError::NotWriter))) if tries < 2 => tries += 1,
            r => return r,
        }
    }
}

fn error(id: &Json, code: i64, message: impl Into<String>) -> Json {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message.into()}})
}

fn ok(id: &Json, result: Json) -> Json {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

/// An MCP message's answer; none for a notification or a response.
fn handle(tenant: &Tenant, agent: &str, m: &Json) -> Option<Json> {
    if !m.is_object() {
        return Some(error(&Json::Null, -32600, "a message is a JSON object"));
    }
    let id = m.get("id").cloned();
    let method = match m.get("method").and_then(|v| v.as_str()) {
        Some(x) => x,
        None => return id.filter(|_| m.get("result").is_none() && m.get("error").is_none()).map(|id| error(&id, -32600, "a request names its method")),
    };
    let id = id?;
    if m.get("jsonrpc").and_then(|v| v.as_str()) != Some("2.0") {
        return Some(error(&id, -32600, "jsonrpc must be \"2.0\""));
    }
    let params = m.get("params").cloned().unwrap_or(Json::Null);
    Some(match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(|v| v.as_str()).unwrap_or(crate::mcp::VERSIONS[0]);
            let version = crate::mcp::VERSIONS.iter().find(|v| **v == asked).copied().unwrap_or(crate::mcp::VERSIONS[0]);
            ok(&id, json!({
                "protocolVersion": version,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "chloedb-blackboard", "title": format!("Blackboard {}", tenant.id()), "version": env!("CARGO_PKG_VERSION")},
                "instructions": format!("{INSTRUCTIONS} You are {agent:?} here."),
            }))
        }
        "ping" | "logging/setLevel" => ok(&id, json!({})),
        "tools/list" => ok(&id, tools_json()),
        "resources/list" => ok(&id, json!({"resources": []})),
        "resources/templates/list" => ok(&id, json!({"resourceTemplates": []})),
        "prompts/list" => ok(&id, json!({"prompts": []})),
        "tools/call" => {
            let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            match call(tenant, agent, name, &args) {
                Ok(j) => ok(&id, json!({"content": [{"type": "text", "text": j.to_string()}], "isError": false})),
                Err(e @ (Failed::NoTool(_) | Failed::Arguments(_))) => error(&id, -32602, e.to_string()),
                Err(e) => ok(&id, json!({"content": [{"type": "text", "text": e.to_string()}], "isError": true})),
            }
        }
        other => error(&id, -32601, format!("no method {other:?}")),
    })
}

/// `POST …/mcp`'s answer to `body`, one message or a batch: `None` when
/// it held only notifications (202, no body), and whether the body was
/// JSON at all (400 when not).
pub fn mcp(tenant: &Tenant, agent: &str, body: &[u8]) -> (bool, Option<Json>) {
    let msg: Json = match serde_json::from_slice(body) {
        Ok(m) => m,
        Err(e) => return (false, Some(error(&Json::Null, -32700, format!("parse error: {e}")))),
    };
    (true, match msg {
        Json::Array(batch) if batch.is_empty() => Some(error(&Json::Null, -32600, "an empty batch")),
        Json::Array(batch) => {
            let out: Vec<Json> = batch.iter().filter_map(|m| handle(tenant, agent, m)).collect();
            (!out.is_empty()).then_some(Json::Array(out))
        }
        m => handle(tenant, agent, &m),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_a_schema_that_names_what_it_requires() {
        let listed = tools_json();
        let tools = listed["tools"].as_array().unwrap();
        assert_eq!(tools.len(), TOOLS.len());
        for t in tools {
            let properties = t["inputSchema"]["properties"].as_object().unwrap();
            for r in t["inputSchema"]["required"].as_array().unwrap() {
                assert!(properties.contains_key(r.as_str().unwrap()), "{}: {r}", t["name"]);
            }
            assert!(t["description"].as_str().unwrap().len() > 40);
        }
    }
}
