//! Group commit: threads that write at once share one transaction and one
//! commit ([`Db::write_ops`], and SQL writes).
//!
//! A thread hands its write to a [`Queue`] and waits. When no thread
//! leads, it leads: it takes the writes waiting, its own first among them,
//! in the order they came, as many as one group may hold (`take`), and
//! applies them (`apply`), which commits them in one transaction or
//! answers each alone. Then it hands the lead to the oldest write still
//! waiting, whose thread leads the next group, and returns its own answer.
//! No thread serves others for more than one group, and a write waits for
//! at most the group ahead of its own.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use super::{Db, PreparedWrite};
use crate::error::{Error, Result};
use crate::query::agg::Part;
use crate::query::logical::Pred;
use crate::query::Write;
use crate::schema::FieldType;
use crate::storage::WriteTxn;
use crate::value::{Document, Value};

/// Operations one shared transaction takes at most (an INSERT counts its
/// rows, an UPDATE or DELETE by key its keys), as chloedb-server's writer
/// groups them.
pub const GROUP_OPS: usize = 4096;

/// One operation of [`Db::write_ops`], on the table it names, as
/// chloedb-server's `/v1/write` takes them.
#[derive(Clone, Debug, PartialEq)]
pub enum WriteOp {
    /// Inserts a document; no live row may have its key.
    Insert { table: String, doc: Document },
    /// Inserts a document, or replaces the row with its key.
    Upsert { table: String, doc: Document },
    /// Deletes the row with this key; it writes nothing if none is live.
    Delete { table: String, key: Value },
    /// Sets a single-valued numeric field of the row with this key, in
    /// place ([`crate::storage::WriteTxn::set`]).
    Set { table: String, key: Value, field: String, value: Value },
    /// Adds `delta` to a numeric field of the row with this key, in place
    /// ([`crate::storage::WriteTxn::add`]).
    Add { table: String, key: Value, field: String, delta: Value },
    /// [`WriteOp::Add`] to the row with the document's key, or inserts the
    /// document when no live row has it
    /// ([`crate::storage::WriteTxn::add_or_insert`]).
    AddOrInsert { table: String, doc: Document, field: String, delta: Value },
}

impl WriteOp {
    /// The table it writes.
    pub fn table(&self) -> &str {
        match self {
            WriteOp::Insert { table, .. } | WriteOp::Upsert { table, .. } | WriteOp::Delete { table, .. } | WriteOp::Set { table, .. } | WriteOp::Add { table, .. } | WriteOp::AddOrInsert { table, .. } => table,
        }
    }

    /// Applies it in transaction `t`; whether it wrote a row. It sees the
    /// rows `t` wrote before it, as every operation of a transaction does.
    pub fn apply(&self, t: &mut WriteTxn<'_>) -> Result<bool> {
        let mut tt = t.table(self.table())?;
        match self {
            WriteOp::Insert { doc, .. } => tt.insert(doc.clone()).map(|_| true),
            WriteOp::Upsert { doc, .. } => tt.upsert(doc.clone()).map(|_| true),
            WriteOp::Delete { key, .. } => tt.delete(key),
            WriteOp::Set { key, field, value, .. } => tt.set(key, field, value.clone()),
            WriteOp::Add { key, field, delta, .. } => tt.add(key, field, delta),
            WriteOp::AddOrInsert { doc, field, delta, .. } => tt.add_or_insert(doc.clone(), field, delta).map(|_| true),
        }
    }
}

/// What the writes of a [`Db`] did since it opened ([`Db::write_stats`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WriteStats {
    /// Writes applied through the grouping writer: calls of
    /// [`Db::write_ops`], SQL writes ([`Db::sql`], [`Db::run`],
    /// [`Db::write_prepared`]).
    pub writes: u64,
    /// Groups they were applied in, each in one transaction (or, after it
    /// failed, each of its writes alone): as many as `writes` when every
    /// write committed alone.
    pub groups: u64,
}

/// The rows writes read and change, by table, to tell which SQL writes may
/// share one transaction ([`Db::write_prepared_group`]).
///
/// An UPDATE or DELETE finds its rows in the last committed snapshot,
/// which does not show its transaction's own changes, and an UPDATE sets
/// values it computed from what it read: two statements of a transaction
/// that touch the same row would lose one's change. So statements share
/// a transaction when their tables differ, or when each names the keys of
/// the rows it may touch and no key comes in two. An INSERT names its
/// rows' keys; an UPDATE or DELETE names them when its filter pins the
/// primary key (`key = v`, `key IN (...)`, an `OR` of those, or an `AND`
/// with one). Any other statement has its table to itself.
#[derive(Default)]
pub struct Footprints {
    /// The keys written so far by table; `None`: any row.
    tables: HashMap<String, Option<HashSet<Part>>>,
}

impl Footprints {
    /// Adds write `p` if it may share a transaction with the writes added
    /// before; whether it was added. The first one always is.
    pub fn admit(&mut self, p: &PreparedWrite) -> bool {
        let keys = p.keys_touched();
        match self.tables.get_mut(&p.schema.table) {
            None => {
                self.tables.insert(p.schema.table.clone(), keys.map(|k| k.into_iter().collect()));
                true
            }
            Some(Some(have)) => match keys {
                Some(keys) if !keys.iter().any(|k| have.contains(k)) => {
                    have.extend(keys);
                    true
                }
                _ => false,
            },
            Some(None) => false,
        }
    }
}

impl PreparedWrite {
    /// The keys of the rows it may read or write, when it names them
    /// ([`Footprints`]): an INSERT's (none in a table without a primary
    /// key), or those an UPDATE or DELETE's filter pins. `None`: any row.
    fn keys_touched(&self) -> Option<Vec<Part>> {
        let Some(pk) = self.schema.primary_key else {
            return matches!(self.write, Write::Insert { .. }).then(Vec::new);
        };
        let keys = match &self.write {
            Write::Insert { docs, .. } => return Some(docs.iter().map(|d| Part::of(&d[pk])).collect()),
            Write::Update { select, .. } | Write::Delete { select } => pinned(&select.filter, pk)?,
        };
        // A value of another type than the key's could match it as text
        // ('7' for 7): no footprint then.
        let typed = match self.schema.fields[pk].ty {
            FieldType::Keyword => keys.iter().all(|k| matches!(k, Value::Str(_))),
            FieldType::Int => keys.iter().all(|k| matches!(k, Value::Int(_))),
            _ => false,
        };
        typed.then(|| keys.iter().map(Part::of).collect())
    }

    /// How much of a group it takes ([`GROUP_OPS`]): an INSERT its rows,
    /// an UPDATE or DELETE by key its keys, any other 1.
    pub fn weight(&self) -> usize {
        match &self.write {
            Write::Insert { docs, .. } => docs.len().max(1),
            Write::Update { select, .. } | Write::Delete { select } => self.schema.primary_key.and_then(|pk| pinned(&select.filter, pk)).map_or(1, |k| k.len().max(1)),
        }
    }
}

/// The values of primary key `pk` that filter `p` holds for at most: rows
/// with any other key fail it. `None` when it can hold for any key.
fn pinned(p: &Pred, pk: usize) -> Option<Vec<Value>> {
    match p {
        Pred::In { field, values } if *field == pk => Some(values.clone()),
        Pred::IntRange { field, lo, hi } if *field == pk && lo == hi => Some(vec![Value::Int(*lo)]),
        Pred::False => Some(Vec::new()),
        Pred::Or(ps) => ps.iter().try_fold(Vec::new(), |mut all, p| {
            all.extend(pinned(p, pk)?);
            Some(all)
        }),
        // The fewest keys of any conjunct that pins them.
        Pred::And(ps) => ps.iter().filter_map(|p| pinned(p, pk)).min_by_key(Vec::len),
        _ => None,
    }
}

/// A write waiting in [`Db`]'s queue.
pub(super) enum Job {
    Ops(Vec<WriteOp>),
    Sql(PreparedWrite),
}

impl Job {
    fn weight(&self) -> usize {
        match self {
            Job::Ops(ops) => ops.len(),
            Job::Sql(p) => p.weight(),
        }
    }
}

/// [`Db`]'s grouping writer.
#[derive(Default)]
pub(super) struct Writes {
    queue: Queue<Job>,
    writes: AtomicU64,
    groups: AtomicU64,
}

impl Db {
    /// Applies write operations in one transaction, all or none, and
    /// returns the rows they wrote (a delete, set or add of a key no row
    /// has writes none). A delete, set or add sees the rows the operations
    /// before it wrote.
    ///
    /// Threads that write at once share commits: while one group of writes
    /// commits, those that come wait, and the next group takes them all
    /// (up to [`GROUP_OPS`] operations) in one transaction, one commit and
    /// one disk flush. Each still gets its own answer: when a group fails,
    /// each write of it runs again alone, so that only one at fault fails.
    /// SQL writes ([`Db::sql`]) share commits with each other the same way
    /// when they touch different rows ([`Footprints`]).
    pub fn write_ops(&self, ops: Vec<WriteOp>) -> Result<u64> {
        Ok(self.write_ops_seq(ops)?.0)
    }

    /// [`Db::write_ops`], also returning the sequence number of the
    /// transaction that committed them in a database that numbers its
    /// writes.
    pub fn write_ops_seq(&self, ops: Vec<WriteOp>) -> Result<(u64, Option<u64>)> {
        if ops.is_empty() {
            return Ok((0, None));
        }
        self.write_queued(Job::Ops(ops))
    }

    /// What the grouping writer has done since the database opened.
    pub fn write_stats(&self) -> WriteStats {
        WriteStats { writes: self.writes.writes.load(Ordering::Relaxed), groups: self.writes.groups.load(Ordering::Relaxed) }
    }

    /// Applies `job` in a group with the writes other threads hand in
    /// meanwhile.
    pub(super) fn write_queued(&self, job: Job) -> Answer {
        self.writes.queue.submit(job, take, |group| self.apply_group(group))
    }

    fn apply_group(&self, group: &[Arc<Slot<Job>>]) -> Vec<Answer> {
        self.writes.groups.fetch_add(1, Ordering::Relaxed);
        self.writes.writes.fetch_add(group.len() as u64, Ordering::Relaxed);
        let mut t = match self.storage.begin() {
            Ok(t) => t,
            // Nothing was written: each write gets the error.
            Err(e) => return group.iter().map(|_| Err(again(&e))).collect(),
        };
        let mut rows = Vec::with_capacity(group.len());
        let mut failed = None;
        for s in group {
            match self.apply_job(&mut t, &s.job) {
                Ok(n) => rows.push(n),
                Err(e) => {
                    failed = Some(e);
                    break;
                }
            }
        }
        let e = match failed {
            None => match t.commit_seq() {
                Ok(seq) => return rows.into_iter().map(|n| Ok((n, seq))).collect(),
                Err(e) => e,
            },
            // Dropped, it lets go of the writer lock and writes nothing.
            Some(e) => {
                drop(t);
                e
            }
        };
        if group.len() == 1 {
            return vec![Err(e)];
        }
        // A failed transaction wrote nothing (Strata reports a durable
        // commit as Ok): each write runs again alone, and only those at
        // fault fail.
        group.iter().map(|s| self.apply_alone(&s.job)).collect()
    }

    fn apply_alone(&self, job: &Job) -> Answer {
        let mut t = self.storage.begin()?;
        let n = self.apply_job(&mut t, job)?;
        Ok((n, t.commit_seq()?))
    }

    fn apply_job(&self, t: &mut WriteTxn<'_>, job: &Job) -> Result<u64> {
        match job {
            Job::Ops(ops) => {
                let mut n = 0;
                for op in ops {
                    n += op.apply(t)? as u64;
                }
                Ok(n)
            }
            Job::Sql(p) => self.write_prepared_in(t, p),
        }
    }
}

/// One group from the front of the writes waiting: the first, and those
/// after it that may share its transaction, up to [`GROUP_OPS`]
/// operations. Operations see the transaction's own writes and SQL writes
/// do not, so the two never share one.
fn take(waiting: &mut VecDeque<Arc<Slot<Job>>>) -> Vec<Arc<Slot<Job>>> {
    let first = waiting.pop_front().expect("the leader's own write waits");
    let mut n = first.job.weight();
    let mut prints = Footprints::default();
    let sql = match &first.job {
        Job::Sql(p) => prints.admit(p),
        Job::Ops(_) => false,
    };
    let mut group = vec![first];
    while let Some(next) = waiting.front() {
        let w = next.job.weight();
        let fits = n + w <= GROUP_OPS
            && match &next.job {
                Job::Ops(_) => !sql,
                Job::Sql(p) => sql && prints.admit(p),
            };
        if !fits {
            break;
        }
        n += w;
        group.push(waiting.pop_front().expect("in front"));
    }
    group
}

/// The same error again, for each write of a group that failed with it.
fn again(e: &Error) -> Error {
    match e {
        Error::Io(io) => Error::Io(std::io::Error::new(io.kind(), io.to_string())),
        Error::Corrupt(m) => Error::Corrupt(m.clone()),
        Error::Schema(m) => Error::Schema(m.clone()),
        Error::Query(m) => Error::Query(m.clone()),
        Error::Busy(m) => Error::Busy(m.clone()),
        Error::Timeout(m) => Error::Timeout(m.clone()),
        Error::Memory(m) => Error::Memory(m.clone()),
        Error::Unavailable(m) => Error::Unavailable(m.clone()),
    }
}

/// What a write answers: the rows it wrote, and the sequence number of
/// the transaction that committed it, in a database that numbers its
/// writes.
pub(crate) type Answer = Result<(u64, Option<u64>)>;

/// Writes waiting for their group.
pub(crate) struct Queue<J> {
    state: Mutex<Waiting<J>>,
}

struct Waiting<J> {
    writes: VecDeque<Arc<Slot<J>>>,
    /// A thread leads a group, or was handed the lead and is waking.
    leading: bool,
}

/// A write in the queue, and its answer once its group is applied.
pub(crate) struct Slot<J> {
    pub(crate) job: J,
    out: Mutex<Out>,
    cv: Condvar,
}

enum Out {
    Waiting,
    /// This write's thread leads the next group.
    Lead,
    Done(Answer),
}

impl<J> Slot<J> {
    fn finish(&self, out: Out) {
        *lock(&self.out) = out;
        self.cv.notify_one();
    }
}

impl<J> Default for Queue<J> {
    fn default() -> Queue<J> {
        Queue { state: Mutex::new(Waiting { writes: VecDeque::new(), leading: false }) }
    }
}

impl<J> Queue<J> {
    /// Applies `job` with the writes other threads hand in meanwhile, and
    /// answers it. `take` removes one group from the front of the writes
    /// waiting: at least the first, then those that may share its
    /// transaction. `apply` answers each write of a group, in its order.
    pub(crate) fn submit(&self, job: J, take: impl Fn(&mut VecDeque<Arc<Slot<J>>>) -> Vec<Arc<Slot<J>>>, apply: impl Fn(&[Arc<Slot<J>>]) -> Vec<Answer>) -> Answer {
        let me = Arc::new(Slot { job, out: Mutex::new(Out::Waiting), cv: Condvar::new() });
        let lead = {
            let mut w = lock(&self.state);
            w.writes.push_back(me.clone());
            !std::mem::replace(&mut w.leading, true)
        };
        if !lead {
            let mut out = lock(&me.out);
            loop {
                match std::mem::replace(&mut *out, Out::Waiting) {
                    Out::Done(r) => return r,
                    Out::Lead => break,
                    Out::Waiting => out = me.cv.wait(out).unwrap_or_else(|e| e.into_inner()),
                }
            }
        }
        // This write is the oldest waiting: a thread leads when it found no
        // leader (the queue was empty) or when the lead was handed to it.
        let group = take(&mut lock(&self.state).writes);
        debug_assert!(group.first().is_some_and(|s| Arc::ptr_eq(s, &me)), "a leader's own write leads its group");
        let mut lead = Leading { queue: self, group, me: &me };
        let answers = apply(&lead.group);
        assert_eq!(answers.len(), lead.group.len(), "an answer for each write of the group");
        let mut mine = None;
        for (s, a) in std::mem::take(&mut lead.group).into_iter().zip(answers) {
            match Arc::ptr_eq(&s, &me) {
                true => mine = Some(a),
                false => s.finish(Out::Done(a)),
            }
        }
        drop(lead);
        mine.expect("the leader's own write is in its group")
    }
}

/// The lead of a group: handed on when dropped, and if `apply` panicked,
/// the writes of the group still unanswered are answered with an error.
struct Leading<'a, J> {
    queue: &'a Queue<J>,
    group: Vec<Arc<Slot<J>>>,
    me: &'a Arc<Slot<J>>,
}

impl<J> Drop for Leading<'_, J> {
    fn drop(&mut self) {
        for s in self.group.drain(..).filter(|s| !Arc::ptr_eq(s, self.me)) {
            s.finish(Out::Done(Err(Error::Io(std::io::Error::other("a write applied in the same group failed unexpectedly (a panic); nothing of this write is known to be committed")))));
        }
        let mut w = lock(&self.queue.state);
        match w.writes.front() {
            Some(next) => next.finish(Out::Lead),
            None => w.leading = false,
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn take_all(w: &mut VecDeque<Arc<Slot<u64>>>) -> Vec<Arc<Slot<u64>>> {
        w.drain(..).collect()
    }

    #[test]
    fn threads_share_groups_and_get_their_own_answers() {
        let q = Queue::<u64>::default();
        let groups = AtomicUsize::new(0);
        let apply = |g: &[Arc<Slot<u64>>]| {
            groups.fetch_add(1, Ordering::Relaxed);
            std::thread::sleep(std::time::Duration::from_millis(2));
            g.iter().map(|s| Ok((s.job * 10, Some(g.len() as u64)))).collect()
        };
        std::thread::scope(|sc| {
            for t in 0..8u64 {
                let (q, apply) = (&q, &apply);
                sc.spawn(move || {
                    for i in 0..50 {
                        let job = t * 1000 + i;
                        assert_eq!(q.submit(job, take_all, apply).unwrap().0, job * 10);
                    }
                });
            }
        });
        let g = groups.load(Ordering::Relaxed);
        assert!(g < 400, "{g} groups for 400 writes: some shared one");
        assert!(lock(&q.state).writes.is_empty() && !lock(&q.state).leading);
    }

    #[test]
    fn a_panic_in_a_group_answers_its_writes_and_hands_the_lead_on() {
        let q = Queue::<u64>::default();
        let apply = |g: &[Arc<Slot<u64>>]| -> Vec<Answer> {
            if g.iter().any(|s| s.job == 7) {
                panic!("planted");
            }
            g.iter().map(|_| Ok((1, None))).collect()
        };
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| q.submit(7, take_all, apply)));
        assert!(r.is_err());
        // The queue still serves.
        assert_eq!(q.submit(1, take_all, apply).unwrap().0, 1);
    }

    #[test]
    fn a_failed_group_runs_each_write_alone() {
        let dir = tempfile::tempdir().unwrap();
        let schema = crate::Schema::from_json(r#"{"table": "t", "key": "id", "fields": [{"name": "id", "type": "keyword"}]}"#).unwrap();
        let db = Db::create(&dir.path().join("g.strata"), schema, crate::storage::DbOptions::default()).unwrap();
        let ins = |id: &str| Job::Ops(vec![WriteOp::Insert { table: "t".into(), doc: vec![Value::Str(id.into())] }]);
        let slot = |job| Arc::new(Slot { job, out: Mutex::new(Out::Waiting), cv: Condvar::new() });
        let answers = db.apply_group(&[slot(ins("a")), slot(ins("a")), slot(ins("b"))]);
        assert!(answers[0].is_ok() && answers[1].is_err() && answers[2].is_ok(), "{answers:?}");
        let answers = db.apply_group(&[slot(ins("c")), slot(ins("d"))]);
        assert_eq!(answers.iter().map(|a| a.as_ref().unwrap().0).collect::<Vec<_>>(), [1, 1]);
        assert_eq!(db.write_stats(), WriteStats { writes: 5, groups: 2 });
        let n = db.query("SELECT count(*) FROM t", &crate::query::Params::new()).unwrap().rows[0][0].as_i64();
        assert_eq!(n, Some(4));
    }

    #[test]
    fn take_keeps_operations_and_sql_apart_and_overlapping_sql_too() {
        let dir = tempfile::tempdir().unwrap();
        let schema = crate::Schema::from_json(r#"{"table": "t", "key": "id", "fields": [{"name": "id", "type": "keyword"}, {"name": "n", "type": "int"}]}"#).unwrap();
        let db = Db::create(&dir.path().join("k.strata"), schema, crate::storage::DbOptions::default()).unwrap();
        let sql = |s: &str| Job::Sql(db.prepare_write(s, &crate::query::Params::new()).unwrap().unwrap());
        let op = || Job::Ops(vec![WriteOp::Delete { table: "t".into(), key: Value::Str("x".into()) }]);
        let slot = |job| Arc::new(Slot { job, out: Mutex::new(Out::Waiting), cv: Condvar::new() });
        let mut waiting: VecDeque<_> = [
            slot(op()),
            slot(op()),
            slot(sql("UPDATE t SET n = n + 1 WHERE id = 'a'")),
            slot(sql("UPDATE t SET n = n + 1 WHERE id = 'b'")),
            slot(sql("UPDATE t SET n = n + 1 WHERE id = 'a'")),
            slot(op()),
        ]
        .into_iter()
        .collect();
        let sizes: Vec<usize> = std::iter::from_fn(|| (!waiting.is_empty()).then(|| take(&mut waiting).len())).collect();
        assert_eq!(sizes, [2, 2, 1, 1]);
    }
}
