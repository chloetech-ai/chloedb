//! `chloedb-gh-pin DB [PICK...]`: opens the database read-only, beside the
//! server that serves it, and locks in memory (`mlock`) the sections a
//! search by meaning reads at random in every segment of every table: the
//! vector codes (`vec.*.sq8`, `sq8p`, `bin`, `rq`, `meta`, `present`) and the
//! graph (`hnsw`, `hnsw_upper`), but not the float32 rows, which a search
//! reads only for its candidates. Then it waits, holding the lock, and
//! checks every minute that the segments are the same ones; when they
//! change (a merge, a load) it locks the new ones and lets the old go.
//!
//! Why: a server's page cache is the kernel's to take back. On a machine
//! whose other server reads tens of gigabytes, the vectors' pages go even
//! from a container of their own, and the next search reads them from the
//! disk again (0.4 to 1 s rather than 30 ms). Locked pages stay. Run it in
//! the server's container (the pages count against its memory limit) with
//! RLIMIT_MEMLOCK high enough or CAP_IPC_LOCK.
//!
//! PICK, if given, are substrings of the section names to lock instead.
//!
//! Built by hand, for the server image's glibc (Debian 12):
//!   docker run --rm -v $PWD/../../..:/src -w /src/chloedb-gh/tools/pin \
//!     rust:1-bookworm cargo build --release

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;

use chloedb::storage::{Database, DbOptions, Snapshot};

const DEFAULT: &[&str] = &[".sq8", ".bin", ".rq", ".meta", ".present", "hnsw"];

fn wanted(name: &str, picks: &[String]) -> bool {
    if !picks.is_empty() {
        return picks.iter().any(|p| name.contains(p.as_str()));
    }
    name.starts_with("vec.") && !name.ends_with(".f32") && DEFAULT.iter().any(|p| name.contains(p))
        || name.contains("hnsw")
}

type Range = (usize, usize, String, u64);

/// The ranges to lock, as (address, length, section name, segment id), and
/// the tables' snapshots, which keep their segments mapped while held.
fn ranges(db: &Database, picks: &[String]) -> (Vec<Range>, Vec<Snapshot>) {
    let snap = db.snapshot();
    let (mut out, mut held) = (Vec::new(), Vec::new());
    for schema in db.tables() {
        let Some(t) = snap.table(&schema.table) else { continue };
        for seg in t.segments() {
            if seg.in_memory {
                continue;
            }
            let image = seg.file.image();
            for (name, _) in seg.file.section_sizes() {
                if !wanted(&name, picks) {
                    continue;
                }
                if let Some(r) = seg.file.section_range(&name) {
                    let start = image.as_ptr() as usize + r.start;
                    out.push((start, r.end - r.start, format!("{}/{name}", schema.table), seg.id));
                }
            }
        }
        held.push(t);
    }
    (out, held)
}

fn lock(rs: &[Range]) -> usize {
    let page = 4096usize;
    let mut total = 0;
    for (start, len, name, seg) in rs {
        let lo = start / page * page;
        let hi = (start + len).div_ceil(page) * page;
        // SAFETY: the range lies within a read-only mapping of a segment
        // whose snapshot the caller holds; mlock only pins its pages.
        let rc = unsafe { libc::mlock(lo as *const libc::c_void, hi - lo) };
        if rc != 0 {
            eprintln!("mlock {name} (segment {seg}, {} MB): {}", len >> 20, std::io::Error::last_os_error());
        } else {
            total += hi - lo;
            eprintln!("locked {name} (segment {seg}): {} MB", len >> 20);
        }
    }
    total
}

fn ids(rs: &[Range]) -> BTreeSet<u64> {
    rs.iter().map(|r| r.3).collect()
}

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next().map(PathBuf::from) else {
        eprintln!("usage: chloedb-gh-pin DB [PICK...]");
        std::process::exit(2);
    };
    let picks: Vec<String> = args.collect();
    let opts = DbOptions { read_only: true, ..DbOptions::default() };
    let db = Database::open(&path, opts).unwrap_or_else(|e| {
        eprintln!("{}: {e}", path.display());
        std::process::exit(1);
    });
    let (mut locked, mut _held) = ranges(&db, &picks);
    let total = lock(&locked);
    eprintln!("{}: {} MB locked in {} sections", path.display(), total >> 20, locked.len());
    loop {
        std::thread::sleep(Duration::from_secs(60));
        let (now, held) = ranges(&db, &picks);
        if ids(&now) != ids(&locked) {
            // Lock the new segments first; the old ones' locks go when
            // their snapshots are dropped and the segments unmapped.
            let total = lock(&now);
            eprintln!("segments changed: {} MB locked in {} sections", total >> 20, now.len());
            locked = now;
            _held = held;
        }
    }
}
