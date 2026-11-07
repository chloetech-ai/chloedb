//! A follower's order: in what order a cluster that follows another
//! applies the transactions of the other's shards' feeds
//! ([`crate::service::feed`]), with no I/O. [`crate::follow`] reads the
//! feeds and writes what [`Order::next`] says; the tests here run it over
//! seeded histories, read in every order and cut short by crashes.
//!
//! Each source shard's transactions apply in their order, but for changes
//! of tables, and two shards' rows are apart, so most interleave freely.
//! Two things are ordered across shards:
//!
//! * A change of a table or its rank profiles (`alter_table`,
//!   `drop_table`, `profile`), which every shard makes: it applies once,
//!   when every shard has reached it, so that each shard's rows of the
//!   table before it are written to the table as it was, and those after
//!   to the table as it became. It orders only the table's own: a shard's
//!   writes of other tables pass it, so changes of two tables that reach
//!   the shards in two orders each apply once.
//! * Slots that change hands: a shard's transactions after it marked
//!   slots `arrived` from another wait until that one's are applied up to
//!   its mark of their leaving (`left`, of the same id). A `left` for a
//!   shard not yet read starts reading it ([`Step::Read`]), and that
//!   shard's transactions before its first `arrived` are skipped: its
//!   tables (the follower has them) and a move's copies (it has the rows).
//!
//! Creating a table is ordered as a write of it: the follower makes it if
//! it lacks it, from whichever shard's feed has it first.
//!
//! As writes pass changes, a shard's transactions are not applied in the
//! order of their numbers, but each table's are. So the follower keeps,
//! per shard, the last transaction before which all are applied (its
//! offset, [`Order::at`]), and per shard and table the last applied
//! ([`Order::table_at`]), which its writes carry as the offsets of their
//! operations: one applied before is skipped. A step taken is applied: a
//! write that fails is tried again, and a follower that restarts makes a
//! new [`Order`] from what it kept ([`Order::resume`]).

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde_json::Value as Json;

/// One transaction of a source shard's feed, as the follower orders it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Txn {
    pub seq: u64,
    pub term: u64,
    /// Its operations, each naming its `table`: rows (and tables made),
    /// or with [`Txn::change`] a change of tables or rank profiles.
    pub ops: Vec<Json>,
    /// A change of tables or rank profiles every shard makes.
    pub change: bool,
    /// Stream sources' offsets it wrote.
    pub sources: Vec<Json>,
    /// Slots that left, as (the shard they left for, the id).
    pub left: Vec<(usize, String)>,
    /// Slots that arrived, as (the shard they came from, the id).
    pub arrived: Vec<(usize, String)>,
    /// Rows of a first copy at version [`Txn::seq`]: the shard's offset
    /// moves at its end ([`Txn::copied`]).
    pub copy: bool,
    /// The end of a first copy: from here on the shard is at its version.
    pub copied: bool,
    /// The versions of tables (or rank profiles) it makes or changes.
    pub versions: Vec<Version>,
    /// The shard holds its clients' writes from here on (`true`: every
    /// write of a client is in this transaction or before it), or no
    /// longer (`false`).
    pub hold: Option<bool>,
}

/// A table (or rank profile) a transaction makes or changes, by `key`:
/// the version it changes (`None` when it makes it), and the one it
/// leaves. A shard added after the follower started was made with its
/// tables as they were then, which may be ahead of the follower: it takes
/// part in their changes, and writes them, once the follower holds them
/// so ([`Order::join`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Version {
    pub key: String,
    pub was: Option<String>,
    pub now: String,
}

impl Txn {
    /// The tables its operations name.
    pub fn tables(&self) -> BTreeSet<&str> {
        self.ops.iter().filter_map(|o| o.get("table").and_then(Json::as_str)).collect()
    }

    /// Whether it is the same change of tables as `other`, on another shard.
    fn same_change(&self, other: &Txn) -> bool {
        self.change && other.change && self.ops == other.ops
    }
}

/// What the follower does next.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// Write these transactions, as (shard, transaction), in this order:
    /// each shard's in its own.
    Write(Vec<(usize, Txn)>),
    /// Apply this change of tables once, which is each listed shard's
    /// transaction (shard, seq, term).
    Change { ops: Vec<Json>, at: Vec<(usize, u64, u64)> },
    /// Start reading shard `i`'s feed, from its first transaction: a new
    /// shard slots left for.
    Read(usize),
    /// Nothing until more is read.
    Wait,
    /// The feeds cannot be ordered: why.
    Stuck(String),
}

/// A source shard as the follower reads it.
#[derive(Clone, Debug, Default)]
struct Shard {
    /// What was read and not yet taken.
    queue: VecDeque<Txn>,
    /// The last transaction before which all are taken (0 for none), and
    /// its term.
    seq: u64,
    term: u64,
    /// Per table, the last transaction of it taken.
    tables: BTreeMap<String, u64>,
    /// Transactions taken ahead of others (by seq, with the term): the
    /// shard passes them once those before are taken.
    ahead: BTreeMap<u64, u64>,
    /// A shard slots left for after the follower started, before its
    /// first `arrived`: its transactions are skipped until then.
    joining: bool,
    /// For such a shard, per table, how many of the table's changes the
    /// follower applies before the shard takes part in its changes and
    /// writes it: the shard was made with the table as they left it.
    joins: Option<BTreeMap<String, u64>>,
    /// For such a shard, whether its first transaction (that makes its
    /// tables) was read.
    made: bool,
}

impl Shard {
    /// Takes the transaction at position `p` of the queue.
    fn take(&mut self, p: usize) -> Txn {
        let t = self.queue.remove(p).expect("in the queue");
        for table in t.tables() {
            let at = self.tables.entry(table.to_string()).or_default();
            *at = (*at).max(t.seq);
        }
        if !t.copy {
            self.ahead.insert(t.seq, t.term);
        }
        self.settle();
        t
    }

    /// Passes the transactions taken that none before them is left of.
    fn settle(&mut self) {
        let front = self.queue.front().map(|t| t.seq);
        while let Some((&seq, &term)) = self.ahead.first_key_value() {
            if front.is_some_and(|f| seq > f) {
                break;
            }
            self.ahead.pop_first();
            if seq > self.seq {
                self.seq = seq;
                self.term = term;
            }
        }
    }

    /// Where in the queue the first transaction of `tables` is.
    fn first_of(&self, tables: &BTreeSet<&str>) -> Option<usize> {
        self.queue.iter().position(|t| !t.tables().is_disjoint(tables))
    }
}

/// The follower's version of a table (or rank profile): as the last
/// change it applied, or the table made, left it (`None` once a restart
/// lost it), and how many changes of it it applied.
#[derive(Clone, Debug, Default, PartialEq)]
struct Applied {
    now: Option<String>,
    changes: u64,
}

/// The order of the source shards' transactions ([`Step`]).
#[derive(Clone, Debug, Default)]
pub struct Order {
    shards: Vec<Shard>,
    /// The ids of the slots marks `left` taken.
    left: BTreeSet<String>,
    /// New shards to start reading.
    read: VecDeque<usize>,
    /// The most operations one [`Step::Write`] holds.
    batch: usize,
    /// Per table (or rank profile), the follower's version.
    applied: BTreeMap<String, Applied>,
}

/// The most operations of one write, by default.
pub const BATCH: usize = 1000;

impl Order {
    /// An order over `n` source shards, each at transaction 0.
    pub fn new(n: usize) -> Order {
        Order { shards: vec![Shard::default(); n], batch: BATCH, ..Default::default() }
    }

    /// An order that starts again from what a follower kept: each shard's
    /// offset (seq, term, [`Order::at`]); the `left` marks taken, each with
    /// its shard and transaction (one past a shard's offset is read again,
    /// so it does not count); and per shard and table the last
    /// transaction applied ([`Order::table_at`]), so that a transaction
    /// all of whose tables are past it is not taken again.
    pub fn resume(at: &[(u64, u64)], left: impl IntoIterator<Item = (usize, String, u64)>, tables: impl IntoIterator<Item = (usize, String, u64)>) -> Order {
        let mut o = Order::new(at.len());
        for (s, &(seq, term)) in o.shards.iter_mut().zip(at) {
            s.seq = seq;
            s.term = term;
        }
        o.left = left.into_iter().filter(|(i, _, seq)| at.get(*i).is_some_and(|a| *seq <= a.0)).map(|(_, id, _)| id).collect();
        for (i, table, seq) in tables {
            if let Some(s) = o.shards.get_mut(i) {
                let at = s.tables.entry(table).or_default();
                *at = (*at).max(seq);
            }
        }
        o
    }

    /// With the follower's tables (or rank profiles) by key: the version
    /// each holds as the source said it, if known, and how many changes of
    /// it were applied ([`Order::changes`]).
    pub fn with_versions(mut self, v: impl IntoIterator<Item = (String, Option<String>, u64)>) -> Order {
        for (key, now, changes) in v {
            self.applied.insert(key, Applied { now, changes });
        }
        self
    }

    /// Has shard `i` (adding the shards up to it) skip its transactions
    /// before its first `arrived`, as a shard slots left for after the
    /// follower started does, with what it joins at ([`Order::joins`]).
    pub fn join(&mut self, i: usize, joins: impl IntoIterator<Item = (String, u64)>) {
        if i >= self.shards.len() {
            self.shards.resize(i + 1, Shard::default());
        }
        self.shards[i].joining = true;
        self.shards[i].joins.get_or_insert_with(BTreeMap::new).extend(joins);
    }

    /// Has shard `i`, which joined, read as the others: it passed its
    /// first `arrived` ([`Order::join`]).
    pub fn unjoin(&mut self, i: usize) {
        if let Some(s) = self.shards.get_mut(i) {
            s.joining = false;
            s.made = true;
        }
    }

    /// With writes of at most `n` operations.
    pub fn with_batch(mut self, n: usize) -> Order {
        self.batch = n.max(1);
        self
    }

    /// The source shards it reads.
    pub fn shards(&self) -> usize {
        self.shards.len()
    }

    /// Shard `i`'s offset: the last transaction before which all are taken
    /// (seq, term).
    pub fn at(&self, i: usize) -> (u64, u64) {
        self.shards.get(i).map_or((0, 0), |s| (s.seq, s.term))
    }

    /// The last transaction of shard `i` that wrote `table`, or changed it.
    pub fn table_at(&self, i: usize, table: &str) -> u64 {
        self.shards.get(i).and_then(|s| s.tables.get(table)).copied().unwrap_or(0)
    }

    /// How many changes of table (or rank profile) `key` were applied.
    pub fn changes(&self, key: &str) -> u64 {
        self.applied.get(key).map_or(0, |a| a.changes)
    }

    /// For a shard added after the follower started, per table, how many
    /// of its changes are applied before the shard writes it ([`Order::join`]).
    pub fn joins(&self, i: usize) -> Option<&BTreeMap<String, u64>> {
        self.shards.get(i).and_then(|s| s.joins.as_ref())
    }

    /// The last transaction read of shard `i` (seq, term): its reader
    /// goes on from the next.
    pub fn read_to(&self, i: usize) -> (u64, u64) {
        let Some(s) = self.shards.get(i) else { return (0, 0) };
        match s.queue.iter().rev().find(|t| !t.copy) {
            Some(t) if t.seq > s.seq => (t.seq, t.term),
            _ => (s.seq, s.term),
        }
    }

    /// How many transactions of shard `i` wait to be taken.
    pub fn queued(&self, i: usize) -> usize {
        self.shards.get(i).map_or(0, |s| s.queue.len())
    }

    /// Whether shard `i` is new and skips until its first `arrived`.
    pub fn joining(&self, i: usize) -> bool {
        self.shards.get(i).is_some_and(|s| s.joining)
    }

    /// Adds transaction `t` read from shard `i`'s feed (a first copy's
    /// rows and its end included). One the shard passed is dropped (read
    /// again after a restart), as is one all of whose tables it passed.
    pub fn offer(&mut self, i: usize, t: Txn) {
        let Some(s) = self.shards.get_mut(i) else { return };
        let last = s.queue.back().map_or(s.seq, |b| b.seq);
        if !t.copy && !t.copied && t.seq <= last {
            return;
        }
        let tables = t.tables();
        if !t.copy && !s.joining && !tables.is_empty() && tables.iter().all(|n| s.tables.get(*n).is_some_and(|&a| a >= t.seq)) {
            s.ahead.insert(t.seq, t.term);
            s.settle();
            return;
        }
        s.queue.push_back(t);
    }

    /// What to do next.
    pub fn next(&mut self) -> Step {
        if let Some(i) = self.read.pop_front() {
            return Step::Read(i);
        }
        self.skip_joining();
        let batch = self.take_writes();
        if !batch.is_empty() {
            return Step::Write(batch);
        }
        self.change()
    }

    /// How many changes of `key` the follower applies before it holds
    /// version `v`: by the changes read and not yet applied. `None` until
    /// enough is read; with the follower's version lost (a restart), as
    /// many as it applied.
    fn changes_to(&self, key: &str, v: &str) -> Option<u64> {
        let a = self.applied.get(key).cloned().unwrap_or_default();
        let Some(mut cur) = a.now else { return Some(a.changes) };
        let next: BTreeMap<&str, &str> = self
            .shards
            .iter()
            .flat_map(|s| &s.queue)
            .filter(|t| t.change)
            .flat_map(|t| &t.versions)
            .filter(|x| x.key == key)
            .filter_map(|x| Some((x.was.as_deref()?, x.now.as_str())))
            .collect();
        let mut n = a.changes;
        while cur != v {
            cur = next.get(cur.as_str())?.to_string();
            n += 1;
            if n > a.changes + next.len() as u64 {
                return None;
            }
        }
        Some(n)
    }

    /// Drops what a joining shard holds before its first `arrived`, but
    /// notes what it was made with and changed since ([`Shard::joins`]).
    fn skip_joining(&mut self) {
        for i in 0..self.shards.len() {
            if !self.shards[i].joining {
                continue;
            }
            while let Some(t) = self.shards[i].queue.front() {
                if !t.arrived.is_empty() {
                    self.shards[i].joining = false;
                    self.shards[i].made = true;
                    break;
                }
                let mut joins = Vec::new();
                for x in &t.versions {
                    if t.change {
                        // A change it made as it joined: it passed it.
                        let had = self.shards[i].joins.as_ref().and_then(|j| j.get(&x.key)).copied();
                        joins.push((x.key.clone(), had.unwrap_or_else(|| self.changes(&x.key)) + 1));
                    } else {
                        match self.changes_to(&x.key, &x.now) {
                            Some(n) => joins.push((x.key.clone(), n)),
                            // Not read far enough to tell.
                            None => return,
                        }
                    }
                }
                let s = &mut self.shards[i];
                s.joins.get_or_insert_with(BTreeMap::new).extend(joins);
                s.made = true;
                let t = s.queue.pop_front().expect("a front");
                s.ahead.insert(t.seq, t.term);
                s.settle();
            }
        }
    }

    /// Whether shard `i` takes part in the next change of table `key`: a
    /// shard that joined, once the follower holds the version it was made
    /// with; one whose first transaction did not make the table holds it
    /// from before it numbered its writes, or made it since, and takes
    /// part in every change; `None` until it is known what it was made
    /// with.
    fn part_in(&self, i: usize, key: &str) -> Option<bool> {
        let s = &self.shards[i];
        let Some(joins) = &s.joins else { return Some(true) };
        match joins.get(key) {
            Some(&n) => Some(self.changes(key) >= n),
            None if s.made => Some(true),
            None => None,
        }
    }

    /// Whether shard `i` may write table `key`: one that joined, once the
    /// follower holds the version it was made with.
    fn joined(&self, i: usize, key: &str) -> bool {
        self.shards[i].joins.as_ref().and_then(|j| j.get(key)).is_none_or(|&n| self.changes(key) >= n)
    }

    /// Notes the versions taken transaction `t` leaves: a table made, or
    /// changed.
    fn applied(&mut self, t: &Txn) {
        for x in &t.versions {
            let a = self.applied.entry(x.key.clone()).or_default();
            a.now = Some(x.now.clone());
            if t.change {
                a.changes += 1;
            }
        }
    }

    /// The transactions that may be written now, up to the batch: each
    /// shard's in order, but that a transaction passes those before it of
    /// other tables (changes too); none passes a held `arrived`, and a
    /// `left` or the end of a copy passes no write.
    fn take_writes(&mut self) -> Vec<(usize, Txn)> {
        let mut out = Vec::new();
        let mut ops = 0;
        loop {
            let mut progress = false;
            for i in 0..self.shards.len() {
                if self.shards[i].joining {
                    continue;
                }
                let mut p = 0;
                let mut blocked: BTreeSet<String> = BTreeSet::new();
                let mut wrote_before = false;
                while ops < self.batch && p < self.shards[i].queue.len() {
                    let t = &self.shards[i].queue[p];
                    let tables = t.tables();
                    if t.change {
                        blocked.extend(tables.into_iter().map(String::from));
                        p += 1;
                        continue;
                    }
                    if !t.arrived.iter().all(|(_, id)| self.left.contains(id)) {
                        break;
                    }
                    let fenced = (!t.left.is_empty() || t.copied) && wrote_before;
                    if fenced || tables.iter().any(|n| blocked.contains(*n) || !self.joined(i, n)) {
                        wrote_before |= !tables.is_empty();
                        blocked.extend(tables.into_iter().map(String::from));
                        p += 1;
                        continue;
                    }
                    let t = self.shards[i].take(p);
                    self.applied(&t);
                    ops += t.ops.len().max(1);
                    for (to, id) in &t.left {
                        self.left.insert(id.clone());
                        // A shard the follower did not know of.
                        if *to >= self.shards.len() {
                            let from = self.shards.len();
                            for j in from..=*to {
                                self.join(j, []);
                            }
                            self.read.extend(from..=*to);
                        }
                    }
                    out.push((i, t));
                    progress = true;
                }
            }
            if !progress || ops >= self.batch {
                break;
            }
        }
        out
    }

    /// Why nothing can be taken, once every feed was read to its end and
    /// [`Order::next`] waits: the first transaction each shard holds.
    pub fn blocked(&self) -> Option<String> {
        let held: Vec<String> = self
            .shards
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.queue.front().map(|t| (i, t)))
            .map(|(i, t)| {
                let what = match () {
                    _ if t.change => format!("a change of table {}", t.tables().into_iter().collect::<Vec<_>>().join(", ")),
                    _ if !t.arrived.is_empty() => format!("slots arrived from shard {}", t.arrived[0].0),
                    _ => format!("writes of {}", t.tables().into_iter().collect::<Vec<_>>().join(", ")),
                };
                format!("shard {i} at transaction {} ({what})", t.seq)
            })
            .collect();
        (!held.is_empty()).then(|| format!("the shards' feeds cannot be put in one order (a table changed while slots moved?): {}", held.join("; ")))
    }

    /// A change of tables every shard that takes part has reached, if
    /// one has: it is the first transaction of its tables in each one's
    /// queue.
    fn change(&mut self) -> Step {
        let mut stuck = None;
        for h in 0..self.shards.len() {
            for want in self.shards[h].queue.iter().filter(|t| t.change) {
                let tables = want.tables();
                if self.shards[h].first_of(&tables).map(|p| &self.shards[h].queue[p]) != Some(want) {
                    continue;
                }
                // The shards that take part: all, but a shard that joined
                // takes part once the follower holds what it was made with.
                let takes: Vec<Option<bool>> = (0..self.shards.len()).map(|i| tables.iter().map(|n| self.part_in(i, n)).try_fold(true, |a, b| Some(a && b?))).collect();
                if takes.iter().any(Option::is_none) || takes[h] != Some(true) {
                    continue;
                }
                let part: Vec<usize> = (0..self.shards.len()).filter(|&i| takes[i] == Some(true)).collect();
                let mut at = Vec::with_capacity(part.len());
                for &i in &part {
                    let Some(p) = self.shards[i].first_of(&tables) else { break };
                    let t = &self.shards[i].queue[p];
                    if !t.change {
                        break;
                    }
                    if !t.same_change(want) {
                        stuck.get_or_insert_with(|| format!("shards {h} and {i} changed table {} in different orders (their transactions {} and {})", tables.iter().next().copied().unwrap_or("?"), want.seq, t.seq));
                        break;
                    }
                    at.push((i, p));
                }
                if at.len() < part.len() {
                    continue;
                }
                let want = want.clone();
                let pos = at.into_iter().map(|(i, p)| (i, self.shards[i].take(p))).map(|(i, t)| (i, t.seq, t.term)).collect();
                self.applied(&want);
                return Step::Change { ops: want.ops, at: pos };
            }
        }
        match stuck {
            Some(why) => Step::Stuck(why),
            None => Step::Wait,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::rng::Rng;
    use serde_json::json;
    use std::collections::BTreeMap;

    const TABLES: u64 = 3;

    /// A source cluster's history: per shard its transactions; the rows'
    /// last versions; how many changes of tables it made.
    struct History {
        /// The shards when the follower starts; later ones are added.
        start: usize,
        feeds: Vec<Vec<Txn>>,
        rows: BTreeMap<u64, u64>,
        changes: u64,
    }

    /// `n` shards, `steps` events: writes of keys a shard holds (each
    /// `{"k", "v", "t", "tv"}`: the key, the version it has after it
    /// counting every write of the key, its table, and how many changes of
    /// the table its shard had logged before it), changes of tables that
    /// every shard logs at a point of its own (two at once only of
    /// different tables), and moves of keys between shards (the copies,
    /// marked moved, then `left` and `arrived`, writes to them held
    /// between).
    fn history(rng: &mut Rng, n: usize, steps: usize) -> History {
        let keys = 64u64;
        let mut owner: Vec<usize> = (0..keys).map(|k| (k as usize) % n).collect();
        let mut feeds: Vec<Vec<Txn>> = vec![Vec::new(); n];
        let mut logged: Vec<BTreeMap<u64, u64>> = vec![BTreeMap::new(); n];
        let mut rows = BTreeMap::new();
        let (mut version, mut changes, mut moves) = (0u64, 0u64, 0u64);
        // Each table's version as its last change left it.
        let mut version_of = vec![0u64; TABLES as usize];
        let push = |feeds: &mut Vec<Vec<Txn>>, i: usize, mut t: Txn| {
            t.seq = feeds[i].len() as u64 + 1;
            t.term = 1 + t.seq / 50;
            feeds[i].push(t);
        };
        let mut pending: Vec<(Txn, u64, Vec<usize>)> = Vec::new();
        for _ in 0..steps {
            match rng.below(10) {
                0 => {
                    let table = rng.below(TABLES);
                    if pending.iter().any(|(_, t, _)| *t == table) {
                        continue;
                    }
                    changes += 1;
                    let v = version_of[table as usize];
                    version_of[table as usize] += 1;
                    let versions = vec![Version { key: format!("t{table}"), was: Some(format!("t{table}@{v}")), now: format!("t{table}@{}", v + 1) }];
                    let t = Txn { ops: vec![json!({"op": "alter_table", "table": format!("t{table}"), "n": changes})], change: true, versions, ..Default::default() };
                    pending.push((t, table, (0..feeds.len()).collect()));
                }
                1 if !pending.is_empty() => {
                    let c = rng.below(pending.len() as u64) as usize;
                    let w = rng.below(pending[c].2.len() as u64) as usize;
                    let i = pending[c].2.remove(w);
                    push(&mut feeds, i, pending[c].0.clone());
                    *logged[i].entry(pending[c].1).or_default() += 1;
                    if pending[c].2.is_empty() {
                        pending.remove(c);
                    }
                }
                2 if pending.is_empty() && (feeds.len() > 1 || rng.below(4) == 0) => {
                    let n = feeds.len();
                    let from = rng.below(n as u64) as usize;
                    let moving: Vec<u64> = (0..keys).filter(|&k| owner[k as usize] == from && rng.below(2) == 0).collect();
                    if moving.is_empty() {
                        continue;
                    }
                    // Now and then to a shard added (made with the tables
                    // as they are, so it logs nothing before the move).
                    let to = match n {
                        _ if n < 6 && rng.below(6) == 0 => {
                            // Its tables as the last changes left them
                            // (none pending), and only after the follower
                            // may have read far behind.
                            feeds.push(Vec::new());
                            logged.push(logged[0].clone());
                            let ops = (0..TABLES).map(|t| json!({"op": "create_table", "table": format!("t{t}")})).collect();
                            let versions = (0..TABLES).map(|t| Version { key: format!("t{t}"), was: None, now: format!("t{t}@{}", version_of[t as usize]) }).collect();
                            push(&mut feeds, n, Txn { ops, versions, ..Default::default() });
                            n
                        }
                        1 => continue,
                        _ => (from + 1 + rng.below(n as u64 - 1) as usize) % n,
                    };
                    moves += 1;
                    let id = format!("m{moves}");
                    push(&mut feeds, to, Txn::default());
                    push(&mut feeds, from, Txn { left: vec![(to, id.clone())], ..Default::default() });
                    push(&mut feeds, to, Txn { arrived: vec![(from, id)], ..Default::default() });
                    for k in moving {
                        owner[k as usize] = to;
                    }
                }
                _ => {
                    let k = rng.below(keys);
                    let i = owner[k as usize];
                    version += 1;
                    rows.insert(k, version);
                    let table = k % TABLES;
                    let tv = logged[i].get(&table).copied().unwrap_or(0);
                    let op = if rng.below(8) == 0 { "delete" } else { "upsert" };
                    push(&mut feeds, i, Txn { ops: vec![json!({"op": op, "table": format!("t{table}"), "k": k, "v": version, "t": table, "tv": tv})], ..Default::default() });
                }
            }
        }
        for (t, table, left) in pending {
            for i in left {
                push(&mut feeds, i, t.clone());
                *logged[i].entry(table).or_default() += 1;
            }
        }
        History { start: n, feeds, rows, changes }
    }

    /// What the follower holds, and what it kept with each step (its
    /// offsets and marks), from which it starts again after a crash.
    #[derive(Default)]
    struct Follower {
        rows: BTreeMap<u64, u64>,
        /// Per table, the changes applied, in order.
        changes: BTreeMap<u64, Vec<u64>>,
        kept: Vec<(u64, u64)>,
        left: Vec<(usize, String, u64)>,
        tables: Vec<(usize, String, u64)>,
    }

    impl Follower {
        fn apply(&mut self, step: &Step, order: &Order) -> Result<(), String> {
            match step {
                Step::Write(b) => {
                    for (i, t) in b {
                        for op in &t.ops {
                            let (k, v) = (op["k"].as_u64().unwrap(), op["v"].as_u64().unwrap());
                            let had = self.rows.get(&k).copied().unwrap_or(0);
                            if v <= had {
                                return Err(format!("shard {i} seq {}: key {k} version {v} after {had}", t.seq));
                            }
                            // Written to the table as its shard had it.
                            let (table, tv) = (op["t"].as_u64().unwrap(), op["tv"].as_u64().unwrap());
                            let applied = self.changes.get(&table).map_or(0, Vec::len) as u64;
                            if applied != tv {
                                return Err(format!("shard {i} seq {}: a row of t{table} written after {applied} of its changes, not {tv}", t.seq));
                            }
                            self.rows.insert(k, v);
                        }
                        self.left.extend(t.left.iter().map(|(_, id)| (*i, id.clone(), t.seq)));
                        self.tables.extend(t.tables().iter().map(|n| (*i, n.to_string(), t.seq)));
                    }
                }
                Step::Change { ops, at } => {
                    let op = &ops[0];
                    let (table, n) = (op["table"].as_str().unwrap()[1..].parse::<u64>().unwrap(), op["n"].as_u64().unwrap());
                    let done = self.changes.entry(table).or_default();
                    if done.last().is_some_and(|&l| l >= n) {
                        return Err(format!("change {n} of t{table} after {done:?}"));
                    }
                    done.push(n);
                    self.tables.extend(at.iter().map(|&(i, seq, _)| (i, format!("t{table}"), seq)));
                }
                _ => {}
            }
            self.kept = (0..order.shards()).map(|i| order.at(i)).collect();
            Ok(())
        }
    }

    /// Runs a follower over a seeded history, reading the feeds a little
    /// at a time in a random order, with crashes that start it again from
    /// what it kept, and checks each step and the end.
    fn follow(seed: u64, n: usize, steps: usize) {
        let mut rng = Rng::new(seed);
        let h = history(&mut rng, n, steps);
        let trace = std::env::var("FOLLOW_TRACE").is_ok_and(|v| v == seed.to_string());
        if trace {
            for (i, f) in h.feeds.iter().enumerate() {
                for t in f {
                    eprintln!("feed {i}: {} change {} ops {:?} left {:?} arrived {:?}", t.seq, t.change, t.ops, t.left, t.arrived);
                }
            }
        }
        let all = h.feeds.len();
        let mut f = Follower { kept: vec![(0, 0); n], ..Default::default() };
        let mut order = Order::new(h.start).with_batch(1 + rng.below(6) as usize).with_versions((0..TABLES).map(|t| (format!("t{t}"), Some(format!("t{t}@0")), 0)));
        let mut read: Vec<usize> = vec![0; all];
        let mut crashes = 0;
        let mut rounds = 0;
        loop {
            rounds += 1;
            if rounds >= 1_000_000 {
                let heads: Vec<String> = order.shards.iter().map(|s| format!("seq {} joining {} queue {:?}", s.seq, s.joining, s.queue.iter().take(6).map(|t| (t.seq, t.change, t.ops.first().map(|o| o.to_string()), t.left.clone(), t.arrived.clone())).collect::<Vec<_>>())).collect();
                panic!("seed {seed}: no end: read {read:?} of {:?}; {heads:#?}", h.feeds.iter().map(Vec::len).collect::<Vec<_>>());
            }
            let i = rng.below(all as u64) as usize;
            for _ in 0..=rng.below(3) {
                if i < order.shards() && read[i] < h.feeds[i].len() {
                    order.offer(i, h.feeds[i][read[i]].clone());
                    read[i] += 1;
                }
            }
            let step = order.next();
            if trace && step != Step::Wait {
                let short = match &step {
                    Step::Write(b) => format!("write {:?}", b.iter().map(|(i, t)| (*i, t.seq)).collect::<Vec<_>>()),
                    s => format!("{s:?}"),
                };
                eprintln!("step {short}; at {:?}", (0..order.shards()).map(|i| order.at(i)).collect::<Vec<_>>());
            }
            match step {
                Step::Read(j) => read[j] = 0,
                Step::Stuck(why) => panic!("seed {seed}: stuck: {why}"),
                Step::Wait => {
                    if order.shards() == all && (0..all).all(|i| read[i] == h.feeds[i].len() && order.queued(i) == 0) {
                        break;
                    }
                }
                step => f.apply(&step, &order).unwrap_or_else(|e| panic!("seed {seed}: {e}")),
            }
            if rng.below(300) == 0 && crashes < 6 {
                crashes += 1;
                if trace {
                    eprintln!("crash: kept {:?} tables {:?}", f.kept, f.tables);
                }
                // Kept with the writes: the shards that joined, with what
                // they join at, and each table's changes; the versions are
                // read from the follower's tables.
                let joined: Vec<(usize, bool, Vec<(String, u64)>)> = (h.start..order.shards()).map(|i| (i, order.joining(i), order.joins(i).map(|j| j.iter().map(|(k, v)| (k.clone(), *v)).collect()).unwrap_or_default())).collect();
                let versions: Vec<(String, Option<String>, u64)> = (0..TABLES)
                    .map(|t| {
                        let n = f.changes.get(&t).map_or(0, Vec::len) as u64;
                        (format!("t{t}"), Some(format!("t{t}@{n}")), n)
                    })
                    .collect();
                order = Order::resume(&f.kept, f.left.clone(), f.tables.clone()).with_batch(1 + rng.below(6) as usize).with_versions(versions);
                for (i, joining, joins) in joined {
                    order.join(i, joins);
                    if !joining {
                        order.unjoin(i);
                    }
                }
                for (i, r) in read.iter_mut().enumerate() {
                    *r = f.kept.get(i).map_or(0, |k| k.0 as usize);
                }
            }
        }
        assert_eq!(f.rows, h.rows, "seed {seed}: the rows' last versions");
        assert_eq!(f.changes.values().map(Vec::len).sum::<usize>() as u64, h.changes, "seed {seed}: every change once");
    }

    #[test]
    fn follows_seeded_histories_through_crashes() {
        let seeds = std::env::var("FOLLOW_SEEDS").ok().and_then(|s| s.parse().ok()).unwrap_or(400);
        for seed in 0..seeds {
            follow(seed, 1 + (seed % 4) as usize, 40 + (seed % 7) as usize * 40);
        }
    }

    #[test]
    fn a_shard_after_arrived_waits_for_the_other_to_pass_left() {
        let mut o = Order::new(2);
        o.offer(1, Txn { seq: 1, arrived: vec![(0, "x".into())], ..Default::default() });
        o.offer(1, Txn { seq: 2, ops: vec![json!({"table": "a", "k": 1})], ..Default::default() });
        o.offer(0, Txn { seq: 1, ops: vec![json!({"table": "a", "k": 1})], ..Default::default() });
        let Step::Write(b) = o.next() else { panic!() };
        assert_eq!(b.iter().map(|(i, t)| (*i, t.seq)).collect::<Vec<_>>(), vec![(0, 1)]);
        assert_eq!(o.next(), Step::Wait);
        o.offer(0, Txn { seq: 2, left: vec![(1, "x".into())], ..Default::default() });
        let Step::Write(b) = o.next() else { panic!() };
        assert_eq!(b.iter().map(|(i, t)| (*i, t.seq)).collect::<Vec<_>>(), vec![(0, 2), (1, 1), (1, 2)]);
    }

    #[test]
    fn a_change_of_tables_applies_once_every_shard_has_it() {
        let ch = |seq| Txn { seq, ops: vec![json!({"op": "drop_table", "table": "a"})], change: true, ..Default::default() };
        let mut o = Order::new(2);
        o.offer(0, ch(1));
        o.offer(0, Txn { seq: 2, ops: vec![json!({"table": "a", "k": 1})], ..Default::default() });
        assert_eq!(o.next(), Step::Wait);
        o.offer(1, Txn { seq: 1, ops: vec![json!({"table": "b", "k": 2})], ..Default::default() });
        let Step::Write(b) = o.next() else { panic!() };
        assert_eq!(b.len(), 1);
        o.offer(1, ch(2));
        assert_eq!(o.next(), Step::Change { ops: vec![json!({"op": "drop_table", "table": "a"})], at: vec![(0, 1, 0), (1, 2, 0)] });
        let Step::Write(b) = o.next() else { panic!() };
        assert_eq!(b.iter().map(|(i, t)| (*i, t.seq)).collect::<Vec<_>>(), vec![(0, 2)]);
    }

    #[test]
    fn changes_of_two_tables_in_two_orders_apply_each_once() {
        let ch = |seq, t: &str| Txn { seq, ops: vec![json!({"op": "drop_table", "table": t})], change: true, ..Default::default() };
        let mut o = Order::new(2);
        o.offer(0, ch(1, "a"));
        o.offer(0, ch(2, "b"));
        o.offer(1, ch(1, "b"));
        o.offer(1, ch(2, "a"));
        let mut tables = Vec::new();
        while let Step::Change { ops, .. } = o.next() {
            tables.push(ops[0]["table"].as_str().unwrap().to_string());
        }
        assert_eq!(tables, ["a", "b"]);
        assert_eq!((o.at(0), o.at(1)), ((2, 0), (2, 0)));
        // The same table changed in two orders cannot be ordered.
        let mut o = Order::new(2);
        o.offer(0, Txn { seq: 1, ops: vec![json!({"op": "alter_table", "table": "a", "n": 1})], change: true, ..Default::default() });
        o.offer(1, Txn { seq: 1, ops: vec![json!({"op": "alter_table", "table": "a", "n": 2})], change: true, ..Default::default() });
        assert!(matches!(o.next(), Step::Stuck(_)));
    }

    #[test]
    fn a_new_shard_is_read_and_skips_to_its_first_arrived() {
        let mut o = Order::new(1);
        o.offer(0, Txn { seq: 1, left: vec![(1, "y".into())], ..Default::default() });
        assert!(matches!(o.next(), Step::Write(_)));
        assert_eq!(o.next(), Step::Read(1));
        assert!(o.joining(1));
        o.offer(1, Txn { seq: 1, ops: vec![json!({"op": "create_table", "table": "a"})], ..Default::default() });
        o.offer(1, Txn { seq: 2, ..Default::default() });
        o.offer(1, Txn { seq: 3, arrived: vec![(0, "y".into())], ..Default::default() });
        o.offer(1, Txn { seq: 4, ops: vec![json!({"table": "a", "k": 1})], ..Default::default() });
        let Step::Write(b) = o.next() else { panic!() };
        assert_eq!(b.iter().map(|(i, t)| (*i, t.seq)).collect::<Vec<_>>(), vec![(1, 3), (1, 4)]);
        assert!(!o.joining(1));
    }
}
