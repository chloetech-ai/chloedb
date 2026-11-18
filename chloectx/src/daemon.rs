//! The per-project daemon: owns the Strata store, keeps session state, and
//! answers hooks, MCP calls and CLI requests over a Unix socket (one JSON
//! request line, one JSON response line per connection).

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};
use chloedb::query::Params;
use chloedb::Value;

use crate::anchor::{self, Checker};
use crate::capture::{self, EpisodeAcc};
use crate::paths::StorePaths;
use crate::recall::{self, Decision, Gate, PackParams, RankParams};
use crate::record::{Draft, Kind, Record};
use crate::store::{Filter, Store, StoreOptions};
use crate::{embed, err, now, Result};

// ---------------------------------------------------------------- config

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Embedder spec (see [`embed::open`]).
    pub embedder: String,
    /// Exit after this many idle seconds.
    pub idle_secs: u64,
    pub rank: RankParams,
    pub pack: PackParams,
    pub gate: Gate,
    pub briefing_chars: usize,
    pub knowledge: recall::KnowledgeParams,
    /// Notes about a file before Edit/Write.
    pub file_notes: bool,
    /// Before Edit/Write, also the files usually edited next, with a note
    /// on each ([`crate::next_files`]; an experiment, `CHLOECTX_NEXT_FILES=1`).
    pub next_files: bool,
    /// A command that fails with an error cleared before gets the steps
    /// that cleared it, and fixes are written down ([`crate::fixes`]).
    pub error_fixes: bool,
    pub sync: bool,
    /// Knowledge written down by a small model after each turn.
    pub distill: crate::distill::DistillConfig,
    /// Related notes merged now and then, so the knowledge stays small.
    pub consolidate: crate::consolidate::ConsolidateConfig,
    /// Query understanding: model-planned searches and a model ranker.
    pub deep: crate::deep::DeepConfig,
    /// A receipt for every recall, and snapshots to replay them on.
    pub receipts: ReceiptConfig,
}

/// Receipts ([`crate::receipt`]) and kept snapshots ([`crate::snapshots`]).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ReceiptConfig {
    /// Write receipts (and keep snapshots). Unless config.json sets it,
    /// `CHLOECTX_RECEIPTS=0` turns them off, read from the daemon's
    /// environment and from each hook's, so it holds whichever process
    /// started the daemon.
    pub enabled: bool,
    pub snapshots: crate::snapshots::Retention,
    /// How long receipts are kept.
    pub retention: crate::receipt::BookRetention,
    /// config.json sets `enabled` (then the environment does not).
    #[serde(skip)]
    pub explicit: bool,
}

impl Default for ReceiptConfig {
    fn default() -> Self {
        ReceiptConfig { enabled: receipts_env().unwrap_or(true), snapshots: Default::default(), retention: Default::default(), explicit: false }
    }
}

/// What `CHLOECTX_RECEIPTS` says, if it is set.
pub fn receipts_env() -> Option<bool> {
    std::env::var("CHLOECTX_RECEIPTS").ok().filter(|v| !v.is_empty()).map(|v| v != "0")
}

impl Default for Config {
    fn default() -> Self {
        Config {
            embedder: std::env::var("CHLOECTX_EMBEDDER").unwrap_or_else(|_| "static".into()),
            idle_secs: 1800,
            rank: RankParams::default(),
            pack: PackParams::default(),
            gate: Gate::default(),
            briefing_chars: 6000,
            knowledge: Default::default(),
            file_notes: true,
            next_files: std::env::var("CHLOECTX_NEXT_FILES").is_ok_and(|v| v == "1"),
            error_fixes: std::env::var("CHLOECTX_ERROR_FIXES").map_or(true, |v| v != "0"),
            sync: true,
            distill: Default::default(),
            consolidate: Default::default(),
            deep: Default::default(),
            receipts: Default::default(),
        }
    }
}

impl Config {
    /// The store's config.json; the gate defaults to the one fitted for the
    /// configured embedder unless the file sets one.
    pub fn load(path: &Path) -> Config {
        let raw: Json = std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Json::Null);
        let mut cfg: Config = serde_json::from_value(raw.clone()).unwrap_or_default();
        if raw.get("gate").is_none() {
            cfg.gate = Gate::for_embedder(&cfg.embedder);
        }
        cfg.receipts.explicit = raw.get("receipts").and_then(|r| r.get("enabled")).is_some_and(|v| v.is_boolean());
        cfg.deep = cfg.deep.resolved();
        cfg
    }
}

// ---------------------------------------------------------------- sessions

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Session {
    /// Records of this session at or after this sequence number are in the
    /// model's context.
    pub window_start: i64,
    pub turn: i64,
    pub injected: HashSet<String>,
    pub annotated: HashSet<String>,
    pub episode: EpisodeAcc,
    pub episode_stored: bool,
    pub last_prompts: VecDeque<String>,
    pub last_compact: i64,
    pub last_seen: i64,
    /// Bytes of the transcript already scanned for narration.
    pub transcript_offset: u64,
    /// Failed commands followed until they pass ([`crate::fixes`]).
    #[serde(default)]
    pub fixes: crate::fixes::Tracker,
    /// The folder the session runs in, as its hooks name it: a worktree of
    /// the repository whose store this is, which may not be the folder the
    /// daemon was started for ([`crate::paths::store_root`]). Its paths
    /// are relative to it and its notes are checked against its files.
    /// Empty for a session whose hooks predate it: the daemon's project.
    #[serde(default)]
    pub worktree: String,
    /// The branch checked out in the worktree when its hooks last ran.
    #[serde(default)]
    pub branch: String,
    /// Who wrote the current turn's prompt ([`crate::record::Source`]):
    /// `user` unless Workbench said otherwise (`CHLOECTX_PROMPT_SOURCE`,
    /// `CHLOECTX_PROMPT_LOG`).
    #[serde(default)]
    pub source: String,
    /// The session's agent name, when Workbench gave one (`CHLOECTX_AGENT_NAME`).
    #[serde(default)]
    pub agent: String,
    /// The end of the assistant's last answer: a prompt may answer a
    /// question it asked (a rule asked for, [`crate::capture::standing_rule`]).
    #[serde(default)]
    pub prev_answer: String,
    /// The session's last final answers, by hash: the transcript may still
    /// hold one when the next turn's narration is read ([`Daemon::sync_narration`]).
    #[serde(default)]
    pub answered: VecDeque<u64>,
    /// The part of the session's briefing given with its first prompt
    /// ([`crate::recall::Briefing::tail`]), and the ids it shows.
    #[serde(default)]
    pub tail: Option<(String, Vec<String>)>,
    /// When each tool call of the turn was made, by `tool_use_id`, from the
    /// transcript, kept until its result arrives ([`crate::report`]).
    #[serde(skip)]
    pub calls: HashMap<String, i64>,
    /// What the window held when a turn's prompt came, before its recall:
    /// the turn, and the records shown ([`crate::repeats::promote`]).
    #[serde(skip)]
    pub shown_before: (i64, HashSet<String>),
    /// The pid of the Claude Code process the session runs in, as its
    /// hooks are told it (`CLAUDE_PID`): the parent of the session's MCP
    /// server, whose calls are filed under the session by it
    /// ([`Daemon::caller_session`]). 0 when no hook said.
    #[serde(default)]
    pub pid: u32,
}

/// A query-understanding search started after a prompt (`async` mode).
#[derive(Clone, Debug, Default)]
struct DeepJob {
    turn: i64,
    done: bool,
    text: String,
    ids: Vec<String>,
    confidence: String,
    delivered: bool,
}

#[derive(Default, Serialize, Deserialize)]
struct State {
    sessions: HashMap<String, Session>,
    active: String,
}

/// The tag of the event that marks a turn distilled, stored with its notes.
pub const TURN_DISTILLED: &str = "distilled_turn";
/// The tag of the event that marks a turn whose distillation failed.
pub const TURN_FAILED: &str = "distill_failed";
/// The tag of the event that marks a turn never to be distilled: one that
/// called `memory_forget`, whose notes would say again what it forgot, or
/// a question that stood on its own, answered in a few lines without a
/// tool ([`EpisodeAcc::only_a_question`]).
pub const TURN_SKIPPED: &str = "distill_skipped";
/// The final answers a session remembers by hash ([`Session::answered`]).
const ANSWERS_KEPT: usize = 8;
/// At most this many turns a stopped daemon left are distilled on a start.
const MISSED_TURNS: usize = 5;

/// The turns [`Daemon::distill_marks`] found marked, by outcome.
#[derive(Default)]
struct Marks {
    ok: HashSet<(String, i64)>,
    failed: HashSet<(String, i64)>,
    skipped: HashSet<(String, i64)>,
    first: Option<i64>,
}

/// A stored turn, as distilling it again needs it.
#[derive(Default)]
struct StoredTurn {
    session: String,
    turn: i64,
    ts: i64,
    prompt: String,
    answer: String,
    commands: Vec<String>,
    files: Vec<String>,
    by: Option<crate::record::Source>,
    prev_answer: String,
}

/// One sync with the server at a time: a push, a pull, or a forget's
/// deletes there (audit CTX-33).
static SYNC_ONE: Mutex<()> = Mutex::new(());

pub struct Daemon {
    pub paths: StorePaths,
    pub project: String,
    pub store: Arc<Store>,
    pub cfg: Config,
    state: Mutex<State>,
    /// The state changed since it was last written (it is written after
    /// the response, off the hook's critical path).
    dirty: std::sync::atomic::AtomicBool,
    /// Background work still running (distillation); shutdown waits for it.
    pending: std::sync::atomic::AtomicUsize,
    /// Held while state.json is written, so an older state never lands
    /// after a newer one.
    saving: Mutex<()>,
    /// Held while events.log is appended to or rewritten (`redact`).
    logging: Mutex<()>,
    /// Distillations run one at a time, so each sees what the last wrote.
    distilling: Mutex<()>,
    /// Turns distilled since the notes were last consolidated.
    distilled_since: std::sync::atomic::AtomicUsize,
    /// Query-understanding searches by session.
    deep_jobs: Mutex<HashMap<String, DeepJob>>,
    /// Files edited after each file, counted from the stored edits.
    next_files: Mutex<crate::next_files::NextFiles>,
    /// The settings Claude Code's hooks last ran with (their environment):
    /// they win over the daemon's own, which may have been started from a
    /// shell or the MCP server without them.
    from_hooks: Mutex<(String, Option<crate::deep::DeepConfig>)>,
    /// `CHLOECTX_DISTILL` as the hooks saw it: 0 unset, 1 on, 2 off.
    hook_distill: std::sync::atomic::AtomicU8,
    /// The turns a stopped daemon did not distill were looked for
    /// ([`Daemon::distill_missed`]).
    missed_checked: std::sync::atomic::AtomicBool,
    last_activity: AtomicI64,
    /// Receipts of recalls: opened when they are on or the file exists
    /// (a forget or redact scrubs it while they are off), unset when its
    /// file failed to open.
    receipts: std::sync::OnceLock<crate::receipt::Book>,
    /// Receipts are written (`receipts.enabled`, `CHLOECTX_RECEIPTS`,
    /// `chloectx receipts on|off`).
    receipts_on: std::sync::atomic::AtomicBool,
    /// Receipts made while answering, written once the answer is sent.
    unsaved: Mutex<Vec<crate::receipt::Receipt>>,
    /// Work to run after an answer, queued or running; shutdown waits.
    deferred: std::sync::atomic::AtomicUsize,
    /// `chloectx receipts on|off` decided: the hooks' environment no longer does.
    no_hook_receipts: std::sync::atomic::AtomicBool,
    snapshots: crate::snapshots::Snapshots,
    /// A recall showed records since the last snapshot was kept.
    keep_pending: std::sync::atomic::AtomicBool,
    /// Connections being answered, from the request read to the answer
    /// written: shutdown waits for them before the exit, which would
    /// close them unanswered.
    serving: std::sync::atomic::AtomicUsize,
    /// Shutdown is past the point where requests are handled: one that
    /// comes is answered [`closing_answer`], and its client sends it to
    /// the next daemon.
    closing: std::sync::atomic::AtomicBool,
}

/// Work to do after the response is sent (so hooks return at once).
pub type Deferred = Box<dyn FnOnce() + Send>;

/// Releases a [`crate::snapshots::Snapshots::hold`] when a forget fails
/// before it hands the hold to its compaction.
struct Held<'a>(&'a crate::snapshots::Snapshots);

impl Drop for Held<'_> {
    fn drop(&mut self) {
        self.0.release();
    }
}

/// What of a store's other files held a forgotten text ([`Daemon::scrub_side`]).
#[derive(Default)]
struct Side {
    sessions: usize,
    log_lines: usize,
    daemon_lines: usize,
    receipts: usize,
    named: Vec<String>,
    failed: Vec<String>,
}

impl Side {
    /// What was scrubbed, in words.
    fn said(&self) -> Vec<String> {
        let mut out = Vec::new();
        let n = |k: usize, one: &str, many: &str| if k == 1 { format!("1 {one}") } else { format!("{k} {many}") };
        if self.sessions > 0 {
            out.push(format!("{} scrubbed", n(self.sessions, "session's state", "sessions' state")));
        }
        if self.log_lines > 0 {
            out.push(format!("{} of events.log scrubbed", n(self.log_lines, "line", "lines")));
        }
        if self.daemon_lines > 0 {
            out.push(format!("{} of daemon.log scrubbed", n(self.daemon_lines, "line", "lines")));
        }
        out.extend(self.failed.iter().cloned());
        out
    }
}

impl Daemon {
    pub fn open(dir: &Path, project: &str) -> Result<Daemon> {
        let paths = StorePaths::new(dir);
        crate::paths::create_private_dir(dir)?;
        let cfg = Config::load(&paths.config);
        let embedder: Arc<dyn embed::Embedder> = Arc::from(embed::open(&cfg.embedder)?);
        let store = Arc::new(Store::open(&paths.db, embedder, StoreOptions { sync: cfg.sync, ..Default::default() })?);
        // A forgotten record's bytes must leave the file: pages freed by a
        // rewrite are zeroed, and so is the tail of every page written after.
        if let Some(db) = store.db() {
            db.storage().set_zero_page_tails(true);
        }
        let state: State = std::fs::read_to_string(&paths.state).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let receipts = std::sync::OnceLock::new();
        if cfg.receipts.enabled || dir.join(crate::receipt::BOOK_FILE).exists() {
            match crate::receipt::Book::open(dir, cfg.sync) {
                Ok(b) => {
                    let _ = receipts.set(b);
                }
                Err(e) => eprintln!("chloectx: receipts are off: {e}"),
            }
        }
        let receipts_on = std::sync::atomic::AtomicBool::new(cfg.receipts.enabled);
        let snapshots = crate::snapshots::Snapshots::new(dir);
        Ok(Daemon { paths, project: project.to_string(), store, cfg, state: Mutex::new(state), dirty: Default::default(), pending: Default::default(), saving: Mutex::new(()),
            logging: Mutex::new(()), distilling: Mutex::new(()), distilled_since: Default::default(), deep_jobs: Mutex::new(HashMap::new()), next_files: Default::default(), from_hooks: Mutex::new((String::new(), None)), hook_distill: Default::default(), missed_checked: Default::default(), last_activity: AtomicI64::new(now()),
            receipts, receipts_on, unsaved: Default::default(), deferred: Default::default(), no_hook_receipts: Default::default(), snapshots, keep_pending: Default::default(),
            serving: Default::default(), closing: Default::default() })
    }

    /// Wraps work to run after the answer, so that shutdown waits for it.
    /// Counted from now, before the answer is sent.
    fn after(self: &Arc<Self>, f: impl FnOnce(&Daemon) + Send + 'static) -> Deferred {
        self.deferred.fetch_add(1, Ordering::AcqRel);
        let me = self.clone();
        Box::new(move || {
            struct Done<'a>(&'a std::sync::atomic::AtomicUsize);
            impl Drop for Done<'_> {
                fn drop(&mut self) {
                    self.0.fetch_sub(1, Ordering::AcqRel);
                }
            }
            let _done = Done(&me.deferred);
            f(&me);
        })
    }

    /// Housekeeping when the daemon starts: drops receipts and automatic
    /// snapshots past their retention, and what lies under `snapshots/`
    /// that no snapshot owns. Runs whatever the settings say now: turning
    /// receipts off must not keep the old ones for good.
    pub fn housekeep(&self) {
        let swept = self.snapshots.sweep();
        let pruned = self.snapshots.prune(&self.cfg.receipts.snapshots, self.store_bytes()).unwrap_or_else(|e| {
            self.log(&json!({"ts": now(), "error": e.to_string(), "event": "snapshot_prune"}));
            0
        });
        let receipts = match self.receipts.get().map(|b| b.prune(&self.cfg.receipts.retention)) {
            Some(Ok(n)) => n,
            Some(Err(e)) => {
                self.log(&json!({"ts": now(), "error": e.to_string(), "event": "receipt_prune"}));
                0
            }
            None => 0,
        };
        if swept + pruned + receipts > 0 {
            self.log(&json!({"ts": now(), "event": "housekeep", "snapshot_dirs_swept": swept, "snapshots_pruned": pruned, "receipts_pruned": receipts}));
        }
    }

    /// The size of the store's file and log, for the snapshots' budget.
    fn store_bytes(&self) -> u64 {
        self.store.db().map_or(0, |db| {
            let fs = db.storage().file_stats();
            fs.file_bytes + fs.wal_bytes
        })
    }

    /// Whether receipts are written now.
    pub fn receipts_on(&self) -> bool {
        self.receipts_on.load(Ordering::Acquire) && self.receipts.get().is_some()
    }

    /// Writes the receipts made while answering.
    fn save_receipts(&self) {
        let mut rs = std::mem::take(&mut *self.unsaved.lock().unwrap());
        if rs.is_empty() {
            return;
        }
        let live = self.store.forgetting.current(now());
        if !live.is_empty() {
            let f = crate::receipt::Forgotten { texts: live, ..Default::default() };
            for r in &mut rs {
                crate::receipt::scrub_receipt(r, &f);
            }
        }
        if let Some(b) = self.receipts.get() {
            if let Err(e) = b.put_all(&rs) {
                self.log(&json!({"ts": now(), "error": e.to_string(), "event": "receipt"}));
            }
        }
    }

    /// Writes the session state: copied under the session lock, written
    /// outside it, so a hook never waits for the file.
    fn save_state(&self) {
        // A text forgotten lately is not written back (a prompt that said
        // it again, a reply).
        let live = self.store.forgetting.current(now());
        if !live.is_empty() {
            self.scrub_sessions(&live, false);
        }
        let _one = self.saving.lock().unwrap_or_else(|e| e.into_inner());
        let mut st2 = {
            let st = self.state.lock().unwrap();
            State { sessions: st.sessions.clone(), active: st.active.clone() }
        };
        if st2.sessions.len() > 64 {
            let mut by: Vec<(String, i64)> = st2.sessions.iter().map(|(k, v)| (k.clone(), v.last_seen)).collect();
            by.sort_by_key(|x| -x.1);
            let keep: HashSet<String> = by.into_iter().take(64).map(|x| x.0).collect();
            st2.sessions.retain(|k, _| keep.contains(k));
        }
        if let Ok(s) = serde_json::to_string(&st2) {
            let tmp = self.paths.state.with_extension("json.tmp");
            if std::fs::write(&tmp, s).is_ok() {
                let _ = std::fs::rename(tmp, &self.paths.state);
            }
        }
    }

    /// Writes the session state if it changed.
    pub fn persist(&self) {
        if self.dirty.swap(false, Ordering::AcqRel) {
            self.save_state();
        }
    }

    /// Appends an event to events.log, its strings redacted.
    fn log(&self, line: &Json) {
        let mut line = line.clone();
        crate::redact::redact_json(&mut line);
        let live = self.store.forgetting.current(now());
        if !live.is_empty() {
            crate::forget::scrub_json(&mut line, &live);
        }
        let _one = self.logging.lock().unwrap_or_else(|e| e.into_inner());
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(self.paths.dir.join("events.log")) {
            let _ = writeln!(f, "{line}");
        }
    }

    /// Handles one request. Returns the response and work to run after it.
    pub fn handle(self: &Arc<Self>, req: &Json) -> (Json, Option<Deferred>) {
        self.last_activity.store(now(), Ordering::Relaxed);
        let op = req.get("op").and_then(|v| v.as_str()).unwrap_or("");
        let res = match op {
            "ping" => Ok((json!({"ok": true, "pid": std::process::id(), "records": self.store.len()}), None)),
            "hook" => {
                self.note_hook_settings(req);
                let payload = req.get("payload").unwrap_or(&Json::Null);
                let sid = payload.get("session_id").and_then(|v| v.as_str()).unwrap_or("unknown");
                self.note_worktree(sid, req);
                // The Claude Code process it runs in, whose MCP server
                // names it as its parent ([`Daemon::caller_session`]).
                // A process runs one session at a time: after a `/clear`
                // the new one.
                if let Some(pid) = req.get("claude_pid").and_then(|v| v.as_u64()).filter(|p| *p > 1 && *p <= u32::MAX as u64).map(|p| p as u32) {
                    let mut st = self.state.lock().unwrap();
                    if st.sessions.get(sid).is_none_or(|s| s.pid != pid) {
                        for s in st.sessions.values_mut().filter(|s| s.pid == pid) {
                            s.pid = 0;
                        }
                        st.sessions.entry(sid.to_string()).or_default().pid = pid;
                    }
                }
                if let Some(a) = req.get("agent").and_then(|v| v.as_str()).filter(|a| !a.is_empty()) {
                    let mut st = self.state.lock().unwrap();
                    if let crate::record::Source::Agent(n) = crate::record::Source::parse(&format!("agent:{a}")) {
                        st.sessions.entry(sid.to_string()).or_default().agent = n;
                    }
                }
                if req.get("event").and_then(|v| v.as_str()) == Some("UserPromptSubmit") {
                    self.note_source(sid, req);
                }
                let res = self.hook(req.get("event").and_then(|v| v.as_str()).unwrap_or(""), payload);
                // The first hook after a start says which model distills and
                // whether it is on: the turns a stopped daemon left are
                // distilled then, after the answer.
                if self.missed_checked.swap(true, Ordering::AcqRel) {
                    res
                } else {
                    let missed = self.after(|me| me.distill_missed());
                    res.map(|(j, d)| {
                        let both: Deferred = match d {
                            Some(f) => Box::new(move || {
                                f();
                                missed();
                            }),
                            None => missed,
                        };
                        (j, Some(both))
                    })
                }
            }
            "search" => self.op_search(req).map(|r| (r, Some(self.after(|me| me.keep_after_recall())))),
            "receipts" => self.op_receipts(req).map(|r| (r, None)),
            "receipt" => self.op_receipt(req).map(|r| (r, None)),
            "blame" => self.op_blame(req).map(|r| (r, None)),
            "snapshot" => self.op_snapshot(req).map(|r| (r, None)),
            "timeline" => self.op_timeline(req).map(|r| (r, None)),
            "sql" => self.op_sql(req).map(|r| (r, None)),
            "remember" => self.op_remember(req).map(|r| (r, None)),
            "forget" => self.op_forget(req),
            "find" => self.op_find(req).map(|r| (r, None)),
            "receipts_switch" => self.op_receipts_switch(req).map(|r| (r, None)),
            "redact" => self.op_redact(req).map(|r| (r, None)),
            "append" => self.op_append(req).map(|r| (r, None)),
            "stats" => self.op_stats().map(|r| (r, None)),
            "backup" => self.op_backup(req).map(|r| (r, None)),
            // Read-only views for the demo inspector (demo/server.py).
            "inspect_query" => crate::inspect::query(&self.store, req).map(|r| (r, None)),
            "inspect_layout" => crate::inspect::layout(&self.store).map(|r| (r, None)),
            "inspect_embed" => crate::inspect::embed(&self.store, req).map(|r| (r, None)),
            "distill_backfill" => {
                let flag = |k: &str| req.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
                self.distill_backfill(flag("again"), flag("dry_run")).map(|r| (r, None))
            }
            "consolidate" => self.op_consolidate().map(|r| (r, None)),
            "repeats" => {
                let days = req.get("days").and_then(|v| v.as_i64()).unwrap_or(30).max(1);
                crate::repeats::scan(&self.store, now() - days * 86_400, 20_000).map(|r| (r, None))
            }
            "ask" => self.op_ask(req).map(|r| (r, None)),
            // What `chloectx app --local-stores` shows of this store, as it
            // shows a space of the cluster ([`crate::app::view`]).
            "view" => crate::app::view::by_name(&self.store.pinned(), req).map(|r| (r, None)),
            "capsule" => self.op_capsule(req).map(|r| (r, None)),
            "report" => self.op_report(req).map(|r| (r, None)),
            "briefing" => {
                let sid = req.get("session").and_then(|v| v.as_str()).unwrap_or("");
                let mut ck = self.checker_in(&self.tree_of(req));
                recall::briefing_checked(&self.store, sid, self.cfg.briefing_chars, now(), Some(&mut ck)).map(|(t, _)| (json!({"text": t}), None))
            }
            "sync" => self.op_sync(req).map(|r| (r, None)),
            "shutdown" => {
                let me = self.clone();
                Ok((json!({"ok": true}), Some(Box::new(move || me.shutdown()) as Deferred)))
            }
            _ => Err(err(format!("unknown op {op:?}"))),
        };
        match res {
            Ok(r) => r,
            Err(e) => (json!({"error": e.to_string()}), None),
        }
    }

    pub fn shutdown(&self) {
        let until = Instant::now() + Duration::from_secs(self.cfg.distill.timeout_secs + 30);
        // Distillation, and the work after each answer: the prompt's own
        // record, a tool call's, a snapshot being written.
        while (self.pending.load(Ordering::Acquire) > 0 || self.deferred.load(Ordering::Acquire) > 0) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(50));
        }
        // What was recorded since the last push goes before the daemon does.
        let _ = self.sync_now(false);
        // Requests were handled until now. From here each one is answered
        // `closing`, not handled, and its client sends it to the next
        // daemon, once this one is gone; the exit would otherwise close it
        // unanswered (`chloectx sync setup`, which stops the daemon, then
        // `chloectx sync push`: "EOF while parsing a value"). SeqCst with
        // `serving` in [`serve`]: a request either sees `closing` or is
        // counted here, and waited for.
        self.closing.store(true, Ordering::SeqCst);
        while (self.serving.load(Ordering::SeqCst) > 0 || self.pending.load(Ordering::Acquire) > 0 || self.deferred.load(Ordering::Acquire) > 0) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = self.store.flush();
        self.save_receipts();
        if let Some(b) = self.receipts.get() {
            let _ = b.flush();
        }
        self.save_state();
        let _ = std::fs::remove_file(&self.paths.socket);
        // The connections made before the socket went, answered `closing`.
        let last = Instant::now() + Duration::from_millis(500);
        while self.serving.load(Ordering::SeqCst) > 0 && Instant::now() < last {
            std::thread::sleep(Duration::from_millis(5));
        }
        std::process::exit(0);
    }

    /// Pushes what the store recorded since the last push to the server
    /// `chloectx sync setup` named (and, with `pull`, first stores the
    /// project's records the server holds and the store lacks): the
    /// sync's state after, or `None` with no server set up. One at a time.
    pub fn sync_now(&self, pull: bool) -> Option<crate::sync::SyncState> {
        self.sync_with(pull, false)
    }

    /// [`Daemon::sync_now`]; `again` sends every record again.
    fn sync_with(&self, pull: bool, again: bool) -> Option<crate::sync::SyncState> {
        // No server set up: done, without asking git what the project is.
        crate::sync::remote(&crate::paths::home())?;
        let key = crate::sync::project_key(&crate::paths::home(), &self.project);
        // Read each time: `chloectx sync setup` may have given the project a key since.
        let remote = crate::sync::remote_for(&crate::paths::home(), &key)?;
        let _one = SYNC_ONE.lock().unwrap_or_else(|e| e.into_inner());
        let saved = crate::sync::saved_state(&self.paths.dir);
        let mut st = crate::sync::load_state(&self.paths.dir, &remote.url, &key);
        if saved.url == remote.url && !saved.key.is_empty() && saved.key != key {
            // Another name is another table: it gets every record.
            self.log(&json!({"ts": now(), "event": "sync", "server": remote.url, "renamed": {"from": saved.key, "to": key}}));
        }
        if again {
            st.pushed_seq = 0;
        }
        // The rows of records forgotten here go first (audit CTX-33): a
        // server that did not answer a forget's delete is asked again.
        let forgets = crate::sync::send_forgets(&remote, &key, &self.paths.dir);
        let pushed = crate::sync::push(&remote, &self.store, &key, &mut st);
        // What a pull stores keeps the server's sequence numbers, and the
        // next push sends again those past what was pushed: the same rows,
        // replaced by themselves. Taking them as pushed (pushed_seq set to
        // the store's last) also took as pushed whatever the hooks recorded
        // while the pull ran, which then never reached the server.
        let pulled = if pull && pushed.is_ok() { Some(crate::sync::pull(&remote, &self.store, &key, &mut st, &self.paths.dir)) } else { None };
        st.last_error = match (&forgets, &pushed, &pulled) {
            (Err(e), _, _) | (_, Err(e), _) | (_, _, Some(Err(e))) => e.to_string(),
            _ => String::new(),
        };
        let (p, g) = (pushed.as_ref().map_or(0, |n| *n), pulled.as_ref().and_then(|r| r.as_ref().ok()).copied().unwrap_or(0));
        if p > 0 || g > 0 || !st.last_error.is_empty() {
            self.log(&json!({"ts": now(), "event": "sync", "server": remote.url, "pushed": p, "pulled": g, "error": st.last_error}));
        }
        crate::sync::save_state(&self.paths.dir, &st);
        Some(st)
    }

    /// Deletes the sync server's rows of the records just forgotten (audit
    /// CTX-33), between pushes: one that read a record before its delete
    /// has sent it by then. What the forget says of them; nothing with no
    /// server set up.
    fn forget_on_server(&self) -> String {
        // No server set up: nothing to delete, without asking git what the project is.
        if crate::sync::remote(&crate::paths::home()).is_none() {
            return String::new();
        }
        let key = crate::sync::project_key(&crate::paths::home(), &self.project);
        // The project's own key where it has one (`chloectx sync setup`), as a push uses.
        let Some(remote) = crate::sync::remote_for(&crate::paths::home(), &key) else { return String::new() };
        let _one = SYNC_ONE.lock().unwrap_or_else(|e| e.into_inner());
        match crate::sync::send_forgets(&remote, &key, &self.paths.dir) {
            Ok(_) => "its row on the sync server deleted too (a machine that pulled it before keeps its copy)".into(),
            Err(e) => {
                self.log(&json!({"ts": now(), "event": "sync", "server": remote.url, "error": format!("forget: {e}")}));
                format!("the sync server's row is not deleted yet ({e}): each push asks again, and a pull never brings it back here")
            }
        }
    }

    /// `{"op": "sync", "what": "status" | "push" | "pull"}`.
    fn op_sync(&self, req: &Json) -> Result<Json> {
        let what = req.get("what").and_then(|v| v.as_str()).unwrap_or("status");
        if what == "accept" {
            let origins: Vec<String> = req.get("origins").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
            let by = crate::sync::accept(&crate::paths::home(), &self.store, &origins)?;
            let mut text = String::new();
            if by.is_empty() {
                text = "No records from other machines wait to be accepted.".into();
            }
            for (o, n, some) in &by {
                let done = origins.iter().any(|x| x == "*" || x == o);
                text += &format!("{o}: {n} record{}{}\n", if *n == 1 { "" } else { "s" }, if done { ", accepted" } else { "" });
                for t in some {
                    text += &format!("    {}\n", crate::envelope::defuse(t));
                }
            }
            if origins.is_empty() && !by.is_empty() {
                text += "Accept a machine's records (its rules and decisions then reach the briefing): chloectx sync accept --origin ID, or --all";
            }
            let me = crate::sync::origin(&crate::paths::home());
            return Ok(json!({"text": text.trim_end(), "this_machine": me, "pending": by.iter().map(|(o, n, _)| json!({"origin": o, "records": n})).collect::<Vec<_>>()}));
        }
        let home = crate::paths::home();
        let key = crate::sync::project_key(&home, &self.project);
        let Some(remote) = crate::sync::remote_for(&home, &key) else {
            let Some(server) = crate::sync::remote(&home) else {
                return Ok(json!({"server": null, "syncs": false, "note": "no server set up: chloectx sync setup --url URL"}));
            };
            // A server is set up for other projects of the machine, not this one (LENA-301).
            let mut note = format!("this project ({key}) does not sync: {}. To sync it, run `chloectx sync setup --url {}` in its folder", crate::sync::scope_line(&home), server.url);
            note += &crate::sync::sent_before(&crate::sync::saved_state(&self.paths.dir), &server.url);
            return Ok(json!({
                "server": server.url, "syncs": false, "project": self.project, "synced_as": key,
                "projects": crate::sync::set_up_projects(&home), "every_project": false, "warning": note,
            }));
        };
        let again = req.get("again").and_then(|v| v.as_bool()).unwrap_or(false);
        let st = match what {
            "push" => self.sync_with(false, again),
            "pull" => self.sync_now(true),
            "status" => Some(crate::sync::load_state(&self.paths.dir, &remote.url, &key)),
            w => return Err(err(format!("sync: unknown {w:?} (status, push or pull)"))),
        }
        .unwrap_or_default();
        let on_server = if what == "status" { remote.count(&key).map_err(|e| e.to_string()) } else { Err(String::new()) };
        // What the server should hold of the records sent or taken from it.
        let upto = self.store.query(&format!("SELECT COUNT(*) FROM memory WHERE seq <= {} LIMIT 1", st.pushed_seq)).ok().and_then(|rs| rs.rows.first().and_then(|r| r[0].as_i64())).unwrap_or(0).max(0) as u64;
        let warning = on_server.as_ref().ok().and_then(|n| crate::sync::short_on_server(upto, *n));
        Ok(json!({
            "warning": warning,
            "server": remote.url, "syncs": true, "why": crate::sync::syncs(&home, &key), "scope": crate::sync::scope_line(&home),
            "token": remote.has_token(), "key_from": remote.key_from, "project": self.project, "synced_as": key,
            "local_records": self.store.len(), "last_seq": self.store.last_seq(),
            "pushed_seq": st.pushed_seq, "pushed": st.pushed, "pulled": st.pulled,
            "last_push": st.last_push, "last_pull": st.last_pull, "last_error": st.last_error,
            "server_records": on_server.as_ref().ok(), "server_error": on_server.err().filter(|e| !e.is_empty()),
        }))
    }

    fn with_session<T>(&self, sid: &str, f: impl FnOnce(&mut Session) -> T) -> T {
        let mut st = self.state.lock().unwrap();
        st.active = sid.to_string();
        let s = st.sessions.entry(sid.to_string()).or_default();
        s.last_seen = now();
        let out = f(s);
        self.dirty.store(true, Ordering::Release);
        out
    }

    fn active_session(&self) -> (String, i64) {
        let st = self.state.lock().unwrap();
        let w = st.sessions.get(&st.active).map_or(0, |s| s.window_start);
        (st.active.clone(), w)
    }

    /// The session a request comes from, and where its window starts:
    /// the one its `caller` names ([`caller`]), not the session whose hook
    /// came last, which is another one whenever two sessions share the
    /// store (the worktrees of a repository share one daemon; ANA-201).
    /// - The Claude Code process's session (a hook said its pid): its
    ///   current one, which after a `/clear` is the new session.
    /// - Else the session Claude Code named in the caller's environment.
    /// - Else, for the MCP server, the active session (an older Claude Code
    ///   that names neither), and for the CLI none: a person typed it.
    ///
    /// A request without `caller` (an older client) gets the active
    /// session, as before. None (or an empty id) means no session.
    fn caller_session(&self, req: &Json) -> Option<(String, i64)> {
        let Some(c) = req.get("caller").filter(|c| c.is_object()) else {
            return Some(self.active_session()).filter(|(s, _)| !s.is_empty());
        };
        let pid = c.get("pid").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let named = c.get("session").and_then(|v| v.as_str()).unwrap_or("");
        let mcp = c.get("via").and_then(|v| v.as_str()) == Some("mcp");
        let st = self.state.lock().unwrap();
        let by_pid = st.sessions.iter().find(|(_, s)| pid > 0 && s.pid == pid);
        if let Some((id, s)) = by_pid.or_else(|| st.sessions.get_key_value(named)) {
            return Some((id.clone(), s.window_start));
        }
        if mcp && !named.is_empty() {
            return Some((named.to_string(), 0));
        }
        if mcp {
            let w = st.sessions.get(&st.active).map_or(0, |s| s.window_start);
            return Some((st.active.clone(), w)).filter(|(s, _)| !s.is_empty());
        }
        None
    }

    /// `base` for a memory tool's search: without what the asking
    /// session's window holds already (unless `include_current`), but with
    /// the notes written during it (ANA-210).
    fn tool_filter(&self, req: &Json, mut base: Filter) -> Filter {
        if !req.get("include_current").and_then(|v| v.as_bool()).unwrap_or(false) {
            if let Some(w) = self.caller_session(req) {
                base.exclude_window = Some(w);
                base.window_notes = true;
            }
        }
        base
    }

    // ------------------------------------------------------------ hooks

    fn hook(self: &Arc<Self>, event: &str, p: &Json) -> Result<(Json, Option<Deferred>)> {
        let sid = p.get("session_id").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
        let deep_cfg = self.deep_cfg();
        let mode = deep_cfg.mode.clone();
        let ctx_out = |event: &str, text: String| -> Json {
            if text.is_empty() {
                json!({"output": null})
            } else {
                json!({"output": {"hookSpecificOutput": {"hookEventName": event, "additionalContext": text}}})
            }
        };
        match event {
            "SessionStart" => {
                let source = p.get("source").and_then(|v| v.as_str()).unwrap_or("startup").to_string();
                let next = self.store.last_seq() + 1;
                let last_prompts = self.with_session(&sid, |s| {
                    match source.as_str() {
                        "resume" => {}
                        "compact" => {
                            if now() - s.last_compact > 120 {
                                s.window_start = next;
                            }
                        }
                        _ => s.window_start = next,
                    }
                    if source != "resume" {
                        s.injected.clear();
                        s.annotated.clear();
                    }
                    s.last_prompts.clone()
                });
                let mut ck = self.checker_in(&self.tree(&sid));
                let pinned = self.store.pinned();
                // What the notes hold now; what changes with every session
                // (counts, where the last one left off) comes with the first
                // prompt, so that this text is the same from one session to
                // the next and the prompt cache holds it (JONAH-7).
                let b = recall::briefing_parts(&pinned, &sid, self.cfg.briefing_chars, now(), Some(&mut ck))?;
                let mut text = b.text.clone();
                if !b.ids.is_empty() || !b.tail_ids.is_empty() {
                    let inputs = crate::receipt::Inputs::briefing(&sid, self.cfg.briefing_chars);
                    let rows = crate::receipt::retrieve(&pinned, &inputs);
                    if let Ok(mut out) = rows {
                        // The receipt names what was shown, checks and all.
                        out.ids = b.ids.iter().chain(&b.tail_ids).cloned().collect();
                        let turn = self.state.lock().unwrap().sessions.get(&sid).map_or(0, |s| s.turn);
                        self.write_receipt(&sid, turn, "briefing", &pinned, inputs, &out, &format!("{}\n{}", b.text, b.tail));
                    }
                }
                // A new conversation's first request is cached; after a
                // compaction or on a resume the conversation goes on, and
                // the tail (the checkpoint's open tasks) is needed now.
                let fresh = !matches!(source.as_str(), "compact" | "resume");
                if !fresh && !b.tail.is_empty() {
                    text = if text.is_empty() { b.tail.clone() } else { format!("{text}\n{}", b.tail) };
                }
                self.with_session(&sid, |s| {
                    s.injected.extend(b.ids);
                    if fresh {
                        s.tail = (!b.tail.is_empty()).then(|| (b.tail, b.tail_ids));
                    } else {
                        s.injected.extend(b.tail_ids);
                        s.tail = None;
                    }
                });
                if source == "compact" && !last_prompts.is_empty() {
                    let q: Vec<String> = last_prompts.iter().rev().take(2).cloned().collect();
                    if let Some((r, _)) = self.recall_for(&sid, &q.join("\n"), true, "compact")? {
                        text += "\n\nRecords relevant to the work in progress, which the compaction summary may have left out:\n";
                        text += &r;
                    }
                }
                // What doctor would report that the daemon sees itself
                // (distillation failing, a newer version): said at the top.
                let home = crate::paths::home();
                let notice = crate::health::session_notice(&self.paths.dir, &home);
                if !notice.model.is_empty() {
                    let head = notice.model.join("\n");
                    text = if text.is_empty() { head } else { format!("{head}\n\n{text}") };
                }
                self.log(&json!({"ts": now(), "event": event, "session": sid, "source": source, "chars": text.len()}));
                let mut out = ctx_out("SessionStart", text);
                if !notice.user.is_empty() {
                    if out["output"].is_null() {
                        out["output"] = json!({});
                    }
                    out["output"]["systemMessage"] = json!(notice.user.join("\n"));
                }
                Ok((out, Some(self.after(move |me| {
                    me.keep_after_recall();
                    // The checkout is looked at again (no fetch) every few hours.
                    if crate::version::cached(&home, crate::version::CHECK_EVERY).is_none() {
                        crate::version::check(&home, None);
                    }
                }))))
            }
            "UserPromptSubmit" => {
                // Redacted at once: the prompt is kept in the session state
                // (state.json) and in the context of the turn's records.
                // A harness's envelopes around the prompt go first: recalled
                // later, they read as the conversation ([`crate::envelope`]).
                let prompt = crate::redact::redact(&crate::envelope::clean(p.get("prompt").and_then(|v| v.as_str()).unwrap_or("")));
                let (turn, recent, tail) = self.with_session(&sid, |s| {
                    s.turn += 1;
                    // The briefing's part that changes every session.
                    let tail = s.tail.take().inspect(|(_, ids)| s.injected.extend(ids.iter().cloned()));
                    s.shown_before = (s.turn, s.injected.clone());
                    s.episode = EpisodeAcc { prompt: prompt.clone(), ..Default::default() };
                    s.episode_stored = false;
                    let recent: Vec<String> = s.last_prompts.iter().rev().take(2).rev().cloned().collect();
                    s.last_prompts.push_back(prompt.chars().take(1000).collect());
                    while s.last_prompts.len() > 4 {
                        s.last_prompts.pop_front();
                    }
                    (s.turn, recent, tail)
                });
                // Recall first: the prompt is in the live window anyway, and
                // storing it afterwards keeps the write (and the memtable
                // rebuild it causes) off the prompt's critical path.
                let recalled = self.recall_for(&sid, &prompt, false, "prompt")?;
                // A question that stands on its own, for which nothing was
                // recalled, gets nothing: the tail waits for the session's
                // next prompt, no search runs after it, and a session that
                // starts with the question the last one started with sends
                // the same first request, which the prompt cache serves
                // (JONAH-202). Claude Code puts every hook's text after the
                // prompt, so the briefing is cached across sessions only then.
                let alone = recalled.is_none() && recall::stands_alone(&prompt);
                if alone {
                    self.with_session(&sid, |s| s.episode.alone = true);
                }
                let mut text = recalled.map(|x| x.0).unwrap_or_default();
                if let Some((t, ids)) = tail {
                    if alone {
                        self.with_session(&sid, |s| {
                            for i in &ids {
                                s.injected.remove(i);
                                s.shown_before.1.remove(i);
                            }
                            s.tail = Some((t, ids));
                        });
                        self.log(&json!({"ts": now(), "event": "tail_held", "session": sid, "turn": turn}));
                    } else {
                        text = if text.is_empty() { t } else { format!("{t}\n{text}") };
                    }
                }
                // The previous turn's search, when it finished after that turn
                // ended: offered now, once.
                if let Some(t) = self.take_late_deep(&sid, turn - 1) {
                    self.log(&json!({"ts": now(), "event": "deep_late", "session": sid, "turn": turn - 1, "chars": t.len()}));
                    let t = format!("ChloeCtx's search for your previous request finished after that turn ended:\n{t}");
                    text = if text.is_empty() { t } else { format!("{text}\n\n{t}") };
                }
                let deep = !alone && deep_cfg.wants(&prompt, self.store.len());
                let deep_async = deep && mode == "async";
                if deep && mode == "sync" {
                    let (t, _) = self.deep_for(&sid, &prompt, &recent, "sync");
                    if !t.is_empty() {
                        text = if text.is_empty() { t } else { format!("{text}\n\n{t}") };
                    }
                }
                {
                    let mut jobs = self.deep_jobs.lock().unwrap();
                    if deep_async {
                        jobs.insert(sid.clone(), DeepJob { turn, ..Default::default() });
                    } else {
                        jobs.remove(&sid);
                    }
                }
                Ok((
                    ctx_out("UserPromptSubmit", text),
                    Some(self.after(move |me| {
                        // The receipt's snapshot holds the version the recall
                        // read, so it is kept before the prompt is stored;
                        // when none is due (at most one an hour by default),
                        // the prompt is stored at once, ahead of the turn's
                        // tool calls.
                        me.save_receipts();
                        let due = me.keep_pending.load(Ordering::Acquire) && me.snapshot_due();
                        if due {
                            me.keep_after_recall();
                        }
                        let by = me.source_of(&sid);
                        let mut drafts = capture::prompt_drafts_by(&prompt, &by);
                        if alone {
                            for d in drafts.iter_mut().filter(|d| d.kind == Kind::Prompt) {
                                d.tag = crate::record::ASIDE.into();
                            }
                        }
                        if let Err(e) = me.store.append(&sid, turn, now(), drafts) {
                            me.log(&json!({"ts": now(), "error": e.to_string(), "event": "UserPromptSubmit"}));
                        }
                        if !due {
                            me.keep_after_recall();
                        }
                        me.store.warm();
                        if deep_async {
                            let (t, res) = me.deep_for(&sid, &prompt, &recent, "async");
                            let mut jobs = me.deep_jobs.lock().unwrap();
                            if let Some(j) = jobs.get_mut(&sid).filter(|j| j.turn == turn) {
                                j.done = true;
                                j.text = t;
                                j.confidence = res.as_ref().map(|r| r.confidence.clone()).unwrap_or_default();
                                j.ids = res.map(|r| r.ids()).unwrap_or_default();
                            }
                        }
                    })),
                ))
            }
            "PreToolUse" => {
                // When the call was made, which its result's record notes.
                if let Some(id) = p.get("tool_use_id").and_then(|v| v.as_str()).filter(|i| !i.is_empty()) {
                    let mut st = self.state.lock().unwrap();
                    let s = st.sessions.entry(sid.clone()).or_default();
                    if s.calls.len() > 64 {
                        s.calls.clear();
                    }
                    s.calls.insert(id.to_string(), now());
                }
                if !self.cfg.file_notes {
                    return Ok((json!({"output": null}), None));
                }
                let tool = p.get("tool_name").and_then(|v| v.as_str()).unwrap_or("");
                let path = p.get("tool_input").and_then(|i| i.get("file_path")).and_then(|v| v.as_str()).unwrap_or("");
                if !matches!(tool, "Edit" | "MultiEdit" | "Write") || path.is_empty() {
                    return Ok((json!({"output": null}), None));
                }
                let tree = self.tree(&sid);
                let rel = path.strip_prefix(tree.as_str()).map(|x| x.trim_start_matches('/')).unwrap_or(path).to_string();
                let mut text = self.file_notes(&sid, &rel)?;
                if self.cfg.next_files {
                    let next = self.next_file_notes(&sid, &rel, &text)?;
                    if !next.is_empty() {
                        self.log(&json!({"ts": now(), "event": "next_files", "session": sid, "file": rel, "chars": next.len()}));
                        text = if text.is_empty() { next } else { format!("{text}\n{next}") };
                    }
                }
                Ok((ctx_out("PreToolUse", text), None))
            }
            "PostToolUse" | "PostToolUseFailure" => {
                let tool = p.get("tool_name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                // A forget's own call names what it forgot: it is not
                // recorded, and its turn's reply is neither stored nor
                // distilled (JONAH-2, [`Daemon::end_forget_turn`]).
                if is_forget_tool(&tool) {
                    let turn = self.with_session(&sid, |s| s.turn);
                    let text = p.get("tool_input").and_then(|i| i.get("text")).and_then(|v| v.as_str());
                    self.store.forgetting.forgot_in(&sid, turn, text, now());
                    self.log(&json!({"ts": now(), "event": "forget_turn", "session": sid, "turn": turn}));
                    return Ok((json!({"output": null}), None));
                }
                let input = p.get("tool_input").cloned().unwrap_or(Json::Null);
                let mut resp = p.get("tool_response").cloned().unwrap_or(Json::Null);
                if event == "PostToolUseFailure" {
                    resp = json!({"is_error": true, "error": p.get("error").cloned().unwrap_or(resp)});
                }
                let project = self.tree(&sid);
                let fixes_on = self.cfg.error_fixes;
                let end = now();
                let call_id = p.get("tool_use_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let tpath = p.get("transcript_path").and_then(|v| v.as_str()).map(String::from);
                let (turn, ctx, fixed, started) = self.with_session(&sid, |s| {
                    let started = s.calls.remove(&call_id);
                    s.episode.add_tool(&tool, &input, &resp, &project);
                    let fixed = if fixes_on { s.fixes.observe(&tool, &input, &resp, &project) } else { Vec::new() };
                    (s.turn, format!("Turn {} request: {}", s.turn, s.episode.prompt.chars().take(200).collect::<String>()), fixed, started)
                });
                let mut text = self.take_deep(&sid, turn, Duration::ZERO, false).unwrap_or_default();
                if fixes_on && tool == "Bash" {
                    let fix = self.fix_for(&sid, &resp);
                    if !fix.is_empty() {
                        text = if text.is_empty() { fix } else { format!("{text}\n\n{fix}") };
                    }
                }
                Ok((
                    ctx_out(event, text),
                    Some(self.after(move |me| {
                        // A file the call read or changed is anchored: its
                        // branch, commit and content hash now, so a later
                        // recall into another worktree can tell its code
                        // from this one's ([`anchor::Checker::label_record`]).
                        let file = ["file_path", "notebook_path"].iter().find_map(|k| input.get(*k).and_then(|v| v.as_str())).unwrap_or("");
                        let rel = file.strip_prefix(project.as_str()).map(|x| x.trim_start_matches('/')).unwrap_or("");
                        let aline = if matches!(tool.as_str(), "Read" | "Edit" | "MultiEdit" | "Write" | "NotebookEdit") && !rel.is_empty() {
                            anchor::of_file(Path::new(&project), rel).to_line()
                        } else {
                            String::new()
                        };
                        let agent = me.state.lock().unwrap().sessions.get(&sid).map(|s| s.agent.clone()).unwrap_or_default();
                        let mut drafts: Vec<Draft> = capture::tool_drafts(&tool, &input, &resp, &project)
                            .into_iter()
                            .map(|d| {
                                let mut c = ctx.clone();
                                for extra in [d.ctx.as_str(), if d.kind == Kind::ToolCall || d.kind == Kind::ToolResult { aline.as_str() } else { "" }] {
                                    if !extra.is_empty() {
                                        c = format!("{c}\n{extra}");
                                    }
                                }
                                let memo = d.tag == capture::MEMORY_FILE_TAG;
                                let d = d.ctx(c);
                                if memo { d.by(&crate::record::Source::Assistant(agent.clone())) } else { d }
                            })
                            .collect();
                        // How long the call took: from its PreToolUse, else
                        // from its message in the transcript.
                        let started = started.or_else(|| tpath.as_deref().and_then(|t| crate::transcript::call_ts(Path::new(t), &call_id)));
                        if let Some(t0) = started {
                            capture::took(&mut drafts, end - t0);
                        }
                        if !fixed.is_empty() {
                            let from = me.prompt_of(&sid, turn);
                            for f in &fixed {
                                let d = f.draft(&ctx);
                                drafts.push(match &from {
                                    Some(p) => d.link("from", p.clone()),
                                    None => d,
                                });
                            }
                            me.log(&json!({"ts": now(), "event": "fix", "session": sid, "turn": turn, "fixes": fixed.len()}));
                        }
                        if let Err(e) = me.store.append(&sid, turn, end, drafts) {
                            me.log(&json!({"ts": now(), "error": e.to_string(), "event": "PostToolUse"}));
                        }
                        me.store.warm();
                    })),
                ))
            }
            "Stop" => {
                let answer = p.get("last_assistant_message").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let active = p.get("stop_hook_active").and_then(|v| v.as_bool()).unwrap_or(false);
                let cur = self.with_session(&sid, |s| s.turn);
                // By default the end of a turn neither waits for a search
                // still running nor asks the model to go on: what the search
                // finds is offered at the next prompt (CTX-1: waiting held
                // short turns ~10 s, and the block that followed made Claude
                // answer twice). With `stop_recheck` turned on, it waits a few
                // seconds at most and, when the search found records the turn
                // did not see, asks the model once to check its answer
                // against them, without repeating it.
                if !active && mode == "async" && deep_cfg.stop_recheck {
                    if let Some(t) = self.take_deep(&sid, cur, deep_cfg.stop_wait(), true) {
                        self.log(&json!({"ts": now(), "event": "deep_recheck", "session": sid, "turn": cur, "chars": t.len()}));
                        let reason = format!(
                            "{t}\n\nChloeCtx found these records for this request after the turn's last tool call (`stop_recheck` is on). If one of them changes your answer or your changes, make the correction now and say in a sentence or two what changed. If none does, end your turn with one short line saying so; do not repeat your answer."
                        );
                        return Ok((json!({"output": {"decision": "block", "reason": reason}}), None));
                    }
                }
                // Without it, a search that has already found a rule or a
                // decision of the user's that the session was not shown is
                // given now, without waiting: the turn may be the session's
                // only one (JONAH-3: the find came after a `-p` answer that
                // went against the user's rule, and never reached it).
                if !active && mode == "async" && !deep_cfg.stop_recheck {
                    if let Some(t) = self.take_unseen_rule(&sid, cur) {
                        self.log(&json!({"ts": now(), "event": "deep_rule_recheck", "session": sid, "turn": cur, "chars": t.len()}));
                        let reason = format!(
                            "{t}\n\nChloeCtx's search for this request finished as you answered, and found a rule or decision of the user's that bears on it. If your answer goes against one of them, correct it now in a sentence or two. If none does, end your turn with one short line saying so; do not repeat your answer."
                        );
                        return Ok((json!({"output": {"decision": "block", "reason": reason}}), None));
                    }
                }
                // A turn that called memory_forget: its reply may say again
                // what it forgot.
                let forgot = self.store.forgetting.is_forget_turn(&sid, cur);
                let (turn, ep, first, ex) = self.with_session(&sid, |s| {
                    let first = !s.episode_stored;
                    s.episode_stored = true;
                    let ex = crate::distill::Exchange { by: Some(crate::record::Source::parse(&s.source)), agent: s.agent.clone(), prev_answer: s.prev_answer.clone() };
                    if first {
                        // Redacted: the state is a file too.
                        let tail: Vec<char> = crate::redact::redact(&answer).chars().collect();
                        s.prev_answer = if forgot { String::new() } else { tail[tail.len().saturating_sub(800)..].iter().collect() };
                        s.answered.push_back(crate::sync::fnv(answer.trim()));
                        while s.answered.len() > ANSWERS_KEPT {
                            s.answered.pop_front();
                        }
                    }
                    (s.turn, s.episode.clone(), first, ex)
                });
                if !first {
                    return Ok((json!({"output": null}), None));
                }
                let tpath = p.get("transcript_path").and_then(|v| v.as_str()).map(String::from);
                if forgot {
                    return Ok((json!({"output": null}), Some(self.after(move |me| me.end_forget_turn(&sid, turn, tpath.as_deref())))));
                }
                Ok((
                    json!({"output": null}),
                    Some(self.after(move |me| {
                        let ctx = format!("Turn {turn} request: {}", ep.prompt.chars().take(200).collect::<String>());
                        if let Some(t) = &tpath {
                            me.sync_narration(&sid, turn, t, &answer, &ctx);
                        }
                        let mut drafts: Vec<Draft> = capture::answer_drafts(&answer).into_iter().map(|d| d.ctx(ctx.clone())).collect();
                        drafts.extend(ep.card(turn, &answer));
                        let aside = ep.only_a_question(&answer);
                        if aside {
                            for d in drafts.iter_mut().filter(|d| d.tag == "-") {
                                d.tag = crate::record::ASIDE.into();
                            }
                        }
                        if let Err(e) = me.store.append(&sid, turn, now(), drafts) {
                            me.log(&json!({"ts": now(), "error": e.to_string(), "event": "Stop"}));
                        }
                        me.store.warm();
                        if aside {
                            // Marked, so that a later daemon or `chloectx distill` leaves it too.
                            let mark = Draft::new(Kind::Event, format!("Turn {turn} was a question that stood on its own, answered without a tool: not distilled")).tag(TURN_SKIPPED);
                            if let Err(e) = me.store.append(&sid, turn, now(), vec![mark]) {
                                me.log(&json!({"ts": now(), "error": e.to_string(), "event": "Stop"}));
                            }
                            me.log(&json!({"ts": now(), "event": "distill_skipped", "session": sid, "turn": turn, "why": "a question that stood on its own"}));
                            return;
                        }
                        let files: Vec<String> = ep.files.iter().cloned().collect();
                        me.distill_turn(&sid, turn, now(), &ep.prompt, &answer, &ep.commands, &files, &ex);
                    })),
                ))
            }
            "PostCompact" => {
                let summary = p.get("compact_summary").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let turn = self.with_session(&sid, |s| s.turn);
                let recs = self.store.append(&sid, turn, now(), capture::summary_drafts(&summary))?;
                let first = recs.first().map_or(self.store.last_seq() + 1, |r| r.seq);
                self.with_session(&sid, |s| {
                    s.window_start = first;
                    s.last_compact = now();
                    s.injected.clear();
                    s.annotated.clear();
                });
                self.log(&json!({"ts": now(), "event": event, "session": sid, "summary_records": recs.len()}));
                Ok((json!({"output": null}), None))
            }
            "PreCompact" => {
                let tpath = p.get("transcript_path").and_then(|v| v.as_str()).map(String::from);
                let (turn, prompt) = self.with_session(&sid, |s| (s.turn, s.episode.prompt.clone()));
                // When it started, for `chloectx report`.
                let started = capture::event_draft(event, p);
                let at = now();
                Ok((
                    json!({"output": null}),
                    Some(self.after(move |me| {
                        if let Some(d) = started {
                            if let Err(e) = me.store.append(&sid, turn, at, vec![d]) {
                                me.log(&json!({"ts": now(), "error": e.to_string(), "event": "PreCompact"}));
                            }
                        }
                        if let Some(t) = &tpath {
                            let ctx = format!("Turn {turn} request: {}", prompt.chars().take(200).collect::<String>());
                            me.sync_narration(&sid, turn, t, "", &ctx);
                        }
                    })),
                ))
            }
            // When the agent waited for the user (a permission prompt, an
            // idle notice) and when a sub-agent finished: recorded for
            // `chloectx report`, never shown ([`Kind::Event`]). Nothing is
            // answered, so a sub-agent is never held back.
            "Notification" | "SubagentStop" => {
                let Some(draft) = capture::event_draft(event, p) else { return Ok((json!({"output": null}), None)) };
                let turn = self.state.lock().unwrap().sessions.get(&sid).map_or(0, |s| s.turn);
                let at = now();
                let event = event.to_string();
                Ok((
                    json!({"output": null}),
                    Some(self.after(move |me| {
                        if let Err(e) = me.store.append(&sid, turn, at, vec![draft]) {
                            me.log(&json!({"ts": now(), "error": e.to_string(), "event": event}));
                        }
                    })),
                ))
            }
            "SessionEnd" => {
                Ok((json!({"output": null}), Some(self.after(move |me| {
                    let _ = me.store.flush();
                }))))
            }
            _ => Ok((json!({"output": null}), None)),
        }
    }

    /// Writes down what a turn decided, ruled or announced (see
    /// [`crate::distill`]): new items that are not near-copies of what is
    /// already noted, replacements (the old item is marked superseded), and
    /// planned work it completed (marked done).
    #[allow(clippy::too_many_arguments)]
    pub fn distill_turn(&self, sid: &str, turn: i64, ts: i64, prompt: &str, answer: &str, commands: &[String], files: &[String], ex: &crate::distill::Exchange) {
        if !self.distill_cfg().active() || (prompt.trim().is_empty() && answer.trim().is_empty()) {
            return;
        }
        self.pending.fetch_add(1, Ordering::AcqRel);
        let _one = self.distilling.lock().unwrap_or_else(|e| e.into_inner());
        // The notes most related to this exchange, wherever they are in the
        // history, and the newest few: the model can update an old note
        // instead of adding a near-copy next to it.
        let about: String = format!("{}\n{}", prompt.chars().take(1500).collect::<String>(), answer.chars().take(1500).collect::<String>());
        let mut known: Vec<Record> = recall::related(&self.store, &about, 30).unwrap_or_default();
        for r in crate::consolidate::live_notes(&self.store, 40).unwrap_or_default() {
            if known.len() >= 40 {
                break;
            }
            if !known.iter().any(|k| k.id == r.id) {
                known.push(r);
            }
        }
        known.sort_by_key(|r| -r.seq);
        let listed: Vec<(String, Kind, String)> = known.iter().rev().map(|r| (r.id.clone(), r.kind, r.text.clone())).collect();
        let t0 = Instant::now();
        match crate::distill::distill_exchange(&self.distill_cfg(), prompt, answer, commands, &listed, ts, ex) {
            Ok(d) => {
                // What the user said again, of their earlier requests and
                // instructions most like this one ([`crate::repeats`]).
                let before: Vec<Record> = if matches!(ex.by, None | Some(crate::record::Source::User)) {
                    let from = self.first_seq_of(sid, turn).unwrap_or(i64::MAX);
                    crate::repeats::said_before(&self.store, prompt, sid, turn, from).unwrap_or_default()
                } else {
                    Vec::new()
                };
                let checked = match crate::repeats::check(&self.distill_cfg(), prompt, &ex.prev_answer, &crate::repeats::listed(&before, sid)) {
                    Ok(c) => c,
                    Err(e) => {
                        self.log(&json!({"ts": now(), "event": "said_again", "session": sid, "turn": turn, "error": e.to_string()}));
                        Default::default()
                    }
                };
                let is_known = |id: &String| known.iter().any(|r| &r.id == id);
                let replaced: Vec<String> = d.replaced.into_iter().filter(|id| is_known(id)).collect();
                let done: Vec<String> = d.done.into_iter().filter(|id| known.iter().any(|r| &r.id == id && r.kind == Kind::Todo)).collect();
                let retired: Vec<String> = d.retired.into_iter().filter(|id| is_known(id) && !replaced.contains(id) && !done.contains(id)).collect();
                // An instruction for this task that the user gave in
                // another session too is a rule of every session now.
                // One that says what a rule says is that rule anew.
                let shown = {
                    let st = self.state.lock().unwrap();
                    st.sessions.get(sid).map(|s| if s.shown_before.0 == turn { s.shown_before.1.clone() } else { s.injected.clone() }).unwrap_or_default()
                };
                let promoted = crate::repeats::promote(d.drafts, &checked.again, sid, &shown, &before);
                // Near-copies of what is noted (other than what they replace,
                // or repeat) add nothing.
                let terms = |t: &str| -> HashSet<String> { chloedb::text::analyze(t).into_iter().collect() };
                let mut seen: Vec<HashSet<String>> = known.iter().filter(|r| !replaced.contains(&r.id) && !promoted.repeats.contains(&r.id)).map(|r| terms(&r.text)).collect();
                let ctx = format!("Turn {turn} request: {}", prompt.chars().take(200).collect::<String>());
                // Each note links to the turn it was written from.
                let source = self.prompt_of(sid, turn);
                let n = promoted.drafts.len();
                let mut drafts = Vec::new();
                let tree = self.tree(sid);
                for x in promoted.drafts {
                    let t = terms(&x.text);
                    let dup = seen.iter().any(|s| {
                        let inter = s.intersection(&t).count() as f64;
                        let union = (s.len() + t.len()) as f64 - inter;
                        union > 0.0 && inter / union >= 0.6
                    });
                    if !dup {
                        seen.push(t);
                        // Anchored to the code the note is about, as it is now.
                        let a = anchor::make(Path::new(&tree), &x.text, &x.files, files);
                        let ctx = if a.is_empty() { ctx.clone() } else { format!("{ctx}\n{}", a.to_line()) };
                        // A rule's counts (its `Sessions:` line) go first.
                        let ctx = if x.ctx.is_empty() { ctx } else { format!("{}\n{ctx}", x.ctx) };
                        let mut x = x.ctx(ctx);
                        if let Some(src) = &source {
                            x = x.link("from", src.clone());
                        }
                        drafts.push(x);
                    }
                }
                let kept = drafts.len();
                // The turn is marked distilled with its notes, in one write:
                // a daemon stopped before it distills the turn again when it
                // starts (ANA-5), and `chloectx distill` leaves it (ANA-1).
                drafts.push(Draft::new(Kind::Event, format!("Distilled turn {turn}: {kept} of {n} notes kept")).tag(TURN_DISTILLED));
                let stored: Vec<Record> = match self.store.append(sid, turn, ts, drafts) {
                    Ok(r) => r.into_iter().filter(|r| r.kind != Kind::Event).collect(),
                    Err(e) => {
                        self.log(&json!({"ts": now(), "error": e.to_string(), "event": "distill"}));
                        Vec::new()
                    }
                };
                // A note is superseded only by a stored note that names it
                // and says the same thing anew ([`successor_of`]); one whose
                // replacement was dropped, or does not cover it, stays.
                let replaced: Vec<String> = replaced
                    .into_iter()
                    .filter(|id| {
                        let Some(old) = known.iter().find(|r| &r.id == id) else { return false };
                        match successor_of(old, &stored) {
                            Some(new) => self.store.supersede(id, &new).unwrap_or(false),
                            None => false,
                        }
                    })
                    .collect();
                for id in &done {
                    let _ = self.store.retag(id, "done");
                }
                // Notes about what this turn scrapped (JONAH-201): replaced
                // by its note that says so (the one that shares most of
                // their words), else retired; either way out of the briefing.
                let retired: Vec<String> = retired
                    .into_iter()
                    .filter(|id| {
                        let Some(old) = known.iter().find(|r| &r.id == id) else { return false };
                        let o = terms(&old.text);
                        let by = stored
                            .iter()
                            .filter(|r| r.kind.is_knowledge() && r.kind != Kind::Todo && r.tag != crate::record::TASK_TAG)
                            .map(|r| (terms(&r.text).intersection(&o).count(), r))
                            .filter(|(n, _)| *n >= 2)
                            .max_by_key(|(n, r)| (*n, -r.seq))
                            .map(|(_, r)| r.id.clone());
                        match by {
                            Some(new) => self.store.supersede(id, &new).unwrap_or(false),
                            None => self.store.retag(id, crate::record::RETIRED).unwrap_or(false),
                        }
                    })
                    .collect();
                // The decision the prompt stated, kept at once in the user's
                // words, is replaced by the turn's note that says it (ANA-204).
                let stated = self.supersede_stated(sid, turn, &stored);
                let repeated = crate::repeats::supersede_repeated(&self.store, &stored);
                self.store.warm();
                self.log(&json!({"ts": now(), "event": "distill", "session": sid, "turn": turn, "items": n, "kept": kept,
                    "replaced": replaced, "done": done, "retired": retired, "stated": stated, "said_again": checked.again.len(), "said_again_dropped": checked.dropped.len(), "repeated": repeated, "cost_usd": d.cost_usd + checked.cost_usd, "tokens": d.usage.json(), "ms": t0.elapsed().as_millis() as u64}));
            }
            Err(e) => {
                self.log(&json!({"ts": now(), "event": "distill", "session": sid, "turn": turn, "error": e.to_string()}));
                // Failed, not lost: doctor says why, and `chloectx distill`
                // tries it again; a restart does not.
                let mark = Draft::new(Kind::Event, format!("Distilling turn {turn} failed: {}", crate::llm::clip(&e.to_string(), 200))).tag(TURN_FAILED);
                let _ = self.store.append(sid, turn, ts, vec![mark]);
            }
        }
        let since = self.distilled_since.fetch_add(1, Ordering::AcqRel) + 1;
        let cc = &self.cfg.consolidate;
        if cc.active() && since >= cc.every_turns {
            let live = crate::consolidate::live_notes(&self.store, 2000).map(|v| v.len()).unwrap_or(0);
            if live >= cc.min_live {
                self.distilled_since.store(0, Ordering::Release);
                self.consolidate_now();
            }
        }
        self.pending.fetch_sub(1, Ordering::AcqRel);
    }

    /// Merges related notes (the caller holds the distillation lock).
    fn consolidate_now(&self) -> Json {
        let t0 = Instant::now();
        match crate::consolidate::run(&self.store, &self.cfg.consolidate, &self.distill_cfg(), now()) {
            Ok(r) => {
                let j = json!({"ts": now(), "event": "consolidate", "live_before": r.live_before, "groups": r.groups, "replaced": r.replaced,
                    "written": r.written, "cost_usd": r.cost_usd, "tokens": r.usage.json(), "errors": r.errors, "ms": t0.elapsed().as_millis() as u64});
                self.log(&j);
                j
            }
            Err(e) => {
                let j = json!({"ts": now(), "event": "consolidate", "error": e.to_string()});
                self.log(&j);
                j
            }
        }
    }

    fn op_consolidate(&self) -> Result<Json> {
        let _one = self.distilling.lock().unwrap_or_else(|e| e.into_inner());
        self.distilled_since.store(0, Ordering::Release);
        let j = self.consolidate_now();
        let text = format!("{j}");
        Ok(json!({"text": text, "report": j}))
    }

    /// Checks notes' anchors against this project's working tree.
    /// Remembers the settings a hook ran with (see `from_hooks`).
    fn note_hook_settings(&self, req: &Json) {
        // CHLOECTX_RECEIPTS as the hook saw it, unless config.json or
        // `chloectx receipts on|off` decided.
        if let Some(on) = req.get("receipts").and_then(|v| v.as_bool()) {
            if !self.cfg.receipts.explicit && !self.no_hook_receipts.load(Ordering::Acquire) {
                self.receipts_on.store(on, Ordering::Release);
            }
        }
        if let Some(c) = req.get("claude").and_then(|v| v.as_str()) {
            crate::llm::set_hook_cli(Path::new(c));
        }
        if let Some(on) = req.get("distill").and_then(|v| v.as_bool()) {
            self.hook_distill.store(if on { 1 } else { 2 }, Ordering::Release);
        }
        let model = req.get("distill_model").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let deep = req.get("deep").cloned().and_then(|d| serde_json::from_value::<crate::deep::DeepConfig>(d).ok());
        let mut h = self.from_hooks.lock().unwrap();
        if !model.is_empty() {
            h.0 = model;
        }
        if deep.is_some() {
            h.1 = deep;
        }
    }

    /// The distiller's settings, with the model the hooks asked for.
    fn distill_cfg(&self) -> crate::distill::DistillConfig {
        let mut c = self.cfg.distill.clone();
        // CHLOECTX_DISTILL=0 in Claude Code's environment turns it off,
        // whichever process started the daemon.
        if self.hook_distill.load(Ordering::Acquire) == 2 {
            c.enabled = false;
        }
        let h = self.from_hooks.lock().unwrap();
        if !h.0.is_empty() && std::env::var("CHLOECTX_DISTILL_MODEL").map_or(true, |m| m.is_empty()) {
            c.model = h.0.clone();
        }
        c
    }

    /// Query-understanding settings: as the hooks resolved them, else the daemon's.
    fn deep_cfg(&self) -> crate::deep::DeepConfig {
        self.from_hooks.lock().unwrap().1.clone().unwrap_or_else(|| self.cfg.deep.clone())
    }

    /// A checker of notes against the files of `tree` (a worktree).
    fn checker_in(&self, tree: &str) -> Checker {
        Checker::new(Path::new(tree)).with_sessions(anchor::session_branches(&self.store))
    }

    /// The folder session `sid` runs in: the worktree its hooks named,
    /// else the daemon's project.
    fn tree(&self, sid: &str) -> String {
        let st = self.state.lock().unwrap();
        st.sessions.get(sid).map(|s| s.worktree.clone()).filter(|w| !w.is_empty()).unwrap_or_else(|| self.project.clone())
    }

    /// The folder a request came from (`worktree`, which the clients send),
    /// else the daemon's project.
    fn tree_of(&self, req: &Json) -> String {
        req.get("worktree").and_then(|v| v.as_str()).filter(|w| !w.is_empty()).map(String::from).unwrap_or_else(|| self.project.clone())
    }

    /// Remembers the folder a session's hooks run in (they may come from
    /// any worktree of the repository) and, when it is new for the session,
    /// logs it with the branch checked out there.
    fn note_worktree(&self, sid: &str, req: &Json) {
        let Some(wt) = req.get("worktree").and_then(|v| v.as_str()).filter(|w| !w.is_empty()) else { return };
        let changed = {
            let mut st = self.state.lock().unwrap();
            let s = st.sessions.entry(sid.to_string()).or_default();
            let changed = s.worktree != wt;
            if changed {
                s.worktree = wt.to_string();
            }
            changed
        };
        // The branch is looked at on every hook (two small file reads): a
        // session can switch it.
        let (head, branch) = anchor::git_head(Path::new(wt));
        let moved = {
            let mut st = self.state.lock().unwrap();
            let s = st.sessions.entry(sid.to_string()).or_default();
            let moved = !branch.is_empty() && s.branch != branch;
            if moved {
                s.branch = branch.clone();
            }
            moved
        };
        if changed || moved {
            self.dirty.store(true, Ordering::Release);
            self.log(&json!({"ts": now(), "event": "worktree", "session": sid, "worktree": wt, "branch": branch, "head": head}));
        }
        if moved {
            // Kept with the records, so that every record of the session
            // has a branch and a commit: its session's at the time
            // ([`anchor::Checker::with_sessions`]).
            let turn = self.state.lock().unwrap().sessions.get(sid).map_or(0, |s| s.turn);
            let d = Draft::new(Kind::Event, anchor::Anchor { head, branch, files: Vec::new(), idents: Vec::new() }.to_line()).tag(anchor::WORKTREE_TAG).ctx(format!("Worktree: {wt}"));
            if let Err(e) = self.store.append(sid, turn, now(), vec![d]) {
                self.log(&json!({"ts": now(), "error": e.to_string(), "event": "worktree"}));
            }
        }
    }

    /// Who wrote the prompt a hook carries: a source Workbench noted for
    /// its text (`source`, from `CHLOECTX_PROMPT_LOG`), else for the
    /// session's first prompt the source of the task it was started with
    /// (`session_source`, `CHLOECTX_PROMPT_SOURCE`), else the user.
    fn note_source(&self, sid: &str, req: &Json) {
        let noted = req.get("source").and_then(|v| v.as_str()).filter(|s| !s.is_empty());
        let first = req.get("session_source").and_then(|v| v.as_str()).filter(|s| !s.is_empty());
        let mut st = self.state.lock().unwrap();
        let s = st.sessions.entry(sid.to_string()).or_default();
        // A coordinator's brief to the session (a relay) is that agent's
        // instruction, as another agent's brief is, not the user's words.
        let relay = req.get("payload").and_then(|p| p.get("prompt")).and_then(|v| v.as_str()).and_then(crate::envelope::relay_from);
        let src = match (noted, first, relay) {
            (Some(n), _, _) => n.to_string(),
            (None, Some(f), _) if s.turn == 0 => f.to_string(),
            (_, _, Some(from)) => format!("agent:{from}"),
            _ => "user".to_string(),
        };
        s.source = crate::record::Source::parse(&src).as_str();
        self.dirty.store(true, Ordering::Release);
    }

    /// Who wrote the current turn's prompt.
    fn source_of(&self, sid: &str) -> crate::record::Source {
        crate::record::Source::parse(&self.state.lock().unwrap().sessions.get(sid).map(|s| s.source.clone()).unwrap_or_default())
    }

    /// Runs query understanding for a prompt of a session: the context
    /// block (records not shown to the session yet) and the result.
    fn deep_for(&self, sid: &str, prompt: &str, recent: &[String], mode: &str) -> (String, Option<crate::deep::DeepResult>) {
        let (w, shown) = {
            let st = self.state.lock().unwrap();
            let s = st.sessions.get(sid).cloned().unwrap_or_default();
            (s.window_start, s.injected)
        };
        // What it puts into the prompt by itself quotes no third party
        // (audit CTX-32): memory_search and memory_ask still find those.
        let filter = Filter { exclude_window: Some((sid.to_string(), w)), first_party: true, ..Default::default() };
        let tree = self.tree(sid);
        let project = Path::new(&tree);
        let res = crate::deep::run(&self.store, &self.deep_cfg(), &self.distill_cfg(), prompt, recent, &filter, Some(project), now());
        let text = crate::deep::format(&res, &shown, now(), Some(self.checker_in(&tree)));
        let mut log = crate::deep::log_json(&res);
        log["ts"] = json!(now());
        log["event"] = json!("deep");
        log["session"] = json!(sid);
        log["mode"] = json!(mode);
        log["chars"] = json!(text.len());
        self.log(&log);
        if !text.is_empty() {
            let ids = res.ids();
            let mut st = self.state.lock().unwrap();
            if let Some(s) = st.sessions.get_mut(sid) {
                if mode == "sync" {
                    s.injected.extend(ids);
                }
            }
            self.dirty.store(true, Ordering::Release);
        }
        (text, Some(res))
    }

    /// The sequence number of a turn's first record (its prompt, stored
    /// with the picks from its words).
    /// Supersedes the decisions a turn's prompt stated ([`capture::decision_sentences`],
    /// tagged `stated`) by the turn's distilled decision or rule that holds
    /// most of their words, when it holds at least half: two copies of one
    /// decision are one. Returns the ids superseded.
    fn supersede_stated(&self, sid: &str, turn: i64, stored: &[Record]) -> Vec<String> {
        let terms = |t: &str| -> HashSet<String> { chloedb::text::analyze(t).into_iter().collect() };
        let Ok(rs) = self.store.query_with(
            &format!("SELECT {} FROM memory WHERE session = :session AND turn = {turn} AND kind = 'decision' AND tag = 'stated' LIMIT 20", crate::record::COLUMNS),
            &Params::new().set("session", Value::Str(sid.to_string())),
        ) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for old in rs.rows.iter().filter_map(|r| crate::store::row_record(r)) {
            let o = terms(old.text.trim_start_matches("The user decided:"));
            let best = stored
                .iter()
                .filter(|r| matches!(r.kind, Kind::Decision | Kind::Preference) && r.tag != crate::record::TASK_TAG)
                .map(|r| (terms(&r.text).intersection(&o).count(), r))
                .max_by_key(|(n, r)| (*n, -r.seq));
            if let Some((n, new)) = best {
                if !o.is_empty() && n * 2 >= o.len() && self.store.supersede(&old.id, &new.id).unwrap_or(false) {
                    out.push(old.id.clone());
                }
            }
        }
        out
    }

    fn first_seq_of(&self, sid: &str, turn: i64) -> Option<i64> {
        self.store
            .query_with(
                &format!("SELECT seq FROM memory WHERE session = :session AND turn = {turn} ORDER BY seq ASC LIMIT 1"),
                &Params::new().set("session", Value::Str(sid.to_string())),
            )
            .ok()
            .and_then(|rs| rs.rows.first().and_then(|r| r[0].as_i64()))
    }

    /// The id of a turn's prompt record, which notes and fixes link to.
    fn prompt_of(&self, sid: &str, turn: i64) -> Option<String> {
        self.store
            .query_with(
                &format!("SELECT id FROM memory WHERE session = :session AND turn = {turn} AND kind = 'prompt' ORDER BY seq ASC LIMIT 1"),
                &Params::new().set("session", Value::Str(sid.to_string())),
            )
            .ok()
            .and_then(|rs| rs.rows.first().and_then(|r| r[0].as_str().map(String::from)))
    }

    /// For a failed command: the fixes known for its error that this
    /// session has not been shown, as context ([`crate::fixes`]).
    fn fix_for(&self, sid: &str, resp: &Json) -> String {
        let text = capture::response_text(resp);
        if !capture::looks_like_error(resp, &text) {
            return String::new();
        }
        let Some((sig, _)) = crate::fixes::signature(&text, &self.tree(sid)) else { return String::new() };
        // Not one the model was shown, nor one of this session's live window
        // (in its context already).
        let (shown, window) = self.with_session(sid, |s| (s.injected.clone(), s.window_start));
        let found: Vec<Record> = match crate::fixes::known(&self.store, &sig, 4) {
            Ok(f) => f.into_iter().filter(|r| !shown.contains(&r.id) && !(r.session == sid && r.seq >= window)).take(2).collect(),
            Err(e) => {
                self.log(&json!({"ts": now(), "event": "fix_lookup", "error": e.to_string()}));
                return String::new();
            }
        };
        if found.is_empty() {
            return String::new();
        }
        let ids: Vec<String> = found.iter().map(|r| r.id.clone()).collect();
        self.log(&json!({"ts": now(), "event": "fix_shown", "session": sid, "ids": ids}));
        self.with_session(sid, |s| s.injected.extend(ids));
        crate::fixes::render(&found)
    }

    /// A search of an earlier `turn` that finished after its turn ended and
    /// was not delivered: its text, once, when the ranker was confident and
    /// the session has not been shown all of its picks since.
    fn take_late_deep(&self, sid: &str, turn: i64) -> Option<String> {
        let ids = {
            let jobs = self.deep_jobs.lock().unwrap();
            let j = jobs.get(sid).filter(|j| j.turn == turn && j.done && !j.delivered && !j.text.is_empty())?;
            j.ids.clone()
        };
        let seen = self.with_session(sid, |s| ids.iter().all(|i| s.injected.contains(i)));
        if seen {
            if let Some(j) = self.deep_jobs.lock().unwrap().get_mut(sid) {
                j.delivered = true;
            }
            return None;
        }
        self.take_deep(sid, turn, Duration::ZERO, true)
    }

    /// The finished search of `turn`, once, when it is confident and holds
    /// a rule or decision of the user's that the session was not shown;
    /// otherwise it is left for the next prompt.
    fn take_unseen_rule(&self, sid: &str, turn: i64) -> Option<String> {
        let ids = {
            let jobs = self.deep_jobs.lock().unwrap();
            let j = jobs.get(sid).filter(|j| j.turn == turn && j.done && !j.delivered && !j.text.is_empty() && matches!(j.confidence.as_str(), "high" | "medium"))?;
            j.ids.clone()
        };
        let unseen: Vec<String> = {
            let st = self.state.lock().unwrap();
            let shown = st.sessions.get(sid).map(|s| s.injected.clone()).unwrap_or_default();
            ids.into_iter().filter(|i| !shown.contains(i)).collect()
        };
        let users = |r: &Record| matches!(r.source(), None | Some(crate::record::Source::User));
        let rule = self.store.get(&unseen).unwrap_or_default().iter().any(|r| matches!(r.kind, Kind::Preference | Kind::Decision) && users(r) && !crate::record::closed_tag(&r.tag));
        if !rule {
            return None;
        }
        self.take_deep(sid, turn, Duration::ZERO, true)
    }

    /// The finished search of `turn` not delivered yet, once: waits up to
    /// `wait` for one still running, and with `confident` returns it only
    /// when the ranker's confidence is high or medium.
    fn take_deep(&self, sid: &str, turn: i64, wait: Duration, confident: bool) -> Option<String> {
        let until = Instant::now() + wait;
        loop {
            {
                let mut jobs = self.deep_jobs.lock().unwrap();
                let j = jobs.get_mut(sid).filter(|j| j.turn == turn && !j.delivered)?;
                if j.done {
                    j.delivered = true;
                    if j.text.is_empty() || (confident && !matches!(j.confidence.as_str(), "high" | "medium")) {
                        return None;
                    }
                    let (text, ids) = (j.text.clone(), j.ids.clone());
                    drop(jobs);
                    let mut st = self.state.lock().unwrap();
                    if let Some(s) = st.sessions.get_mut(sid) {
                        s.injected.extend(ids);
                    }
                    self.dirty.store(true, Ordering::Release);
                    return Some(text);
                }
            }
            if Instant::now() >= until {
                return None;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// memory_ask: query understanding for a question, everything it picks.
    fn op_ask(&self, req: &Json) -> Result<Json> {
        let q = req.get("question").or_else(|| req.get("query")).and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        if q.is_empty() {
            return Err(err("question is empty"));
        }
        // By search alone unless a model is asked to plan and pick.
        let deep = req.get("deep").and_then(|v| v.as_bool()).unwrap_or(false) || req.get("model").and_then(|v| v.as_str()).is_some_and(|m| !m.is_empty());
        if !deep {
            let filter = self.tool_filter(req, Filter::default());
            let built = crate::context::build(&self.store, &q, &filter, &crate::context::ContextParams::default())?;
            let text = if built.text.is_empty() { "ChloeCtx found nothing relevant in memory.".to_string() } else { built.text };
            let ids: Vec<String> = built.records.iter().map(|r| r.id.clone()).collect();
            self.log(&json!({"ts": now(), "event": "ask", "mode": "search", "question": q, "records": ids.len(), "ms": built.ms}));
            return Ok(json!({"text": text, "ids": ids, "ms": built.ms}));
        }
        let mut cfg = self.deep_cfg();
        if let Some(m) = req.get("model").and_then(|v| v.as_str()).filter(|m| !m.is_empty()) {
            cfg.planner_model = m.to_string();
            cfg.ranker_model = m.to_string();
        }
        let filter = self.tool_filter(req, Filter::default());
        let tree = self.tree_of(req);
        let project = Path::new(&tree);
        let distill = self.distill_cfg();
        let mut res = crate::deep::run(&self.store, &cfg, &distill, &q, &[], &filter, Some(project), now());
        // A question asked on purpose gets an answer even when the planner
        // thought memory could not matter.
        if !res.plan.need {
            res.plan.need = true;
            let (cands, _) = crate::deep::gather(&self.store, &cfg, &q, &res.plan, &filter);
            let r = crate::deep::rank(&self.store, &cfg, &distill, &q, &cands, &HashMap::new(), false, now());
            res.adopt(r, cands);
        }
        let mut text = crate::deep::format(&res, &HashSet::new(), now(), Some(self.checker_in(&tree)));
        if text.is_empty() {
            text = format!("ChloeCtx found nothing relevant in memory ({} searches over {} candidates).", res.plan.queries.len() + 2, res.candidates.len());
        }
        let mut log = crate::deep::log_json(&res);
        log["ts"] = json!(now());
        log["event"] = json!("ask");
        self.log(&log);
        Ok(json!({"text": text, "ids": res.ids(), "answer": res.answer, "confidence": res.confidence, "cost_usd": res.cost_usd, "ms": res.ms}))
    }

    /// The stored turns with a prompt, oldest first: (session, turn) ->
    /// what distilling it again needs.
    fn stored_turns(&self, since_seq: i64, since_ts: i64) -> Result<Vec<StoredTurn>> {
        let rs = self.store.query(&format!("SELECT {} FROM memory WHERE kind IN ('prompt', 'answer', 'episode') AND seq > {since_seq} AND ts >= {since_ts} ORDER BY seq ASC LIMIT 1000000", crate::record::COLUMNS))?;
        let recs: Vec<Record> = rs.rows.iter().filter_map(|r| crate::store::row_record(r)).collect();
        // By turn, in the order they started: sessions that ran at once
        // interleave their records.
        let mut turns: Vec<StoredTurn> = Vec::new();
        let mut at: HashMap<(String, i64), usize> = HashMap::new();
        let mut prev_answer: HashMap<String, String> = HashMap::new();
        for r in recs {
            let i = *at.entry((r.session.clone(), r.turn)).or_insert_with(|| {
                let prev = prev_answer.get(&r.session).cloned().unwrap_or_default();
                turns.push(StoredTurn { session: r.session.clone(), turn: r.turn, prev_answer: prev, ..Default::default() });
                turns.len() - 1
            });
            let t = &mut turns[i];
            t.ts = t.ts.max(r.ts);
            match r.kind {
                Kind::Prompt => {
                    if t.prompt.is_empty() {
                        t.by = r.source();
                    }
                    t.prompt.push_str(&r.text);
                    t.prompt.push('\n');
                }
                Kind::Answer if r.tag != "narration" => {
                    t.answer.push_str(&r.text);
                    t.answer.push('\n');
                    let tail: Vec<char> = t.answer.trim().chars().collect();
                    prev_answer.insert(r.session.clone(), tail[tail.len().saturating_sub(800)..].iter().collect());
                }
                Kind::Episode => {
                    if let Some(line) = r.text.lines().find(|l| l.starts_with("Ran: ")) {
                        t.commands.extend(line[5..].split(" ; ").map(String::from));
                    }
                    t.files.extend(r.files.split_whitespace().map(String::from));
                }
                _ => {}
            }
        }
        turns.retain(|t| !t.prompt.trim().is_empty());
        Ok(turns)
    }

    /// The turns a distillation marked, by its outcome ([`TURN_DISTILLED`],
    /// [`TURN_FAILED`], [`TURN_SKIPPED`]), and the oldest mark's sequence
    /// number.
    fn distill_marks(&self) -> Result<Marks> {
        let rs = self.store.query(&format!("SELECT session, turn, tag, seq FROM memory WHERE kind = 'event' AND tag IN ('{TURN_DISTILLED}', '{TURN_FAILED}', '{TURN_SKIPPED}') ORDER BY seq ASC LIMIT 1000000"))?;
        let mut m = Marks::default();
        for r in &rs.rows {
            let (Some(sid), Some(turn), Some(tag)) = (r[0].as_str(), r[1].as_i64(), r[2].as_str()) else { continue };
            m.first = m.first.or(r[3].as_i64());
            match tag {
                TURN_DISTILLED => &mut m.ok,
                TURN_FAILED => &mut m.failed,
                _ => &mut m.skipped,
            }
            .insert((sid.to_string(), turn));
        }
        Ok(m)
    }

    fn distill_stored(&self, t: &StoredTurn) {
        let ex = crate::distill::Exchange { by: t.by.clone(), agent: String::new(), prev_answer: t.prev_answer.clone() };
        self.distill_turn(&t.session, t.turn, t.ts, t.prompt.trim(), t.answer.trim(), &t.commands, &t.files, &ex);
    }

    /// `chloectx distill`: distills the stored turns that were not, for
    /// stores recorded before distillation existed or a turn whose
    /// distillation failed. A turn distilled already is left as it is
    /// (ANA-1: each run distilled every turn again, and counted the rules
    /// again), unless `again`; `dry` only says what it would do.
    fn distill_backfill(&self, again: bool, dry: bool) -> Result<Json> {
        let m = self.distill_marks()?;
        // A turn marked never to be distilled is left out, even `again`.
        let turns: Vec<StoredTurn> = self.stored_turns(0, 0)?.into_iter().filter(|t| !m.skipped.contains(&(t.session.clone(), t.turn))).collect();
        let todo: Vec<&StoredTurn> = turns.iter().filter(|t| again || !m.ok.contains(&(t.session.clone(), t.turn))).collect();
        let retried = todo.iter().filter(|t| m.failed.contains(&(t.session.clone(), t.turn))).count();
        let skipped = turns.len() - todo.len();
        let active = self.distill_cfg().active();
        let mut j = json!({"turns": turns.len(), "to_distill": todo.len(), "already": skipped, "failed_before": retried, "again": again, "active": active});
        if dry || !active {
            return Ok(j);
        }
        let before = self.store.len();
        let t0 = Instant::now();
        for t in &todo {
            self.distill_stored(t);
        }
        // Notes only, not the turns' marks.
        let marks = todo.len() as u64;
        j["items"] = json!(self.store.len().saturating_sub(before + marks));
        j["seconds"] = json!(t0.elapsed().as_secs());
        Ok(j)
    }

    /// Distills the turns a daemon stopped before distilling (it was killed,
    /// or the machine slept: ANA-5), once, when the first hook after the
    /// start says which model and whether distilling is on: the newest few
    /// that ended (have an answer) with no mark, since the store's first
    /// mark (a store from before marks has every turn unmarked) and within
    /// a week.
    fn distill_missed(&self) {
        let mut missed = self.missed_turns();
        let n = missed.len();
        missed.drain(..n.saturating_sub(MISSED_TURNS));
        if missed.is_empty() {
            return;
        }
        self.log(&json!({"ts": now(), "event": "distill_missed", "turns": missed.iter().map(|t| json!({"session": t.session, "turn": t.turn})).collect::<Vec<_>>(), "left": n - missed.len()}));
        for t in &missed {
            self.distill_stored(t);
        }
    }

    /// The turns of the last week, since the store's first distill mark,
    /// that ended (have an answer) with no mark, oldest first.
    fn missed_turns(&self) -> Vec<StoredTurn> {
        let Ok(m) = self.distill_marks() else { return Vec::new() };
        let Some(first) = m.first else { return Vec::new() };
        let Ok(turns) = self.stored_turns(first, now() - 7 * 86_400) else { return Vec::new() };
        let marked = |t: &StoredTurn| [&m.ok, &m.failed, &m.skipped].iter().any(|s| s.contains(&(t.session.clone(), t.turn)));
        turns.into_iter().filter(|t| !t.answer.trim().is_empty() && !marked(t)).collect()
    }

    /// How many turns [`Daemon::missed_turns`] finds, unless one is being
    /// distilled now (its mark is not written yet).
    fn undistilled(&self) -> usize {
        if self.pending.load(Ordering::Acquire) > 0 {
            return 0;
        }
        self.missed_turns().len()
    }

    /// The end of a turn that called `memory_forget` (JONAH-2): its reply,
    /// its card and the narration in its transcript are not stored, it is
    /// not distilled (a note would say again what it forgot), and a search
    /// it left running is not offered at the next prompt. The texts it
    /// forgot are forgotten once more, so that what was written while it
    /// ran (a tool's output that came in late) goes too.
    fn end_forget_turn(&self, sid: &str, turn: i64, transcript: Option<&str>) {
        if let Some(t) = transcript {
            let offset = self.state.lock().unwrap().sessions.get(sid).map_or(0, |s| s.transcript_offset);
            if let Ok((_, next)) = crate::transcript::tail_texts(Path::new(t), offset) {
                self.with_session(sid, |s| s.transcript_offset = next);
            }
        }
        {
            let mut jobs = self.deep_jobs.lock().unwrap();
            if jobs.get(sid).is_some_and(|j| j.turn == turn) {
                jobs.remove(sid);
            }
        }
        let texts = self.store.forgetting.end_turn(sid, turn).unwrap_or_default();
        // Marked, so that `chloectx distill` (even `--again`) and a daemon
        // that starts later leave the turn undistilled too.
        let mark = Draft::new(Kind::Event, format!("Turn {turn} called memory_forget: not distilled")).tag(TURN_SKIPPED);
        if let Err(e) = self.store.append(sid, turn, now(), vec![mark]) {
            self.log(&json!({"ts": now(), "event": "forget_turn", "session": sid, "turn": turn, "error": e.to_string()}));
        }
        let mut again = 0;
        for t in &texts {
            let left = self.scan_forgotten(std::slice::from_ref(t)).map(|(own, ctx)| own.len() + ctx.len()).unwrap_or(0);
            let side = self.scrub_side(std::slice::from_ref(t), true);
            if left == 0 && side.sessions + side.log_lines + side.daemon_lines + side.receipts == 0 {
                continue;
            }
            again += left;
            // `t` is a needle already: lowercase, long enough.
            match self.forget(&json!({"op": "forget", "text": t, "wait": true})) {
                Ok(_) => {}
                Err(e) => self.log(&json!({"ts": now(), "event": "forget_turn", "session": sid, "turn": turn, "error": e.to_string()})),
            }
        }
        self.log(&json!({"ts": now(), "event": "forget_turn_end", "session": sid, "turn": turn, "texts": texts.len(), "found_again": again}));
    }

    /// Stores assistant text written between tool calls since the last scan
    /// of the transcript (hooks only see the final message of a turn).
    fn sync_narration(&self, sid: &str, turn: i64, path: &str, final_answer: &str, ctx: &str) {
        let offset = {
            let st = self.state.lock().unwrap();
            st.sessions.get(sid).map_or(0, |s| s.transcript_offset)
        };
        let Ok((texts, next)) = crate::transcript::tail_texts(Path::new(path), offset) else { return };
        // A final answer the transcript held only after its turn's scan is
        // read with the next turn's: stored already, it is not narration
        // (ANA-4: 4 of 5 multi-turn sessions stored an answer twice).
        let answered = self.with_session(sid, |s| {
            s.transcript_offset = next;
            s.answered.clone()
        });
        let drafts: Vec<Draft> = texts
            .iter()
            .filter(|t| t.trim() != final_answer.trim() && !answered.contains(&crate::sync::fnv(t.trim())))
            .flat_map(|t| capture::narration_drafts(t))
            .map(|d| d.ctx(ctx.to_string()))
            .collect();
        if let Err(e) = self.store.append(sid, turn, now(), drafts) {
            self.log(&json!({"ts": now(), "error": e.to_string(), "event": "narration"}));
        }
    }

    /// Gated recall for a prompt in a session; `force` skips the gate.
    /// Returns the context and the ids it shows. `source` names the recall
    /// in its receipt (`prompt`, `compact`).
    fn recall_for(&self, sid: &str, prompt: &str, force: bool, source: &str) -> Result<Option<(String, Vec<String>)>> {
        let (w, skip, turn) = {
            let st = self.state.lock().unwrap();
            let s = st.sessions.get(sid).cloned().unwrap_or_default();
            (s.window_start, s.injected, s.turn)
        };
        let query: String = prompt.chars().take(2000).collect();
        // A page, an issue or a mail read once is not recalled into later
        // prompts as earlier work (audit CTX-32): memory_search finds it.
        let filter = Filter { exclude_window: Some((sid.to_string(), w)), first_party: true, ..Default::default() };
        let t0 = Instant::now();
        // Knowledge that matches the prompt on its own text is shown even
        // when the gate holds back the rest: a request that does not sound
        // like earlier work can still be bound by what was agreed.
        // The two searches are independent (only packing needs both), so
        // they run side by side when a core is free, on one snapshot
        // ([`crate::receipt::retrieve`], which a receipt's replay runs too).
        let store = self.store.pinned();
        let inputs = crate::receipt::Inputs::recall(&query, sid, &filter, &skip, force, &self.cfg.rank, &self.cfg.pack, &self.cfg.gate, &self.cfg.knowledge);
        let out = crate::receipt::retrieve(&store, &inputs)?;
        let decision = out.decision.unwrap_or(Decision::Skip);
        let mut parts = Vec::new();
        let mut ck = self.checker_in(&self.tree(sid));
        if !out.known.is_empty() {
            parts.push(recall::fmt_knowledge_checked(&out.known, now(), Some(&mut ck)));
        }
        if !out.items.is_empty() {
            let mut t = recall::fmt_recall_checked(&out.items, now(), out.weak, Some(&mut ck));
            t += &recall::fmt_more(&out.hits, &out.shown, if out.weak { crate::receipt::MORE_BRIEF } else { crate::receipt::MORE_FULL });
            parts.push(t);
        }
        let ids = out.ids.clone();
        let text = if parts.is_empty() { None } else { Some((parts.join("\n"), ids.clone())) };
        if !ids.is_empty() {
            let mut st = self.state.lock().unwrap();
            if let Some(s) = st.sessions.get_mut(sid) {
                s.injected.extend(ids.iter().cloned());
            }
            self.dirty.store(true, Ordering::Release);
        }
        let receipt = self.write_receipt(sid, turn, source, &store, inputs, &out, text.as_ref().map_or("", |t| t.0.as_str()));
        let known: Vec<Json> = out.known.iter().map(|(r, c, v)| json!({"id": r.id, "cos": c, "cov": v})).collect();
        self.log(&json!({
            "ts": now(), "event": "recall", "session": sid, "decision": format!("{decision:?}"), "gate": out.gate,
            "evidence": out.ev, "candidates": out.hits.len(), "injected": ids, "knowledge": known, "chars": text.as_ref().map_or(0, |t| t.0.len()),
            "receipt": receipt, "txn": store.db().map(|_| store.snapshot().txn()),
            "ms": t0.elapsed().as_secs_f64() * 1e3,
        }));
        Ok(text)
    }

    /// Makes a recall's receipt; its id, or None when receipts are off.
    /// It is written after the answer is sent ([`Daemon::save_receipts`]),
    /// off the hook's critical path. A recall that showed records asks
    /// for a snapshot to be kept, which [`Daemon::keep_after_recall`]
    /// takes once the hook has answered.
    #[allow(clippy::too_many_arguments)]
    fn write_receipt(&self, sid: &str, turn: i64, source: &str, store: &Store, inputs: crate::receipt::Inputs, out: &crate::receipt::Outcome, text: &str) -> Option<String> {
        if !self.receipts_on() {
            return None;
        }
        let book = self.receipts.get()?;
        let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64) + crate::clock_offset() * 1000;
        let r = crate::receipt::Receipt::new(book.next_id(ms), now(), sid, turn, source, store, inputs, out, text);
        let id = r.id.clone();
        self.unsaved.lock().unwrap().push(r);
        if !out.ids.is_empty() && self.cfg.receipts.snapshots.after_recall {
            self.keep_pending.store(true, Ordering::Release);
        }
        Some(id)
    }

    /// Whether an automatic snapshot would be taken now.
    fn snapshot_due(&self) -> bool {
        self.store.db().is_some_and(|db| self.snapshots.due(db, &self.cfg.receipts.snapshots))
    }

    /// Writes the receipts made while answering, then keeps a snapshot of
    /// the store if a recall asked for one since the last, before anything
    /// else is written: the receipt then replays on the very version it
    /// read. Old snapshots are pruned first, whether or not one is taken.
    /// Runs after the hook has answered.
    fn keep_after_recall(&self) {
        self.save_receipts();
        if !self.keep_pending.swap(false, Ordering::AcqRel) {
            return;
        }
        // A forget is rewriting the store: the next recall keeps one.
        if self.snapshots.held() {
            self.keep_pending.store(true, Ordering::Release);
            return;
        }
        let Some(db) = self.store.db() else { return };
        let r = &self.cfg.receipts.snapshots;
        if let Err(e) = self.snapshots.prune(r, self.store_bytes()) {
            self.log(&json!({"ts": now(), "error": e.to_string(), "event": "snapshot_prune"}));
        }
        if !self.receipts_on() {
            return;
        }
        if let Err(e) = self.snapshots.keep(db, None, r) {
            self.log(&json!({"ts": now(), "error": e.to_string(), "event": "snapshot"}));
        }
    }

    fn file_notes(&self, sid: &str, rel: &str) -> Result<String> {
        let (w, seen) = {
            let st = self.state.lock().unwrap();
            let s = st.sessions.get(sid).cloned().unwrap_or_default();
            (s.window_start, s.annotated.contains(rel))
        };
        if seen {
            return Ok(String::new());
        }
        self.with_session(sid, |s| {
            s.annotated.insert(rel.to_string());
        });
        let terms: Vec<String> = chloedb::text::analyze(rel);
        if terms.is_empty() {
            return Ok(String::new());
        }
        let kinds = vec![Kind::Error, Kind::Decision, Kind::Preference, Kind::Fact, Kind::Note, Kind::Episode];
        let filter = Filter { exclude_window: Some((sid.to_string(), w)), kinds, first_party: true, ..Default::default() };
        let mut where_ = filter.conds();
        where_.push("MATCH(files, :files)".into());
        let params = filter.params().set("files", Value::Str(terms.join(" AND ")));
        let rs = self.store.query_with(&format!("SELECT {} FROM memory WHERE {} ORDER BY seq DESC LIMIT 3", crate::record::COLUMNS, where_.join(" AND ")), &params)?;
        let recs: Vec<Record> = rs.rows.iter().filter_map(|r| crate::store::row_record(r)).collect();
        if recs.is_empty() {
            return Ok(String::new());
        }
        let mut s = format!("ChloeCtx notes on {rel} from earlier work (newest first):\n");
        let mut ck = self.checker_in(&self.tree(sid));
        for r in recs.iter().take(2) {
            let snip: String = r.text.chars().take(500).collect();
            let label = ck.label_record(&r);
            s += &format!("\n{}\n", recall::fmt_record_labeled(r, &snip, now(), &label));
        }
        Ok(s)
    }

    /// The files usually edited after `rel`, each with one note not in
    /// `shown`: the newest decision, rule or fact about it, else its newest
    /// error (episodes, which name every file of a turn, are left out).
    /// Once per file and session.
    fn next_file_notes(&self, sid: &str, rel: &str, shown: &str) -> Result<String> {
        if !self.with_session(sid, |s| s.annotated.insert(format!("next:{rel}"))) {
            return Ok(String::new());
        }
        let next = {
            let mut nf = self.next_files.lock().unwrap();
            nf.update(&self.store)?;
            nf.after(rel, 2, 1.0)
        };
        let tree = self.tree(sid);
        let next: Vec<String> = next.into_iter().map(|(f, _)| f).filter(|f| Path::new(&tree).join(f).exists()).collect();
        if next.is_empty() {
            return Ok(String::new());
        }
        let w = self.with_session(sid, |s| s.window_start);
        let kinds = vec![Kind::Error, Kind::Decision, Kind::Preference, Kind::Fact, Kind::Note];
        let filter = Filter { exclude_window: Some((sid.to_string(), w)), kinds, first_party: true, ..Default::default() };
        let mut s = format!("In earlier work, edits of {rel} were usually followed by edits of: {}.\n", next.join(", "));
        let mut shown = shown.to_string();
        let mut ck = self.checker_in(&tree);
        for f in &next {
            let terms: Vec<String> = chloedb::text::analyze(f);
            if terms.is_empty() {
                continue;
            }
            let mut where_ = filter.conds();
            where_.push("MATCH(files, :files)".into());
            let params = filter.params().set("files", Value::Str(terms.join(" AND ")));
            let rs = self.store.query_with(&format!("SELECT {} FROM memory WHERE {} ORDER BY seq DESC LIMIT 12", crate::record::COLUMNS, where_.join(" AND ")), &params)?;
            let recs: Vec<Record> = rs.rows.iter().filter_map(|r| crate::store::row_record(r)).filter(|r| !shown.contains(&format!("[{}]", r.id))).collect();
            if let Some(r) = recs.iter().find(|r| r.kind.is_knowledge()).or(recs.first()).cloned() {
                let snip: String = r.text.chars().take(300).collect();
                let label = ck.label_record(&r);
                let one = format!("\nChloeCtx note on {f}:\n{}\n", recall::fmt_record_labeled(&r, &snip, now(), &label));
                shown += &one;
                s += &one;
            }
        }
        Ok(s)
    }

    // ------------------------------------------------------------ tools

    fn op_search(&self, req: &Json) -> Result<Json> {
        let q = req.get("query").and_then(|v| v.as_str()).unwrap_or("");
        let limit = req.get("limit").and_then(|v| v.as_u64()).unwrap_or(8).clamp(1, 50) as usize;
        let kinds: Vec<Kind> = req.get("kinds").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|k| k.as_str().and_then(Kind::parse)).collect()).unwrap_or_default();
        let mut f = self.tool_filter(req, Filter { kinds, ..Default::default() });
        if let Some(days) = req.get("since_days").and_then(|v| v.as_f64()) {
            f.since_ts = Some(now() - (days * 86_400.0) as i64);
        }
        if let Some(s) = req.get("session").and_then(|v| v.as_str()) {
            f.session = Some(s.to_string());
        }
        let store = self.store.pinned();
        let inputs = crate::receipt::Inputs::search(q, &f, limit, &self.cfg.rank);
        let out = crate::receipt::retrieve(&store, &inputs)?;
        let terms: HashSet<String> = chloedb::text::analyze_query(q).into_iter().collect();
        let mut text = String::new();
        for h in out.hits.iter().take(limit) {
            text += &recall::fmt_record(&h.record, &recall::snippet(&h.record.text, &terms, 700), now());
            text += &format!("\n(score {:.3})\n\n", h.score);
        }
        if text.is_empty() {
            text = "No matching records.".into();
        }
        // The receipt is the asking session's (ANA-201).
        let active = self.caller_session(req).map(|(s, _)| s).unwrap_or_default();
        let turn = self.state.lock().unwrap().sessions.get(&active).map_or(0, |s| s.turn);
        let receipt = self.write_receipt(&active, turn, "search", &store, inputs, &out, &text);
        if let Some(id) = receipt.as_ref().filter(|_| !out.ids.is_empty()) {
            text += &format!("(receipt {id}: `chloectx receipt {id}` shows why each record ranked where it did, and replays it)");
        }
        Ok(json!({"text": text, "evidence": out.ev, "ids": out.ids, "receipt": receipt}))
    }

    fn book(&self) -> Result<&crate::receipt::Book> {
        // Receipts made by an answer just sent are readable at once.
        self.save_receipts();
        self.receipts.get().ok_or_else(|| err("receipts are off for this store (`chloectx receipts on`, config.json receipts.enabled, or CHLOECTX_RECEIPTS=0)"))
    }

    /// `{"op": "receipts_switch", "on": bool, "purge"?: bool}`: turns
    /// receipts and automatic snapshots on or off for this store, in its
    /// config.json (which wins over `CHLOECTX_RECEIPTS`), from now on.
    /// With `purge`, the receipts kept so far and the automatic snapshots
    /// are removed too.
    fn op_receipts_switch(&self, req: &Json) -> Result<Json> {
        let on = req.get("on").and_then(|v| v.as_bool()).ok_or_else(|| err("receipts_switch needs on: true or false"))?;
        let purge = req.get("purge").and_then(|v| v.as_bool()).unwrap_or(false);
        let mut cfg: Json = std::fs::read_to_string(&self.paths.config).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_else(|| json!({}));
        if !cfg.is_object() {
            return Err(err(format!("{} is not a JSON object", self.paths.config.display())));
        }
        if !cfg.get("receipts").is_some_and(|r| r.is_object()) {
            cfg["receipts"] = json!({});
        }
        cfg["receipts"]["enabled"] = json!(on);
        let tmp = self.paths.config.with_extension("json.tmp");
        {
            let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
            f.write_all(serde_json::to_string_pretty(&cfg)?.as_bytes())?;
        }
        std::fs::rename(&tmp, &self.paths.config)?;
        if on && self.receipts.get().is_none() {
            let b = crate::receipt::Book::open(&self.paths.dir, self.cfg.sync)?;
            let _ = self.receipts.set(b);
        }
        self.receipts_on.store(on, Ordering::Release);
        self.no_hook_receipts.store(true, Ordering::Release);
        let mut text = format!("Receipts and automatic snapshots are {} for this store.", if on { "on" } else { "off" });
        if purge {
            self.unsaved.lock().unwrap().clear();
            let receipts = match self.receipts.get() {
                Some(b) => b.purge()?,
                None => 0,
            };
            let (_, snaps, named) = self.snapshots.drop_automatic_after(|| ())?;
            text += &format!(" Removed {receipts} receipts and {snaps} automatic snapshots.");
            if !named.is_empty() {
                text += &format!(" The named snapshots {} are kept (`chloectx snapshot drop NAME`).", named.join(", "));
            }
        } else if !on {
            text += " The receipts and snapshots kept so far stay until their retention ends (`chloectx receipts off --purge` removes them now); a forget or redact still scrubs them.";
        }
        Ok(json!({"text": text, "on": on}))
    }

    /// `{"op": "receipts", "session"?, "limit"?}`: the newest receipts.
    fn op_receipts(&self, req: &Json) -> Result<Json> {
        let limit = req.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;
        let session = req.get("session").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty());
        let rs = self.book()?.latest(session, limit)?;
        let mut text = String::new();
        for r in &rs {
            text += &format!(
                "{}  {}  session {} turn {:<3} {:<7} {:<6} {} shown, {} tokens · {}\n",
                r.id,
                recall::fmt_time(r.ts),
                r.session.chars().take(8).collect::<String>(),
                r.turn,
                r.source,
                r.decision.as_deref().unwrap_or("-"),
                r.shown.len(),
                r.tokens,
                r.inputs.query.chars().take(60).collect::<String>().replace('\n', " ")
            );
        }
        if text.is_empty() {
            text = match session {
                Some(s) => format!("No receipts for session {s} (no session id starts with it)."),
                None => "No receipts yet.".into(),
            };
        }
        // The rows each one showed, with their text as it is now (none
        // when deleted since), for programs (`chloectx receipts --json`).
        let mut ids: Vec<String> = rs.iter().flat_map(|r| r.rows.iter().filter(|x| x.shown()).map(|x| x.id.clone())).collect();
        ids.sort();
        ids.dedup();
        let texts: std::collections::HashMap<String, String> = self.store.get(&ids)?.into_iter().map(|x| (x.id.clone(), x.text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(300).collect())).collect();
        let list: Vec<Json> = rs
            .iter()
            .map(|r| {
                let rows: Vec<Json> = r.rows.iter().filter(|x| x.shown()).map(|x| json!({"id": x.id, "kind": x.kind, "tag": x.tag, "rank": x.rank, "score": x.score, "decision": x.decision, "text": texts.get(&x.id)})).collect();
                json!({"id": r.id, "ts": r.ts, "session": r.session, "turn": r.turn, "source": r.source, "decision": r.decision, "shown": r.shown, "rows": rows, "tokens": r.tokens, "txn": r.snapshot.txn, "query": r.inputs.query.chars().take(200).collect::<String>()})
            })
            .collect();
        Ok(json!({"text": text, "receipts": list}))
    }

    /// `{"op": "receipt", "id", "replay"?}`: one receipt, and its replay on
    /// the snapshot kept at its commit and on today's store.
    fn op_receipt(&self, req: &Json) -> Result<Json> {
        let id = req.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let r = self.book()?.get(id)?.ok_or_else(|| err(format!("no receipt {id}")))?;
        let mut text = crate::receipt::fmt_receipt(&r);
        let replays = if req.get("replay").and_then(|v| v.as_bool()).unwrap_or(true) {
            let rs = crate::receipt::replay(&r, &self.snapshots, &self.store)?;
            text += "\nreplay:\n";
            text += &crate::blame::fmt_replays(&rs);
            rs
        } else {
            Vec::new()
        };
        if req.get("text").and_then(|v| v.as_bool()).unwrap_or(false) {
            text += &format!("\nwhat the agent was shown:\n{}\n", r.text);
        }
        // Each candidate's text as it is now (gone if deleted since), for a page.
        let ids: Vec<String> = r.rows.iter().map(|x| x.id.clone()).collect();
        let records: serde_json::Map<String, Json> = self
            .store
            .get(&ids)?
            .iter()
            .map(|x| (x.id.clone(), json!({"text": x.text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(160).collect::<String>(), "tag": x.tag})))
            .collect();
        Ok(json!({"text": text, "receipt": r, "replays": replays, "records": records}))
    }

    /// `{"op": "blame", "target": "path[:line]", "replay"?}`.
    fn op_blame(&self, req: &Json) -> Result<Json> {
        let target = req.get("target").and_then(|v| v.as_str()).ok_or_else(|| err("blame needs a target: path or path:line"))?;
        let replay = req.get("replay").and_then(|v| v.as_bool()).unwrap_or(true);
        self.save_receipts();
        crate::blame::blame(&self.store, self.receipts.get(), &self.snapshots, &self.tree_of(req), target, replay)
    }

    /// `{"op": "snapshot", "what": "keep" | "list" | "drop", "name"?}`.
    fn op_snapshot(&self, req: &Json) -> Result<Json> {
        let name = req.get("name").and_then(|v| v.as_str()).filter(|n| !n.is_empty());
        match req.get("what").and_then(|v| v.as_str()).unwrap_or("list") {
            "keep" => {
                let db = self.store.db().ok_or_else(|| err("the store is a table of a ChloeDB server"))?;
                let k = self.snapshots.keep(db, Some(name.ok_or_else(|| err("keep needs a name"))?), &self.cfg.receipts.snapshots)?;
                Ok(json!({"text": format!("kept {} at commit {} ({} records)", k.name, k.at, k.records), "snapshot": k}))
            }
            "drop" => {
                let n = name.ok_or_else(|| err("drop needs a name"))?;
                let found = self.snapshots.drop_named(n)?;
                Ok(json!({"text": if found { format!("dropped {n}") } else { format!("no snapshot {n}") }, "dropped": found}))
            }
            _ => {
                let all = self.snapshots.list();
                let mut text = String::new();
                for k in &all {
                    text += &format!("{:<28} commit {:<8} {}  {} records{}\n", k.name, k.at, recall::fmt_time(k.ts), k.records, if k.auto { "  (automatic)" } else { "" });
                }
                if text.is_empty() {
                    text = "No snapshots kept.".into();
                }
                Ok(json!({"text": text, "snapshots": all}))
            }
        }
    }

    /// After a record is forgotten or records are redacted: the receipts'
    /// copies of their text are scrubbed (whether receipts are on now or
    /// not), and the automatic snapshots, which hold the old text, are
    /// dropped. A forget then rewrites the store's segments that held the
    /// record ([`Daemon::compact_forgotten`]), no snapshot being taken
    /// until it has. What it did, in words.
    fn scrub_history(&self, forgotten: Option<&crate::receipt::Forgotten>) -> String {
        let mut out = Vec::new();
        self.save_receipts();
        if let Some(b) = self.receipts.get() {
            match b.scrub(forgotten) {
                Ok(0) => {}
                Ok(n) => out.push(format!("{n} receipt{} scrubbed", if n == 1 { "" } else { "s" })),
                Err(e) => out.push(format!("scrubbing the receipts failed: {e}")),
            }
        }
        out.extend(self.drop_automatic(|| None));
        out.join("; ")
    }

    /// Drops the automatic snapshots after `first` (run with no snapshot
    /// being taken), in words: what `first` said, what was dropped, and
    /// the named snapshots that still hold the old text.
    fn drop_automatic(&self, first: impl FnOnce() -> Option<String>) -> Vec<String> {
        let mut out = Vec::new();
        match self.snapshots.drop_automatic_after(first) {
            Ok((said, dropped, named)) => {
                out.extend(said);
                if dropped > 0 {
                    out.push(format!("{dropped} automatic snapshot{} dropped", if dropped == 1 { "" } else { "s" }));
                }
                if !named.is_empty() {
                    out.push(format!("the named snapshots {} still hold the old text (`chloectx snapshot drop NAME`)", named.join(", ")));
                }
            }
            Err(e) => out.push(format!("dropping the automatic snapshots failed: {e}")),
        }
        out
    }

    /// The second half of a forget: the segments that held the forgotten
    /// records are rewritten without them, and the file's free pages,
    /// where their old copies were, are written with zeros
    /// ([`compact_forgotten`]). No snapshot is taken from the forget's
    /// start until this is done ([`crate::snapshots::Snapshots::hold`]),
    /// which releases the hold. What went wrong, in words (empty when
    /// nothing did).
    fn compact_forgotten(&self, before: &SegmentsLive) -> String {
        let said = self.drop_automatic(|| {
            let db = self.store.db()?;
            match compact_forgotten(db, before) {
                Ok(c) if c.pending_bytes > 0 => Some(format!(
                    "{} KB of the file's freed pages are still read by an open snapshot and keep their old bytes until the next forget or `chloectx redact`",
                    c.pending_bytes.div_ceil(1024)
                )),
                Ok(_) => None,
                Err(e) => Some(format!("compacting the store failed ({e}): the record's bytes stay in its file until a merge rewrites them")),
            }
        });
        self.snapshots.release();
        // What the answer said already (snapshots dropped) is no news.
        let said: Vec<String> = said.into_iter().filter(|s| !s.contains("snapshot") || s.contains("open snapshot")).collect();
        if !said.is_empty() {
            self.log(&json!({"ts": now(), "event": "forget_compact", "said": said}));
        }
        said.join("; ")
    }

    fn op_timeline(&self, req: &Json) -> Result<Json> {
        let id = req.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let before = req.get("before").and_then(|v| v.as_u64()).unwrap_or(8) as usize;
        let after = req.get("after").and_then(|v| v.as_u64()).unwrap_or(8) as usize;
        Ok(json!({"text": timeline_text(&self.store, id, before, after)?}))
    }

    fn op_sql(&self, req: &Json) -> Result<Json> {
        let sql = req.get("sql").and_then(|v| v.as_str()).unwrap_or("").trim();
        let rs = self.store.query_guarded(sql, &chloedb::query::Params::new(), &crate::sqlguard::Limits::TOOL)?;
        if req.get("json").and_then(|v| v.as_bool()).unwrap_or(false) {
            // The cells as JSON values, for programs (`chloectx sql --json`).
            let rows: Vec<Json> = rs.rows.iter().take(1000).map(|r| Json::Array(r.iter().map(value_json).collect())).collect();
            return Ok(json!({"text": rows_text(&rs), "columns": rs.columns, "rows": rows, "count": rs.rows.len()}));
        }
        Ok(json!({"text": rows_text(&rs)}))
    }

    fn op_remember(&self, req: &Json) -> Result<Json> {
        // A kind that is not one is refused rather than stored as a note.
        let kind = match req.get("kind").and_then(|v| v.as_str()).filter(|k| !k.is_empty()) {
            None => Kind::Note,
            Some(k) => Kind::parse(k).filter(|k| k.is_knowledge()).ok_or_else(|| err(format!("no kind {k:?}: a record to remember is a decision, preference, fact, todo or note")))?,
        };
        let text = crate::redact::redact(req.get("text").and_then(|v| v.as_str()).unwrap_or("").trim());
        if crate::forget::holds(&text, &self.store.forgetting.current(now())) {
            return Err(err(format!(
                "not stored: it holds text that was forgotten in the last {} minutes, and nothing that holds it is stored for that long after a forget",
                crate::forget::LIVE_SECS / 60
            )));
        }
        if text.is_empty() {
            return Err(err("text is empty"));
        }
        let files: Vec<String> = req.get("files").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        // The session that asked (ANA-201), else a person's note: `manual`.
        let sid = self.caller_session(req).map(|(s, _)| s).unwrap_or_else(|| "manual".to_string());
        let turn = self.state.lock().unwrap().sessions.get(&sid).map_or(0, |s| s.turn);
        let old = match req.get("supersedes").and_then(|v| v.as_str()).filter(|o| !o.is_empty()) {
            Some(o) => Some(self.store.get(&[o.to_string()])?.pop().ok_or_else(|| err(format!("no record {o} to supersede: nothing was stored")))?),
            None => None,
        };
        // Who asks to keep it: `by` (the CLI says `user`: a person ran it,
        // or Workbench for one); else the memory tool, which the session's
        // agent calls.
        // The agent says who stated it (`said_by`); when it does not, a
        // note that the turn's prompt says in most of its words is the
        // prompt author's (JONAH-205: the user's decision, kept by the
        // agent, read "the assistant said").
        let by = match req.get("by").and_then(|v| v.as_str()).filter(|b| !b.is_empty()) {
            Some(b) => crate::record::Source::parse(b),
            None => {
                let st = self.state.lock().unwrap();
                let s = st.sessions.get(&sid);
                let agent = s.map(|s| s.agent.clone()).unwrap_or_default();
                let author = s.map(|s| crate::record::Source::parse(&s.source)).unwrap_or(crate::record::Source::User);
                let prompted = s.and_then(|s| s.last_prompts.back()).is_some_and(|p| says_most_of(p, &text));
                match req.get("said_by").and_then(|v| v.as_str()) {
                    Some("user") => author,
                    Some("assistant") => crate::record::Source::Assistant(agent),
                    _ if prompted => author,
                    _ => crate::record::Source::Assistant(agent),
                }
            }
        };
        let a = anchor::make(Path::new(&self.tree_of(req)), &text, &files, &[]);
        let mut d = Draft::new(kind, text).files(files).tag("remembered").by(&by);
        if !a.is_empty() {
            d = d.ctx(a.to_line());
        }
        // What it replaced, as a distilled replacement links it.
        if let Some(o) = &old {
            d = d.link("supersedes", o.id.clone());
        }
        let recs = self.store.append(&sid, turn, now(), vec![d])?;
        let mut replaced = None;
        if let Some(o) = &old {
            if self.store.supersede(&o.id, &recs[0].id)? {
                replaced = Some(o.id.clone());
            }
        }
        let place = self.paths.dir.join("memory.strata");
        Ok(json!({"id": recs[0].id, "supersedes": replaced, "store": place, "by": by.as_str(),
            "text": format!("Stored {} as {}{} in ChloeCtx ({}), where every later session sees it.", kind.as_str(), recs[0].id,
                replaced.as_ref().map(|o| format!(", replacing {o}")).unwrap_or_default(), place.display())}))
    }

    /// `{"op": "forget", "id": ID}`, or `{"op": "forget", "ids": [...]}` for
    /// several at once, or `{"op": "forget", "text": TEXT}` for every
    /// record that holds TEXT (whatever its case): each record is deleted
    /// and scrubbed from the receipts, and the automatic snapshots are
    /// dropped; the answer (`deleted` and `missing`) goes then, and the
    /// segments that held the records are rewritten without them after it,
    /// once for all, with no snapshot taken until that is done. `"wait":
    /// true` answers only once it is (`chloectx forget`); MCP's 20 s
    /// deadline is not spent on it.
    ///
    /// With TEXT ([`crate::forget`]), the records whose own text holds it
    /// are deleted, and those that hold it only in their context (the
    /// turn's request every record of a turn carries: a rule distilled
    /// from a prompt that said it too) keep their text and lose the lines
    /// of their context that held it (`cut`). The session state, events.log,
    /// daemon.log and every receipt lose it too, and for a while nothing
    /// that holds it is written ([`crate::forget::Live`]).
    fn op_forget(self: &Arc<Self>, req: &Json) -> Result<(Json, Option<Deferred>)> {
        let (r, later) = self.forget(req)?;
        Ok((r, later.map(|f| self.after(f))))
    }

    /// [`Daemon::op_forget`]'s work, and what it leaves to run after its
    /// answer (none with `"wait": true`).
    fn forget(&self, req: &Json) -> Result<(Json, Option<Box<dyn FnOnce(&Daemon) + Send + 'static>>)> {
        let text = req.get("text").and_then(|v| v.as_str()).map(str::trim).filter(|t| !t.is_empty());
        let needles: Vec<String> = match text {
            Some(t) => vec![crate::forget::needle(t).ok_or_else(|| err(format!("give at least {} characters to forget", crate::forget::MIN_CHARS)))?],
            None => Vec::new(),
        };
        let mut ids: Vec<String> = match req.get("ids").and_then(|v| v.as_array()) {
            Some(a) => a.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
            None => req.get("id").and_then(|v| v.as_str()).map(|i| vec![i.to_string()]).unwrap_or_default(),
        };
        ids.retain(|i| !i.trim().is_empty());
        if needles.is_empty() && ids.is_empty() {
            return Err(err(format!("forget needs the id of a record, or the text to forget ({} characters or more)", crate::forget::MIN_CHARS)));
        }
        let wait = req.get("wait").and_then(|v| v.as_bool()).unwrap_or(false);
        // From here nothing that holds the text is written.
        self.store.forgetting.add(&needles, now());
        // Which segment holds how many live rows before the deletes: those
        // whose count falls hold the forgotten records.
        let before = self.store.db().map(|db| segments_live(db)).unwrap_or_default();
        // No snapshot from here until the segments are rewritten.
        self.snapshots.hold();
        let held = Held(&self.snapshots);
        let mut cut: Vec<Record> = Vec::new();
        if !needles.is_empty() {
            let (own, ctx_only) = self.scan_forgotten(&needles)?;
            for r in own {
                if !ids.contains(&r.id) {
                    ids.push(r.id);
                }
            }
            for mut r in ctx_only.into_iter().filter(|r| !ids.contains(&r.id)) {
                if let Some(c) = crate::forget::scrub_ctx(&r.ctx, &needles) {
                    r.ctx = c;
                    cut.push(r);
                }
            }
        }
        // Noted as forgotten for the sync before they are deleted (audit
        // CTX-33): a pull that runs meanwhile then either stores none of
        // them again or stores them before the deletes, which remove them.
        let present: Vec<String> = self.store.get(&ids)?.into_iter().map(|r| r.id).collect();
        let mut noted = crate::sync::forget(&self.paths.dir, &present);
        let (mut deleted, mut missing) = (Vec::new(), Vec::new());
        let mut forgotten = crate::receipt::Forgotten { texts: needles.clone(), ..Default::default() };
        for id in &ids {
            let rec = self.store.get(&[id.clone()])?.pop();
            if !self.store.delete(id)? {
                missing.push(id.clone());
                continue;
            }
            deleted.push(id.clone());
            forgotten.ids.push(id.clone());
            if let Some(r) = rec.filter(|r| r.kind == Kind::Prompt) {
                forgotten.prompts.push((id.clone(), r.text));
            }
        }
        // The records that keep their text, without the lines of their
        // context that held it (their old versions leave the file with the
        // segments rewritten below).
        let mut scrubbed = Vec::new();
        if !cut.is_empty() {
            match self.store.insert(&cut) {
                Ok(()) => {}
                Err(e) => {
                    scrubbed.push(format!("cutting it from the context of {} records failed ({e})", cut.len()));
                    cut.clear();
                }
            }
        }
        // The receipts, in one pass; the automatic snapshots are dropped
        // when a record went or changed.
        if !deleted.is_empty() || !cut.is_empty() {
            scrubbed.push(self.scrub_history(Some(&forgotten)));
        } else if !forgotten.is_empty() {
            self.save_receipts();
            if let Some(b) = self.receipts.get() {
                match b.scrub(Some(&forgotten)) {
                    Ok(0) => {}
                    Ok(n) => scrubbed.push(format!("{n} receipt{} scrubbed", if n == 1 { "" } else { "s" })),
                    Err(e) => scrubbed.push(format!("scrubbing the receipts failed: {e}")),
                }
            }
        }
        // The session state and the logs.
        if !needles.is_empty() {
            let side = self.scrub_side(&needles, false);
            scrubbed.extend(side.said());
        }
        // One a pull stored after the look above, and the sync server's
        // rows go too.
        let late: Vec<String> = deleted.iter().filter(|d| !present.contains(d)).cloned().collect();
        if noted.is_ok() {
            noted = crate::sync::forget(&self.paths.dir, &late);
        }
        if let Err(e) = noted {
            scrubbed.push(format!("noting it forgotten for the sync failed ({e}): a pull may bring it back"));
        }
        let synced = (!deleted.is_empty() || !cut.is_empty()) && crate::sync::remote(&crate::paths::home()).is_some();
        let mut later = None;
        if !deleted.is_empty() || !cut.is_empty() {
            // The hold passes to the compaction, which releases it.
            std::mem::forget(held);
            let what = if deleted.len() + cut.len() == 1 { "the segment that held it" } else { "the segments that held them" };
            let cut2 = cut.clone();
            if wait {
                let failed = self.compact_forgotten(&before);
                scrubbed.push(if failed.is_empty() { format!("{what} rewritten without it") } else { failed });
                if synced {
                    scrubbed.push(self.forget_on_server());
                    scrubbed.push(self.cut_on_server(&cut2));
                }
            } else {
                later = Some(Box::new(move |me: &Daemon| {
                    me.compact_forgotten(&before);
                    if synced {
                        me.forget_on_server();
                        me.cut_on_server(&cut2);
                    }
                }) as Box<dyn FnOnce(&Daemon) + Send>);
                scrubbed.push(format!("{what} being rewritten"));
                if synced {
                    scrubbed.push("its row on the sync server being deleted (a pull never brings it back here)".into());
                }
            }
        }
        if !needles.is_empty() {
            let old = chloedb::upgrade::old_path(&self.paths.db);
            if old.exists() {
                scrubbed.push(format!("an older copy of the store, from before its format was upgraded, still holds it: {} (delete it)", old.display()));
            }
            self.log(&json!({"ts": now(), "event": "forget", "deleted": deleted.len(), "cut": cut.len()}));
        }
        let scrubbed: Vec<String> = scrubbed.into_iter().filter(|s| !s.is_empty()).collect();
        let cut_ids: Vec<String> = cut.iter().map(|r| r.id.clone()).collect();
        let text = if needles.is_empty() {
            match (deleted.as_slice(), missing.as_slice()) {
                ([], [m]) => format!("No record {m}."),
                ([], ms) => format!("No records {}.", ms.join(", ")),
                (ds, ms) => {
                    let mut t = if ds.len() == 1 { format!("Deleted {}", ds[0]) } else { format!("Deleted {} records", ds.len()) };
                    if !scrubbed.is_empty() {
                        t += &format!("; {}", scrubbed.join("; "));
                    }
                    if !ms.is_empty() {
                        t += &format!("; no record {}", ms.join(", "));
                    }
                    t + "."
                }
            }
        } else {
            // At most 20 ids named: the rest counted.
            let some = |ids: &[String]| {
                let mut t = ids.iter().take(20).cloned().collect::<Vec<_>>().join(", ");
                if ids.len() > 20 {
                    t += &format!(" and {} more", ids.len() - 20);
                }
                t
            };
            let mut t = match deleted.as_slice() {
                [] => "No record holds it in its own text".to_string(),
                [d] => format!("Deleted {d}, the one record that held it"),
                ds => format!("Deleted {} records that held it ({})", ds.len(), some(ds)),
            };
            match cut_ids.as_slice() {
                [] => {}
                [c] => t += &format!("; cut it from the context of {c}, which stays"),
                cs => t += &format!("; cut it from the context of {} records, which stay ({})", cs.len(), some(cs)),
            }
            if !scrubbed.is_empty() {
                t += &format!("; {}", scrubbed.join("; "));
            }
            t + "."
        };
        let changed = !deleted.is_empty() || !cut_ids.is_empty() || !scrubbed.is_empty();
        Ok((json!({"text": text, "deleted": deleted, "missing": missing, "cut": cut_ids, "changed": changed}), later))
    }

    /// The records whose own text holds one of `needles`, and those that
    /// hold one only in their context.
    fn scan_forgotten(&self, needles: &[String]) -> Result<(Vec<Record>, Vec<Record>)> {
        use crate::forget::holds;
        let rs = self.store.query(&format!("SELECT id FROM memory LIMIT {}", self.store.len() + 1000))?;
        let ids: Vec<String> = rs.rows.iter().filter_map(|r| r.first().and_then(|v| v.as_str()).map(String::from)).collect();
        let (mut own, mut ctx) = (Vec::new(), Vec::new());
        for chunk in ids.chunks(500) {
            for r in self.store.get(chunk)? {
                if holds(&r.text, needles) {
                    own.push(r);
                } else if holds(&r.ctx, needles) {
                    ctx.push(r);
                }
            }
        }
        own.sort_by_key(|r| r.seq);
        ctx.sort_by_key(|r| r.seq);
        Ok((own, ctx))
    }

    /// The session state, events.log, daemon.log (and, with `dry`, the
    /// receipts, counted) without `needles`: what held them, by file.
    /// With `dry`, only counted.
    fn scrub_side(&self, needles: &[String], dry: bool) -> Side {
        let mut side = Side::default();
        (side.sessions, side.failed) = self.scrub_sessions(needles, dry);
        if side.sessions > 0 && !dry {
            self.save_state();
        }
        {
            let _one = self.logging.lock().unwrap_or_else(|e| e.into_inner());
            // events.log: written by this daemon alone, under this lock: replaced.
            let path = self.paths.dir.join("events.log");
            if let Ok(bytes) = std::fs::read(&path) {
                let (out, n) = crate::forget::scrub_lines(&String::from_utf8_lossy(&bytes), needles);
                side.log_lines = n;
                if n > 0 && !dry {
                    let tmp = path.with_extension("log.tmp");
                    let done = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp).and_then(|mut f| f.write_all(out.as_bytes())).and_then(|_| std::fs::rename(&tmp, &path));
                    if let Err(e) = done {
                        side.failed.push(format!("scrubbing events.log failed ({e})"));
                    }
                }
            }
            // daemon.log: the daemon's error output, which it keeps open:
            // rewritten in place.
            if let Ok(bytes) = std::fs::read(&self.paths.log) {
                let (out, n) = crate::forget::scrub_lines(&String::from_utf8_lossy(&bytes), needles);
                side.daemon_lines = n;
                if n > 0 && !dry {
                    let done = std::fs::OpenOptions::new().write(true).truncate(true).open(&self.paths.log).and_then(|mut f| f.write_all(out.as_bytes()));
                    if let Err(e) = done {
                        side.failed.push(format!("scrubbing daemon.log failed ({e})"));
                    }
                }
            }
        }
        if dry {
            self.save_receipts();
            if let Some(b) = self.receipts.get() {
                side.receipts = b.count_holding(needles).unwrap_or(0);
            }
            side.named = self.snapshots.list().into_iter().filter(|k| !k.auto).map(|k| k.name).collect();
        }
        side
    }

    /// The sessions' state (in memory, and so in state.json when it is next
    /// written) without `needles`: how many sessions held one, and what
    /// failed. With `dry`, only counted.
    fn scrub_sessions(&self, needles: &[String], dry: bool) -> (usize, Vec<String>) {
        let (mut n, mut failed) = (0, Vec::new());
        let mut st = self.state.lock().unwrap();
        for s in st.sessions.values_mut() {
            let Ok(mut v) = serde_json::to_value(&*s) else { continue };
            if !crate::forget::scrub_json(&mut v, needles) {
                continue;
            }
            n += 1;
            if dry {
                continue;
            }
            match serde_json::from_value::<Session>(v) {
                Ok(mut new) => {
                    // What the state does not keep in its file stays.
                    new.calls = std::mem::take(&mut s.calls);
                    new.shown_before = std::mem::take(&mut s.shown_before);
                    *s = new;
                }
                Err(e) => failed.push(format!("scrubbing a session's state failed ({e})")),
            }
        }
        (n, failed)
    }

    /// Sends the records whose context a forget cut to the sync server
    /// again, replacing its copies. What it says of them.
    fn cut_on_server(&self, cut: &[Record]) -> String {
        if cut.is_empty() || crate::sync::remote(&crate::paths::home()).is_none() {
            return String::new();
        }
        let key = crate::sync::project_key(&crate::paths::home(), &self.project);
        let Some(remote) = crate::sync::remote_for(&crate::paths::home(), &key) else { return String::new() };
        let _one = SYNC_ONE.lock().unwrap_or_else(|e| e.into_inner());
        match crate::sync::push_records(&remote, &key, cut) {
            Ok(_) => "the sync server's copies of the records it was cut from replaced".into(),
            Err(e) => {
                self.log(&json!({"ts": now(), "event": "sync", "server": remote.url, "error": format!("forget: {e}")}));
                format!("the sync server's copies of the records it was cut from are not replaced yet ({e}): run `chloectx redact` once it answers")
            }
        }
    }

    /// `{"op": "find", "text": TEXT}`: the records whose own text holds
    /// TEXT (whatever its case), which a forget of it deletes, those that
    /// hold it only in their context, which it keeps and cuts it from, and
    /// how many session states, log lines and receipts hold it;
    /// `{"op": "find", "ids": [...]}`: those of the ids that exist. Each
    /// with its first line, for `chloectx forget` to list before it
    /// forgets them.
    fn op_find(&self, req: &Json) -> Result<Json> {
        let limit = req.get("limit").and_then(|v| v.as_u64()).unwrap_or(1000) as usize;
        let row = |r: &Record, needles: &[String]| {
            let at = needles.iter().find_map(|n| crate::forget::find_ci(&r.text, n)).unwrap_or(0);
            json!({"id": r.id, "kind": r.kind.as_str(), "session": r.session, "ts": r.ts, "line": first_line(&r.text, 120), "snippet": snippet_at(&r.text, at, 160)})
        };
        if let Some(ids) = req.get("ids").and_then(|v| v.as_array()) {
            let ids: Vec<String> = ids.iter().filter_map(|v| v.as_str().map(String::from)).collect();
            let found: Vec<Json> = self.store.get(&ids)?.iter().map(|r| row(r, &[])).collect();
            return Ok(json!({"matches": found, "total": found.len()}));
        }
        let Some(needle) = crate::forget::needle(req.get("text").and_then(|v| v.as_str()).unwrap_or("")) else {
            return Err(err(format!("give at least {} characters to look for", crate::forget::MIN_CHARS)));
        };
        let needles = vec![needle];
        let (own, ctx) = self.scan_forgotten(&needles)?;
        let side = self.scrub_side(&needles, true);
        Ok(json!({
            "matches": own.iter().take(limit).map(|r| row(r, &needles)).collect::<Vec<_>>(), "total": own.len(),
            "context": ctx.iter().take(limit).map(|r| row(r, &[])).collect::<Vec<_>>(), "context_total": ctx.len(),
            "side": {"sessions": side.sessions, "log_lines": side.log_lines + side.daemon_lines, "receipts": side.receipts, "named_snapshots": side.named},
        }))
    }

    /// `{"op": "redact", "dry_run": bool}`, for a store written before
    /// redaction knew a secret's shape: rewrites the records whose text or
    /// context [`crate::redact`] changes, then compacts the store, so that
    /// their old versions leave its segments and its write-ahead log;
    /// redacts the session state and events.log the same way; and, when a
    /// sync is set up, sends the rewritten records again, replacing the
    /// server's copies. Answers how many changed and the kinds of secret
    /// found, never what they were.
    fn op_redact(&self, req: &Json) -> Result<Json> {
        use crate::redact::{count_markers, redact_json, redact_record};
        use std::collections::BTreeMap;
        let dry = req.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(false);
        let mut kinds: BTreeMap<String, u64> = BTreeMap::new();
        let mut found = |before: &str, after: &str| {
            let (mut was, mut is) = (BTreeMap::new(), BTreeMap::new());
            count_markers(before, &mut was);
            count_markers(after, &mut is);
            for (k, n) in is {
                let more = n.saturating_sub(was.get(&k).copied().unwrap_or(0));
                if more > 0 {
                    *kinds.entry(k).or_default() += more;
                }
            }
        };

        // The records, in batches by id.
        let rs = self.store.query(&format!("SELECT id FROM memory LIMIT {}", self.store.len() + 1000))?;
        let ids: Vec<String> = rs.rows.iter().filter_map(|r| r.first().and_then(|v| v.as_str()).map(String::from)).collect();
        let mut fixed: Vec<Record> = Vec::new();
        for chunk in ids.chunks(500) {
            let mut batch = Vec::new();
            for mut r in self.store.get(chunk)? {
                let before = format!("{}\n{}", r.text, r.ctx);
                if redact_record(&mut r) {
                    found(&before, &format!("{}\n{}", r.text, r.ctx));
                    batch.push(r);
                }
            }
            if !dry {
                self.store.insert(&batch)?;
            }
            fixed.extend(batch);
        }
        let compacted = if dry || fixed.is_empty() {
            Json::Null
        } else {
            match self.store.db().map(|db| db.optimize()) {
                Some(Ok(_)) => json!(true),
                Some(Err(e)) => json!(format!("the rewritten records are stored, but compacting failed ({e}): their old versions stay in the file until merges reach them")),
                // A cluster's table: the server's merges drop the old versions.
                None => Json::Null,
            }
        };

        // The session state (prompts kept for recall after a compaction).
        let mut sessions = 0;
        {
            let mut st = self.state.lock().unwrap();
            for s in st.sessions.values_mut() {
                let Ok(mut v) = serde_json::to_value(&*s) else { continue };
                let before = v.to_string();
                if redact_json(&mut v) {
                    sessions += 1;
                    found(&before, &v.to_string());
                    if !dry {
                        if let Ok(new) = serde_json::from_value(v) {
                            *s = new;
                        }
                    }
                }
            }
        }
        if sessions > 0 && !dry {
            self.save_state();
        }

        // events.log, line by line, while nothing appends to it.
        let mut log_lines = 0;
        {
            let _one = self.logging.lock().unwrap_or_else(|e| e.into_inner());
            let path = self.paths.dir.join("events.log");
            if let Ok(text) = std::fs::read_to_string(&path) {
                let mut out = String::with_capacity(text.len());
                for line in text.split_inclusive('\n') {
                    let (body, nl) = line.strip_suffix('\n').map_or((line, ""), |b| (b, "\n"));
                    let new = match serde_json::from_str::<Json>(body) {
                        Ok(mut v) => {
                            if redact_json(&mut v) {
                                v.to_string()
                            } else {
                                body.to_string()
                            }
                        }
                        Err(_) => crate::redact::redact(body),
                    };
                    if new != body {
                        log_lines += 1;
                        found(body, &new);
                    }
                    out += &new;
                    out += nl;
                }
                if log_lines > 0 && !dry {
                    let tmp = path.with_extension("log.tmp");
                    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
                    f.write_all(out.as_bytes())?;
                    drop(f);
                    std::fs::rename(&tmp, &path)?;
                }
            }
        }

        // The server's copies.
        let key = crate::sync::project_key(&crate::paths::home(), &self.project);
        let sync = match crate::sync::remote_for(&crate::paths::home(), &key) {
            Some(remote) if !dry && !fixed.is_empty() => {
                match crate::sync::push_records(&remote, &key, &fixed) {
                    Ok(n) => json!({"server": remote.url, "replaced": n}),
                    Err(e) => json!({"server": remote.url, "error": e.to_string()}),
                }
            }
            _ => Json::Null,
        };
        let old = chloedb::upgrade::old_path(&self.paths.db);
        let old_copy = old.exists().then(|| old.display().to_string());

        let verb = if dry { "would be redacted" } else { "redacted" };
        let what: Vec<String> = kinds.iter().map(|(k, n)| format!("{k} x{n}")).collect();
        let mut text = format!(
            "{} of {} records {verb}{}; {sessions} session states and {log_lines} lines of events.log {verb}.",
            fixed.len(),
            ids.len(),
            if what.is_empty() { String::new() } else { format!(" ({})", what.join(", ")) }
        );
        if let Some(e) = compacted.as_str() {
            text += &format!(" Note: {e}.");
        }
        // The receipts' copies of the text, and the snapshots that hold it.
        if !dry && (!fixed.is_empty() || sessions > 0 || log_lines > 0) {
            let scrubbed = self.scrub_history(None);
            if !scrubbed.is_empty() {
                text += &format!(" Receipts and snapshots: {scrubbed}.");
            }
        }
        match (sync.get("replaced"), sync.get("error")) {
            (Some(n), _) => text += &format!(" Replaced {n} records on {}.", sync["server"].as_str().unwrap_or("")),
            (_, Some(e)) => text += &format!(" Could not replace them on {}: {}.", sync["server"].as_str().unwrap_or(""), e.as_str().unwrap_or("")),
            _ => {}
        }
        if let Some(p) = &old_copy {
            text += &format!(" An older copy of the store, from before its format was upgraded, is at {p}: delete it once you no longer need it.");
        }
        self.log(&json!({"ts": now(), "event": "redact", "dry_run": dry, "records": fixed.len(), "sessions": sessions, "log_lines": log_lines, "kinds": kinds}));
        Ok(json!({
            "records": ids.len(), "changed": fixed.len(), "kinds": kinds, "sessions": sessions, "log_lines": log_lines,
            "dry_run": dry, "compacted": compacted, "sync": sync, "old_copy": old_copy, "text": text,
        }))
    }

    /// Stores drafts for a session (transcript back-fill, tests).
    fn op_append(&self, req: &Json) -> Result<Json> {
        let sid = req.get("session").and_then(|v| v.as_str()).unwrap_or("manual");
        let turn = req.get("turn").and_then(|v| v.as_i64()).unwrap_or(0);
        let ts = req.get("ts").and_then(|v| v.as_i64()).unwrap_or_else(now);
        let drafts: Vec<Draft> = serde_json::from_value(req.get("drafts").cloned().unwrap_or(json!([])))?;
        let recs = self.store.append(sid, turn, ts, drafts)?;
        Ok(json!({"stored": recs.len(), "last_seq": self.store.last_seq()}))
    }

    /// Copies the store to the new directory `dest` while it serves: the
    /// database as a checkpoint (a consistent copy, which copying
    /// `memory.strata` while the daemon writes is not), the session state
    /// and config as they are, and the event log up to its last whole line.
    /// Restoring is putting these files back with the daemon stopped.
    fn op_backup(&self, req: &Json) -> Result<Json> {
        let dest = PathBuf::from(req.get("dest").and_then(|v| v.as_str()).ok_or_else(|| err("backup needs a dest"))?);
        if !dest.is_absolute() {
            return Err(err(format!("{} is not an absolute path", dest.display())));
        }
        // The folders it goes in are made, and the backup is as private
        // as the store: its folder 0700 and its files 0600 (ANA-208).
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| err(format!("create {}: {e}", parent.display())))?;
        }
        {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new().mode(0o700).create(&dest).map_err(|e| err(format!("create {}: {e}", dest.display())))?;
        }
        let db = self.store.db().ok_or_else(|| err("the store is a table of a ChloeDB server"))?;
        let stats = db.storage().checkpoint(&dest.join("memory.strata"), &chloedb::storage::CheckpointOptions::default())?;
        self.persist();
        for name in ["state.json", "config.json"] {
            match std::fs::copy(self.paths.dir.join(name), dest.join(name)) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(err(format!("copy {name}: {e}"))),
                _ => {}
            }
        }
        if let Ok(log) = std::fs::read(self.paths.dir.join("events.log")) {
            let whole = log.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
            std::fs::write(dest.join("events.log"), &log[..whole]).map_err(|e| err(format!("write events.log: {e}")))?;
        }
        private_tree(&dest).map_err(|e| err(format!("make {} private: {e}", dest.display())))?;
        Ok(json!({"dest": dest, "records": stats.live_docs, "bytes": stats.bytes_written, "txn": stats.txn, "seconds": stats.seconds}))
    }

    /// `{"op": "capsule", "what": "list" | "cut", "title"?, "kinds"?, "since_days"?}`:
    /// the capsules cut from this store (under its `capsules/`), or a new
    /// one cut there from the store as it is now ([`crate::capsule::pack`]).
    fn op_capsule(&self, req: &Json) -> Result<Json> {
        let dir = self.paths.dir.join("capsules");
        if req.get("what").and_then(|v| v.as_str()) == Some("cut") {
            let kinds: Vec<Kind> = match req.get("kinds").and_then(|v| v.as_array()) {
                Some(a) if !a.is_empty() => a.iter().filter_map(|k| k.as_str().and_then(Kind::parse)).collect(),
                _ => crate::capsule::DEFAULT_KINDS.to_vec(),
            };
            if kinds.is_empty() {
                return Err(err("no kind of record to pack"));
            }
            let title = req.get("title").and_then(|v| v.as_str()).map(str::trim).filter(|t| !t.is_empty()).map(String::from);
            let since_ts = req.get("since_days").and_then(|v| v.as_f64()).filter(|d| *d > 0.0).map(|d| now() - (d * 86_400.0) as i64);
            let slug: String = title.as_deref().unwrap_or("capsule").chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect::<String>().split('-').filter(|w| !w.is_empty()).take(6).collect::<Vec<_>>().join("-");
            let out = dir.join(format!("{}-{}", if slug.is_empty() { "capsule".to_string() } else { slug }, crate::recall::fmt_time(now()).replace([' ', ':'], "-").chars().take(16).collect::<String>()));
            crate::paths::create_private_dir(&dir)?;
            let opts = crate::capsule::PackOptions { kinds, since_ts, title, ..Default::default() };
            let m = crate::capsule::pack(&self.store.pinned(), &self.cfg.embedder, &self.project, &out, &opts)?;
            let mut j = serde_json::to_value(&m)?;
            j["dir"] = json!(out);
            return Ok(j);
        }
        let mut list = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                if let Ok(m) = crate::capsule::manifest(&e.path()) {
                    let mut j = serde_json::to_value(&m)?;
                    j["dir"] = json!(e.path());
                    list.push(j);
                }
            }
        }
        list.sort_by_key(|j| std::cmp::Reverse(j["created_ts"].as_i64().unwrap_or(0)));
        Ok(json!({"dir": dir, "capsules": list}))
    }

    /// `chloectx report`: where the time went ([`crate::report`]), from the
    /// records of the last `days` days; reads, writes nothing.
    fn op_report(&self, req: &Json) -> Result<Json> {
        let days = req.get("days").and_then(|v| v.as_f64()).filter(|d| *d > 0.0).unwrap_or(7.0);
        let session = req.get("session").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(String::from);
        let at = now();
        let since = at - (days * 86_400.0) as i64;
        let kinds = "'prompt', 'answer', 'episode', 'tool_call', 'tool_result', 'error', 'summary', 'event'";
        let rs = self.store.query(&format!("SELECT {} FROM memory WHERE ts >= {since} AND kind IN ({kinds}) ORDER BY seq ASC LIMIT 10000000", crate::record::COLUMNS))?;
        let recs: Vec<Record> = rs.rows.iter().filter_map(|r| crate::store::row_record(r)).collect();
        let worktrees: HashMap<String, String> = self.state.lock().unwrap().sessions.iter().map(|(k, s)| (k.clone(), s.worktree.clone())).collect();
        let opts = crate::report::Options { project: self.project.clone(), days, session, now: at };
        let r = crate::report::build(&recs, &worktrees, &opts);
        Ok(json!({"text": crate::report::render(&r), "report": r}))
    }

    fn op_stats(&self) -> Result<Json> {
        let db = self.store.db().ok_or_else(|| err("the store is a table of a ChloeDB server"))?;
        let fs = db.storage().file_stats();
        let kinds = self.store.query("SELECT kind, COUNT(*) FROM memory GROUP BY kind ORDER BY 2 DESC LIMIT 20")?;
        let by_kind: serde_json::Map<String, Json> = kinds.rows.iter().filter_map(|r| Some((r[0].as_str()?.to_string(), json!(r[1].as_i64()?)))).collect();
        // Entity records belong to no session.
        let sessions = self.store.query("SELECT session, COUNT(*) FROM memory WHERE kind != 'entity' GROUP BY session ORDER BY 2 DESC LIMIT 1000")?.rows.len();
        self.save_receipts();
        let (receipt_file, receipt_wal) = self.receipts.get().map_or((0, 0), |b| b.bytes());
        let snaps = self.snapshots.list();
        Ok(json!({
            "records": self.store.len(), "sessions": sessions, "by_kind": by_kind, "last_seq": self.store.last_seq(),
            "embedder": self.store.embedder.name(), "file_bytes": fs.file_bytes, "wal_bytes": fs.wal_bytes,
            "segments": fs.segments.len(), "memtable_docs": fs.memtable_docs, "store": self.paths.db,
            "receipts": {
                "on": self.receipts_on(), "count": self.receipts.get().map_or(0, |b| b.len()),
                "file_bytes": receipt_file, "wal_bytes": receipt_wal, "retention": self.cfg.receipts.retention,
            },
            "snapshots": {
                "automatic": snaps.iter().filter(|k| k.auto).count(), "named": snaps.iter().filter(|k| !k.auto).count(),
                "bytes": self.snapshots.bytes(), "retention": self.cfg.receipts.snapshots,
                "budget": self.cfg.receipts.snapshots.budget(fs.file_bytes + fs.wal_bytes),
                "last_copied": snaps.iter().rev().find(|k| k.auto).is_some_and(|k| k.copied),
                "newest": snaps.iter().rev().find(|k| k.auto).map(|k| k.ts),
            },
            "total_bytes": fs.file_bytes + fs.wal_bytes + receipt_file + receipt_wal + self.snapshots.bytes(),
            // The model CLI as this daemon resolves it (its PATH is the one
            // of whichever process started it), for doctor (CTX-3).
            "claude": match crate::llm::resolve_cli(&self.distill_cfg().command) {
                Ok(p) => json!({"path": p}),
                Err(e) => json!({"error": e}),
            },
            "distill": self.distill_cfg().active(),
            "undistilled": self.undistilled(),
        }))
    }
}

/// Whether a tool is ChloeCtx's `memory_forget`, by whichever name its MCP
/// server has (`mcp__chloectx__…`, the plugin's, an install from before the
/// rename).
fn is_forget_tool(tool: &str) -> bool {
    tool.ends_with("__memory_forget") && capture::is_own_tool(tool)
}

/// Makes the folders under `dir` (and it) 0700 and its files 0600, as the
/// store's own folder is.
fn private_tree(dir: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        let t = e.file_type()?;
        if t.is_dir() {
            private_tree(&e.path())?;
        } else if t.is_file() {
            std::fs::set_permissions(e.path(), std::fs::Permissions::from_mode(0o600))?;
        }
    }
    Ok(())
}

/// The first line of `text`, at most `width` characters, its spaces
/// folded.
fn first_line(text: &str, width: usize) -> String {
    let line = text.trim().lines().next().unwrap_or("");
    let s: String = line.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() > width {
        s.chars().take(width).collect::<String>() + "…"
    } else if text.trim().lines().nth(1).is_some() {
        s + " …"
    } else {
        s
    }
}

/// About `width` characters of `text` around byte offset `at`, on one line.
fn snippet_at(text: &str, at: usize, width: usize) -> String {
    let at = (0..=at.min(text.len())).rev().find(|&i| text.is_char_boundary(i)).unwrap_or(0);
    let before: Vec<char> = text[..at].chars().rev().take(width / 3).collect();
    let start = at - before.iter().map(|c| c.len_utf8()).sum::<usize>();
    let body: String = text[start..].chars().take(width).collect();
    let mut s = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if start > 0 {
        s = format!("…{s}");
    }
    if start + body.len() < text.len() {
        s.push('…');
    }
    s
}

/// One hybrid search of `store`, as `memory_search` shows it: the first
/// `limit` hits with a snippet each, their ids, and the search's evidence.
/// The stored note that replaces `old`: one of `stored` that links to it
/// with `supersedes` and says the same thing anew, sharing a fair part of
/// its words (a replacement keeps what still holds, [`crate::distill`]).
/// A note about something else that names `old` replaces nothing: half a
/// ruling is not superseded by the other half.
/// Whether `prompt` says most of `note`: at least half of the note's words
/// (stemmed, stop words out), and two or more.
fn says_most_of(prompt: &str, note: &str) -> bool {
    let terms = |t: &str| -> HashSet<String> { chloedb::text::analyze(t).into_iter().collect() };
    let n = terms(note);
    let shared = terms(prompt).intersection(&n).count();
    shared >= 2 && shared * 2 >= n.len()
}

pub fn successor_of(old: &Record, stored: &[Record]) -> Option<String> {
    let terms = |t: &str| -> HashSet<String> { chloedb::text::analyze(t).into_iter().collect() };
    let o = terms(&old.text);
    stored
        .iter()
        .filter(|r| r.linked("supersedes").any(|t| t == old.id))
        .find(|r| {
            let n = terms(&r.text);
            let shared = o.intersection(&n).count() as f64;
            let least = o.len().min(n.len()).max(1) as f64;
            o.is_empty() || shared / least >= 0.3
        })
        .map(|r| r.id.clone())
}

pub fn search_text(store: &Store, q: &str, f: &Filter, rank: &RankParams, limit: usize) -> Result<(String, Vec<String>, Json)> {
    let (hits, ev) = recall::search(store, q, f, rank)?;
    let terms: HashSet<String> = chloedb::text::analyze_query(q).into_iter().collect();
    let mut text = String::new();
    for h in hits.iter().take(limit) {
        text += &recall::fmt_record(&h.record, &recall::snippet(&h.record.text, &terms, 700), now());
        text += &format!("\n(score {:.3})\n\n", h.score);
    }
    if text.is_empty() {
        text = "No matching records.".into();
    }
    let ids = hits.iter().take(limit).map(|h| h.record.id.clone()).collect();
    Ok((text, ids, serde_json::to_value(ev).unwrap_or(Json::Null)))
}

/// The records just before and after record `id` in its session, with
/// their text, as `memory_timeline` shows them.
pub fn timeline_text(store: &Store, id: &str, before: usize, after: usize) -> Result<String> {
    let store = store.pinned();
    let rec = store.get(&[id.to_string()])?.pop().ok_or_else(|| err(format!("no record {id}")))?;
    // Events (notifications, sub-agents' ends) are not part of what was said.
    let mut near = store.range(&rec.session, rec.seq - 200, rec.seq + 200)?;
    near.retain(|r| r.kind != Kind::Event || r.id == rec.id);
    let at = near.iter().position(|r| r.id == rec.id).unwrap_or(0);
    let lo = at.saturating_sub(before);
    let hi = (at + after + 1).min(near.len());
    let mut text = String::new();
    for r in &near[lo..hi] {
        let body: String = r.text.chars().take(1500).collect();
        text += &format!("{}{}\n\n", if r.id == rec.id { ">> " } else { "" }, recall::fmt_record(r, &body, now()));
    }
    Ok(text)
}

/// A cell as JSON: numbers, text and lists as themselves, a timestamp as
/// its text, a vector as its length (`"vector(384)"`).
pub fn value_json(v: &chloedb::Value) -> Json {
    match v {
        chloedb::Value::Null => Json::Null,
        chloedb::Value::Int(x) => json!(x),
        chloedb::Value::Float(x) => json!(x),
        chloedb::Value::Str(s) => json!(s),
        chloedb::Value::Vector(v) => json!(format!("vector({})", v.len())),
        chloedb::Value::List(xs) => Json::Array(xs.iter().map(value_json).collect()),
        chloedb::Value::Bool(b) => json!(b),
        chloedb::Value::Timestamp(t) => json!(chloedb::timestamp::format(*t)),
    }
}

/// A result set as `memory_sql` shows it: a header, at most 100 rows of
/// cells cut to 400 characters, and the row count.
pub fn rows_text(rs: &chloedb::query::exec::ResultSet) -> String {
    let mut text = rs.columns.join(" | ") + "\n";
    for row in rs.rows.iter().take(100) {
        let cells: Vec<String> = row
            .iter()
            .map(|v| match v {
                chloedb::Value::Null => "NULL".into(),
                chloedb::Value::Int(x) => x.to_string(),
                chloedb::Value::Float(x) => format!("{x:.4}"),
                chloedb::Value::Str(s) => {
                    let one = s.split_whitespace().collect::<Vec<_>>().join(" ");
                    if one.chars().count() > 400 {
                        format!("{}…", one.chars().take(400).collect::<String>())
                    } else {
                        one
                    }
                }
                chloedb::Value::Vector(v) => format!("vector({})", v.len()),
                chloedb::Value::List(xs) => format!("list({})", xs.len()),
                chloedb::Value::Bool(b) => b.to_string(),
                chloedb::Value::Timestamp(t) => chloedb::timestamp::format(*t),
            })
            .collect();
        text += &cells.join(" | ");
        text += "\n";
    }
    text += &format!("({} rows)", rs.rows.len());
    text
}

// ---------------------------------------------------------------- server

/// Runs the daemon in this process until it is idle or shut down.
pub fn serve(dir: &Path, project: &str) -> Result<()> {
    let paths = StorePaths::new(dir);
    crate::paths::create_private_dir(dir)?;
    if UnixStream::connect(&paths.socket).is_ok() {
        return Err(err(format!("a daemon already listens on {}", paths.socket.display())));
    }
    let _ = std::fs::remove_file(&paths.socket);
    let daemon = Arc::new(Daemon::open(dir, project)?);
    // Which project this store records, for `chloectx ui`.
    if !project.is_empty() {
        let _ = std::fs::write(dir.join("project.txt"), format!("{project}\n"));
    }
    let listener = UnixListener::bind(&paths.socket)?;
    eprintln!("chloectx: serving {} on {} ({} records, embedder {})", project, paths.socket.display(), daemon.store.len(), daemon.store.embedder.name());
    // Retention of receipts and snapshots, beside the first requests.
    {
        let d = daemon.clone();
        std::thread::spawn(move || d.housekeep());
    }
    // Rules a store took from a harness's envelopes before they were
    // cleaned off prompts: retired once, off the hooks' path.
    {
        let d = daemon.clone();
        std::thread::spawn(move || {
            if let Some(n) = crate::envelope::retire_once(&d.paths.dir, &d.store).filter(|n| *n > 0) {
                d.log(&json!({"ts": now(), "event": "envelope_rules_retired", "count": n}));
            }
        });
    }
    // The sync to a server, when one is set up: what this store lacks on
    // start, then what it records, every PUSH_EVERY.
    if crate::sync::remote(&crate::paths::home()).is_some() {
        let d = daemon.clone();
        std::thread::spawn(move || {
            d.sync_now(true);
            loop {
                std::thread::sleep(crate::sync::PUSH_EVERY);
                d.sync_now(false);
            }
        });
    }
    {
        let d = daemon.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(5));
            if now() - d.last_activity.load(Ordering::Relaxed) > d.cfg.idle_secs as i64 {
                eprintln!("chloectx: idle, exiting");
                d.shutdown();
            }
        });
    }
    for conn in listener.incoming() {
        let Ok(stream) = conn else { continue };
        let d = daemon.clone();
        std::thread::spawn(move || {
            // Counted until the answer is written (and the state saved),
            // not through the work after it, which `deferred` counts: the
            // shutdown a request asks for runs there, and waits for this.
            struct Serving<'a>(&'a std::sync::atomic::AtomicUsize);
            impl Drop for Serving<'_> {
                fn drop(&mut self) {
                    self.0.fetch_sub(1, Ordering::SeqCst);
                }
            }
            d.serving.fetch_add(1, Ordering::SeqCst);
            let serving = Serving(&d.serving);
            let mut reader = BufReader::new(match stream.try_clone() {
                Ok(s) => s,
                Err(_) => return,
            });
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                return;
            }
            let req: Json = serde_json::from_str(&line).unwrap_or(Json::Null);
            let (resp, deferred) = if d.closing.load(Ordering::SeqCst) { (closing_answer(&req), None) } else { d.handle(&req) };
            let mut w = stream;
            let _ = writeln!(w, "{resp}");
            let _ = w.flush();
            drop(w);
            d.persist();
            drop(serving);
            if let Some(f) = deferred {
                f();
            }
        });
    }
    Ok(())
}

/// What a daemon that is shutting down answers a request it did not
/// handle. A shutdown has what it asked for; any other request is sent
/// again by [`request`], to the daemon started once this one is gone.
fn closing_answer(req: &Json) -> Json {
    if req.get("op").and_then(|v| v.as_str()) == Some("shutdown") {
        json!({"ok": true, "closing": true})
    } else {
        json!({"error": "the daemon is shutting down and did not handle the request; send it again", "closing": true})
    }
}

/// An answer [`closing_answer`] gave for a request to send again.
fn is_closing(r: &Json) -> bool {
    r.get("closing").and_then(|v| v.as_bool()) == Some(true) && r.get("error").is_some()
}

// ---------------------------------------------------------------- client

fn try_request(socket: &Path, req: &Json, deadline: Instant) -> std::io::Result<Json> {
    let stream = UnixStream::connect(socket)?;
    let left = deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(1));
    stream.set_read_timeout(Some(left))?;
    stream.set_write_timeout(Some(left))?;
    let mut w = stream.try_clone()?;
    writeln!(w, "{req}")?;
    w.flush()?;
    let mut line = String::new();
    if BufReader::new(stream).read_line(&mut line)? == 0 {
        // Not a JSON error ("EOF while parsing a value at line 1 column
        // 0"): the daemon went away, or failed, while it had the request.
        return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "it closed the connection without answering (it stopped, or failed on the request)"));
    }
    serde_json::from_str(&line).map_err(|e| std::io::Error::other(e.to_string()))
}

/// Who sends a request, for the daemon to file what it stores under the
/// session that made it ([`Daemon::caller_session`]): `via` is `mcp` (the
/// session's MCP server) or `cli`. Claude Code names its session in the
/// environment of the processes it starts (`CLAUDE_CODE_SESSION_ID`, as it
/// was when it started them: a `/clear` makes a new one) and tells its
/// hooks and commands its own pid (`CLAUDE_PID`); an MCP server is its
/// child, so its parent is that process (an MCP server's `CLAUDE_PID`, when
/// it has one, is an outer Claude Code's). A command typed in a terminal
/// names neither, and its note is a person's (`manual`).
pub fn caller(via: &str) -> Json {
    let session = std::env::var("CLAUDE_CODE_SESSION_ID").unwrap_or_default();
    let pid = if via == "mcp" { std::os::unix::process::parent_id() } else { claude_pid().unwrap_or(0) };
    json!({"via": via, "session": session.trim(), "pid": if pid > 1 { pid } else { 0 }})
}

/// The pid of the Claude Code process a hook runs under (`CLAUDE_PID`), if
/// it says.
pub fn claude_pid() -> Option<u32> {
    std::env::var("CLAUDE_PID").ok().and_then(|p| p.trim().parse::<u32>().ok()).filter(|p| *p > 1)
}

/// Sends one request to the project's daemon, starting it if needed.
/// `project` is the folder the request comes from: it travels with the
/// request as `worktree` (the daemon serves every worktree of a
/// repository), and the daemon is started for the folder its store is
/// keyed by ([`crate::paths::store_root`]).
pub fn request(dir: &Path, project: &str, req: &Json, timeout: Duration) -> Result<Json> {
    let paths = StorePaths::new(dir);
    let deadline = Instant::now() + timeout;
    let mut with_tree;
    let req = if !project.is_empty() && req.is_object() && req.get("worktree").is_none() {
        with_tree = req.clone();
        with_tree["worktree"] = json!(project);
        &with_tree
    } else {
        req
    };
    let gone = |e: &std::io::Error| matches!(e.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused);
    // A daemon is started when none listens: at first, and again after one
    // went away under the request.
    let mut start = true;
    let mut first = true;
    loop {
        match try_request(&paths.socket, req, deadline) {
            // A daemon shutting down, which did not handle the request: it
            // goes to the next one, started once this one is gone.
            Ok(r) if is_closing(&r) => {
                if Instant::now() >= deadline {
                    return Err(err(format!("daemon did not answer in time: {}", r["error"].as_str().unwrap_or("it was shutting down"))));
                }
                start = true;
            }
            Ok(r) => return Ok(r),
            // A daemon that died as the request reached it (CTX-16): a new one
            // is started, as for one that is not there.
            Err(e) if matches!(e.kind(), std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe) => start = true,
            Err(e) if first && !gone(&e) => {
                let log = if e.kind() == std::io::ErrorKind::UnexpectedEof { format!("; its log: {}", paths.log.display()) } else { String::new() };
                return Err(err(format!("daemon: {e}{log}")));
            }
            Err(e) if Instant::now() >= deadline => return Err(err(format!("daemon did not answer in time: {e}"))),
            Err(e) if start && gone(&e) => {
                spawn(&paths, project)?;
                (start, first) = (false, false);
                continue;
            }
            Err(_) => {}
        }
        first = false;
        std::thread::sleep(Duration::from_millis(15));
    }
}

/// A request to the store's daemon only when one runs: never starts one
/// (and so never creates a store).
pub fn request_running(dir: &Path, req: &Json, timeout: Duration) -> Result<Json> {
    try_request(&StorePaths::new(dir).socket, req, Instant::now() + timeout).map_err(|e| err(format!("daemon: {e}")))
}

/// The live rows of each on-disk segment of the store, by segment id.
pub type SegmentsLive = std::collections::HashMap<u64, u32>;

/// [`SegmentsLive`] of `db` now.
pub fn segments_live(db: &chloedb::db::Db) -> SegmentsLive {
    let snap = db.storage().snapshot();
    let names: Vec<String> = snap.schemas().map(|s| s.table.clone()).collect();
    let mut out = SegmentsLive::new();
    for n in names {
        if let Some(t) = snap.table(&n) {
            out.extend(t.segments().iter().filter(|v| !v.in_memory).map(|v| (v.id, v.n_live)));
        }
    }
    out
}

/// The segments a forget rewrites: those with deleted rows whose live
/// rows fell since `before`, or that are new since (the flush of what the
/// memtable held, a merge installed meanwhile, which kept the deleted
/// rows' bytes). Other segments' deleted rows are older than the forget
/// and are left to the merges that take them anyway.
fn forgotten_segments(db: &chloedb::db::Db, before: &SegmentsLive) -> std::collections::HashSet<u64> {
    let snap = db.storage().snapshot();
    let names: Vec<String> = snap.schemas().map(|s| s.table.clone()).collect();
    let mut out = std::collections::HashSet::new();
    for n in names {
        let Some(t) = snap.table(&n) else { continue };
        for v in t.segments().iter().filter(|v| !v.in_memory) {
            let deleted = (v.n_live as usize) < v.file.n_docs();
            if deleted && before.get(&v.id).is_none_or(|&was| v.n_live < was) {
                out.insert(v.id);
            }
        }
    }
    out
}

/// What [`compact_forgotten`] did.
#[derive(Debug, Default)]
pub struct Compacted {
    pub merge: chloedb::storage::MergeStats,
    /// Freed bytes written with zeros.
    pub zeroed_bytes: u64,
    /// Freed bytes an open snapshot still reads, left as they were.
    pub pending_bytes: u64,
}

/// After records were deleted (a forget): rewrites the segments that held
/// them, and no others ([`forgotten_segments`]), so that no later
/// checkpoint (a kept snapshot) copies their bytes, then writes zeros over
/// the file's free pages, where the old segments were
/// ([`chloedb::storage::Database::zero_free_pages`]). The cost is the
/// size of those segments and of the free pages, not of the file.
///
/// The store's background merger is woken by the flush and may claim a
/// segment that held a record; a merge here leaves the segments another
/// one holds alone, and until that merge is installed the old segment,
/// record and all, is the one a checkpoint copies. So this returns only
/// once no merge is running and no such segment is left: a merge
/// installed meanwhile wrote a new segment that still holds the record,
/// marked deleted, which is then rewritten too.
pub fn compact_forgotten(db: &chloedb::db::Db, before: &SegmentsLive) -> Result<Compacted> {
    let wait = Duration::from_secs(60);
    let s = db.storage();
    s.flush()?;
    let policy = chloedb::storage::MergePolicy { merge_factor: 1 << 40, max_deleted_ratio: 0.0, ..Default::default() };
    let deadline = Instant::now() + wait;
    let mut out = Compacted::default();
    loop {
        while s.merge_status().running > 0 {
            if Instant::now() >= deadline {
                return Err(err(format!("a background merge still holds the store's segments after {} s", wait.as_secs())));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let targets = forgotten_segments(db, before);
        if targets.is_empty() {
            // And no merge started meanwhile on one of them.
            if s.merge_status().running == 0 {
                break;
            }
            continue;
        }
        let m = s.merge_only(&policy, &|id| targets.contains(&id))?;
        out.merge.merges += m.merges;
        out.merge.segments_in += m.segments_in;
        out.merge.docs_merged += m.docs_merged;
        out.merge.docs_dropped += m.docs_dropped;
        out.merge.bytes_written += m.bytes_written;
        out.merge.seconds += m.seconds;
        if Instant::now() >= deadline {
            return Err(err(format!("the segments that held the record were not all rewritten within {} s", wait.as_secs())));
        }
    }
    let z = s.zero_free_pages()?;
    out.zeroed_bytes = z.zeroed_bytes;
    out.pending_bytes = z.pending_bytes;
    Ok(out)
}

/// Replaces the store's file with a fresh copy of its live rows, after a
/// `redact`: the versions it replaced are gone from every segment, but the
/// pages they were on stay in the file, free, until later writes reuse
/// them, and this copy has no free pages. It needs the file to itself: it
/// holds the lock that clients take to start a daemon (so a hook that
/// comes meanwhile finds no daemon and records nothing, for the few
/// seconds this takes), stops the project's daemon, waits up to `wait`
/// for it to let go of the file, and opens it here. The next request
/// starts a daemon on the copy. Returns the file's size before and after.
pub fn rewrite_file(dir: &Path, wait: Duration) -> Result<Json> {
    use std::os::unix::io::AsRawFd;
    let paths = StorePaths::new(dir);
    if !paths.db.exists() {
        return Err(err(format!("no store at {}", paths.db.display())));
    }
    let deadline = Instant::now() + wait;
    let lock = std::fs::OpenOptions::new().create(true).truncate(false).write(true).mode(0o600).open(&paths.spawn_lock)?;
    // SAFETY: flock on an open descriptor.
    while unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        if Instant::now() >= deadline {
            return Err(err("a daemon is being started; try again"));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if UnixStream::connect(&paths.socket).is_ok() {
        let _ = try_request(&paths.socket, &json!({"op": "shutdown"}), Instant::now() + Duration::from_secs(10));
    }
    let db = loop {
        match chloedb::db::Db::open(&paths.db, chloedb::storage::DbOptions::default()) {
            Ok(db) => break db,
            Err(chloedb::Error::Busy(_)) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(err(format!("open {}: {e}", paths.db.display()))),
        }
    };
    let before = std::fs::metadata(&paths.db).map_or(0, |m| m.len());
    // Without what merges dropped, then copied: the copy holds only live data.
    db.optimize()?;
    let copy = dir.join("memory.strata.redacted");
    let _ = std::fs::remove_file(&copy);
    let stats = db.storage().checkpoint(&copy, &chloedb::storage::CheckpointOptions::default())?;
    let live = db.snapshot().n_live_docs();
    drop(db);
    if stats.live_docs != live {
        let _ = std::fs::remove_file(&copy);
        return Err(err(format!("the copy holds {} records, the store {live}: the store was left as it was", stats.live_docs)));
    }
    std::fs::rename(&copy, &paths.db)?;
    // Empty since the checkpoint flushed; the copy starts its own.
    match std::fs::remove_file(dir.join("memory.strata-wal")) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
        _ => {}
    }
    drop(lock);
    let after = std::fs::metadata(&paths.db).map_or(0, |m| m.len());
    Ok(json!({"records": live, "bytes_before": before, "bytes_after": after}))
}

/// Starts `chloectx serve` in the background unless one is starting already.
fn spawn(paths: &StorePaths, project: &str) -> Result<()> {
    crate::paths::create_private_dir(&paths.dir)?;
    let lock = std::fs::OpenOptions::new().create(true).truncate(false).write(true).mode(0o600).open(&paths.spawn_lock)?;
    use std::os::unix::io::AsRawFd;
    // SAFETY: flock on an open descriptor.
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Ok(()); // another client is spawning it
    }
    if UnixStream::connect(&paths.socket).is_ok() {
        return Ok(());
    }
    let exe: PathBuf = std::env::current_exe()?;
    let log = std::fs::OpenOptions::new().create(true).append(true).open(&paths.log)?;
    let mut cmd = std::process::Command::new(exe);
    let root = if project.is_empty() { PathBuf::new() } else { crate::paths::store_root(Path::new(project)) };
    cmd.args(["serve", "--dir"]).arg(&paths.dir).arg("--project").arg(&root);
    cmd.stdin(std::process::Stdio::null()).stdout(log.try_clone()?).stderr(log);
    // SAFETY: setsid is async-signal-safe; it detaches the daemon from the hook's session.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    cmd.spawn()?;
    // Hold the lock until the socket appears (or a short while), so other
    // clients wait instead of spawning a second daemon.
    let until = Instant::now() + Duration::from_secs(5);
    while Instant::now() < until && UnixStream::connect(&paths.socket).is_err() {
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}
