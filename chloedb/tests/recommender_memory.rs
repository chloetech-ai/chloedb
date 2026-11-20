//! What CREATE RECOMMENDER holds in memory while it counts, and after
//! (NADIA-401): its own test binary, since it counts every allocation of
//! the process.

use std::alloc::{GlobalAlloc, Layout as AllocLayout, System};
use std::sync::atomic::{AtomicIsize, Ordering};

use chloedb::db::{Db, SqlResult};
use chloedb::query::Params;
use chloedb::schema::{Field, FieldType, Schema};
use chloedb::storage::{DbOptions, Layout};

struct Counting;

static LIVE: AtomicIsize = AtomicIsize::new(0);
static PEAK: AtomicIsize = AtomicIsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: AllocLayout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            let now = LIVE.fetch_add(l.size() as isize, Ordering::Relaxed) + l.size() as isize;
            PEAK.fetch_max(now, Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: AllocLayout) {
        unsafe { System.dealloc(p, l) };
        LIVE.fetch_sub(l.size() as isize, Ordering::Relaxed);
    }
    unsafe fn realloc(&self, p: *mut u8, l: AllocLayout, new: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, new) };
        if !q.is_null() {
            let now = LIVE.fetch_add(new as isize - l.size() as isize, Ordering::Relaxed) + new as isize - l.size() as isize;
            PEAK.fetch_max(now, Ordering::Relaxed);
        }
        q
    }
}

#[global_allocator]
static A: Counting = Counting;

fn sql(db: &Db, s: &str) {
    match db.sql(s, &Params::new()).unwrap_or_else(|e| panic!("{s}: {e}")) {
        SqlResult::Rows(_) | SqlResult::Written(_) => {}
    }
}

const MB: f64 = 1024.0 * 1024.0;

#[test]
fn create_recommender_holds_memory_in_proportion_to_its_events_and_lets_it_go() {
    let dir = tempfile::tempdir().unwrap();
    let seed = Schema::new("seed", vec![Field::new("id", FieldType::Keyword)], Some("id")).unwrap();
    let db = Db::create(&dir.path().join("r.strata"), seed, DbOptions { sync: false, layout: Layout::SingleFile, ..DbOptions::default() }).unwrap();
    sql(&db, "CREATE TABLE events (id KEYWORD PRIMARY KEY, usr KEYWORD NOT NULL, item KEYWORD NOT NULL, ts INT) OPTIONS (cluster_by = 'usr')");
    // MovieLens-like: 2,000 users of 100 events each over 3,000 items, so
    // that most pairs are new (the counts table has about as many pair
    // rows as events, as ML-1M's does: 1.78M for 994k).
    let (users, per, items) = (2000usize, 100usize, 3000usize);
    let mut n = 0;
    for chunk in (0..users).collect::<Vec<_>>().chunks(200) {
        let mut rows = Vec::new();
        for u in chunk {
            for s in 0..per {
                n += 1;
                rows.push(format!("('e{n}', 'u{u}', 'i{}', {s})", (u * 7919 + s * s * 13 + s * 101) % items));
            }
        }
        sql(&db, &format!("INSERT INTO events VALUES {}", rows.join(", ")));
    }
    db.optimize().unwrap();
    let events = n as f64;
    let before = LIVE.load(Ordering::Relaxed);
    PEAK.store(before, Ordering::Relaxed);
    sql(&db, "CREATE RECOMMENDER ml ON events(usr, item, ts) OPTIONS (window = 3, history = 20)");
    let peak = PEAK.load(Ordering::Relaxed) - before;
    let after = LIVE.load(Ordering::Relaxed) - before;
    let pairs = match db.sql("SELECT count(*) FROM ml_counts WHERE kind = 'pair'", &Params::new()).unwrap() {
        SqlResult::Rows(r) => r.rows[0][0].as_i64().unwrap(),
        _ => unreachable!(),
    };
    let per_event = peak as f64 / events;
    eprintln!(
        "{events} events, {pairs} pairs: peak {:.1} MB above before ({per_event:.0} bytes an event), {:.1} MB still held after",
        peak as f64 / MB,
        after as f64 / MB
    );
    // A bound on what counting takes an event: the events, their pairs
    // (numbered, not named twice) and the transaction's rows of counts.
    // Round 4 saw 2.6 GB more resident after 994,169 events.
    assert!(per_event < 4096.0, "{per_event:.0} bytes an event at the peak ({:.1} MB for {events} events)", peak as f64 / MB);
    // Once it has committed, the counts are in segments, not in memory.
    assert!((after as f64) < peak as f64 / 4.0, "{:.1} MB held after a peak of {:.1} MB", after as f64 / MB, peak as f64 / MB);
}
