//! `reduce`: one shard's rows from `prepare` into the rows its keyed and
//! summary tables load. A bulk load refuses a key given twice, and `prepare`
//! dedupes only within a day, so this keeps one row a key across days:
//! - `repos/sN.parquet`: each repo with at least `--min-stars` stars in the
//!   archive, its latest name and what events last said of it
//!   (description, language, topics, license...), and the stars and forks
//!   the archive counted.
//! - `repo_names/sN.parquet`: every name those repos had, with when it was
//!   first seen, so that an old link finds the repo.
//! - `actors/sN.parquet`: each actor's latest login, first and last seen.
//! - `repo_months/sN/MONTH.parquet`: what happened in each of those repos
//!   each month (stars, forks, pushes, commits, pull requests opened and
//!   merged, issues, comments, releases, and how many people took part).
//! - `lang_months/sN.parquet`, `topic_months/sN.parquet`: the same summed
//!   by language and by topic, for the trends (a shard's part: the cluster
//!   sums the shards).
//!
//! Inputs are one or more `prepare` folders; a table's files are read in
//! the order of their days, so the last value read is the newest.

use anyhow::Result;
use arrow_array::cast::AsArray;
use arrow_array::types::Int64Type;
use arrow_array::{Array, ArrayRef, BooleanArray, Int64Array, RecordBatch, StringArray};
use chrono::{Datelike, TimeZone, Utc};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use rustc_hash::{FxHashMap, FxHashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use crate::prepare::write_parquet;

pub struct Opts {
    pub inputs: Vec<PathBuf>,
    pub shard: usize,
    pub out: PathBuf,
    pub min_stars: i64,
    pub shards: usize,
}

/// A shard's files of `table` in every input, oldest day first.
fn files(inputs: &[PathBuf], table: &str, shard: usize) -> Result<Vec<PathBuf>> {
    let mut v = Vec::new();
    for i in inputs {
        let d = i.join(table).join(format!("s{shard}"));
        if !d.exists() {
            continue;
        }
        for e in std::fs::read_dir(&d)? {
            let p = e?.path();
            if p.extension().is_some_and(|x| x == "parquet") {
                v.push(p);
            }
        }
    }
    v.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    Ok(v)
}

fn each_batch(files: &[PathBuf], mut f: impl FnMut(&Batch)) -> Result<()> {
    for p in files {
        let r = ParquetRecordBatchReaderBuilder::try_new(std::fs::File::open(p)?)?.with_batch_size(65536).build()?;
        for b in r {
            f(&Batch(b?));
        }
    }
    Ok(())
}

/// A batch's columns by name, read once a batch.
struct Batch(RecordBatch);

impl Batch {
    fn col(&self, name: &str) -> Option<&ArrayRef> {
        self.0.schema().index_of(name).ok().map(|i| self.0.column(i))
    }
    fn ints(&self, name: &str) -> Option<&arrow_array::Int64Array> {
        self.col(name).map(|c| c.as_primitive::<Int64Type>())
    }
    fn strs(&self, name: &str) -> Option<&StringArray> {
        self.col(name).map(|c| c.as_string::<i32>())
    }
    fn bools(&self, name: &str) -> Option<&BooleanArray> {
        self.col(name).map(|c| c.as_boolean())
    }
    fn rows(&self) -> usize {
        self.0.num_rows()
    }
}

fn at_i(c: Option<&Int64Array>, i: usize) -> Option<i64> {
    c.filter(|c| c.is_valid(i)).map(|c| c.value(i))
}
fn at_s(c: Option<&StringArray>, i: usize) -> Option<&str> {
    c.filter(|c| c.is_valid(i)).map(|c| c.value(i))
}
fn at_b(c: Option<&BooleanArray>, i: usize) -> Option<bool> {
    c.filter(|c| c.is_valid(i)).map(|c| c.value(i))
}

#[derive(Default, Clone)]
struct Repo {
    name: Option<String>,
    name_t: i64,
    first_seen: Option<i64>,
    description: Option<String>,
    language: Option<String>,
    homepage: Option<String>,
    topics: Option<String>,
    license: Option<String>,
    fork: Option<bool>,
    archived: Option<bool>,
    created: Option<i64>,
    stars_api: Option<i64>,
    forks_api: Option<i64>,
    api_t: Option<i64>,
    stars: i64,
    forks: i64,
}

/// What happened in a repo (or a language, or a topic) in a month.
#[derive(Default, Clone, Copy)]
struct Month {
    stars: i64,
    forks: i64,
    pushes: i64,
    commits: i64,
    prs: i64,
    merged: i64,
    issues: i64,
    comments: i64,
    releases: i64,
    people: i64,
    repos: i64,
}

impl Month {
    fn add(&mut self, o: &Month) {
        self.stars += o.stars;
        self.forks += o.forks;
        self.pushes += o.pushes;
        self.commits += o.commits;
        self.prs += o.prs;
        self.merged += o.merged;
        self.issues += o.issues;
        self.comments += o.comments;
        self.releases += o.releases;
        self.people += o.people;
        self.repos += o.repos;
    }
}

/// The first second of the month of `t` (UTC).
fn month_of(t: i64) -> i64 {
    let d = Utc.timestamp_opt(t, 0).single().unwrap_or_default();
    Utc.with_ymd_and_hms(d.year(), d.month(), 1, 0, 0, 0).single().map_or(0, |m| m.timestamp())
}

fn month_cols<K>(rows: &[(K, Month)]) -> Vec<(&'static str, ArrayRef)> {
    let c = |f: &dyn Fn(&Month) -> i64| -> ArrayRef { Arc::new(rows.iter().map(|r| f(&r.1)).collect::<Int64Array>()) };
    vec![
        ("stars", c(&|m| m.stars)),
        ("forks", c(&|m| m.forks)),
        ("pushes", c(&|m| m.pushes)),
        ("commits", c(&|m| m.commits)),
        ("prs", c(&|m| m.prs)),
        ("merged", c(&|m| m.merged)),
        ("issues", c(&|m| m.issues)),
        ("comments", c(&|m| m.comments)),
        ("releases", c(&|m| m.releases)),
        ("people", c(&|m| m.people)),
        ("repos", c(&|m| m.repos)),
    ]
}

pub fn run(o: Opts) -> Result<()> {
    let sh = o.shard;
    let t0 = Instant::now();
    std::fs::create_dir_all(&o.out)?;
    // Stars and forks first: they decide which repos are kept.
    let mut counts: FxHashMap<i64, (i64, i64)> = FxHashMap::default();
    each_batch(&files(&o.inputs, "stars", sh)?, |b| {
        let (r, s, f) = (b.ints("repo"), b.ints("stars"), b.ints("forks"));
        for i in 0..b.rows() {
            let Some(repo) = at_i(r, i) else { continue };
            let c = counts.entry(repo).or_default();
            c.0 += at_i(s, i).unwrap_or(0);
            c.1 += at_i(f, i).unwrap_or(0);
        }
    })?;
    let mut repos: FxHashMap<i64, Repo> = counts.iter().filter(|(_, c)| c.0 >= o.min_stars).map(|(r, c)| (*r, Repo { stars: c.0, forks: c.1, ..Default::default() })).collect();
    eprintln!("reduce s{sh}: {} repos with stars or forks, {} with {}+ stars ({:.0} s)", counts.len(), repos.len(), o.min_stars, t0.elapsed().as_secs_f64());
    drop(counts);
    // Names: the latest, and every one with when it was first seen.
    let mut names: FxHashMap<(i64, String), i64> = FxHashMap::default();
    each_batch(&files(&o.inputs, "names", sh)?, |b| {
        let (ids, ts, ns) = (b.ints("repo"), b.ints("t"), b.strs("name"));
        for i in 0..b.rows() {
            let (Some(id), Some(t), Some(n)) = (at_i(ids, i), at_i(ts, i), at_s(ns, i)) else { continue };
            let Some(r) = repos.get_mut(&id) else { continue };
            r.first_seen = Some(r.first_seen.map_or(t, |f| f.min(t)));
            let e = names.entry((id, n.to_string())).or_insert(t);
            *e = (*e).min(t);
            if t >= r.name_t {
                r.name = Some(n.to_string());
                r.name_t = t;
            }
        }
    })?;
    // What events said of each repo, newest last.
    each_batch(&files(&o.inputs, "meta", sh)?, |b| {
        let (ids, ts) = (b.ints("repo"), b.ints("t"));
        let (desc, lang, home, topics, lic, name) = (b.strs("description"), b.strs("language"), b.strs("homepage"), b.strs("topics"), b.strs("license"), b.strs("name"));
        let (fork, arch, created, stars, forks) = (b.bools("fork"), b.bools("archived"), b.ints("created"), b.ints("stars"), b.ints("forks"));
        for i in 0..b.rows() {
            let Some(id) = at_i(ids, i) else { continue };
            let Some(r) = repos.get_mut(&id) else { continue };
            let t = at_i(ts, i);
            macro_rules! set_s {
                ($f:ident, $c:expr) => {
                    if let Some(v) = at_s($c, i) {
                        r.$f = Some(v.to_string());
                    }
                };
            }
            set_s!(description, desc);
            set_s!(language, lang);
            set_s!(homepage, home);
            set_s!(topics, topics);
            set_s!(license, lic);
            if let Some(v) = at_b(fork, i) {
                r.fork = Some(v);
            }
            if let Some(v) = at_b(arch, i) {
                r.archived = Some(v);
            }
            if let Some(v) = at_i(created, i) {
                r.created = Some(v);
            }
            if let Some(s) = at_i(stars, i) {
                r.stars_api = Some(s);
                r.forks_api = at_i(forks, i).or(r.forks_api);
                r.api_t = t;
            }
            if r.name.is_none() {
                r.name = at_s(name, i).map(str::to_string);
            }
        }
    })?;
    repos.retain(|_, r| r.name.is_some());
    let mut ids: Vec<i64> = repos.keys().copied().collect();
    ids.sort_unstable();
    {
        let rs: Vec<&Repo> = ids.iter().map(|id| &repos[id]).collect();
        let s = |f: &dyn Fn(&Repo) -> Option<String>| -> ArrayRef { Arc::new(rs.iter().map(|r| f(r)).collect::<StringArray>()) };
        let n = |f: &dyn Fn(&Repo) -> Option<i64>| -> ArrayRef { Arc::new(rs.iter().map(|r| f(r)).collect::<Int64Array>()) };
        let bo = |f: &dyn Fn(&Repo) -> Option<bool>| -> ArrayRef { Arc::new(rs.iter().map(|r| f(r)).collect::<BooleanArray>()) };
        write_parquet(
            &o.out.join("repos").join(format!("s{sh}.parquet")),
            vec![
                ("repo", Arc::new(Int64Array::from(ids.clone())) as ArrayRef),
                ("name", s(&|r| r.name.clone())),
                ("owner", s(&|r| r.name.as_deref().and_then(|n| n.split_once('/')).map(|(o, _)| o.to_string()))),
                // GitHub's names ignore case: these find "Owner/Name" by any.
                ("lname", s(&|r| r.name.as_deref().map(str::to_lowercase))),
                ("lowner", s(&|r| r.name.as_deref().and_then(|n| n.split_once('/')).map(|(o, _)| o.to_lowercase()))),
                ("description", s(&|r| r.description.clone())),
                ("language", s(&|r| r.language.clone())),
                ("homepage", s(&|r| r.homepage.clone())),
                ("topics", s(&|r| r.topics.clone())),
                ("license", s(&|r| r.license.clone())),
                ("fork", bo(&|r| r.fork)),
                ("archived", bo(&|r| r.archived)),
                ("created", n(&|r| r.created)),
                ("first_seen", n(&|r| r.first_seen)),
                ("stars", n(&|r| Some(r.stars))),
                ("forks", n(&|r| Some(r.forks))),
                ("stars_api", n(&|r| r.stars_api)),
                ("forks_api", n(&|r| r.forks_api)),
                ("api_t", n(&|r| r.api_t)),
            ],
        )?;
    }
    let mut nm: Vec<((i64, String), i64)> = names.into_iter().collect();
    nm.sort_unstable();
    write_parquet(
        &o.out.join("repo_names").join(format!("s{sh}.parquet")),
        vec![
            ("repo", Arc::new(nm.iter().map(|x| x.0 .0).collect::<Int64Array>()) as ArrayRef),
            ("name", Arc::new(nm.iter().map(|x| Some(x.0 .1.as_str())).collect::<StringArray>())),
            ("lname", Arc::new(nm.iter().map(|x| Some(x.0 .1.to_lowercase())).collect::<StringArray>())),
            ("first_seen", Arc::new(nm.iter().map(|x| x.1).collect::<Int64Array>())),
        ],
    )?;
    eprintln!("reduce s{sh}: wrote {} repos and {} names ({:.0} s)", ids.len(), nm.len(), t0.elapsed().as_secs_f64());
    drop(nm);
    months(&o, &repos, t0)?;
    drop(repos);
    // Actors: the latest login, first and last seen.
    let mut actors: FxHashMap<i64, (String, i64, i64)> = FxHashMap::default();
    each_batch(&files(&o.inputs, "actors", sh)?, |b| {
        let (ids, ts, ls) = (b.ints("actor"), b.ints("t"), b.strs("login"));
        for i in 0..b.rows() {
            let (Some(id), Some(t), Some(l)) = (at_i(ids, i), at_i(ts, i), at_s(ls, i)) else { continue };
            match actors.get_mut(&id) {
                Some(e) => {
                    if t >= e.2 {
                        if e.0 != l {
                            e.0 = l.to_string();
                        }
                        e.2 = t;
                    }
                    e.1 = e.1.min(t);
                }
                None => {
                    actors.insert(id, (l.to_string(), t, t));
                }
            }
        }
    })?;
    let mut av: Vec<(i64, (String, i64, i64))> = actors.into_iter().collect();
    av.sort_unstable_by_key(|x| x.0);
    write_parquet(
        &o.out.join("actors").join(format!("s{sh}.parquet")),
        vec![
            ("actor", Arc::new(av.iter().map(|x| x.0).collect::<Int64Array>()) as ArrayRef),
            ("login", Arc::new(av.iter().map(|x| Some(x.1 .0.as_str())).collect::<StringArray>())),
            ("llogin", Arc::new(av.iter().map(|x| Some(x.1 .0.to_lowercase())).collect::<StringArray>())),
            ("first_seen", Arc::new(av.iter().map(|x| x.1 .1).collect::<Int64Array>())),
            ("last_seen", Arc::new(av.iter().map(|x| x.1 .2).collect::<Int64Array>())),
        ],
    )?;
    eprintln!("reduce s{sh}: wrote {} actors; done in {:.0} s", av.len(), t0.elapsed().as_secs_f64());
    Ok(())
}

/// The kept repos' months from the events, a file a month, and the
/// shard's sums by language and topic.
fn months(o: &Opts, repos: &FxHashMap<i64, Repo>, t0: Instant) -> Result<()> {
    let sh = o.shard;
    let dir = o.out.join("repo_months").join(format!("s{sh}"));
    std::fs::create_dir_all(&dir)?;
    let mut month = i64::MIN;
    let mut cur: FxHashMap<i64, Month> = FxHashMap::default();
    let mut people: FxHashSet<(i64, i64)> = FxHashSet::default();
    let mut by_lang: FxHashMap<(i64, String), Month> = FxHashMap::default();
    let mut by_topic: FxHashMap<(i64, String), Month> = FxHashMap::default();
    let mut rows = 0u64;
    let mut flush = |month: i64, cur: &mut FxHashMap<i64, Month>, people: &mut FxHashSet<(i64, i64)>| -> Result<()> {
        if cur.is_empty() {
            return Ok(());
        }
        for (repo, _) in people.drain() {
            if let Some(m) = cur.get_mut(&repo) {
                m.people += 1;
            }
        }
        let mut v: Vec<(i64, Month)> = cur.drain().collect();
        v.sort_unstable_by_key(|x| x.0);
        for (repo, m) in &mut v {
            m.repos = 1;
            let Some(r) = repos.get(repo) else { continue };
            if let Some(l) = &r.language {
                by_lang.entry((month, l.clone())).or_default().add(m);
            }
            for t in r.topics.iter().flat_map(|t| t.split(',')).filter(|t| !t.is_empty()) {
                by_topic.entry((month, t.to_string())).or_default().add(m);
            }
        }
        let name = Utc.timestamp_opt(month, 0).single().map(|d| d.format("%Y-%m").to_string()).unwrap_or_default();
        let mut cols = vec![("repo", Arc::new(v.iter().map(|x| x.0).collect::<Int64Array>()) as ArrayRef), ("month", Arc::new(Int64Array::from(vec![month; v.len()])) as ArrayRef)];
        cols.extend(month_cols(&v));
        write_parquet(&dir.join(format!("{name}.parquet")), cols)
    };
    for p in files(&o.inputs, "events", sh)? {
        let r = ParquetRecordBatchReaderBuilder::try_new(std::fs::File::open(&p)?)?.with_batch_size(65536).build()?;
        for b in r {
            let b = Batch(b?);
            let (ts, kinds, rs, actors, ns) = (b.ints("t"), b.strs("kind"), b.ints("repo"), b.ints("actor"), b.ints("n"));
            for i in 0..b.rows() {
                rows += 1;
                let (Some(t), Some(k), Some(repo)) = (at_i(ts, i), at_s(kinds, i), at_i(rs, i)) else { continue };
                if !repos.contains_key(&repo) {
                    continue;
                }
                let m = month_of(t);
                if m != month {
                    // Days are read in order, so a month ends once.
                    if m > month {
                        flush(month, &mut cur, &mut people)?;
                        month = m;
                    }
                }
                let c = cur.entry(repo).or_default();
                match k {
                    "star" => c.stars += 1,
                    "fork" => c.forks += 1,
                    "push" => {
                        c.pushes += 1;
                        c.commits += at_i(ns, i).unwrap_or(0).max(0);
                    }
                    "pr_opened" => c.prs += 1,
                    "pr_merged" => c.merged += 1,
                    "issue_opened" => c.issues += 1,
                    "comment" | "review" | "review_comment" | "commit_comment" => c.comments += 1,
                    "release" => c.releases += 1,
                    _ => {}
                }
                if k != "star" && k != "fork" {
                    if let Some(a) = at_i(actors, i).filter(|a| *a != 0) {
                        people.insert((repo, a));
                    }
                }
            }
        }
    }
    flush(month, &mut cur, &mut people)?;
    drop(flush);
    eprintln!("reduce s{sh}: months from {rows} events ({:.0} s)", t0.elapsed().as_secs_f64());
    for (table, map) in [("lang_months", by_lang), ("topic_months", by_topic)] {
        let mut v: Vec<((i64, String), Month)> = map.into_iter().collect();
        v.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        let key = if table == "lang_months" { "language" } else { "topic" };
        // These sums have no key of their own: `part` places them (shard_by),
        // a value that falls on this shard.
        let part = (0..).find(|p| crate::slots::shard(*p, o.shards) == sh).unwrap_or(0);
        let mut cols = vec![("part", Arc::new(Int64Array::from(vec![part; v.len()])) as ArrayRef), ("month", Arc::new(v.iter().map(|x| x.0 .0).collect::<Int64Array>()) as ArrayRef), (key, Arc::new(v.iter().map(|x| Some(x.0 .1.as_str())).collect::<StringArray>()) as ArrayRef)];
        cols.extend(month_cols(&v));
        write_parquet(&o.out.join(table).join(format!("s{sh}.parquet")), cols)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn months_start_on_the_first() {
        // 2024-06-18 12:00 UTC
        assert_eq!(month_of(1_718_712_000), 1_717_200_000);
        assert_eq!(month_of(1_717_200_000), 1_717_200_000);
    }
}
