//! `chloedb-server restore`: a checkpoint put in place of a server's
//! database, offline, with what the checkpoint keeps beside it
//! ([`crate::service::Service::checkpoint`]).
//!
//! A checkpoint `<backup-dir>/<name>` is a database (one file, or a
//! directory of the directory layout), and beside it are the cluster map
//! its server held, `<name>.slots`, and its API keys, `<name>.keys.json`:
//! the names a server started with `--db <name>` reads. Restoring copies
//! the checkpoint beside the database, opens the copy and checks every
//! checksum, and only then moves the database's own files aside (into
//! `<db>.before-restore-<time>/`: the database, its `-wal`, `-readers`
//! and `-merging`, its part in replication, `<db>.replication`, and its
//! map, `<db>.slots`), puts the copy in its place and the map beside it,
//! and merges the checkpoint's keys into the server's keys file as a
//! cluster merges keys ([`KeySet::merged`]): a key revoked since the
//! checkpoint, which that file holds as revoked, stays revoked.
//!
//! The server starts again with the flags it had. Its part in replication
//! went aside with the old files, so a copy of a replicated shard starts in
//! the part its flags give it (`--replicas` on the shard's primary,
//! `--replica` on the others) as one of its shard's first copies: every
//! copy restored from the same checkpoint holds the same transactions, and
//! a copy started on an empty disk is sent a copy by its primary.

use std::path::{Path, PathBuf};

use chloedb::storage::{Database, DbOptions, SeqNo};
use serde_json::Value as Json;

use crate::keys::{KeySet, Keys};
use crate::slots::{self, SlotMap};

/// What [`restore`] is asked to do.
#[derive(Clone, Debug)]
pub struct Restore {
    /// The checkpoint, `<backup-dir>/<name>`.
    pub from: PathBuf,
    /// The database it takes the place of (`--db`); it need not exist.
    pub db: PathBuf,
    /// The server's keys file (default `<db>.keys.json`, as `--keys`).
    pub keys: Option<PathBuf>,
    /// Restore a checkpoint of one shard onto a copy whose kept map says
    /// it is a copy of another.
    pub force: bool,
}

/// The cluster map kept beside a checkpoint or a database: the shard it
/// is of, and the map.
#[derive(Clone, Debug)]
pub struct KeptMap {
    pub shard: usize,
    pub map: SlotMap,
}

impl KeptMap {
    /// The map kept at `path` (as `<db>.slots` holds it), if there is one.
    pub fn read(path: &Path) -> Result<Option<KeptMap>, String> {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let j: Json = serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        let map = SlotMap::from_json(&j["map"]).map_err(|e| format!("{}: {e}", path.display()))?;
        let shard = j["shard"].as_u64().map(|s| s as usize).filter(|&s| s < map.shards()).ok_or_else(|| format!("{}: no shard of the map", path.display()))?;
        Ok(Some(KeptMap { shard, map }))
    }
}

/// What [`restore`] did.
#[derive(Clone, Debug)]
pub struct Restored {
    /// The checkpoint is a directory of the directory layout.
    pub directory: bool,
    pub tables: usize,
    pub rows: u64,
    /// Its last transaction, when it numbers its writes (`--seq-no`).
    pub at: Option<SeqNo>,
    /// The cluster map beside it.
    pub map: Option<KeptMap>,
    /// The keys file it merged the checkpoint's keys into, and the keys
    /// and revocations that file holds now.
    pub keys: Option<(PathBuf, usize, usize)>,
    /// Where the files it replaced went.
    pub aside: Option<PathBuf>,
}

impl Restored {
    /// What `chloedb-server restore` says it did, a line each.
    pub fn lines(&self, r: &Restore) -> Vec<String> {
        let mut out = Vec::new();
        let at = self.at.map_or(String::new(), |s| format!(", last transaction {} of term {}", s.seq, s.term));
        out.push(format!(
            "restored {} from the checkpoint {} ({}): {} {}, {} rows{at}",
            r.db.display(),
            r.from.display(),
            if self.directory { "a directory" } else { "one file" },
            self.tables,
            if self.tables == 1 { "table" } else { "tables" },
            self.rows
        ));
        out.push(match &self.map {
            Some(k) => format!("its cluster map: shard {} of {}, epoch {}, in {}", k.shard, k.map.shards(), k.map.epoch, slots::file_of(&r.db).display()),
            None => "no cluster map beside the checkpoint (a server that is no shard of a cluster, or a checkpoint from before 0.3.8): a coordinator gives one".to_string(),
        });
        out.push(match &self.keys {
            Some((file, keys, revoked)) => format!("its API keys merged into {}: {keys} keys and {revoked} revoked there now (a key revoked since the checkpoint stays revoked where that file says so)", file.display()),
            None => "no API keys beside the checkpoint: the server keeps its own keys file as it is".to_string(),
        });
        if let Some(a) = &self.aside {
            out.push(format!("the files it replaced are in {} (delete them once the server serves again)", a.display()));
        }
        out.push(match &self.map {
            Some(_) => "next: start the server with the flags it had (--replicas on its shard's primary, --replica on the shard's other copies); restore every copy of the shard from the same checkpoint, or start the others on empty disks and the primary sends each a copy".to_string(),
            None => "next: start the server with the flags it had".to_string(),
        });
        out
    }
}

/// The files of the database at `db` that a restore replaces: the
/// database, its write-ahead log, readers' pins and merge record, and
/// what its server kept of replication and the cluster map.
fn replaced(db: &Path) -> Vec<PathBuf> {
    let with = |s: &str| {
        let mut p = db.as_os_str().to_owned();
        p.push(s);
        PathBuf::from(p)
    };
    [db.to_path_buf(), with("-wal"), with("-readers"), with("-merging"), crate::replication::file_of(db), slots::file_of(db)].into_iter().filter(|p| p.symlink_metadata().is_ok()).collect()
}

/// Whether a process has the database at `db` open: it holds the file
/// (or the directory's `LOCK`) shared, which an exclusive lock waits for.
fn in_use(db: &Path) -> Result<bool, String> {
    let main = match db.is_dir() {
        true => db.join(chloedb::storage::dir::LOCK),
        false => db.to_path_buf(),
    };
    let file = match std::fs::File::open(&main) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(format!("{}: {e}", main.display())),
    };
    match chloedb::storage::share::lock_exclusive(&file) {
        Ok(()) => Ok(false),
        Err(chloedb::Error::Busy(_)) => Ok(true),
        Err(e) => Err(format!("{}: {e}", main.display())),
    }
}

/// Copies `from` (a file, or a directory and everything in it) to `to`.
fn copy_all(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        std::fs::create_dir(to)?;
        for e in std::fs::read_dir(from)? {
            let e = e?;
            copy_all(&e.path(), &to.join(e.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to)?;
        std::fs::File::open(to)?.sync_all()
    }
}

fn remove_all(p: &Path) {
    let _ = match p.is_dir() {
        true => std::fs::remove_dir_all(p),
        false => std::fs::remove_file(p),
    };
}

/// What a copy of a checkpoint holds, read without changing it, every
/// checksum checked: its tables, rows and last transaction.
fn check(copy: &Path, directory: bool) -> Result<(usize, u64, Option<SeqNo>), String> {
    // A read-only open of the directory layout shares its `LOCK`, which a
    // checkpoint leaves out: the copy is this restore's alone.
    if directory && !copy.join(chloedb::storage::dir::LOCK).exists() {
        std::fs::File::create(copy.join(chloedb::storage::dir::LOCK)).map_err(|e| e.to_string())?;
    }
    let opened = (|| {
        let db = Database::open(copy, DbOptions { read_only: true, ..DbOptions::default() })?;
        db.check()?;
        let stats = db.file_stats();
        let rows = stats.segments.iter().map(|s| u64::from(s.n_docs.saturating_sub(s.n_deleted))).sum::<u64>() + stats.memtable_docs as u64;
        Ok::<_, chloedb::Error>((db.tables().iter().filter(|t| !t.is_placeholder()).count(), rows, db.numbering().map(|n| n.at())))
    })();
    // What the open made beside a single file: its readers' pins.
    chloedb::storage::share::remove(copy, directory);
    if !directory {
        let mut wal = copy.as_os_str().to_owned();
        wal.push("-wal");
        let wal = PathBuf::from(wal);
        if wal.metadata().is_ok_and(|m| m.len() == 0) {
            let _ = std::fs::remove_file(wal);
        }
    }
    opened.map_err(|e| e.to_string())
}

/// Puts the checkpoint `r.from` in place of the database `r.db`, with the
/// cluster map and keys beside it (see the module). Nothing changes until
/// a copy of the checkpoint was checked whole; a server must not have the
/// database open.
pub fn restore(r: &Restore) -> Result<Restored, String> {
    let from = &r.from;
    if from.symlink_metadata().is_err() {
        return Err(format!("no checkpoint at {}", from.display()));
    }
    let directory = from.is_dir();
    if directory && !from.join("CURRENT").exists() {
        return Err(format!("{} is a directory but no database of the directory layout (it has no CURRENT)", from.display()));
    }
    let map = KeptMap::read(&slots::file_of(from))?;
    let keys = match std::fs::read(Keys::file_of(from)) {
        Ok(b) => Some(KeySet::from_json(&b).map_err(|e| format!("{}: {e}", Keys::file_of(from).display()))?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("{}: {e}", Keys::file_of(from).display())),
    };
    let db = &r.db;
    if let (Some(theirs), Some(ours)) = (&map, KeptMap::read(&slots::file_of(db))?) {
        if theirs.shard != ours.shard && !r.force {
            return Err(format!(
                "the checkpoint is of shard {} and {} is a copy of shard {} (its {}): restore that shard's checkpoint, or pass --force to restore this one",
                theirs.shard,
                db.display(),
                ours.shard,
                slots::file_of(db).display()
            ));
        }
    }
    if in_use(db)? {
        return Err(format!("a server has {} open: stop it first (for a shard, every copy of it)", db.display()));
    }
    let name = db.file_name().ok_or_else(|| format!("{} names no file", db.display()))?.to_string_lossy().to_string();
    let parent = db.parent().filter(|p| !p.as_os_str().is_empty()).map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    std::fs::create_dir_all(&parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let tmp = parent.join(format!(".{name}.restore-{nanos:x}"));
    if let Err(e) = copy_all(from, &tmp) {
        remove_all(&tmp);
        return Err(format!("copy {} to {}: {e}", from.display(), tmp.display()));
    }
    let (tables, rows, at) = match check(&tmp, directory) {
        Ok(c) => c,
        Err(e) => {
            remove_all(&tmp);
            return Err(format!("the checkpoint {} does not open whole, so nothing was changed: {e}", from.display()));
        }
    };
    let old = replaced(db);
    let aside = match old.is_empty() {
        true => None,
        false => {
            let stamp = chloedb::timestamp::strftime("%Y%m%d-%H%M%S", chloedb::timestamp::now()).unwrap_or_else(|_| format!("{}", nanos / 1_000_000_000));
            let mut dir = parent.join(format!("{name}.before-restore-{stamp}"));
            let mut n = 1;
            while dir.exists() {
                n += 1;
                dir = parent.join(format!("{name}.before-restore-{stamp}-{n}"));
            }
            std::fs::create_dir(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
            for p in &old {
                let to = dir.join(p.file_name().unwrap_or_default());
                if let Err(e) = std::fs::rename(p, &to) {
                    remove_all(&tmp);
                    return Err(format!("move {} aside to {}: {e} (what moved before it is in {})", p.display(), to.display(), dir.display()));
                }
            }
            Some(dir)
        }
    };
    let keys_file = r.keys.clone().unwrap_or_else(|| Keys::file_of(db));
    if let (Some(dir), true) = (&aside, keys_file.exists()) {
        let _ = std::fs::copy(&keys_file, dir.join(keys_file.file_name().unwrap_or_default()));
    }
    std::fs::rename(&tmp, db).map_err(|e| format!("move the checked copy {} to {}: {e}", tmp.display(), db.display()))?;
    if map.is_some() {
        std::fs::copy(slots::file_of(from), slots::file_of(db)).map_err(|e| format!("copy the cluster map to {}: {e}", slots::file_of(db).display()))?;
    }
    let keys = match keys {
        Some(set) => {
            let k = Keys::open(keys_file.clone())?;
            k.merge(&set).map_err(|e| e.message)?;
            let now = k.set();
            Some((keys_file, now.keys.len(), now.revoked.len()))
        }
        None => None,
    };
    Ok(Restored { directory, tables, rows, at, map, keys, aside })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::Scope;
    use chloedb::db::Db;
    use chloedb::query::Params;
    use chloedb::storage::{CheckpointOptions, Layout};

    fn with(p: &Path, s: &str) -> PathBuf {
        let mut o = p.as_os_str().to_owned();
        o.push(s);
        PathBuf::from(o)
    }

    /// A database of `n` rows at `path`, closed.
    fn make(path: &Path, layout: Layout, from: usize, n: usize) {
        let opts = DbOptions { layout, ..DbOptions::default() };
        let db = match path.exists() {
            true => Db::open(path, opts).unwrap(),
            false => {
                let db = Db::create_empty(path, opts).unwrap();
                db.sql("CREATE TABLE t (id KEYWORD PRIMARY KEY, n INT)", &Params::new()).unwrap();
                db
            }
        };
        for i in from..from + n {
            db.sql(&format!("INSERT INTO t VALUES ('r{i:04}', {i})"), &Params::new()).unwrap();
        }
        db.close().unwrap();
    }

    fn count(path: &Path) -> u64 {
        let db = Db::open(path, DbOptions::default()).unwrap();
        let r = db.sql("SELECT id FROM t LIMIT 10000", &Params::new()).unwrap();
        db.close().unwrap();
        match r {
            chloedb::db::SqlResult::Rows(r) => r.rows.len() as u64,
            chloedb::db::SqlResult::Written(_) => panic!("a SELECT wrote"),
        }
    }

    fn checkpoint(db: &Path, to: &Path, layout: Layout) {
        let d = Database::open(db, DbOptions { layout, ..DbOptions::default() }).unwrap();
        d.checkpoint(to, &CheckpointOptions { layout, link_from: None }).unwrap();
        drop(d);
    }

    fn map_file(path: &Path, shard: usize, epoch: u64) {
        let map = SlotMap::even(2, epoch);
        std::fs::write(slots::file_of(path), serde_json::to_vec(&serde_json::json!({"shard": shard, "since": 1, "map": map.to_json()})).unwrap()).unwrap();
    }

    #[test]
    fn restores_either_layout_in_place_of_a_database_with_its_map_and_keys() {
        for layout in [Layout::SingleFile, Layout::Directory] {
            let dir = tempfile::tempdir().unwrap();
            let db = dir.path().join("shop.chloedb");
            let cp = dir.path().join("nightly");
            make(&db, layout, 0, 30);
            checkpoint(&db, &cp, layout);
            map_file(&cp, 1, 3);
            // The cluster's keys at the checkpoint: two; one revoked since,
            // in the server's own file.
            let at_cp = Keys::open(Keys::file_of(&cp)).unwrap();
            let (kept, _) = at_cp.create("dashboards", Scope::Read, None, true, false).unwrap();
            let (gone, _) = at_cp.create("old-job", Scope::Write, None, true, false).unwrap();
            let ours = Keys::open(Keys::file_of(&db)).unwrap();
            ours.merge(&at_cp.set()).unwrap();
            ours.revoke(&gone.id, true).unwrap();
            drop(ours);
            // Writes after the checkpoint, and the copy's own map.
            make(&db, layout, 30, 20);
            map_file(&db, 1, 4);
            assert_eq!(count(&db), 50);

            let r = Restore { from: cp.clone(), db: db.clone(), keys: None, force: false };
            let done = restore(&r).unwrap();
            assert_eq!((done.directory, done.tables, done.rows), (layout == Layout::Directory, 1, 30), "{layout:?}");
            assert_eq!(done.map.as_ref().map(|m| (m.shard, m.map.epoch)), Some((1, 3)));
            let aside = done.aside.clone().unwrap();
            assert!(aside.join("shop.chloedb").exists() && aside.join("shop.chloedb.slots").exists() && aside.join("shop.chloedb.keys.json").exists(), "{:?}", std::fs::read_dir(&aside).unwrap().collect::<Vec<_>>());
            assert_eq!(KeptMap::read(&slots::file_of(&db)).unwrap().map(|m| m.map.epoch), Some(3));
            let lines = done.lines(&r).join("\n");
            assert!(lines.contains("30 rows") && lines.contains("shard 1 of 2, epoch 3") && lines.contains("before-restore"), "{lines}");
            // The checkpoint's rows, and its keys merged: the one revoked
            // since stays revoked.
            assert_eq!(count(&db), 30, "{layout:?}");
            let now = Keys::open(Keys::file_of(&db)).unwrap().set();
            assert!(now.keys.iter().any(|k| k.id == kept.id));
            assert!(now.revoked.iter().any(|k| k.id == gone.id), "{:?}", now.revoked);
            // The checkpoint is as it was, for the next copy.
            assert!(cp.exists() && slots::file_of(&cp).exists() && !with(&cp, "-wal").exists());
            // No copy of it left beside the database.
            let left: Vec<_> = std::fs::read_dir(dir.path()).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.contains(".restore-")).collect();
            assert!(left.is_empty(), "{left:?}");
        }
    }

    #[test]
    fn a_new_disk_is_made_from_the_checkpoint() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("a.chloedb");
        make(&src, Layout::SingleFile, 0, 12);
        let cp = dir.path().join("cp");
        checkpoint(&src, &cp, Layout::SingleFile);
        let db = dir.path().join("new").join("b.chloedb");
        let done = restore(&Restore { from: cp, db: db.clone(), keys: None, force: false }).unwrap();
        assert!(done.aside.is_none() && done.map.is_none() && done.keys.is_none());
        assert_eq!(count(&db), 12);
    }

    #[test]
    fn refuses_another_shard_an_open_database_and_a_damaged_checkpoint() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("shop.chloedb");
        let cp = dir.path().join("nightly");
        make(&db, Layout::SingleFile, 0, 10);
        checkpoint(&db, &cp, Layout::SingleFile);
        map_file(&cp, 1, 3);
        map_file(&db, 0, 3);
        let r = Restore { from: cp.clone(), db: db.clone(), keys: None, force: false };
        let e = restore(&r).unwrap_err();
        assert!(e.contains("the checkpoint is of shard 1") && e.contains("--force"), "{e}");

        std::fs::remove_file(slots::file_of(&db)).unwrap();
        let open = Database::open(&db, DbOptions::default()).unwrap();
        let e = restore(&r).unwrap_err();
        assert!(e.contains("stop it first"), "{e}");
        drop(open);

        // A checkpoint cut short (a copy that stopped half way): nothing
        // changes.
        let bad = dir.path().join("bad");
        let bytes = std::fs::read(&cp).unwrap();
        std::fs::write(&bad, &bytes[..bytes.len() / 3]).unwrap();
        let before = std::fs::read(&db).unwrap();
        let e = restore(&Restore { from: bad, db: db.clone(), keys: None, force: false }).unwrap_err();
        assert!(e.contains("nothing was changed"), "{e}");
        assert_eq!(std::fs::read(&db).unwrap(), before);
        assert!(std::fs::read_dir(dir.path()).unwrap().filter_map(|e| e.ok()).all(|e| !e.file_name().to_string_lossy().contains("restore")));

        // Forced onto the other shard's copy, it restores.
        map_file(&db, 0, 3);
        let done = restore(&Restore { force: true, ..r }).unwrap();
        assert_eq!(done.rows, 10);
    }
}
