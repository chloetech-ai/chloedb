//! A server's feed: the changes of its numbered transactions, one line of
//! JSON each, in order, for another cluster to follow
//! ([`chloedb::storage::feed`]). A cluster's feed is each shard's, read
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
//! ([`chloedb::storage::WriteTxn::mark_source`]).
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
use chloedb::db::Db;
use chloedb::schema::Schema;
use chloedb::storage::feed::{Change, Changes, Cursor, Read, Start};
use chloedb::storage::{Mask, Snapshot, SOURCES};

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

/// The mark by which a shard holds its clients' writes
/// (`POST /v1/admin/hold-writes`, for a switch-over to a cluster that
/// follows this one): as for [`FOLLOWS`], its offset is twice the time it
/// was written (in ms), plus one while it holds. Its transaction is the
/// last with a client's rows until the hold ends, and a feed says so
/// (`"hold": true`).
pub const HOLD: &str = "_strata/hold";

/// The start of the marks by which a cluster keeps its promotion from a
/// cluster it followed (`POST /v1/admin/follow/promote`):
/// `_strata/promoted <name>`, when (in ms), and `_strata/promoted
/// <name>/<j>`, the number of its shard `j`'s transaction that ended
/// following, after which every transaction is its own.
pub const PROMOTED: &str = "_strata/promoted ";

/// Whether `name` is a mark a server keeps of its own state in `_sources`
/// ([`FOLLOWS`], [`HOLD`], [`PROMOTED`]), which a request may carry from
/// the cluster's own alone ([`super::own_marks_ok`]).
pub(crate) fn own_mark(name: &str) -> bool {
    name.starts_with(FOLLOWS) || name == HOLD || name.starts_with(PROMOTED)
}

/// The offset of a [`FOLLOWS`] mark written now: following, or not.
pub fn follows_offset(following: bool) -> i64 {
    crate::telemetry::now_ms() as i64 * 2 + following as i64
}

/// The offset of a mark of [`FOLLOWS`]'s kind (twice the time, plus `on`)
/// written now after one of offset `cur`: past it, though a clock behind
/// the one that wrote it.
pub fn next_offset(cur: Option<i64>, on: bool) -> i64 {
    let now = crate::telemetry::now_ms() as i64 * 2 + on as i64;
    match cur {
        Some(c) if c >= now => c + 1 + ((c + 1) % 2 != on as i64) as i64,
        _ => now,
    }
}

/// The name an operation's source has.
fn source_name(op: &Json) -> Option<&str> {
    op.pointer("/source/name").and_then(Json::as_str)
}

/// The greatest offset `ops` carry of source `name`.
fn carried(ops: &[Json], name: &str) -> Option<i64> {
    ops.iter().filter(|op| source_name(op) == Some(name)).filter_map(|op| op.pointer("/source/offset").and_then(Json::as_i64)).max()
}

/// Whether operation `op` carries one of the server's own marks
/// ([`own_mark`]).
pub(crate) fn marks_own(op: &Json) -> bool {
    source_name(op).is_some_and(own_mark)
}

/// Whether operation `op` carries a [`PROMOTED`] mark.
pub(crate) fn marks_promoted(op: &Json) -> bool {
    source_name(op).is_some_and(|n| n.starts_with(PROMOTED))
}

/// Whether operation `op` carries a [`HOLD`] mark.
pub(crate) fn marks_hold(op: &Json) -> bool {
    source_name(op) == Some(HOLD)
}

/// The offset of the [`FOLLOWS`] mark of cluster `name` that `ops`
/// carry, the greatest.
pub(crate) fn carried_follows(ops: &[Json], name: &str) -> Option<i64> {
    carried(ops, &format!("{FOLLOWS}{name}"))
}

/// The offset of the [`HOLD`] mark `ops` carry, the greatest.
pub(crate) fn carried_hold(ops: &[Json]) -> Option<i64> {
    carried(ops, HOLD)
}

/// The offset `ops` carry of the mark by which the follow process of
/// cluster `name` names the copy it follows from (`follow/<name>/copy`,
/// when it began, in ms: [`crate::follow`]).
pub(crate) fn carried_copy(ops: &[Json], name: &str) -> Option<i64> {
    carried(ops, &format!("follow/{name}/copy"))
}

/// A shard's own marks ([`own_mark`]) as it holds them.
#[derive(Debug, Default)]
pub(crate) struct Marks {
    /// The [`FOLLOWS`] marks, by the name of the cluster.
    pub follows: BTreeMap<String, i64>,
    /// The [`HOLD`] mark.
    pub hold: Option<i64>,
    /// When it was promoted from each cluster ([`PROMOTED`], in ms), by
    /// its name.
    pub promoted: BTreeMap<String, i64>,
}

impl Marks {
    /// The marks among sources `all` (name and offset).
    pub(crate) fn of<'a>(all: impl IntoIterator<Item = (&'a str, i64)>) -> Marks {
        let mut m = Marks::default();
        for (name, offset) in all {
            if let Some(n) = name.strip_prefix(FOLLOWS) {
                m.follows.insert(n.to_string(), offset);
            } else if name == HOLD {
                m.hold = Some(offset);
            } else if let Some(n) = name.strip_prefix(PROMOTED).filter(|n| !n.contains('/')) {
                m.promoted.insert(n.to_string(), offset);
            }
        }
        m
    }

    /// Since when (ms) the shard holds its clients' writes, if it does.
    pub(crate) fn holds(&self) -> Option<i64> {
        self.hold.filter(|h| h % 2 == 1).map(|h| h / 2)
    }
}

/// A shard's own marks ([`Marks`]), as last read from its `_sources` with
/// the term it was read in (a new primary reads again); the holds of its
/// writes under way ([`Service::hold_pending`]); and what each cluster's
/// follow process last said of itself (`POST /v1/admin/follow/status`).
#[derive(Default)]
pub(crate) struct Follows {
    marks: std::sync::RwLock<Option<(u64, Arc<Marks>)>>,
    /// Writes of a [`HOLD`] under way: from before the writes it waits
    /// for commit until it does, the shard takes no other.
    holding: std::sync::atomic::AtomicU64,
    status: std::sync::Mutex<BTreeMap<String, (Instant, Json)>>,
}

/// A hold under way ([`Follows::holding`]), until dropped.
pub(crate) struct Holding<'a>(&'a Follows);

impl Drop for Holding<'_> {
    fn drop(&mut self) {
        self.0.holding.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
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

/// Each shard's own marks ([`Marks`]) of its stream sources `shards`
/// (`_sources`, in shard order).
pub(crate) fn own_marks_of(shards: &[Vec<chloedb::db::Source>]) -> impl Iterator<Item = Marks> + '_ {
    shards.iter().map(|s| Marks::of(s.iter().map(|x| (x.name.as_str(), x.offset))))
}

/// The number of each shard's transaction that ended following cluster
/// `name` at its promotion ([`PROMOTED`]), once every shard's is kept:
/// each shard keeps them all, the greatest of each counts.
pub(crate) fn promoted_seqs(shards: &[Vec<chloedb::db::Source>], name: &str) -> Option<Vec<u64>> {
    let prefix = format!("{PROMOTED}{name}/");
    let mut seqs: BTreeMap<usize, u64> = BTreeMap::new();
    for x in shards.iter().flatten() {
        if let Some(j) = x.name.strip_prefix(&prefix).and_then(|j| j.parse::<usize>().ok()) {
            let e = seqs.entry(j).or_default();
            *e = (*e).max(x.offset.max(0) as u64);
        }
    }
    let n = shards.len();
    (seqs.len() == n && seqs.keys().copied().eq(0..n)).then(|| seqs.into_values().collect())
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

/// The call that holds a cluster's writes, and ends the hold.
pub(crate) const HOLD_WRITES: &str = "POST /v1/admin/hold-writes";

/// How long a follow process that says how it stands every 2 s may be
/// silent before it counts as behind for as long ([`Service::follow_lags`]).
pub const SILENT_MS: u64 = 10_000;

impl Service {
    /// This server's feed from transaction `from` on, in chunks of lines
    /// (each ending with a newline). `after` is the term of transaction
    /// `from - 1` as the reader holds it, if it holds one. With `copy`,
    /// the feed starts with a copy of the server's rows instead
    /// ([`Self::copy_feed`]): `from` is not read, and `after` is the term
    /// of transaction [`CopyOpts::since`] as the reader holds it.
    pub async fn feed(self: &Arc<Self>, from: u64, after: Option<u64>, copy: Option<CopyOpts>) -> Result<tokio::sync::mpsc::Receiver<Vec<u8>>, ApiError> {
        let db = self.local().cloned().ok_or_else(|| ApiError::bad("a cluster's feed is each shard's: name one (?shard=0 for the first)"))?;
        if db.storage().numbering().is_none() {
            return Err(conflict("this server does not number its writes, so it has no feed: start it with --seq-no (a shard with replicas numbers them)".into()));
        }
        self.feed_reaches()?;
        if let Some(c) = copy {
            return self.copy_feed(db, c, after).await;
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
    ///   sources' offsets at the version, and with [`CopyOpts::digests`] the
    ///   digests of its keys by slot (`digests`, [`key_digests`]), and
    ///   `"hold": true` when the shard holds its clients' writes there
    ///   ([`HOLD`], protocol 17);
    ///
    /// and then the feed from the transaction after it, which the log
    /// keeps until the reader reaches its end. The rows of slots this
    /// shard holds without serving them (a move's) are left out: the
    /// version is taken as no new slot map is, so they are those of its
    /// epoch. Every table needs a primary key, by which the follower
    /// replaces rows. With [`CopyOpts::since`] (protocol 15), only the rows
    /// a transaction after it last wrote, but every row of the tables
    /// [`CopyOpts::whole`]: a follower that fell behind the log catches up
    /// with them, and with the digests finds the keys deleted meanwhile.
    /// `since_term` is the term of transaction `since` as the follower
    /// holds it: one this shard holds under another term diverged.
    async fn copy_feed(self: &Arc<Self>, db: Arc<Db>, c: CopyOpts, since_term: Option<u64>) -> Result<tokio::sync::mpsc::Receiver<Vec<u8>>, ApiError> {
        let (snap, epoch) = self.served(&db).await?;
        let seq = snap.seq_no().unwrap_or(0);
        if c.since > seq {
            return Err(conflict(format!("this shard's last transaction is {seq}, before {}, which the follower applied: a failover lost it; copy again (--copy)", c.since)));
        }
        let term_of_since = db.storage().numbering().map_or(0, |n| n.term_at(c.since));
        if let Some(t) = since_term.filter(|&t| c.since > 0 && t != term_of_since) {
            return Err(conflict(format!("this shard holds transaction {} of term {term_of_since}, and the follower applied it of term {t}: a failover lost it; copy again (--copy)", c.since)));
        }
        let hold = Held { id: db.storage().hold_log(seq + 1), db: db.clone() };
        let tables: Vec<Schema> = snap.schemas().filter(|t| t.table != SOURCES && !t.is_placeholder()).cloned().collect();
        if let Some(t) = tables.iter().find(|t| t.primary_key.is_none()) {
            return Err(ApiError::bad(format!("table {} has no primary key: a follower copies rows by their keys", t.table)));
        }
        let deadline = Instant::now() + COPY_WAIT;
        while self.feed_reaches()? < seq {
            if Instant::now() >= deadline {
                return Err(ApiError { code: None, status: 503, message: format!("the replicas in sync did not reach transaction {seq} within {} s: copy again", COPY_WAIT.as_secs()) });
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
            let copied = tokio::task::spawn_blocking(move || {
                let Some((rows, sources)) = copy_rows(&d, &snap, &tables, c.since, &c.whole, &t)? else { return Ok(None) };
                let mut end = json!({"seq": seq, "term": term, "rows": rows, "sources": sources});
                // Whether the shard holds its clients' writes at the version.
                if d.sources_at(&snap)?.iter().any(|s| s.name == HOLD && s.offset % 2 == 1) {
                    end["hold"] = json!(true);
                }
                if c.digests {
                    let names: Vec<String> = tables.iter().map(|t| t.table.clone()).collect();
                    end["digests"] = digests_json(&key_digests(&snap, &names)?);
                }
                Ok::<_, ApiError>(Some(end))
            })
            .await;
            let end = match copied {
                Ok(Ok(Some(end))) => json!({"copied": end}),
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

    /// This shard's own marks ([`Marks`]): the [`FOLLOWS`] mark written
    /// last says which cluster it follows, if its offset is odd. A
    /// coordinator holds none (its shards do).
    pub(crate) fn own_marks(&self) -> Result<Arc<Marks>, ApiError> {
        let Some(db) = self.local() else { return Ok(Arc::default()) };
        let term = self.term();
        if let Some((t, m)) = &*self.follows.marks.read().unwrap_or_else(|e| e.into_inner()) {
            if *t == term {
                return Ok(m.clone());
            }
        }
        let all = db.sources()?;
        let m = Arc::new(Marks::of(all.iter().map(|s| (s.name.as_str(), s.offset))));
        *self.follows.marks.write().unwrap_or_else(|e| e.into_inner()) = Some((term, m.clone()));
        Ok(m)
    }

    /// The cluster this shard follows, if any ([`FOLLOWS`]): its writes
    /// come from that cluster's follow process alone.
    pub fn follows(&self) -> Result<Option<Arc<str>>, ApiError> {
        Ok(followed(&self.own_marks()?.follows).map(|(n, _)| n.into()))
    }

    /// Refuses a write (409) on a shard of a cluster that follows
    /// another ([`Self::follows`]), unless the follow process of that one
    /// sends it; and one that follow process sends once following stopped
    /// (`POST /v1/admin/follow/stop`), unless it starts again: its write
    /// carries a [`FOLLOWS`] mark (`carried`, its offset) later than the
    /// stop. A cluster promoted from the one it followed ([`PROMOTED`])
    /// takes its follow process's writes again only from a copy begun
    /// since (`copy`, the offset of the copy's mark the write carries,
    /// [`carried_copy`]).
    pub(super) fn not_following(&self, carried: Option<i64>, copy: Option<i64>) -> Result<(), ApiError> {
        let marks = self.own_marks()?;
        let sender = super::following();
        match (followed(&marks.follows), sender.as_deref()) {
            (Some((source, _)), Some(s)) if s == source => Ok(()),
            (Some((source, _)), _) => Err(conflict(format!("this cluster follows {source}: it takes the writes of {source}'s follow process and none of its own; write to {source}, or stop following ({FOLLOW_STOP}) first"))),
            (None, Some(s)) => {
                if let Some(&at) = marks.promoted.get(s).filter(|&&p| !copy.is_some_and(|c| c > p)) {
                    return Err(conflict(format!(
                        "this cluster was promoted from {s} (at {}): it takes its own writes, and its follow process's no more; to follow {s} again, copy it afresh (chloedb-server follow --copy)",
                        chloedb::timestamp::format(at.saturating_mul(1000))
                    )));
                }
                match marks.follows.get(s) {
                    Some(&stopped) if stopped % 2 == 0 && !carried.is_some_and(|c| c > stopped) => {
                        Err(conflict(format!("this cluster stopped following {s} ({FOLLOW_STOP}): its follow process stops too; start it again to follow {s} again")))
                    }
                    _ => Ok(()),
                }
            }
            (None, None) => Ok(()),
        }
    }

    /// Counts a write of a [`HOLD`] in ([`Follows::holding`]) until the
    /// returned guard drops: meanwhile the shard takes no other write
    /// ([`Self::not_held`]).
    pub(super) fn hold_pending(&self) -> Holding<'_> {
        self.follows.holding.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Holding(&self.follows)
    }

    /// Refuses a write (503) while this shard holds its writes ([`HOLD`]),
    /// or a hold is under way. Checked once the write is counted in
    /// ([`super::moving::Moving::enter`]), which a hold waits for: so a
    /// write either commits before the hold's transaction or is refused.
    pub(super) fn not_held(&self) -> Result<(), ApiError> {
        let since = match self.follows.holding.load(std::sync::atomic::Ordering::SeqCst) {
            0 => self.own_marks()?.holds(),
            _ => Some(crate::telemetry::now_ms() as i64),
        };
        match since {
            None => Ok(()),
            Some(ms) => Err(ApiError {
                code: None, status: 503,
                message: format!(
                    "this cluster holds its writes (since {}, {HOLD_WRITES}): a switch-over to a cluster that follows it may be under way; write to the cluster that takes over, or again once the hold ends",
                    chloedb::timestamp::format(ms.saturating_mul(1000))
                ),
            }),
        }
    }

    /// Keeps what follow process `name` says of itself, for
    /// [`Self::follow_view`].
    pub fn note_follow_status(&self, name: &str, status: Json) {
        self.follows.note(name, status);
    }

    /// Per follow process that writes through this server and has not
    /// stopped following: how far behind its source it is, in seconds (as
    /// it last said, and once it is silent for over [`SILENT_MS`], plus
    /// how long since: it applies nothing), and in transactions.
    pub(crate) fn follow_lags(&self) -> Vec<(String, f64, f64)> {
        self.follows
            .said()
            .into_iter()
            .filter(|(_, (_, st))| st["state"] != "stopped")
            .map(|(name, (age_ms, st))| {
                let lag_ms = st["lag_ms"].as_u64().unwrap_or(0) + if age_ms > SILENT_MS { age_ms } else { 0 };
                (name, lag_ms as f64 / 1e3, st["behind"].as_f64().unwrap_or(0.0))
            })
            .collect()
    }

    /// The clusters this one follows or followed ([`crate::follow`]): per
    /// name, whether its shards follow it now (`following`: how many do,
    /// of `shards`), since when (`since_ms`, or `stopped_ms`), the copy
    /// it started from (`copy_ms`, `copied`), how far it applied each
    /// source shard's feed (`applied`, the least over this cluster's
    /// shards, with `held` when the source shard held its clients' writes
    /// as of that transaction: [`HOLD`]), whether every source shard did
    /// (`source_holds`), when it was promoted from it (`promoted_ms`, and
    /// `promoted_seqs`, each of its shards' transaction that ended
    /// following: [`PROMOTED`]), and what its follow process last said
    /// (`status`, with `status_age_ms`), which only the coordinator it
    /// writes through holds. `holding_ms`: since when this cluster holds
    /// its own clients' writes, if it does.
    pub async fn follow_view(self: &Arc<Self>) -> Result<Json, ApiError> {
        let shards = self.sources_by_shard().await?;
        let said = self.follows.said();
        let mut names: BTreeMap<String, ()> = said.keys().map(|n| (n.clone(), ())).collect();
        let own: Vec<Marks> = own_marks_of(&shards).collect();
        for m in &own {
            names.extend(m.follows.keys().map(|n| (n.clone(), ())));
            names.extend(m.promoted.keys().map(|n| (n.clone(), ())));
        }
        let mut clusters = Vec::new();
        for name in names.keys() {
            let following = own.iter().filter(|m| followed(&m.follows).is_some_and(|(n, _)| n == name)).count();
            let last = own.iter().filter_map(|m| m.follows.get(name)).max().copied();
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
            let mark = |rest: String| least.get(format!("{gen}{rest}").as_str()).copied();
            let applied: BTreeMap<usize, Json> = least
                .iter()
                .filter_map(|(rest, o)| {
                    let i = rest.strip_prefix(&gen)?.strip_suffix("/at")?.parse::<usize>().ok()?;
                    // Every transaction up to the hold applied.
                    let held = mark(format!("{i}/held")).is_some_and(|h| h > mark(format!("{i}/released")).unwrap_or(0) && *o >= h);
                    Some((i, json!({"shard": i, "seq": o, "term": mark(format!("{i}/term")).unwrap_or(0), "held": held})))
                })
                .collect();
            let source_holds = !applied.is_empty() && applied.values().all(|a| a["held"] == true);
            let mut j = json!({
                "name": name,
                "following": following,
                "shards": shards.len(),
                "copy_ms": copy,
                "copied": copy.is_some_and(|_| least.contains_key(format!("{gen}copied").as_str())),
                "applied": applied.into_values().collect::<Vec<_>>(),
                "source_holds": source_holds,
            });
            match last {
                Some(o) if o % 2 == 1 => j["since_ms"] = json!(o / 2),
                Some(o) => j["stopped_ms"] = json!(o / 2),
                None => {}
            }
            if let Some(p) = own.iter().filter_map(|m| m.promoted.get(name)).max() {
                j["promoted_ms"] = json!(p);
                if let Some(seqs) = promoted_seqs(&shards, name) {
                    j["promoted_seqs"] = json!(seqs);
                }
            }
            if let Some((age, st)) = said.get(name) {
                j["status"] = st.clone();
                j["status_age_ms"] = json!(age);
            }
            clusters.push(j);
        }
        let now = own.iter().find_map(|m| followed(&m.follows).map(|(n, _)| n.to_string()));
        let holding = own.iter().filter_map(Marks::holds).min();
        Ok(json!({"following": now, "holding_ms": holding, "clusters": clusters}))
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
            return Err(ApiError { code: None, status: 404, message: format!("this cluster never followed {name}") });
        };
        if c["following"] == 0 {
            return Ok(json!({"stopped": name, "already": true}));
        }
        let shards = self.sources_by_shard().await?;
        let at = next_offset(own_marks_of(&shards).filter_map(|m| m.follows.get(name).copied()).max(), false);
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

/// A hold of the ship log ([`chloedb::storage::Database::hold_log`]),
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

/// Sends `tx` every row of `tables` in `snap` as upserts (with `since`,
/// those a transaction after it last wrote), [`COPY_ROWS`] a line.
/// Returns the rows sent and the stream sources' offsets at `snap`, or
/// `None` once the reader left.
fn copy_rows(db: &Db, snap: &Snapshot, tables: &[Schema], since: u64, whole: &[String], tx: &tokio::sync::mpsc::Sender<Vec<u8>>) -> Result<Option<(u64, Vec<Json>)>, ApiError> {
    let every = Mask::new(|_, _| true);
    let mut sent = 0u64;
    for schema in tables {
        let Some(t) = snap.table(&schema.table) else { continue };
        let all = since == 0 || whole.iter().any(|w| w.eq_ignore_ascii_case(&schema.table));
        let rows = t.covered(&every, (!all).then_some(since))?;
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

/// What a copy feed sends ([`Service::copy_feed`]).
#[derive(Clone, Debug, Default)]
pub struct CopyOpts {
    /// Only the rows a transaction after this one last wrote (0: all).
    pub since: u64,
    /// But every row of these tables (a catch-up's tables that changed).
    pub whole: Vec<String>,
    /// The digests of its keys by slot, with its end.
    pub digests: bool,
}

/// The digest of a slot's keys: how many, and the sum of their hashes
/// ([`key_hash`]), which two clusters compare whatever their shards.
pub type Digest = (u64, u64);

/// The hash of a row's key as two clusters compare them: of its table's
/// name (in lower case: the clusters number their tables apart) and of
/// its key as slots hash it ([`slots::encode`]).
pub fn key_hash(table: &str, key: &chloedb::value::Value) -> u64 {
    let mut bytes = table.to_ascii_lowercase().into_bytes();
    bytes.push(0xff);
    slots::encode(key, |b| bytes.extend_from_slice(b));
    twox_hash::XxHash3_64::oneshot(&bytes)
}

/// The digests of the keys of `tables` in `snap` by slot (of the value
/// that places each row), for the slots that hold one.
pub fn key_digests(snap: &Snapshot, tables: &[String]) -> Result<BTreeMap<usize, Digest>, ApiError> {
    let every = Mask::new(|_, _| true);
    let mut out: BTreeMap<usize, Digest> = BTreeMap::new();
    for name in tables {
        let Some(t) = snap.table(name) else { continue };
        t.each_covered_key(&every, |key, place| {
            let d = out.entry(slots::slot(place)).or_default();
            d.0 += 1;
            d.1 = d.1.wrapping_add(key_hash(name, key));
        })?;
    }
    Ok(out)
}

/// The keys of `tables` in `snap` whose rows `slots` place, each as
/// `{"table", "key", "place"}` (`place`: the shard_by value of a table
/// placed by one).
pub fn slot_keys(snap: &Snapshot, tables: &[String], slots: &std::collections::HashSet<usize>) -> Result<Vec<Json>, ApiError> {
    let every = Mask::new(|_, _| true);
    let mut out = Vec::new();
    for name in tables {
        let Some(t) = snap.table(name) else { continue };
        let by = t.schema().shard_by.is_some();
        let table = t.schema().table.clone();
        t.each_covered_key(&every, |key, place| {
            if slots.contains(&slots::slot(place)) {
                let mut k = json!({"table": table, "key": js::to_json(key)});
                if by {
                    k["place"] = js::to_json(place);
                }
                out.push(k);
            }
        })?;
    }
    Ok(out)
}

/// Digests as JSON: `[[slot, count, "sum in hex"], ...]` (a sum may not
/// fit a JSON number).
pub fn digests_json(d: &BTreeMap<usize, Digest>) -> Json {
    Json::Array(d.iter().map(|(s, (n, h))| json!([s, n, format!("{h:016x}")])).collect())
}

/// [`digests_json`] read back.
pub fn digests_of(j: &Json) -> Result<BTreeMap<usize, Digest>, ApiError> {
    let bad = || ApiError::bad(format!("digests are [[slot, count, \"sum in hex\"], ...], not {j}"));
    let mut out = BTreeMap::new();
    for d in j.as_array().ok_or_else(bad)? {
        let (Some(s), Some(n), Some(h)) = (d[0].as_u64(), d[1].as_u64(), d[2].as_str().and_then(|h| u64::from_str_radix(h, 16).ok())) else { return Err(bad()) };
        out.insert(s as usize, (n, h));
    }
    Ok(out)
}

impl Service {
    /// This server's keys of `tables` as a follower's sweep compares them
    /// (protocol 15): with no `slots`, their digests by slot
    /// ([`key_digests`], `{"digests"}`); with `slots`, the keys those place
    /// (`{"keys"}`, [`slot_keys`]). Of the rows it serves: not those of
    /// slots a move sends it before they are its own.
    pub async fn feed_keys(self: &Arc<Self>, tables: Vec<String>, slots: Option<Vec<usize>>) -> Result<Json, ApiError> {
        let db = self.local().cloned().ok_or_else(|| ApiError::internal("a coordinator's keys are its shards'"))?;
        let (snap, epoch) = self.served(&db).await?;
        tokio::task::spawn_blocking(move || match slots {
            None => Ok(json!({"digests": digests_json(&key_digests(&snap, &tables)?), "epoch": epoch})),
            Some(s) => Ok(json!({"keys": slot_keys(&snap, &tables, &s.into_iter().collect())?, "epoch": epoch})),
        })
        .await
        .map_err(ApiError::internal)?
    }

    /// A version of `db` as this shard serves it, and the epoch of the
    /// slot map it serves (0 with none): the rows of slots it holds
    /// without serving them (a move's) are left out. A slot map's marks
    /// are written before the map is taken ([`Self::set_slots`]): taken
    /// while none is, the version has the marks of the epoch it serves.
    async fn served(&self, db: &Db) -> Result<(Snapshot, u64), ApiError> {
        let _taking = self.slots_file.lock().await;
        let held = self.slots.read().unwrap_or_else(|e| e.into_inner());
        let epoch = held.as_ref().map_or(0, |(m, _)| m.epoch);
        let snap = db.snapshot();
        let mask = self.moving.mask();
        drop(held);
        match mask {
            Some(m) => Ok((snap.masked(&m)?, epoch)),
            None => Ok((snap, epoch)),
        }
    }
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
    let mut hold = None;
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
                Some(_) if name == HOLD => hold = Some(offset % 2 == 1),
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
    // The shard holds its clients' writes from here on, or no longer.
    if let Some(h) = hold {
        j["hold"] = json!(h);
    }
    if t.unknown > 0 {
        j["unknown"] = json!(t.unknown);
    }
    j
}
