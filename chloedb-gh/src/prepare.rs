//! `prepare`: GH Archive's hourly files into Parquet rows, one file per
//! table, shard and day, ready for `chloedb-server import` on each shard.
//!
//! A day is a unit of work: a worker downloads its 24 hours one after the
//! other, reads every event and writes the day's rows when the day is done,
//! with a marker (`done/DAY.json`, its counts) so that a run started again
//! skips the days it has. Workers are threads; downloading and reading
//! overlap across them.
//!
//! Tables (each row goes to the shard of its repo, actors to the shard of
//! their id; see [`crate::slots`]):
//! - `events`: every event, `t, kind, repo, actor, n`.
//! - `names`: a repo's `owner/name` whenever this run first sees it or sees
//!   it change.
//! - `meta`: what an event says about a repo beyond its name (description,
//!   language, stars, topics, license...), whenever it changes.
//! - `actors`: an actor's public login whenever it is new or changes.
//! - `stars`: stars and forks each repo got each day.
//!
//! Before 2015 the archive has no actor ids, only logins: `--logins DIR`
//! (the `actors` rows of a run over 2015 on) gives each login its id, and a
//! login it does not know gets a negative id of its own.

use crate::archive::{self, Event, RepoMeta};
use crate::slots;
use anyhow::{bail, Context, Result};
use arrow_array::builder::{BooleanBuilder, Int64Builder, StringBuilder};
use arrow_array::{ArrayRef, RecordBatch};
use arrow_schema::{DataType, Field, Schema};
use chrono::NaiveDate;
use parquet::arrow::ArrowWriter;
use parquet::basic::{Compression, ZstdLevel};
use parquet::file::properties::WriterProperties;
use rustc_hash::{FxHashMap, FxHasher};
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct Opts {
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub shards: usize,
    pub out: PathBuf,
    pub jobs: usize,
    pub logins: Option<PathBuf>,
    /// Before repo ids: `names` files of a later run, to give each
    /// "owner/name" its id.
    pub names: Option<PathBuf>,
    /// Logins and names are looked up only among ids below this (accounts
    /// and repos that existed before 2015), which keeps the maps small.
    pub old_ids_below: i64,
    pub base_url: String,
    /// This machine's part of the days: (i, n) takes every n-th day from i.
    pub part: (usize, usize),
    /// Read hours from this folder (`DAY-H.json.gz`) instead of downloading.
    pub local: Option<PathBuf>,
}

/// Remembers a value's hash per key, across workers, to write a row only
/// when a key is new or its value changed. GitHub's ids fit in 32 bits,
/// and a 32-bit fingerprint of the value is enough to see a change, so an
/// entry takes 8 bytes: hundreds of millions of repos fit in a few GB.
struct Seen {
    parts: Vec<Mutex<(FxHashMap<u32, u32>, FxHashMap<i64, u32>)>>,
}

impl Seen {
    fn new() -> Seen {
        Seen { parts: (0..1024).map(|_| Mutex::new(Default::default())).collect() }
    }
    /// True when `key` is new or held another value.
    fn changed(&self, key: i64, value: u64) -> bool {
        let i = ((key as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 54) as usize;
        let v = (value ^ (value >> 32)) as u32;
        let mut m = self.parts[i].lock().unwrap();
        let old = match u32::try_from(key) {
            Ok(k) => m.0.insert(k, v),
            Err(_) => m.1.insert(key, v),
        };
        old != Some(v)
    }
    fn len(&self) -> usize {
        self.parts.iter().map(|p| p.lock().map(|m| m.0.len() + m.1.len()).unwrap_or(0)).sum()
    }
}

/// The kinds of a day's events, numbered as they come.
#[derive(Default)]
struct Kinds {
    ids: FxHashMap<String, u8>,
    names: Vec<String>,
}

impl Kinds {
    fn id(&mut self, k: String) -> u8 {
        if let Some(i) = self.ids.get(&k) {
            return *i;
        }
        // GH Archive has about 60 kinds; a 256th would be one too many.
        let i = self.names.len().min(255) as u8;
        if self.names.len() < 255 {
            self.names.push(k.clone());
            self.ids.insert(k, i);
        }
        i
    }
}

fn hash_of<T: Hash + ?Sized>(v: &T) -> u64 {
    let mut h = FxHasher::default();
    v.hash(&mut h);
    h.finish()
}

/// The id a login or repo name without one gets: negative, so it never
/// meets a real id.
pub fn synthetic_id(login: &str) -> i64 {
    -((twox_hash::XxHash3_64::oneshot(login.to_ascii_lowercase().as_bytes()) >> 2) as i64) - 1
}

struct Shared {
    opts: Opts,
    names: Seen,
    meta: Seen,
    actors: Seen,
    logins: Option<FxHashMap<String, i64>>,
    /// Before ids (until 2012-09 or so): "owner/name" to the repo's id.
    repo_ids: Option<FxHashMap<String, i64>>,
    client: reqwest::blocking::Client,
    bytes: AtomicU64,
    events: AtomicU64,
    days_done: AtomicUsize,
}

#[derive(Default)]
struct Day {
    kinds: Kinds,
    events: Vec<EventRows>,
    names: Vec<NameRows>,
    meta: Vec<MetaRows>,
    actors: Vec<NameRows>,
    stars: FxHashMap<i64, (i64, i64)>,
    seen: u64,
    skipped: u64,
    bad: u64,
    missing_hours: Vec<u32>,
    bytes: u64,
}

#[derive(Default)]
struct EventRows {
    t: Vec<i64>,
    kind: Vec<u8>,
    repo: Vec<i64>,
    actor: Vec<i64>,
    n: Vec<Option<i64>>,
}

#[derive(Default)]
struct NameRows {
    id: Vec<i64>,
    t: Vec<i64>,
    name: Vec<String>,
}

#[derive(Default)]
struct MetaRows {
    repo: Vec<i64>,
    t: Vec<i64>,
    name: Vec<Option<String>>,
    description: Vec<Option<String>>,
    language: Vec<Option<String>>,
    homepage: Vec<Option<String>>,
    stars: Vec<Option<i64>>,
    forks: Vec<Option<i64>>,
    created: Vec<Option<i64>>,
    topics: Vec<Option<String>>,
    license: Vec<Option<String>>,
    fork: Vec<Option<bool>>,
    archived: Vec<Option<bool>>,
}

pub fn run(opts: Opts) -> Result<()> {
    if opts.shards == 0 || opts.part.1 == 0 || opts.part.0 >= opts.part.1 {
        bail!("--shards must be 1 or more, and --part i/n needs i < n");
    }
    std::fs::create_dir_all(opts.out.join("done"))?;
    let mut days = Vec::new();
    let mut d = opts.from;
    let mut i = 0usize;
    while d <= opts.to {
        if i % opts.part.1 == opts.part.0 && !opts.out.join("done").join(format!("{d}.json")).exists() {
            days.push(d);
        }
        d = d.succ_opt().unwrap();
        i += 1;
    }
    // The newest days first: they are the biggest, so the run ends on small ones.
    days.reverse();
    let logins = match &opts.logins {
        Some(dir) => Some(load_first_ids(dir, opts.old_ids_below, "logins")?),
        None => None,
    };
    let repo_ids = match &opts.names {
        Some(dir) => Some(load_first_ids(dir, opts.old_ids_below, "repo names")?),
        None => None,
    };
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(600))
        .connect_timeout(Duration::from_secs(20))
        .user_agent("chloedb-gh-prepare")
        .build()?;
    eprintln!("prepare: {} days to do, {} workers, {} shards, out {}", days.len(), opts.jobs, opts.shards, opts.out.display());
    let total = days.len();
    let queue = Arc::new(Mutex::new(days));
    let shared = Arc::new(Shared {
        opts,
        names: Seen::new(),
        meta: Seen::new(),
        actors: Seen::new(),
        logins,
        repo_ids,
        client,
        bytes: AtomicU64::new(0),
        events: AtomicU64::new(0),
        days_done: AtomicUsize::new(0),
    });
    let start = Instant::now();
    let failed = Arc::new(Mutex::new(Vec::new()));
    let mut threads = Vec::new();
    for _ in 0..shared.opts.jobs.max(1) {
        let (q, s, f) = (queue.clone(), shared.clone(), failed.clone());
        threads.push(std::thread::spawn(move || loop {
            let Some(day) = q.lock().unwrap().pop() else { break };
            match do_day(&s, day) {
                Ok(()) => {
                    let done = s.days_done.fetch_add(1, Ordering::Relaxed) + 1;
                    let secs = start.elapsed().as_secs_f64().max(1.0);
                    let gb = s.bytes.load(Ordering::Relaxed) as f64 / 1e9;
                    let ev = s.events.load(Ordering::Relaxed) as f64;
                    eprintln!(
                        "prepare: {day} done ({done}/{total}); {gb:.1} GB in {secs:.0} s = {:.0} MB/s, {:.0}k events/s; maps: {} names, {} actors",
                        gb * 1e3 / secs,
                        ev / secs / 1e3,
                        s.names.len(),
                        s.actors.len()
                    );
                }
                Err(e) => {
                    eprintln!("prepare: {day} FAILED: {e:#}");
                    f.lock().unwrap().push(day);
                }
            }
        }));
    }
    for t in threads {
        let _ = t.join();
    }
    let failed = failed.lock().unwrap();
    if !failed.is_empty() {
        bail!("{} days failed (run again to retry them): {:?}", failed.len(), &failed[..failed.len().min(20)]);
    }
    eprintln!("prepare: all {total} days done in {:.0} s", start.elapsed().as_secs_f64());
    Ok(())
}

fn fetch(s: &Shared, name: &str) -> Result<Option<Vec<u8>>> {
    if let Some(dir) = &s.opts.local {
        let p = dir.join(name);
        return Ok(if p.exists() { Some(std::fs::read(p)?) } else { None });
    }
    let url = format!("{}/{name}", s.opts.base_url.trim_end_matches('/'));
    let mut wait = 2u64;
    for attempt in 0.. {
        let r = s.client.get(&url).send();
        match r {
            Ok(r) if r.status() == reqwest::StatusCode::NOT_FOUND => return Ok(None),
            Ok(r) if r.status().is_success() => match r.bytes() {
                Ok(b) => return Ok(Some(b.to_vec())),
                Err(e) if attempt < 8 => eprintln!("prepare: {name}: {e}; again in {wait} s"),
                Err(e) => return Err(e.into()),
            },
            Ok(r) if attempt < 8 => eprintln!("prepare: {name}: HTTP {}; again in {wait} s", r.status()),
            Ok(r) => bail!("{name}: HTTP {}", r.status()),
            Err(e) if attempt < 8 => eprintln!("prepare: {name}: {e}; again in {wait} s"),
            Err(e) => return Err(e.into()),
        }
        std::thread::sleep(Duration::from_secs(wait));
        wait = (wait * 2).min(120);
    }
    unreachable!()
}

fn do_day(s: &Shared, day: NaiveDate) -> Result<()> {
    let n = s.opts.shards;
    let mut d = Day {
        events: (0..n).map(|_| EventRows::default()).collect(),
        names: (0..n).map(|_| NameRows::default()).collect(),
        meta: (0..n).map(|_| MetaRows::default()).collect(),
        actors: (0..n).map(|_| NameRows::default()).collect(),
        ..Default::default()
    };
    let t0 = Instant::now();
    for hour in 0..24u32 {
        let name = format!("{day}-{hour}.json.gz");
        let Some(gz) = fetch(s, &name)? else {
            d.missing_hours.push(hour);
            continue;
        };
        d.bytes += gz.len() as u64;
        s.bytes.fetch_add(gz.len() as u64, Ordering::Relaxed);
        let before = d.seen;
        read_hour(s, &mut d, &gz).with_context(|| name.clone())?;
        s.events.fetch_add(d.seen - before, Ordering::Relaxed);
    }
    write_day(s, day, &d)?;
    let marker = serde_json::json!({
        "day": day.to_string(), "events": d.seen, "skipped": d.skipped, "bad_lines": d.bad,
        "missing_hours": d.missing_hours, "gz_bytes": d.bytes, "secs": t0.elapsed().as_secs_f64(),
    });
    let p = s.opts.out.join("done").join(format!("{day}.json"));
    std::fs::write(&p, serde_json::to_vec(&marker)?)?;
    Ok(())
}

fn read_hour(s: &Shared, d: &mut Day, gz: &[u8]) -> Result<()> {
    let mut r = BufReader::with_capacity(1 << 20, flate2::read::MultiGzDecoder::new(gz));
    let mut line = Vec::with_capacity(1 << 16);
    loop {
        line.clear();
        let got = match r.read_until(b'\n', &mut line) {
            Ok(n) => n,
            // A truncated hour (a few exist): keep what was read.
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof || e.kind() == std::io::ErrorKind::InvalidInput => break,
            Err(e) => {
                eprintln!("prepare: a damaged hour, kept what was read: {e}");
                break;
            }
        };
        if got == 0 {
            break;
        }
        let text = line.trim_ascii();
        if text.is_empty() {
            continue;
        }
        match serde_json::from_slice::<Event>(text) {
            Ok(e) => one(s, d, e),
            Err(_) => d.bad += 1,
        }
    }
    // Drain anything after a damaged member so the reader is not reused.
    let _ = r.read_to_end(&mut Vec::new());
    Ok(())
}

fn one(s: &Shared, d: &mut Day, e: Event) {
    let n = s.opts.shards;
    let (Some(ty), Some(t)) = (e.ty.as_deref(), e.created_at.as_deref().and_then(archive::unix)) else {
        d.skipped += 1;
        return;
    };
    // The repo: `repo` from 2015, `repository` before, and before ids
    // (until 2012-09 or so) the id its name had later, or one of its own.
    let (repo, repo_name) = match (&e.repo, &e.repository) {
        (Some(r), _) if r.id.is_some() => (r.id.unwrap(), r.name.clone()),
        (_, Some(r)) if r.id.is_some() => (r.id.unwrap(), r.full()),
        (_, Some(r)) => match r.full() {
            Some(full) => {
                let id = s.repo_ids.as_ref().and_then(|m| m.get(&full.to_ascii_lowercase()).copied()).unwrap_or_else(|| synthetic_id(&format!("repo:{full}")));
                (id, Some(full))
            }
            None => {
                d.skipped += 1;
                return;
            }
        },
        _ => {
            d.skipped += 1;
            return;
        }
    };
    let (actor, login) = match &e.actor {
        Some(a) => match (a.id, &a.login) {
            (Some(id), l) => (id, l.clone()),
            (None, Some(l)) => {
                let id = s.logins.as_ref().and_then(|m| m.get(&l.to_ascii_lowercase()).copied()).unwrap_or_else(|| synthetic_id(l));
                (id, Some(l.clone()))
            }
            (None, None) => (0, None),
        },
        None => (0, None),
    };
    let p = e.payload.as_ref();
    let kind = archive::kind(ty, p);
    let num = archive::number(ty, p);
    let sh = slots::shard(repo, n);
    d.seen += 1;
    if kind == "star" || kind == "fork" {
        let c = d.stars.entry(repo).or_default();
        if kind == "star" {
            c.0 += 1
        } else {
            c.1 += 1
        }
    }
    let kind = d.kinds.id(kind);
    {
        let r = &mut d.events[sh];
        r.t.push(t);
        r.kind.push(kind);
        r.repo.push(repo);
        r.actor.push(actor);
        r.n.push(num);
    }
    if let Some(name) = repo_name {
        if s.names.changed(repo, hash_of(&name)) {
            let r = &mut d.names[sh];
            r.id.push(repo);
            r.t.push(t);
            r.name.push(name);
        }
    }
    if let Some(login) = login {
        if actor != 0 && s.actors.changed(actor, hash_of(&login)) {
            let r = &mut d.actors[slots::shard(actor, n)];
            r.id.push(actor);
            r.t.push(t);
            r.name.push(login);
        }
    }
    // What the event says about repos: the old format's repository, a
    // pull request's base repo, a CreateEvent's description.
    if let Some(r) = &e.repository {
        meta(s, d, r, t);
    }
    if let Some(r) = p.and_then(|p| p.pull_request.as_ref()).and_then(|r| r.base.as_ref()).and_then(|b| b.repo.as_ref()) {
        meta(s, d, r, t);
    }
    if ty == "CreateEvent" {
        if let Some(desc) = p.filter(|p| p.ref_type.as_deref() == Some("repository")).and_then(|p| p.description.clone()) {
            let m = RepoMeta { id: Some(repo), description: Some(desc), ..Default::default() };
            meta(s, d, &m, t);
        }
    }
}

fn meta(s: &Shared, d: &mut Day, r: &RepoMeta, t: i64) {
    let Some(id) = r.id else { return };
    if !r.has_details() {
        return;
    }
    let topics = r.topics.as_ref().map(|v| v.join(","));
    let created = r.created_at.as_deref().and_then(archive::unix);
    let h = hash_of(&(&r.description, &r.language, &r.homepage, r.stars(), r.fork_count(), &topics, &r.license, r.fork, r.archived, created));
    if !s.meta.changed(id, h) {
        return;
    }
    let m = &mut d.meta[slots::shard(id, s.opts.shards)];
    m.repo.push(id);
    m.t.push(t);
    m.name.push(r.full());
    m.description.push(r.description.clone());
    m.language.push(r.language.clone());
    m.homepage.push(r.homepage.clone().filter(|h| !h.is_empty()));
    m.stars.push(r.stars());
    m.forks.push(r.fork_count());
    m.created.push(created);
    m.topics.push(topics);
    m.license.push(r.license.clone());
    m.fork.push(r.fork);
    m.archived.push(r.archived);
}

fn props() -> WriterProperties {
    WriterProperties::builder()
        .set_compression(Compression::ZSTD(ZstdLevel::try_new(3).unwrap()))
        .set_max_row_group_row_count(Some(1 << 20))
        .build()
}

/// Writes the columns as one Parquet file at `path` (nothing when empty).
pub fn write_parquet(path: &Path, cols: Vec<(&str, ArrayRef)>) -> Result<()> {
    if cols.first().map(|c| c.1.len()).unwrap_or(0) == 0 {
        return Ok(());
    }
    std::fs::create_dir_all(path.parent().unwrap())?;
    let schema = Arc::new(Schema::new(cols.iter().map(|(n, a)| Field::new(*n, a.data_type().clone(), a.null_count() > 0 || matches!(a.data_type(), DataType::Null))).collect::<Vec<_>>()));
    let batch = RecordBatch::try_new(schema.clone(), cols.into_iter().map(|c| c.1).collect())?;
    let tmp = path.with_extension("parquet.tmp");
    let f = std::fs::File::create(&tmp)?;
    let mut w = ArrowWriter::try_new(f, schema, Some(props()))?;
    w.write(&batch)?;
    w.close()?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn ints(v: &[i64]) -> ArrayRef {
    let mut b = Int64Builder::with_capacity(v.len());
    b.append_slice(v);
    Arc::new(b.finish())
}
fn opt_ints(v: &[Option<i64>]) -> ArrayRef {
    let mut b = Int64Builder::with_capacity(v.len());
    for x in v {
        b.append_option(*x);
    }
    Arc::new(b.finish())
}
fn strs<S: AsRef<str>>(v: &[S]) -> ArrayRef {
    let mut b = StringBuilder::new();
    for x in v {
        b.append_value(x.as_ref());
    }
    Arc::new(b.finish())
}
fn opt_strs(v: &[Option<String>]) -> ArrayRef {
    let mut b = StringBuilder::new();
    for x in v {
        b.append_option(x.as_deref());
    }
    Arc::new(b.finish())
}
fn opt_bools(v: &[Option<bool>]) -> ArrayRef {
    let mut b = BooleanBuilder::with_capacity(v.len());
    for x in v {
        b.append_option(*x);
    }
    Arc::new(b.finish())
}

fn write_day(s: &Shared, day: NaiveDate, d: &Day) -> Result<()> {
    let out = &s.opts.out;
    let file = |table: &str, sh: usize| out.join(table).join(format!("s{sh}")).join(format!("{day}.parquet"));
    let day_start = day.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp();
    let mut stars: Vec<Vec<(i64, i64, i64)>> = vec![Vec::new(); s.opts.shards];
    for (repo, (st, fk)) in &d.stars {
        stars[slots::shard(*repo, s.opts.shards)].push((*repo, *st, *fk));
    }
    for sh in 0..s.opts.shards {
        let e = &d.events[sh];
        let kinds: Vec<&str> = e.kind.iter().map(|k| d.kinds.names[*k as usize].as_str()).collect();
        write_parquet(&file("events", sh), vec![("t", ints(&e.t)), ("kind", strs(&kinds)), ("repo", ints(&e.repo)), ("actor", ints(&e.actor)), ("n", opt_ints(&e.n))])?;
        let r = &d.names[sh];
        write_parquet(&file("names", sh), vec![("repo", ints(&r.id)), ("t", ints(&r.t)), ("name", strs(&r.name))])?;
        let r = &d.actors[sh];
        write_parquet(&file("actors", sh), vec![("actor", ints(&r.id)), ("t", ints(&r.t)), ("login", strs(&r.name))])?;
        let m = &d.meta[sh];
        write_parquet(
            &file("meta", sh),
            vec![
                ("repo", ints(&m.repo)),
                ("t", ints(&m.t)),
                ("name", opt_strs(&m.name)),
                ("description", opt_strs(&m.description)),
                ("language", opt_strs(&m.language)),
                ("homepage", opt_strs(&m.homepage)),
                ("stars", opt_ints(&m.stars)),
                ("forks", opt_ints(&m.forks)),
                ("created", opt_ints(&m.created)),
                ("topics", opt_strs(&m.topics)),
                ("license", opt_strs(&m.license)),
                ("fork", opt_bools(&m.fork)),
                ("archived", opt_bools(&m.archived)),
            ],
        )?;
        let mut st = std::mem::take(&mut stars[sh]);
        st.sort_unstable();
        let repo: Vec<i64> = st.iter().map(|x| x.0).collect();
        let n_st: Vec<i64> = st.iter().map(|x| x.1).collect();
        let n_fk: Vec<i64> = st.iter().map(|x| x.2).collect();
        let days = vec![day_start; st.len()];
        write_parquet(&file("stars", sh), vec![("repo", ints(&repo)), ("day", ints(&days)), ("stars", ints(&n_st)), ("forks", ints(&n_fk))])?;
    }
    Ok(())
}

/// Names to ids, from `actors` or `names` Parquet files under `dir` (any
/// depth; their columns are id, t, name), among ids below `below`: the id
/// that held a name first wins, which is the one that held it before 2015
/// more often than not.
fn load_first_ids(dir: &Path, below: i64, what: &str) -> Result<FxHashMap<String, i64>> {
    use arrow_array::cast::AsArray;
    use arrow_array::types::Int64Type;
    use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(p) = stack.pop() {
        for e in std::fs::read_dir(&p)? {
            let e = e?.path();
            if e.is_dir() {
                stack.push(e);
            } else if e.extension().is_some_and(|x| x == "parquet") {
                files.push(e);
            }
        }
    }
    let mut first: FxHashMap<String, (i64, i64)> = FxHashMap::default();
    for f in &files {
        let r = ParquetRecordBatchReaderBuilder::try_new(std::fs::File::open(f)?)?.build()?;
        for b in r {
            let b = b?;
            let (ids, ts, logins) = (b.column(0).as_primitive::<Int64Type>(), b.column(1).as_primitive::<Int64Type>(), b.column(2).as_string::<i32>());
            for i in 0..b.num_rows() {
                let (id, t) = (ids.value(i), ts.value(i));
                if id <= 0 || id >= below {
                    continue;
                }
                let l = logins.value(i).to_ascii_lowercase();
                match first.get_mut(&l) {
                    Some(v) if v.0 <= t => {}
                    Some(v) => *v = (t, id),
                    None => {
                        first.insert(l, (t, id));
                    }
                }
            }
        }
    }
    eprintln!("prepare: {} {what} from {} files", first.len(), files.len());
    Ok(first.into_iter().map(|(l, (_, id))| (l, id)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_ids_are_negative_and_stable() {
        let a = synthetic_id("Octocat");
        assert!(a < 0);
        assert_eq!(a, synthetic_id("octocat"));
        assert_ne!(a, synthetic_id("octocat2"));
    }

    #[test]
    fn seen_reports_new_and_changed() {
        let s = Seen::new();
        assert!(s.changed(5, 1));
        assert!(!s.changed(5, 1));
        assert!(s.changed(5, 2));
        assert_eq!(s.len(), 1);
    }
}
