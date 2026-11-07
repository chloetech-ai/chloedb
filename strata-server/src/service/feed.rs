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
//! log keeps them) and rank profiles (`profile`). A transaction may also
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
use strata::schema::Schema;
use strata::storage::feed::{Change, Changes, Read, Start};

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

impl Service {
    /// This server's feed from transaction `from` on, in chunks of lines
    /// (each ending with a newline). `after` is the term of transaction
    /// `from - 1` as the reader holds it, if it holds one.
    pub async fn feed(self: &Arc<Self>, from: u64, after: Option<u64>) -> Result<tokio::sync::mpsc::Receiver<Vec<u8>>, ApiError> {
        let db = self.local().cloned().ok_or_else(|| ApiError::bad("a cluster's feed is each shard's: name one (?shard=0 for the first)"))?;
        if db.storage().numbering().is_none() {
            return Err(conflict("this server does not number its writes, so it has no feed: start it with --seq-no (a shard with replicas numbers them)".into()));
        }
        self.feed_reaches()?;
        let d = db.clone();
        let start = tokio::task::spawn_blocking(move || d.storage().changes_from(from, after)).await.map_err(ApiError::internal)??;
        let (tx, rx) = tokio::sync::mpsc::channel(4);
        let mut cursor = match start {
            Start::Gone { oldest } => {
                let _ = tx.send(lines(&[json!({"gone": {"oldest": oldest}})])).await;
                return Ok(rx);
            }
            Start::Diverged { seq, term } => {
                let _ = tx.send(lines(&[json!({"diverged": {"seq": seq, "term": term}})])).await;
                return Ok(rx);
            }
            Start::At(c) => c,
        };
        let n = db.storage().numbering().unwrap_or_default();
        let _ = tx.send(lines(&[json!({"feed": {"from": cursor.next(), "last": n.seq, "term": n.term}})])).await;
        let svc = self.clone();
        tokio::spawn(async move {
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
        });
        Ok(rx)
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
            Change::Drop { was } => ops.push(json!({"op": "drop_table", "table": was.table})),
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
