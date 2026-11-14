//! The state database: `<data-dir>/workbench.chloedb`, a ChloeDB file with
//! two tables, `agents` (a row per session, replaced on every change) and
//! `events` (a row per hook call, spawn, input and exit). The server is its
//! only writer.

use std::path::Path;
use std::sync::Arc;

use chloedb::db::{Db, SqlResult};
use chloedb::query::Params;
use chloedb::storage::{DbOptions, MergerOptions};
use chloedb::Value;
use serde_json::{json, Value as Json};

pub type Result<T> = std::result::Result<T, String>;

const AGENTS: &str = "CREATE TABLE IF NOT EXISTS agents (
  id KEYWORD PRIMARY KEY,
  name KEYWORD,
  repo KEYWORD,
  cwd KEYWORD,
  branch KEYWORD,
  task TEXT,
  parent KEYWORD,
  depth INT,
  state KEYWORD,
  state_since TIMESTAMP,
  waiting_for TEXT,
  last_event TEXT,
  session_id KEYWORD,
  pid INT,
  exit_code INT,
  model KEYWORD,
  created TIMESTAMP,
  ended TIMESTAMP
)";

const EVENTS: &str = "CREATE TABLE IF NOT EXISTS events (
  id KEYWORD PRIMARY KEY,
  seq INT,
  ts TIMESTAMP,
  agent KEYWORD,
  kind KEYWORD,
  tool KEYWORD,
  summary TEXT,
  data TEXT
)";

const AGENT_COLS: &str = "id, name, repo, cwd, branch, task, parent, depth, state, state_since, waiting_for, last_event, session_id, pid, exit_code, model, created, ended";
const EVENT_COLS: &str = "id, seq, ts, agent, kind, tool, summary, data";

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> i64 {
    chloedb::timestamp::now() / 1000
}

/// ISO 8601 text of a time in milliseconds.
pub fn iso(ms: i64) -> String {
    chloedb::timestamp::format(ms * 1000)
}

/// An agent's row.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub repo: String,
    pub cwd: String,
    pub branch: Option<String>,
    pub task: Option<String>,
    pub parent: Option<String>,
    pub depth: i64,
    pub state: String,
    pub state_since: i64,
    pub waiting_for: Option<String>,
    pub last_event: Option<String>,
    pub session_id: Option<String>,
    pub pid: Option<i64>,
    pub exit_code: Option<i64>,
    pub model: Option<String>,
    pub created: i64,
    pub ended: Option<i64>,
}

impl Agent {
    /// The row as JSON (times as ISO 8601 text), plus `running` and
    /// `children`, as the API answers it.
    pub fn to_json(&self, running: bool, children: Vec<String>) -> Json {
        json!({
            "id": self.id,
            "name": self.name,
            "repo": self.repo,
            "cwd": self.cwd,
            "branch": self.branch,
            "task": self.task,
            "parent": self.parent,
            "depth": self.depth,
            "state": self.state,
            "state_since": iso(self.state_since),
            "waiting_for": self.waiting_for,
            "last_event": self.last_event,
            "session_id": self.session_id,
            "pid": self.pid,
            "exit_code": self.exit_code,
            "model": self.model,
            "created": iso(self.created),
            "ended": self.ended.map(iso),
            "running": running,
            "children": children,
        })
    }
}

/// An event's row.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Event {
    pub id: String,
    pub seq: i64,
    pub ts: i64,
    pub agent: String,
    pub kind: String,
    pub tool: Option<String>,
    pub summary: Option<String>,
    pub data: Option<String>,
}

impl Event {
    pub fn to_json(&self, with_data: bool) -> Json {
        let mut v = json!({
            "id": self.id,
            "seq": self.seq,
            "ts": iso(self.ts),
            "agent": self.agent,
            "kind": self.kind,
            "tool": self.tool,
            "summary": self.summary,
        });
        if with_data {
            v["data"] = json!(self.data);
        }
        v
    }
}

pub struct StateDb {
    db: Arc<Db>,
}

fn e(err: chloedb::Error) -> String {
    err.to_string()
}

fn opt_str(v: &Option<String>) -> Value {
    v.as_ref().map_or(Value::Null, |s| Value::Str(s.clone()))
}

fn opt_int(v: Option<i64>) -> Value {
    v.map_or(Value::Null, Value::Int)
}

fn ts(ms: i64) -> Value {
    Value::Timestamp(ms * 1000)
}

fn get_str(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::Str(s) => Some(s.clone()),
        Value::List(l) => l.first().and_then(get_str),
        Value::Int(i) => Some(i.to_string()),
        _ => None,
    }
}

fn get_int(v: &Value) -> Option<i64> {
    match v {
        Value::Int(i) => Some(*i),
        Value::Timestamp(t) => Some(*t),
        Value::Float(f) => Some(*f as i64),
        Value::Bool(b) => Some(*b as i64),
        _ => None,
    }
}

fn get_ms(v: &Value) -> Option<i64> {
    match v {
        Value::Timestamp(t) => Some(t.div_euclid(1000)),
        Value::Int(t) => Some(t.div_euclid(1000)),
        Value::Str(s) => chloedb::timestamp::parse(s).map(|t| t.div_euclid(1000)),
        _ => None,
    }
}

impl StateDb {
    /// Opens `<dir>/workbench.chloedb`, creating it and its tables if needed.
    pub fn open(dir: &Path) -> Result<StateDb> {
        std::fs::create_dir_all(dir).map_err(|err| format!("cannot create {}: {err}", dir.display()))?;
        let path = dir.join("workbench.chloedb");
        let opts = DbOptions {
            // A crash of this process loses nothing without it; a power
            // failure may lose the last moments of a session's events.
            sync: false,
            ..DbOptions::default()
        };
        let db = if path.exists() { Db::open(&path, opts) } else { Db::create_empty(&path, opts) }.map_err(|err| format!("cannot open {}: {err}", path.display()))?;
        db.sql(AGENTS, &Params::new()).map_err(e)?;
        db.sql(EVENTS, &Params::new()).map_err(e)?;
        let db = Arc::new(db);
        db.spawn_flusher();
        db.spawn_merger(MergerOptions { threads: 1, ..MergerOptions::default() });
        Ok(StateDb { db })
    }

    fn rows(&self, sql: &str, params: &Params) -> Result<Vec<Vec<Value>>> {
        match self.db.sql(sql, params).map_err(e)? {
            SqlResult::Rows(r) => Ok(r.rows),
            SqlResult::Written(_) => Ok(Vec::new()),
        }
    }

    /// Writes an agent's row, replacing the one of its id.
    pub fn put_agent(&self, a: &Agent) -> Result<()> {
        let p = Params::new()
            .set("id", Value::Str(a.id.clone()))
            .set("name", Value::Str(a.name.clone()))
            .set("repo", Value::Str(a.repo.clone()))
            .set("cwd", Value::Str(a.cwd.clone()))
            .set("branch", opt_str(&a.branch))
            .set("task", opt_str(&a.task))
            .set("parent", opt_str(&a.parent))
            .set("depth", Value::Int(a.depth))
            .set("state", Value::Str(a.state.clone()))
            .set("state_since", ts(a.state_since))
            .set("waiting_for", opt_str(&a.waiting_for))
            .set("last_event", opt_str(&a.last_event))
            .set("session_id", opt_str(&a.session_id))
            .set("pid", opt_int(a.pid))
            .set("exit_code", opt_int(a.exit_code))
            .set("model", opt_str(&a.model))
            .set("created", ts(a.created))
            .set("ended", a.ended.map_or(Value::Null, ts));
        let names: Vec<String> = AGENT_COLS.split(", ").map(|c| format!(":{c}")).collect();
        let sql = format!("INSERT OR REPLACE INTO agents ({AGENT_COLS}) VALUES ({})", names.join(", "));
        self.db.sql(&sql, &p).map_err(e)?;
        Ok(())
    }

    /// Every agent, oldest first.
    pub fn agents(&self) -> Result<Vec<Agent>> {
        let rows = self.rows(&format!("SELECT {AGENT_COLS} FROM agents ORDER BY created ASC LIMIT 1000000"), &Params::new())?;
        Ok(rows
            .iter()
            .map(|r| Agent {
                id: get_str(&r[0]).unwrap_or_default(),
                name: get_str(&r[1]).unwrap_or_default(),
                repo: get_str(&r[2]).unwrap_or_default(),
                cwd: get_str(&r[3]).unwrap_or_default(),
                branch: get_str(&r[4]),
                task: get_str(&r[5]),
                parent: get_str(&r[6]),
                depth: get_int(&r[7]).unwrap_or(0),
                state: get_str(&r[8]).unwrap_or_default(),
                state_since: get_ms(&r[9]).unwrap_or(0),
                waiting_for: get_str(&r[10]),
                last_event: get_str(&r[11]),
                session_id: get_str(&r[12]),
                pid: get_int(&r[13]),
                exit_code: get_int(&r[14]),
                model: get_str(&r[15]),
                created: get_ms(&r[16]).unwrap_or(0),
                ended: get_ms(&r[17]),
            })
            .collect())
    }

    /// Writes an event's row.
    pub fn add_event(&self, ev: &Event) -> Result<()> {
        let p = Params::new()
            .set("id", Value::Str(ev.id.clone()))
            .set("seq", Value::Int(ev.seq))
            .set("ts", ts(ev.ts))
            .set("agent", Value::Str(ev.agent.clone()))
            .set("kind", Value::Str(ev.kind.clone()))
            .set("tool", opt_str(&ev.tool))
            .set("summary", opt_str(&ev.summary))
            .set("data", opt_str(&ev.data));
        let sql = format!("INSERT OR REPLACE INTO events ({EVENT_COLS}) VALUES (:id, :seq, :ts, :agent, :kind, :tool, :summary, :data)");
        self.db.sql(&sql, &p).map_err(e)?;
        Ok(())
    }

    /// The events of `agent` after `after`, oldest first.
    pub fn events(&self, agent: &str, after: i64, limit: usize) -> Result<Vec<Event>> {
        let p = Params::new().set("agent", Value::Str(agent.to_string())).set("after", Value::Int(after));
        let rows = self.rows(&format!("SELECT {EVENT_COLS} FROM events WHERE agent = :agent AND seq > :after ORDER BY seq ASC LIMIT {limit}"), &p)?;
        Ok(rows.iter().map(event_of).collect())
    }

    /// The highest event number written, or 0.
    pub fn last_seq(&self) -> Result<i64> {
        let rows = self.rows("SELECT seq FROM events ORDER BY seq DESC LIMIT 1", &Params::new())?;
        Ok(rows.first().and_then(|r| get_int(&r[0])).unwrap_or(0))
    }

    /// Writes what is in memory to the file.
    pub fn close(&self) {
        let _ = self.db.close();
    }
}

fn event_of(r: &Vec<Value>) -> Event {
    Event {
        id: get_str(&r[0]).unwrap_or_default(),
        seq: get_int(&r[1]).unwrap_or(0),
        ts: get_ms(&r[2]).unwrap_or(0),
        agent: get_str(&r[3]).unwrap_or_default(),
        kind: get_str(&r[4]).unwrap_or_default(),
        tool: get_str(&r[5]),
        summary: get_str(&r[6]),
        data: get_str(&r[7]),
    }
}
