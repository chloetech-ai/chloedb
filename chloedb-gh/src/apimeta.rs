//! What GitHub says of each repo today, from its public search API, for the
//! repos with `min_stars` or more: stars, forks, description, language,
//! topics, license, homepage, archived and the current name. GH Archive's
//! payloads stopped carrying a repo's description, language and topics in
//! October 2025, so repos made since have none without this; and the stars
//! in events miss those given before 2012 or taken back since.
//!
//! The search API answers at most 1,000 repos a query, so the repos are
//! walked in bands of stars, a band split into halves (of stars, then of
//! the days they were created) until each part has 1,000 or fewer. It is
//! read 100 repos a call and at most 10 calls a minute without a token
//! (30 with one, GITHUB_TOKEN): `gap` is the pause between calls, and a
//! machine works on every n-th band (`part`), so that machines with their
//! own addresses share the walk. A band done is written down in
//! OUT/api-done-I.txt, so a run started again goes on.
//!
//! The rows go out as `prepare`'s `meta` and `names` rows, in files named
//! api-*.parquet that `reduce` reads after the days': OUT/meta/sN/ and
//! OUT/names/sN/. The repos made since `since` come first.

use crate::prepare::{ints, opt_bools, opt_ints, opt_strs, strs, write_parquet};
use crate::slots;
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Days, NaiveDate, Utc};
use serde::Deserialize;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub struct Opts {
    pub out: PathBuf,
    pub part: (usize, usize),
    pub shards: usize,
    pub min_stars: i64,
    pub since: NaiveDate,
    pub gap: Duration,
    pub token: Option<String>,
    pub base_url: String,
}

#[derive(Deserialize)]
struct Page {
    total_count: i64,
    #[serde(default)]
    incomplete_results: bool,
    #[serde(default)]
    items: Vec<Item>,
}

#[derive(Deserialize)]
struct Item {
    id: i64,
    full_name: String,
    description: Option<String>,
    language: Option<String>,
    homepage: Option<String>,
    stargazers_count: Option<i64>,
    forks_count: Option<i64>,
    created_at: Option<String>,
    #[serde(default)]
    topics: Vec<String>,
    license: Option<License>,
    fork: Option<bool>,
    archived: Option<bool>,
}

#[derive(Deserialize)]
struct License {
    spdx_id: Option<String>,
}

/// A part of the walk: stars from `lo` to `hi` (None: no top), made from
/// `from` to `to`.
#[derive(Clone, Debug)]
struct Range {
    lo: i64,
    hi: Option<i64>,
    from: NaiveDate,
    to: NaiveDate,
}

impl Range {
    fn query(&self) -> String {
        let stars = match self.hi {
            Some(h) if h == self.lo => format!("stars:{}", self.lo),
            Some(h) => format!("stars:{}..{}", self.lo, h),
            None => format!("stars:>={}", self.lo),
        };
        // fork:true takes forks as well as the rest (search leaves them out otherwise).
        format!("{stars} created:{}..{} fork:true", self.from, self.to)
    }

    /// Two halves: of stars while there is more than one count, then of days.
    fn split(&self) -> Option<(Range, Range)> {
        match self.hi {
            None => {
                let mid = self.lo * 2;
                Some((
                    Range {
                        hi: Some(mid - 1),
                        ..self.clone()
                    },
                    Range {
                        lo: mid,
                        ..self.clone()
                    },
                ))
            }
            Some(h) if h > self.lo => {
                let mid = self.lo + (h - self.lo) / 2;
                Some((
                    Range {
                        hi: Some(mid),
                        ..self.clone()
                    },
                    Range {
                        lo: mid + 1,
                        ..self.clone()
                    },
                ))
            }
            _ => {
                let days = (self.to - self.from).num_days();
                if days < 1 {
                    return None;
                }
                let mid = self
                    .from
                    .checked_add_days(Days::new((days / 2) as u64))
                    .unwrap();
                Some((
                    Range {
                        to: mid,
                        ..self.clone()
                    },
                    Range {
                        from: mid.succ_opt().unwrap(),
                        ..self.clone()
                    },
                ))
            }
        }
    }
}

/// The bands of stars a walk starts from: narrow where repos are many.
fn bands(min: i64) -> Vec<(i64, Option<i64>)> {
    let mut v = Vec::new();
    let mut s = min;
    let step = |s: i64| match s {
        ..=199 => 1,
        ..=499 => 5,
        ..=1999 => 25,
        ..=9999 => 250,
        _ => 5000,
    };
    while s < 50_000 {
        let st = step(s);
        v.push((s, Some(s + st - 1)));
        s += st;
    }
    v.push((s, None));
    v
}

struct Client {
    http: reqwest::blocking::Client,
    o: Opts,
    last: Option<Instant>,
    calls: u64,
}

impl Client {
    fn page(&mut self, q: &str, page: u32) -> Result<Option<Page>> {
        let mut wait = Duration::from_secs(30);
        for attempt in 0..10 {
            if let Some(l) = self.last {
                let next = l + self.o.gap;
                let now = Instant::now();
                if next > now {
                    std::thread::sleep(next - now);
                }
            }
            self.last = Some(Instant::now());
            self.calls += 1;
            let mut req = self
                .http
                .get(format!("{}/search/repositories", self.o.base_url))
                .query(&[
                    ("q", q),
                    ("per_page", "100"),
                    ("page", &page.to_string()),
                    ("sort", "stars"),
                    ("order", "desc"),
                ])
                .header("Accept", "application/vnd.github+json");
            if let Some(t) = &self.o.token {
                req = req.bearer_auth(t);
            }
            let r = match req.send() {
                Ok(r) => r,
                Err(e) => {
                    eprintln!(
                        "api-meta: {q} page {page}: {e}; again in {} s",
                        wait.as_secs()
                    );
                    std::thread::sleep(wait);
                    wait = (wait * 2).min(Duration::from_secs(600));
                    continue;
                }
            };
            let status = r.status();
            if status.is_success() {
                return Ok(Some(
                    r.json::<Page>()
                        .with_context(|| format!("{q} page {page}"))?,
                ));
            }
            if status.as_u16() == 422 {
                eprintln!("api-meta: {q} page {page}: refused ({status}); skipped");
                return Ok(None);
            }
            // A limit: wait for as long as GitHub says, or for the window to reset.
            let h = |n: &str| {
                r.headers()
                    .get(n)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<i64>().ok())
            };
            let pause = if let Some(s) = h("retry-after") {
                Duration::from_secs(s.max(1) as u64)
            } else if h("x-ratelimit-remaining") == Some(0) {
                let reset = h("x-ratelimit-reset").unwrap_or(0) - Utc::now().timestamp();
                Duration::from_secs(reset.clamp(1, 3600) as u64 + 2)
            } else {
                wait
            };
            eprintln!(
                "api-meta: {q} page {page}: {status} (attempt {}); waiting {} s",
                attempt + 1,
                pause.as_secs()
            );
            std::thread::sleep(pause);
            wait = (wait * 2).min(Duration::from_secs(600));
        }
        bail!("GitHub did not answer {q} page {page}")
    }
}

#[derive(Default)]
struct Rows {
    items: Vec<(i64, Item)>,
}

impl Rows {
    fn write(&mut self, o: &Opts, tag: &str) -> Result<()> {
        if self.items.is_empty() {
            return Ok(());
        }
        let mut items = std::mem::take(&mut self.items);
        items.sort_unstable_by_key(|x| x.1.id);
        items.dedup_by_key(|x| x.1.id);
        for sh in 0..o.shards {
            let rows: Vec<&(i64, Item)> = items
                .iter()
                .filter(|x| slots::shard(x.1.id, o.shards) == sh)
                .collect();
            if rows.is_empty() {
                continue;
            }
            let name = format!("api-{tag}.parquet");
            let id: Vec<i64> = rows.iter().map(|x| x.1.id).collect();
            let t: Vec<i64> = rows.iter().map(|x| x.0).collect();
            let s = |f: &dyn Fn(&Item) -> Option<String>| {
                opt_strs(&rows.iter().map(|x| f(&x.1)).collect::<Vec<_>>())
            };
            let n = |f: &dyn Fn(&Item) -> Option<i64>| {
                opt_ints(&rows.iter().map(|x| f(&x.1)).collect::<Vec<_>>())
            };
            let b = |f: &dyn Fn(&Item) -> Option<bool>| {
                opt_bools(&rows.iter().map(|x| f(&x.1)).collect::<Vec<_>>())
            };
            let nonempty =
                |v: &Option<String>| v.as_ref().filter(|s| !s.trim().is_empty()).cloned();
            write_parquet(
                &o.out.join("meta").join(format!("s{sh}")).join(&name),
                vec![
                    ("repo", ints(&id)),
                    ("t", ints(&t)),
                    ("name", s(&|x| Some(x.full_name.clone()))),
                    ("description", s(&|x| nonempty(&x.description))),
                    ("language", s(&|x| x.language.clone())),
                    ("homepage", s(&|x| nonempty(&x.homepage))),
                    ("stars", n(&|x| x.stargazers_count)),
                    ("forks", n(&|x| x.forks_count)),
                    (
                        "created",
                        n(&|x| {
                            x.created_at
                                .as_deref()
                                .and_then(|c| DateTime::parse_from_rfc3339(c).ok())
                                .map(|d| d.timestamp())
                        }),
                    ),
                    (
                        "topics",
                        s(&|x| (!x.topics.is_empty()).then(|| x.topics.join(","))),
                    ),
                    (
                        "license",
                        s(&|x| {
                            x.license
                                .as_ref()
                                .and_then(|l| l.spdx_id.clone())
                                .filter(|l| l != "NOASSERTION")
                        }),
                    ),
                    ("fork", b(&|x| x.fork)),
                    ("archived", b(&|x| x.archived)),
                ],
            )?;
            let names: Vec<&str> = rows.iter().map(|x| x.1.full_name.as_str()).collect();
            write_parquet(
                &o.out.join("names").join(format!("s{sh}")).join(&name),
                vec![("repo", ints(&id)), ("t", ints(&t)), ("name", strs(&names))],
            )?;
        }
        Ok(())
    }
}

pub fn run(o: Opts) -> Result<()> {
    let (part, parts) = o.part;
    if parts == 0 || part >= parts || o.shards == 0 {
        bail!("--part i/n needs i < n, and --shards 1 or more");
    }
    std::fs::create_dir_all(&o.out)?;
    let done_path = o.out.join(format!("api-done-{part}.txt"));
    let done: std::collections::HashSet<String> = std::fs::read_to_string(&done_path)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect();
    let today = Utc::now().date_naive();
    let first = NaiveDate::from_ymd_opt(2007, 10, 1).unwrap();
    // The repos made since `since` first, then the rest, most stars first.
    let mut work: Vec<Range> = Vec::new();
    for (lo, hi) in bands(o.min_stars) {
        work.push(Range {
            lo,
            hi,
            from: o.since,
            to: today,
        });
    }
    let mut older: Vec<Range> = bands(o.min_stars)
        .into_iter()
        .map(|(lo, hi)| Range {
            lo,
            hi,
            from: first,
            to: o.since.pred_opt().unwrap(),
        })
        .collect();
    older.reverse();
    work.extend(older);
    let mine: Vec<(usize, Range)> = work
        .into_iter()
        .enumerate()
        .filter(|(i, _)| i % parts == part)
        .filter(|(_, r)| !done.contains(&r.query()))
        .collect();
    let http = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent("chloedb-gh-meta")
        .build()?;
    let gap = o.gap;
    let mut c = Client {
        http,
        o,
        last: None,
        calls: 0,
    };
    eprintln!(
        "api-meta: part {part}/{parts}: {} bands to walk, a call every {:.1} s",
        mine.len(),
        gap.as_secs_f64()
    );
    let mut done_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&done_path)?;
    let t0 = Instant::now();
    let mut repos = 0u64;
    for (k, (idx, band)) in mine.iter().enumerate() {
        let mut rows = Rows::default();
        let mut stack = vec![band.clone()];
        while let Some(r) = stack.pop() {
            let q = r.query();
            let Some(first_page) = c.page(&q, 1)? else {
                continue;
            };
            if first_page.total_count > 1000 || first_page.incomplete_results {
                if let Some((a, b)) = r.split() {
                    stack.push(b);
                    stack.push(a);
                    continue;
                }
                eprintln!(
                    "api-meta: {q}: {} repos and no way to split it; taking the first 1,000",
                    first_page.total_count
                );
            }
            let now = Utc::now().timestamp();
            let pages = ((first_page.total_count.min(1000) + 99) / 100) as u32;
            rows.items
                .extend(first_page.items.into_iter().map(|i| (now, i)));
            for p in 2..=pages {
                let Some(pg) = c.page(&q, p)? else { break };
                let empty = pg.items.is_empty();
                rows.items.extend(pg.items.into_iter().map(|i| (now, i)));
                if empty {
                    break;
                }
            }
        }
        repos += rows.items.len() as u64;
        rows.write(&c.o, &format!("{part}-{idx:05}"))?;
        writeln!(done_file, "{}", band.query())?;
        done_file.flush()?;
        eprintln!(
            "api-meta: band {} done ({}/{}): {} repos so far, {} calls, {:.0} min",
            band.query(),
            k + 1,
            mine.len(),
            repos,
            c.calls,
            t0.elapsed().as_secs_f64() / 60.0
        );
    }
    eprintln!(
        "api-meta: part {part}/{parts} done: {repos} repos in {} calls, {:.0} min",
        c.calls,
        t0.elapsed().as_secs_f64() / 60.0
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_split_stars_then_days() {
        let d = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        let r = Range {
            lo: 50,
            hi: Some(59),
            from: d("2020-01-01"),
            to: d("2020-12-31"),
        };
        assert_eq!(
            r.query(),
            "stars:50..59 created:2020-01-01..2020-12-31 fork:true"
        );
        let (a, b) = r.split().unwrap();
        assert_eq!((a.lo, a.hi, b.lo, b.hi), (50, Some(54), 55, Some(59)));
        let one = Range {
            lo: 50,
            hi: Some(50),
            ..r.clone()
        };
        assert_eq!(
            one.query(),
            "stars:50 created:2020-01-01..2020-12-31 fork:true"
        );
        let (a, b) = one.split().unwrap();
        assert_eq!(
            (a.from, a.to, b.from, b.to),
            (
                d("2020-01-01"),
                d("2020-07-01"),
                d("2020-07-02"),
                d("2020-12-31")
            )
        );
        let day = Range {
            from: d("2020-01-01"),
            to: d("2020-01-01"),
            ..one
        };
        assert!(day.split().is_none());
        let top = Range {
            lo: 50_000,
            hi: None,
            ..r
        };
        let (a, b) = top.split().unwrap();
        assert_eq!((a.hi, b.lo, b.hi), (Some(99_999), 100_000, None));
        let bs = bands(50);
        assert_eq!(bs[0], (50, Some(50)));
        assert_eq!(bs.last().unwrap().1, None);
        // Every count from 50 up is in exactly one band.
        for w in bs.windows(2) {
            assert_eq!(w[0].1.unwrap() + 1, w[1].0);
        }
    }
}
