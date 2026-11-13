//! Crash tests of tenants ([`chloedb::tenant`]) and of the blackboard
//! ([`chloedb::blackboard`]) on a directory store: processes that write
//! one tenant, each a node with a cache of its own, are killed at random
//! (SIGKILL) and started again on the cache they left.
//!
//! - No write a process was told is committed is lost, none it was told
//!   is refused is there, and nothing is there that no process tried.
//! - No task of a blackboard is done twice, every task is done in the
//!   end, and the timeline has each event once.
//!
//! The processes are this test binary run again with
//! `CHLOEDB_CRASH_WORKER` set, as the test `worker_process`, which does
//! nothing otherwise. A worker says what it does in a file of its own,
//! one line per `write`, before and after each call.

#![cfg(unix)]

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chloedb::blackboard::{self, Board, BoardError, NewTask, DONE};
use chloedb::db::{Db, SqlResult};
use chloedb::query::Params;
use chloedb::storage::{DirStore, ObjectStore};
use chloedb::tenant::{Tenant, TenantError, TenantOptions};
use chloedb::{Result, Value};

const WORKER: &str = "CHLOEDB_CRASH_WORKER";

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("chloedb-crash-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn store(dir: &Path) -> Arc<dyn ObjectStore> {
    Arc::new(DirStore::new(dir.join("store")).unwrap())
}

/// A generator of numbers that needs no crate: xorshift64*.
struct Rng(u64);

impl Rng {
    fn new() -> Rng {
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64 | 1;
        eprintln!("seed {seed}");
        Rng(seed)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) % n
    }
}

/// What a worker says it does: one whole line per `write`, so that a
/// kill leaves whole lines.
struct Said(std::fs::File);

impl Said {
    fn say(&mut self, line: String) {
        self.0.write_all(format!("{line}\n").as_bytes()).unwrap();
    }
}

/// The lines a worker wrote, without a last one a kill cut.
fn said(path: &Path) -> Vec<(String, String)> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let whole = text.rfind('\n').map_or("", |end| &text[..end]);
    whole.lines().filter_map(|l| l.split_once(' ')).map(|(a, b)| (a.to_string(), b.to_string())).collect()
}

/// The test binary again, as worker `n` of `mode` on `dir`, started for
/// the `run`th time.
fn spawn(mode: &str, dir: &Path, n: usize, run: usize) -> Child {
    Command::new(std::env::current_exe().unwrap())
        .args(["worker_process", "--exact", "--nocapture", "--test-threads=1"])
        .env(WORKER, mode)
        .env("CHLOEDB_CRASH_DIR", dir)
        .env("CHLOEDB_CRASH_N", n.to_string())
        .env("CHLOEDB_CRASH_RUN", run.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::from(std::fs::OpenOptions::new().create(true).append(true).open(dir.join(format!("w{n}.err"))).unwrap()))
        .spawn()
        .unwrap()
}

/// Runs `workers` of `mode` on `dir`, killing one at random and starting
/// it again every few tens of milliseconds for `chaos`, then leaves them
/// alone until `done` or `patience` has passed, and kills them.
fn run(mode: &str, dir: &Path, workers: usize, chaos: Duration, patience: Duration, done: impl Fn() -> bool) -> usize {
    let mut rng = Rng::new();
    let mut runs = vec![0usize; workers];
    let mut children: Vec<Child> = (0..workers).map(|n| spawn(mode, dir, n, 0)).collect();
    let t0 = Instant::now();
    let mut kills = 0;
    while t0.elapsed() < chaos {
        std::thread::sleep(Duration::from_millis(20 + rng.below(120)));
        let n = rng.below(workers as u64) as usize;
        children[n].kill().unwrap();
        children[n].wait().unwrap();
        kills += 1;
        runs[n] += 1;
        children[n] = spawn(mode, dir, n, runs[n]);
    }
    while !done() && t0.elapsed() < chaos + patience {
        std::thread::sleep(Duration::from_millis(50));
    }
    for c in &mut children {
        c.kill().unwrap();
        c.wait().unwrap();
    }
    kills
}

fn errors(dir: &Path, workers: usize) -> String {
    (0..workers).map(|n| std::fs::read_to_string(dir.join(format!("w{n}.err"))).unwrap_or_default()).filter(|e| e.contains("panicked")).collect::<Vec<_>>().join("\n")
}

fn ids(t: &Tenant) -> BTreeSet<i64> {
    let rows = t.read_latest(|db| Ok(match db.sql("SELECT id FROM notes LIMIT 1000000", &Params::new())? {
        SqlResult::Rows(r) => r.rows,
        SqlResult::Written(_) => Vec::new(),
    }));
    rows.unwrap().iter().map(|r| match r[0] {
        Value::Int(n) => n,
        ref other => panic!("{other:?}"),
    }).collect()
}

/// A worker of `writes`: inserts rows of its own ids, one a write, and
/// says `try`, then `ack`, `nack` (refused: another node wrote first) or
/// `doubt` for each.
fn write_rows(dir: &Path, n: usize, run: usize, mut said: Said) {
    let t = loop {
        match Tenant::open(store(dir), "t", TenantOptions::new(dir.join(format!("node{n}")))) {
            Ok(t) => break t,
            // The process this one replaces may still hold the cache.
            Err(TenantError::Db(chloedb::Error::Busy(_))) => std::thread::sleep(Duration::from_millis(1)),
            Err(e) => panic!("open: {e}"),
        }
    };
    for i in 0..i64::MAX {
        let id = (n as i64) * 1_000_000_000 + (run as i64) * 1_000_000 + i;
        said.say(format!("try {id}"));
        let wrote = t.write(|db: &Db| -> Result<()> { db.sql(&format!("INSERT INTO notes VALUES ({id}, 'row {id}')"), &Params::new()).map(|_| ()) });
        match wrote {
            Ok(()) => said.say(format!("ack {id}")),
            Err(TenantError::NotWriter) => said.say(format!("nack {id}")),
            Err(TenantError::InDoubt(_)) => said.say(format!("doubt {id}")),
            Err(e) => panic!("write {id}: {e}"),
        }
    }
}

/// A worker of `board`: claims a task, works on it for a moment, and
/// completes it, each call with a key and sent again until it is
/// answered; says `done <task>` for each task it was told is done.
fn work_tasks(dir: &Path, n: usize, run: usize, mut said: Said) {
    let t = loop {
        match Tenant::open(store(dir), "t", TenantOptions::new(dir.join(format!("node{n}")))) {
            Ok(t) => break t,
            Err(TenantError::Db(chloedb::Error::Busy(_))) => std::thread::sleep(Duration::from_millis(1)),
            Err(e) => panic!("open: {e}"),
        }
    };
    let me = Board::new(&t, &format!("w{n}")).unwrap();
    fn again<T>(call: impl Fn() -> std::result::Result<T, BoardError>) -> std::result::Result<T, String> {
        loop {
            match call() {
                Ok(x) => return Ok(x),
                Err(BoardError::Tenant(TenantError::NotWriter | TenantError::InDoubt(_))) => continue,
                Err(BoardError::Refused(why)) => return Err(why),
                Err(e) => panic!("{e}"),
            }
        }
    }
    for i in 0..i64::MAX {
        let key = format!("w{n}-{run}-{i}");
        let tasks = again(|| me.claim(1, 1, Some(&key))).unwrap();
        let Some(task) = tasks.first() else {
            std::thread::sleep(Duration::from_millis(20));
            continue;
        };
        said.say(format!("claimed {}", task.id));
        std::thread::sleep(Duration::from_millis(5 + (i as u64 * 7 + n as u64 * 13) % 40));
        match again(|| me.complete(&task.id, &format!("by w{n}, run {run}"), Some(&key))) {
            Ok(_) => said.say(format!("done {}", task.id)),
            // Its lease passed while it worked, and another has the task.
            Err(why) => said.say(format!("late {} {why}", task.id)),
        }
    }
}

#[test]
fn worker_process() {
    let Ok(mode) = std::env::var(WORKER) else { return };
    let dir = PathBuf::from(std::env::var("CHLOEDB_CRASH_DIR").unwrap());
    let n: usize = std::env::var("CHLOEDB_CRASH_N").unwrap().parse().unwrap();
    let run: usize = std::env::var("CHLOEDB_CRASH_RUN").unwrap().parse().unwrap();
    let said = Said(std::fs::OpenOptions::new().create(true).append(true).open(dir.join(format!("w{n}.said"))).unwrap());
    match mode.as_str() {
        "writes" => write_rows(&dir, n, run, said),
        "board" => work_tasks(&dir, n, run, said),
        other => panic!("no worker {other:?}"),
    }
}

#[test]
fn writers_killed_at_random_lose_no_acknowledged_write() {
    const WORKERS: usize = 3;
    let dir = tmp("writes");
    drop(Tenant::create(store(&dir), "t", TenantOptions::new(dir.join("seed")), |db| db.sql("CREATE TABLE notes (id INT PRIMARY KEY, body TEXT)", &Params::new()).map(|_| ())).unwrap());

    let kills = run("writes", &dir, WORKERS, Duration::from_secs(6), Duration::ZERO, || true);
    assert_eq!(errors(&dir, WORKERS), "", "a worker failed");

    let reader = Tenant::open(store(&dir), "t", TenantOptions::new(dir.join("reader"))).unwrap();
    let there = ids(&reader);
    let (mut tried, mut acked, mut nacked) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    for n in 0..WORKERS {
        for (what, id) in said(&dir.join(format!("w{n}.said"))) {
            let id: i64 = id.parse().unwrap();
            match what.as_str() {
                "try" => tried.insert(id),
                "ack" => acked.insert(id),
                "nack" => nacked.insert(id),
                "doubt" => true,
                other => panic!("{other}"),
            };
        }
    }
    eprintln!("{kills} kills, {} tried, {} acknowledged, {} refused, {} there", tried.len(), acked.len(), nacked.len(), there.len());
    assert!(kills > 10 && acked.len() > 50, "the test did too little: {kills} kills, {} acknowledged", acked.len());
    let lost: Vec<_> = acked.difference(&there).collect();
    assert!(lost.is_empty(), "acknowledged and lost: {lost:?}");
    let kept: Vec<_> = nacked.intersection(&there).collect();
    assert!(kept.is_empty(), "refused and there: {kept:?}");
    let unknown: Vec<_> = there.difference(&tried).collect();
    assert!(unknown.is_empty(), "there, and no process tried them: {unknown:?}");

    // Each node's cache, left by a kill, opens to the same rows.
    for n in 0..WORKERS {
        let t = Tenant::open(store(&dir), "t", TenantOptions::new(dir.join(format!("node{n}")))).unwrap();
        assert_eq!(ids(&t), there, "node {n}");
    }
    assert_eq!(chloedb::tenant::list(&*store(&dir)).unwrap(), ["t"]);
}

#[test]
fn a_blackboards_workers_killed_at_random_do_each_task_once() {
    const WORKERS: usize = 4;
    const TASKS: usize = 40;
    let dir = tmp("board");
    {
        let t = blackboard::create(store(&dir), "t", TenantOptions::new(dir.join("seed"))).unwrap();
        let tasks: Vec<NewTask> = (0..TASKS).map(|i| NewTask { id: None, title: format!("task {i}"), spec: String::new() }).collect();
        Board::new(&t, "lead").unwrap().add_tasks(&tasks, None).unwrap();
    }
    let reader = Tenant::open(store(&dir), "t", TenantOptions::new(dir.join("reader"))).unwrap();
    let board = Board::new(&reader, "reader").unwrap().latest(true);
    let all_done = || board.status().unwrap().tasks[DONE] == TASKS as i64;

    let kills = run("board", &dir, WORKERS, Duration::from_secs(4), Duration::from_secs(60), all_done);
    assert_eq!(errors(&dir, WORKERS), "", "a worker failed");
    let status = board.status().unwrap();
    eprintln!("{kills} kills, {status:?}");
    assert_eq!(status.tasks[DONE], TASKS as i64, "every task done: {status:?}");

    // The timeline has each event once, in order, and one `done` a task.
    let mut events = Vec::new();
    loop {
        let more = board.timeline(events.last().map_or(0, |e: &blackboard::Event| e.id), 500).unwrap();
        if more.is_empty() {
            break;
        }
        events.extend(more);
    }
    assert_eq!(events.iter().map(|e| e.id).collect::<Vec<_>>(), (1..=events.len() as i64).collect::<Vec<_>>());
    let mut done_by: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for e in events.iter().filter(|e| e.kind == "done") {
        done_by.entry(&e.about).or_default().push(&e.agent);
    }
    assert_eq!(done_by.len(), TASKS);
    assert!(done_by.values().all(|by| by.len() == 1), "done twice: {done_by:?}");

    // What the workers were told agrees: a task one of them was told is
    // done is done by it, and by no other.
    let mut told: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for n in 0..WORKERS {
        for (what, task) in said(&dir.join(format!("w{n}.said"))) {
            if what == "done" {
                told.entry(task).or_default().push(format!("w{n}"));
            }
        }
    }
    for (task, by) in &told {
        assert_eq!(by.len(), 1, "{task} told done to {by:?}");
        assert_eq!(done_by[task.as_str()], [by[0].as_str()], "{task}");
    }
    // Tasks claimed more than once were taken over after a kill.
    let retaken = events.iter().filter(|e| e.kind == "claim" && e.body.contains("after the lease")).count();
    eprintln!("{retaken} claims after a lease passed, {} tasks told done to their worker", told.len());
}
