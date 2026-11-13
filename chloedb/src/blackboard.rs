//! The blackboard: a [tenant](crate::tenant) that many agents share to
//! work one job (experimental, as tenants are).
//!
//! It is a database of four things: `tasks` that agents claim under a
//! lease, `findings` they post, the `events` of both in order, and the
//! idempotency keys of the calls already `applied`. An agent calls it
//! through a [`Board`]:
//!
//! | call | does |
//! |---|---|
//! | [`Board::add_tasks`] | takes tasks in, each id once |
//! | [`Board::claim`] | gives the caller open tasks, and claimed ones whose lease has passed |
//! | [`Board::heartbeat`] | extends the caller's lease |
//! | [`Board::complete`], [`Board::fail`] | end the caller's claim |
//! | [`Board::post`] | records a finding, as a duplicate of one that says the same |
//! | [`Board::search`], [`Board::timeline`], [`Board::status`] | read |
//!
//! Why it holds. A call that writes is one transaction, run on the
//! tenant's local copy while no other write of the handle runs, and
//! committed by the tenant's replacement of `HEAD`: the store orders
//! every commit, so of two agents that claim the same task, through one
//! node or two, one is given it and the other is not. A call that was
//! acknowledged is in the store. A call that was not may be sent again
//! with the same `key`: when the first did commit, the second changes
//! nothing and answers what the first answered. A key is one agent's
//! name for one call: another agent's call under the same key is another
//! call.
//!
//! A lease is a time on the clock of the node that serves the call. A
//! task whose lease has passed may be given to another agent while the
//! first still works on it; the first's `complete` is then refused, which
//! is what makes a task done once.
//!
//! Not built: findings are compared by their words (the share of words
//! two of them have in common), not by meaning, which waits for an
//! embedding model the tenant can name.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::db::{Db, SqlResult};
use crate::query::Params;
use crate::storage::{ObjectStore, WriteTxn};
use crate::tenant::{Tenant, TenantError, TenantOptions};
use crate::{timestamp, Error, Result, Value};

/// The template's name, where a tenant is made from one.
pub const TEMPLATE: &str = "blackboard";

/// The tables of a blackboard.
pub const SCHEMA: [&str; 5] = [
    "CREATE TABLE tasks (id KEYWORD PRIMARY KEY, seq INT, title TEXT, spec TEXT, state KEYWORD, owner KEYWORD, \
     lease_until TIMESTAMP, attempts INT, result TEXT OPTIONS(indexed = false), ts TIMESTAMP)",
    "CREATE TABLE findings (id KEYWORD PRIMARY KEY, seq INT, task KEYWORD, agent KEYWORD, body TEXT, dup_of KEYWORD, ts TIMESTAMP)",
    "CREATE TABLE events (id INT PRIMARY KEY, ts TIMESTAMP, agent KEYWORD, kind KEYWORD, about KEYWORD, body TEXT OPTIONS(indexed = false))",
    "CREATE TABLE applied (id KEYWORD PRIMARY KEY, ts TIMESTAMP, answer TEXT OPTIONS(indexed = false))",
    "CREATE TABLE counters (id KEYWORD PRIMARY KEY, n INT)",
];

/// The states of a task.
pub const OPEN: &str = "open";
pub const CLAIMED: &str = "claimed";
pub const DONE: &str = "done";
pub const FAILED: &str = "failed";

/// Tasks one `claim` gives, and one `add_tasks` takes, at most.
pub const MAX_TASKS: usize = 100;
/// A lease's length, at most: a day.
pub const MAX_LEASE_S: u64 = 86_400;
/// Characters of a title, and of a spec, a result or a finding, at most.
pub const MAX_TITLE: usize = 500;
pub const MAX_BODY: usize = 100_000;
/// Rows a read answers, at most.
pub const MAX_ROWS: usize = 500;
/// The words of a finding that look for one like it, at most.
const MATCH_WORDS: usize = 48;

/// Why a call failed.
#[derive(Debug)]
pub enum BoardError {
    /// The board refused it, and nothing was written: the task is another
    /// agent's, or done, or not there; an argument is out of range.
    Refused(String),
    /// The tenant's error ([`TenantError`]): with `NotWriter` nothing was
    /// written, and the call can be sent again.
    Tenant(TenantError),
}

pub type BResult<T> = std::result::Result<T, BoardError>;

impl fmt::Display for BoardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BoardError::Refused(m) => write!(f, "refused: {m}"),
            BoardError::Tenant(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for BoardError {}

impl From<TenantError> for BoardError {
    fn from(e: TenantError) -> Self {
        BoardError::Tenant(e)
    }
}

/// Times as ISO 8601 text in JSON, microseconds since 1970 in Rust.
mod iso {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(t: &i64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&crate::timestamp::format(*t))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
        let text = String::deserialize(d)?;
        crate::timestamp::parse(&text).ok_or_else(|| serde::de::Error::custom(format!("not a time: {text:?}")))
    }

    pub mod opt {
        use serde::{Deserialize, Deserializer, Serializer};

        pub fn serialize<S: Serializer>(t: &Option<i64>, s: S) -> Result<S::Ok, S::Error> {
            match t {
                Some(t) => s.serialize_str(&crate::timestamp::format(*t)),
                None => s.serialize_none(),
            }
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
            match Option::<String>::deserialize(d)? {
                Some(text) => crate::timestamp::parse(&text).map(Some).ok_or_else(|| serde::de::Error::custom(format!("not a time: {text:?}"))),
                None => Ok(None),
            }
        }
    }
}

/// A task. Times are microseconds since 1970, and ISO 8601 text in JSON.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    /// The order the board took the tasks in.
    pub seq: i64,
    pub title: String,
    pub spec: String,
    /// [`OPEN`], [`CLAIMED`], [`DONE`] or [`FAILED`].
    pub state: String,
    /// The agent that claimed it last.
    pub owner: Option<String>,
    /// Until when it is the owner's; past that, another agent may claim it.
    #[serde(with = "iso::opt")]
    pub lease_until: Option<i64>,
    /// How many times it was claimed.
    pub attempts: i64,
    /// What `complete` or `fail` said.
    pub result: Option<String>,
    #[serde(with = "iso")]
    pub ts: i64,
}

/// A task to take in.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NewTask {
    /// Its id; the board gives one (`t1`, `t2`, ...) to a task without.
    #[serde(default)]
    pub id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub spec: String,
}

/// What `add_tasks` did with each id, in the order given.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Added {
    pub added: Vec<String>,
    /// The ids the board had already: left as they were.
    pub existing: Vec<String>,
}

/// A finding.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    pub seq: i64,
    pub task: Option<String>,
    pub agent: String,
    pub body: String,
    /// The earlier finding this one repeats, if it does.
    pub dup_of: Option<String>,
    #[serde(with = "iso")]
    pub ts: i64,
}

/// One thing that happened on the board. `kind` is `task` (taken in),
/// `claim`, `done`, `failed` or `finding`, and `about` the task's or the
/// finding's id.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub id: i64,
    #[serde(with = "iso")]
    pub ts: i64,
    pub agent: String,
    pub kind: String,
    pub about: String,
    pub body: String,
}

/// A finding a search found, and how many findings repeat it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FoundFinding {
    #[serde(flatten)]
    pub finding: Finding,
    pub duplicates: i64,
    pub score: f64,
}

/// A task a search found.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FoundTask {
    #[serde(flatten)]
    pub task: Task,
    pub score: f64,
}

/// What a search found, best first.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Found {
    pub findings: Vec<FoundFinding>,
    pub tasks: Vec<FoundTask>,
}

/// The board in numbers.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Status {
    /// Tasks by state. A claimed task whose lease has passed counts as
    /// claimed until another agent claims it.
    pub tasks: BTreeMap<String, i64>,
    /// Claimed tasks whose lease has passed.
    pub lapsed: i64,
    /// Findings, the duplicates among them, and the last event's id.
    pub findings: i64,
    pub duplicates: i64,
    pub events: i64,
}

/// Creates tenant `id` in `store` as a blackboard.
pub fn create(store: Arc<dyn ObjectStore>, id: &str, opts: TenantOptions) -> std::result::Result<Tenant, TenantError> {
    Tenant::create(store, id, opts, init)
}

/// Makes the tables of a blackboard in an empty database.
pub fn init(db: &Db) -> Result<()> {
    for s in SCHEMA {
        db.sql(s, &Params::new())?;
    }
    Ok(())
}

/// Whether `db` has a blackboard's tables.
pub fn is_board(db: &Db) -> bool {
    let tables: BTreeSet<String> = db.storage().tables().iter().map(|t| t.table.to_ascii_lowercase()).collect();
    ["tasks", "findings", "events", "applied", "counters"].iter().all(|t| tables.contains(*t))
}

/// A blackboard, for one agent's calls.
pub struct Board<'a> {
    tenant: &'a Tenant,
    agent: String,
    now: Option<i64>,
    same: f64,
    latest: bool,
}

impl<'a> Board<'a> {
    /// The board of `tenant` (made by [`create`]) for the agent named
    /// `agent`: the name its claims and findings carry.
    pub fn new(tenant: &'a Tenant, agent: &str) -> BResult<Board<'a>> {
        check_name("agent", agent)?;
        Ok(Board { tenant, agent: agent.to_string(), now: None, same: 0.8, latest: false })
    }

    /// With the time fixed at `micros` since 1970, not read from the
    /// clock (for tests of leases).
    pub fn at(mut self, micros: i64) -> Self {
        self.now = Some(micros);
        self
    }

    /// With findings counted as the same when they share at least this
    /// part of their words (0.8 unless set; above 1, never).
    pub fn same_at(mut self, share: f64) -> Self {
        self.same = share;
        self
    }

    /// With reads that look at the store first ([`Tenant::read_latest`]),
    /// so that they see what other nodes committed. Writes always do.
    pub fn latest(mut self, latest: bool) -> Self {
        self.latest = latest;
        self
    }

    fn now(&self) -> i64 {
        self.now.unwrap_or_else(timestamp::now)
    }

    /// A call that writes: one transaction, and one commit of the tenant.
    /// With a `key`, a call the board has applied for this agent under
    /// that key answers what it answered then, and writes nothing.
    fn apply<T: Serialize + DeserializeOwned>(&self, tool: &str, key: Option<&str>, op: impl for<'d> FnOnce(&'d Db, i64) -> Result<std::result::Result<(T, Tx<'d>), String>>) -> BResult<T> {
        let key = match key {
            Some(k) => {
                check_name("key", k)?;
                // A key is its agent's: two agents that choose the same
                // word are not answered each other's calls. (The name's
                // length keeps `a:b` with key `c` from `a` with `b:c`.)
                Some(format!("{tool}:{}:{}:{k}", self.agent.len(), self.agent))
            }
            None => None,
        };
        let now = self.now();
        let out = self.tenant.write(|db| {
            if let Some(k) = &key {
                if let Some(answer) = one(db, "SELECT answer FROM applied WHERE id = :k", Params::new().set("k", Value::Str(k.clone())))? {
                    let answer = text(&answer[0]).unwrap_or_default();
                    return serde_json::from_str(&answer).map(Ok).map_err(|e| Error::Corrupt(format!("the answer kept for key {k:?}: {e}")));
                }
            }
            let (out, mut tx) = match op(db, now)? {
                Ok(x) => x,
                Err(why) => return Ok(Err(why)),
            };
            if let Some(k) = key {
                let answer = serde_json::to_string(&out).map_err(|e| Error::Schema(e.to_string()))?;
                tx.txn.table("applied")?.upsert(vec![Value::Str(k), Value::Timestamp(now), Value::Str(answer)])?;
            }
            tx.txn.commit()?;
            Ok(Ok(out))
        })?;
        out.map_err(BoardError::Refused)
    }

    fn read<T>(&self, f: impl FnOnce(&Db) -> Result<T>) -> BResult<T> {
        Ok(match self.latest {
            true => self.tenant.read_latest(f),
            false => self.tenant.read(f),
        }?)
    }

    /// Takes tasks in. A task whose id the board has already is left as
    /// it is, so the same list sent twice makes each task once.
    pub fn add_tasks(&self, tasks: &[NewTask], key: Option<&str>) -> BResult<Added> {
        if tasks.is_empty() || tasks.len() > MAX_TASKS {
            return Err(BoardError::Refused(format!("1 to {MAX_TASKS} tasks at a time, not {}", tasks.len())));
        }
        for t in tasks {
            if let Some(id) = &t.id {
                check_name("task id", id)?;
            }
            check_text("title", &t.title, 1, MAX_TITLE)?;
            check_text("spec", &t.spec, 0, MAX_BODY)?;
        }
        self.apply("add_tasks", key, |db, now| {
            let mut tx = Tx::begin(db, now, &self.agent)?;
            let mut out = Added::default();
            for t in tasks {
                let id = match &t.id {
                    Some(id) if tx.has_task(id)? => {
                        out.existing.push(id.clone());
                        continue;
                    }
                    Some(id) => id.clone(),
                    // A number no task took, by an id given to it either.
                    None => loop {
                        let id = format!("t{}", tx.next("task ids")?);
                        if !tx.has_task(&id)? {
                            break id;
                        }
                    },
                };
                let seq = tx.next("tasks")?;
                let task = Task { id: id.clone(), seq, title: t.title.clone(), spec: t.spec.clone(), state: OPEN.into(), owner: None, lease_until: None, attempts: 0, result: None, ts: now };
                tx.put_task(&task)?;
                tx.event("task", &id, &t.title)?;
                out.added.push(id);
            }
            Ok(Ok((out, tx)))
        })
    }

    /// Gives the caller up to `n` tasks, oldest first: open ones, and
    /// claimed ones whose lease has passed. Each is the caller's for
    /// `lease_s` seconds, which [`Board::heartbeat`] extends. None when
    /// there is nothing to do now.
    pub fn claim(&self, n: usize, lease_s: u64, key: Option<&str>) -> BResult<Vec<Task>> {
        let (n, lease) = (n.clamp(1, MAX_TASKS), lease_micros(lease_s)?);
        self.apply("claim", key, |db, now| {
            let free = tasks(
                db,
                &format!("WHERE state = 'open' OR (state = 'claimed' AND lease_until < :now) ORDER BY seq LIMIT {n}"),
                Params::new().set("now", Value::Timestamp(now)),
            )?;
            let mut tx = Tx::begin(db, now, &self.agent)?;
            let mut out = Vec::new();
            for mut t in free {
                let from = t.owner.replace(self.agent.clone()).filter(|_| t.state == CLAIMED);
                t.state = CLAIMED.into();
                t.lease_until = Some(now + lease);
                t.attempts += 1;
                tx.put_task(&t)?;
                tx.event("claim", &t.id, &match from {
                    Some(from) => format!("attempt {}, after the lease of {from} passed", t.attempts),
                    None => format!("attempt {}", t.attempts),
                })?;
                out.push(t);
            }
            Ok(Ok((out, tx)))
        })
    }

    /// The caller's task, to end or extend: an error text when it is not
    /// the caller's.
    fn mine(&self, db: &Db, id: &str) -> Result<std::result::Result<Task, String>> {
        Ok(match task(db, id)? {
            None => Err(format!("no task {id:?}")),
            Some(t) if t.state == CLAIMED && t.owner.as_deref() == Some(&self.agent) => Ok(t),
            Some(t) if t.state == CLAIMED => Err(format!("task {id:?} is claimed by {}: the lease of {} passed", t.owner.as_deref().unwrap_or("another agent"), self.agent)),
            Some(t) => Err(format!("task {id:?} is {}, not claimed by {}", t.state, self.agent)),
        })
    }

    /// Extends the caller's lease on `task` to `lease_s` seconds from
    /// now. Refused when the task is not the caller's any more.
    pub fn heartbeat(&self, task: &str, lease_s: u64) -> BResult<Task> {
        let lease = lease_micros(lease_s)?;
        self.apply("heartbeat", None, |db, now| {
            let mut t = match self.mine(db, task)? {
                Ok(t) => t,
                Err(why) => return Ok(Err(why)),
            };
            t.lease_until = Some(now + lease);
            let mut tx = Tx::begin(db, now, &self.agent)?;
            tx.put_task(&t)?;
            Ok(Ok((t, tx)))
        })
    }

    /// Ends the caller's claim on `task`: done, with `result`. Refused
    /// when the task is not the caller's any more, so a task is done once.
    pub fn complete(&self, task: &str, result: &str, key: Option<&str>) -> BResult<Task> {
        self.end("complete", DONE, task, result, key)
    }

    /// Ends the caller's claim on `task`: failed, with the reason.
    pub fn fail(&self, task: &str, why: &str, key: Option<&str>) -> BResult<Task> {
        self.end("fail", FAILED, task, why, key)
    }

    fn end(&self, tool: &str, state: &str, task: &str, result: &str, key: Option<&str>) -> BResult<Task> {
        check_text("result", result, 0, MAX_BODY)?;
        self.apply(tool, key, |db, now| {
            let mut t = match self.mine(db, task)? {
                Ok(t) => t,
                Err(why) => return Ok(Err(why)),
            };
            t.state = state.into();
            t.lease_until = None;
            t.result = Some(result.to_string());
            let mut tx = Tx::begin(db, now, &self.agent)?;
            tx.put_task(&t)?;
            tx.event(state, &t.id, result)?;
            Ok(Ok((t, tx)))
        })
    }

    /// Records a finding, about `task` if one is named. When an earlier
    /// finding says the same (they share most of their words:
    /// [`Board::same_at`]), the new one is kept as its duplicate
    /// (`dup_of`), and searches answer the earlier one.
    pub fn post(&self, body: &str, task: Option<&str>, key: Option<&str>) -> BResult<Finding> {
        check_text("body", body, 1, MAX_BODY)?;
        if let Some(t) = task {
            check_name("task id", t)?;
        }
        let said = words(body);
        self.apply("post", key, |db, now| {
            let dup_of = self.said_before(db, &said)?;
            let mut tx = Tx::begin(db, now, &self.agent)?;
            let seq = tx.next("findings")?;
            let f = Finding { id: format!("f{seq}"), seq, task: task.map(String::from), agent: self.agent.clone(), body: body.to_string(), dup_of, ts: now };
            tx.put_finding(&f)?;
            tx.event("finding", &f.id, &f.dup_of.as_ref().map(|d| format!("a duplicate of {d}")).unwrap_or_default())?;
            Ok(Ok((f, tx)))
        })
    }

    /// The earliest finding, itself no duplicate, that shares enough of
    /// its words with `said`.
    fn said_before(&self, db: &Db, said: &BTreeSet<String>) -> Result<Option<String>> {
        let Some(query) = any_of(said, "80%") else { return Ok(None) };
        let near = rows(
            db,
            "SELECT id, body, seq FROM findings WHERE dup_of IS NULL AND MATCH(body, :q) ORDER BY bm25(body, :q) DESC LIMIT 16",
            Params::new().set("q", Value::Str(query)),
        )?;
        let mut same: Vec<(i64, String)> =
            near.iter().filter(|r| jaccard(said, &words(&text(&r[1]).unwrap_or_default())) >= self.same).map(|r| (int(&r[2]), text(&r[0]).unwrap_or_default())).collect();
        same.sort();
        Ok(same.into_iter().next().map(|(_, id)| id))
    }

    /// The task with this id.
    pub fn task(&self, id: &str) -> BResult<Option<Task>> {
        self.read(|db| task(db, id))
    }

    /// The findings and the tasks that match the words of `q`, best
    /// first, up to `limit` of each. A finding's duplicates are counted,
    /// not listed.
    pub fn search(&self, q: &str, limit: usize) -> BResult<Found> {
        let limit = limit.clamp(1, MAX_ROWS);
        let Some(query) = any_of(&words(q), "1") else { return Err(BoardError::Refused("a search needs a word".into())) };
        self.read(|db| {
            let p = || Params::new().set("q", Value::Str(query.clone()));
            let found = db.sql(&format!("SELECT {FINDING} FROM findings WHERE dup_of IS NULL AND MATCH(body, :q) ORDER BY bm25(body, :q) DESC LIMIT {limit}"), &p())?;
            let mut out = Found::default();
            if let SqlResult::Rows(r) = found {
                for (row, score) in r.rows.iter().zip(r.scores.iter().chain(std::iter::repeat(&0.0))) {
                    let finding = finding_of(row)?;
                    let duplicates = count(db, "SELECT COUNT(*) FROM findings WHERE dup_of = :id", Params::new().set("id", Value::Str(finding.id.clone())))?;
                    out.findings.push(FoundFinding { finding, duplicates, score: *score });
                }
            }
            let found = db.sql(&format!("SELECT {TASK} FROM tasks WHERE MATCH(title, :q) OR MATCH(spec, :q) ORDER BY bm25(title, :q) + bm25(spec, :q) DESC LIMIT {limit}"), &p())?;
            if let SqlResult::Rows(r) = found {
                for (row, score) in r.rows.iter().zip(r.scores.iter().chain(std::iter::repeat(&0.0))) {
                    out.tasks.push(FoundTask { task: task_of(row)?, score: *score });
                }
            }
            Ok(out)
        })
    }

    /// The events after the one with id `since` (0: from the first), in
    /// order, up to `limit`. The last one's id is the next call's `since`.
    pub fn timeline(&self, since: i64, limit: usize) -> BResult<Vec<Event>> {
        let limit = limit.clamp(1, MAX_ROWS);
        self.read(|db| {
            rows(db, &format!("SELECT id, ts, agent, kind, about, body FROM events WHERE id > :since ORDER BY id LIMIT {limit}"), Params::new().set("since", Value::Int(since)))?
                .iter()
                .map(|r| Ok(Event { id: int(&r[0]), ts: int(&r[1]), agent: need(&r[2])?, kind: need(&r[3])?, about: need(&r[4])?, body: text(&r[5]).unwrap_or_default() }))
                .collect()
        })
    }

    /// The board in numbers.
    pub fn status(&self) -> BResult<Status> {
        let now = self.now();
        self.read(|db| {
            let mut s = Status::default();
            for state in [OPEN, CLAIMED, DONE, FAILED] {
                s.tasks.insert(state.into(), count(db, "SELECT COUNT(*) FROM tasks WHERE state = :s", Params::new().set("s", Value::Str(state.into())))?);
            }
            s.lapsed = count(db, "SELECT COUNT(*) FROM tasks WHERE state = 'claimed' AND lease_until < :now", Params::new().set("now", Value::Timestamp(now)))?;
            s.findings = count(db, "SELECT COUNT(*) FROM findings", Params::new())?;
            s.duplicates = count(db, "SELECT COUNT(*) FROM findings WHERE dup_of IS NOT NULL", Params::new())?;
            s.events = one(db, "SELECT n FROM counters WHERE id = 'events'", Params::new())?.map_or(0, |r| int(&r[0]));
            Ok(s)
        })
    }
}

/// A call's transaction: its rows, the numbers it takes, its events.
struct Tx<'d> {
    txn: WriteTxn<'d>,
    now: i64,
    agent: String,
}

impl<'d> Tx<'d> {
    fn begin(db: &'d Db, now: i64, agent: &str) -> Result<Tx<'d>> {
        Ok(Tx { txn: db.storage().begin()?, now, agent: agent.to_string() })
    }

    /// The next number of counter `name`, from 1.
    fn next(&mut self, name: &str) -> Result<i64> {
        let key = Value::Str(name.to_string());
        let mut t = self.txn.table("counters")?;
        t.add_or_insert(vec![key.clone(), Value::Int(0)], "n", &Value::Int(1))?;
        match t.value(&key, "n")? {
            Some(Value::Int(n)) => Ok(n),
            other => Err(Error::Corrupt(format!("counter {name:?} holds {other:?}"))),
        }
    }

    /// Whether a task has this id, one this transaction wrote too.
    fn has_task(&mut self, id: &str) -> Result<bool> {
        Ok(self.txn.table("tasks")?.value(&Value::Str(id.to_string()), "seq")?.is_some())
    }

    fn event(&mut self, kind: &str, about: &str, body: &str) -> Result<()> {
        let id = self.next("events")?;
        let body: String = body.chars().take(MAX_TITLE).collect();
        self.txn.table("events")?.upsert(vec![Value::Int(id), Value::Timestamp(self.now), Value::Str(self.agent.clone()), Value::Str(kind.into()), Value::Str(about.into()), Value::Str(body)])
    }

    fn put_task(&mut self, t: &Task) -> Result<()> {
        self.txn.table("tasks")?.upsert(vec![
            Value::Str(t.id.clone()),
            Value::Int(t.seq),
            Value::Str(t.title.clone()),
            Value::Str(t.spec.clone()),
            Value::Str(t.state.clone()),
            opt(&t.owner),
            t.lease_until.map_or(Value::Null, Value::Timestamp),
            Value::Int(t.attempts),
            opt(&t.result),
            Value::Timestamp(t.ts),
        ])
    }

    fn put_finding(&mut self, f: &Finding) -> Result<()> {
        self.txn.table("findings")?.upsert(vec![Value::Str(f.id.clone()), Value::Int(f.seq), opt(&f.task), Value::Str(f.agent.clone()), Value::Str(f.body.clone()), opt(&f.dup_of), Value::Timestamp(f.ts)])
    }
}

const TASK: &str = "id, seq, title, spec, state, owner, lease_until, attempts, result, ts";
const FINDING: &str = "id, seq, task, agent, body, dup_of, ts";

fn opt(v: &Option<String>) -> Value {
    v.as_ref().map_or(Value::Null, |s| Value::Str(s.clone()))
}

fn text(v: &Value) -> Option<String> {
    match v {
        Value::Str(s) => Some(s.clone()),
        _ => None,
    }
}

fn need(v: &Value) -> Result<String> {
    text(v).ok_or_else(|| Error::Corrupt(format!("a blackboard's row holds {v:?} where it holds text")))
}

fn int(v: &Value) -> i64 {
    match v {
        Value::Int(n) | Value::Timestamp(n) => *n,
        _ => 0,
    }
}

fn time(v: &Value) -> Option<i64> {
    match v {
        Value::Int(n) | Value::Timestamp(n) => Some(*n),
        _ => None,
    }
}

fn rows(db: &Db, sql: &str, params: Params) -> Result<Vec<Vec<Value>>> {
    Ok(match db.sql(sql, &params)? {
        SqlResult::Rows(r) => r.rows,
        SqlResult::Written(_) => Vec::new(),
    })
}

fn one(db: &Db, sql: &str, params: Params) -> Result<Option<Vec<Value>>> {
    Ok(rows(db, sql, params)?.into_iter().next())
}

fn count(db: &Db, sql: &str, params: Params) -> Result<i64> {
    Ok(one(db, sql, params)?.map_or(0, |r| int(&r[0])))
}

fn task_of(r: &[Value]) -> Result<Task> {
    Ok(Task {
        id: need(&r[0])?,
        seq: int(&r[1]),
        title: text(&r[2]).unwrap_or_default(),
        spec: text(&r[3]).unwrap_or_default(),
        state: need(&r[4])?,
        owner: text(&r[5]),
        lease_until: time(&r[6]),
        attempts: int(&r[7]),
        result: text(&r[8]),
        ts: int(&r[9]),
    })
}

fn finding_of(r: &[Value]) -> Result<Finding> {
    Ok(Finding { id: need(&r[0])?, seq: int(&r[1]), task: text(&r[2]), agent: need(&r[3])?, body: text(&r[4]).unwrap_or_default(), dup_of: text(&r[5]), ts: int(&r[6]) })
}

fn tasks(db: &Db, rest: &str, params: Params) -> Result<Vec<Task>> {
    rows(db, &format!("SELECT {TASK} FROM tasks {rest}"), params)?.iter().map(|r| task_of(r)).collect()
}

fn task(db: &Db, id: &str) -> Result<Option<Task>> {
    Ok(tasks(db, "WHERE id = :id", Params::new().set("id", Value::Str(id.to_string())))?.pop())
}

fn lease_micros(lease_s: u64) -> BResult<i64> {
    match lease_s {
        1..=MAX_LEASE_S => Ok(lease_s as i64 * 1_000_000),
        _ => Err(BoardError::Refused(format!("a lease is 1 to {MAX_LEASE_S} seconds, not {lease_s}"))),
    }
}

/// An agent's name, a task's id, a key: 1 to 200 characters, none of them
/// a control character.
fn check_name(what: &str, name: &str) -> BResult<()> {
    match (1..=200).contains(&name.chars().count()) && !name.chars().any(char::is_control) && name.trim() == name {
        true => Ok(()),
        false => Err(BoardError::Refused(format!("{what} is 1 to 200 characters, without control characters or spaces around it"))),
    }
}

fn check_text(what: &str, text: &str, min: usize, max: usize) -> BResult<()> {
    let n = text.chars().count();
    match n >= min && n <= max && (min == 0 || !text.trim().is_empty()) {
        true => Ok(()),
        false => Err(BoardError::Refused(format!("{what} is {min} to {max} characters, not {n}"))),
    }
}

/// The words of a text: its runs of letters and digits, in lower case.
fn words(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect()
}

/// A `MATCH` query for rows with at least `least` (a count or a
/// percentage) of `words`; none without words.
fn any_of(words: &BTreeSet<String>, least: &str) -> Option<String> {
    // The longest words say most about a text; a query has a bound.
    let mut by_length: Vec<&String> = words.iter().collect();
    by_length.sort_by_key(|w| std::cmp::Reverse(w.chars().count()));
    by_length.truncate(MATCH_WORDS);
    match by_length.len() {
        0 => None,
        _ => Some(format!("({})@{least}", by_length.iter().map(|w| w.as_str()).collect::<Vec<_>>().join(" "))),
    }
}

/// The share of their words two texts have in common.
fn jaccard(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    let both = a.intersection(b).count();
    match a.len() + b.len() - both {
        0 => 0.0,
        either => both as f64 / either as f64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_and_their_share() {
        let a = words("The build fails: linker error on ARM64, in release.");
        let b = words("the build FAILS (linker error on arm64 in release)");
        assert_eq!(jaccard(&a, &b), 1.0);
        let c = words("The tests pass on ARM64");
        assert!(jaccard(&a, &c) < 0.5);
        assert_eq!(jaccard(&words(""), &words("")), 0.0);
        assert_eq!(any_of(&words("a NOT b"), "1").as_deref(), Some("(not a b)@1"));
        assert_eq!(any_of(&words("?!"), "1"), None);
    }

    #[test]
    fn names_and_times() {
        assert!(check_name("agent", "worker-7").is_ok());
        assert!(check_name("agent", "").is_err());
        assert!(check_name("agent", " a").is_err());
        assert!(check_name("agent", "a\nb").is_err());
        assert!(lease_micros(0).is_err());
        assert_eq!(lease_micros(60).unwrap(), 60_000_000);
        let t = Task { id: "t1".into(), seq: 1, title: "x".into(), spec: String::new(), state: OPEN.into(), owner: None, lease_until: Some(1_000_000), attempts: 0, result: None, ts: 0 };
        let j = serde_json::to_value(&t).unwrap();
        assert_eq!(j["lease_until"], "1970-01-01T00:00:01Z");
        assert_eq!(serde_json::from_value::<Task>(j).unwrap(), t);
    }
}
