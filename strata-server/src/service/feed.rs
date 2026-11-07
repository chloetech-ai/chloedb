//! A server's feed: the changes of its numbered transactions, one line of
//! JSON each, in order, for another cluster to follow
//! ([`strata::storage::feed`]). A cluster's feed is each shard's, read
//! from its primary through a coordinator (`GET /v1/feed?shard=i`, gRPC
//! `Feed`); a server of its own serves its own.
//!
//! The first line says where the feed starts, or why it cannot:
//! `{"feed": {"from", "last", "term"}}`, `{"gone": {"oldest"}}` when the
//! ship log no longer holds the number asked for, or `{"diverged": {"seq",
//! "term"}}` when the reader's last transaction is not this server's
//! (a failover lost it). Then one line per transaction,
//! `{"seq", "term", "ops": [...]}`, whose operations `/v1/write` takes
//! (`upsert`, or `insert` in a table without a primary key, `delete` and
//! `set`, each naming its table), and the changes of tables
//! (`create_table`, `alter_table`, `drop_table` with the schemas as the
//! log keeps them: `schema`, and `was` before an alter or a drop) and rank profiles (`profile`). A transaction may also
//! carry the stream sources' offsets it wrote (`sources`), and the
//! operations of tables it could not read (`unknown`).
//!
//! A slot move's transactions say so and carry no operations: the copies
//! a coordinator writes to the shard the slots move to and the deletes of
//! the rows they left (`"moved": "copy"` or `"cleanup"`, with the epoch of
//! the cluster map), which the feed of the shard the rows came from
//! already had. The moment slots change hands is marked on both shards,
//! before the new map lets a write reach them: `"slots": [{"left": epoch,
//! "to": shard, "slots": n, "id": ...}]` where they left, and
//! `[{"arrived": epoch, "from": shard, ...}]` where they arrived, with the
//! same `id` (the epochs may differ, when a shard took a later map). A
//! follower applies a shard's transactions after an `arrived` only once
//! it has applied the other shard's up to its `left` of the same `id`.
//! These marks are offsets the server logs under names a client's sources
//! may not take ([`MARK`]) and `_sources` does not keep
//! ([`strata::storage::WriteTxn::mark_source`]).
//!
//! Lines `{"last", "term", "time_ms"}` say, every second, how far the feed
//! reaches; a feed ends with `{"gone": ...}` once its reader falls behind
//! the ship log, or `{"error", "status"}`. On a primary it reaches the
//! transactions every replica in sync holds, so that one a failover loses
//! is not read; it ends once another primary replaces this one.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value as Json};
use strata::db::Db;
use strata::schema::Schema;
use strata::storage::feed::{Change, Changes, Cursor, Read, Start};
use strata::storage::{Mask, Snapshot, SOURCES};

use super::{conflict, Replication, Service};
use crate::slots::{self, SlotMap};
use crate::{json as js, ApiError};

/// The start of the names under which a server marks its own
/// transactions in `_sources`, which clients' stream sources may not take.
pub const MARK: &str = "_strata/";

/// A slot move's copies, written to the shard the slots move to.
pub(crate) const MOVE_COPY: &str = "_strata/move copy";

/// The deletes of the rows a slot move left on a shard.
pub(crate) const MOVE_CLEANUP: &str = "_strata/move cleanup";

/// The start of the source name by which a shard keeps the cluster it
/// follows: `_strata/follows <name>`, which only that cluster's follow
/// process writes ([`super::FOLLOW`]). The offset is twice the time it
/// was written (in ms), plus one while it follows: the last one written
/// says, as an offset only grows.
pub const FOLLOWS: &str = "_strata/follows ";

/// The offset of a [`FOLLOWS`] mark written now: following, or not.
pub fn follows_offset(following: bool) -> i64 {
    crate::telemetry::now_ms() as i64 * 2 + following as i64
}

/// Whether operation `op` carries a [`FOLLOWS`] mark.
pub(crate) fn marks_follows(op: &Json) -> bool {
    op.pointer("/source/name").and_then(Json::as_str).is_some_and(|n| n.starts_with(FOLLOWS))
}

/// The offset of the [`FOLLOWS`] mark of cluster `name` that `ops`
/// carry, the greatest.
pub(crate) fn carried_follows(ops: &[Json], name: &str) -> Option<i64> {
    ops.iter()
        .filter(|op| op.pointer("/source/name").and_then(Json::as_str).and_then(|n| n.strip_prefix(FOLLOWS)) == Some(name))
        .filter_map(|op| op.pointer("/source/offset").and_then(Json::as_i64))
        .max()
}

/// The [`FOLLOWS`] marks a shard holds, as last read from its `_sources`
/// with the term it was read in (a new primary reads again), and what
/// each cluster's follow process last said of itself
/// (`POST /v1/admin/follow/status`).
#[derive(Default)]
pub(crate) struct Follows {
    marks: std::sync::RwLock<Option<(u64, Arc<BTreeMap<String, i64>>)>>,
    status: std::sync::Mutex<BTreeMap<String, (Instant, Json)>>,
}

impl Follows {
    /// Reads them again on the next write.
    pub(crate) fn forget(&self) {
        *self.marks.write().unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// Keeps what follow process `name` says of itself.
    pub(crate) fn note(&self, name: &str, status: Json) {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).insert(name.to_string(), (Instant::now(), status));
    }

    /// What each follow process last said, and how long ago (ms).
    pub(crate) fn said(&self) -> BTreeMap<String, (u64, Json)> {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).iter().map(|(n, (t, j))| (n.clone(), (t.elapsed().as_millis() as u64, j.clone()))).collect()
    }
}

/// The cluster followed now, of [`FOLLOWS`] marks `marks`: the last
/// written, while its offset is odd.
pub(crate) fn followed(marks: &BTreeMap<String, i64>) -> Option<(&str, i64)> {
    marks.iter().max_by_key(|(_, o)| **o).filter(|(_, o)| *o % 2 == 1).map(|(n, o)| (n.as_str(), *o))
}

/// The marks of the slots that change hands on shard `shard` as it takes
/// cluster map `new` in place of `old`: per shard they leave for,
/// `{"left": epoch, "to": shard, "slots": n, "id": ...}`, and per shard
/// they arrive from, `{"arrived": epoch, "from": shard, ...}`, where `id`
/// names the slots (a hash of them), the same on both shards.
pub(crate) fn flip_marks(old: &SlotMap, new: &SlotMap, shard: usize) -> Vec<String> {
    let mut left: BTreeMap<usize, Vec<u16>> = BTreeMap::new();
    let mut arrived: BTreeMap<usize, Vec<u16>> = BTreeMap::new();
    for s in 0..slots::SLOTS {
        let (a, b) = (old.owner(s), new.owner(s));
        if a != b && a == shard {
            left.entry(b).or_default().push(s as u16);
        } else if a != b && b == shard {
            arrived.entry(a).or_default().push(s as u16);
        }
    }
    let id = |slots: &[u16]| format!("{:016x}", twox_hash::XxHash3_64::oneshot(&slots.iter().flat_map(|s| s.to_le_bytes()).collect::<Vec<u8>>()));
    let left = left.iter().map(|(to, s)| json!({"left": new.epoch, "to": to, "slots": s.len(), "id": id(s)}));
    let arrived = arrived.iter().map(|(from, s)| json!({"arrived": new.epoch, "from": from, "slots": s.len(), "id": id(s)}));
    left.chain(arrived).map(|j| format!("{MARK}slots {j}")).collect()
}

/// How many bytes of the log one read takes.
const READ_BYTES: usize = 1 << 20;

/// How often a feed with nothing new looks again.
const POLL: Duration = Duration::from_millis(25);

/// How often a feed says how far it reaches.
const BEAT: Duration = Duration::from_secs(1);

/// How long a copy waits for every replica in sync to hold its version.
const COPY_WAIT: Duration = Duration::from_secs(30);

/// How many rows a line of a copy holds.
const COPY_ROWS: usize = 500;

/// How to stop following, as a refused write says.
const FOLLOW_STOP: &str = "POST /v1/admin/follow/stop";

impl Service {
    /// This server's feed from transaction `from` on, in chunks of lines
    /// (each ending with a newline). `after` is the term of transaction
    /// `from - 1` as the reader holds it, if it holds one. With `copy`,
    /// the feed starts with a copy of the server's rows instead
    /// ([`Self::copy_feed`]), and `from` and `after` are not read.
    pub async fn feed(self: &Arc<Self>, from: u64, after: Option<u64>, copy: bool) -> Result<tokio::sync::mpsc::Receiver<Vec<u8>>, ApiError> {
        let db = self.local().cloned().ok_or_else(|| ApiError::bad("a cluster's feed is each shard's: name one (?shard=0 for the first)"))?;
        if db.storage().numbering().is_none() {
            return Err(conflict("this server does not number its writes, so it has no feed: start it with --seq-no (a shard with replicas numbers them)".into()));
        }
        self.feed_reaches()?;
        if copy {
            return self.copy_feed(db).await;
        }
        let d = db.clone();
        let start = tokio::task::spawn_blocking(move || d.storage().changes_from(from, after)).await.map_err(ApiError::internal)??;
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        let Some(cursor) = started(start, &tx).await else { return Ok(rx) };
        let n = db.storage().numbering().unwrap_or_default();
        let _ = tx.send(lines(&[json!({"feed": {"from": cursor.next(), "last": n.seq, "term": n.term}})])).await;
        tokio::spawn(tail(self.clone(), db, cursor, tx, None));
        Ok(rx)
    }

    /// A feed that starts with a copy of this server's rows, for a
    /// follower's first start: rows that took no number (bulk loads,
    /// attached segments) are in no transaction. It pins a version that
    /// every replica in sync holds, and sends
    ///
    /// - `{"copy": {"seq", "term", "epoch", "tables", "profiles"}}`: the
    ///   version's last transaction, the slot map's epoch (0 with none),
    ///   and its tables' schemas and rank profiles (as `profile`
    ///   operations take them);
    /// - `{"copy": true, "ops": [...]}`: its rows, as upserts;
    /// - `{"copied": {"seq", "term", "rows", "sources"}}`: the stream
    ///   sources' offsets at the version;
    ///
    /// and then the feed from the transaction after it, which the log
    /// keeps until the reader reaches its end. The rows of slots this
    /// shard holds without serving them (a move's) are left out: the
    /// version is taken as no new slot map is, so they are those of its
    /// epoch. Every table needs a primary key, by which the follower
    /// replaces rows.
    async fn copy_feed(self: &Arc<Self>, db: Arc<Db>) -> Result<tokio::sync::mpsc::Receiver<Vec<u8>>, ApiError> {
        let (snap, epoch) = {
            // A slot map's marks are written before the map is taken
            // ([`Self::set_slots`]): taken while none is, the version has
            // the marks of the epoch it serves.
            let _taking = self.slots_file.lock().await;
            let held = self.slots.read().unwrap_or_else(|e| e.into_inner());
            let epoch = held.as_ref().map_or(0, |(m, _)| m.epoch);
            let snap = db.snapshot();
            let mask = self.moving.mask();
            drop(held);
            match mask {
                Some(m) => (snap.masked(&m)?, epoch),
                None => (snap, epoch),
            }
        };
        let seq = snap.seq_no().unwrap_or(0);
        let hold = Held { id: db.storage().hold_log(seq + 1), db: db.clone() };
        let tables: Vec<Schema> = snap.schemas().filter(|t| t.table != SOURCES && !t.is_placeholder()).cloned().collect();
        if let Some(t) = tables.iter().find(|t| t.primary_key.is_none()) {
            return Err(ApiError::bad(format!("table {} has no primary key: a follower copies rows by their keys", t.table)));
        }
        let deadline = Instant::now() + COPY_WAIT;
        while self.feed_reaches()? < seq {
            if Instant::now() >= deadline {
                return Err(ApiError { status: 503, message: format!("the replicas in sync did not reach transaction {seq} within {} s: copy again", COPY_WAIT.as_secs()) });
            }
            tokio::time::sleep(POLL).await;
        }
        let term = db.storage().numbering().map_or(0, |n| n.term_at(seq));
        let profiles: Vec<Json> = tables
            .iter()
            .filter_map(|t| snap.table(&t.table))
            .flat_map(|v| {
                let table = v.schema().table.clone();
                v.rank_profiles().iter().map(move |p| json!({"op": "profile", "table": table, "name": p.name, "def": serde_json::to_value(p).unwrap_or(Json::Null)})).collect::<Vec<_>>()
            })
            .collect();
        let d = db.clone();
        let start = tokio::task::spawn_blocking(move || d.storage().changes_from(seq + 1, None)).await.map_err(ApiError::internal)??;
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        let Some(cursor) = started(start, &tx).await else { return Ok(rx) };
        let head = json!({"copy": {"seq": seq, "term": term, "epoch": epoch, "tables": tables.iter().map(schema_json).collect::<Vec<_>>(), "profiles": profiles}});
        let _ = tx.send(lines(&[head])).await;
        let svc = self.clone();
        tokio::spawn(async move {
            let (d, t) = (db.clone(), tx.clone());
            let copied = tokio::task::spawn_blocking(move || copy_rows(&d, &snap, &tables, &t)).await;
            let end = match copied {
                Ok(Ok(Some((rows, sources)))) => json!({"copied": {"seq": seq, "term": term, "rows": rows, "sources": sources}}),
                // The reader left.
                Ok(Ok(None)) => return,
                Ok(Err(e)) => json!({"error": e.message, "status": e.status}),
                Err(e) => json!({"error": format!("the copy stopped: {e}"), "status": 500}),
            };
            let failed = end.get("error").is_some();
            if tx.send(lines(&[end])).await.is_err() || failed {
                return;
            }
            tail(svc, db, cursor, tx, Some(hold)).await;
        });
        Ok(rx)
    }

    /// The [`FOLLOWS`] marks of this shard, by the name of the cluster:
    /// the one written last says which this shard follows, if its offset
    /// is odd. A coordinator holds none (its shards do).
    pub(crate) fn follow_marks(&self) -> Result<Arc<BTreeMap<String, i64>>, ApiError> {
        let Some(db) = self.local() else { return Ok(Arc::default()) };
        let term = self.term();
        if let Some((t, m)) = &*self.follows.marks.read().unwrap_or_else(|e| e.into_inner()) {
            if *t == term {
                return Ok(m.clone());
            }
        }
        let m: Arc<BTreeMap<String, i64>> = Arc::new(db.sources()?.into_iter().filter_map(|s| s.name.strip_prefix(FOLLOWS).map(|n| (n.to_string(), s.offset))).collect());
        *self.follows.marks.write().unwrap_or_else(|e| e.into_inner()) = Some((term, m.clone()));
        Ok(m)
    }

    /// The cluster this shard follows, if any ([`FOLLOWS`]): its writes
    /// come from that cluster's follow process alone.
    pub fn follows(&self) -> Result<Option<Arc<str>>, ApiError> {
        Ok(followed(&*self.follow_marks()?).map(|(n, _)| n.into()))
    }

    /// Refuses a write (409) on a shard of a cluster that follows
    /// another ([`Self::follows`]), unless the follow process of that one
    /// sends it; and one that follow process sends once following stopped
    /// (`POST /v1/admin/follow/stop`), unless it starts again: its write
    /// carries a [`FOLLOWS`] mark (`carried`, its offset) later than the
    /// stop.
    pub(super) fn not_following(&self, carried: Option<i64>) -> Result<(), ApiError> {
        let marks = self.follow_marks()?;
        let sender = super::following();
        match (followed(&marks), sender.as_deref()) {
            (Some((source, _)), Some(s)) if s == source => Ok(()),
            (Some((source, _)), _) => Err(conflict(format!("this cluster follows {source}: it takes the writes of {source}'s follow process and none of its own; write to {source}, or stop following ({FOLLOW_STOP}) first"))),
            (None, Some(s)) => match marks.get(s) {
                Some(&stopped) if stopped % 2 == 0 && !carried.is_some_and(|c| c > stopped) => {
                    Err(conflict(format!("this cluster stopped following {s} ({FOLLOW_STOP}): its follow process stops too; start it again to follow {s} again")))
                }
                _ => Ok(()),
            },
            (None, None) => Ok(()),
        }
    }

    /// Keeps what follow process `name` says of itself, for
    /// [`Self::follow_view`].
    pub fn note_follow_status(&self, name: &str, status: Json) {
        self.follows.note(name, status);
    }

    /// The clusters this one follows or followed ([`crate::follow`]): per
    /// name, whether its shards follow it now (`following`: how many do,
    /// of `shards`), since when (`since_ms`, or `stopped_ms`), the copy
    /// it started from (`copy_ms`, `copied`), how far it applied each
    /// source shard's feed (`applied`, the least over this cluster's
    /// shards), and what its follow process last said (`status`, with
    /// `status_age_ms`), which only the coordinator it writes through
    /// holds.
    pub async fn follow_view(self: &Arc<Self>) -> Result<Json, ApiError> {
        let shards = self.sources_by_shard().await?;
        let said = self.follows.said();
        let mut names: BTreeMap<String, ()> = said.keys().map(|n| (n.clone(), ())).collect();
        let marks: Vec<BTreeMap<String, i64>> = shards.iter().map(|s| s.iter().filter_map(|x| x.name.strip_prefix(FOLLOWS).map(|n| (n.to_string(), x.offset))).collect()).collect();
        for m in &marks {
            names.extend(m.keys().map(|n| (n.clone(), ())));
        }
        let mut clusters = Vec::new();
        for name in names.keys() {
            let following = marks.iter().filter(|m| followed(m).is_some_and(|(n, _)| n == name)).count();
            let last = marks.iter().filter_map(|m| m.get(name)).max().copied();
            // The least over the shards that hold each mark.
            let prefix = format!("follow/{name}/");
            let mut least: BTreeMap<&str, i64> = BTreeMap::new();
            for x in shards.iter().flatten() {
                if let Some(rest) = x.name.strip_prefix(&prefix) {
                    let e = least.entry(rest).or_insert(x.offset);
                    *e = (*e).min(x.offset);
                }
            }
            let copy = shards.iter().flatten().filter(|x| x.name.strip_prefix(&prefix) == Some("copy")).map(|x| x.offset).max();
            let gen = copy.map(|c| format!("{c}/")).unwrap_or_default();
            let applied: BTreeMap<usize, Json> = least
                .iter()
                .filter_map(|(rest, o)| {
                    let i = rest.strip_prefix(&gen)?.strip_suffix("/at")?.parse::<usize>().ok()?;
                    let term = least.get(format!("{gen}{i}/term").as_str()).copied().unwrap_or(0);
                    Some((i, json!({"shard": i, "seq": o, "term": term})))
                })
                .collect();
            let mut j = json!({
                "name": name,
                "following": following,
                "shards": shards.len(),
                "copy_ms": copy,
                "copied": copy.is_some_and(|_| least.contains_key(format!("{gen}copied").as_str())),
                "applied": applied.into_values().collect::<Vec<_>>(),
            });
            match last {
                Some(o) if o % 2 == 1 => j["since_ms"] = json!(o / 2),
                Some(o) => j["stopped_ms"] = json!(o / 2),
                None => {}
            }
            if let Some((age, st)) = said.get(name) {
                j["status"] = st.clone();
                j["status_age_ms"] = json!(age);
            }
            clusters.push(j);
        }
        let now = marks.iter().find_map(|m| followed(m).map(|(n, _)| n.to_string()));
        Ok(json!({"following": now, "clusters": clusters}))
    }

    /// Stops following cluster `name`: a [`FOLLOWS`] mark of it, even,
    /// on every shard. Its follow process's writes are refused from then
    /// on, and clients' taken.
    pub async fn stop_following(self: &Arc<Self>, name: &str) -> Result<Json, ApiError> {
        if !super::follow_name_ok(name) {
            return Err(ApiError::bad("a cluster's name is 1 to 64 letters, digits, '.', '_' or '-'"));
        }
        let view = self.follow_view().await?;
        let Some(c) = view["clusters"].as_array().into_iter().flatten().find(|c| c["name"] == name) else {
            return Err(ApiError { status: 404, message: format!("this cluster never followed {name}") });
        };
        if c["following"] == 0 {
            return Ok(json!({"stopped": name, "already": true}));
        }
        let at = follows_offset(false);
        let op = json!({"op": "source", "source": {"name": format!("{FOLLOWS}{name}"), "offset": at}});
        super::FOLLOW.scope(Some(name.into()), self.write_ops(vec![op], None)).await?;
        Ok(json!({"stopped": name, "at_ms": at / 2}))
    }

    /// The last transaction the feed may read now: on a primary, the last
    /// every replica in sync holds; refused on a replica, and on a primary
    /// another replaced, whose transactions from then on are lost.
    fn feed_reaches(&self) -> Result<u64, ApiError> {
        match self.replication() {
            Replication::Replica(_) => Err(conflict("this server is a replica: read the shard's feed from its primary (through a coordinator)".into())),
            Replication::Primary(p) if p.replaced_by() > 0 => Err(conflict(format!("this server is no longer its shard's primary (term {} replaced it): read the feed from the new one", p.replaced_by()))),
            Replication::Primary(p) => Ok(p.global_checkpoint()),
            Replication::None => Ok(self.local().and_then(|db| db.storage().seq_no()).map_or(0, |s| s.seq)),
        }
    }
}

/// The cursor `start` gives, or else its line sent on `tx`.
async fn started(start: Start, tx: &tokio::sync::mpsc::Sender<Vec<u8>>) -> Option<Cursor> {
    match start {
        Start::Gone { oldest } => {
            let _ = tx.send(lines(&[json!({"gone": {"oldest": oldest}})])).await;
            None
        }
        Start::Diverged { seq, term } => {
            let _ = tx.send(lines(&[json!({"diverged": {"seq": seq, "term": term}})])).await;
            None
        }
        Start::At(c) => Some(c),
    }
}

/// A hold of the ship log ([`strata::storage::Database::hold_log`]),
/// released when dropped.
struct Held {
    db: Arc<Db>,
    id: u64,
}

impl Drop for Held {
    fn drop(&mut self) {
        self.db.storage().release_log(self.id);
    }
}

/// Sends `tx` the transactions from `cursor` on as `svc`'s feed reaches
/// them, with a line of how far it reaches every second, until the
/// reader leaves or the feed ends. `hold` keeps the log from the cursor
/// until the reader first reaches its end.
async fn tail(svc: Arc<Service>, db: Arc<Db>, mut cursor: Cursor, tx: tokio::sync::mpsc::Sender<Vec<u8>>, mut hold: Option<Held>) {
    let mut beat: Option<Instant> = None;
    loop {
        let upto = match svc.feed_reaches() {
            Ok(u) => u,
            Err(e) => {
                let _ = tx.send(lines(&[json!({"error": e.message, "status": e.status})])).await;
                return;
            }
        };
        if beat.is_none_or(|b| b.elapsed() >= BEAT) {
            let term = db.storage().numbering().map_or(0, |n| n.term_at(upto));
            if tx.send(lines(&[json!({"last": upto, "term": term, "time_ms": crate::telemetry::now_ms()})])).await.is_err() {
                return;
            }
            beat = Some(Instant::now());
        }
        let d = db.clone();
        let read = tokio::task::spawn_blocking(move || {
            let r = d.storage().read_changes(&mut cursor, READ_BYTES, upto);
            (cursor, r)
        })
        .await;
        let r = match read {
            Ok((c, r)) => {
                cursor = c;
                r
            }
            Err(e) => {
                let _ = tx.send(lines(&[json!({"error": format!("the feed's reader stopped: {e}"), "status": 500})])).await;
                return;
            }
        };
        let last = match r {
            Ok(Read::Txns(t)) if t.is_empty() => {
                drop(hold.take());
                if tx.is_closed() {
                    return;
                }
                tokio::time::sleep(POLL).await;
                continue;
            }
            Ok(Read::Txns(t)) => lines(&t.iter().map(line).collect::<Vec<_>>()),
            Ok(Read::Gone { oldest }) => {
                let _ = tx.send(lines(&[json!({"gone": {"oldest": oldest}})])).await;
                return;
            }
            Err(e) => {
                let e = ApiError::from(e);
                let _ = tx.send(lines(&[json!({"error": e.message, "status": e.status})])).await;
                return;
            }
        };
        if tx.send(last).await.is_err() {
            return;
        }
    }
}

/// Sends `tx` every row of `tables` in `snap` as upserts, [`COPY_ROWS`]
/// a line. Returns the rows sent and the stream sources' offsets at
/// `snap`, or `None` once the reader left.
fn copy_rows(db: &Db, snap: &Snapshot, tables: &[Schema], tx: &tokio::sync::mpsc::Sender<Vec<u8>>) -> Result<Option<(u64, Vec<Json>)>, ApiError> {
    let every = Mask::new(|_, _| true);
    let mut sent = 0u64;
    for schema in tables {
        let Some(t) = snap.table(&schema.table) else { continue };
        let rows = t.covered(&every, None)?;
        for chunk in rows.chunks(COPY_ROWS) {
            let ops: Vec<Json> = db.documents_of(&t, chunk)?.iter().map(|d| json!({"op": "upsert", "table": schema.table, "doc": js::document_json(t.schema(), d)})).collect();
            sent += ops.len() as u64;
            if tx.blocking_send(lines(&[json!({"copy": true, "ops": ops})])).is_err() {
                return Ok(None);
            }
        }
    }
    let sources = db.sources_at(snap)?.into_iter().filter(|s| !s.name.starts_with(MARK)).map(|s| json!({"name": s.name, "offset": s.offset})).collect();
    Ok(Some((sent, sources)))
}

/// `js` as lines of JSON.
fn lines(js: &[Json]) -> Vec<u8> {
    let mut out = Vec::new();
    for j in js {
        serde_json::to_writer(&mut out, j).expect("JSON serialises");
        out.push(b'\n');
    }
    out
}

/// A schema as the log keeps it, which `Schema`'s JSON reads back.
fn schema_json(s: &Schema) -> Json {
    serde_json::to_value(s).unwrap_or(Json::Null)
}

/// The line of one transaction.
pub(crate) fn line(t: &Changes) -> Json {
    let mut j = json!({"seq": t.seq.seq, "term": t.seq.term});
    let mut ops = Vec::with_capacity(t.changes.len());
    let mut sources = Vec::new();
    let mut slots = Vec::new();
    let mut moved = None;
    for c in &t.changes {
        match c {
            Change::Upsert { table, doc } => {
                let op = if table.primary_key.is_some() { "upsert" } else { "insert" };
                ops.push(json!({"op": op, "table": table.table, "doc": js::document_json(table, doc)}));
            }
            Change::Delete { table, key } => ops.push(json!({"op": "delete", "table": table.table, "key": js::to_json(key)})),
            Change::Set { table, key, field, value } => {
                ops.push(json!({"op": "set", "table": table.table, "key": js::to_json(key), "field": table.fields[*field].name, "value": js::to_json(value)}));
            }
            Change::Profile { table, name, def } => {
                ops.push(json!({"op": "profile", "table": table.table, "name": name, "def": def.as_ref().map(|p| serde_json::to_value(&**p).unwrap_or(Json::Null))}));
            }
            Change::Create { def } => ops.push(json!({"op": "create_table", "table": def.table, "schema": schema_json(def)})),
            Change::Alter { was, def } => ops.push(json!({"op": "alter_table", "table": was.table, "was": schema_json(was), "schema": schema_json(def)})),
            Change::Drop { was } => ops.push(json!({"op": "drop_table", "table": was.table, "was": schema_json(was)})),
            Change::Source { name, offset } => match name.strip_prefix(MARK) {
                None => sources.push(json!({"name": name, "offset": offset})),
                Some(_) if name == MOVE_COPY => moved = Some(("copy", *offset)),
                Some(_) if name == MOVE_CLEANUP => moved = Some(("cleanup", *offset)),
                Some(m) => {
                    if let Some(j) = m.strip_prefix("slots ").and_then(|j| serde_json::from_str::<Json>(j).ok()) {
                        slots.push(j);
                    }
                }
            },
        }
    }
    // A slot move's copies and deletes: the shard the rows came from had
    // them.
    if let Some((what, epoch)) = moved {
        j["moved"] = json!(what);
        j["epoch"] = json!(epoch);
        return j;
    }
    j["ops"] = Json::Array(ops);
    if !sources.is_empty() {
        j["sources"] = Json::Array(sources);
    }
    if !slots.is_empty() {
        j["slots"] = Json::Array(slots);
    }
    if t.unknown > 0 {
        j["unknown"] = json!(t.unknown);
    }
    j
}
