//! A database kept in an object store ([`chloedb::tenant`]): what is
//! acknowledged is in the store and is read back from an empty cache;
//! of two nodes that write one tenant, the one that lost the race is
//! told so and its write is nowhere; a writer that slept through
//! another's bases and cleanup is refused; logs are sealed, bases are
//! made and what `HEAD` no longer names is deleted; a store that fails,
//! before or after it took a write, never leaves a write half-known as
//! committed. The store is a directory ([`DirStore`]), wrapped where a
//! test makes it fail.

use std::collections::BTreeSet;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chloedb::db::{Db, SqlResult};
use chloedb::query::Params;
use chloedb::storage::{DirStore, ObjectStore};
use chloedb::tenant::{check_store, Head, Tenant, TenantError, TenantOptions};
use chloedb::{Error, Result, Value};

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("chloedb-tenant-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A store in `dir/store`, and options for a node whose cache is
/// `dir/<node>`.
fn store(dir: &Path) -> Arc<DirStore> {
    Arc::new(DirStore::new(dir.join("store")).unwrap())
}

fn node(dir: &Path, name: &str) -> TenantOptions {
    TenantOptions::new(dir.join(name))
}

fn sql(db: &Db, s: &str) -> Result<Vec<Vec<Value>>> {
    Ok(match db.sql(s, &Params::new())? {
        SqlResult::Rows(r) => r.rows,
        SqlResult::Written(_) => Vec::new(),
    })
}

fn notes(db: &Db) -> Result<()> {
    sql(db, "CREATE TABLE notes (id INT PRIMARY KEY, body TEXT)").map(|_| ())
}

fn insert(t: &Tenant, id: i64) -> std::result::Result<(), TenantError> {
    t.write(|db| sql(db, &format!("INSERT INTO notes VALUES ({id}, 'note {id} about the harbour')"))).map(|_| ())
}

/// The ids in `notes`, as the handle's local copy has them.
fn ids(t: &Tenant) -> BTreeSet<i64> {
    t.read(|db| sql(db, "SELECT id FROM notes LIMIT 100000")).unwrap().into_iter().map(|r| match r[0] {
        Value::Int(i) => i,
        ref v => panic!("{v:?}"),
    }).collect()
}

fn set(r: impl IntoIterator<Item = i64>) -> BTreeSet<i64> {
    r.into_iter().collect()
}

#[test]
fn what_is_written_is_read_back_from_an_empty_cache() {
    let dir = tmp("reopen");
    let s = store(&dir);
    let t = Tenant::create(s.clone(), "agent-7", node(&dir, "a"), notes).unwrap();
    assert!(t.is_writer().unwrap());
    for id in 1..=5 {
        insert(&t, id).unwrap();
    }
    // A text query on the local copy, as on any database.
    let hit = t.read(|db| sql(db, "SELECT id FROM notes WHERE MATCH(body, 'harbour') ORDER BY id LIMIT 3")).unwrap();
    assert_eq!(hit.len(), 3);
    assert!(matches!(Tenant::create(s.clone(), "agent-7", node(&dir, "x"), notes), Err(TenantError::Exists)));
    assert!(Tenant::exists(&*s, "agent-7").unwrap() && !Tenant::exists(&*s, "agent-8").unwrap());
    assert!(matches!(Tenant::open(s.clone(), "agent-8", node(&dir, "x")), Err(TenantError::Missing)));
    assert!(Tenant::open(s.clone(), "../agent-7", node(&dir, "x")).is_err());
    drop(t);

    // Another node, which has nothing: two reads of the store.
    let b = Tenant::open(s.clone(), "agent-7", node(&dir, "b")).unwrap();
    assert_eq!(ids(&b), set(1..=5));
    assert!(!b.is_writer().unwrap(), "opening takes nothing");
    let head = b.head().unwrap();
    assert_eq!((head.epoch, head.last_seq - head.base.seq), (1, 5));
    drop(b);

    // The first node again, on the cache it left: the base is not fetched
    // again, and the result is the same.
    let a = Tenant::open(s, "agent-7", node(&dir, "a")).unwrap();
    assert_eq!(ids(&a), set(1..=5));
    insert(&a, 6).unwrap();
    assert_eq!((ids(&a), a.head().unwrap().epoch), (set(1..=6), 2));
}

#[test]
fn of_two_writers_the_later_is_refused_and_its_write_is_nowhere() {
    let dir = tmp("two");
    let s = store(&dir);
    let a = Tenant::create(s.clone(), "t", node(&dir, "a"), notes).unwrap();
    insert(&a, 1).unwrap();
    let b = Tenant::open(s.clone(), "t", node(&dir, "b")).unwrap();
    assert_eq!(ids(&b), set([1]));

    // B writes: it takes the next epoch first.
    insert(&b, 2).unwrap();
    assert_eq!((b.head().unwrap().epoch, b.is_writer().unwrap()), (2, true));
    // A has not heard, and reads what it has.
    assert_eq!(ids(&a), set([1]));

    // A, which believes it is the writer, writes: refused, with nothing
    // of the write left anywhere, and B's row now in its copy.
    assert!(matches!(insert(&a, 3), Err(TenantError::NotWriter)));
    assert_eq!(ids(&a), set([1, 2]));
    assert!(!a.is_writer().unwrap());

    // Trying again works: A takes epoch 3.
    insert(&a, 3).unwrap();
    assert_eq!(a.head().unwrap().epoch, 3);
    assert_eq!(ids(&b), set([1, 2]), "B's copy is as of its last look");
    assert_eq!(b.read_latest(|db| sql(db, "SELECT id FROM notes LIMIT 10")).unwrap().len(), 3);
    assert!(!b.is_writer().unwrap());

    // A change of tables reaches the other node as rows do.
    a.write(|db| sql(db, "CREATE TABLE tasks (id INT PRIMARY KEY, state KEYWORD)")).unwrap();
    a.write(|db| sql(db, "INSERT INTO tasks VALUES (1, 'open')")).unwrap();
    assert!(b.refresh().unwrap() && !b.refresh().unwrap());
    assert_eq!(b.read(|db| sql(db, "SELECT state FROM tasks WHERE id = 1")).unwrap(), vec![vec![Value::Str("open".into())]]);

    let fresh = Tenant::open(s, "t", node(&dir, "c")).unwrap();
    assert_eq!(ids(&fresh), set([1, 2, 3]));
}

#[test]
fn nodes_writing_at_once_lose_nothing_acknowledged_and_keep_nothing_refused() {
    let dir = tmp("contend");
    let s = store(&dir);
    drop(Tenant::create(s.clone(), "t", node(&dir, "seed"), notes).unwrap());
    let threads: Vec<_> = (0..4i64)
        .map(|n| {
            let (s, dir) = (s.clone(), dir.clone());
            std::thread::spawn(move || {
                let t = Tenant::open(s, "t", node(&dir, &format!("n{n}"))).unwrap();
                let (mut acked, mut refused) = (Vec::new(), Vec::new());
                // Every attempt writes a key of its own, so that a refused
                // write that reached the store anyway would show.
                for attempt in 0..60 {
                    let id = n * 1_000_000 + attempt;
                    match insert(&t, id) {
                        Ok(()) => acked.push(id),
                        Err(TenantError::NotWriter) => refused.push(id),
                        Err(e) => panic!("node {n}: {e}"),
                    }
                }
                (acked, refused)
            })
        })
        .collect();
    let (mut acked, mut refused) = (BTreeSet::new(), Vec::new());
    for t in threads {
        let (a, r) = t.join().unwrap();
        acked.extend(a);
        refused.extend(r);
    }
    assert!(!acked.is_empty());
    let t = Tenant::open(s, "t", node(&dir, "check")).unwrap();
    let got = ids(&t);
    assert_eq!(got, acked, "{} acknowledged, {} refused", acked.len(), refused.len());
    assert!(refused.iter().all(|id| !got.contains(id)));
    // Each transaction once, in one history.
    let head = t.head().unwrap();
    assert_eq!(head.last_seq as usize, acked.len() + 1, "the table's creation and one transaction a row");
}

/// The keys under the tenant's incarnation, relative to the tenant.
fn objects(s: &DirStore, head: &Head) -> BTreeSet<String> {
    s.list(&format!("{}/{}/", head.tenant, head.incarnation)).unwrap().into_iter().map(|k| k[head.tenant.len() + 1..].to_string()).collect()
}

fn named(head: &Head) -> BTreeSet<String> {
    let kept = head.kept.iter().flat_map(|k| k.logs.iter().chain([&k.base]));
    head.logs.iter().map(|l| &l.key).chain([&head.base.key]).chain(kept).cloned().collect()
}

#[test]
fn logs_are_sealed_bases_made_and_what_head_no_longer_names_deleted() {
    let dir = tmp("bases");
    let s = store(&dir);
    let small = |name: &str| TenantOptions { tail_max: 300, base_after_txns: 12, keep: Duration::ZERO, ..node(&dir, name) };
    let t = Tenant::create(s.clone(), "t", small("a"), notes).unwrap();
    let reader = Tenant::open(s.clone(), "t", small("r")).unwrap();
    let (mut sealed, mut bases) = (0, BTreeSet::new());
    for id in 1..=40 {
        insert(&t, id).unwrap();
        let head = t.head().unwrap();
        sealed += usize::from(!head.logs.is_empty());
        bases.insert(head.base.seq);
        // The store holds what HEAD names, and after a base nothing else.
        let (have, want) = (objects(&s, &head), named(&head));
        assert!(want.is_subset(&have), "after {id}: {have:?} lacks some of {want:?}");
        if head.last_seq == head.base.seq {
            assert_eq!(have, want, "after the base at {id}");
        }
        assert!(head.tail.iter().map(|e| e.bytes.len()).sum::<usize>() <= 300);
    }
    assert!(sealed > 0 && bases.len() >= 3, "{sealed} heads with logs, bases at {bases:?}");
    // One write larger than the tail goes out as a log object of its own.
    let big = "harbour ".repeat(200);
    t.write(|db| sql(db, &format!("INSERT INTO notes VALUES (41, '{big}')"))).unwrap();
    let head = t.head().unwrap();
    assert!(head.tail.is_empty() && head.logs.last().is_some_and(|l| l.last == head.last_seq));

    // A reader that slept through all of it: the base it has is gone,
    // and it comes to the tenant as it is.
    assert!(ids(&reader).is_empty());
    assert!(reader.refresh().unwrap());
    assert_eq!(ids(&reader), set(1..=41));
    assert_eq!(ids(&Tenant::open(s.clone(), "t", small("c")).unwrap()), set(1..=41));

    // A base on request, as before a tenant is left alone: no logs after.
    t.make_base().unwrap();
    let head = t.head().unwrap();
    assert!(head.logs.is_empty() && head.tail.is_empty() && head.base.seq == head.last_seq);
    t.make_base().unwrap();
    assert_eq!(t.head().unwrap(), head, "nothing to do the second time");
    assert_eq!(ids(&Tenant::open(s, "t", small("d")).unwrap()), set(1..=41));
}

#[test]
fn a_writer_that_slept_through_bases_and_cleanup_is_refused() {
    let dir = tmp("slept");
    let s = store(&dir);
    let small = |name: &str| TenantOptions { tail_max: 300, base_after_txns: 8, keep: Duration::ZERO, ..node(&dir, name) };
    let a = Tenant::create(s.clone(), "t", small("a"), notes).unwrap();
    insert(&a, 1).unwrap();
    let before = a.head().unwrap();

    // Another node takes over and writes until nothing A knows of is left
    // in the store but HEAD's name.
    let b = Tenant::open(s.clone(), "t", small("b")).unwrap();
    for id in 100..140 {
        insert(&b, id).unwrap();
    }
    let now = b.head().unwrap();
    assert!(!objects(&s, &now).contains(&before.base.key), "A's base is deleted");

    // A wakes, still the writer by its own account, and writes.
    assert!(a.is_writer().unwrap());
    assert!(matches!(insert(&a, 2), Err(TenantError::NotWriter)));
    let mut want = set(100..140);
    want.insert(1);
    assert_eq!(ids(&a), want);
    assert_eq!(ids(&Tenant::open(s.clone(), "t", small("c")).unwrap()), want);
    // A's refused write left no object behind once a writer has swept.
    insert(&a, 3).unwrap();
    a.make_base().unwrap();
    let head = a.head().unwrap();
    assert_eq!(objects(&s, &head), named(&head));
}

#[test]
fn a_tenant_deleted_and_created_again_is_another_tenant() {
    let dir = tmp("again");
    let s = store(&dir);
    let old = Tenant::create(s.clone(), "t", node(&dir, "a"), notes).unwrap();
    insert(&old, 1).unwrap();
    let first = old.head().unwrap();

    assert!(Tenant::delete(&*s, "t").unwrap());
    assert!(!Tenant::delete(&*s, "t").unwrap() && !Tenant::exists(&*s, "t").unwrap());
    assert!(s.list("t/").unwrap().is_empty(), "{:?}", s.list("t/"));
    // The old handle reads what it has, and cannot write.
    assert_eq!(ids(&old), set([1]));
    assert!(matches!(insert(&old, 2), Err(TenantError::Missing)));
    assert!(matches!(old.refresh(), Err(TenantError::Missing)));

    let new = Tenant::create(s.clone(), "t", node(&dir, "b"), notes).unwrap();
    insert(&new, 10).unwrap();
    assert_ne!(new.head().unwrap().incarnation, first.incarnation);

    // A handle of the old tenant that never heard of the delete: its
    // write is refused, and it finds the new tenant.
    let dir2 = tmp("again2");
    let s2 = store(&dir2);
    let sleeper = Tenant::create(s2.clone(), "t", node(&dir2, "a"), notes).unwrap();
    insert(&sleeper, 1).unwrap();
    Tenant::delete(&*s2, "t").unwrap();
    let new2 = Tenant::create(s2.clone(), "t", node(&dir2, "b"), notes).unwrap();
    insert(&new2, 10).unwrap();
    assert!(matches!(insert(&sleeper, 2), Err(TenantError::NotWriter)));
    assert_eq!(ids(&sleeper), set([10]));
    assert_eq!(ids(&Tenant::open(s2, "t", node(&dir2, "c")).unwrap()), set([10]));
}

#[test]
fn a_statement_that_fails_commits_nothing_and_those_before_it_stay() {
    let dir = tmp("partial");
    let s = store(&dir);
    let t = Tenant::create(s.clone(), "t", node(&dir, "a"), notes).unwrap();
    let before = t.head().unwrap();
    assert!(matches!(t.write(|db| sql(db, "INSERT INTO nowhere VALUES (1)")), Err(TenantError::Db(_))));
    assert_eq!(t.head().unwrap(), before, "a write that wrote nothing replaces nothing");
    let r = t.write(|db| {
        sql(db, "INSERT INTO notes VALUES (1, 'first')")?;
        sql(db, "INSERT INTO nowhere VALUES (2)")
    });
    assert!(matches!(r, Err(TenantError::Db(_))));
    assert_eq!(ids(&t), set([1]));
    assert_eq!(ids(&Tenant::open(s, "t", node(&dir, "b")).unwrap()), set([1]));
}

#[test]
fn a_write_that_panics_commits_nothing() {
    let dir = tmp("panic");
    let s = store(&dir);
    let t = Tenant::create(s.clone(), "t", node(&dir, "a"), notes).unwrap();
    insert(&t, 1).unwrap();
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        t.write(|db| -> Result<()> {
            sql(db, "INSERT INTO notes VALUES (2, 'written, then the program fails')")?;
            panic!("in the middle of a write");
        })
    }));
    assert!(panicked.is_err());
    // The local copy held row 2: it is dropped, not shown.
    assert_eq!(ids(&t), set([1]));
    insert(&t, 3).unwrap();
    assert_eq!(ids(&t), set([1, 3]));
    assert_eq!(ids(&Tenant::open(s, "t", node(&dir, "b")).unwrap()), set([1, 3]));
}

#[test]
fn a_working_copy_is_taken_up_again_only_when_it_is_a_copy_of_the_tenant() {
    let dir = tmp("resume");
    let s = store(&dir);
    let a = Tenant::create(s.clone(), "t", node(&dir, "a"), notes).unwrap();
    for id in 1..=3 {
        insert(&a, id).unwrap();
    }
    // Two handles do not share a cache directory.
    assert!(matches!(Tenant::open(s.clone(), "t", node(&dir, "a")), Err(TenantError::Db(Error::Busy(_)))));
    let head = a.head().unwrap();
    drop(a);

    // With neither the base in the store nor the one in its cache, the
    // node opens the tenant from the working copy it left, and applies
    // what another node wrote since.
    let base = s.get(&format!("t/{}", head.base.key)).unwrap().unwrap().0;
    let b = Tenant::open(s.clone(), "t", node(&dir, "b")).unwrap();
    insert(&b, 4).unwrap();
    s.delete(&format!("t/{}", head.base.key)).unwrap();
    std::fs::remove_dir_all(dir.join("a").join("base")).unwrap();
    let a = Tenant::open(s.clone(), "t", node(&dir, "a")).unwrap();
    assert_eq!(ids(&a), set(1..=4));
    assert!(Tenant::open(s.clone(), "t", node(&dir, "c")).is_err(), "a node with nothing needs the base");
    s.put(&format!("t/{}", head.base.key), &base).unwrap();

    // A write is cut short after it reached the working copy and before
    // it was committed; the handle is gone with its process.
    insert(&a, 5).unwrap();
    let cut = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        a.write(|db| -> Result<()> {
            sql(db, "INSERT INTO notes VALUES (6, 'never committed')")?;
            panic!("the process ends here");
        })
    }));
    assert!(cut.is_err());
    drop(a);
    // Opened again on that cache: row 6 is not there,
    let a = Tenant::open(s.clone(), "t", node(&dir, "a")).unwrap();
    assert_eq!(ids(&a), set(1..=5));
    // nor when another node has since written a transaction of the same
    // number, which the working copy must not be taken to hold.
    let cut = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        a.write(|db| -> Result<()> {
            sql(db, "INSERT INTO notes VALUES (7, 'never committed either')")?;
            panic!("and here");
        })
    }));
    assert!(cut.is_err());
    drop(a);
    // (B has not heard of A's fifth row: its first try is refused.)
    assert!(matches!(insert(&b, 8), Err(TenantError::NotWriter)));
    insert(&b, 8).unwrap();
    let a = Tenant::open(s.clone(), "t", node(&dir, "a")).unwrap();
    assert_eq!(ids(&a), set([1, 2, 3, 4, 5, 8]));
    assert_eq!(ids(&a), ids(&Tenant::open(s.clone(), "t", node(&dir, "c")).unwrap()));

    // A tenant deleted and created again is not the one the cache holds.
    drop((a, b));
    Tenant::delete(&*s, "t").unwrap();
    drop(Tenant::create(s.clone(), "t", node(&dir, "d"), notes).unwrap());
    assert!(ids(&Tenant::open(s, "t", node(&dir, "a")).unwrap()).is_empty());
}

#[test]
fn a_stores_tenants_are_listed() {
    let dir = tmp("list");
    let s = store(&dir);
    assert!(chloedb::tenant::list(&*s).unwrap().is_empty());
    for id in ["b", "a", "c.d"] {
        drop(Tenant::create(s.clone(), id, node(&dir, id), notes).unwrap());
    }
    s.put("_templates/x", b"not a tenant").unwrap();
    assert_eq!(chloedb::tenant::list(&*s).unwrap(), ["a", "b", "c.d"]);
    Tenant::delete(&*s, "b").unwrap();
    assert_eq!(chloedb::tenant::list(&*s).unwrap(), ["a", "c.d"]);
}

/// What a [`Faulty`] store does to a conditional put of `HEAD`.
const TAKES: u32 = 0;
/// Fails it, having written nothing.
const DROPS: u32 = 1;
/// Writes it, then fails: the caller cannot tell.
const TAKES_AND_FAILS: u32 = 2;

/// A store that fails on request.
#[derive(Debug)]
struct Faulty {
    inner: DirStore,
    /// The next conditional puts of `HEAD` to treat as `mode` says.
    faults: AtomicU64,
    mode: AtomicU32,
    /// Whether reads fail too (the store is unreachable).
    down: AtomicU32,
    /// Whether puts of other objects fail.
    no_uploads: AtomicU32,
}

impl Faulty {
    fn new(dir: &Path) -> Arc<Faulty> {
        Arc::new(Faulty { inner: DirStore::new(dir.join("store")).unwrap(), faults: AtomicU64::new(0), mode: AtomicU32::new(TAKES), down: AtomicU32::new(0), no_uploads: AtomicU32::new(0) })
    }

    fn fail_next(&self, n: u64, mode: u32) {
        self.mode.store(mode, Ordering::SeqCst);
        self.faults.store(n, Ordering::SeqCst);
    }

    fn fault(&self) -> u32 {
        // (`fetch_update` is deprecated on newer toolchains and its
        // replacement missing on older ones.)
        let mut left = self.faults.load(Ordering::SeqCst);
        while left > 0 {
            match self.faults.compare_exchange(left, left - 1, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => return self.mode.load(Ordering::SeqCst),
                Err(now) => left = now,
            }
        }
        TAKES
    }

    fn up(&self) -> Result<()> {
        match self.down.load(Ordering::SeqCst) {
            0 => Ok(()),
            _ => Err(failed("the store is unreachable")),
        }
    }
}

fn failed(what: &str) -> Error {
    Error::Io(std::io::Error::other(what.to_string()))
}

impl ObjectStore for Faulty {
    fn put_file(&self, key: &str, path: &Path) -> Result<()> {
        self.up()?;
        self.inner.put_file(key, path)
    }
    fn put(&self, key: &str, bytes: &[u8]) -> Result<()> {
        self.up()?;
        if self.no_uploads.load(Ordering::SeqCst) != 0 {
            return Err(failed("an upload failed"));
        }
        self.inner.put(key, bytes)
    }
    fn get_range(&self, key: &str, range: Range<u64>) -> Result<Vec<u8>> {
        self.up()?;
        self.inner.get_range(key, range)
    }
    fn delete(&self, key: &str) -> Result<()> {
        self.up()?;
        self.inner.delete(key)
    }
    fn describe(&self) -> String {
        self.inner.describe()
    }
    fn get(&self, key: &str) -> Result<Option<(Vec<u8>, String)>> {
        self.up()?;
        self.inner.get(key)
    }
    fn put_if_absent(&self, key: &str, bytes: &[u8]) -> Result<Option<String>> {
        self.up()?;
        self.inner.put_if_absent(key, bytes)
    }
    fn put_if_version(&self, key: &str, bytes: &[u8], version: &str) -> Result<Option<String>> {
        // A put the store takes and does not answer reaches it before it
        // goes away.
        match self.fault() {
            DROPS => Err(failed("the put timed out (and was not made)")),
            TAKES_AND_FAILS => {
                self.inner.put_if_version(key, bytes, version)?;
                Err(failed("the put timed out (and was made)"))
            }
            _ => {
                self.up()?;
                self.inner.put_if_version(key, bytes, version)
            }
        }
    }
    fn list(&self, prefix: &str) -> Result<Vec<String>> {
        self.up()?;
        self.inner.list(prefix)
    }
    fn get_file(&self, key: &str, dest: &Path) -> Result<()> {
        self.up()?;
        self.inner.get_file(key, dest)
    }
}

#[test]
fn a_put_the_store_does_not_answer_is_settled_one_way_or_the_other() {
    let dir = tmp("faulty");
    let s = Faulty::new(&dir);
    let t = Tenant::create(s.clone(), "t", node(&dir, "a"), notes).unwrap();
    insert(&t, 1).unwrap();

    // The put fails and was not made: the write is not committed, says
    // the error, and it stays that way.
    s.fail_next(1, DROPS);
    assert!(matches!(insert(&t, 2), Err(TenantError::Db(_))));
    assert_eq!(ids(&t), set([1]));
    assert_eq!(ids(&Tenant::open(s.clone(), "t", node(&dir, "b")).unwrap()), set([1]));
    insert(&t, 3).unwrap();

    // The put fails but was made: the write is found committed.
    s.fail_next(1, TAKES_AND_FAILS);
    insert(&t, 4).unwrap();
    assert_eq!(ids(&t), set([1, 3, 4]));
    assert_eq!(ids(&Tenant::open(s.clone(), "t", node(&dir, "b")).unwrap()), set([1, 3, 4]));

    // The same while the handle is taking the epoch: nothing of the
    // user's write was tried, so it is an error, and the next try works.
    let c = Tenant::open(s.clone(), "t", node(&dir, "c")).unwrap();
    s.fail_next(1, TAKES_AND_FAILS);
    assert!(matches!(insert(&c, 5), Err(TenantError::Db(_))));
    insert(&c, 5).unwrap();
    assert_eq!(ids(&c), set([1, 3, 4, 5]));

    // The store goes away in the middle of a write that it took: the
    // handle cannot tell, and says so; when the store is back the write
    // is there, once.
    s.fail_next(1, TAKES_AND_FAILS);
    let r = c.write(|db| {
        let rows = sql(db, "INSERT INTO notes VALUES (6, 'six')");
        s.down.store(1, Ordering::SeqCst);
        rows
    });
    assert!(matches!(r, Err(TenantError::InDoubt(_))), "{r:?}");
    assert!(c.read(|db| sql(db, "SELECT id FROM notes LIMIT 10")).is_err(), "no copy to read while the store is away");
    s.down.store(0, Ordering::SeqCst);
    assert_eq!(ids(&c), set([1, 3, 4, 5, 6]));
    // And one it did not take: in doubt as well, and absent after.
    insert(&c, 8).unwrap();
    s.fail_next(1, DROPS);
    let r = c.write(|db| {
        let rows = sql(db, "INSERT INTO notes VALUES (7, 'seven')");
        s.down.store(1, Ordering::SeqCst);
        rows
    });
    assert!(matches!(r, Err(TenantError::InDoubt(_))), "{r:?}");
    s.down.store(0, Ordering::SeqCst);
    assert_eq!(ids(&c), set([1, 3, 4, 5, 6, 8]));

    // A log object that cannot be uploaded: not committed.
    let big = "harbour ".repeat(40_000);
    s.no_uploads.store(1, Ordering::SeqCst);
    assert!(matches!(c.write(|db| sql(db, &format!("INSERT INTO notes VALUES (9, '{big}')"))), Err(TenantError::Db(_))));
    s.no_uploads.store(0, Ordering::SeqCst);
    assert_eq!(ids(&c), set([1, 3, 4, 5, 6, 8]));
    c.write(|db| sql(db, &format!("INSERT INTO notes VALUES (9, '{big}')"))).unwrap();
    assert_eq!(ids(&Tenant::open(s, "t", node(&dir, "d")).unwrap()), set([1, 3, 4, 5, 6, 8, 9]));
}

#[test]
fn a_damaged_head_or_base_is_an_error_not_other_rows() {
    let dir = tmp("damaged");
    let s = store(&dir);
    let t = Tenant::create(s.clone(), "t", node(&dir, "a"), notes).unwrap();
    insert(&t, 1).unwrap();
    let head = t.head().unwrap();
    drop(t);
    let (good, _) = s.get("t/HEAD").unwrap().unwrap();

    let mut bad = good.clone();
    let at = bad.len() - 20;
    bad[at] ^= 1;
    s.put("t/HEAD", &bad).unwrap();
    assert!(matches!(Tenant::open(s.clone(), "t", node(&dir, "b")), Err(TenantError::Db(Error::Corrupt(_)))));

    // A HEAD that names a base which is gone.
    s.put("t/HEAD", &good).unwrap();
    s.delete(&format!("t/{}", head.base.key)).unwrap();
    assert!(Tenant::open(s.clone(), "t", node(&dir, "c")).is_err());
    // The node that has the base cached still opens the tenant.
    assert_eq!(ids(&Tenant::open(s, "t", node(&dir, "a")).unwrap()), set([1]));
}

/// A store whose replacements of `HEAD` take a while, or wait to be let
/// through.
#[derive(Debug)]
struct Slow {
    inner: DirStore,
    delay: Duration,
    /// Whether a replacement of `HEAD` waits, and how many of those
    /// waiting are let through all the same.
    held: std::sync::Mutex<(bool, u64)>,
    let_go: std::sync::Condvar,
    /// The replacements of `HEAD` asked for.
    puts: AtomicU64,
}

impl Slow {
    fn new(dir: &Path, delay: Duration) -> Arc<Slow> {
        Arc::new(Slow { inner: DirStore::new(dir.join("store")).unwrap(), delay, held: std::sync::Mutex::new((false, 0)), let_go: std::sync::Condvar::new(), puts: AtomicU64::new(0) })
    }

    fn hold(&self, held: bool) {
        *self.held.lock().unwrap() = (held, 0);
        self.let_go.notify_all();
    }

    /// Lets one of the replacements that wait through.
    fn let_one(&self) {
        self.held.lock().unwrap().1 += 1;
        self.let_go.notify_all();
    }

    /// Holds the replacements until the guard is dropped, by a failed
    /// assertion too: the threads that wait must end for the test to.
    fn holding(&self) -> Holding<'_> {
        self.hold(true);
        Holding(self)
    }
}

struct Holding<'a>(&'a Slow);

impl Drop for Holding<'_> {
    fn drop(&mut self) {
        self.0.hold(false);
    }
}

impl ObjectStore for Slow {
    fn put_file(&self, key: &str, path: &Path) -> Result<()> {
        self.inner.put_file(key, path)
    }
    fn put(&self, key: &str, bytes: &[u8]) -> Result<()> {
        self.inner.put(key, bytes)
    }
    fn get_range(&self, key: &str, range: Range<u64>) -> Result<Vec<u8>> {
        self.inner.get_range(key, range)
    }
    fn delete(&self, key: &str) -> Result<()> {
        self.inner.delete(key)
    }
    fn describe(&self) -> String {
        self.inner.describe()
    }
    fn get(&self, key: &str) -> Result<Option<(Vec<u8>, String)>> {
        self.inner.get(key)
    }
    fn put_if_absent(&self, key: &str, bytes: &[u8]) -> Result<Option<String>> {
        self.inner.put_if_absent(key, bytes)
    }
    fn put_if_version(&self, key: &str, bytes: &[u8], version: &str) -> Result<Option<String>> {
        self.puts.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(self.delay);
        let mut held = self.held.lock().unwrap();
        while held.0 && held.1 == 0 {
            held = self.let_go.wait(held).unwrap();
        }
        held.1 = held.1.saturating_sub(1);
        drop(held);
        self.inner.put_if_version(key, bytes, version)
    }
    fn list(&self, prefix: &str) -> Result<Vec<String>> {
        self.inner.list(prefix)
    }
}

fn wait_for(what: &str, done: impl Fn() -> bool) {
    let t0 = std::time::Instant::now();
    while !done() {
        assert!(t0.elapsed() < Duration::from_secs(60), "waited a minute for {what}");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn writes_that_wait_for_a_commit_share_the_next() {
    let dir = tmp("group");
    let s = Slow::new(&dir, Duration::from_millis(15));
    let t = Tenant::create(s.clone(), "t", node(&dir, "a"), notes).unwrap();
    insert(&t, 0).unwrap();
    let (commit, puts) = (t.head().unwrap().commit, s.puts.load(Ordering::SeqCst));
    std::thread::scope(|scope| {
        for n in 1..=6i64 {
            let t = &t;
            scope.spawn(move || {
                for i in 0..10 {
                    insert(t, n * 100 + i).unwrap();
                }
            });
        }
    });
    let want: BTreeSet<i64> = (1..=6).flat_map(|n| (0..10).map(move |i| n * 100 + i)).chain([0]).collect();
    assert_eq!(ids(&t), want);
    let head = t.head().unwrap();
    assert_eq!(head.last_seq, 62, "the table, row 0 and sixty rows, each a transaction");
    // Sixty writes from six threads, each waiting for its own to be
    // committed: a commit takes the five that waited while the one
    // before it was away.
    let (commits, puts) = (head.commit - commit, s.puts.load(Ordering::SeqCst) - puts);
    assert!(commits < 40 && puts < 40, "{commits} commits, {puts} puts for 60 writes");
    assert_eq!(ids(&Tenant::open(s, "t", node(&dir, "b")).unwrap()), want);
}

#[test]
fn a_commit_is_all_of_its_writes_or_none() {
    let dir = tmp("group-fail");
    let s = Slow::new(&dir, Duration::ZERO);
    let t = Tenant::create(s.clone(), "t", node(&dir, "a"), notes).unwrap();
    insert(&t, 0).unwrap();

    // One write is on its way to the store; two more arrive meanwhile,
    // and the second of them panics in the middle of its statements.
    let puts = s.puts.load(Ordering::SeqCst);
    let (first, second, third) = std::thread::scope(|scope| {
        let held = s.holding();
        let first = scope.spawn(|| insert(&t, 1));
        wait_for("the first write to reach the store", || s.puts.load(Ordering::SeqCst) > puts);
        let second = scope.spawn(|| insert(&t, 2));
        wait_for("the second write to wait", || t.waiting_writers() == 1);
        let third = scope.spawn(|| {
            t.write(|db| -> Result<()> {
                sql(db, "INSERT INTO notes VALUES (3, 'half a write')")?;
                panic!("in the middle of a write that shares a commit");
            })
        });
        wait_for("the third write to wait", || t.waiting_writers() == 2);
        drop(held);
        (first.join(), second.join(), third.join())
    });
    first.unwrap().unwrap();
    // The second led the commit the third was part of: none of it is
    // written, and it is told so.
    let e = second.unwrap().unwrap_err();
    assert!(matches!(&e, TenantError::Db(e) if e.to_string().contains("panicked")), "{e}");
    assert!(third.is_err());
    assert_eq!(ids(&t), set([0, 1]));
    // The handle loaded the tenant again: its next write takes it back.
    insert(&t, 4).unwrap();

    // Another node takes the tenant while a commit of two writes is on
    // its way to the store: both are refused, and neither is anywhere.
    let puts = s.puts.load(Ordering::SeqCst);
    let (first, second, third) = std::thread::scope(|scope| {
        let held = s.holding();
        let first = scope.spawn(|| insert(&t, 5));
        wait_for("the first write to reach the store", || s.puts.load(Ordering::SeqCst) == puts + 1);
        let second = scope.spawn(|| insert(&t, 6));
        wait_for("the second write to wait", || t.waiting_writers() == 1);
        let third = scope.spawn(|| insert(&t, 7));
        wait_for("the third write to wait", || t.waiting_writers() == 2);
        s.let_one();
        wait_for("the commit of the two to reach the store", || s.puts.load(Ordering::SeqCst) == puts + 2);
        assert_eq!(t.waiting_writers(), 0);
        // B's own puts go to the directory behind the store's back.
        let direct: Arc<dyn ObjectStore> = Arc::new(DirStore::new(dir.join("store")).unwrap());
        let b = Tenant::open(direct, "t", node(&dir, "b")).unwrap();
        insert(&b, 8).unwrap();
        drop(held);
        (first.join().unwrap(), second.join().unwrap(), third.join().unwrap())
    });
    first.unwrap();
    assert!(matches!(second, Err(TenantError::NotWriter)), "{second:?}");
    assert!(matches!(third, Err(TenantError::NotWriter)), "{third:?}");
    assert_eq!(ids(&t), set([0, 1, 4, 5, 8]));
    // Asked again, they are committed.
    insert(&t, 6).unwrap();
    insert(&t, 7).unwrap();
    assert_eq!(ids(&Tenant::open(s, "t", node(&dir, "c")).unwrap()), set([0, 1, 4, 5, 6, 7, 8]));
}

/// A store that takes every put, whatever the condition: what a tenant
/// must not be kept in.
#[derive(Debug)]
struct Unconditional(DirStore);

impl ObjectStore for Unconditional {
    fn put_file(&self, key: &str, path: &Path) -> Result<()> {
        self.0.put_file(key, path)
    }
    fn put(&self, key: &str, bytes: &[u8]) -> Result<()> {
        self.0.put(key, bytes)
    }
    fn get_range(&self, key: &str, range: Range<u64>) -> Result<Vec<u8>> {
        self.0.get_range(key, range)
    }
    fn delete(&self, key: &str) -> Result<()> {
        self.0.delete(key)
    }
    fn describe(&self) -> String {
        self.0.describe()
    }
    fn get(&self, key: &str) -> Result<Option<(Vec<u8>, String)>> {
        self.0.get(key)
    }
    fn put_if_absent(&self, key: &str, bytes: &[u8]) -> Result<Option<String>> {
        self.0.put(key, bytes)?;
        Ok(self.0.get(key)?.map(|(_, v)| v))
    }
    fn put_if_version(&self, key: &str, bytes: &[u8], _: &str) -> Result<Option<String>> {
        self.put_if_absent(key, bytes)
    }
    fn list(&self, prefix: &str) -> Result<Vec<String>> {
        self.0.list(prefix)
    }
}

/// A store with the calls of before conditional writes only.
#[derive(Debug)]
struct Plain(DirStore);

impl ObjectStore for Plain {
    fn put_file(&self, key: &str, path: &Path) -> Result<()> {
        self.0.put_file(key, path)
    }
    fn put(&self, key: &str, bytes: &[u8]) -> Result<()> {
        self.0.put(key, bytes)
    }
    fn get_range(&self, key: &str, range: Range<u64>) -> Result<Vec<u8>> {
        self.0.get_range(key, range)
    }
    fn delete(&self, key: &str) -> Result<()> {
        self.0.delete(key)
    }
    fn describe(&self) -> String {
        self.0.describe()
    }
}

#[test]
fn a_store_is_probed_before_it_is_trusted() {
    let dir = tmp("probe");
    let s = store(&dir);
    check_store(&*s).unwrap();
    assert!(s.list("_check/").unwrap().is_empty(), "the probe leaves nothing");
    let e = check_store(&Unconditional(DirStore::new(dir.join("u")).unwrap())).unwrap_err();
    assert!(e.to_string().contains("created an object that existed"), "{e}");
    let e = check_store(&Plain(DirStore::new(dir.join("p")).unwrap())).unwrap_err();
    assert!(e.to_string().contains("no conditional writes"), "{e}");
    assert!(Tenant::create(Arc::new(Plain(DirStore::new(dir.join("p")).unwrap())), "t", node(&dir, "a"), notes).is_err());
}

/// Two nodes on S3 or a store that speaks its API, at the URL in
/// `CHLOEDB_TENANT_S3_URL` (`s3://bucket/prefix/`, configured by
/// environment as [`chloedb::objects`] says): the store is probed, then
/// written by both in turn through sealed logs, bases and cleanup, and
/// the tenant deleted. Skipped without the URL.
#[cfg(feature = "s3")]
#[test]
fn tenants_on_s3() {
    let Ok(url) = chloedb::env::var("CHLOEDB_TENANT_S3_URL") else {
        eprintln!("CHLOEDB_TENANT_S3_URL is not set: skipped");
        return;
    };
    let url = format!("{}/{}-{:x}/", url.trim_end_matches('/'), std::process::id(), rand::random::<u32>());
    let s = chloedb::storage::open_store(&url).unwrap();
    check_store(&*s).unwrap();
    let dir = tmp("s3");
    let small = |name: &str| TenantOptions { tail_max: 300, base_after_txns: 8, keep: Duration::ZERO, ..node(&dir, name) };
    let a = Tenant::create(s.clone(), "t", small("a"), notes).unwrap();
    assert!(matches!(Tenant::create(s.clone(), "t", small("x"), notes), Err(TenantError::Exists)));
    for id in 1..=20 {
        insert(&a, id).unwrap();
    }
    let b = Tenant::open(s.clone(), "t", small("b")).unwrap();
    assert_eq!(ids(&b), set(1..=20));
    for id in 21..=40 {
        insert(&b, id).unwrap();
    }
    assert!(matches!(insert(&a, 99), Err(TenantError::NotWriter)));
    assert_eq!(ids(&a), set(1..=40));
    insert(&a, 41).unwrap();
    a.make_base().unwrap();
    // The store holds what HEAD names and nothing else of the tenant.
    let head = a.head().unwrap();
    let have: BTreeSet<String> = s.list(&format!("t/{}/", head.incarnation)).unwrap().into_iter().map(|k| k["t/".len()..].to_string()).collect();
    assert_eq!(have, named(&head));
    assert_eq!(ids(&Tenant::open(s.clone(), "t", small("c")).unwrap()), set(1..=41));
    assert_eq!(chloedb::tenant::list(&*s).unwrap(), ["t"]);
    assert!(Tenant::delete(&*s, "t").unwrap());
    assert!(chloedb::tenant::list(&*s).unwrap().is_empty());
    assert!(s.list("t/").unwrap().is_empty() && !Tenant::exists(&*s, "t").unwrap());
    assert!(matches!(insert(&b, 50), Err(TenantError::Missing)));
}
