//! `people`: one shard's events into what a person's page shows, one row a
//! person (`people/sSHARD/pPASS.parquet`, table `gh_people`):
//! - `events`, `stars`, `work`: how many events, stars and events of work
//!   ([`crate::pages::WORK`]) the person has in this shard's repos;
//! - `kinds`: their events by kind, `star:12,push:3`, most first;
//! - `years`: their work by year, `2015:3,2016:10`;
//! - `repos`: the 24 repos of this shard they worked on most,
//!   `repo:n:first:last;...` (first and last in seconds).
//!
//! Why: `gh_events` is sorted by repo, so one person's events are rows all
//! over it, and a person's page read the `actor` column of every event (30
//! s for anyone, refused at the server's 5 s). A repo is on one shard, so
//! a shard's repos are its own and a page adds up a person's rows of every
//! shard: four rows, in `actor` order (`cluster_by`).
//!
//! The shard's events are read `--passes` times, each pass keeping the
//! people whose id hashes to it, so that a pass's sums fit in memory beside
//! a running shard; `--threads` decode the files of a pass.

use anyhow::{Context, Result};
use arrow_array::cast::AsArray;
use arrow_array::types::Int64Type;
use arrow_array::{Array, ArrayRef, Int64Array, RecordBatch, StringArray};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ProjectionMask;
use rustc_hash::FxHashMap;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Instant;

use crate::prepare::write_parquet;

pub struct Opts {
    pub inputs: Vec<PathBuf>,
    pub shard: usize,
    pub out: PathBuf,
    pub shards: usize,
    pub passes: usize,
    pub threads: usize,
    /// The shard's kept repos (`reduce`'s `repos/sN.parquet`): who worked
    /// on each of them goes to `people_repos/sSHARD`, for [`repo_people`].
    pub kept: Option<PathBuf>,
}

/// The kinds of event that are work (as [`crate::pages::WORK`] lists them).
const WORK: [&str; 13] = [
    "push", "pr_opened", "pr_merged", "pr_closed", "issue_opened", "issue_closed", "comment", "review",
    "review_comment", "commit_comment", "release", "create_branch", "create_tag",
];

/// How many repos a person's row keeps.
pub const TOP_REPOS: usize = 24;

/// Seconds in a year, as the pages count them.
const YEAR: i64 = crate::pages::YEAR;

/// The pass a person is in.
fn pass_of(actor: i64, passes: usize) -> usize {
    (((actor as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 40) % passes as u64) as usize
}

/// Kinds by name, numbered as first seen (shared by the threads).
#[derive(Default)]
struct Kinds {
    names: Vec<String>,
    ids: FxHashMap<String, u8>,
}

impl Kinds {
    fn id(&mut self, name: &str) -> u8 {
        if let Some(i) = self.ids.get(name) {
            return *i;
        }
        let i = self.names.len().min(255) as u8;
        if self.names.len() < 255 {
            self.names.push(name.to_string());
        }
        self.ids.insert(name.to_string(), i);
        i
    }
}

/// One event of a pass: actor, kind, time, repo.
type Ev = (i64, u8, i64, i64);

fn read_file(path: &Path, pass: usize, passes: usize, kinds: &Mutex<Kinds>, tx: &mpsc::SyncSender<Vec<Ev>>) -> Result<u64> {
    let f = std::fs::File::open(path).with_context(|| path.display().to_string())?;
    let b = ParquetRecordBatchReaderBuilder::try_new(f)?;
    let mask = ProjectionMask::columns(b.parquet_schema(), ["t", "kind", "repo", "actor"]);
    let r = b.with_projection(mask).with_batch_size(65536).build()?;
    let mut local: FxHashMap<String, u8> = FxHashMap::default();
    let mut n = 0u64;
    for batch in r {
        let batch: RecordBatch = batch?;
        let col = |name: &str| batch.schema().index_of(name).ok().map(|i| batch.column(i).clone());
        let (Some(t), Some(k), Some(rp), Some(a)) = (col("t"), col("kind"), col("repo"), col("actor")) else {
            continue;
        };
        let (t, k, rp, a) = (t.as_primitive::<Int64Type>(), k.as_string::<i32>(), rp.as_primitive::<Int64Type>(), a.as_primitive::<Int64Type>());
        let mut out = Vec::with_capacity(batch.num_rows() / passes + 16);
        for i in 0..batch.num_rows() {
            if a.is_null(i) || k.is_null(i) {
                continue;
            }
            let actor = a.value(i);
            if pass_of(actor, passes) != pass {
                continue;
            }
            let name = k.value(i);
            let kind = match local.get(name) {
                Some(x) => *x,
                None => {
                    let x = kinds.lock().unwrap().id(name);
                    local.insert(name.to_string(), x);
                    x
                }
            };
            let tv = if t.is_null(i) { 0 } else { t.value(i) };
            let rv = if rp.is_null(i) { 0 } else { rp.value(i) };
            out.push((actor, kind, tv, rv));
        }
        n += batch.num_rows() as u64;
        if !out.is_empty() && tx.send(out).is_err() {
            break;
        }
    }
    Ok(n)
}

pub fn run(o: Opts) -> Result<()> {
    let sh = o.shard;
    let t0 = Instant::now();
    let files = crate::reduce::files(&o.inputs, "events", sh)?;
    anyhow::ensure!(!files.is_empty(), "no events files of shard {sh} in the inputs");
    let dir = o.out.join("people").join(format!("s{sh}"));
    std::fs::create_dir_all(&dir)?;
    // As the other sums with no key of their own: `part` places them.
    let part = (0..).find(|p| crate::slots::shard(*p, o.shards) == sh).unwrap_or(0);
    let kinds = Arc::new(Mutex::new(Kinds::default()));
    let kept_repos: rustc_hash::FxHashSet<i64> = match &o.kept {
        Some(f) => {
            let mut set = rustc_hash::FxHashSet::default();
            crate::reduce::each_batch(std::slice::from_ref(f), |b| {
                if let Some(r) = b.ints("repo") {
                    set.extend(r.iter().flatten());
                }
            })?;
            set
        }
        None => Default::default(),
    };
    let pairs_dir = o.out.join("people_repos").join(format!("s{sh}"));
    if o.kept.is_some() {
        std::fs::create_dir_all(&pairs_dir)?;
    }
    let mut total_rows = 0u64;
    for pass in 0..o.passes {
        let out_file = dir.join(format!("p{pass:02}-of-{:02}.parquet", o.passes));
        let pairs_file = pairs_dir.join(format!("p{pass:02}-of-{:02}.parquet", o.passes));
        if out_file.exists() && (o.kept.is_none() || pairs_file.exists()) {
            eprintln!("people s{sh}: pass {pass} done before");
            continue;
        }
        let (tx, rx) = mpsc::sync_channel::<Vec<Ev>>(64);
        let next = Arc::new(Mutex::new(0usize));
        let files = Arc::new(files.clone());
        let mut handles = Vec::new();
        for _ in 0..o.threads.max(1) {
            let (tx, next, files, kinds) = (tx.clone(), next.clone(), files.clone(), kinds.clone());
            let passes = o.passes;
            handles.push(std::thread::spawn(move || -> Result<u64> {
                let mut n = 0;
                loop {
                    let i = {
                        let mut g = next.lock().unwrap();
                        let i = *g;
                        *g += 1;
                        i
                    };
                    let Some(f) = files.get(i) else { return Ok(n) };
                    n += read_file(f, pass, passes, &kinds, &tx)?;
                }
            }));
        }
        drop(tx);
        // Sums of the pass's people.
        let mut by_kind: FxHashMap<(i64, u8), u32> = FxHashMap::default();
        let mut by_year: FxHashMap<(i64, u16), u32> = FxHashMap::default();
        // Times as u32 seconds (to 2106): a pass's pairs are most of its memory.
        let mut by_repo: FxHashMap<(i64, i64), (u32, u32, u32)> = FxHashMap::default();
        let mut work_ids: Vec<Option<bool>> = vec![None; 256];
        let mut kept = 0u64;
        for chunk in rx {
            for (actor, kind, t, repo) in chunk {
                kept += 1;
                *by_kind.entry((actor, kind)).or_default() += 1;
                let work = match work_ids[kind as usize] {
                    Some(w) => w,
                    None => {
                        let w = kinds.lock().unwrap().names.get(kind as usize).is_some_and(|n| WORK.contains(&n.as_str()));
                        work_ids[kind as usize] = Some(w);
                        w
                    }
                };
                if !work {
                    continue;
                }
                *by_year.entry((actor, (1970 + t / YEAR) as u16)).or_default() += 1;
                let t = t.clamp(0, u32::MAX as i64) as u32;
                let e = by_repo.entry((actor, repo)).or_insert((0, t, t));
                e.0 += 1;
                e.1 = e.1.min(t);
                e.2 = e.2.max(t);
            }
        }
        let mut read = 0u64;
        for h in handles {
            read += h.join().map_err(|_| anyhow::anyhow!("a reading thread panicked"))??;
        }
        let names = kinds.lock().unwrap().names.clone();
        let is_star = |k: u8| names.get(k as usize).is_some_and(|n| n == "star");
        let is_work = |k: u8| names.get(k as usize).is_some_and(|n| WORK.contains(&n.as_str()));
        // A person's rows: by actor, then most first.
        let mut kv: Vec<(i64, u8, u32)> = by_kind.into_iter().map(|((a, k), n)| (a, k, n)).collect();
        kv.sort_unstable_by(|x, y| x.0.cmp(&y.0).then(y.2.cmp(&x.2)));
        let mut yv: Vec<(i64, u16, u32)> = by_year.into_iter().map(|((a, y), n)| (a, y, n)).collect();
        yv.sort_unstable();
        let mut rv: Vec<(i64, i64, u32, u32, u32)> = by_repo.into_iter().map(|((a, r), (n, f, l))| (a, r, n, f, l)).collect();
        rv.sort_unstable_by(|x, y| x.0.cmp(&y.0).then(y.2.cmp(&x.2)).then(x.1.cmp(&y.1)));
        let (mut actors, mut events, mut stars, mut works) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let (mut ks, mut ys, mut rs) = (Vec::<String>::new(), Vec::<String>::new(), Vec::<String>::new());
        let (mut yi, mut ri) = (0usize, 0usize);
        let mut i = 0usize;
        while i < kv.len() {
            let a = kv[i].0;
            let (mut ev, mut st, mut wk) = (0i64, 0i64, 0i64);
            let mut k = String::new();
            while i < kv.len() && kv[i].0 == a {
                let (_, kind, n) = kv[i];
                ev += n as i64;
                if is_star(kind) {
                    st += n as i64;
                }
                if is_work(kind) {
                    wk += n as i64;
                }
                if !k.is_empty() {
                    k.push(',');
                }
                k.push_str(names.get(kind as usize).map(String::as_str).unwrap_or("other"));
                k.push(':');
                k.push_str(&n.to_string());
                i += 1;
            }
            while yi < yv.len() && yv[yi].0 < a {
                yi += 1;
            }
            let mut y = String::new();
            while yi < yv.len() && yv[yi].0 == a {
                if !y.is_empty() {
                    y.push(',');
                }
                y.push_str(&format!("{}:{}", yv[yi].1, yv[yi].2));
                yi += 1;
            }
            while ri < rv.len() && rv[ri].0 < a {
                ri += 1;
            }
            let mut r = String::new();
            let mut taken = 0;
            while ri < rv.len() && rv[ri].0 == a {
                if taken < TOP_REPOS {
                    let (_, repo, n, f, l) = rv[ri];
                    if !r.is_empty() {
                        r.push(';');
                    }
                    r.push_str(&format!("{repo}:{n}:{f}:{l}"));
                    taken += 1;
                }
                ri += 1;
            }
            actors.push(a);
            events.push(ev);
            stars.push(st);
            works.push(wk);
            ks.push(k);
            ys.push(y);
            rs.push(r);
        }
        let rows = actors.len();
        let col = |v: Vec<i64>| Arc::new(Int64Array::from(v)) as ArrayRef;
        if o.kept.is_some() {
            // Who worked on the kept repos, every pair (a person is in one
            // pass, so each pair is in one file).
            let keep: Vec<&(i64, i64, u32, u32, u32)> = rv.iter().filter(|x| kept_repos.contains(&x.1)).collect();
            write_parquet(
                &pairs_file,
                vec![
                    ("repo", col(keep.iter().map(|x| x.1).collect())),
                    ("actor", col(keep.iter().map(|x| x.0).collect())),
                    ("n", col(keep.iter().map(|x| x.2 as i64).collect())),
                    ("first", col(keep.iter().map(|x| x.3 as i64).collect())),
                    ("last", col(keep.iter().map(|x| x.4 as i64).collect())),
                ],
            )?;
        }
        let txt = |v: Vec<String>| Arc::new(StringArray::from(v)) as ArrayRef;
        write_parquet(
            &out_file,
            vec![
                ("part", col(vec![part; rows])),
                ("actor", col(actors)),
                ("events", col(events)),
                ("stars", col(stars)),
                ("work", col(works)),
                ("kinds", txt(ks)),
                ("years", txt(ys)),
                ("repos", txt(rs)),
            ],
        )?;
        total_rows += rows as u64;
        eprintln!(
            "people s{sh}: pass {}/{}: {read} events read, {kept} kept, {rows} people ({:.0} s)",
            pass + 1,
            o.passes,
            t0.elapsed().as_secs_f64()
        );
    }
    eprintln!("people s{sh}: {total_rows} rows in {} ({:.0} s)", dir.display(), t0.elapsed().as_secs_f64());
    Ok(())
}

/// How many people a repo's row of `gh_repo_people` lists.
pub const TOP_PEOPLE: usize = 30;

/// `repo-people`: [`run`]'s pairs of a shard (`people_repos/sSHARD`) into
/// the people who worked most on each kept repo, [`TOP_PEOPLE`] rows a repo
/// (`repo_people/sSHARD/pPASS.parquet`, table `gh_repo_people`: repo,
/// actor, n, first, last), so that a repo's page reads them rather than
/// every event of a busy repo (seconds from the disk). `passes` split the
/// repos.
pub fn repo_people(out: &Path, shard: usize, passes: usize) -> Result<()> {
    let t0 = Instant::now();
    let src = crate::reduce::files(&[out.to_path_buf()], "people_repos", shard)?;
    anyhow::ensure!(!src.is_empty(), "no people_repos files of shard {shard} under {}", out.display());
    let dir = out.join("repo_people").join(format!("s{shard}"));
    std::fs::create_dir_all(&dir)?;
    let mut total = 0usize;
    for pass in 0..passes {
        let mut v: Vec<(i64, u32, i64, u32, u32)> = Vec::new();
        crate::reduce::each_batch(&src, |b| {
            let (r, a, n, f, l) = (b.ints("repo"), b.ints("actor"), b.ints("n"), b.ints("first"), b.ints("last"));
            let (Some(r), Some(a), Some(n), Some(f), Some(l)) = (r, a, n, f, l) else { return };
            for i in 0..b.rows() {
                let repo = r.value(i);
                if pass_of(repo, passes) == pass {
                    v.push((repo, n.value(i) as u32, a.value(i), f.value(i) as u32, l.value(i) as u32));
                }
            }
        })?;
        v.sort_unstable_by(|x, y| x.0.cmp(&y.0).then(y.1.cmp(&x.1)).then(x.2.cmp(&y.2)));
        let mut keep = Vec::new();
        let mut i = 0;
        while i < v.len() {
            let mut j = i;
            while j < v.len() && v[j].0 == v[i].0 {
                if j - i < TOP_PEOPLE {
                    keep.push(v[j]);
                }
                j += 1;
            }
            i = j;
        }
        let col = |x: Vec<i64>| Arc::new(Int64Array::from(x)) as ArrayRef;
        write_parquet(
            &dir.join(format!("p{pass:02}-of-{passes:02}.parquet")),
            vec![
                ("repo", col(keep.iter().map(|x| x.0).collect())),
                ("actor", col(keep.iter().map(|x| x.2).collect())),
                ("n", col(keep.iter().map(|x| x.1 as i64).collect())),
                ("first", col(keep.iter().map(|x| x.3 as i64).collect())),
                ("last", col(keep.iter().map(|x| x.4 as i64).collect())),
            ],
        )?;
        total += keep.len();
        eprintln!("repo-people s{shard}: pass {}/{passes}: {} pairs, {} kept ({:.0} s)", pass + 1, v.len(), keep.len(), t0.elapsed().as_secs_f64());
    }
    eprintln!("repo-people s{shard}: {total} rows in {} ({:.0} s)", dir.display(), t0.elapsed().as_secs_f64());
    Ok(())
}

/// A person's rows of every shard, added up: what [`run`] wrote.
#[derive(Debug, Default, PartialEq)]
pub struct Summary {
    pub events: i64,
    pub stars: i64,
    pub work: i64,
    /// Kind and count, most first.
    pub kinds: Vec<(String, i64)>,
    /// Year and count of work, oldest first.
    pub years: Vec<(i64, i64)>,
    /// Repo, events of work, first and last: most first, [`TOP_REPOS`].
    pub repos: Vec<(i64, i64, i64, i64)>,
}

/// Adds up a person's `gh_people` rows (each with `events`, `stars`,
/// `work`, `kinds`, `years` and `repos`).
pub fn summary<'a>(rows: impl IntoIterator<Item = (i64, i64, i64, &'a str, &'a str, &'a str)>) -> Summary {
    let mut s = Summary::default();
    let mut kinds: FxHashMap<String, i64> = FxHashMap::default();
    let mut years: FxHashMap<i64, i64> = FxHashMap::default();
    for (events, stars, work, k, y, r) in rows {
        s.events += events;
        s.stars += stars;
        s.work += work;
        for part in k.split(',').filter(|p| !p.is_empty()) {
            if let Some((name, n)) = part.rsplit_once(':') {
                *kinds.entry(name.to_string()).or_default() += n.parse::<i64>().unwrap_or(0);
            }
        }
        for part in y.split(',').filter(|p| !p.is_empty()) {
            if let Some((yr, n)) = part.split_once(':') {
                *years.entry(yr.parse().unwrap_or(0)).or_default() += n.parse::<i64>().unwrap_or(0);
            }
        }
        for part in r.split(';').filter(|p| !p.is_empty()) {
            let v: Vec<i64> = part.split(':').map(|x| x.parse().unwrap_or(0)).collect();
            if let [repo, n, f, l] = v[..] {
                s.repos.push((repo, n, f, l));
            }
        }
    }
    s.kinds = kinds.into_iter().collect();
    s.kinds.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    s.years = years.into_iter().collect();
    s.years.sort();
    s.repos.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    s.repos.truncate(TOP_REPOS);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_is_the_pages() {
        let listed: Vec<String> = crate::pages::WORK
            .trim_matches(|c| c == '(' || c == ')')
            .split(", ")
            .map(|s| s.trim_matches('\'').to_string())
            .collect();
        assert_eq!(listed, WORK.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    }

    #[test]
    fn rows_of_shards_add_up() {
        let s = summary([
            (10, 4, 6, "star:4,push:5,comment:1", "2015:2,2016:4", "7:5:100:200;8:1:150:150"),
            (3, 0, 3, "push:3", "2016:3", "9:3:50:300"),
        ]);
        assert_eq!((s.events, s.stars, s.work), (13, 4, 9));
        assert_eq!(s.kinds, vec![("push".into(), 8), ("star".into(), 4), ("comment".into(), 1)]);
        assert_eq!(s.years, vec![(2015, 2), (2016, 7)]);
        assert_eq!(s.repos, vec![(7, 5, 100, 200), (9, 3, 50, 300), (8, 1, 150, 150)]);
    }

    #[test]
    fn a_pass_writes_each_person_once() {
        let dir = tempfile::tempdir().unwrap();
        let ev = dir.path().join("in/events/s0");
        std::fs::create_dir_all(&ev).unwrap();
        let rows: Vec<(i64, &str, i64, i64)> = vec![
            (100, "push", 1, 7),
            (YEAR * 46, "push", 1, 7),
            (200, "star", 1, 8),
            (300, "comment", 2, 7),
            (400, "star", 3, 9),
        ];
        write_parquet(
            &ev.join("2016-01-01.parquet"),
            vec![
                ("t", Arc::new(Int64Array::from(rows.iter().map(|r| r.0).collect::<Vec<_>>())) as ArrayRef),
                ("kind", Arc::new(StringArray::from(rows.iter().map(|r| r.1).collect::<Vec<_>>())) as ArrayRef),
                ("repo", Arc::new(Int64Array::from(rows.iter().map(|r| r.3).collect::<Vec<_>>())) as ArrayRef),
                ("actor", Arc::new(Int64Array::from(rows.iter().map(|r| r.2).collect::<Vec<_>>())) as ArrayRef),
            ],
        )
        .unwrap();
        let kept = dir.path().join("repos.parquet");
        write_parquet(&kept, vec![("repo", Arc::new(Int64Array::from(vec![7i64, 9])) as ArrayRef)]).unwrap();
        run(Opts { inputs: vec![dir.path().join("in")], shard: 0, out: dir.path().join("out"), shards: 1, passes: 3, threads: 2, kept: Some(kept) }).unwrap();
        repo_people(&dir.path().join("out"), 0, 2).unwrap();
        let mut pairs = Vec::new();
        for e in std::fs::read_dir(dir.path().join("out/repo_people/s0")).unwrap() {
            let r = ParquetRecordBatchReaderBuilder::try_new(std::fs::File::open(e.unwrap().path()).unwrap()).unwrap().build().unwrap();
            for b in r {
                let b = b.unwrap();
                let c = |i: usize| b.column(i).as_primitive::<Int64Type>().clone();
                let (r, a, n) = (c(0), c(1), c(2));
                for i in 0..b.num_rows() {
                    pairs.push((r.value(i), a.value(i), n.value(i)));
                }
            }
        }
        pairs.sort();
        // Repo 8 is not kept; repo 9 has a star only, which is not work.
        assert_eq!(pairs, vec![(7, 1, 2), (7, 2, 1)]);
        let mut people = Vec::new();
        for e in std::fs::read_dir(dir.path().join("out/people/s0")).unwrap() {
            let r = ParquetRecordBatchReaderBuilder::try_new(std::fs::File::open(e.unwrap().path()).unwrap()).unwrap().build().unwrap();
            for b in r {
                let b = b.unwrap();
                let a = b.column(1).as_primitive::<Int64Type>();
                let ev = b.column(2).as_primitive::<Int64Type>();
                let k = b.column(5).as_string::<i32>();
                let y = b.column(6).as_string::<i32>();
                let rp = b.column(7).as_string::<i32>();
                for i in 0..b.num_rows() {
                    people.push((a.value(i), ev.value(i), k.value(i).to_string(), y.value(i).to_string(), rp.value(i).to_string()));
                }
            }
        }
        people.sort();
        assert_eq!(
            people,
            vec![
                (1, 3, "push:2,star:1".into(), "1970:1,2016:1".into(), format!("7:2:100:{}", YEAR * 46)),
                (2, 1, "comment:1".into(), "1970:1".into(), "7:1:300:300".into()),
                (3, 1, "star:1".into(), "".into(), "".into()),
            ]
        );
    }
}
