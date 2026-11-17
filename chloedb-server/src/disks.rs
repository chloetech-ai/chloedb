//! The disk under a database server, as the command center judges it:
//! how fast it reads now, how busy it is, how much room it has before
//! Kubernetes evicts the node's pods, and what fills it.
//!
//! - A probe (`--disk-probe-s`, every minute by default) reads the
//!   database's file around the page cache (`O_DIRECT`; `F_NOCACHE` on
//!   macOS): eight blocks of 4 KiB at random places, timed one by one, and
//!   4 MiB in a row, timed as a whole, two seconds at most. It reads what
//!   the queries read, on the disk they read it from, and evicts nothing
//!   from the cache. On Linux the device's own counters say how busy it
//!   was since the last probe (`/sys/dev/block/M:m/stat`): a slow disk
//!   that is not busy is the disk's fault, or the machine's under it, not
//!   the load's. The latest reading goes into the series
//!   (`disk_read_ms`, `disk_mb_s`, `disk_busy`), so that a coordinator
//!   compares its copies' disks ([`crate::fleet`]).
//! - Under Kubernetes, a database on the node's own file system (a
//!   `hostPath` or an `emptyDir`, which share it with the kubelet) is in
//!   the kubelet's way: it evicts every pod of the node once that file
//!   system has less than its eviction threshold free (`--evict-free-ratio`,
//!   15% by default, the kubelet's `imagefs.available`, which applies to
//!   the node's disk when images are kept there). The server tells it by
//!   the device of `/etc/hosts`, which the kubelet writes into each pod
//!   from its own directory, and reports the room left above that line
//!   (`disk_evict_headroom_bytes`).
//! - `GET /v1/admin/disk` ([`report`]) lists what fills the volume the
//!   database is on: the database's own files, and every other file and
//!   folder there with its size and when it last changed (a build's
//!   output, a copy left behind); `POST /v1/admin/disk/clean` ([`clean`])
//!   deletes those a person picked, never the server's own, and only as
//!   they were listed.

use std::collections::{HashSet, VecDeque};
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value as Json};

use crate::telemetry::now_ms;
use crate::ApiError;

/// How often the probe reads, by default.
pub const EVERY_S: u64 = 60;
/// The free share of a node's disk below which the kubelet evicts its
/// pods, by default (`imagefs.available<15%`).
pub const EVICT_FREE: f64 = 0.15;
const BLOCK: usize = 4096;
/// Random reads each probe times.
pub const READS: usize = 8;
/// Bytes each probe reads in a row, and in how many reads.
const SEQ: usize = 4 << 20;
const CHUNK: usize = 1 << 20;
/// The longest a probe reads.
const BUDGET: Duration = Duration::from_secs(2);
/// Readings kept: an hour at the default rate.
const KEPT: usize = 60;
/// The most entries a listing walks, and for how long.
const WALK_ENTRIES: usize = 200_000;
const WALK_FOR: Duration = Duration::from_secs(5);
/// Entries a listing shows, the largest first.
const SHOWN: usize = 40;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// One probe's reading.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Reading {
    pub at_ms: u64,
    /// The median of the random reads, and the slowest.
    pub read_ms: f64,
    pub read_max_ms: f64,
    pub reads: usize,
    /// The reads in a row, in MB a second (none from a file under 64 KiB).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mb_s: Option<f64>,
    /// The share of the time since the last probe the device had reads or
    /// writes in flight, and its reads' mean time (all of them, the
    /// queries' too): Linux only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub busy: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub await_ms: Option<f64>,
    /// A scrub read the disk meanwhile: a slow reading then says less.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub scrubbing: bool,
    pub took_ms: f64,
}

/// The device's counters at one moment.
#[derive(Clone, Copy, Debug)]
struct DevStat {
    at: Instant,
    reads: u64,
    read_ms: u64,
    io_ms: u64,
}

/// The probe of this server's disk, and where the database is.
pub struct Probe {
    /// How often it reads (zero: never).
    pub every: Duration,
    /// The kubelet's eviction threshold, as a free share.
    pub evict_free: f64,
    /// Added to each read it times (`CHLOEDB_DISK_PROBE_ADD_MS`), so that a
    /// test makes one copy's disk look slow.
    pub add_ms: f64,
    /// The database shares the node's file system under Kubernetes
    /// ([`on_node`]), and whether this runs under Kubernetes at all.
    on_node: AtomicBool,
    kube: AtomicBool,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    kept: VecDeque<Reading>,
    dev: Option<DevStat>,
    error: Option<(u64, String)>,
    running: bool,
}

impl Default for Probe {
    fn default() -> Probe {
        Probe {
            every: Duration::from_secs(EVERY_S),
            evict_free: EVICT_FREE,
            add_ms: std::env::var("CHLOEDB_DISK_PROBE_ADD_MS").ok().and_then(|v| v.parse().ok()).filter(|v: &f64| v.is_finite() && *v > 0.0).unwrap_or(0.0),
            on_node: AtomicBool::new(false),
            kube: AtomicBool::new(false),
            state: Mutex::new(State::default()),
        }
    }
}

/// Clears `running` however the probe ends.
struct Running<'a>(&'a Probe);

impl Drop for Running<'_> {
    fn drop(&mut self) {
        lock(&self.0.state).running = false;
    }
}

impl Probe {
    /// Learns where the database at `db` is: on the node's own file system
    /// under Kubernetes, or not.
    pub fn place(&self, db: &Path) {
        let kube = std::env::var_os("KUBERNETES_SERVICE_HOST").is_some();
        self.kube.store(kube, Ordering::Relaxed);
        self.on_node.store(kube && same_device(db, Path::new("/etc/hosts")), Ordering::Relaxed);
    }

    pub fn on_node(&self) -> bool {
        self.on_node.load(Ordering::Relaxed)
    }

    pub fn kube(&self) -> bool {
        self.kube.load(Ordering::Relaxed)
    }

    /// Reads the disk now (blocking): the database's file at `db` (its
    /// largest file, for a database kept as a directory).
    pub fn run(&self, db: &Path, scrubbing: bool) -> Result<Reading, String> {
        {
            let mut st = lock(&self.state);
            if st.running {
                return Err("a probe runs already".into());
            }
            st.running = true;
        }
        let _running = Running(self);
        let t0 = Instant::now();
        let at_ms = now_ms();
        let result = data_file(db).ok_or_else(|| format!("{} has no file to read", db.display())).and_then(|f| measure(&f, self.add_ms, at_ms).map_err(|e| format!("{}: {e}", f.display())));
        let mut st = lock(&self.state);
        let (reads, mb_s) = match result {
            Ok(r) => r,
            Err(e) => {
                st.error = Some((at_ms, e.clone()));
                return Err(e);
            }
        };
        // The device's counters against the last probe's.
        let dev = dev_stat(db);
        let (busy, await_ms) = match (st.dev, dev) {
            (Some(a), Some(b)) => {
                let ms = b.at.duration_since(a.at).as_secs_f64() * 1e3;
                let busy = (ms > 0.0).then(|| (b.io_ms.saturating_sub(a.io_ms) as f64 / ms).clamp(0.0, 1.0));
                let n = b.reads.saturating_sub(a.reads);
                (busy, (n > 0).then(|| b.read_ms.saturating_sub(a.read_ms) as f64 / n as f64))
            }
            _ => (None, None),
        };
        st.dev = dev;
        let mut sorted = reads.clone();
        sorted.sort_by(f64::total_cmp);
        let r = Reading {
            at_ms,
            read_ms: round3(sorted[sorted.len() / 2]),
            read_max_ms: round3(*sorted.last().unwrap_or(&0.0)),
            reads: reads.len(),
            mb_s: mb_s.map(|m| (m * 10.0).round() / 10.0),
            busy: busy.map(round3),
            await_ms: await_ms.map(round3),
            scrubbing,
            took_ms: round3(t0.elapsed().as_secs_f64() * 1e3),
        };
        st.error = None;
        st.kept.push_back(r.clone());
        while st.kept.len() > KEPT {
            st.kept.pop_front();
        }
        Ok(r)
    }

    /// The latest reading, if it is recent: not older than three probes.
    pub fn latest(&self) -> Option<Reading> {
        let st = lock(&self.state);
        let r = st.kept.back()?;
        let stale = (self.every.as_millis() as u64 * 3).max(180_000);
        (now_ms().saturating_sub(r.at_ms) <= stale).then(|| r.clone())
    }

    /// Every reading kept, the oldest first, and the last error.
    pub fn json(&self) -> Json {
        let st = lock(&self.state);
        json!({
            "every_s": self.every.as_secs(), "kept": st.kept.iter().collect::<Vec<_>>(),
            "error": st.error.as_ref().map(|(at, e)| json!({"at_ms": at, "message": e})),
        })
    }

    /// The series' gauges of the disk: the latest reading, and where the
    /// database is, with `total` and `avail` its file system's bytes.
    pub fn gauges(&self, g: &mut impl FnMut(&'static str, f64), total: Option<(u64, u64)>) {
        if let Some(r) = self.latest() {
            g("disk_read_ms", r.read_ms);
            g("disk_read_max_ms", r.read_max_ms);
            g("disk_probe_at_ms", r.at_ms as f64);
            if let Some(m) = r.mb_s {
                g("disk_mb_s", m);
            }
            if let Some(b) = r.busy {
                g("disk_busy", b);
            }
            if let Some(a) = r.await_ms {
                g("disk_await_ms", a);
            }
            if r.scrubbing {
                g("disk_probe_scrubbing", 1.0);
            }
        }
        if self.kube() {
            g("disk_on_node", self.on_node() as u8 as f64);
            if let (true, Some((total, avail))) = (self.on_node(), total) {
                g("disk_evict_free_ratio", self.evict_free);
                let h = headroom(total, avail, self.evict_free);
                g("disk_evict_headroom_bytes", h);
                if total > 0 {
                    g("disk_evict_headroom_ratio", h / total as f64);
                }
            }
        }
    }
}

/// Probes the disk under this server's database every
/// [`Probe::every`] while the server serves (none on a coordinator), the
/// first within a few seconds of the start.
pub fn start(svc: &std::sync::Arc<crate::service::Service>) {
    let Some(db) = svc.local_db() else { return };
    let path = db.storage().path().to_path_buf();
    svc.disk_probe.place(&path);
    let every = svc.disk_probe.every;
    if every.is_zero() {
        return;
    }
    let me = std::sync::Arc::downgrade(svc);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(5)).await;
        loop {
            let Some(svc) = me.upgrade() else { return };
            let _ = probe_now(&svc, &path).await;
            drop(svc);
            tokio::time::sleep(every).await;
        }
    });
}

/// Probes the disk under the database at `db` now.
pub async fn probe_now(svc: &std::sync::Arc<crate::service::Service>, db: &Path) -> Result<Reading, ApiError> {
    let (s, db) = (svc.clone(), db.to_path_buf());
    let r = tokio::task::spawn_blocking(move || {
        let scrubbing = s.scrubs.running();
        s.disk_probe.run(&db, scrubbing)
    })
    .await
    .map_err(ApiError::internal)?;
    r.map_err(|e| ApiError { code: None, status: 409, message: format!("the disk probe could not read: {e}") })
}

/// The paths besides those named after the database that are this
/// server's own: its backup folder, keys, audit log and alert rules.
fn keep_of(svc: &crate::service::Service) -> Vec<PathBuf> {
    let mut keep: Vec<PathBuf> = Vec::new();
    keep.extend(svc.backup_dir().map(Path::to_path_buf));
    keep.extend(svc.keys.file().map(Path::to_path_buf));
    keep.extend(svc.audit.path().map(Path::to_path_buf));
    if let Some(a) = svc.alerts.file() {
        keep.push(a.to_path_buf());
        keep.push(a.with_file_name("contain.json"));
        keep.push(a.with_file_name(format!("{}.state.json", a.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "alerts".into()))));
    }
    keep
}

fn local_path(svc: &crate::service::Service) -> Result<PathBuf, ApiError> {
    svc.local_db().map(|db| db.storage().path().to_path_buf()).ok_or_else(|| ApiError::bad("a coordinator has no disk of its own to judge: name a copy"))
}

/// [`report`] of this server's database.
pub async fn report_of(svc: &std::sync::Arc<crate::service::Service>) -> Result<Json, ApiError> {
    let (path, keep, s) = (local_path(svc)?, keep_of(svc), svc.clone());
    tokio::task::spawn_blocking(move || report(&path, &keep, &s.disk_probe)).await.map_err(ApiError::internal)?
}

/// The disk's reads timed now, of this server's database.
pub async fn probe_of(svc: &std::sync::Arc<crate::service::Service>) -> Result<Json, ApiError> {
    let path = local_path(svc)?;
    let r = probe_now(svc, &path).await?;
    Ok(json!({"reading": r}))
}

/// [`clean`] of this server's volume, for `by`.
pub async fn clean_of(svc: &std::sync::Arc<crate::service::Service>, paths: &[Json], by: &str) -> Result<Json, ApiError> {
    let (path, keep, asked) = (local_path(svc)?, keep_of(svc), paths.to_vec());
    let j = tokio::task::spawn_blocking(move || clean(&path, &keep, &asked)).await.map_err(ApiError::internal)??;
    tracing::warn!(by, deleted = j["deleted"].as_array().map_or(0, Vec::len), freed_bytes = j["freed_bytes"].as_u64().unwrap_or(0), "entries deleted from the database's volume");
    Ok(j)
}

/// The bytes a file system of `total` bytes with `avail` free has before
/// it is under `free_ratio` free (negative under it).
pub fn headroom(total: u64, avail: u64, free_ratio: f64) -> f64 {
    avail as f64 - free_ratio * total as f64
}

/// A disk this much slower than the median of its peers', and at least
/// [`SLOW_ABOVE_MS`] slower, reads slowly; and any that takes [`SLOW_MS`]
/// a read, whatever its peers.
pub const SLOW_TIMES: f64 = 5.0;
pub const SLOW_ABOVE_MS: f64 = 2.0;
pub const SLOW_MS: f64 = 30.0;
/// Reads in a row below the peers' over [`SLOW_TIMES`] are slow too, when
/// the peers read at this many MB a second at least.
const SLOW_PEERS_MB_S: f64 = 20.0;

/// A disk that reads slowly, of those [`outliers`] is given.
#[derive(Clone, Debug, PartialEq)]
pub struct Outlier {
    /// Its index in what was given.
    pub i: usize,
    /// The median of the other disks' random reads and reads in a row
    /// (none for a disk alone).
    pub peers_ms: Option<f64>,
    pub peers_mb_s: Option<f64>,
    /// It reads in a row slowly, besides (or rather than) at random.
    pub slow_mb_s: bool,
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    let n = v.len();
    Some(if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 })
}

/// The disks of `disks` (each its random 4 KiB read in ms, and its reads
/// in a row in MB a second) that read slowly against the others', the
/// cluster's copies of the same server: a disk degraded under one copy
/// shows against its peers though nothing is busy, and a disk alike
/// under all of them is the cluster's kind of disk, not a fault.
pub fn outliers(disks: &[(f64, Option<f64>)]) -> Vec<Outlier> {
    let mut out = Vec::new();
    for (i, &(ms, mb)) in disks.iter().enumerate() {
        let others = || disks.iter().enumerate().filter(move |(j, _)| *j != i).map(|(_, d)| *d);
        let peers_ms = median(others().map(|d| d.0).collect());
        let peers_mb_s = median(others().filter_map(|d| d.1).collect());
        let slow_ms = ms >= SLOW_MS || peers_ms.is_some_and(|p| ms >= SLOW_TIMES * p && ms >= p + SLOW_ABOVE_MS);
        let slow_mb_s = matches!((mb, peers_mb_s), (Some(m), Some(p)) if p >= SLOW_PEERS_MB_S && m * SLOW_TIMES <= p);
        if slow_ms || slow_mb_s {
            out.push(Outlier { i, peers_ms, peers_mb_s, slow_mb_s });
        }
    }
    out
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// Whether `a` and `b` are on one file system.
fn same_device(a: &Path, b: &Path) -> bool {
    let dev = |p: &Path| std::fs::metadata(p).ok().map(|m| m.dev());
    let a = dev(a).or_else(|| a.parent().and_then(dev));
    matches!((a, dev(b)), (Some(x), Some(y)) if x == y)
}

/// The file a probe reads of the database at `db`: itself, or a database
/// kept as a directory's largest file.
pub fn data_file(db: &Path) -> Option<PathBuf> {
    let m = std::fs::metadata(db).ok()?;
    if m.is_file() {
        return Some(db.to_path_buf());
    }
    std::fs::read_dir(db).ok()?.flatten().filter_map(|e| Some((e.metadata().ok().filter(|m| m.is_file())?.len(), e.path()))).max().map(|(_, p)| p)
}

/// A buffer aligned for reads around the cache.
struct Aligned {
    ptr: *mut u8,
    layout: std::alloc::Layout,
}

impl Aligned {
    fn new(len: usize) -> Aligned {
        let layout = std::alloc::Layout::from_size_align(len, BLOCK).expect("a block-aligned layout");
        // SAFETY: the layout's size is not zero.
        let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
        if ptr.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        Aligned { ptr, layout }
    }
}

impl Drop for Aligned {
    fn drop(&mut self) {
        // SAFETY: allocated with this layout in `new`.
        unsafe { std::alloc::dealloc(self.ptr, self.layout) }
    }
}

/// Opens `file` to be read around the page cache; on a file system that
/// cannot (tmpfs), through it, which is memory there anyway.
fn open_direct(file: &Path) -> io::Result<std::fs::File> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        match std::fs::OpenOptions::new().read(true).custom_flags(libc::O_DIRECT).open(file) {
            Err(e) if e.raw_os_error() == Some(libc::EINVAL) => std::fs::File::open(file),
            r => r,
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        use std::os::unix::io::AsRawFd;
        let f = std::fs::File::open(file)?;
        #[cfg(target_os = "macos")]
        {
            // SAFETY: fcntl on an open descriptor.
            unsafe {
                libc::fcntl(f.as_raw_fd(), libc::F_NOCACHE, 1);
            }
        }
        let _ = f.as_raw_fd();
        Ok(f)
    }
}

fn pread(f: &std::fs::File, buf: &mut Aligned, len: usize, at: u64) -> io::Result<usize> {
    use std::os::unix::io::AsRawFd;
    // SAFETY: the buffer holds at least `len` bytes.
    let n = unsafe { libc::pread(f.as_raw_fd(), buf.ptr.cast(), len, at as libc::off_t) };
    if n < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(n as usize)
    }
}

/// Times [`READS`] random block reads of `file` (milliseconds each) and a
/// read of up to [`SEQ`] bytes in a row (MB a second), around the cache;
/// `add_ms` is added to each read, `seed` picks the places.
pub fn measure(file: &Path, add_ms: f64, seed: u64) -> io::Result<(Vec<f64>, Option<f64>)> {
    let f = open_direct(file)?;
    let size = f.metadata()?.len();
    let blocks = size / BLOCK as u64;
    if blocks < 2 {
        return Err(io::Error::other("the file is too small to time a read of"));
    }
    let wait = |t: Instant| {
        if add_ms > 0.0 {
            std::thread::sleep(Duration::from_secs_f64(add_ms / 1e3));
        }
        t.elapsed().as_secs_f64() * 1e3
    };
    let t_all = Instant::now();
    let mut buf = Aligned::new(CHUNK);
    let mut x = seed | 1;
    let mut next = || {
        // xorshift64
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let mut reads = Vec::with_capacity(READS);
    for _ in 0..READS {
        let at = (next() % blocks) * BLOCK as u64;
        let t = Instant::now();
        pread(&f, &mut buf, BLOCK, at)?;
        reads.push(wait(t));
        if t_all.elapsed() > BUDGET && reads.len() >= 2 {
            break;
        }
    }
    // In a row: from a random block, as far as the file and the budget go.
    let run = (SEQ as u64).min(blocks * BLOCK as u64);
    if run < 16 * BLOCK as u64 {
        return Ok((reads, None));
    }
    let start = (next() % (blocks - run / BLOCK as u64 + 1)) * BLOCK as u64;
    let t = Instant::now();
    let mut read = 0u64;
    while read < run && t_all.elapsed() < BUDGET * 2 {
        let len = (CHUNK as u64).min(run - read) as usize / BLOCK * BLOCK;
        let n = pread(&f, &mut buf, len, start + read)?;
        if n == 0 {
            break;
        }
        read += n as u64;
        if add_ms > 0.0 {
            std::thread::sleep(Duration::from_secs_f64(add_ms / 1e3));
        }
    }
    let secs = t.elapsed().as_secs_f64();
    Ok((reads, (read > 0 && secs > 0.0).then(|| read as f64 / 1e6 / secs)))
}

/// The counters of the device `path` is on (Linux).
fn dev_stat(path: &Path) -> Option<DevStat> {
    #[cfg(target_os = "linux")]
    {
        let dev = std::fs::metadata(path).ok()?.dev();
        // glibc's encoding of a device number.
        let major = ((dev >> 8) & 0xfff) | ((dev >> 32) & !0xfff);
        let minor = (dev & 0xff) | ((dev >> 12) & !0xff);
        let text = std::fs::read_to_string(format!("/sys/dev/block/{major}:{minor}/stat")).ok()?;
        let f: Vec<u64> = text.split_whitespace().filter_map(|x| x.parse().ok()).collect();
        // reads, merged, sectors, read ms, writes, merged, sectors, write ms, in flight, io ms, ...
        (f.len() >= 10).then(|| DevStat { at: Instant::now(), reads: f[0], read_ms: f[3], io_ms: f[9] })
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        None
    }
}

// ---------------------------------------------------------------- what fills it

/// Where a listing of the volume under `db` starts: in a container, the
/// root of the volume the database is on (the highest folder above it on
/// the same file system: a `hostPath`'s or a claim's mount), so that a
/// build's output beside the database's folder shows; elsewhere the
/// database's folder alone (on a machine's one file system the volume's
/// root is `/`).
pub fn scan_root(db: &Path) -> PathBuf {
    let dir = db_dir(db);
    let contained = std::env::var_os("KUBERNETES_SERVICE_HOST").is_some() || Path::new("/.dockerenv").exists();
    if !contained {
        return dir;
    }
    let Ok(dev) = std::fs::metadata(&dir).map(|m| m.dev()) else { return dir };
    let mut root = dir.clone();
    while let Some(up) = root.parent() {
        if up.as_os_str().is_empty() || std::fs::metadata(up).map(|m| m.dev()).ok() != Some(dev) {
            break;
        }
        root = up.to_path_buf();
    }
    if root == Path::new("/") {
        dir
    } else {
        root
    }
}

/// The folder the database at `db` is in (a database kept as a directory:
/// the folder that holds it).
fn db_dir(db: &Path) -> PathBuf {
    let abs = std::fs::canonicalize(db).unwrap_or_else(|_| db.to_path_buf());
    abs.parent().filter(|p| !p.as_os_str().is_empty()).map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

/// Whether the entry named `name` in the database's folder is the
/// server's: the database and everything named after it (its log, its
/// readers, its slots, its log store, a transfer's copy `.<db>.copy-…`).
fn named_after(name: &str, db_name: &str) -> bool {
    !db_name.is_empty() && (name == db_name || name.starts_with(&format!("{db_name}.")) || name.starts_with(&format!(".{db_name}.")) || name.starts_with(&format!("..{db_name}.")))
}

/// The files this process holds open (Linux), which are its own.
fn open_files() -> HashSet<PathBuf> {
    std::fs::read_dir("/proc/self/fd").into_iter().flatten().flatten().filter_map(|e| std::fs::read_link(e.path()).ok()).filter(|p| p.is_absolute()).collect()
}

/// The size of `p` and everything under it on device `dev`, its files,
/// and when the newest of them changed; `walked` counts entries against
/// the walk's budget, which cuts it short (`complete` false).
struct Size {
    bytes: u64,
    files: u64,
    modified_ms: u64,
    complete: bool,
}

fn size_of(p: &Path, dev: u64, walked: &mut usize, t0: Instant, open: &HashSet<PathBuf>, holds_open: &mut bool) -> Size {
    let mut s = Size { bytes: 0, files: 0, modified_ms: 0, complete: true };
    let mut stack = vec![p.to_path_buf()];
    while let Some(q) = stack.pop() {
        *walked += 1;
        if *walked > WALK_ENTRIES || t0.elapsed() > WALK_FOR {
            s.complete = false;
            break;
        }
        let Ok(m) = std::fs::symlink_metadata(&q) else { continue };
        let mtime = (m.mtime().max(0) as u64) * 1000;
        s.modified_ms = s.modified_ms.max(mtime);
        if m.is_dir() {
            if m.dev() != dev {
                continue;
            }
            stack.extend(std::fs::read_dir(&q).into_iter().flatten().flatten().map(|e| e.path()));
        } else {
            // Blocks taken, as `du` counts them: a sparse file takes less.
            s.bytes += m.blocks() * 512;
            s.files += 1;
            if open.contains(&q) {
                *holds_open = true;
            }
        }
    }
    s
}

/// One entry of a listing.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Entry {
    pub path: String,
    pub bytes: u64,
    pub files: u64,
    pub modified_ms: u64,
    pub dir: bool,
    /// `own` (the server's: never deleted here) or `other`.
    pub kind: &'static str,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub why: String,
    /// The walk stopped before its end: at least this much.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub partial: bool,
}

/// What a listing found.
pub struct Listing {
    pub root: PathBuf,
    pub entries: Vec<Entry>,
    pub complete: bool,
    pub walked: usize,
    pub ms: f64,
}

/// Lists what fills the volume under the database at `db` (blocking):
/// every entry of each folder from [`scan_root`] down to the database's
/// folder, with its size, those named after the database and those the
/// server holds open marked `own`, as are `keep` (its other files: the
/// backup folder, the keys).
pub fn list(db: &Path, keep: &[PathBuf]) -> io::Result<Listing> {
    let t0 = Instant::now();
    let dir = db_dir(db);
    let root = scan_root(db);
    let dev = std::fs::metadata(&dir)?.dev();
    let db_abs = std::fs::canonicalize(db).unwrap_or_else(|_| db.to_path_buf());
    let db_name = db_abs.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let keep: Vec<PathBuf> = keep.iter().map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.clone())).collect();
    let open = open_files();
    // The folders from the root down to the database's.
    let mut chain = vec![dir.clone()];
    let mut up = dir.clone();
    while up != root {
        match up.parent() {
            Some(p) if p.starts_with(&root) => {
                up = p.to_path_buf();
                chain.push(up.clone());
            }
            _ => break,
        }
    }
    let mut entries = Vec::new();
    let mut walked = 0usize;
    let mut complete = true;
    for folder in &chain {
        for e in std::fs::read_dir(folder)?.flatten() {
            let p = e.path();
            if chain.contains(&p) {
                continue;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            let Ok(m) = std::fs::symlink_metadata(&p) else { continue };
            let mut holds_open = false;
            let s = size_of(&p, dev, &mut walked, t0, &open, &mut holds_open);
            complete &= s.complete;
            let why = if *folder == dir && named_after(&name, &db_name) {
                if name == db_name {
                    "the database".to_string()
                } else if name.starts_with('.') {
                    "a copy of the database a transfer made (a restart removes one it left)".to_string()
                } else {
                    format!("the database's {}", name.trim_start_matches(&db_name).trim_start_matches('.'))
                }
            } else if keep.iter().any(|k| k == &p || k.starts_with(&p)) {
                "the server's (a flag names it)".to_string()
            } else if holds_open {
                "the server holds a file in it open".to_string()
            } else {
                String::new()
            };
            entries.push(Entry {
                path: p.display().to_string(),
                bytes: s.bytes,
                files: s.files,
                modified_ms: s.modified_ms,
                dir: m.is_dir(),
                kind: if why.is_empty() { "other" } else { "own" },
                why,
                partial: !s.complete,
            });
        }
    }
    entries.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.path.cmp(&b.path)));
    Ok(Listing { root, entries, complete, walked, ms: t0.elapsed().as_secs_f64() * 1e3 })
}

/// `GET /v1/admin/disk` of this server (blocking): its volume, the probe's
/// readings, and what fills the volume ([`list`]).
pub fn report(db: &Path, keep: &[PathBuf], probe: &Probe) -> Result<Json, ApiError> {
    let l = list(db, keep).map_err(|e| ApiError::internal(format!("listing {}: {e}", db.display())))?;
    let fs = crate::telemetry::disk(db).map(|(total, free, avail)| {
        let used = total.saturating_sub(free);
        let mut j = json!({"total_bytes": total, "used_bytes": used, "available_bytes": avail, "used_ratio": crate::telemetry::disk_used_ratio(total, free, avail)});
        if probe.on_node() {
            j["evict_free_ratio"] = json!(probe.evict_free);
            j["evict_headroom_bytes"] = json!(headroom(total, avail, probe.evict_free));
        }
        j
    });
    let own: u64 = l.entries.iter().filter(|e| e.kind == "own").map(|e| e.bytes).sum();
    let other: u64 = l.entries.iter().filter(|e| e.kind == "other").map(|e| e.bytes).sum();
    let shown: Vec<&Entry> = l.entries.iter().take(SHOWN).collect();
    Ok(json!({
        "path": db.display().to_string(), "root": l.root.display().to_string(),
        "kubernetes": probe.kube(), "on_node": probe.on_node(),
        "filesystem": fs, "probe": probe.json(),
        "own_bytes": own, "other_bytes": other, "entries": shown, "more": l.entries.len().saturating_sub(SHOWN),
        "complete": l.complete, "walked": l.walked, "ms": (l.ms * 10.0).round() / 10.0,
    }))
}

/// `POST /v1/admin/disk/clean` of this server (blocking): deletes each of
/// `asked` (`{path, modified_ms}`) that a listing made now has as an
/// `other` entry changed no later than the listing the person saw
/// (`modified_ms`), and nothing else: an entry the server owns, holds
/// open, or that changed since, is refused with the others kept.
pub fn clean(db: &Path, keep: &[PathBuf], asked: &[Json]) -> Result<Json, ApiError> {
    if asked.is_empty() {
        return Err(ApiError::bad("paths names the entries to delete: [{path, modified_ms}], as GET /v1/admin/disk listed them"));
    }
    if asked.len() > 100 {
        return Err(ApiError::bad("at most 100 entries at once"));
    }
    let l = list(db, keep).map_err(|e| ApiError::internal(format!("listing {}: {e}", db.display())))?;
    let mut deleted = Vec::new();
    let mut refused = Vec::new();
    let mut freed = 0u64;
    for a in asked {
        let path = a["path"].as_str().unwrap_or_default();
        let seen = a["modified_ms"].as_u64();
        let Some(e) = l.entries.iter().find(|e| e.path == path) else {
            refused.push(json!({"path": path, "why": "not an entry of the volume's listing now (deleted already, or not under the database's volume)"}));
            continue;
        };
        if e.kind != "other" {
            refused.push(json!({"path": path, "why": format!("the server's own: {}", e.why)}));
            continue;
        }
        if e.partial {
            refused.push(json!({"path": path, "why": "too large to list whole: delete it on the machine"}));
            continue;
        }
        if seen.is_none_or(|s| e.modified_ms > s) {
            refused.push(json!({"path": path, "why": "it changed since it was listed: list again and look first"}));
            continue;
        }
        let p = Path::new(&e.path);
        let r = if e.dir { std::fs::remove_dir_all(p) } else { std::fs::remove_file(p) };
        match r {
            Ok(()) => {
                freed += e.bytes;
                tracing::warn!(path = %e.path, bytes = e.bytes, "deleted from the database's volume, as asked");
                deleted.push(json!({"path": e.path, "bytes": e.bytes}));
            }
            Err(err) => refused.push(json!({"path": path, "why": err.to_string()})),
        }
    }
    let fs = crate::telemetry::disk(db).map(|(total, _, avail)| json!({"total_bytes": total, "available_bytes": avail}));
    Ok(json!({"deleted": deleted, "refused": refused, "freed_bytes": freed, "filesystem": fs}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("chloedb-disks-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_probe_reads_the_file_and_times_it() {
        let d = dir("probe");
        let f = d.join("x.chloedb");
        std::fs::write(&f, vec![7u8; 1 << 20]).unwrap();
        let p = Probe::default();
        match p.run(&f, false) {
            Ok(r) => {
                assert_eq!(r.reads, READS);
                assert!(r.read_ms >= 0.0 && r.read_max_ms >= r.read_ms, "{r:?}");
                assert!(r.mb_s.is_some_and(|m| m > 0.0), "{r:?}");
                assert_eq!(p.latest(), Some(r));
            }
            // A file system that does not read around its cache (tmpfs): said so.
            Err(e) => assert!(e.contains("x.chloedb"), "{e}"),
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn added_time_shows_in_the_reading() {
        let d = dir("slow");
        let f = d.join("x.chloedb");
        std::fs::write(&f, vec![1u8; 256 << 10]).unwrap();
        if let Ok((reads, mb_s)) = measure(&f, 5.0, 42) {
            assert!(reads.iter().all(|&r| r >= 5.0), "{reads:?}");
            // 256 KiB in four reads of a chunk, each 5 ms late.
            assert!(mb_s.is_some_and(|m| m < 256.0 / 1e3 / 0.005 * 1.1), "{mb_s:?}");
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_listing_keeps_the_servers_files_apart() {
        let d = dir("list");
        let db = d.join("gh.chloedb");
        std::fs::write(&db, vec![0u8; 8192]).unwrap();
        std::fs::write(d.join("gh.chloedb.wal"), vec![0u8; 4096]).unwrap();
        std::fs::create_dir_all(d.join("gh.chloedb.logs")).unwrap();
        std::fs::create_dir_all(d.join(".gh.chloedb.copy-1a2b")).unwrap();
        std::fs::create_dir_all(d.join("red/people/s1")).unwrap();
        std::fs::write(d.join("red/people/s1/p0.parquet"), vec![0u8; 64 << 10]).unwrap();
        std::fs::write(d.join("keys.json"), b"{}").unwrap();
        let l = list(&db, &[d.join("keys.json")]).unwrap();
        let kind = |name: &str| l.entries.iter().find(|e| e.path.ends_with(name)).map(|e| (e.kind, e.why.clone()));
        assert_eq!(kind("gh.chloedb").unwrap().0, "own");
        assert_eq!(kind("gh.chloedb.wal").unwrap(), ("own", "the database's wal".into()));
        assert_eq!(kind(".gh.chloedb.copy-1a2b").unwrap().0, "own");
        assert_eq!(kind("keys.json").unwrap().0, "own");
        let red = l.entries.iter().find(|e| e.path.ends_with("/red")).unwrap();
        assert_eq!((red.kind, red.dir, red.files), ("other", true, 1));
        assert!(red.bytes >= 64 << 10, "{red:?}");
        // The largest first.
        assert!(l.entries.windows(2).all(|w| w[0].bytes >= w[1].bytes));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn clean_deletes_only_what_was_listed_and_is_not_the_servers() {
        let d = dir("clean");
        let db = d.join("gh.chloedb");
        std::fs::write(&db, vec![0u8; 8192]).unwrap();
        std::fs::create_dir_all(d.join("exp")).unwrap();
        std::fs::write(d.join("exp/a.bin"), vec![0u8; 8192]).unwrap();
        std::fs::write(d.join("later.bin"), vec![0u8; 4096]).unwrap();
        let l = list(&db, &[]).unwrap();
        let seen = |name: &str| l.entries.iter().find(|e| e.path.ends_with(name)).unwrap().clone();
        let (exp, later, own) = (seen("/exp"), seen("later.bin"), seen("gh.chloedb"));
        let asked = vec![
            json!({"path": exp.path, "modified_ms": exp.modified_ms}),
            json!({"path": own.path, "modified_ms": own.modified_ms}),
            // Listed before it changed: refused.
            json!({"path": later.path, "modified_ms": later.modified_ms.saturating_sub(5_000)}),
            json!({"path": "/etc/passwd", "modified_ms": u64::MAX}),
        ];
        let j = clean(&db, &[], &asked).unwrap();
        assert_eq!(j["deleted"].as_array().unwrap().len(), 1, "{j}");
        assert_eq!(j["refused"].as_array().unwrap().len(), 3, "{j}");
        assert!(!d.join("exp").exists());
        assert!(db.exists() && d.join("later.bin").exists());
        assert!(j["refused"][0]["why"].as_str().unwrap().starts_with("the server's own"), "{j}");
        assert!(j["refused"][1]["why"].as_str().unwrap().contains("changed since"), "{j}");
        assert!(clean(&db, &[], &[]).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn headroom_is_what_is_left_above_the_line() {
        let gib = 1u64 << 30;
        // A 157 GiB node disk with 29 GiB free: 5.45 GiB above 15%.
        let h = headroom(157 * gib, 29 * gib, 0.15) / gib as f64;
        assert!((h - 5.45).abs() < 0.01, "{h}");
        assert!(headroom(100, 10, 0.15) < 0.0);
    }

    /// gh, 11-17: one of four machines' local disks read at 12 ms and
    /// 4 MB/s, its peers at 0.2 ms and 600 MB/s.
    #[test]
    fn a_disk_far_slower_than_its_peers_is_an_outlier() {
        let gh = [(0.2, Some(610.0)), (0.21, Some(590.0)), (12.0, Some(4.0)), (0.19, Some(600.0))];
        let o = outliers(&gh);
        assert_eq!(o.len(), 1, "{o:?}");
        assert_eq!((o[0].i, o[0].peers_ms, o[0].peers_mb_s, o[0].slow_mb_s), (2, Some(0.2), Some(600.0), true));
        // Alike, slow or fast: the cluster's kind of disk, not a fault.
        assert!(outliers(&[(1.5, None), (1.8, None), (1.6, None)]).is_empty());
        // Five times slower but under 2 ms more (NVMe beside a volume): not told.
        assert!(outliers(&[(0.1, None), (0.1, None), (1.2, None)]).is_empty());
        // Alone, only past 30 ms.
        assert!(outliers(&[(12.0, None)]).is_empty());
        assert_eq!(outliers(&[(45.0, None)])[0].peers_ms, None);
        // Reads in a row a tenth of the peers', the random ones fine.
        let o = outliers(&[(0.3, Some(500.0)), (0.3, Some(40.0)), (0.3, Some(520.0))]);
        assert_eq!((o.len(), o[0].i, o[0].slow_mb_s), (1, 1, true));
    }

    #[test]
    fn only_names_after_the_database_are_its_own() {
        assert!(named_after("gh.chloedb", "gh.chloedb"));
        assert!(named_after("gh.chloedb.wal", "gh.chloedb"));
        assert!(named_after(".gh.chloedb.restore-9", "gh.chloedb"));
        assert!(named_after("..gh.chloedb.copy-1.checkpoint-2", "gh.chloedb"));
        assert!(!named_after("gh.chloedb2", "gh.chloedb"));
        assert!(!named_after("gh", "gh.chloedb"));
        assert!(!named_after("x", ""));
    }
}
