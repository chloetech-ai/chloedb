//! The pages the stories link to, read into `hn_pages` by ChloeDB's own
//! crawler ([`chloedb_crawl`]): its fetcher (robots.txt read before a
//! site's first page and obeyed, redirects checked hop by hop, public
//! addresses only, HTML only, a cap on a page's size), its frontier (one
//! queue a site, at most `per_host` requests at once to it and `delay`
//! between two, slower when the site is slow or asks in its robots.txt,
//! backing off when it pushes back) and its reading of a page (title,
//! description, text, dates, author, image).
//!
//! The links are the stories' (`hn_stories.url`), one a canonical link
//! (`curl`), most points first, those of at least `min_points` points; a
//! link already in `hn_pages` is not asked again, so a crawl that stops
//! goes on where it was when started again. Every link asked makes a row,
//! whatever came of it (`status`), so the table also says which of HN's
//! links still answer. A page whose `<meta name="robots">` says `noindex`
//! keeps no text.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chloedb_crawl::extract;
use chloedb_crawl::fetch::{Fetcher, Miss};
use chloedb_crawl::frontier::{Frontier, Item};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use url::Url;

use crate::strata::{RowExt, Strata};
use crate::text;

/// How long a crawl's robots.txt `Crawl-delay` may make it wait between
/// two requests to a site; a site that asks for more is read that slowly.
const MAX_CRAWL_DELAY: Duration = Duration::from_secs(60);

/// What to crawl and how.
#[derive(Clone, Debug)]
pub struct Opts {
    /// Links of stories with at least this many points.
    pub min_points: i64,
    /// The most links asked in this run (0: all).
    pub limit: usize,
    /// Requests out at once, over all sites.
    pub concurrency: usize,
    /// Requests out at once to one site, and the time between two.
    pub per_host: usize,
    pub delay_ms: u64,
    /// Whom a site's owner reaches about the crawl, in its user agent.
    pub contact: Option<String>,
    /// The most characters of a page's text kept.
    pub max_text: usize,
    /// Rows a write sends.
    pub batch: usize,
    pub timeout_ms: u64,
    pub max_bytes: usize,
}

/// A link to read: the most voted story that links it, how many do, and
/// their points.
#[derive(Clone, Debug)]
struct Seed {
    curl: String,
    url: String,
    story: i64,
    points: i64,
    stories: i64,
}

/// The crawl's counts, for its progress line and `hn_state`.
#[derive(Default)]
struct Counts {
    asked: AtomicU64,
    ok: AtomicU64,
    written: AtomicU64,
    bytes: AtomicU64,
    misses: Mutex<HashMap<String, u64>>,
}

impl Counts {
    fn miss(&self, kind: &str) {
        *self.misses.lock().unwrap_or_else(|e| e.into_inner()).entry(kind.to_string()).or_default() += 1;
    }
    fn view(&self) -> Value {
        let misses = self.misses.lock().unwrap_or_else(|e| e.into_inner()).clone();
        json!({
            "asked": self.asked.load(Ordering::Relaxed), "ok": self.ok.load(Ordering::Relaxed),
            "written": self.written.load(Ordering::Relaxed), "bytes": self.bytes.load(Ordering::Relaxed), "misses": misses,
        })
    }
}

/// The links of the stories, one a canonical link, most points first,
/// without those already in `hn_pages`.
async fn seeds(db: &Strata, min_points: i64, say: &(dyn Fn(String) + Sync)) -> Result<Vec<Seed>, String> {
    let t0 = Instant::now();
    let mut done: HashSet<String> = HashSet::new();
    db.scroll("SELECT id FROM hn_pages", Value::Null, 10_000, |rows| done.extend(rows.iter().map(|r| r.s("id"))))
        .await
        .map_err(|e| format!("hn_pages: {e}"))?;
    let mut by: HashMap<String, Seed> = HashMap::new();
    let n = db
        .scroll("SELECT id, url, curl, points FROM hn_stories WHERE url != '' AND points >= :min", json!({"min": min_points}), 10_000, |rows| {
            for r in rows {
                let curl = r.s("curl");
                if curl.is_empty() || done.contains(&curl) {
                    continue;
                }
                let (id, points) = (r.i("id"), r.i("points"));
                let s = by.entry(curl.clone()).or_insert_with(|| Seed { curl, url: r.s("url"), story: id, points, stories: 0 });
                s.stories += 1;
                if points > s.points {
                    (s.points, s.story, s.url) = (points, id, r.s("url"));
                }
            }
        })
        .await
        .map_err(|e| format!("hn_stories: {e}"))?;
    let mut out: Vec<Seed> = by.into_values().collect();
    out.sort_by(|a, b| b.points.cmp(&a.points).then(a.story.cmp(&b.story)));
    say(format!("{} links of {n} stories with {min_points} points or more, {} read before, in {:.0} s", out.len(), done.len(), t0.elapsed().as_secs_f64()));
    Ok(out)
}

/// A date as a page states it (`2019-03-04T10:00:00+01:00`, `2019-03-04`),
/// as RFC 3339 in UTC; anything else is left out.
fn date(s: &str) -> Option<String> {
    let s = s.trim();
    if let Ok(t) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(t.with_timezone(&chrono::Utc).to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
    }
    let day = s.get(..10)?;
    let d = chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()?;
    (1990..=2100).contains(&chrono::Datelike::year(&d)).then(|| format!("{day}T00:00:00Z"))
}

/// At most `n` characters of `s`.
fn cut(s: &str, n: usize) -> String {
    match s.char_indices().nth(n) {
        Some((i, _)) => s[..i].to_string(),
        None => s.to_string(),
    }
}

/// The row of a link asked, whatever came of it.
fn row(seed: &Seed, status: &str, code: Option<u16>, ms: Option<u64>) -> Value {
    json!({
        "id": seed.curl, "url": seed.url, "site": text::site(&seed.url),
        "story": seed.story, "stories": seed.stories, "points": seed.points,
        "status": status, "code": code, "ms": ms,
        "fetched": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    })
}

/// The row of a page read.
fn page_row(seed: &Seed, final_url: &str, code: u16, ms: u64, bytes: usize, p: &extract::Page, max_text: usize) -> Value {
    let mut r = row(seed, if p.noindex { "noindex" } else { "ok" }, Some(code), Some(ms));
    let o = r.as_object_mut().expect("an object");
    o.insert("final".into(), json!(final_url));
    o.insert("bytes".into(), json!(bytes));
    o.insert("title".into(), json!(p.title));
    o.insert("lang".into(), json!(p.lang));
    o.insert("site_name".into(), json!(p.site_name));
    o.insert("kind".into(), json!(p.kind));
    o.insert("published".into(), json!(p.published.as_deref().and_then(date)));
    if !p.noindex {
        o.insert("description".into(), json!(p.description));
        o.insert("body".into(), json!(cut(&p.body, max_text)));
        o.insert("words".into(), json!(p.words));
        o.insert("author".into(), json!(p.author.as_deref().map(|a| cut(a, 200))));
        o.insert("image".into(), json!(p.image));
    }
    r
}

/// Reads the links into `hn_pages`, `say` told of its progress. Stops
/// when every link was asked, or at `stop`.
pub async fn run(db: &Strata, o: &Opts, say: Arc<dyn Fn(String) + Send + Sync>, stop: Arc<tokio::sync::Notify>) -> Result<Value, String> {
    let mut seeds = seeds(db, o.min_points, &*say).await?;
    if o.limit > 0 {
        seeds.truncate(o.limit);
    }
    let agent = chloedb_crawl::fetch::user_agent(Some("chloedb-hn (reads the pages Hacker News stories link to, once each)"), o.contact.as_deref());
    let fetcher = Arc::new(Fetcher::new(&agent, Duration::from_millis(o.timeout_ms), o.max_bytes, o.per_host, true));
    let frontier = Arc::new(Frontier::new(o.per_host, Duration::from_millis(o.delay_ms), u64::MAX, seeds.len() + 1));
    let counts = Arc::new(Counts::default());
    let (tx, mut rx) = mpsc::channel::<Value>(o.batch * 8);
    // The links not worth a request (a file, not a page) and those that
    // do not parse get their row now; the rest are queued, a site's in
    // order of points.
    let mut by_url: HashMap<String, Seed> = HashMap::with_capacity(seeds.len());
    let mut skipped = Vec::new();
    for s in seeds {
        match Url::parse(&s.url) {
            Ok(u) if matches!(u.scheme(), "http" | "https") && u.host_str().is_some() => {
                if chloedb_crawl::urls::not_a_page(&u) {
                    skipped.push(row(&s, "not_html", None, None));
                } else if frontier.push(Item::new(u.clone(), 0)) {
                    by_url.insert(u.to_string(), s);
                }
            }
            _ => skipped.push(row(&s, "bad_url", None, None)),
        }
    }
    let queued = by_url.len();
    say(format!("{queued} links queued over {} sites, {} not pages", frontier.counts().hosts, skipped.len()));
    let by_url = Arc::new(by_url);

    // The writer: rows in batches, as they come.
    let wdb = db.clone();
    let wcounts = counts.clone();
    let batch = o.batch.max(1);
    let writer = tokio::spawn(async move {
        let mut buf: Vec<Value> = skipped;
        let mut last = Instant::now();
        loop {
            let got = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await;
            let closed = matches!(got, Ok(None));
            if let Ok(Some(r)) = got {
                buf.push(r);
            }
            if buf.len() >= batch || (!buf.is_empty() && (closed || last.elapsed() > Duration::from_secs(5))) {
                let rows = std::mem::take(&mut buf);
                let n = rows.len() as u64;
                match wdb.upsert("hn_pages", rows).await {
                    Ok(_) => {
                        wcounts.written.fetch_add(n, Ordering::Relaxed);
                    }
                    Err(e) => tracing::warn!("hn_pages: a write of {n} rows failed: {e}"),
                }
                last = Instant::now();
            }
            if closed {
                return;
            }
        }
    });

    // Progress, every half minute.
    let t0 = Instant::now();
    let (pf, pc, psay, pdb) = (frontier.clone(), counts.clone(), say.clone(), db.clone());
    let progress = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            let f = pf.counts();
            let c = pc.view();
            let secs = t0.elapsed().as_secs_f64();
            psay(format!(
                "asked {} ({:.1} a second), {} read, {} written; {} queued over {} sites, {} out; misses {}",
                c["asked"], c["asked"].as_f64().unwrap_or(0.0) / secs, c["ok"], c["written"], f.queued, f.hosts, f.in_flight, c["misses"]
            ));
            let state = json!({"counts": c, "queued": f.queued, "in_flight": f.in_flight, "hosts": f.hosts, "secs": secs.round(), "of": queued});
            let _ = pdb.upsert("hn_state", vec![json!({"id": "crawl:progress", "val": state.to_string(), "n": c["asked"], "at": chrono::Utc::now().to_rfc3339()})]).await;
        }
    });

    let stopper = frontier.clone();
    let stop_wait = tokio::spawn(async move {
        stop.notified().await;
        stopper.stop();
    });

    let mut workers = Vec::new();
    for _ in 0..o.concurrency.max(1) {
        let (f, fe, c, tx, by_url) = (frontier.clone(), fetcher.clone(), counts.clone(), tx.clone(), by_url.clone());
        let max_text = o.max_text;
        let delay_ms = o.delay_ms;
        workers.push(tokio::spawn(async move {
            while let Some(item) = f.next().await {
                visit(&f, &fe, &c, &tx, &by_url, &item, max_text, delay_ms).await;
                f.done(&item.url);
            }
        }));
    }
    drop(tx);
    for w in workers {
        let _ = w.await;
    }
    stop_wait.abort();
    progress.abort();
    let _ = writer.await;
    let c = counts.view();
    say(format!("done in {:.0} s: {c}", t0.elapsed().as_secs_f64()));
    Ok(c)
}

#[allow(clippy::too_many_arguments)]
async fn visit(f: &Frontier, fe: &Fetcher, c: &Counts, tx: &mpsc::Sender<Value>, by_url: &HashMap<String, Seed>, item: &Item, max_text: usize, delay_ms: u64) {
    let Some(seed) = by_url.get(item.url.as_str()) else { return };
    let host = item.url.host_str().unwrap_or("").to_string();
    if let Some(d) = fe.robots(&item.url).await.crawl_delay {
        let d = Duration::from_secs_f64(d.clamp(0.0, MAX_CRAWL_DELAY.as_secs_f64()));
        if d > Duration::from_millis(delay_ms) {
            f.slow_down(&host, d);
        }
    }
    c.asked.fetch_add(1, Ordering::Relaxed);
    let got = match fe.page(&item.url).await {
        Ok(g) => g,
        Err(miss) => {
            c.miss(miss.kind());
            if let Some((again, after)) = miss.refusal() {
                let r = f.refused(item, again, after);
                if r.again {
                    // Asked again later: its row then.
                    return;
                }
            } else if miss.fetched() {
                f.answered(&host, None);
            }
            let code = match miss {
                Miss::Status(s) | Miss::Busy(s, _) => Some(s),
                _ => None,
            };
            let _ = tx.send(row(seed, miss.kind(), code, None)).await;
            return;
        }
    };
    f.answered(&host, Some(got.ms));
    c.ok.fetch_add(1, Ordering::Relaxed);
    c.bytes.fetch_add(got.bytes as u64, Ordering::Relaxed);
    let (body, url) = (got.body, got.url.clone());
    let page = tokio::task::spawn_blocking(move || extract::extract(&body, &url, &extract::Options { max_text: 200_000, ..Default::default() })).await;
    let r = match page {
        Ok(p) => page_row(seed, got.url.as_str(), got.status, got.ms, got.bytes, &p, max_text),
        Err(_) => row(seed, "parse", Some(got.status), Some(got.ms)),
    };
    let _ = tx.send(r).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_as_pages_state_them() {
        assert_eq!(date("2019-03-04T10:00:00+01:00").as_deref(), Some("2019-03-04T09:00:00Z"));
        assert_eq!(date("2019-03-04").as_deref(), Some("2019-03-04T00:00:00Z"));
        assert_eq!(date("2019-03-04 10:00").as_deref(), Some("2019-03-04T00:00:00Z"));
        assert_eq!(date("March 4, 2019"), None);
        assert_eq!(date("0001-01-01"), None);
    }

    #[test]
    fn a_page_that_asks_not_to_be_indexed_keeps_no_text() {
        let seed = Seed { curl: "example.com/a".into(), url: "https://example.com/a".into(), story: 1, points: 10, stories: 2 };
        let p = extract::extract(
            r#"<html lang="en"><head><title>A</title><meta name="robots" content="noindex"><meta name="description" content="d"></head><body><p>secret words</p></body></html>"#,
            &Url::parse("https://example.com/a").unwrap(),
            &extract::Options::default(),
        );
        let r = page_row(&seed, "https://example.com/a", 200, 5, 100, &p, 1000);
        assert_eq!(r["status"], "noindex");
        assert_eq!(r["title"], "A");
        assert!(r.get("body").is_none() && r.get("description").is_none());
        let p = extract::extract(
            r#"<html lang="en"><head><title>A</title></head><body><p>some words of the page</p></body></html>"#,
            &Url::parse("https://example.com/a").unwrap(),
            &extract::Options::default(),
        );
        let r = page_row(&seed, "https://example.com/a", 200, 5, 100, &p, 9);
        assert_eq!(r["status"], "ok");
        assert_eq!(r["body"].as_str().unwrap().chars().count(), 9);
        assert_eq!(r["site"], "example.com");
    }
}
