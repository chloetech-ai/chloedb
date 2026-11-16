//! `reduce`: one shard's rows from `prepare` into the rows its keyed tables
//! load, one row a key (a bulk load refuses a key given twice):
//! - `repos/sN.parquet`: each repo with at least `--min-stars` stars in the
//!   archive, its latest name and what events last said of it
//!   (description, language, topics, license...), and the stars and forks
//!   the archive counted.
//! - `repo_names/sN.parquet`: every name those repos had, with when it was
//!   first seen, so that an old link finds the repo.
//! - `actors/sN.parquet`: each actor's latest login, with when it was first
//!   and last seen.
//!
//! Inputs are one or more `prepare` folders (several machines' parts of
//! the days); a table's files are read in the order of their days, so the
//! last value read is the newest.

use anyhow::Result;
use arrow_array::cast::AsArray;
use arrow_array::types::Int64Type;
use arrow_array::{Array, ArrayRef, BooleanArray, Int64Array, RecordBatch, StringArray};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use rustc_hash::FxHashMap;
use std::path::PathBuf;
use std::sync::Arc;

pub struct Opts {
    pub inputs: Vec<PathBuf>,
    pub shard: usize,
    pub out: PathBuf,
    pub min_stars: i64,
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

fn each_batch(files: &[PathBuf], mut f: impl FnMut(&RecordBatch)) -> Result<()> {
    for p in files {
        let r = ParquetRecordBatchReaderBuilder::try_new(std::fs::File::open(p)?)?.with_batch_size(65536).build()?;
        for b in r {
            f(&b?);
        }
    }
    Ok(())
}

fn col<'a>(b: &'a RecordBatch, name: &str) -> Option<&'a ArrayRef> {
    b.schema().index_of(name).ok().map(|i| b.column(i))
}
fn int(b: &RecordBatch, name: &str, i: usize) -> Option<i64> {
    let c = col(b, name)?.as_primitive::<Int64Type>();
    c.is_valid(i).then(|| c.value(i))
}
fn text(b: &RecordBatch, name: &str, i: usize) -> Option<String> {
    let c = col(b, name)?.as_string::<i32>();
    c.is_valid(i).then(|| c.value(i).to_string())
}
fn boolean(b: &RecordBatch, name: &str, i: usize) -> Option<bool> {
    let c = col(b, name)?.as_boolean();
    c.is_valid(i).then(|| c.value(i))
}

#[derive(Default, Clone)]
struct Repo {
    name: Option<String>,
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

pub fn run(o: Opts) -> Result<()> {
    let sh = o.shard;
    std::fs::create_dir_all(&o.out)?;
    // Stars and forks first: they decide which repos are kept.
    let mut counts: FxHashMap<i64, (i64, i64)> = FxHashMap::default();
    each_batch(&files(&o.inputs, "stars", sh)?, |b| {
        for i in 0..b.num_rows() {
            let Some(r) = int(b, "repo", i) else { continue };
            let c = counts.entry(r).or_default();
            c.0 += int(b, "stars", i).unwrap_or(0);
            c.1 += int(b, "forks", i).unwrap_or(0);
        }
    })?;
    let mut repos: FxHashMap<i64, Repo> = counts
        .iter()
        .filter(|(_, c)| c.0 >= o.min_stars)
        .map(|(r, c)| (*r, Repo { stars: c.0, forks: c.1, ..Default::default() }))
        .collect();
    eprintln!("reduce s{sh}: {} repos with stars or forks, {} with {}+ stars", counts.len(), repos.len(), o.min_stars);
    drop(counts);
    // Names: the latest, and every one with when it was first seen.
    let mut names: FxHashMap<(i64, String), i64> = FxHashMap::default();
    each_batch(&files(&o.inputs, "names", sh)?, |b| {
        for i in 0..b.num_rows() {
            let (Some(id), Some(t), Some(n)) = (int(b, "repo", i), int(b, "t", i), text(b, "name", i)) else { continue };
            let Some(r) = repos.get_mut(&id) else { continue };
            r.first_seen = Some(r.first_seen.map_or(t, |f| f.min(t)));
            let e = names.entry((id, n.clone())).or_insert(t);
            *e = (*e).min(t);
            r.name = Some(n);
        }
    })?;
    // What events said of each repo, newest last.
    each_batch(&files(&o.inputs, "meta", sh)?, |b| {
        for i in 0..b.num_rows() {
            let Some(id) = int(b, "repo", i) else { continue };
            let Some(r) = repos.get_mut(&id) else { continue };
            let t = int(b, "t", i);
            macro_rules! set {
                ($f:ident, $v:expr) => {
                    if let Some(v) = $v {
                        r.$f = Some(v);
                    }
                };
            }
            set!(description, text(b, "description", i));
            set!(language, text(b, "language", i));
            set!(homepage, text(b, "homepage", i));
            set!(topics, text(b, "topics", i));
            set!(license, text(b, "license", i));
            set!(fork, boolean(b, "fork", i));
            set!(archived, boolean(b, "archived", i));
            set!(created, int(b, "created", i));
            if let Some(s) = int(b, "stars", i) {
                r.stars_api = Some(s);
                r.forks_api = int(b, "forks", i).or(r.forks_api);
                r.api_t = t;
            }
            if r.name.is_none() {
                r.name = text(b, "name", i);
            }
        }
    })?;
    let mut ids: Vec<i64> = repos.keys().copied().filter(|id| repos[id].name.is_some()).collect();
    ids.sort_unstable();
    let rs: Vec<&Repo> = ids.iter().map(|id| &repos[id]).collect();
    let s = |f: &dyn Fn(&Repo) -> Option<String>| -> ArrayRef { Arc::new(rs.iter().map(|r| f(r)).collect::<StringArray>()) };
    let n = |f: &dyn Fn(&Repo) -> Option<i64>| -> ArrayRef { Arc::new(rs.iter().map(|r| f(r)).collect::<Int64Array>()) };
    let bo = |f: &dyn Fn(&Repo) -> Option<bool>| -> ArrayRef { Arc::new(rs.iter().map(|r| f(r)).collect::<BooleanArray>()) };
    crate::prepare::write_parquet(
        &o.out.join("repos").join(format!("s{sh}.parquet")),
        vec![
            ("repo", Arc::new(Int64Array::from(ids.clone())) as ArrayRef),
            ("name", s(&|r| r.name.clone())),
            ("owner", s(&|r| r.name.as_deref().and_then(|n| n.split_once('/')).map(|(o, _)| o.to_string()))),
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
    let mut nm: Vec<((i64, String), i64)> = names.into_iter().collect();
    nm.sort_unstable();
    crate::prepare::write_parquet(
        &o.out.join("repo_names").join(format!("s{sh}.parquet")),
        vec![
            ("repo", Arc::new(nm.iter().map(|x| x.0 .0).collect::<Int64Array>()) as ArrayRef),
            ("name", Arc::new(nm.iter().map(|x| Some(x.0 .1.as_str())).collect::<StringArray>())),
            ("first_seen", Arc::new(nm.iter().map(|x| x.1).collect::<Int64Array>())),
        ],
    )?;
    eprintln!("reduce s{sh}: wrote {} repos and {} names", ids.len(), nm.len());
    drop(repos);
    // Actors: the latest login, first and last seen.
    let mut actors: FxHashMap<i64, (String, i64, i64)> = FxHashMap::default();
    each_batch(&files(&o.inputs, "actors", sh)?, |b| {
        for i in 0..b.num_rows() {
            let (Some(id), Some(t), Some(l)) = (int(b, "actor", i), int(b, "t", i), text(b, "login", i)) else { continue };
            let e = actors.entry(id).or_insert_with(|| (l.clone(), t, t));
            if t >= e.2 {
                e.0 = l;
                e.2 = t;
            }
            e.1 = e.1.min(t);
        }
    })?;
    let mut av: Vec<(i64, (String, i64, i64))> = actors.into_iter().collect();
    av.sort_unstable_by_key(|x| x.0);
    crate::prepare::write_parquet(
        &o.out.join("actors").join(format!("s{sh}.parquet")),
        vec![
            ("actor", Arc::new(av.iter().map(|x| x.0).collect::<Int64Array>()) as ArrayRef),
            ("login", Arc::new(av.iter().map(|x| Some(x.1 .0.as_str())).collect::<StringArray>())),
            ("first_seen", Arc::new(av.iter().map(|x| x.1 .1).collect::<Int64Array>())),
            ("last_seen", Arc::new(av.iter().map(|x| x.1 .2).collect::<Int64Array>())),
        ],
    )?;
    eprintln!("reduce s{sh}: wrote {} actors", av.len());
    Ok(())
}
