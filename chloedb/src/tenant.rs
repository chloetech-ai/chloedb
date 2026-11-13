//! A database kept in an object store: a **tenant**.
//!
//! **Experimental** (layout 0). The design it is the first slice of is
//! `docs/design/0.7.0-file-per-agent.md`, which is not accepted yet: the
//! objects written here may change without a migration until it is.
//!
//! A tenant is one database under a prefix of an object store
//! ([`crate::storage::ObjectStore`]: a directory, or S3 and the stores
//! that speak it with feature `s3`). The store holds all of it; a node
//! works on a local copy that it can lose at any time.
//!
//! ```text
//! <tenant>/HEAD                                    the one object that is replaced
//! <tenant>/<incarnation>/base/<seq>-<epoch>.chloedb   the database up to seq, as one file
//! <tenant>/<incarnation>/log/<first>-<epoch>.arc      transactions after it (STRARC01)
//! ```
//!
//! `HEAD` ([`Head`]) names the base and the log objects that follow it,
//! and carries the newest transactions itself (the tail). **A write is
//! committed by replacing `HEAD` on the condition that it is still the
//! version this node last read** ([`ObjectStore::put_if_version`]). The
//! store is the only arbiter: a node that was partitioned, paused or
//! wrongly routed holds an old version, and its next write is refused,
//! whatever its clock says. There is no lease.
//!
//! Every other object is written once, under a name no other writer can
//! choose: names carry the tenant's *incarnation* (a random id made when
//! it is created, so that a tenant deleted and created again shares no
//! name with the old one) and the writer's *epoch* (a counter in `HEAD`
//! that a node raises, by a commit of its own, before it writes; it is
//! the term the database numbers that node's transactions under).
//!
//! ```no_run
//! # use std::sync::Arc;
//! # use chloedb::{query::Params, storage::DirStore, tenant::{Tenant, TenantOptions}};
//! let store = Arc::new(DirStore::new("/mnt/tenants")?);
//! let t = Tenant::create(store.clone(), "agent-7", TenantOptions::new("/var/cache/agent-7"), |db| {
//!     db.sql("CREATE TABLE notes (id INT PRIMARY KEY, body TEXT)", &Params::new()).map(|_| ())
//! })?;
//! t.write(|db| db.sql("INSERT INTO notes VALUES (1, 'kept in the store')", &Params::new()))?;
//! // Elsewhere, later, with an empty cache:
//! let t = Tenant::open(store, "agent-7", TenantOptions::new("/var/cache/other"))?;
//! t.read(|db| db.sql("SELECT body FROM notes WHERE id = 1", &Params::new()))?;
//! # Ok::<(), chloedb::tenant::TenantError>(())
//! ```
//!
//! What this slice does not do, and the design does: bases spread over
//! segment objects (a base here is always one file, fetched whole), reads
//! by range without a local copy, group commit (a write here is one
//! conditional put, and writes of one handle wait for each other), the
//! log objects fetched together with the base, and the directory of
//! tenants. Bulk loads and attached segments skip the database's log
//! ([`crate::storage::Database::start_seq_no`]), so they are **not**
//! committed by [`Tenant::write`]: write rows with SQL or transactions.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, RwLock, RwLockWriteGuard};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::db::Db;
use crate::error::{corrupt, Error, Result};
use crate::storage::arc::{Entry, LogObject, SEQ_NO};
use crate::storage::{Applied, CheckpointOptions, DbOptions, LogRead, ObjectStore, SeqNo};

const MAGIC: &[u8; 8] = b"STRHEAD1";
/// The layout this module writes: 0, which is experimental.
pub const LAYOUT: u32 = 0;
/// The object that names a tenant's database.
const HEAD: &str = "HEAD";
/// The most replaced bases `HEAD` names ([`TenantOptions::keep`]).
const KEPT_MAX: usize = 16;

/// How a node holds a tenant.
#[derive(Clone)]
pub struct TenantOptions {
    /// A directory of this handle's own for its local copy: the base as
    /// fetched, and the database it works on. Two handles, in one process
    /// or two, must not share one. What it holds is only a cache.
    pub cache_dir: PathBuf,
    /// The bytes of transactions `HEAD` carries before they are moved to a
    /// log object of their own.
    pub tail_max: usize,
    /// A new base is made once the transactions after the last one take
    /// this many bytes,
    pub base_after_bytes: u64,
    /// or are this many.
    pub base_after_txns: u64,
    /// How long a base and its logs stay in the store after a newer base
    /// replaces them, for readers still opening the older one; the 16
    /// newest at most.
    pub keep: Duration,
    /// The local database's options. [`TenantOptions::new`] turns `sync`
    /// off (the store is what makes a write durable, not the local file)
    /// and keeps a ship log of 16 MiB, which bounds one write's size at
    /// four times that ([`crate::storage::ship::HELD`]).
    pub db: DbOptions,
}

impl TenantOptions {
    pub fn new(cache_dir: impl Into<PathBuf>) -> TenantOptions {
        TenantOptions {
            cache_dir: cache_dir.into(),
            tail_max: 256 << 10,
            base_after_bytes: 4 << 20,
            base_after_txns: 1000,
            keep: Duration::from_secs(24 * 3600),
            db: DbOptions { sync: false, ship_log_bytes: 16 << 20, ..DbOptions::default() },
        }
    }
}

/// Why an operation on a tenant failed.
#[derive(Debug)]
pub enum TenantError {
    /// [`Tenant::create`]: a tenant has that id.
    Exists,
    /// No tenant has that id (any more).
    Missing,
    /// Another node committed since this one last read `HEAD`: nothing of
    /// the write was committed. The handle has loaded the tenant again;
    /// the write can be tried again.
    NotWriter,
    /// The store did not say whether it took the write, and reading `HEAD`
    /// again did not settle it: it may or may not be committed.
    InDoubt(String),
    /// Anything else; a write that fails with it was not committed.
    Db(Error),
}

impl fmt::Display for TenantError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TenantError::Exists => write!(f, "a tenant with this id exists"),
            TenantError::Missing => write!(f, "no tenant has this id"),
            TenantError::NotWriter => write!(f, "another node wrote the tenant first: nothing was committed, try again"),
            TenantError::InDoubt(m) => write!(f, "the write may or may not be committed: {m}"),
            TenantError::Db(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for TenantError {}

impl From<Error> for TenantError {
    fn from(e: Error) -> Self {
        TenantError::Db(e)
    }
}

impl From<std::io::Error> for TenantError {
    fn from(e: std::io::Error) -> Self {
        TenantError::Db(e.into())
    }
}

impl From<TenantError> for Error {
    fn from(e: TenantError) -> Self {
        match e {
            TenantError::Db(e) => e,
            TenantError::Exists => Error::Schema(e.to_string()),
            TenantError::Missing => Error::Io(std::io::Error::new(std::io::ErrorKind::NotFound, e.to_string())),
            TenantError::NotWriter => Error::Busy(e.to_string()),
            TenantError::InDoubt(_) => Error::Unavailable(e.to_string()),
        }
    }
}

type TResult<T> = std::result::Result<T, TenantError>;

/// The base `HEAD` names: a whole database as one file, holding every
/// transaction up to `seq`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaseRef {
    /// Its key under the tenant's prefix.
    pub key: String,
    pub seq: u64,
    /// The term of transaction `seq` (0 when `seq` is 0).
    pub term: u64,
}

/// A log object `HEAD` names: the transactions `first` to `last`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogRef {
    pub key: String,
    pub first: u64,
    pub last: u64,
    /// The bytes of its transactions.
    pub bytes: u64,
}

/// A base and its logs that a newer base replaced, kept until `until_ms`
/// (milliseconds since 1970) for readers that are still opening them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kept {
    pub base: String,
    pub logs: Vec<String>,
    pub until_ms: u64,
}

/// A tenant's `HEAD` object: where its database is.
///
/// ```text
/// "STRHEAD1" | header_len: u32 | header: JSON (this struct, without the tail)
///            | tail_len: u32 | tail: a log object (STRARC01) | crc32 of all before it: u32
/// ```
///
/// Integers are little-endian. A reader refuses a layout or a feature it
/// does not know.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Head {
    pub layout: u32,
    pub tenant: String,
    /// 32 hex digits, made when the tenant was created.
    pub incarnation: String,
    pub features: Vec<String>,
    /// Raised by each node that starts writing; the term of its
    /// transactions, and part of the name of every object it uploads.
    pub epoch: u64,
    /// The handle that holds `epoch`: a random name it gives itself each
    /// time it loads the tenant.
    pub writer: String,
    /// One more with every `HEAD`, so that its bytes never repeat (a store
    /// may make an object's version from its bytes).
    pub commit: u64,
    pub base: BaseRef,
    /// The log objects after the base, in order.
    pub logs: Vec<LogRef>,
    /// The tenant's last transaction.
    pub last_seq: u64,
    pub kept: Vec<Kept>,
    /// The transactions after the last log object, up to `last_seq`.
    #[serde(skip)]
    pub tail: Vec<Entry>,
}

impl Head {
    fn stream(&self) -> Result<[u8; 16]> {
        let bad = || corrupt(format!("HEAD: incarnation {:?} is not 32 hex digits", self.incarnation));
        if self.incarnation.len() != 32 || !self.incarnation.is_ascii() {
            return Err(bad());
        }
        let mut out = [0u8; 16];
        for (i, b) in out.iter_mut().enumerate() {
            *b = u8::from_str_radix(&self.incarnation[2 * i..2 * i + 2], 16).map_err(|_| bad())?;
        }
        Ok(out)
    }

    /// The bytes of the transactions after the base.
    fn bytes_since_base(&self) -> u64 {
        self.logs.iter().map(|l| l.bytes).sum::<u64>() + self.tail.iter().map(|e| e.bytes.len() as u64).sum::<u64>()
    }

    /// The keys it names, under the tenant's prefix.
    fn names(&self) -> HashSet<&str> {
        let kept = self.kept.iter().flat_map(|k| std::iter::once(k.base.as_str()).chain(k.logs.iter().map(String::as_str)));
        std::iter::once(self.base.key.as_str()).chain(self.logs.iter().map(|l| l.key.as_str())).chain(kept).collect()
    }

    /// Whether what it says agrees with itself.
    fn check(&self) -> Result<()> {
        let bad = |what: String| Err(corrupt(format!("HEAD of tenant {:?}: {what}", self.tenant)));
        let mut at = self.base.seq;
        for l in &self.logs {
            if l.first != at + 1 || l.last < l.first {
                return bad(format!("log object {} ({} to {}) does not follow transaction {at}", l.key, l.first, l.last));
            }
            at = l.last;
        }
        match (self.tail.first(), self.tail.last()) {
            (Some(first), Some(last)) if first.seq.seq != at + 1 || last.seq.seq != self.last_seq => {
                return bad(format!("its tail ({} to {}) does not lead from transaction {at} to {}", first.seq.seq, last.seq.seq, self.last_seq));
            }
            (None, _) if at != self.last_seq => return bad(format!("its logs end at transaction {at}, not at {}", self.last_seq)),
            _ => {}
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        self.check()?;
        let json = serde_json::to_vec(self).map_err(|e| Error::Schema(format!("HEAD: {e}")))?;
        let tail = LogObject { stream: self.stream()?, db_id: 0, generation: 0, entries: self.tail.clone() }.encode()?;
        if json.len() > u32::MAX as usize || tail.len() > u32::MAX as usize {
            return Err(Error::Schema("HEAD: too large".into()));
        }
        let mut b = Vec::with_capacity(8 + 4 + json.len() + 4 + tail.len() + 4);
        b.extend_from_slice(MAGIC);
        b.extend_from_slice(&(json.len() as u32).to_le_bytes());
        b.extend_from_slice(&json);
        b.extend_from_slice(&(tail.len() as u32).to_le_bytes());
        b.extend_from_slice(&tail);
        let crc = crc32fast::hash(&b);
        b.extend_from_slice(&crc.to_le_bytes());
        Ok(b)
    }

    pub fn decode(b: &[u8]) -> Result<Head> {
        let bad = |what: &str| corrupt(format!("HEAD: {what}"));
        if b.len() < 8 + 4 + 4 + 4 || &b[..8] != MAGIC {
            return Err(bad("not a tenant's HEAD (STRHEAD1), or cut short"));
        }
        let (body, crc) = b.split_at(b.len() - 4);
        if crc32fast::hash(body) != u32::from_le_bytes(crc.try_into().expect("4 bytes")) {
            return Err(bad("its checksum does not match"));
        }
        let len_at = |at: usize| body.get(at..at + 4).map(|l| u32::from_le_bytes(l.try_into().expect("4 bytes")) as usize);
        let json_len = len_at(8).ok_or_else(|| bad("cut short"))?;
        let json = body.get(12..12 + json_len).ok_or_else(|| bad("its header runs past its end"))?;
        let tail_len = len_at(12 + json_len).ok_or_else(|| bad("cut short"))?;
        let tail = body.get(16 + json_len..).filter(|t| t.len() == tail_len).ok_or_else(|| bad("its tail is not as long as it says"))?;
        // The layout and the features first: a header of a later layout
        // may not read as this one's.
        #[derive(Deserialize)]
        struct Kind {
            layout: u32,
            #[serde(default)]
            features: Vec<String>,
        }
        let kind: Kind = serde_json::from_slice(json).map_err(|e| bad(&format!("its header does not read: {e}")))?;
        if kind.layout != LAYOUT {
            return Err(Error::Schema(format!("tenant of layout {}: this version of ChloeDB reads layout {LAYOUT}", kind.layout)));
        }
        if let Some(f) = kind.features.iter().find(|f| *f != SEQ_NO) {
            return Err(Error::Schema(format!("tenant with feature {f:?}, which this version of ChloeDB does not know")));
        }
        let mut head: Head = serde_json::from_slice(json).map_err(|e| bad(&format!("its header does not read: {e}")))?;
        let tail = LogObject::decode(tail)?;
        if tail.stream != head.stream()? {
            return Err(bad("its tail is another tenant's"));
        }
        head.tail = tail.entries;
        head.check()?;
        Ok(head)
    }
}

/// Whether `id` can name a tenant: 1 to 128 of `A-Z a-z 0-9 . _ -`, not
/// starting with `.` or `_` (names the layout keeps for itself).
pub fn valid_id(id: &str) -> bool {
    (1..=128).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')) && !id.starts_with(['.', '_'])
}

fn check_id(id: &str) -> Result<()> {
    match valid_id(id) {
        true => Ok(()),
        false => Err(Error::Schema(format!("tenant id {id:?}: 1 to 128 of A-Z a-z 0-9 . _ -, not starting with . or _"))),
    }
}

/// Checks that `store` does what tenants need of it, by doing it to an
/// object of its own under `_check/`: creates it twice and expects the
/// second refused, replaces it at its version and then at a stale one,
/// which must be refused, and reads right after each write. An error
/// says which it failed. A store's documentation is not taken on trust.
pub fn check_store(store: &dyn ObjectStore) -> Result<()> {
    let key = format!("_check/{:032x}", rand::random::<u128>());
    let fail = |what: &str| Error::Schema(format!("{}: {what}; tenants need a store that writes under conditions and reads its last write", store.describe()));
    let reads = |bytes: &[u8], version: &str| match store.get(&key)? {
        Some((b, v)) if b == bytes && v == version => Ok(()),
        _ => Err(fail("a read did not see the last write")),
    };
    let run = || {
        let v1 = store.put_if_absent(&key, b"one")?.ok_or_else(|| fail("a new object was refused as if it existed"))?;
        if store.put_if_absent(&key, b"two")?.is_some() {
            return Err(fail("it created an object that existed"));
        }
        reads(b"one", &v1)?;
        let v2 = store.put_if_version(&key, b"three", &v1)?.ok_or_else(|| fail("it refused to replace an object at its own version"))?;
        if v2 == v1 {
            return Err(fail("two writes of one object have one version"));
        }
        if store.put_if_version(&key, b"four", &v1)?.is_some() {
            return Err(fail("it replaced an object at a stale version"));
        }
        reads(b"three", &v2)?;
        if !store.list("_check/")?.contains(&key) {
            return Err(fail("its listing lacks an object just written"));
        }
        store.delete(&key)?;
        if store.put_if_version(&key, b"five", &v2)?.is_some() {
            return Err(fail("it replaced an object that was deleted"));
        }
        match store.get(&key)? {
            None => Ok(()),
            Some(_) => Err(fail("a deleted object is still read")),
        }
    };
    let checked = run();
    let _ = store.delete(&key);
    checked
}

/// The ids of `store`'s tenants, in order: one listing of its root, and
/// a short read of each `HEAD`, which is what makes a tenant exist. (A
/// fleet keeps a directory rather than ask this of a large store.)
pub fn list(store: &dyn ObjectStore) -> Result<Vec<String>> {
    let mut ids = Vec::new();
    for id in store.list_dirs("")?.into_iter().filter(|id| valid_id(id)) {
        match store.get_range(&format!("{id}/{HEAD}"), 0..MAGIC.len() as u64) {
            Ok(_) => ids.push(id),
            Err(e) if not_found(&e) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(ids)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// The epoch in an object's name (`…/<number>-<epoch>.<ext>`).
fn epoch_of(key: &str) -> Option<u64> {
    let name = key.rsplit('/').next()?;
    let stem = name.split('.').next()?;
    let (number, epoch) = stem.split_once('-')?;
    (number.len() == 20 && epoch.len() == 10).then(|| epoch.parse().ok()).flatten()
}

fn not_found(e: &Error) -> bool {
    matches!(e, Error::Io(e) if e.kind() == std::io::ErrorKind::NotFound)
}

/// A tenant as this handle holds it: its local copy, and the `HEAD` the
/// copy is at.
struct Loaded {
    /// Shared with the writers of a commit while they take their turns
    /// ([`Tenant::write`]), and with nobody otherwise.
    db: Arc<Db>,
    head: Head,
    /// The store's version of `head`, which the next commit replaces.
    version: String,
    /// This load's name. The handle is the tenant's writer while `HEAD`
    /// names it, which no `HEAD` does when it is made: a handle takes a
    /// new epoch after every load, so that no two of its lives, one of
    /// which may have left a put on its way, upload under one name.
    me: String,
}

impl Loaded {
    fn is_writer(&self) -> bool {
        self.head.writer == self.me
    }

    fn last_seq(&self) -> Result<u64> {
        Ok(numbering(&self.db)?.last().seq)
    }
}

fn numbering(db: &Db) -> Result<crate::storage::Numbering> {
    db.storage().numbering().ok_or_else(|| corrupt("a tenant's database does not number its writes"))
}

/// A tenant, open on this node. See [the module](self).
pub struct Tenant {
    store: Arc<dyn ObjectStore>,
    id: String,
    opts: TenantOptions,
    /// `None`: to be loaded from the store before the next use (a write
    /// failed after it changed the local copy, and loading failed too).
    state: RwLock<Option<Loaded>>,
    /// The lock on the cache directory, held while the handle lives.
    _cache: std::fs::File,
    /// The writers waiting to commit, and whose turn it is.
    group: Mutex<Group>,
    turn: Condvar,
}

/// The writes of one handle that wait to be committed, and those being
/// committed together ([`Tenant::write`]).
#[derive(Default)]
struct Group {
    /// The next writer's ticket.
    next: u64,
    /// The writers no commit has taken yet, in the order they came.
    waiting: VecDeque<u64>,
    /// Whether one of them is committing (the leader).
    leading: bool,
    /// The writer whose statements run now, and the database they run on.
    turn: Option<(u64, Arc<Db>)>,
    /// The writer whose statements have just run,
    ran: Option<u64>,
    /// and whether they ended in a panic.
    panicked: bool,
    /// What became of the commits of writers that did not lead them.
    done: HashMap<u64, Outcome>,
}

/// What became of a commit, for the writers in it that did not lead it.
#[derive(Clone, Debug)]
enum Outcome {
    Committed,
    NotWriter,
    Missing,
    InDoubt(String),
    Failed(String),
}

impl Outcome {
    fn of(r: &TResult<()>) -> Outcome {
        match r {
            Ok(()) => Outcome::Committed,
            Err(TenantError::NotWriter) => Outcome::NotWriter,
            Err(TenantError::Missing) => Outcome::Missing,
            Err(TenantError::InDoubt(m)) => Outcome::InDoubt(m.clone()),
            Err(e) => Outcome::Failed(e.to_string()),
        }
    }

    fn result(self) -> TResult<()> {
        match self {
            Outcome::Committed => Ok(()),
            Outcome::NotWriter => Err(TenantError::NotWriter),
            Outcome::Missing => Err(TenantError::Missing),
            Outcome::InDoubt(m) => Err(TenantError::InDoubt(m)),
            Outcome::Failed(m) => Err(TenantError::Db(Error::Io(std::io::Error::other(m)))),
        }
    }
}

/// A writer's turn: says that its statements have run when dropped,
/// by a panic too, so that the commit's leader does not wait for ever.
struct Turn<'a> {
    tenant: &'a Tenant,
    ticket: u64,
    returned: bool,
}

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        let mut q = self.tenant.queue();
        q.ran = Some(self.ticket);
        q.panicked |= !self.returned;
        self.tenant.turn.notify_all();
    }
}

/// A commit's leader: when it is dropped, by a panic too, the writers
/// it took learn what became of the commit, or wait for the next one
/// when their statements have not run, and another writer may lead.
struct Lead<'a> {
    tenant: &'a Tenant,
    /// The writers of this commit after the leader, in order, and how
    /// many of them have run.
    members: Vec<u64>,
    ran: usize,
    /// What the writers that ran are told: a failure until the commit's
    /// end is known.
    outcome: Outcome,
}

impl Drop for Lead<'_> {
    fn drop(&mut self) {
        let mut q = self.tenant.queue();
        for (i, m) in self.members.iter().enumerate().rev() {
            match i < self.ran {
                true => drop(q.done.insert(*m, self.outcome.clone())),
                false => q.waiting.push_front(*m),
            }
        }
        q.turn = None;
        q.leading = false;
        self.tenant.turn.notify_all();
    }
}

impl fmt::Debug for Tenant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tenant").field("id", &self.id).field("store", &self.store.describe()).finish()
    }
}

impl Tenant {
    /// A handle with nothing loaded, holding its cache directory: an
    /// error when another handle, of any process, holds it.
    fn handle(store: Arc<dyn ObjectStore>, id: &str, opts: TenantOptions) -> Result<Tenant> {
        std::fs::create_dir_all(&opts.cache_dir)?;
        let lock = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(opts.cache_dir.join("lock"))?;
        // SAFETY: `lock` is an open file, whose descriptor the call only locks.
        if unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(&lock), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(Error::Busy(format!("{}: another handle of a tenant uses this cache directory", opts.cache_dir.display())));
        }
        Ok(Tenant { store, id: id.to_string(), opts, state: RwLock::new(None), _cache: lock, group: Mutex::default(), turn: Condvar::new() })
    }

    /// Creates tenant `id` in `store`, which must have none of that id
    /// ([`TenantError::Exists`]): an empty database on which `init` has
    /// run (its tables, say), uploaded as the tenant's first base, then
    /// `HEAD` put on the condition that there is none. When that put
    /// fails without an answer, the tenant may exist all the same.
    pub fn create(store: Arc<dyn ObjectStore>, id: &str, opts: TenantOptions, init: impl FnOnce(&Db) -> Result<()>) -> TResult<Tenant> {
        check_id(id)?;
        let t = Tenant::handle(store, id, opts)?;
        if t.store.get(&t.key(HEAD))?.is_some() {
            return Err(TenantError::Exists);
        }
        let incarnation = format!("{:032x}", rand::random::<u128>());
        let me = format!("{:032x}", rand::random::<u128>());
        let work = t.fresh_work()?;
        let db = Db::create_empty(&work, t.opts.db.clone())?;
        db.storage().start_seq_no()?;
        init(&db)?;
        let SeqNo { term, seq } = numbering(&db)?.last();
        let base = BaseRef { key: format!("{incarnation}/base/{seq:020}-{:010}.chloedb", 1), seq, term };
        let image = t.cached(&base.key)?;
        let _ = std::fs::remove_file(&image);
        db.storage().checkpoint(&image, &CheckpointOptions::default())?;
        t.store.put_file(&t.key(&base.key), &image)?;
        let head = Head { layout: LAYOUT, tenant: t.id.clone(), incarnation, features: vec![SEQ_NO.to_string()], epoch: 1, writer: me.clone(), commit: 1, base, logs: Vec::new(), last_seq: seq, kept: Vec::new(), tail: Vec::new() };
        match t.store.put_if_absent(&t.key(HEAD), &head.encode()?) {
            Ok(Some(version)) => {
                t.keep_cached(&head.base.key);
                t.mark_work(&head)?;
                *t.state.write().unwrap_or_else(|e| e.into_inner()) = Some(Loaded { db: Arc::new(db), head, version, me });
                Ok(t)
            }
            Ok(None) => {
                // Lost to another creator: the base is under an
                // incarnation no HEAD will ever name.
                let _ = t.store.delete(&t.key(&head.base.key));
                Err(TenantError::Exists)
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Opens tenant `id` of `store`: reads `HEAD`, fetches the base it
    /// names and applies the transactions after it. Opening writes
    /// nothing to the store.
    pub fn open(store: Arc<dyn ObjectStore>, id: &str, opts: TenantOptions) -> TResult<Tenant> {
        check_id(id)?;
        let t = Tenant::handle(store, id, opts)?;
        let loaded = t.load()?;
        *t.state.write().unwrap_or_else(|e| e.into_inner()) = Some(loaded);
        Ok(t)
    }

    /// Whether `store` has tenant `id`.
    pub fn exists(store: &dyn ObjectStore, id: &str) -> TResult<bool> {
        check_id(id)?;
        Ok(store.get(&format!("{id}/{HEAD}"))?.is_some())
    }

    /// Deletes tenant `id` of `store`; `false` when there is none. `HEAD`
    /// goes first: from then on the tenant is gone for every node (a
    /// writer's next commit is refused, a reader finds nothing). Its other
    /// objects are deleted after, and what an interruption leaves of them
    /// is garbage that no `HEAD` names.
    pub fn delete(store: &dyn ObjectStore, id: &str) -> TResult<bool> {
        check_id(id)?;
        let head_key = format!("{id}/{HEAD}");
        let Some((bytes, _)) = store.get(&head_key)? else { return Ok(false) };
        // An unreadable HEAD is deleted too; its objects are then left.
        let incarnation = Head::decode(&bytes).ok().map(|h| h.incarnation);
        store.delete(&head_key)?;
        if let Some(incarnation) = incarnation {
            for key in store.list(&format!("{id}/{incarnation}/"))? {
                store.delete(&key)?;
            }
        }
        Ok(true)
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// The `HEAD` this handle's copy is at.
    pub fn head(&self) -> TResult<Head> {
        self.read_state(|l| Ok(l.head.clone()))
    }

    /// Whether `HEAD`, as this handle last read it, names it as the
    /// writer. Another node may have taken over since.
    pub fn is_writer(&self) -> TResult<bool> {
        self.read_state(|l| Ok(l.is_writer()))
    }

    /// Runs `f` on the local copy, which holds every transaction committed
    /// by this handle and those of other nodes up to its last
    /// [`Tenant::refresh`], and nothing that is not committed. `f` must
    /// not write.
    pub fn read<T>(&self, f: impl FnOnce(&Db) -> Result<T>) -> TResult<T> {
        self.read_state(|l| f(&l.db))
    }

    /// [`Tenant::read`] after [`Tenant::refresh`]: what `f` sees holds
    /// every write committed before the call began. One round trip more.
    pub fn read_latest<T>(&self, f: impl FnOnce(&Db) -> Result<T>) -> TResult<T> {
        self.refresh()?;
        self.read(f)
    }

    /// Reads `HEAD` again and brings the local copy to it: applies the
    /// transactions other nodes committed since, or loads the tenant anew
    /// when they are no longer in the store as logs. `true` when `HEAD`
    /// had changed.
    pub fn refresh(&self) -> TResult<bool> {
        let mut g = self.exclusive();
        self.refresh_locked(&mut g)
    }

    /// Runs `f` on the local copy and commits the transactions it made,
    /// by a replacement of `HEAD`. When this returns `Ok`, they are in
    /// the store and visible to readers of this handle; until then no
    /// reader sees them.
    ///
    /// Writes of several threads are committed together: those that
    /// arrive while a commit is on its way to the store run one after
    /// the other when it is back, and share the next replacement of
    /// `HEAD`. So a handle commits as many transactions as were asked of
    /// it during one round trip, not one.
    ///
    /// A handle that is not the tenant's writer becomes it first, with a
    /// commit of its own that raises the epoch. Then, if another node
    /// commits before this one does, the write fails with
    /// [`TenantError::NotWriter`]: nothing of it was committed (nor of
    /// the writes it shared the commit with), and the handle has loaded
    /// the tenant again.
    ///
    /// When `f` fails, the transactions it committed before failing are
    /// committed all the same (each statement is a transaction of its
    /// own), and `f`'s error is returned. `f` must not call the handle.
    pub fn write<T>(&self, f: impl FnOnce(&Db) -> Result<T>) -> TResult<T> {
        let mut q = self.queue();
        let me = q.next;
        q.next += 1;
        q.waiting.push_back(me);
        let mut f = Some(f);
        let mut out = None;
        loop {
            if let Some(outcome) = q.done.remove(&me) {
                outcome.result()?;
                return Ok(out.expect("a writer whose commit is done has run")?);
            }
            if let Some((_, db)) = q.turn.as_ref().filter(|(t, _)| *t == me) {
                // A leader took this write into its commit: its statements
                // run here, on this thread, while the leader waits.
                let db = db.clone();
                q.turn = None;
                drop(q);
                let mut turn = Turn { tenant: self, ticket: me, returned: false };
                out = Some({
                    let db = db;
                    (f.take().expect("one turn"))(&db)
                });
                turn.returned = true;
                drop(turn);
                q = self.queue();
                continue;
            }
            if !q.leading && q.waiting.front() == Some(&me) {
                q.leading = true;
                q.waiting.pop_front();
                drop(q);
                return self.lead(f.take().expect("not run yet"));
            }
            q = self.turn.wait(q).unwrap_or_else(|e| e.into_inner());
        }
    }

    /// The writes waiting for a commit to take them (for tests).
    #[doc(hidden)]
    pub fn waiting_writers(&self) -> usize {
        self.queue().waiting.len()
    }

    fn queue(&self) -> MutexGuard<'_, Group> {
        self.group.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Commits `f`'s write and those of the writers waiting behind it.
    fn lead<T>(&self, f: impl FnOnce(&Db) -> Result<T>) -> TResult<T> {
        let mut lead = Lead { tenant: self, members: Vec::new(), ran: 0, outcome: Outcome::Failed("the write it shared a commit with failed".into()) };
        let mut g = self.exclusive();
        self.become_writer(&mut g)?;
        let l = g.as_mut().expect("loaded by become_writer");
        let before = l.last_seq()?;
        let db = l.db.clone();
        let hold = db.storage().hold_log(before + 1);
        // Those that came while the commit before this one was away.
        lead.members = {
            let mut q = self.queue();
            q.waiting.drain(..).collect()
        };
        let out = f(&db);
        let mut panicked = false;
        for i in 0..lead.members.len() {
            let m = lead.members[i];
            let mut q = self.queue();
            (q.turn, q.ran, q.panicked) = (Some((m, db.clone())), None, false);
            self.turn.notify_all();
            while q.ran != Some(m) {
                q = self.turn.wait(q).unwrap_or_else(|e| e.into_inner());
            }
            panicked |= q.panicked;
            lead.ran = i + 1;
        }
        let made = db.storage().log_from(before + 1, usize::MAX);
        db.storage().release_log(hold);
        drop(db);
        let committed = match panicked {
            // The local copy holds part of a write that did not end.
            true => {
                self.reload(&mut g);
                Err(Error::Io(std::io::Error::other("a write of the same commit panicked: nothing of the commit was written")).into())
            }
            false => self.commit(&mut g, before, made),
        };
        lead.outcome = Outcome::of(&committed);
        // The others learn of it now, not after the base.
        drop(lead);
        committed?;
        if g.as_ref().is_some_and(|l| l.is_writer() && head_due(&self.opts, &l.head)) {
            // The write is committed whatever becomes of the base.
            if let Err(e) = self.make_base_locked(&mut g) {
                tracing::warn!(tenant = %self.id, error = %e, "a tenant's new base was not made");
            }
        }
        Ok(out?)
    }

    /// Commits the transactions the local copy holds after `before`,
    /// which `made` read from its log: one replacement of `HEAD`. On an
    /// error nothing of them is committed, unless the error says that it
    /// cannot tell, and the local copy is loaded again.
    fn commit(&self, g: &mut RwLockWriteGuard<'_, Option<Loaded>>, before: u64, made: LogRead) -> TResult<()> {
        let l = g.as_mut().expect("loaded");
        let after = match l.last_seq() {
            Ok(after) => after,
            Err(e) => {
                **g = None;
                return Err(e.into());
            }
        };
        if after == before {
            return Ok(());
        }
        let entries: Vec<Entry> = match made {
            LogRead::Txns(t) if t.len() as u64 == after - before => {
                let commit_ms = now_ms();
                t.into_iter().map(|s| Entry { seq: s.seq, commit_ms, bytes: s.bytes }).collect()
            }
            _ => {
                // The local copy holds what cannot be committed.
                self.reload(g);
                return Err(Error::Schema(format!("a write of {} transactions is too large to commit at once (TenantOptions::db.ship_log_bytes)", after - before)).into());
            }
        };
        let mut head = l.head.clone();
        head.tail.extend(entries);
        head.last_seq = after;
        head.commit += 1;
        if head.tail.iter().map(|e| e.bytes.len()).sum::<usize>() > self.opts.tail_max {
            if let Err(e) = self.seal(&mut head) {
                // Nothing reached HEAD: not committed.
                self.reload(g);
                return Err(e.into());
            }
        }
        let bytes = match head.encode() {
            Ok(b) => b,
            Err(e) => {
                self.reload(g);
                return Err(e.into());
            }
        };
        match self.store.put_if_version(&self.key(HEAD), &bytes, &l.version) {
            Ok(Some(version)) => {
                l.head = head;
                l.version = version;
                Ok(())
            }
            Ok(None) => {
                self.reload(g);
                Err(match &**g {
                    Some(_) => TenantError::NotWriter,
                    None => self.gone_or(TenantError::NotWriter),
                })
            }
            Err(e) => {
                let (epoch, incarnation, version) = (l.head.epoch, l.head.incarnation.clone(), l.version.clone());
                self.settle(g, after, epoch, &incarnation, &version, e)
            }
        }
    }

    /// Makes a new base now, whatever the log's size: the database up to
    /// its last transaction as one object, after which `HEAD` names no
    /// logs. For a tenant about to be left alone, so that its next open is
    /// two reads. Does nothing when no transaction follows the base.
    pub fn make_base(&self) -> TResult<()> {
        let mut g = self.exclusive();
        self.become_writer(&mut g)?;
        self.make_base_locked(&mut g)
    }

    // ---- the store ----

    fn key(&self, rest: &str) -> String {
        format!("{}/{rest}", self.id)
    }

    /// `TenantError::Missing` when the store has no `HEAD`, else `other`.
    fn gone_or(&self, other: TenantError) -> TenantError {
        match self.store.get(&self.key(HEAD)) {
            Ok(None) => TenantError::Missing,
            _ => other,
        }
    }

    // ---- the local copy ----

    /// Where the base with key `key` is cached.
    fn cached(&self, key: &str) -> Result<PathBuf> {
        let dir = self.opts.cache_dir.join("base");
        std::fs::create_dir_all(&dir)?;
        Ok(dir.join(key.replace('/', "_")))
    }

    /// Drops the cached bases other than `key`'s.
    fn keep_cached(&self, key: &str) {
        let keep = key.replace('/', "_");
        for e in std::fs::read_dir(self.opts.cache_dir.join("base")).into_iter().flatten().flatten() {
            if e.file_name().to_string_lossy() != keep {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }

    /// An empty directory for the working database, and its path there.
    fn fresh_work(&self) -> Result<PathBuf> {
        let dir = self.opts.cache_dir.join("work");
        match std::fs::remove_dir_all(&dir) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
        std::fs::create_dir_all(&dir)?;
        Ok(dir.join("db.chloedb"))
    }

    /// The tenant as the store has it now, on a new local copy.
    fn load(&self) -> TResult<Loaded> {
        let mut last = None;
        // A base or a log HEAD names can be gone by the time it is
        // fetched, when a newer base replaced it longer ago than `keep`:
        // HEAD names the newer one by then.
        for _ in 0..4 {
            let Some((bytes, version)) = self.store.get(&self.key(HEAD))? else { return Err(TenantError::Missing) };
            let head = Head::decode(&bytes)?;
            if head.tenant != self.id {
                return Err(corrupt(format!("the HEAD of tenant {:?} is that of {:?}", self.id, head.tenant)).into());
            }
            match self.build(&head) {
                Ok(db) => return Ok(Loaded { db: Arc::new(db), head, version, me: format!("{:032x}", rand::random::<u128>()) }),
                Err(e) if not_found(&e) => last = Some(e),
                Err(e) => return Err(e.into()),
            }
        }
        Err(last.expect("tried").into())
    }

    /// Where the working database's directory says whose copy it is.
    fn mark(&self) -> PathBuf {
        self.opts.cache_dir.join("work").join("incarnation")
    }

    /// Says that the working database is a copy of `head`'s tenant.
    fn mark_work(&self, head: &Head) -> Result<()> {
        Ok(std::fs::write(self.mark(), &head.incarnation)?)
    }

    /// A local database at `head`: the working copy an earlier load or
    /// process left, when it is still a copy of the tenant, brought up
    /// to date; else the base, fetched unless cached, with its logs and
    /// its tail applied.
    fn build(&self, head: &Head) -> Result<Db> {
        if let Some(db) = self.resume(head) {
            return Ok(db);
        }
        let image = self.cached(&head.base.key)?;
        if !image.exists() {
            let part = image.with_extension("part");
            self.store.get_file(&self.key(&head.base.key), &part)?;
            std::fs::rename(&part, &image)?;
        }
        self.keep_cached(&head.base.key);
        let work = self.fresh_work()?;
        std::fs::copy(&image, &work)?;
        let db = Db::open(&work, self.opts.db.clone())?;
        let at = numbering(&db)?.last();
        if (at.seq, at.term) != (head.base.seq, head.base.term) {
            // Not the base HEAD names: do not trust the cached file again.
            let _ = std::fs::remove_file(&image);
            return Err(corrupt(format!("tenant {:?}: base {} ends at transaction {} of term {}, not {} of term {}", self.id, head.base.key, at.seq, at.term, head.base.seq, head.base.term)));
        }
        self.apply(&db, head, at)?;
        self.mark_work(head)?;
        Ok(db)
    }

    /// The working database the cache directory holds, brought to
    /// `head`, when it is a copy of this incarnation whose last
    /// transaction `head`'s history has ([`Tenant::apply`]). It may hold
    /// transactions that were never committed (a write was cut short, by
    /// a failure or the process's end): then it is not used.
    fn resume(&self, head: &Head) -> Option<Db> {
        let work = work_path(&self.opts.cache_dir);
        if !work.exists() || std::fs::read_to_string(self.mark()).ok()? != head.incarnation {
            return None;
        }
        let db = Db::open(&work, self.opts.db.clone()).ok()?;
        let at = numbering(&db).ok()?.last();
        self.apply(&db, head, at).ok()?;
        Some(db)
    }

    /// Applies to `db`, whose last transaction is `at`, those of `head`
    /// after it: from its log objects, fetched, and its tail. An error,
    /// with nothing applied, unless `at` is a transaction of `head`'s
    /// history: its base's last, or one of its logs or tail with the same
    /// number and term. A term is one handle's, and each handle builds on
    /// the history `HEAD` had, so two histories that share a transaction
    /// share all before it: the database is then a copy of the tenant as
    /// it was at `at`.
    fn apply(&self, db: &Db, head: &Head, at: SeqNo) -> Result<()> {
        let apart = |why: String| corrupt(format!("tenant {:?}: the local copy is not a copy of the tenant: {why}", self.id));
        if at.seq < head.base.seq || at.seq > head.last_seq {
            return Err(apart(format!("its last transaction is {}, the tenant's are {} to {}", at.seq, head.base.seq, head.last_seq)));
        }
        if at.seq == head.base.seq && at.term != head.base.term {
            return Err(apart(format!("transaction {} is of term {} here and of term {} in the base", at.seq, at.term, head.base.term)));
        }
        let stream = head.stream()?;
        let mut entries: Vec<Entry> = Vec::new();
        // The log that holds `at` too, to compare its term.
        for l in head.logs.iter().filter(|l| l.last >= at.seq) {
            let (bytes, _) = self.store.get(&self.key(&l.key))?.ok_or_else(|| Error::Io(std::io::Error::new(std::io::ErrorKind::NotFound, format!("{}: log object {} is missing", self.store.describe(), l.key))))?;
            let o = LogObject::decode(&bytes)?;
            if o.stream != stream || o.range() != Some((l.first, l.last)) {
                return Err(corrupt(format!("tenant {:?}: log object {} is not the one HEAD names", self.id, l.key)));
            }
            entries.extend(o.entries);
        }
        entries.extend(head.tail.iter().cloned());
        if at.seq > head.base.seq {
            match entries.iter().find(|e| e.seq.seq == at.seq) {
                Some(e) if e.seq.term == at.term => {}
                Some(e) => return Err(apart(format!("transaction {} is of term {} here and of term {} in the tenant", at.seq, at.term, e.seq.term))),
                None => return Err(corrupt(format!("tenant {:?}: HEAD's logs lack transaction {}", self.id, at.seq))),
            }
        }
        entries.retain(|e| e.seq.seq > at.seq);
        let Some(first) = entries.first() else { return Ok(()) };
        if first.seq.seq != at.seq + 1 {
            return Err(corrupt(format!("tenant {:?}: transaction {} does not follow the local copy's last, {}", self.id, first.seq.seq, at.seq)));
        }
        let txns: Vec<&[u8]> = entries.iter().map(|e| &*e.bytes).collect();
        match db.storage().apply_shipped(at, &txns)? {
            Applied::At(now) if now.seq == head.last_seq => Ok(()),
            other => Err(corrupt(format!("tenant {:?}: applying its log up to transaction {} gave {other:?}", self.id, head.last_seq))),
        }
    }

    /// Loads the tenant again after the local copy came to hold what is
    /// not committed; when that fails, the next use loads it.
    fn reload(&self, g: &mut RwLockWriteGuard<'_, Option<Loaded>>) {
        // The old copy's files are closed before they are replaced.
        **g = None;
        **g = self.load().ok();
    }

    /// The state, to change it. A write that panicked half-way left the
    /// local copy holding what is not committed: it is dropped, and the
    /// tenant loaded again from the store.
    fn exclusive(&self) -> RwLockWriteGuard<'_, Option<Loaded>> {
        self.state.write().unwrap_or_else(|poisoned| {
            let mut g = poisoned.into_inner();
            *g = None;
            self.state.clear_poison();
            g
        })
    }

    fn read_state<T>(&self, f: impl FnOnce(&Loaded) -> Result<T>) -> TResult<T> {
        // Poisoned: by the write path, which drops the copy.
        if let Ok(g) = self.state.read() {
            if let Some(l) = &*g {
                return Ok(f(l)?);
            }
        }
        let mut g = self.exclusive();
        if g.is_none() {
            *g = Some(self.load()?);
        }
        Ok(f(g.as_ref().expect("loaded"))?)
    }

    fn refresh_locked(&self, g: &mut RwLockWriteGuard<'_, Option<Loaded>>) -> TResult<bool> {
        let Some(l) = g.as_mut() else {
            **g = Some(self.load()?);
            return Ok(true);
        };
        let Some((bytes, version)) = self.store.get(&self.key(HEAD))? else {
            **g = None;
            return Err(TenantError::Missing);
        };
        if version == l.version {
            return Ok(false);
        }
        let head = Head::decode(&bytes)?;
        // The local copy is at an earlier HEAD of the same history: the
        // transactions since are applied when HEAD still has them as logs.
        let followed = head.incarnation == l.head.incarnation && head.tenant == self.id && numbering(&l.db).and_then(|n| self.apply(&l.db, &head, n.last())).is_ok();
        if followed {
            l.head = head;
            l.version = version;
        } else {
            **g = None;
            **g = Some(self.load()?);
        }
        Ok(true)
    }

    // ---- writing ----

    /// Makes this handle the tenant's writer unless it is: brings the
    /// local copy to `HEAD`, then replaces `HEAD` with one of the next
    /// epoch that names this handle. Only after that does the handle
    /// upload under that epoch, so no two nodes upload under one name.
    fn become_writer(&self, g: &mut RwLockWriteGuard<'_, Option<Loaded>>) -> TResult<()> {
        if g.as_ref().is_some_and(|l| l.is_writer()) {
            return Ok(());
        }
        self.refresh_locked(g)?;
        match self.claim(g.as_mut().expect("loaded by refresh")) {
            Ok(true) => Ok(()),
            Ok(false) => Err(TenantError::NotWriter),
            Err(e) => {
                // The put may be on its way still: the next use loads
                // the tenant again, under a new name.
                **g = None;
                Err(e.into())
            }
        }
    }

    /// Replaces `HEAD` with one of the next epoch naming `l`, and begins
    /// that term in the local database; `false` when `HEAD` has changed
    /// since `l` read it.
    fn claim(&self, l: &mut Loaded) -> Result<bool> {
        let mut head = l.head.clone();
        head.epoch += 1;
        head.writer = l.me.clone();
        head.commit += 1;
        let Some(version) = self.store.put_if_version(&self.key(HEAD), &head.encode()?, &l.version)? else { return Ok(false) };
        l.db.storage().begin_term(head.epoch)?;
        l.head = head;
        l.version = version;
        Ok(true)
    }

    /// Moves `head`'s tail to a log object of its own, uploaded now.
    fn seal(&self, head: &mut Head) -> Result<()> {
        let o = LogObject { stream: head.stream()?, db_id: 0, generation: 0, entries: std::mem::take(&mut head.tail) };
        let Some((first, last)) = o.range() else { return Ok(()) };
        let key = format!("{}/log/{first:020}-{:010}.arc", head.incarnation, head.epoch);
        self.store.put(&self.key(&key), &o.encode()?)?;
        head.logs.push(LogRef { key, first, last, bytes: o.payload_bytes() as u64 });
        Ok(())
    }

    /// After a put of `HEAD` failed with `err` without saying whether the
    /// store took it: finds out. `Ok` when the write, whose last
    /// transaction is `after` of term `epoch`, is committed. Otherwise the
    /// put is made unable to land (`HEAD` has moved on, or this replaces
    /// it under a new epoch, and the put was conditional on the version
    /// before) and the error says that nothing was committed; or, when
    /// the store keeps failing, that the write is in doubt.
    fn settle(&self, g: &mut RwLockWriteGuard<'_, Option<Loaded>>, after: u64, epoch: u64, incarnation: &str, version: &str, err: Error) -> TResult<()> {
        **g = None;
        for _ in 0..3 {
            let mut l = match self.load() {
                Ok(l) => l,
                Err(TenantError::Missing) => return Err(TenantError::Missing),
                Err(e) => return Err(TenantError::InDoubt(format!("{err}; then reading the tenant again: {e}"))),
            };
            if l.head.incarnation != incarnation {
                // Deleted and created again: the write is gone with the
                // tenant it was made to.
                **g = Some(l);
                return Err(TenantError::Missing);
            }
            // Epochs are one handle's, so a transaction of this number
            // and term is this write's.
            let n = numbering(&l.db).map_err(|e| TenantError::InDoubt(format!("{err}; then: {e}")))?;
            if n.seq >= after && n.term_at(after) == epoch {
                **g = Some(l);
                return Ok(());
            }
            if l.version != version {
                **g = Some(l);
                return Err(TenantError::NotWriter);
            }
            match self.claim(&mut l) {
                Ok(true) => {
                    **g = Some(l);
                    return Err(err.into());
                }
                // HEAD changed this instant: by the put, or by another node.
                Ok(false) => continue,
                Err(e) => return Err(TenantError::InDoubt(format!("{err}; then replacing HEAD: {e}"))),
            }
        }
        Err(TenantError::InDoubt(err.to_string()))
    }

    /// Uploads the local copy as a new base and commits a `HEAD` that
    /// names it and no logs, keeping the old base and logs for
    /// [`TenantOptions::keep`]; then deletes what no `HEAD` can name.
    fn make_base_locked(&self, g: &mut RwLockWriteGuard<'_, Option<Loaded>>) -> TResult<()> {
        let l = g.as_mut().expect("loaded");
        let seq = l.head.last_seq;
        if seq == l.head.base.seq {
            return Ok(());
        }
        let term = numbering(&l.db)?.last().term;
        let base = BaseRef { key: format!("{}/base/{seq:020}-{:010}.chloedb", l.head.incarnation, l.head.epoch), seq, term };
        let image = self.cached(&base.key)?;
        let _ = std::fs::remove_file(&image);
        l.db.storage().checkpoint(&image, &CheckpointOptions::default())?;
        self.store.put_file(&self.key(&base.key), &image)?;
        let now = now_ms();
        let mut head = l.head.clone();
        head.kept.retain(|k| k.until_ms > now);
        head.kept.push(Kept { base: head.base.key.clone(), logs: head.logs.iter().map(|l| l.key.clone()).collect(), until_ms: now + self.opts.keep.as_millis() as u64 });
        // Every commit carries the list: the oldest go first.
        let over = head.kept.len().saturating_sub(KEPT_MAX);
        head.kept.drain(..over);
        head.base = base;
        head.logs.clear();
        head.tail.clear();
        head.commit += 1;
        match self.store.put_if_version(&self.key(HEAD), &head.encode()?, &l.version) {
            Ok(Some(version)) => {
                self.keep_cached(&head.base.key);
                l.head = head;
                l.version = version;
                self.sweep(l);
                Ok(())
            }
            // Another node writes the tenant now, or the put may be on its
            // way still. The local copy holds only what is committed, but
            // the next use loads the tenant again, under a new name.
            Ok(None) => {
                **g = None;
                Err(TenantError::NotWriter)
            }
            Err(e) => {
                **g = None;
                Err(e.into())
            }
        }
    }

    /// Deletes the tenant's objects that the `HEAD` this handle just
    /// committed does not name, and that none can come to name: those of
    /// an older epoch (their writers cannot commit any more, and a later
    /// writer names only what `HEAD` names and what it uploads itself),
    /// and those of this handle's epoch, which is uploading nothing now.
    /// Failures leave garbage for the next sweep.
    fn sweep(&self, l: &Loaded) {
        if !l.is_writer() {
            return;
        }
        let prefix = self.key(&format!("{}/", l.head.incarnation));
        let Ok(keys) = self.store.list(&prefix) else { return };
        let named = l.head.names();
        for key in keys {
            let Some(rest) = key.strip_prefix(&format!("{}/", self.id)) else { continue };
            if !named.contains(rest) && epoch_of(rest).is_some_and(|e| e <= l.head.epoch) {
                let _ = self.store.delete(&key);
            }
        }
    }
}

fn head_due(opts: &TenantOptions, head: &Head) -> bool {
    head.last_seq - head.base.seq >= opts.base_after_txns || head.bytes_since_base() >= opts.base_after_bytes
}

/// The cache directory's layout, for tests.
#[doc(hidden)]
pub fn work_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join("work").join("db.chloedb")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::SeqNo;

    fn head() -> Head {
        let e = |seq, bytes: &[u8]| Entry { seq: SeqNo { term: 2, seq }, commit_ms: 5, bytes: bytes.into() };
        Head {
            layout: LAYOUT,
            tenant: "t".into(),
            incarnation: "00112233445566778899aabbccddeeff".into(),
            features: vec![SEQ_NO.into()],
            epoch: 2,
            writer: "w".into(),
            commit: 9,
            base: BaseRef { key: "i/base/x".into(), seq: 3, term: 1 },
            logs: vec![LogRef { key: "i/log/y".into(), first: 4, last: 6, bytes: 10 }],
            last_seq: 8,
            kept: vec![Kept { base: "i/base/old".into(), logs: vec!["i/log/old".into()], until_ms: 77 }],
            tail: vec![e(7, b"seven"), e(8, b"eight")],
        }
    }

    #[test]
    fn a_head_reads_back_as_written() {
        let h = head();
        let b = h.encode().unwrap();
        assert_eq!(Head::decode(&b).unwrap(), h);
        assert_eq!(h.names(), ["i/base/x", "i/log/y", "i/base/old", "i/log/old"].into_iter().collect());
        let empty = Head { logs: Vec::new(), tail: Vec::new(), last_seq: 3, ..head() };
        assert_eq!(Head::decode(&empty.encode().unwrap()).unwrap(), empty);
    }

    #[test]
    fn a_damaged_head_is_refused() {
        let b = head().encode().unwrap();
        for cut in [0, 8, 20, b.len() - 1] {
            assert!(Head::decode(&b[..cut]).is_err(), "cut at {cut}");
        }
        for at in [3, 9, 40, b.len() - 30, b.len() - 2] {
            let mut d = b.clone();
            d[at] ^= 0x10;
            assert!(Head::decode(&d).is_err(), "a bit flipped at {at}");
        }
    }

    #[test]
    fn a_head_that_disagrees_with_itself_is_not_written() {
        assert!(Head { last_seq: 9, ..head() }.encode().is_err());
        assert!(Head { base: BaseRef { key: "b".into(), seq: 2, term: 1 }, ..head() }.encode().is_err());
        assert!(Head { tail: Vec::new(), ..head() }.encode().is_err());
        assert!(Head { incarnation: "short".into(), ..head() }.encode().is_err());
    }

    #[test]
    fn a_later_layout_or_feature_is_refused_by_name() {
        let e = Head::decode(&Head { features: vec![SEQ_NO.into(), "spread".into()], ..head() }.encode().unwrap()).unwrap_err();
        assert!(e.to_string().contains("spread"), "{e}");
        let e = Head::decode(&Head { layout: 7, ..head() }.encode().unwrap()).unwrap_err();
        assert!(e.to_string().contains("layout 7"), "{e}");
    }

    #[test]
    fn ids_and_epochs() {
        assert!(valid_id("agent-7") && valid_id("a.b_c") && valid_id(&"x".repeat(128)));
        assert!(!valid_id("") && !valid_id("_templates") && !valid_id(".x") && !valid_id("a/b") && !valid_id("a b") && !valid_id(&"x".repeat(129)));
        assert_eq!(epoch_of("i/base/00000000000000001200-0000000007.chloedb"), Some(7));
        assert_eq!(epoch_of("i/log/00000000000000000001-0000000012.arc"), Some(12));
        assert_eq!(epoch_of("i/other/thing"), None);
    }
}
