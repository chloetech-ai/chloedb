//! A database opened read-only by a process other than its writer's
//! ([`crate::storage::DbOptions::read_only`]): it reads the version on disk
//! and the write-ahead log as the writer leaves them, and follows them.
//!
//! The reader holds the database as the writer's own process would after
//! opening it: an on-disk version, with the transactions logged since
//! replayed into memtables. Before a snapshot (at most once each
//! [`crate::storage::DbOptions::refresh_interval`]), and once a second from
//! a thread of its own, it looks whether the files changed: a few reads and
//! a `stat` ([`Probe`]: the newest version, the writer's session, which
//! file the log is). When only the log grew, it replays the transactions
//! appended. When there is a new version, the log was replaced (by a flush
//! or by the writer opening the database) or another writer opened it, it
//! reads the database again, keeping the segments it already has open. A
//! read that a change could have cut through (the probe differs after it)
//! is read again, so what it holds is always a version and a run of the
//! transactions that followed it, without gaps.
//!
//! In the single file, the reader's pin ([`crate::storage::share`]) keeps
//! the pages of every version it may read: its snapshots' versions and the
//! one it holds. It is raised as those go, after each look.

use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::error::{corrupt, Error, Result};
use crate::postings::bytes::OwnedBytes;
use crate::schema::Schema;
use crate::storage::db::{op_fits, DbOptions};
use crate::storage::dir::{self, Current};
use crate::storage::freelist::FreeList;
use crate::storage::lock;
use crate::storage::manifest::Manifest;
use crate::storage::pager::{self, Meta, META_PAGES, PAGE};
use crate::storage::share::{ReaderPin, Session};
use crate::storage::ship::ShipLog;
use crate::storage::snapshot::Pins;
use crate::storage::state::{Check, SegHandle, State, TableState};
use crate::storage::wal::{self, LogTail, Op, WalTxn};

/// How often the reader's thread looks for changes when no snapshot did.
pub const REFRESH_IDLE: Duration = Duration::from_secs(1);
/// How many times a read that changes cut through is read again before
/// the reader gives up for now.
const READ_TRIES: u32 = 200;
/// How many reads a refusal to fall back past a damaged meta slot or
/// version must survive before it is believed (a slot the writer is
/// writing can read as damaged for a moment).
const FALLBACK_TRIES: u32 = 20;
/// How long a reader that failed to follow a change waits before it tries
/// again by itself.
const RETRY_AFTER: Duration = Duration::from_millis(100);

/// What a reader looks at to tell whether the database changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Probe {
    /// The newest valid meta slot's txn, or `CURRENT`'s.
    version: u64,
    /// The writer's session ([`Session`]).
    session: u64,
    /// The device and inode of the file the log's name names.
    log: Option<(u64, u64)>,
}

/// What the reader holds of the files.
struct Cursor {
    probe: Probe,
    /// The log it reads, at the file the probe saw.
    log: Option<LogTail>,
    /// The newest transaction it replayed (or the version's `wal_txn`).
    applied: u64,
    pin: Option<ReaderPin>,
    session: Session,
    /// When it last looked.
    looked: Option<Instant>,
    /// Set when following a change failed: when to try again unasked.
    retry_at: Option<Instant>,
}

/// A database read from another process's files.
pub(crate) struct Reader {
    path: PathBuf,
    wal_path: PathBuf,
    dir: Option<PathBuf>,
    /// The database file, opened read-only (`LOCK` in the directory layout).
    file: File,
    opts: DbOptions,
    pub state: Arc<Mutex<State>>,
    pub pins: Arc<Pins>,
    cursor: Mutex<Cursor>,
    stop: Mutex<bool>,
    wake: Condvar,
}

/// A version read, with the transactions logged since.
struct Loaded {
    state: State,
    log: Option<LogTail>,
    applied: u64,
    probe: Probe,
    damaged: Option<String>,
}

/// What a new version can take from the state before it.
struct Before {
    tables: Vec<(u32, Arc<Schema>, Vec<Arc<SegHandle>>, u64)>,
    map: OwnedBytes,
    version: u64,
}

fn missing(e: &Error) -> bool {
    matches!(e, Error::Io(e) if e.kind() == std::io::ErrorKind::NotFound)
}

impl Reader {
    /// Reads the database at `path` (`file`: the database file or `LOCK`,
    /// held shared) for the first time; `pin` is registered already.
    #[allow(clippy::too_many_arguments)]
    pub fn open(path: &Path, wal_path: PathBuf, dir: Option<PathBuf>, file: File, opts: DbOptions, pin: Option<ReaderPin>, mut session: Session) -> Result<(Arc<Reader>, Option<String>)> {
        let first = Reader {
            path: path.to_path_buf(),
            wal_path,
            dir,
            file,
            opts,
            state: Arc::new(Mutex::new(empty_state())),
            pins: Arc::new(Pins::default()),
            cursor: Mutex::new(Cursor { probe: Probe { version: 0, session: 0, log: None }, log: None, applied: 0, pin: None, session: Session::new(Path::new("")), looked: None, retry_at: None }),
            stop: Mutex::new(false),
            wake: Condvar::new(),
        };
        let loaded = first.load(&mut session, None)?;
        let cursor = Cursor { probe: loaded.probe, log: loaded.log, applied: loaded.applied, pin, session, looked: Some(Instant::now()), retry_at: None };
        let r = Arc::new(Reader { state: Arc::new(Mutex::new(loaded.state)), cursor: Mutex::new(cursor), ..first });
        r.repin(&mut lock(&r.cursor))?;
        Ok((r, loaded.damaged))
    }

    /// Follows the files if the reader last looked `max_age` ago or more:
    /// returns whether what it holds changed. A failure leaves what it held;
    /// the reader's thread tries again a little later.
    /// `asked`: by the caller ([`crate::storage::Database::refresh`]),
    /// which tries again at once after a failure.
    pub fn sync(&self, max_age: Duration, asked: bool) -> Result<bool> {
        let mut c = lock(&self.cursor);
        if !asked && c.looked.is_some_and(|t| t.elapsed() < max_age) {
            return Ok(false);
        }
        if !asked && c.retry_at.is_some_and(|t| Instant::now() < t) {
            return Ok(false);
        }
        let r = self.catch_up(&mut c);
        c.looked = Some(Instant::now());
        c.retry_at = r.is_err().then(|| Instant::now() + RETRY_AFTER);
        self.repin(&mut c)?;
        r
    }

    /// Raises the pin to the oldest version still read: the oldest
    /// snapshot's, else the one held.
    fn repin(&self, c: &mut Cursor) -> Result<()> {
        let Some(pin) = &mut c.pin else { return Ok(()) };
        let held = lock(&self.state).meta.txn;
        let want = self.pins.min().map_or(held, |m| m.min(held));
        pin.set(want)
    }

    fn catch_up(&self, c: &mut Cursor) -> Result<bool> {
        let p = self.probe(&mut c.session)?;
        if p != c.probe {
            return self.reload(c);
        }
        let Some(log) = &mut c.log else { return Ok(false) };
        if !log.grew()? {
            return Ok(false);
        }
        let (db_id, numbered, schemas) = {
            let st = lock(&self.state);
            (st.meta.db_id, st.manifest.seq_no.is_some(), st.tables.iter().map(|t| (t.id, t.schema.clone())).collect::<Vec<_>>())
        };
        let fits = |table: u32, op: &Op| schemas.iter().find(|(id, _)| *id == table).is_none_or(|(_, s)| op_fits(s, op));
        let (txns, end, logged) = log.read(db_id, numbered, &fits)?;
        // A change under the read (a writer opening the database cuts a torn
        // tail and appends after it) is read again whole.
        if self.probe(&mut c.session)? != p {
            return self.reload(c);
        }
        let log = c.log.as_mut().expect("checked above");
        log.advance(end, logged);
        if txns.is_empty() {
            return Ok(false);
        }
        let mut st = lock(&self.state);
        let before = st.version;
        c.applied = replay(&mut st, txns, c.applied, numbered)?;
        Ok(st.version != before)
    }

    /// Reads the database again, keeping the segments that did not change.
    fn reload(&self, c: &mut Cursor) -> Result<bool> {
        let before = {
            let st = lock(&self.state);
            Before {
                tables: st.tables.iter().map(|t| (t.id, t.schema.clone(), t.handles.clone(), t.mem.gen)).collect(),
                map: st.map.clone(),
                version: st.version,
            }
        };
        let loaded = self.load(&mut c.session, Some(&before))?;
        let old = std::mem::replace(&mut *lock(&self.state), loaded.state);
        // Its segments and mappings go outside the lock.
        drop(old);
        c.probe = loaded.probe;
        c.log = loaded.log;
        c.applied = loaded.applied;
        if let Some(d) = loaded.damaged {
            tracing::warn!(path = %self.path.display(), "{d}");
        }
        Ok(true)
    }

    fn probe(&self, session: &mut Session) -> Result<Probe> {
        let version = match &self.dir {
            Some(d) => dir::read_current_file(d)?.txn,
            None => pager::read_metas(&self.file)?.first().map(|(m, _)| m.txn).ok_or_else(|| corrupt("no valid meta slot"))?,
        };
        Ok(Probe { version, session: session.read()?, log: wal::file_id(&self.wal_path)? })
    }

    /// Reads the newest version and the logs after it, again while a change
    /// cuts through the read.
    fn load(&self, session: &mut Session, before: Option<&Before>) -> Result<Loaded> {
        let mut last = None;
        for attempt in 0..READ_TRIES {
            if attempt > 0 {
                std::thread::sleep(Duration::from_micros(200 * attempt.min(50) as u64));
            }
            match self.try_load(session, before) {
                Ok(Some(l)) => return Ok(l),
                Ok(None) => {}
                Err(e) if missing(&e) => last = Some(e),
                // Refused to fall back: true only if it stays so a while.
                Err(Error::Corrupt(m)) if m.contains("would lose acknowledged writes") && attempt + 1 < FALLBACK_TRIES => {}
                Err(e) => return Err(e),
            }
        }
        Err(Error::Busy(format!(
            "{} kept changing while it was read{}",
            self.path.display(),
            last.map_or(String::new(), |e| format!(" (last: {e})"))
        )))
    }

    /// One read of the newest version and the logs after it; `None` when a
    /// change cut through it.
    fn try_load(&self, session: &mut Session, before: Option<&Before>) -> Result<Option<Loaded>> {
        let probe = self.probe(session)?;
        let (manifest, meta, slot, freelist, map, handles, damaged) = match &self.dir {
            Some(d) => {
                let (cur, manifest) = dir::read_current(d)?;
                let handles = self.open_segments(&manifest, before, |m, schema, check| {
                    // The cache files are the writer's: it fetches, evicts and
                    // cleans up after them ([`crate::storage::cold`]).
                    if m.cold.is_some() {
                        return Err(Error::Schema(format!("segment {} is kept on object storage, which a read-only open does not read", m.id)));
                    }
                    SegHandle::open_in(d, m, schema, self.opts.read_ahead, self.opts.read_ahead && self.opts.random_hints, check, None)
                })?;
                (manifest, dir_meta(&cur), 0, FreeList::new(META_PAGES), OwnedBytes::empty(), handles, None)
            }
            None => {
                let metas = pager::read_metas(&self.file)?;
                let mut damaged = None;
                let mut found = None;
                for (i, &(meta, slot)) in metas.iter().enumerate() {
                    match self.version(meta, before) {
                        Ok((manifest, freelist, map, handles)) => {
                            found = Some((manifest, meta, slot, freelist, map, handles));
                            break;
                        }
                        Err(Error::Corrupt(m)) if i + 1 < metas.len() => damaged = Some(format!("version {} skipped: {m}", meta.txn)),
                        Err(e) => return Err(e),
                    }
                }
                let (manifest, meta, slot, freelist, map, handles) = found.ok_or_else(|| corrupt("no valid meta slot"))?;
                let skipped = damaged.as_ref().map(|m| (metas[0].0, m.clone()));
                let damaged = match crate::storage::db::fallback_decision(&self.file, &self.wal_path, meta, &manifest, skipped.as_ref())? {
                    // A meta slot being written as it is read looks damaged:
                    // `load` reads again before it believes it.
                    Some(d) if !d.safe && !self.opts.recover => return Err(Error::Corrupt(crate::storage::db::refused_fallback(&self.path, &d, meta.txn))),
                    Some(d) => Some(format!("{}; reading the version before it (txn {}){}", d.what, meta.txn, d.lost)),
                    None => damaged,
                };
                (manifest, meta, slot, freelist, map, handles, damaged)
            }
        };
        if manifest.is_legacy() && !self.opts.open_old {
            return Err(Error::Schema(format!(
                "{} was written by a version of ChloeDB from before field ids, which this one opens only to copy it into a new database: run chloedb::upgrade on it",
                self.path.display()
            )));
        }
        let numbered = manifest.seq_no.is_some();
        let fits = |table: u32, op: &Op| manifest.table(table).is_none_or(|t| op_fits(&t.schema, op));
        // The open log first, then the sealed ones: a log sealed meanwhile
        // is then read twice (its transactions once), never missed.
        let tail = LogTail::open(&self.wal_path)?;
        let mut txns = self.sealed(&manifest, meta.db_id, &fits)?;
        let (active, end, logged) = match &tail {
            Some(t) => t.read(meta.db_id, numbered, &fits)?,
            None => (Vec::new(), 0, numbered),
        };
        txns.extend(active);
        if self.probe(session)? != probe || tail.as_ref().map(|t| t.id) != probe.log {
            return Ok(None);
        }
        let tail = tail.map(|mut t| {
            t.advance(end, logged);
            t
        });
        let mut st = new_state(manifest, meta, slot, freelist, map, handles, &self.opts, before);
        let from = st.manifest.wal_txn;
        let applied = replay(&mut st, txns, from, numbered)?;
        Ok(Some(Loaded { state: st, log: tail, applied, probe, damaged }))
    }

    /// The transactions of the sealed logs no version holds yet, oldest
    /// first, as opening replays them (without changing them: damage stops
    /// the replay there, with `wal_drop_damaged`, or fails it).
    fn sealed(&self, manifest: &Manifest, db_id: u64, fits: &dyn Fn(u32, &Op) -> bool) -> Result<Vec<WalTxn>> {
        let mut txns = Vec::new();
        for (t, p) in wal::sealed_files(&self.wal_path)? {
            if t <= manifest.wal_txn {
                continue;
            }
            let Some((got, damage)) = wal::read_sealed(&p, db_id, fits)? else { continue };
            txns.extend(got);
            if let Some(at) = damage {
                if !self.opts.wal_drop_damaged {
                    return Err(corrupt(format!("{} is damaged at byte {at}; open with DbOptions::wal_drop_damaged to read up to there", p.display())));
                }
                break;
            }
        }
        Ok(txns)
    }

    /// The version `meta` names in the single file, with its segments
    /// opened (those `before` holds unchanged are taken from it).
    #[allow(clippy::type_complexity)]
    fn version(&self, meta: Meta, before: Option<&Before>) -> Result<(Manifest, FreeList, OwnedBytes, Vec<Vec<Arc<SegHandle>>>)> {
        let manifest = Manifest::decode(&pager::read_extent(&self.file, meta.manifest, meta.manifest_crc)?)?;
        let freelist = FreeList::decode(&pager::read_extent(&self.file, meta.freelist, meta.freelist_crc)?, meta.page_count)?;
        // A mapping that reaches every page the version names: the one
        // before when it does (the file only shrinks by pages no version a
        // reader reads names).
        let map = match before.map(|b| &b.map).filter(|m| m.len() as u64 >= meta.page_count * PAGE) {
            Some(m) => m.clone(),
            None => pager::map_file(&self.file, self.opts.read_ahead)?,
        };
        let handles = self.open_segments(&manifest, before, |m, schema, check| SegHandle::open(&map, m, schema, self.opts.read_ahead, self.opts.read_ahead && self.opts.random_hints, check))?;
        Ok((manifest, freelist, map, handles))
    }

    /// Each table's segments: the handle `before` has for one that did not
    /// change (same entry in the manifest, same schema), else `open`'s.
    fn open_segments(
        &self,
        manifest: &Manifest,
        before: Option<&Before>,
        open: impl Fn(&crate::storage::manifest::SegmentMeta, &Schema, Check) -> Result<SegHandle>,
    ) -> Result<Vec<Vec<Arc<SegHandle>>>> {
        // A segment opened anew is checked as on open; once open, as reads go.
        let check = self.opts.check_at_open();
        manifest
            .tables
            .iter()
            .map(|t| {
                let kept = before.and_then(|b| b.tables.iter().find(|(id, s, _, _)| *id == t.id && **s == t.schema)).map(|(_, _, h, _)| h);
                t.segments
                    .iter()
                    .map(|m| match kept.and_then(|h| h.iter().find(|h| h.meta == *m)) {
                        Some(h) => Ok(h.clone()),
                        None => open(m, &t.schema, check).map(Arc::new),
                    })
                    .collect()
            })
            .collect()
    }

    /// Runs the reader's thread: looks for changes once a second when no
    /// snapshot did, so that an idle reader's pin does not hold the
    /// writer's freed pages, until [`Reader::close`].
    pub fn run(&self) {
        let mut stop = lock(&self.stop);
        loop {
            stop = self.wake.wait_timeout(stop, REFRESH_IDLE).unwrap_or_else(|e| e.into_inner()).0;
            if *stop {
                return;
            }
            drop(stop);
            if let Err(e) = self.sync(REFRESH_IDLE, false) {
                tracing::warn!(path = %self.path.display(), error = %e, "a read-only database could not follow its writer");
            }
            stop = lock(&self.stop);
        }
    }

    pub fn close(&self) {
        *lock(&self.stop) = true;
        self.wake.notify_all();
    }
}

/// Applies the transactions after `applied` (in order; one read twice is
/// skipped) to `st`; returns the newest.
fn replay(st: &mut State, txns: Vec<WalTxn>, mut applied: u64, numbered: bool) -> Result<u64> {
    for t in txns {
        if t.seq.is_some() != numbered {
            return Err(corrupt(format!(
                "the write-ahead log holds transactions {} sequence numbers, and the database {}",
                if numbered { "without" } else { "with" },
                if numbered { "numbers its writes" } else { "does not number its writes" }
            )));
        }
        if t.txn <= applied {
            continue;
        }
        st.seq = st.seq.max(t.seq.map_or(0, |n| n.seq));
        for (table, op) in t.ops {
            if let Some(i) = st.table_index(table) {
                st.apply(i, op, t.seq.map_or(0, |n| n.seq));
            }
        }
        st.txn = st.txn.max(t.txn);
        applied = t.txn;
    }
    Ok(applied)
}

/// The meta of a directory-layout version (it has no pages).
fn dir_meta(cur: &Current) -> Meta {
    let none = pager::Extent { page: 0, len: 0 };
    Meta { format: cur.format, db_id: cur.db_id, txn: cur.txn, manifest: none, manifest_crc: cur.crc, freelist: none, freelist_crc: 0, page_count: 0 }
}

/// A placeholder until the first read.
fn empty_state() -> State {
    let meta = dir_meta(&Current { db_id: 0, txn: 0, manifest: String::new(), crc: 0, format: 1 });
    new_state(Manifest::new(Schema::placeholder()), meta, 0, FreeList::new(META_PAGES), OwnedBytes::empty(), vec![Vec::new()], &DbOptions::default(), None)
}

/// The state of a version, with empty memtables. Its memtables' generations
/// and its change counter go on from `before`'s, so that nothing built for
/// the state before (a memtable segment, a snapshot to cache) is taken
/// for this one's.
#[allow(clippy::too_many_arguments)]
fn new_state(manifest: Manifest, meta: Meta, slot: usize, freelist: FreeList, map: OwnedBytes, handles: Vec<Vec<Arc<SegHandle>>>, opts: &DbOptions, before: Option<&Before>) -> State {
    let gens: HashMap<u32, u64> = before.map(|b| b.tables.iter().map(|(id, _, _, g)| (*id, *g)).collect()).unwrap_or_default();
    let tables: Vec<TableState> = manifest
        .tables
        .iter()
        .zip(handles)
        .map(|(t, h)| {
            let gen = gens.get(&t.id).map_or(0, |g| g + 1);
            let mut ts = TableState::new(t.id, Arc::new(t.schema.clone()), h, opts.memtable_chunk_docs, gen);
            ts.profiles = Arc::new(t.profiles.clone());
            ts
        })
        .collect();
    let seq = manifest.seq_no.as_ref().map_or(0, |n| n.seq);
    State {
        meta,
        slot,
        freelist,
        file_pages: meta.page_count,
        map,
        tables,
        txn: meta.txn,
        seq,
        version: before.map_or(0, |b| b.version + 1),
        pending: Default::default(),
        pending_updates: Default::default(),
        n_pending: 0,
        live_cache: HashMap::new(),
        updates_cache: HashMap::new(),
        cached: None,
        reserved: Vec::new(),
        next_segment_id: 0,
        ship: ShipLog::new(0),
        dropped: Vec::new(),
        pin_versions: true,
        manifest,
    }
}
