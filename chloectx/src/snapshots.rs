//! Kept snapshots: copies of the store, by name, that a receipt's recall
//! can be replayed on ([`crate::receipt`]).
//!
//! Each is a checkpoint of the store in ChloeDB's directory layout
//! (`Database::checkpoint`), under `snapshots/<name>/` in the store's
//! directory. A checkpoint hard-links the segment files of the one before
//! it that no merge has replaced since, so a snapshot after a turn costs
//! the records written in that turn, not a copy of the store. Nothing in
//! the store's own file changes: a kept snapshot is a backup that is itself
//! a database, opened read-only to replay.
//!
//! `snapshots/index.json` lists them: the name, the commit it holds, the
//! commit the store was at when it was taken (the two differ by the flush
//! the checkpoint makes, which changes no row), the time, and whether it
//! was kept automatically (then pruned by age and count) or by name (kept
//! until dropped).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use chloedb::db::Db;
use chloedb::storage::{CheckpointOptions, Layout};

use crate::embed::Embedder;
use crate::store::Store;
use crate::{err, now, Result};

/// One kept snapshot.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Kept {
    pub name: String,
    /// The commit the copy holds.
    pub txn: u64,
    /// The commit readers of the store saw just before the copy was made:
    /// a receipt read at this commit replays exactly on this snapshot.
    pub at: u64,
    pub ts: i64,
    /// Kept automatically after a recall (pruned), or by name (kept).
    pub auto: bool,
    pub records: u64,
}

/// How long automatic snapshots are kept.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Retention {
    /// Keep a snapshot after each recall that showed records, when the
    /// store changed since the last one.
    pub after_recall: bool,
    /// At most one automatic snapshot this often (0: after every recall
    /// that showed records). Recalls in between replay on the nearest
    /// later one, which says how far it is.
    pub min_interval_secs: i64,
    /// Automatic snapshots older than this are dropped.
    pub keep_days: i64,
    /// At most this many automatic snapshots (the oldest go first).
    pub max_auto: usize,
}

impl Default for Retention {
    fn default() -> Self {
        Retention { after_recall: true, min_interval_secs: 0, keep_days: 30, max_auto: 2000 }
    }
}

/// The kept snapshots of one store.
pub struct Snapshots {
    dir: PathBuf,
    lock: Mutex<()>,
}

/// A name a person gives a snapshot: letters, digits, `.`, `_` and `-`,
/// not starting with `.`, at most 64 bytes.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && !name.starts_with('.') && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

impl Snapshots {
    /// The snapshots of the store in `store_dir`.
    pub fn new(store_dir: &Path) -> Snapshots {
        Snapshots { dir: store_dir.join("snapshots"), lock: Mutex::new(()) }
    }

    fn index_path(&self) -> PathBuf {
        self.dir.join("index.json")
    }

    /// Every kept snapshot, oldest first.
    pub fn list(&self) -> Vec<Kept> {
        let mut v: Vec<Kept> = std::fs::read_to_string(self.index_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        v.retain(|k| self.dir.join(&k.name).is_dir());
        v.sort_by_key(|k| (k.at, k.ts));
        v
    }

    fn write_index(&self, v: &[Kept]) -> Result<()> {
        let tmp = self.dir.join("index.json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(v).map_err(|e| err(e.to_string()))?)?;
        std::fs::rename(&tmp, self.index_path())?;
        Ok(())
    }

    /// The snapshot a recall read at commit `txn` replays on exactly, if
    /// one was kept.
    pub fn exact(&self, txn: u64) -> Option<Kept> {
        self.list().into_iter().find(|k| k.at == txn || k.txn == txn)
    }

    /// The earliest snapshot kept at or after commit `txn`.
    pub fn after(&self, txn: u64) -> Option<Kept> {
        self.list().into_iter().find(|k| k.at >= txn)
    }

    pub fn get(&self, name: &str) -> Option<Kept> {
        self.list().into_iter().find(|k| k.name == name)
    }

    /// Keeps a snapshot of `db` now: under `name`, or (automatic) named by
    /// its commit. An automatic one is not taken when the newest kept
    /// snapshot already holds the store's commit, nor within
    /// `min_interval` of the last automatic one.
    pub fn keep(&self, db: &Db, name: Option<&str>, r: &Retention) -> Result<Kept> {
        let _g = self.lock.lock().unwrap();
        crate::paths::create_private_dir(&self.dir)?;
        let at = db.snapshot().txn();
        let mut all = self.list();
        let auto = name.is_none();
        if auto {
            if let Some(k) = all.iter().rev().find(|k| k.at == at || k.txn == at) {
                return Ok(k.clone());
            }
            if r.min_interval_secs > 0 {
                if let Some(k) = all.iter().rev().find(|k| k.auto) {
                    if now() - k.ts < r.min_interval_secs {
                        return Ok(k.clone());
                    }
                }
            }
        }
        let name = match name {
            Some(n) if !valid_name(n) => return Err(err(format!("{n:?} is not a snapshot name: use letters, digits, '.', '_' and '-' (at most 64)"))),
            Some(n) if all.iter().any(|k| k.name == n) => return Err(err(format!("a snapshot named {n} is kept already"))),
            Some(n) => n.to_string(),
            None => format!("auto-{at}"),
        };
        let dest = self.dir.join(&name);
        if dest.exists() {
            // Left by a checkpoint the index never named (a crash between
            // the two): it is replaced.
            std::fs::remove_dir_all(&dest)?;
        }
        let link_from = all.iter().rev().map(|k| self.dir.join(&k.name)).find(|p| p.is_dir());
        let opts = CheckpointOptions { layout: Layout::Directory, link_from };
        let stats = db.storage().checkpoint(&dest, &opts)?;
        let k = Kept { name, txn: stats.txn, at, ts: now(), auto, records: stats.live_docs };
        all.push(k.clone());
        self.write_index(&all)?;
        Ok(k)
    }

    /// Drops the automatic snapshots past their retention.
    pub fn prune(&self, r: &Retention) -> Result<usize> {
        let _g = self.lock.lock().unwrap();
        let all = self.list();
        let autos: Vec<&Kept> = all.iter().filter(|k| k.auto).collect();
        let over = autos.len().saturating_sub(r.max_auto);
        let old = now() - r.keep_days.max(0) * 86_400;
        let gone: Vec<String> = autos.iter().enumerate().filter(|(i, k)| *i < over || k.ts < old).map(|(_, k)| k.name.clone()).collect();
        if gone.is_empty() {
            return Ok(0);
        }
        for n in &gone {
            let _ = std::fs::remove_dir_all(self.dir.join(n));
        }
        let left: Vec<Kept> = all.into_iter().filter(|k| !gone.contains(&k.name)).collect();
        self.write_index(&left)?;
        Ok(gone.len())
    }

    /// Drops a snapshot by name.
    pub fn drop_named(&self, name: &str) -> Result<bool> {
        let _g = self.lock.lock().unwrap();
        let all = self.list();
        if !all.iter().any(|k| k.name == name) {
            return Ok(false);
        }
        std::fs::remove_dir_all(self.dir.join(name))?;
        let left: Vec<Kept> = all.into_iter().filter(|k| k.name != name).collect();
        self.write_index(&left)?;
        Ok(true)
    }

    /// Opens a kept snapshot to read.
    pub fn open(&self, k: &Kept, embedder: Arc<dyn Embedder>) -> Result<Store> {
        Store::open_read_only(&self.dir.join(&k.name), embedder)
    }
}
