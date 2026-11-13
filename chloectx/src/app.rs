//! `chloectx app`: ChloeCtx as an app of a ChloeDB cluster, as the crawler
//! and Sheets are. Its memory lives in the cluster, one table per *space*
//! (`ctx_<space>`, the records of [`crate::record`], [`Store::open_remote`]),
//! and the spaces themselves in `ctx_spaces`. The command center shows it
//! (`chloedb-server --app chloectx=URL`, which loads `ui/view.js` from
//! here) with two pages:
//!
//! * **Context**: any agent writes notes and exchanges into a space and
//!   asks it for context within a token budget, answered by search alone
//!   ([`context::build`]); the page shows what went into an answer and why.
//! * **Seen before**: incidents (symptom, cause, fix) kept in the space
//!   `incidents`; the cluster's firing alerts each with the closest past
//!   incidents, which can be handed to the on-call agent.
//!
//! A space's page also makes apps from it ([`from_context`]): suggested from
//! its notes, built by the cluster's app engine, loaded with the notes the
//! person approved.
//!
//! Model calls (distilling exchanges into notes, drafting incidents from a
//! postmortem or an on-call run) go to Anthropic's API when
//! `ANTHROPIC_API_KEY` is set, else to the `claude` CLI if there is one;
//! everything else works without a model.
//!
//! The API (JSON; every call needs `--token` when one is set):
//!
//! | call | body | answer |
//! |---|---|---|
//! | `GET /app.json`, `GET /ui/view.js` | | the manifest and the page, for the command center |
//! | `GET /v1/info` | | the server, the embedder, the model, the spaces' count |
//! | `GET /v1/spaces`, `POST /v1/spaces` | `{name, title?, description?}` | `{spaces}` with their counts; the new space |
//! | `POST /v1/spaces/:s/notes` | `{kind, text, replaces?, entities?, session?, ts?}` | the note; `replaces` marks the older one superseded |
//! | `GET /v1/spaces/:s/notes?kind=&limit=&all=` | | `{notes}`, the newest first (live ones unless `all=1`) |
//! | `GET /v1/spaces/:s/notes/:id` | | the note and its history: what it replaced and what replaced it |
//! | `DELETE /v1/spaces/:s/notes/:id` | | the note, retired (superseded by nothing) |
//! | `POST /v1/spaces/:s/exchanges` | `{user, assistant, session?, ts?, distill?}` | the two messages stored; notes follow when a model is set |
//! | `GET /v1/spaces/:s/ask?q=&budget=`, `POST` the same | `{question, budget?}` | `{context, tokens, ms, items, left_out}` |
//! | `GET /v1/incidents`, `POST /v1/incidents` | `{title, symptom, cause, fix, alert?, when?, replaces?}` | `{incidents}`; the incident |
//! | `GET /v1/incidents/firing` | | the cluster's firing (and pending) alerts, each with its closest incidents and the context for the on-call agent |
//! | `GET /v1/incidents/match?q=` | | the incidents closest to a description |
//! | `POST /v1/incidents/draft` | `{text}` or `{run}` | incidents a model drafted from a postmortem or an on-call run (not stored) |
//! | `POST /v1/incidents/handoff` | `{alert}` | the on-call agent's run, started with the closest incidents |
//! | `POST /v1/spaces/:s/apps/suggest` | `{n?}` | two or three apps the space could become (one model call; [`from_context`]) |
//! | `GET /v1/spaces/:s/apps/estimate` | | what a build costs here, measured, and what the app engine may still spend |
//! | `POST /v1/spaces/:s/apps/items` | `{kinds}` | the notes a build of those kinds would be loaded with |
//! | `POST /v1/spaces/:s/apps`, `GET` | `{title, description, kinds, exclude?, confirm}` | a build by the app engine, loaded with the approved notes; the builds |
//! | `POST /v1/spaces/:s/apps/:build/open` | | a one-time link into the studio, at the built app |

use std::collections::{HashMap, HashSet, VecDeque};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value as Json};

use crate::cluster::{self, Cluster};
use crate::context::{self, ContextParams};
use crate::embed::Embedder;
use crate::llm::{self, Reply};
use crate::recall::{self, RankParams};
use crate::record::{Draft, Kind, Link, Record};
use crate::store::{Filter, Store};
use crate::ui::{read_request, respond, respond_json, same, Request};
use crate::{distill, err, Result};

mod from_context;

pub const VIEW: &str = include_str!("../ui/view.js");

/// The spaces' table.
pub const SPACES: &str = "ctx_spaces";
const CREATE_SPACES: &str = "CREATE TABLE IF NOT EXISTS ctx_spaces (name KEYWORD PRIMARY KEY, title TEXT, description TEXT, kind KEYWORD, created INT)";

/// The space Seen before keeps incidents in.
pub const INCIDENTS: &str = "incidents";

/// The least cosine similarity of an incident that shares no word with an
/// alert, for it to count as one like it (one that shares a word needs 0.1
/// less), and it must be at least half as close as the closest.
pub const MIN_MATCH: f64 = 0.3;

/// The note kinds a space shows.
const NOTE_KINDS: [Kind; 5] = [Kind::Decision, Kind::Preference, Kind::Fact, Kind::Todo, Kind::Note];

/// Layers, for the command center's navigation.
const ICON: &str = r#"<path d="m12 3 9 4.5-9 4.5-9-4.5z"/><path d="m3 12 9 4.5 9-4.5"/><path d="m3 16.5 9 4.5 9-4.5"/>"#;

/// How the app reaches a model.
pub enum Model {
    /// Anthropic's API with a key.
    Api { key: String, model: String },
    /// The `claude` CLI.
    Cli { command: String, model: String },
    Off,
}

impl Model {
    /// `ANTHROPIC_API_KEY` (model `CHLOECTX_APP_MODEL`, default Haiku), else
    /// the `claude` CLI on the PATH (model `CHLOECTX_DISTILL_MODEL`), else
    /// none. `CHLOECTX_APP_MODEL=off` turns model calls off.
    pub fn from_env() -> Model {
        let model = std::env::var("CHLOECTX_APP_MODEL").unwrap_or_default();
        if model == "off" {
            return Model::Off;
        }
        if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
            if !key.trim().is_empty() {
                return Model::Api { key, model: if model.is_empty() { "claude-haiku-5-5".into() } else { model } };
            }
        }
        let on_path = std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join("claude").is_file()));
        if on_path {
            let m = std::env::var("CHLOECTX_DISTILL_MODEL").unwrap_or_else(|_| if model.is_empty() { "haiku".into() } else { model });
            return Model::Cli { command: "claude".into(), model: m };
        }
        Model::Off
    }

    fn json(&self) -> Json {
        match self {
            Model::Api { model, .. } => json!({"how": "api", "model": model}),
            Model::Cli { model, .. } => json!({"how": "cli", "model": model}),
            Model::Off => json!({"how": "off"}),
        }
    }

    pub fn complete(&self, system: &str, prompt: &str) -> Result<Reply> {
        match self {
            Model::Api { key, model } => llm::complete_api(key, model, system, prompt, Duration::from_secs(120)),
            Model::Cli { command, model } => llm::complete(&llm::Cli { command, model, timeout: Duration::from_secs(120), isolated: true }, system, prompt),
            Model::Off => Err(err("no model: start chloectx app with ANTHROPIC_API_KEY set, or with the claude CLI on its PATH")),
        }
    }
}

pub struct Options {
    pub host: String,
    pub port: u16,
    /// The chloedb-server (a coordinator) the spaces live in.
    pub server: String,
    /// Its key, for this app's reads and writes.
    pub key: Option<String>,
    /// The token callers must send (chloedb-server sends its app token).
    pub token: Option<String>,
    /// Serve any caller on an address other than loopback, with no token.
    pub insecure: bool,
    /// Embedder spec ([`crate::embed::open`]).
    pub embedder: String,
    /// Show this machine's Claude Code stores (under this ChloeCtx home):
    /// their receipts, kept snapshots and blame. Off unless asked for, since
    /// a store is a transcript of its sessions and the command center's
    /// users would read it.
    pub local: Option<std::path::PathBuf>,
}

/// Distillations in the background, for `/v1/info`.
#[derive(Default)]
struct Jobs {
    /// Workers running (at most [`DISTILLERS`]).
    running: usize,
    /// Exchanges waiting for a worker (at most [`DISTILL_QUEUE`]).
    queue: VecDeque<Distill>,
    done: u64,
    notes: u64,
    failed: u64,
    /// Exchanges stored but not distilled: the queue was full.
    skipped: u64,
    last_error: Option<String>,
}

/// An exchange to write notes down from.
struct Distill {
    space: String,
    first: Record,
    user: String,
    assistant: String,
}

/// Distillations at once: each is a model call, billed to the app's key.
const DISTILLERS: usize = 4;
/// Exchanges that may wait for one; past that an exchange is stored and
/// not distilled, and `/v1/info` counts it under `skipped`.
const DISTILL_QUEUE: usize = 256;
/// Requests served at once; one more is answered 503.
const CONNECTIONS: usize = 64;

pub struct App {
    pub cluster: Arc<Cluster>,
    embedder: Arc<dyn Embedder>,
    stores: Mutex<HashMap<String, Arc<Store>>>,
    pub model: Model,
    token: Option<String>,
    jobs: Mutex<Jobs>,
    /// The ChloeCtx home whose stores the Receipts and Blame pages read.
    local: Option<std::path::PathBuf>,
}

/// An error with the HTTP status it answers with.
pub struct Fail(pub u16, pub String);

impl From<crate::Error> for Fail {
    fn from(e: crate::Error) -> Fail {
        Fail(502, e.0)
    }
}

type Answer = std::result::Result<Json, Fail>;

fn bad(m: impl Into<String>) -> Fail {
    Fail(400, m.into())
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// The table a space's records are in.
pub fn table_of(space: &str) -> String {
    format!("ctx_{space}")
}

/// A note kind by the name callers give it.
fn note_kind(name: &str) -> Option<(Kind, &'static str)> {
    Some(match name {
        "decision" => (Kind::Decision, "noted"),
        "rule" | "preference" => (Kind::Preference, "noted"),
        "fact" => (Kind::Fact, "noted"),
        "todo" | "requirement" => (Kind::Todo, "planned"),
        "note" => (Kind::Note, "noted"),
        _ => return None,
    })
}

/// A note kind's name for callers.
fn kind_name(r: &Record) -> &'static str {
    name_of_kind(r.kind)
}

fn name_of_kind(k: Kind) -> &'static str {
    match k {
        Kind::Preference => "rule",
        Kind::Todo => "todo",
        Kind::Prompt => "user",
        Kind::Answer => "assistant",
        k => k.as_str(),
    }
}

fn record_json(r: &Record) -> Json {
    json!({
        "id": r.id, "kind": kind_name(r), "tag": r.tag, "text": r.text, "ts": r.ts, "session": r.session, "seq": r.seq,
        "supersedes": r.linked("supersedes").collect::<Vec<_>>(), "superseded_by": r.linked("superseded_by").collect::<Vec<_>>(),
    })
}

/// About four characters a token: what a context costs a reader.
pub fn tokens(chars: usize) -> usize {
    chars.div_ceil(4)
}

/// An incident, as its note's text holds it.
#[derive(Default, Debug, PartialEq)]
pub struct Incident {
    pub title: String,
    pub symptom: String,
    pub cause: String,
    pub fix: String,
    pub alert: String,
}

impl Incident {
    pub fn text(&self) -> String {
        let mut t = format!("{}\nSymptom: {}\nCause: {}\nFix: {}", one_line(&self.title), one_line(&self.symptom), one_line(&self.cause), one_line(&self.fix));
        if !self.alert.trim().is_empty() {
            t += &format!("\nAlert: {}", one_line(&self.alert));
        }
        t
    }

    pub fn parse(text: &str) -> Incident {
        let mut i = Incident::default();
        for (n, line) in text.lines().enumerate() {
            let field = |p: &str| line.strip_prefix(p).map(|v| v.trim().to_string());
            if let Some(v) = field("Symptom:") {
                i.symptom = v;
            } else if let Some(v) = field("Cause:") {
                i.cause = v;
            } else if let Some(v) = field("Fix:") {
                i.fix = v;
            } else if let Some(v) = field("Alert:") {
                i.alert = v;
            } else if n == 0 {
                i.title = line.trim().to_string();
            }
        }
        i
    }

    fn of_json(j: &Json) -> Incident {
        let s = |k: &str| j[k].as_str().unwrap_or("").trim().to_string();
        Incident { title: s("title"), symptom: s("symptom"), cause: s("cause"), fix: s("fix"), alert: s("alert") }
    }

    fn json(&self) -> Json {
        json!({"title": self.title, "symptom": self.symptom, "cause": self.cause, "fix": self.fix, "alert": self.alert})
    }
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

const DRAFT_SYSTEM: &str = "You read what was written during or after an incident in a database cluster (a postmortem, an incident thread, an on-call agent's report) and write down each incident it describes, for the people who will meet the same symptom later. Output JSON only.";

const DRAFT_PROMPT: &str = r#"Here is the text:

<text>
{text}
</text>

Write each distinct incident it describes as one JSON object on its own line, with these fields:
- "title": a short name for what happened (at most 12 words), in plain words.
- "symptom": what was seen first, as someone watching dashboards and alerts would see it (the metric, the shard or copy, the numbers if given).
- "cause": why it happened, as the text establishes it. If the text does not establish a cause, write "not found".
- "fix": what made it stop, and what was changed so it does not come back. If nothing fixed it yet, write "not fixed".
- "alert": the alert rule's name if the text names one, else "".

Use only what the text says. Do not invent numbers, names or causes. If the text describes no incident, output nothing."#;

impl App {
    pub fn new(cluster: Arc<Cluster>, embedder: Arc<dyn Embedder>, model: Model, token: Option<String>) -> Result<App> {
        cluster.sql(CREATE_SPACES)?;
        Ok(App { cluster, embedder, stores: Mutex::new(HashMap::new()), model, token, jobs: Mutex::new(Jobs::default()), local: None })
    }

    fn space_row(&self, name: &str) -> Result<Option<Json>> {
        let rs = self.cluster.query(&format!("SELECT name, title, description, kind, created FROM {SPACES} WHERE name = {} LIMIT 1", cluster::lit(name)), &Default::default())?;
        Ok(rs.rows.first().map(|r| {
            let s = |i: usize| r.get(i).and_then(|v| v.as_str()).unwrap_or("").to_string();
            json!({"name": s(0), "title": s(1), "description": s(2), "kind": s(3), "created": r.get(4).and_then(|v| v.as_i64()).unwrap_or(0), "table": table_of(&s(0))})
        }))
    }

    /// Makes a space (its row and its table); one that exists is left as it is.
    pub fn create_space(&self, name: &str, title: &str, description: &str, kind: &str) -> Result<Json> {
        cluster::check_name(name)?;
        if name == "spaces" {
            return Err(err("\"spaces\" names the table of spaces"));
        }
        if let Some(row) = self.space_row(name)? {
            return Ok(row);
        }
        let title = if title.trim().is_empty() { name } else { title.trim() };
        self.cluster.write(vec![json!({"op": "upsert", "table": SPACES, "doc": {"name": name, "title": title, "description": description.trim(), "kind": kind, "created": now()}})])?;
        self.store(name)?;
        Ok(self.space_row(name)?.unwrap_or(Json::Null))
    }

    /// A space's store, opened once.
    pub fn store(&self, name: &str) -> Result<Arc<Store>> {
        cluster::check_name(name)?;
        if let Some(s) = self.stores.lock().unwrap_or_else(|e| e.into_inner()).get(name) {
            return Ok(s.clone());
        }
        let s = Arc::new(Store::open_remote(self.cluster.clone(), &table_of(name), self.embedder.clone())?);
        Ok(self.stores.lock().unwrap_or_else(|e| e.into_inner()).entry(name.to_string()).or_insert(s).clone())
    }

    /// An existing space's store, or 404.
    fn space(&self, name: &str) -> std::result::Result<Arc<Store>, Fail> {
        if !cluster::valid_name(name) || self.space_row(name)?.is_none() {
            return Err(Fail(404, format!("no space {name:?}")));
        }
        Ok(self.store(name)?)
    }

    pub fn spaces(&self) -> Result<Json> {
        let rs = self.cluster.query(&format!("SELECT name, title, description, kind, created FROM {SPACES} ORDER BY name ASC LIMIT 1000"), &Default::default())?;
        let mut out = Vec::new();
        for r in &rs.rows {
            let s = |i: usize| r.get(i).and_then(|v| v.as_str()).unwrap_or("").to_string();
            let name = s(0);
            let counts = self.cluster.query(&format!("SELECT kind, COUNT(*) FROM {} GROUP BY kind LIMIT 50", table_of(&name)), &Default::default()).map(|c| c.rows).unwrap_or_default();
            let mut by: serde_json::Map<String, Json> = serde_json::Map::new();
            let mut total = 0;
            for c in &counts {
                if let (Some(k), Some(n)) = (c.first().and_then(|v| v.as_str()), c.get(1).and_then(|v| v.as_i64())) {
                    total += n;
                    by.insert(k.to_string(), json!(n));
                }
            }
            out.push(json!({"name": name, "title": s(1), "description": s(2), "kind": s(3), "created": r.get(4).and_then(|v| v.as_i64()).unwrap_or(0), "table": table_of(&name), "records": total, "by_kind": by}));
        }
        Ok(json!({"spaces": out}))
    }

    /// Stores a note; with `replaces`, the older note is marked superseded
    /// and linked to it both ways.
    pub fn remember(&self, space: &str, b: &Json) -> Answer {
        let store = self.space(space)?;
        let text = b["text"].as_str().unwrap_or("").trim();
        if text.is_empty() {
            return Err(bad("a note needs \"text\""));
        }
        let (kind, tag) = note_kind(b["kind"].as_str().unwrap_or("note")).ok_or_else(|| bad("kind is decision, rule, fact, todo or note"))?;
        let tag = b["tag"].as_str().filter(|t| !t.is_empty() && *t != "superseded").unwrap_or(tag);
        let entities: Vec<String> = b["entities"].as_array().into_iter().flatten().filter_map(|e| e.as_str().map(str::to_string)).take(12).collect();
        let mut d = Draft::new(kind, text).tag(tag).entities(entities);
        let replaces = b["replaces"].as_str().filter(|s| !s.is_empty());
        let old = match replaces {
            Some(id) => Some(store.get(&[id.to_string()])?.pop().ok_or_else(|| Fail(404, format!("no note {id} in {space}")))?),
            None => None,
        };
        if let Some(o) = &old {
            d = d.link("supersedes", o.id.clone());
        }
        let r = self.append(&store, b, vec![d])?.pop().ok_or_else(|| Fail(500, "the note was not stored".into()))?;
        if let Some(mut o) = old {
            o.tag = "superseded".into();
            o.links.push(Link::new("superseded_by", r.id.clone()));
            store.insert(&[o])?;
        }
        Ok(record_json(&r))
    }

    fn append(&self, store: &Store, b: &Json, drafts: Vec<Draft>) -> Result<Vec<Record>> {
        let session = b["session"].as_str().filter(|s| !s.is_empty()).unwrap_or("api");
        let ts = b["ts"].as_i64().filter(|t| *t > 0).unwrap_or_else(now);
        store.append(session, store.last_seq() + 1, ts, drafts)
    }

    /// Retires a note: superseded by nothing.
    pub fn retire(&self, space: &str, id: &str) -> Answer {
        let store = self.space(space)?;
        let mut r = store.get(&[id.to_string()])?.pop().ok_or_else(|| Fail(404, format!("no note {id} in {space}")))?;
        r.tag = "superseded".into();
        store.insert(&[r.clone()])?;
        Ok(record_json(&r))
    }

    /// Notes, the newest first: the live ones, or with `all` every one.
    pub fn notes(&self, space: &str, kind: Option<&str>, limit: usize, all: bool) -> Answer {
        let store = self.space(space)?;
        let kinds: Vec<Kind> = match kind {
            Some(k) if !k.is_empty() => vec![note_kind(k).ok_or_else(|| bad("kind is decision, rule, fact, todo or note"))?.0],
            _ => NOTE_KINDS.to_vec(),
        };
        let want = limit.clamp(1, 1000);
        let got = store.latest(&kinds, if all { want } else { (want * 4).min(cluster::WINDOW) }, &Filter::default())?;
        let notes: Vec<Json> = got.iter().filter(|r| all || (r.tag != "superseded" && r.tag != "done")).take(want).map(record_json).collect();
        Ok(json!({"notes": notes}))
    }

    /// A note and its history: the notes it replaced, back to the first,
    /// and the ones that replaced it.
    pub fn history(&self, space: &str, id: &str) -> Answer {
        let store = self.space(space)?;
        let note = store.get(&[id.to_string()])?.pop().ok_or_else(|| Fail(404, format!("no note {id} in {space}")))?;
        let walk = |start: &Record, label: &str| -> Result<Vec<Record>> {
            let mut out = Vec::new();
            let mut seen: HashSet<String> = HashSet::from([start.id.clone()]);
            let mut next: Vec<String> = start.linked(label).map(String::from).collect();
            while !next.is_empty() && out.len() < 50 {
                let got = store.get(&next)?;
                next = Vec::new();
                for r in got {
                    if seen.insert(r.id.clone()) {
                        next.extend(r.linked(label).map(String::from));
                        out.push(r);
                    }
                }
            }
            out.sort_by_key(|r| std::cmp::Reverse(r.seq));
            Ok(out)
        };
        let earlier = walk(&note, "supersedes")?;
        let later = walk(&note, "superseded_by")?;
        Ok(json!({"note": record_json(&note), "earlier": earlier.iter().map(record_json).collect::<Vec<_>>(), "later": later.iter().map(record_json).collect::<Vec<_>>()}))
    }

    /// Stores an exchange (the user's message and the reply) and, with a
    /// model, distills it into notes in the background.
    pub fn exchange(self: &Arc<Self>, space: &str, b: &Json) -> Answer {
        let store = self.space(space)?;
        let user = b["user"].as_str().unwrap_or("").trim().to_string();
        let assistant = b["assistant"].as_str().unwrap_or("").trim().to_string();
        if user.is_empty() && assistant.is_empty() {
            return Err(bad("an exchange needs \"user\" or \"assistant\""));
        }
        let mut drafts = Vec::new();
        if !user.is_empty() {
            drafts.push(Draft::new(Kind::Prompt, user.clone()));
        }
        if !assistant.is_empty() {
            drafts.push(Draft::new(Kind::Answer, assistant.clone()));
        }
        let stored = self.append(&store, b, drafts)?;
        let wants = b["distill"].as_bool().unwrap_or(true) && !matches!(self.model, Model::Off);
        let distilling = wants && self.queue_distill(Distill { space: space.to_string(), first: stored[0].clone(), user, assistant });
        let mut out = json!({"stored": stored.iter().map(record_json).collect::<Vec<_>>(), "distilling": distilling});
        if wants && !distilling {
            out["skipped"] = json!(format!("{DISTILL_QUEUE} exchanges are already waiting to be distilled: this one is stored, and no notes are written from it"));
        }
        Ok(out)
    }

    /// Queues an exchange for a distiller, starting one if fewer than
    /// [`DISTILLERS`] run; false when the queue is full.
    fn queue_distill(self: &Arc<Self>, d: Distill) -> bool {
        let mut j = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        if j.queue.len() >= DISTILL_QUEUE {
            j.skipped += 1;
            return false;
        }
        j.queue.push_back(d);
        if j.running < DISTILLERS {
            j.running += 1;
            let me = self.clone();
            std::thread::spawn(move || me.distiller());
        }
        true
    }

    /// One distiller: takes exchanges off the queue until it is empty.
    fn distiller(&self) {
        loop {
            let d = {
                let mut j = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
                match j.queue.pop_front() {
                    Some(d) => d,
                    None => {
                        j.running -= 1;
                        return;
                    }
                }
            };
            let r = self.distill(&d.space, &d.first, &d.user, &d.assistant);
            let mut j = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
            match r {
                Ok(n) => {
                    j.done += 1;
                    j.notes += n as u64;
                }
                Err(e) => {
                    j.failed += 1;
                    j.last_error = Some(e.0);
                }
            }
        }
    }

    /// Writes down an exchange's notes, as the daemon does after a turn.
    fn distill(&self, space: &str, first: &Record, user: &str, assistant: &str) -> Result<usize> {
        let store = self.store(space)?;
        let probe = format!("{user}\n{assistant}");
        let known: Vec<(String, Kind, String)> = recall::search(&store, &crate::llm::clip(&probe, 2000), &Filter { kinds: NOTE_KINDS.to_vec(), ..Default::default() }, &RankParams { n: 20, ..Default::default() })?
            .0
            .into_iter()
            .map(|h| h.record)
            .filter(|r| r.tag != "superseded" && r.tag != "done")
            .map(|r| (r.id, r.kind, r.text))
            .collect();
        let prompt = distill::prompt_for(user, assistant, &[], &known, first.ts);
        let reply = self.model.complete(distill::SYSTEM, &prompt)?;
        let d = distill::parse(&reply.text);
        let drafts: Vec<Draft> = d.drafts.into_iter().map(|x| x.link("from", first.id.clone())).collect();
        let n = drafts.len();
        let new = store.append(&first.session, first.turn, first.ts, drafts)?;
        // What the new notes replace, and planned work now done.
        let by_old: HashMap<String, String> = new.iter().flat_map(|r| r.linked("supersedes").map(move |o| (o.to_string(), r.id.clone()))).collect();
        let mut olds = store.get(&by_old.keys().cloned().collect::<Vec<_>>())?;
        for o in &mut olds {
            o.tag = "superseded".into();
            o.links.push(Link::new("superseded_by", by_old[&o.id].clone()));
        }
        let mut done = store.get(&d.done)?;
        for r in &mut done {
            r.tag = "done".into();
        }
        olds.extend(done);
        store.insert(&olds)?;
        Ok(n)
    }

    /// The context for `question` from a space, with what went into it and
    /// why, and what the searches found that it leaves out.
    pub fn ask(&self, space: &str, question: &str, budget: usize) -> Answer {
        let store = self.space(space)?;
        if question.trim().is_empty() {
            return Err(bad("ask needs a question (q)"));
        }
        let budget = budget.clamp(200, 50_000);
        let built = context::build(&store, question, &Filter::default(), &params_for(budget))?;
        let mut items = Vec::new();
        let mut shown: HashSet<String> = HashSet::new();
        for (r, p) in built.records.iter().zip(&built.picks) {
            shown.insert(r.id.clone());
            let mut j = record_json(r);
            j["section"] = json!(p.section);
            j["chars"] = json!(p.chars);
            j["tokens"] = json!(tokens(p.chars));
            j["bm25"] = json!(p.bm25);
            j["cos"] = json!(p.cos);
            j["replaced"] = json!(p.replaced);
            items.push(j);
        }
        // The versions shown notes replaced: in the context as a reminder
        // of what changed, not as notes of their own.
        let older: Vec<String> = built.records.iter().flat_map(|r| r.linked("supersedes").map(String::from)).filter(|id| !shown.contains(id)).collect();
        let mut left: Vec<Json> = store
            .get(&older)?
            .iter()
            .map(|r| {
                let mut j = record_json(r);
                j["why"] = json!("superseded");
                j
            })
            .collect();
        for (r, why) in &built.left_out {
            if shown.contains(&r.id) {
                continue;
            }
            let mut j = record_json(r);
            j["why"] = json!(why);
            left.push(j);
        }
        left.truncate(30);
        let chars = built.text.len();
        Ok(json!({
            "space": space, "question": question, "budget": budget, "context": built.text, "chars": chars, "tokens": tokens(chars),
            "ms": (built.ms * 100.0).round() / 100.0, "model_calls": 0, "items": items, "left_out": left,
        }))
    }

    // ---- Seen before ----

    fn incidents_store(&self) -> Result<Arc<Store>> {
        if self.space_row(INCIDENTS)?.is_none() {
            self.create_space(INCIDENTS, "Incidents", "Past incidents of this cluster: symptom, cause and fix. Seen before matches firing alerts against them.", "incidents")?;
        }
        self.store(INCIDENTS)
    }

    fn incident_json(&self, store: &Store, r: &Record) -> Result<Json> {
        let i = Incident::parse(&r.text);
        let mut j = i.json();
        j["id"] = json!(r.id);
        j["ts"] = json!(r.ts);
        j["live"] = json!(r.tag != "superseded");
        // The fixes it replaced, newest first.
        let mut earlier = Vec::new();
        let mut next: Vec<String> = r.linked("supersedes").map(String::from).collect();
        let mut seen = HashSet::from([r.id.clone()]);
        while !next.is_empty() && earlier.len() < 10 {
            let got = store.get(&next)?;
            next = Vec::new();
            for o in got {
                if seen.insert(o.id.clone()) {
                    next.extend(o.linked("supersedes").map(String::from));
                    let mut e = Incident::parse(&o.text).json();
                    e["id"] = json!(o.id);
                    e["ts"] = json!(o.ts);
                    earlier.push(e);
                }
            }
        }
        j["replaced"] = json!(earlier);
        Ok(j)
    }

    /// The live incidents, the newest first.
    pub fn incidents(&self) -> Answer {
        let store = self.incidents_store()?;
        let got = store.latest(&[Kind::Note], 1000, &Filter::default())?;
        let mut out = Vec::new();
        for r in got.iter().filter(|r| r.tag == "incident") {
            out.push(self.incident_json(&store, r)?);
        }
        Ok(json!({"incidents": out}))
    }

    /// Stores an incident; `replaces` marks an older one superseded (a
    /// better fix for the same thing).
    pub fn record_incident(&self, b: &Json) -> Answer {
        let store = self.incidents_store()?;
        let i = Incident::of_json(b);
        if i.title.is_empty() || (i.symptom.is_empty() && i.cause.is_empty() && i.fix.is_empty()) {
            return Err(bad("an incident needs a title and a symptom, cause or fix"));
        }
        let mut d = Draft::new(Kind::Note, i.text()).tag("incident");
        if !i.alert.is_empty() {
            d = d.entities(vec![i.alert.clone()]);
        }
        let old = match b["replaces"].as_str().filter(|s| !s.is_empty()) {
            Some(id) => Some(store.get(&[id.to_string()])?.pop().ok_or_else(|| Fail(404, format!("no incident {id}")))?),
            None => None,
        };
        if let Some(o) = &old {
            d = d.link("supersedes", o.id.clone());
        }
        let when = json!({"session": "incidents", "ts": b["when"].as_i64().unwrap_or(0)});
        let r = self.append(&store, &when, vec![d])?.pop().ok_or_else(|| Fail(500, "the incident was not stored".into()))?;
        if let Some(mut o) = old {
            o.tag = "superseded".into();
            o.links.push(Link::new("superseded_by", r.id.clone()));
            store.insert(&[o])?;
        }
        Ok(self.incident_json(&store, &r)?)
    }

    /// The incidents closest to `text`: by the same hybrid search as
    /// recall, each with its cosine similarity to the text (`match`). One
    /// that shares no word with it must be close in meaning
    /// ([`MIN_MATCH`]). With `rule`, the incidents recorded for that alert
    /// rule come first (`same_alert`).
    pub fn matches(&self, text: &str, rule: Option<&str>, n: usize) -> Result<Vec<Json>> {
        let store = self.incidents_store()?;
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }
        let notes = Filter { kinds: vec![Kind::Note], ..Default::default() };
        let hits = recall::search(&store, text, &notes, &RankParams { n: 30, ..Default::default() })?.0;
        // Every incident's similarity, from one vector search.
        let qv = self.embedder.embed_query(text)?;
        let cos: HashMap<String, f64> = store.vector_top(&qv, 200, &notes)?.into_iter().map(|(r, c)| (r.id, c)).collect();
        let sim = |r: &Record, given: Option<f64>| given.or_else(|| cos.get(&r.id).copied()).unwrap_or(0.0).max(0.0);
        let mut picked: Vec<(Record, f64, Option<f64>, bool)> = Vec::new();
        if let Some(rule) = rule.filter(|r| !r.is_empty()) {
            for r in store.latest(&[Kind::Note], 1000, &Filter::default())? {
                if r.tag == "incident" && Incident::parse(&r.text).alert == rule && picked.len() < n {
                    let c = sim(&r, None);
                    picked.push((r, c, None, true));
                }
            }
        }
        // The search's hits that are close enough: in meaning (a shared
        // word lowers the bar), and not far behind the closest one.
        let found: Vec<(Record, f64, Option<f64>)> = hits
            .into_iter()
            .filter(|h| h.record.tag == "incident")
            .map(|h| {
                let c = sim(&h.record, h.cos);
                (h.record, c, h.bm25)
            })
            .filter(|(_, c, bm25)| *c >= if bm25.is_some() { MIN_MATCH - 0.1 } else { MIN_MATCH })
            .collect();
        let best = found.iter().map(|f| f.1).fold(0.0, f64::max);
        for (r, c, bm25) in found {
            if picked.len() >= n {
                break;
            }
            if c >= best * 0.5 && !picked.iter().any(|p| p.0.id == r.id) {
                picked.push((r, c, bm25, false));
            }
        }
        let mut out = Vec::new();
        for (r, c, bm25, same) in picked {
            let mut j = self.incident_json(&store, &r)?;
            j["match"] = json!((c * 100.0).round() / 100.0);
            j["bm25"] = json!(bm25);
            j["same_alert"] = json!(same);
            out.push(j);
        }
        Ok(out)
    }

    /// What an alert says, as a search: its name, what it watches and its
    /// description, and where it fires.
    fn alert_text(a: &Json) -> String {
        let mut t = format!("{} {} {} {}", a["name"].as_str().unwrap_or(""), a["rule"].as_str().unwrap_or(""), a["metric"].as_str().unwrap_or(""), a["description"].as_str().unwrap_or(""));
        for w in a["where"].as_array().into_iter().flatten().take(5) {
            for k in ["name", "short", "why", "error"] {
                if let Some(s) = w[k].as_str() {
                    t += " ";
                    t += s;
                }
            }
        }
        one_line(&t)
    }

    /// The context Seen before hands the on-call agent for an alert: the
    /// closest incidents with their fixes (and the fixes they replaced),
    /// and the incidents space's rules that bear on it.
    fn handoff_context(&self, alert: &Json, matches: &[Json]) -> Result<(String, Vec<Json>)> {
        let mut text = String::new();
        let mut parts = Vec::new();
        if !matches.is_empty() {
            text += "Past incidents of this cluster that look like this one (closest first):\n";
            for m in matches {
                let mut block = format!("- {} ({}, match {:.0}%)\n  Symptom: {}\n  Cause: {}\n  Fix: {}\n", m["title"].as_str().unwrap_or(""), crate::recall::fmt_time(m["ts"].as_i64().unwrap_or(0)).chars().take(10).collect::<String>(), m["match"].as_f64().unwrap_or(0.0) * 100.0, m["symptom"].as_str().unwrap_or(""), m["cause"].as_str().unwrap_or(""), m["fix"].as_str().unwrap_or(""));
                for e in m["replaced"].as_array().into_iter().flatten() {
                    block += &format!("  Replaced fix (no longer used): {}\n", e["fix"].as_str().unwrap_or(""));
                }
                parts.push(json!({"what": format!("Incident: {}", m["title"].as_str().unwrap_or("")), "tokens": tokens(block.len()), "id": m["id"]}));
                text += &block;
            }
        }
        let store = self.incidents_store()?;
        let rules = store.vector_top(&self.embedder.embed_query(&Self::alert_text(alert))?, 20, &Filter { kinds: vec![Kind::Preference], ..Default::default() })?;
        let rules: Vec<Record> = rules.into_iter().filter(|(r, c)| *c >= 0.3 && r.tag != "superseded").map(|(r, _)| r).take(3).collect();
        if !rules.is_empty() {
            text += "Rules the team keeps for incidents like this:\n";
            for r in &rules {
                let line = format!("- {}\n", one_line(&r.text));
                parts.push(json!({"what": format!("Rule: {}", one_line(&r.text)), "tokens": tokens(line.len()), "id": r.id}));
                text += &line;
            }
        }
        Ok((text, parts))
    }

    /// The cluster's firing and pending alerts, each with the closest past
    /// incidents and the context the on-call agent would get.
    pub fn firing(&self) -> Answer {
        let t0 = std::time::Instant::now();
        let alerts = self.cluster.get("/v1/alerts").map_err(|f| Fail(502, format!("the cluster's alerts: {f}")))?;
        let mut out = Vec::new();
        for a in alerts["states"].as_array().into_iter().flatten().filter(|a| matches!(a["state"].as_str(), Some("firing") | Some("pending"))) {
            let matches = self.matches(&Self::alert_text(a), a["rule"].as_str(), 3)?;
            let (context, parts) = self.handoff_context(a, &matches)?;
            out.push(json!({
                "rule": a["rule"], "name": a["name"], "severity": a["severity"], "state": a["state"], "since_ms": a["since_ms"], "value": a["value"],
                "threshold": a["threshold"], "op": a["op"], "metric": a["metric"], "description": a["description"], "where": a["where"],
                "matches": matches, "context": context, "context_parts": parts, "context_tokens": tokens(context.len()),
            }));
        }
        Ok(json!({"alerts": out, "ms": (t0.elapsed().as_secs_f64() * 1e5).round() / 100.0}))
    }

    /// Starts the on-call agent on an alert, with the closest incidents.
    pub fn handoff(&self, b: &Json) -> Answer {
        let rule = b["alert"].as_str().filter(|s| !s.is_empty()).ok_or_else(|| bad("handoff needs \"alert\" (a rule)"))?;
        let alerts = self.cluster.get("/v1/alerts").map_err(|f| Fail(502, format!("the cluster's alerts: {f}")))?;
        let a = alerts["states"].as_array().into_iter().flatten().find(|a| a["rule"].as_str() == Some(rule)).cloned().ok_or_else(|| Fail(404, format!("no alert rule {rule}")))?;
        let matches = self.matches(&Self::alert_text(&a), Some(rule), 3)?;
        let (context, _) = self.handoff_context(&a, &matches)?;
        let question = if context.is_empty() {
            format!("Alert {rule} is firing. Find out why, how bad it is, and what to do. ChloeCtx found no past incident like it.")
        } else {
            format!("Alert {rule} is firing. Find out why, how bad it is, and what to do. Check first whether it is one of these past incidents, which ChloeCtx found for it:\n\n{context}")
        };
        let run = self.cluster.post("/v1/admin/oncall", &json!({"alert": rule, "question": question, "wait": false})).map_err(|f| Fail(if f.status == 0 { 502 } else { f.status }, format!("the on-call agent: {}", f.message)))?;
        Ok(json!({"run": run, "question": question}))
    }

    /// Incidents a model drafts from a postmortem's text, or from an
    /// on-call run's report; nothing is stored.
    pub fn draft(&self, b: &Json) -> Answer {
        let text = match (b["text"].as_str().filter(|s| !s.trim().is_empty()), b["run"].as_str().filter(|s| !s.is_empty())) {
            (Some(t), _) => t.to_string(),
            (None, Some(run)) => {
                let r = self.cluster.get(&format!("/v1/admin/oncall/runs/{run}")).map_err(|f| Fail(502, format!("the on-call run: {f}")))?;
                let r = if r["run"].is_object() { r["run"].clone() } else { r };
                format!("On-call agent run about {}.\nQuestion: {}\nReport: {}", r["alert"].as_str().unwrap_or("a question"), r["question"].as_str().unwrap_or(""), r["answer"].as_str().unwrap_or(""))
            }
            _ => return Err(bad("draft needs \"text\" (a postmortem) or \"run\" (an on-call run's id)")),
        };
        let prompt = DRAFT_PROMPT.replace("{text}", &crate::llm::clip(&crate::redact::redact(&text), 30_000));
        let reply = self.model.complete(DRAFT_SYSTEM, &prompt).map_err(|e| Fail(503, e.0))?;
        let drafts: Vec<Json> = reply
            .text
            .lines()
            .filter_map(|l| serde_json::from_str::<Json>(l.trim().trim_end_matches(',')).ok())
            .filter(|j| j.is_object())
            .map(|j| Incident::of_json(&j))
            .filter(|i| !i.title.is_empty())
            .take(10)
            .map(|i| i.json())
            .collect();
        Ok(json!({"drafts": drafts, "usage": reply.usage.json()}))
    }

    // ------------------------------------------------------------ local stores

    fn local_home(&self) -> std::result::Result<&std::path::Path, Fail> {
        self.local.as_deref().ok_or_else(|| {
            Fail(404, "this chloectx app shows no local stores: start it with --local-stores (or CHLOECTX_APP_LOCAL=1) on the machine whose Claude Code sessions ChloeCtx records".into())
        })
    }

    /// This machine's ChloeCtx stores, one per project.
    pub fn local_stores(&self) -> Answer {
        let home = self.local_home()?;
        let ss: Vec<Json> = crate::ui::list_stores(home).iter().map(|s| s.json()).collect();
        Ok(json!({"home": home, "stores": ss}))
    }

    /// One request to a local store's daemon (started if it is not running).
    fn local_rpc(&self, store: &str, req: &Json, secs: u64) -> Answer {
        let home = self.local_home()?;
        let s = crate::ui::list_stores(home).into_iter().find(|s| s.id == store).ok_or_else(|| Fail(404, format!("no local store {store}")))?;
        let r = crate::daemon::request(&s.dir, s.project.as_deref().unwrap_or(""), req, Duration::from_secs(secs)).map_err(|e| Fail(502, format!("the daemon of {store}: {e}")))?;
        match r.get("error").and_then(|e| e.as_str()) {
            Some(e) if e.starts_with("no receipt") => Err(Fail(404, e.to_string())),
            Some(e) => Err(Fail(400, e.to_string())),
            None => Ok(r),
        }
    }

    pub fn info(&self) -> Json {
        let health = self.cluster.get("/v1/health");
        let j = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        json!({
            "version": env!("CARGO_PKG_VERSION"),
            "server": {"url": self.cluster.url, "ok": health.is_ok(), "error": health.err().map(|e| e.to_string())},
            "embedder": self.embedder.name(), "dim": self.embedder.dim(), "model": self.model.json(),
            "distill": {"running": j.running, "waiting": j.queue.len(), "done": j.done, "notes": j.notes, "failed": j.failed, "skipped": j.skipped, "last_error": j.last_error},
            "tables": {"spaces": SPACES, "space": "ctx_<space>"},
            "local": self.local.is_some(),
        })
    }
}

/// A token budget as the context's parameters: the defaults hold about
/// 4,000 tokens, scaled to `budget`.
pub fn params_for(budget: usize) -> ContextParams {
    let d = ContextParams::default();
    let f = budget as f64 / 4000.0;
    let scale = |n: usize| ((n as f64 * f).round() as usize).max(1);
    ContextParams {
        budget: scale(d.budget),
        reply_chars: scale(d.reply_chars).min(4000),
        reply_user_chars: scale(d.reply_user_chars).min(800),
        broad_reply_chars: scale(d.broad_reply_chars).min(2000),
        broad_timeline: scale(d.broad_timeline),
        replies: if budget < 1000 { 1 } else { d.replies },
        ..d
    }
}

fn manifest() -> Json {
    json!({
        "name": "chloectx", "title": "ChloeCtx",
        "description": "Memory spaces any agent writes to and asks for context, by search alone; and Seen before: the cluster's firing alerts beside the past incidents they look like",
        "icon_svg": ICON, "view": "ui/view.js", "version": env!("CARGO_PKG_VERSION"),
    })
}

fn view() -> String {
    match std::env::var_os("CHLOECTX_UI_DIR") {
        Some(d) => std::fs::read_to_string(std::path::Path::new(&d).join("view.js")).unwrap_or_else(|_| VIEW.into()),
        None => VIEW.into(),
    }
}

fn body_json(req: &Request) -> std::result::Result<Json, Fail> {
    if req.body.is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_slice(&req.body).map_err(|e| bad(format!("the body is not JSON: {e}")))
}

/// Answers one request (the path split on `/`).
fn route(app: &Arc<App>, req: &Request) -> Answer {
    let parts: Vec<String> = req.path.trim_matches('/').split('/').map(crate::ui::percent_decode).collect();
    let p: Vec<&str> = parts.iter().map(|s| s.as_str()).collect();
    let m = req.method.as_str();
    let num = |k: &str, d: usize| req.param(k).and_then(|v| v.parse().ok()).unwrap_or(d);
    match (m, p.as_slice()) {
        ("GET", ["v1", "info"]) => Ok(app.info()),
        ("GET", ["v1", "spaces"]) => Ok(app.spaces()?),
        ("POST", ["v1", "spaces"]) => {
            let b = body_json(req)?;
            let name = b["name"].as_str().unwrap_or("").trim();
            if !cluster::valid_name(name) || name == "spaces" {
                return Err(bad("a space's name is lowercase letters, digits and _, starting with a letter (at most 48), and not \"spaces\""));
            }
            Ok(app.create_space(name, b["title"].as_str().unwrap_or(""), b["description"].as_str().unwrap_or(""), "notes")?)
        }
        ("GET", ["v1", "spaces", s, "notes"]) => app.notes(s, req.param("kind"), num("limit", 100), req.param("all").is_some_and(|v| v == "1" || v == "true")),
        ("POST", ["v1", "spaces", s, "notes"]) => app.remember(s, &body_json(req)?),
        ("GET", ["v1", "spaces", s, "notes", id]) => app.history(s, id),
        ("DELETE", ["v1", "spaces", s, "notes", id]) => app.retire(s, id),
        ("POST", ["v1", "spaces", s, "exchanges"]) => app.exchange(s, &body_json(req)?),
        ("GET", ["v1", "spaces", s, "ask"]) => app.ask(s, req.param("q").unwrap_or(""), num("budget", 4000)),
        ("POST", ["v1", "spaces", s, "ask"]) => {
            let b = body_json(req)?;
            app.ask(s, b["question"].as_str().or(b["q"].as_str()).unwrap_or(""), b["budget"].as_u64().map_or(4000, |n| n as usize))
        }
        ("GET", ["v1", "incidents"]) => app.incidents(),
        ("POST", ["v1", "incidents"]) => app.record_incident(&body_json(req)?),
        ("GET", ["v1", "incidents", "firing"]) => app.firing(),
        ("GET", ["v1", "incidents", "match"]) => Ok(json!({"matches": app.matches(req.param("q").unwrap_or(""), req.param("alert"), num("n", 3).clamp(1, 20))?})),
        ("POST", ["v1", "incidents", "draft"]) => app.draft(&body_json(req)?),
        ("POST", ["v1", "incidents", "handoff"]) => app.handoff(&body_json(req)?),
        ("GET", ["v1", "spaces", s, "apps"]) => app.builds(s),
        ("POST", ["v1", "spaces", s, "apps"]) => app.build(s, &body_json(req)?),
        ("GET", ["v1", "spaces", _, "apps", "estimate"]) => app.estimate(),
        ("POST", ["v1", "spaces", s, "apps", "suggest"]) => app.suggest(s, &body_json(req)?),
        ("POST", ["v1", "spaces", s, "apps", "items"]) => app.build_items(s, &body_json(req)?),
        ("POST", ["v1", "spaces", s, "apps", id, "open"]) => app.open_build(s, id),
        ("GET", ["v1", "local"]) => app.local_stores(),
        ("GET", ["v1", "local", st, "receipts"]) => app.local_rpc(st, &json!({"op": "receipts", "session": req.param("session").filter(|s| !s.is_empty()), "limit": num("limit", 50)}), 30),
        ("GET", ["v1", "local", st, "receipts", id]) => app.local_rpc(st, &json!({"op": "receipt", "id": id, "replay": req.param("replay") != Some("0"), "text": true}), 120),
        ("GET", ["v1", "local", st, "blame"]) => app.local_rpc(st, &json!({"op": "blame", "target": req.param("target").unwrap_or(""), "replay": req.param("replay") != Some("0")}), 120),
        ("GET", ["v1", "local", st, "snapshots"]) => app.local_rpc(st, &json!({"op": "snapshot", "what": "list"}), 30),
        ("POST", ["v1", "local", st, "snapshots"]) => app.local_rpc(st, &json!({"op": "snapshot", "what": "keep", "name": body_json(req)?["name"]}), 600),
        ("DELETE", ["v1", "local", st, "snapshots", name]) => app.local_rpc(st, &json!({"op": "snapshot", "what": "drop", "name": name}), 60),
        (_, ["v1", ..]) => Err(Fail(404, format!("no {m} {}", req.path))),
        _ => Err(Fail(404, format!("no {}", req.path))),
    }
}

fn status_line(code: u16) -> &'static str {
    match code {
        200 => "200 OK",
        400 => "400 Bad Request",
        401 => "401 Unauthorized",
        403 => "403 Forbidden",
        402 => "402 Payment Required",
        404 => "404 Not Found",
        409 => "409 Conflict",
        429 => "429 Too Many Requests",
        503 => "503 Service Unavailable",
        500 => "500 Internal Server Error",
        _ => "502 Bad Gateway",
    }
}

fn handle(app: &Arc<App>, mut stream: std::net::TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let Some(req) = read_request(&stream) else { return };
    // Requests come from chloedb-server (for the command center) or from a
    // program, never from a page in a browser.
    if req.header("origin").is_some() {
        respond_json(&mut stream, "403 Forbidden", &json!({"error": "chloectx app takes no requests from browsers: open it in ChloeDB's command center (chloedb-server --app chloectx=URL)"}));
        return;
    }
    if let Some(t) = &app.token {
        let given = req.header("authorization").and_then(|v| v.strip_prefix("Bearer ")).unwrap_or("");
        if !same(given, t) {
            respond_json(&mut stream, "401 Unauthorized", &json!({"error": "chloectx app needs its token (Authorization: Bearer ...)"}));
            return;
        }
    }
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/app.json") => return respond_json(&mut stream, "200 OK", &manifest()),
        ("GET", "/ui/view.js") => return respond(&mut stream, "200 OK", "text/javascript; charset=utf-8", &[], view().as_bytes()),
        _ => {}
    }
    match route(app, &req) {
        Ok(j) => respond_json(&mut stream, "200 OK", &j),
        Err(Fail(code, m)) => respond_json(&mut stream, status_line(code), &json!({"error": m})),
    }
}

/// Whether `host` (as given to `--host`) is a loopback address, which only
/// this machine reaches.
pub fn loopback(host: &str) -> bool {
    let h = host.trim_start_matches('[').trim_end_matches(']');
    h.eq_ignore_ascii_case("localhost") || h.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// Refuses to serve every caller of a network with no token: anyone who
/// reaches the port could read and write every space and start model calls
/// billed to the app's key.
pub fn check_exposure(host: &str, token: Option<&str>, insecure: bool) -> Result<()> {
    if token.is_none() && !insecure && !loopback(host) {
        return Err(err(format!(
            "chloectx app would serve anyone who reaches {host} with no token: set --token (CHLOECTX_APP_TOKEN; chloedb-server sends its CHLOEDB_APP_TOKEN), listen on 127.0.0.1, or pass --insecure"
        )));
    }
    Ok(())
}

/// Serves the app until the process ends.
pub fn run(opts: Options) -> Result<()> {
    check_exposure(&opts.host, opts.token.as_deref(), opts.insecure)?;
    let cluster = Arc::new(Cluster::new(&opts.server, opts.key.clone()));
    let embedder: Arc<dyn Embedder> = Arc::from(crate::embed::open(&opts.embedder)?);
    // The cluster may still be starting: wait for it, up to a minute.
    let mut tries = 0;
    let app = loop {
        match App::new(cluster.clone(), embedder.clone(), Model::from_env(), opts.token.clone()) {
            Ok(mut a) => {
                a.local = opts.local.clone();
                break Arc::new(a);
            }
            Err(e) if tries < 30 => {
                tries += 1;
                eprintln!("chloectx app: waiting for {} ({e})", opts.server);
                std::thread::sleep(Duration::from_secs(2));
            }
            Err(e) => return Err(err(format!("{}: {e}", opts.server))),
        }
    };
    app.resume_builds();
    let listener = TcpListener::bind((opts.host.as_str(), opts.port)).map_err(|e| err(format!("cannot listen on {}:{}: {e}", opts.host, opts.port)))?;
    eprintln!(
        "chloectx app: on {}:{}, spaces in {} (embedder {}, model {})",
        opts.host,
        listener.local_addr()?.port(),
        opts.server,
        embedder.name(),
        app.model.json()["how"].as_str().unwrap_or("off")
    );
    let busy = Arc::new(AtomicUsize::new(0));
    for conn in listener.incoming() {
        let Ok(mut stream) = conn else { continue };
        if busy.fetch_add(1, Ordering::SeqCst) >= CONNECTIONS {
            busy.fetch_sub(1, Ordering::SeqCst);
            let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
            respond_json(&mut stream, "503 Service Unavailable", &json!({"error": format!("chloectx app is serving {CONNECTIONS} requests: try again")}));
            continue;
        }
        let (a, b) = (app.clone(), busy.clone());
        std::thread::spawn(move || {
            handle(&a, stream);
            b.fetch_sub(1, Ordering::SeqCst);
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_token_is_refused_off_loopback() {
        for host in ["127.0.0.1", "::1", "[::1]", "localhost", "127.0.0.2"] {
            assert!(check_exposure(host, None, false).is_ok(), "{host} is this machine only");
        }
        for host in ["0.0.0.0", "::", "10.0.0.7", "chloectx.example.com"] {
            let e = check_exposure(host, None, false).unwrap_err().to_string();
            assert!(e.contains("--token") && e.contains("--insecure"), "{host}: {e}");
            assert!(check_exposure(host, Some("t"), false).is_ok());
            assert!(check_exposure(host, None, true).is_ok(), "--insecure is the caller's choice");
        }
    }

    #[test]
    fn incidents_read_back_their_fields() {
        let i = Incident { title: "p99 jumped during a merge".into(), symptom: "search p99 above 250 ms on shard 2".into(), cause: "a large merge".into(), fix: "pause merges".into(), alert: "search-latency".into() };
        assert_eq!(Incident::parse(&i.text()), i);
        let j = Incident::parse("Lost writes\nFix: idempotency keys in _sources");
        assert_eq!((j.title.as_str(), j.fix.as_str(), j.cause.as_str()), ("Lost writes", "idempotency keys in _sources", ""));
    }

    #[test]
    fn budgets_scale_the_context() {
        let d = ContextParams::default();
        assert_eq!(params_for(4000).budget, d.budget);
        assert_eq!(params_for(2000).budget, d.budget / 2);
        assert_eq!(params_for(500).replies, 1);
        assert_eq!(tokens(4001), 1001);
    }

    #[test]
    fn note_kinds_by_name() {
        assert_eq!(note_kind("rule").map(|k| k.0), Some(Kind::Preference));
        assert_eq!(note_kind("todo").map(|k| k.1), Some("planned"));
        assert!(note_kind("mood").is_none());
    }
}
