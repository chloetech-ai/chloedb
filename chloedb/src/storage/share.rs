//! Sharing a database with readers in other processes: the file locks, the
//! writer's session and the readers' pins.
//!
//! One process writes a database; any number of others open it read-only
//! ([`crate::storage::DbOptions::read_only`]) and see its commits as they
//! are made, by reading the same files.
//!
//! ```text
//! one file X                     directory layout D/
//! X                              D/LOCK       flock shared by the writer and every reader
//! X-readers/WRITER               D/WRITER     flock exclusive by the writer; its session
//! X-readers/<pid>-<id>.pin       (none)       one per reader, flock exclusive by it:
//!                                             the oldest version it may read
//! ```
//!
//! * The writer takes the database file (`LOCK` in the directory layout)
//!   shared and `WRITER` exclusively, so a second writer finds `WRITER`
//!   taken and fails with `Busy`. Versions of Strata from before readers
//!   take the database file exclusively: such a writer and a writer or
//!   reader of this version keep each other out, so a reader never reads a
//!   file whose writer does not know about pins.
//! * The writer writes a random session id into `WRITER` once it holds it
//!   and before it changes anything; a reader that sees the session change
//!   reads the database again from the start, since opening may cut a torn
//!   log tail or start the log again.
//! * A reader of the single file registers a pin (version 0: every page is
//!   kept) before it reads a meta slot, and keeps it at most the oldest
//!   version it may still read: its snapshots' versions and the version it
//!   holds. A commit makes the pages a version freed reusable only once the
//!   newer meta slot names a later version and every pin is past it, as for
//!   the snapshots of its own process ([`crate::storage::freelist`]); so a
//!   version a reader reads (one in a meta slot when the reader's pin was
//!   already in place) keeps every page it reaches, and the file never gets
//!   shorter than they are.
//! * A pin's flock says its reader is alive. The writer removes a pin whose
//!   lock is free (its process ended) when it next reads them.
//! * The directory layout needs no pins: its files are written once and
//!   only unlinked, and a reader that finds one gone reads `CURRENT` again.
//!
//! A pin or a session is 24 bytes: an 8-byte tag, the value (u64), the pid
//! (u32) and a crc32 of the 20 bytes before it.

use std::fs::{File, OpenOptions};
use std::io::ErrorKind;
use std::os::unix::fs::FileExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::error::{Error, Result};

/// The writer's lock and session, in the share directory.
pub const WRITER: &str = "WRITER";
const PIN_TAG: &[u8; 8] = b"STRPIN01";
const SESSION_TAG: &[u8; 8] = b"STRWRT01";
const RECORD: usize = 24;
const PIN_SUFFIX: &str = ".pin";

/// How long an open waits for a lock that is held.
pub const LOCK_WAIT: Duration = Duration::from_millis(500);

/// Where the writer's lock and the readers' pins are kept: `<file>-readers`
/// beside a single file, the directory itself in the directory layout.
pub fn share_dir(path: &Path, directory: bool) -> PathBuf {
    if directory {
        return path.to_path_buf();
    }
    let mut s = path.as_os_str().to_owned();
    s.push("-readers");
    PathBuf::from(s)
}

/// Removes what [`share_dir`] holds for the database at `path`, which no
/// process may have open (a copy being made, which nobody else knows of):
/// the directory beside a single file, `WRITER` in the directory layout.
pub fn remove(path: &Path, directory: bool) {
    let r = match directory {
        true => std::fs::remove_file(path.join(WRITER)),
        false => std::fs::remove_dir_all(share_dir(path, false)),
    };
    if let Err(e) = r {
        if e.kind() != ErrorKind::NotFound {
            tracing::warn!(path = %path.display(), error = %e, "could not remove the writer's lock of a copy");
        }
    }
}

fn record(tag: &[u8; 8], value: u64) -> [u8; RECORD] {
    let mut b = [0u8; RECORD];
    b[..8].copy_from_slice(tag);
    b[8..16].copy_from_slice(&value.to_le_bytes());
    b[16..20].copy_from_slice(&std::process::id().to_le_bytes());
    let crc = crc32fast::hash(&b[..20]);
    b[20..].copy_from_slice(&crc.to_le_bytes());
    b
}

/// The value and pid of a record; `None` for one that is torn or not one.
fn parse(tag: &[u8; 8], b: &[u8]) -> Option<(u64, u32)> {
    if b.len() < RECORD || &b[..8] != tag || crc32fast::hash(&b[..20]).to_le_bytes() != b[20..RECORD] {
        return None;
    }
    Some((u64::from_le_bytes(b[8..16].try_into().unwrap()), u32::from_le_bytes(b[16..20].try_into().unwrap())))
}

/// Reads a record, again while it is torn by a write under way; `None` if
/// it stays torn.
fn read_record(f: &File, tag: &[u8; 8]) -> std::io::Result<Option<(u64, u32)>> {
    let mut b = [0u8; RECORD];
    for _ in 0..3 {
        let n = read_up_to(f, &mut b)?;
        if let Some(r) = parse(tag, &b[..n]) {
            return Ok(Some(r));
        }
        std::thread::yield_now();
    }
    Ok(None)
}

fn read_up_to(f: &File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match f.read_at(&mut buf[n..], n as u64)? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

/// One non-blocking `flock`: whether it was taken.
fn try_flock(f: &File, op: libc::c_int) -> std::io::Result<bool> {
    // SAFETY: flock on a valid descriptor.
    if unsafe { libc::flock(f.as_raw_fd(), op | libc::LOCK_NB) } == 0 {
        return Ok(true);
    }
    let e = std::io::Error::last_os_error();
    match e.raw_os_error() {
        Some(libc::EWOULDBLOCK) => Ok(false),
        _ => Err(e),
    }
}

/// A `flock` held elsewhere is retried for [`LOCK_WAIT`]: a child process
/// holds a copy of every descriptor of its parent from its fork until its
/// exec closes them, so a database that a process closes and opens again
/// while it starts a child can find its own lock still held for a moment.
fn flock_wait(f: &File, op: libc::c_int) -> std::io::Result<bool> {
    let deadline = Instant::now() + LOCK_WAIT;
    loop {
        if try_flock(f, op)? {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// The byte of the database file (or `LOCK`) the writer holds a write
/// lock on, far past any page: a record lock of the file itself, so that
/// a writer that reaches it by another name (a symlink, a hard link, a
/// second mount, or after its `-readers` directory was deleted) finds it
/// held. Readers take none, and it does not meet `flock`.
const WRITER_BYTE: libc::off_t = 0x3fff_ffff_ffff_fff0;

/// One non-blocking write lock on [`WRITER_BYTE`] of `f`: whether it was
/// taken. On Linux an open file description's lock (`F_OFD_SETLK`), which
/// a second open of the file in the same process also meets; elsewhere a
/// process's lock (`F_SETLK`), which only other processes meet, and which
/// closing any descriptor of the file in this process lets go of (the
/// `WRITER` lock still keeps a second writer of this process out).
fn try_writer_byte(f: &File) -> std::io::Result<bool> {
    // SAFETY: a zeroed flock is a valid argument, filled in below.
    let mut l: libc::flock = unsafe { std::mem::zeroed() };
    l.l_type = libc::F_WRLCK as _;
    l.l_whence = libc::SEEK_SET as _;
    l.l_start = WRITER_BYTE;
    l.l_len = 1;
    #[cfg(target_os = "linux")]
    let cmd = libc::F_OFD_SETLK;
    #[cfg(not(target_os = "linux"))]
    let cmd = libc::F_SETLK;
    // SAFETY: fcntl on a valid descriptor with a valid flock.
    if unsafe { libc::fcntl(f.as_raw_fd(), cmd, &l) } == 0 {
        return Ok(true);
    }
    let e = std::io::Error::last_os_error();
    match e.raw_os_error() {
        Some(libc::EAGAIN) | Some(libc::EACCES) => Ok(false),
        _ => Err(e),
    }
}

/// [`try_writer_byte`], retried for [`LOCK_WAIT`] as [`flock_wait`] is.
fn writer_byte_wait(f: &File) -> std::io::Result<bool> {
    let deadline = Instant::now() + LOCK_WAIT;
    loop {
        if try_writer_byte(f)? {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn elsewhere() -> Error {
    Error::Busy("the database is open elsewhere".into())
}

/// The lock of a database being created (no one else can know of it yet)
/// or of a version of Strata from before readers: exclusive.
pub fn lock_exclusive(f: &File) -> Result<()> {
    if flock_wait(f, libc::LOCK_EX)? {
        Ok(())
    } else {
        Err(elsewhere())
    }
}

/// Takes `main` (the database file, or `LOCK`) shared, as a reader. Fails
/// with `Busy` while a version of Strata from before readers has the
/// database open, which holds it exclusively.
pub fn lock_shared(main: &File) -> Result<()> {
    if flock_wait(main, libc::LOCK_SH)? {
        Ok(())
    } else {
        Err(Error::Busy("the database is open elsewhere, by a version of Strata that does not let other processes read it".into()))
    }
}

/// The writer's hold on a database: `main` shared, a write lock on a byte
/// of `main` ([`WRITER_BYTE`]), and `WRITER` exclusive.
pub struct WriterLock {
    _file: File,
    session: u64,
}

impl WriterLock {
    /// Takes the database for writing: `main` (the database file, or
    /// `LOCK`) shared, then `WRITER` in `share` exclusively, and writes a
    /// new session into it. `Busy` while another writer has it.
    pub fn take(main: &File, share: &Path) -> Result<WriterLock> {
        if !flock_wait(main, libc::LOCK_SH)? {
            return Err(elsewhere());
        }
        // The file's own lock first: `share` is found by the path, which
        // another name of the file does not share. Held as long as `main`'s
        // description is open (the database keeps it).
        if !writer_byte_wait(main)? {
            return Err(elsewhere());
        }
        match std::fs::create_dir(share) {
            Err(e) if e.kind() != ErrorKind::AlreadyExists => return Err(e.into()),
            _ => {}
        }
        let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(share.join(WRITER))?;
        if !flock_wait(&file, libc::LOCK_EX)? {
            return Err(elsewhere());
        }
        let session = loop {
            let s: u64 = rand::random();
            if s != 0 {
                break s;
            }
        };
        // Readers compare it with what they read last; it need not be
        // durable (they are alive, and read the page cache), and is not
        // part of what a crash leaves (so not written through `io`).
        file.write_all_at(&record(SESSION_TAG, session), 0)?;
        Ok(WriterLock { _file: file, session })
    }

    pub fn session(&self) -> u64 {
        self.session
    }
}

/// A reader's view of `WRITER`: which writer's session the files are in.
pub struct Session {
    path: PathBuf,
    file: Option<File>,
}

impl Session {
    pub fn new(share: &Path) -> Session {
        Session { path: share.join(WRITER), file: None }
    }

    /// The session of the writer that last opened the database, 0 if none
    /// has (or its record is torn).
    pub fn read(&mut self) -> Result<u64> {
        if self.file.is_none() {
            match File::open(&self.path) {
                Ok(f) => self.file = Some(f),
                Err(e) if e.kind() == ErrorKind::NotFound => return Ok(0),
                Err(e) => return Err(e.into()),
            }
        }
        let f = self.file.as_ref().expect("opened above");
        Ok(read_record(f, SESSION_TAG)?.map_or(0, |(s, _)| s))
    }
}

/// A reader's pin: a file in the share directory, locked by the reader for
/// as long as it lives, holding the oldest version it may read.
pub struct ReaderPin {
    path: PathBuf,
    file: File,
    value: u64,
}

impl ReaderPin {
    /// Registers a reader in `share` (made if missing), pinning every
    /// version. `None` on a read-only file system, where no writer can
    /// change the database either.
    pub fn register(share: &Path) -> Result<Option<ReaderPin>> {
        match std::fs::create_dir(share) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
            Err(e) if e.raw_os_error() == Some(libc::EROFS) => return Ok(None),
            Err(e) => return Err(Error::Io(std::io::Error::new(e.kind(), format!("a reader registers in {}: {e}", share.display())))),
        }
        for _ in 0..8 {
            let id = format!("{}-{:016x}", std::process::id(), rand::random::<u64>());
            let tmp = share.join(format!(".{id}.tmp"));
            let file = match OpenOptions::new().read(true).write(true).create_new(true).open(&tmp) {
                Ok(f) => f,
                Err(e) if e.raw_os_error() == Some(libc::EROFS) => return Ok(None),
                Err(e) => return Err(Error::Io(std::io::Error::new(e.kind(), format!("a reader registers in {}: {e}", share.display())))),
            };
            // A new file: nobody else holds it, so the lock is taken at once.
            if !try_flock(&file, libc::LOCK_EX)? {
                continue;
            }
            file.write_all_at(&record(PIN_TAG, 0), 0)?;
            let path = share.join(format!("{id}{PIN_SUFFIX}"));
            // Named once it is locked and pinned: a writer that reads the pins
            // takes a named one whose lock is free for a reader that is gone.
            // One that took the temporary name meanwhile for such a leftover
            // removed it, and the rename fails: register again.
            match std::fs::rename(&tmp, &path) {
                Ok(()) => return Ok(Some(ReaderPin { path, file, value: 0 })),
                Err(e) if e.kind() == ErrorKind::NotFound => continue,
                Err(e) => {
                    let _ = std::fs::remove_file(&tmp);
                    return Err(e.into());
                }
            }
        }
        Err(Error::Busy(format!("could not register a reader in {}", share.display())))
    }

    /// The version pinned.
    pub fn value(&self) -> u64 {
        self.value
    }

    /// Pins version `v` and those after it. Lowering a pin protects nothing
    /// that a commit may already have reused: only raise it past versions
    /// no longer read, or lower it while a version at least as old is
    /// still pinned.
    pub fn set(&mut self, v: u64) -> Result<()> {
        if v != self.value {
            self.file.write_all_at(&record(PIN_TAG, v), 0)?;
            self.value = v;
        }
        Ok(())
    }
}

impl Drop for ReaderPin {
    fn drop(&mut self) {
        // The lock goes with the descriptor, after the name.
        let _ = std::fs::remove_file(&self.path);
    }
}

/// A reader registered in a share directory, as the writer sees it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ReaderInfo {
    pub pid: u32,
    /// The oldest version it may read; `None` while its pin is unreadable,
    /// which keeps every version.
    pub pin: Option<u64>,
}

/// The readers registered in `share` that are alive, removing the pins of
/// those that are gone. Reading pins never fails a commit: a pin that
/// cannot be read counts as pinning everything.
pub fn readers(share: &Path) -> Vec<ReaderInfo> {
    let entries = match std::fs::read_dir(share) {
        Ok(it) => it,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let temporary = name.starts_with('.') && name.ends_with(".tmp");
        if !name.ends_with(PIN_SUFFIX) && !temporary {
            continue;
        }
        let Ok(f) = File::open(e.path()) else { continue };
        match try_flock(&f, libc::LOCK_SH) {
            // Its reader is gone (or, for a temporary name, is about to lock
            // it, and registers again when the rename fails).
            Ok(true) => {
                let _ = std::fs::remove_file(e.path());
            }
            Ok(false) if !temporary => match read_record(&f, PIN_TAG) {
                Ok(Some((pin, pid))) => out.push(ReaderInfo { pid, pin: Some(pin) }),
                _ => out.push(ReaderInfo { pid: 0, pin: None }),
            },
            Ok(false) => {}
            Err(_) => out.push(ReaderInfo { pid: 0, pin: None }),
        }
    }
    out
}

/// The oldest version a reader in another process may read (`Some(0)` for
/// one whose pin cannot be read), or `None` when there are no readers.
pub fn oldest_pin(share: &Path) -> Option<u64> {
    readers(share).iter().map(|r| r.pin.unwrap_or(0)).min()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_round_trip_and_tears_are_caught() {
        let r = record(PIN_TAG, 42);
        assert_eq!(parse(PIN_TAG, &r), Some((42, std::process::id())));
        assert_eq!(parse(SESSION_TAG, &r), None);
        for i in 0..RECORD {
            let mut bad = r;
            bad[i] ^= 1;
            assert_eq!(parse(PIN_TAG, &bad), None, "flip at byte {i}");
        }
        assert_eq!(parse(PIN_TAG, &r[..20]), None);
    }

    #[test]
    fn pins_are_seen_until_their_reader_goes() {
        let dir = tempfile::tempdir().unwrap();
        let share = share_dir(&dir.path().join("x.strata"), false);
        assert_eq!(oldest_pin(&share), None);
        let mut a = ReaderPin::register(&share).unwrap().unwrap();
        let mut b = ReaderPin::register(&share).unwrap().unwrap();
        assert_eq!(oldest_pin(&share), Some(0));
        a.set(7).unwrap();
        b.set(5).unwrap();
        assert_eq!(oldest_pin(&share), Some(5));
        drop(b);
        assert_eq!(oldest_pin(&share), Some(7));
        // A pin whose reader is gone without removing it: its lock is free.
        let left = share.join(format!("1-{:016x}{PIN_SUFFIX}", 1));
        std::fs::write(&left, record(PIN_TAG, 1)).unwrap();
        assert_eq!(oldest_pin(&share), Some(7));
        assert!(!left.exists());
        drop(a);
        assert_eq!(oldest_pin(&share), None);
        assert_eq!(std::fs::read_dir(&share).unwrap().count(), 0);
    }

    #[test]
    fn a_second_writer_is_busy_and_a_session_is_new_each_time() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.strata");
        std::fs::write(&path, b"").unwrap();
        let share = share_dir(&path, false);
        // The writer's descriptor is open for writing, as the database's
        // is: the lock on WRITER_BYTE is a write lock.
        let rw = || OpenOptions::new().read(true).write(true).open(&path).unwrap();
        let main = rw();
        let mut seen = Session::new(&share);
        assert_eq!(seen.read().unwrap(), 0);
        let w = WriterLock::take(&main, &share).unwrap();
        assert_eq!(seen.read().unwrap(), w.session());
        let other = rw();
        assert!(matches!(WriterLock::take(&other, &share), Err(Error::Busy(_))));
        // A reader shares the file with the writer.
        lock_shared(&File::open(&path).unwrap()).unwrap();
        let s = w.session();
        // The byte lock lives as long as the writer's descriptor of the
        // file, which the database closes with its lock.
        drop(w);
        drop(main);
        let w = WriterLock::take(&other, &share).unwrap();
        assert_ne!(w.session(), s);
        assert_eq!(seen.read().unwrap(), w.session());
        // An older Strata's writer holds the file exclusively: a reader
        // keeps it out, and is kept out by it.
        drop(w);
        let old = File::open(&path).unwrap();
        assert!(!try_flock(&old, libc::LOCK_EX).unwrap(), "the last writer's descriptor still holds it shared");
    }
}
