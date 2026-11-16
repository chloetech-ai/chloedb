//! The live feed: new items from the official HN API as they are posted,
//! and the stories whose points and comment counts change, written into the
//! same tables the archive filled. The API (Firebase) states no rate limit;
//! the feed keeps a few requests in flight and pauses when it errs.
//!
//! - `maxitem.json` says the newest id: every id from the feed's place to
//!   it is fetched, in order, and written.
//! - `updates.json` lists the items and people changed in the last minutes:
//!   their stories are fetched again for their points and comments.
//! - A comment's thread is found from its parent: a parent the tables hold
//!   gives its story and depth, else the parent is fetched too.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::stream::{self, StreamExt};
use serde_json::{json, Value};

use crate::prepare::{comment_row, story_row};
use crate::strata::{RowExt, Strata};

pub const API: &str = "https://hacker-news.firebaseio.com/v0";

/// How far behind the newest item the feed starts when the tables are
/// further behind than this (about a day and a half of HN).
pub const HEAD_START: i64 = 20_000;

/// What the feed has done, for the status page.
#[derive(Default)]
pub struct Stats {
    pub cursor: AtomicI64,
    pub max_item: AtomicI64,
    pub fetched: AtomicU64,
    pub written: AtomicU64,
    pub refreshed: AtomicU64,
    pub errors: AtomicU64,
    pub last_ok: AtomicI64,
    pub running: AtomicBool,
    /// Ids left between the archive and the feed's start.
    pub backfill_left: AtomicI64,
    pub backfilled: AtomicU64,
    pub last_error: Mutex<String>,
}

pub struct Feed {
    pub db: Strata,
    http: reqwest::Client,
    pub stats: Stats,
    /// Requests in flight.
    pub concurrency: usize,
    /// The newest comments' threads, so a reply to a reply needs no lookup.
    cache: Mutex<HashMap<u32, (u32, u32, String, String)>>,
}

/// Unix seconds now.
pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

impl Feed {
    pub fn new(db: Strata, concurrency: usize) -> Arc<Feed> {
        let http = reqwest::Client::builder().timeout(Duration::from_secs(20)).user_agent("chloedb-hn (HN search demo; polite feed)").pool_max_idle_per_host(concurrency.max(4)).build().expect("an HTTP client");
        Arc::new(Feed { db, http, stats: Stats::default(), concurrency: concurrency.clamp(1, 64), cache: Mutex::new(HashMap::new()) })
    }

    async fn get(&self, path: &str) -> Result<Value, String> {
        let mut tries = 0;
        loop {
            let r = self.http.get(format!("{API}/{path}")).send().await;
            match r {
                Ok(r) if r.status().is_success() => return r.json::<Value>().await.map_err(|e| e.to_string()),
                Ok(r) if tries < 3 && (r.status().as_u16() == 429 || r.status().is_server_error()) => {}
                Ok(r) => return Err(format!("{path}: HTTP {}", r.status())),
                Err(e) if tries < 3 => {
                    let _ = e;
                }
                Err(e) => return Err(format!("{path}: {e}")),
            }
            tries += 1;
            tokio::time::sleep(Duration::from_millis(500 * (1 << tries))).await;
        }
    }

    pub async fn item(&self, id: u32) -> Result<Value, String> {
        self.stats.fetched.fetch_add(1, Ordering::Relaxed);
        self.get(&format!("item/{id}.json")).await
    }

    fn fail(&self, e: &str) {
        self.stats.errors.fetch_add(1, Ordering::Relaxed);
        *self.stats.last_error.lock().unwrap_or_else(|e| e.into_inner()) = e.chars().take(300).collect();
        tracing::warn!("feed: {e}");
    }

    /// The feed's place: the last id it wrote (kept in hn_state).
    pub async fn cursor(&self) -> i64 {
        let rows = self.db.query("SELECT n FROM hn_state WHERE id = 'feed:cursor'", Value::Null).await.unwrap_or_default();
        if let Some(n) = rows.first().map(|r| r.i("n")) {
            if n > 0 {
                return n;
            }
        }
        // No place yet: after the newest item the tables hold.
        let a = self.db.query("SELECT max(id) AS m FROM hn_stories", Value::Null).await.unwrap_or_default();
        let b = self.db.query("SELECT max(id) AS m FROM hn_comments", Value::Null).await.unwrap_or_default();
        a.first().map(|r| r.i("m")).unwrap_or(0).max(b.first().map(|r| r.i("m")).unwrap_or(0))
    }

    async fn put_state(&self, id: &str, n: i64, val: &str) {
        let _ = self.db.write(vec![json!({"op": "upsert", "table": "hn_state", "doc": {"id": id, "n": n, "val": val, "at": chrono::Utc::now().to_rfc3339()}})]).await;
    }

    /// The ids left between the archive and where the feed started, filled
    /// in the background with fewer requests in flight than the head's,
    /// newest first so recent threads complete soonest. Its place is kept
    /// (`feed:backfill`), so a restart goes on from it.
    pub async fn backfill(self: Arc<Self>) {
        loop {
            let rows = self.db.query("SELECT n, val FROM hn_state WHERE id = 'feed:backfill'", Value::Null).await.unwrap_or_default();
            let Some(r) = rows.first() else {
                tokio::time::sleep(Duration::from_secs(60)).await;
                continue;
            };
            let (lo, hi) = match r.s("val").split_once("..") {
                Some((a, b)) => (a.parse::<i64>().unwrap_or(0), b.parse::<i64>().unwrap_or(0)),
                None => (0, 0),
            };
            if hi < lo || lo <= 0 {
                tokio::time::sleep(Duration::from_secs(300)).await;
                continue;
            }
            // Newest first, 500 at a time: the range shrinks from its top.
            let from = (hi - 499).max(lo);
            self.stats.backfill_left.store(hi - lo + 1, Ordering::Relaxed);
            let fill = Feed { db: self.db.clone(), http: self.http.clone(), stats: Stats::default(), concurrency: (self.concurrency / 2).max(2), cache: Mutex::new(HashMap::new()) };
            let fill = Arc::new(fill);
            match fill.fill(from, hi).await {
                Ok(n) => {
                    self.stats.backfilled.fetch_add(n as u64, Ordering::Relaxed);
                    self.put_state("feed:backfill", lo, &format!("{lo}..{}", from - 1)).await;
                }
                Err(e) => {
                    self.fail(&format!("backfill: {e}"));
                    tokio::time::sleep(Duration::from_secs(30)).await;
                }
            }
        }
    }

    /// Fetches and writes the ids `from..=to`, without moving the cursor.
    async fn fill(self: &Arc<Self>, from: i64, to: i64) -> Result<usize, String> {
        let ids: Vec<u32> = (from..=to).map(|x| x as u32).collect();
        let items: Vec<Result<Value, String>> = stream::iter(ids).map(|id| { let me = self.clone(); async move { me.item(id).await } }).buffered(self.concurrency).collect().await;
        let mut ops = Vec::new();
        for it in items.into_iter().flatten() {
            ops.extend(self.ops_for(&it).await);
        }
        let n = ops.len();
        self.db.write(ops).await.map_err(|e| e.to_string())?;
        Ok(n)
    }

    async fn set_cursor(&self, n: i64) {
        let _ = self.db.write(vec![json!({"op": "upsert", "table": "hn_state", "doc": {"id": "feed:cursor", "n": n, "at": chrono::Utc::now().to_rfc3339()}})]).await;
        self.stats.cursor.store(n, Ordering::Relaxed);
    }

    /// The story and depth above a comment whose parent is `parent`, with
    /// the parent's author and the story's title.
    async fn thread_of(&self, parent: u32) -> Option<(u32, u32, String, String)> {
        if let Some((story, depth, author, title)) = self.cache.lock().unwrap_or_else(|e| e.into_inner()).get(&parent).cloned() {
            return Some((story, depth + 1, author, title));
        }
        // A story the tables hold.
        let s = self.db.post("/v1/kv/get", &json!({"table": "hn_stories", "keys": [parent], "columns": ["id", "author", "title"]})).await.ok()?;
        if let Some(r) = s["rows"].get(0).filter(|r| r.is_object()) {
            return Some((parent, 1, r["author"].as_str().unwrap_or("").to_string(), r["title"].as_str().unwrap_or("").to_string()));
        }
        let c = self.db.post("/v1/kv/get", &json!({"table": "hn_comments", "keys": [parent], "columns": ["id", "author", "story", "story_title", "depth"]})).await.ok()?;
        if let Some(r) = c["rows"].get(0).filter(|r| r.is_object()) {
            return Some((r["story"].as_u64()? as u32, r["depth"].as_u64().unwrap_or(0) as u32 + 1, r["author"].as_str().unwrap_or("").to_string(), r["story_title"].as_str().unwrap_or("").to_string()));
        }
        // Not here: walk up through the API (a reply to an old thread).
        let mut id = parent;
        let mut depth = 1;
        let mut parent_author = None;
        for _ in 0..40 {
            let it = self.item(id).await.ok()?;
            if parent_author.is_none() {
                parent_author = Some(it["by"].as_str().unwrap_or("").to_string());
            }
            match it["type"].as_str() {
                Some("comment") => {
                    id = it["parent"].as_u64()? as u32;
                    depth += 1;
                }
                _ => return Some((id, depth, parent_author.unwrap_or_default(), it["title"].as_str().unwrap_or("").to_string())),
            }
        }
        None
    }

    /// An item of the API as writes: its row, and what it adds to its
    /// author's counts and to the reply graph.
    pub async fn ops_for(&self, it: &Value) -> Vec<Value> {
        let mut ops = Vec::new();
        if it.is_null() || it["deleted"].as_bool() == Some(true) || it["dead"].as_bool() == Some(true) {
            return ops;
        }
        let id = it["id"].as_u64().unwrap_or(0) as u32;
        let by = it["by"].as_str().unwrap_or("");
        let t_us = it["time"].as_i64().unwrap_or(0) * 1_000_000;
        let created = chrono::DateTime::from_timestamp(it["time"].as_i64().unwrap_or(0), 0).map(|t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string()).unwrap_or_default();
        match it["type"].as_str().unwrap_or("") {
            "story" | "job" | "poll" => {
                let title = it["title"].as_str().unwrap_or("");
                if title.is_empty() {
                    return ops;
                }
                let ty = match it["type"].as_str() { Some("job") => 5, Some("poll") => 3, _ => 1 };
                let points = it["score"].as_i64().unwrap_or(0);
                let row = story_row(id, ty, by, t_us, title, it["text"].as_str().unwrap_or(""), it["url"].as_str().unwrap_or(""), points, it["descendants"].as_i64().unwrap_or(0));
                ops.push(json!({"op": "upsert", "table": "hn_stories", "doc": row}));
                if !by.is_empty() {
                    ops.push(json!({"op": "add", "table": "hn_users", "key": by, "field": "stories", "value": 1, "doc": {"id": by, "stories": 0, "comments": 0, "points": 0, "first": created, "last": created}}));
                    ops.push(json!({"op": "set", "table": "hn_users", "key": by, "field": "last", "value": created}));
                }
            }
            "comment" => {
                let parent = it["parent"].as_u64().unwrap_or(0) as u32;
                let html = it["text"].as_str().unwrap_or("");
                let Some((story, depth, parent_author, title)) = self.thread_of(parent).await else { return ops };
                self.cache.lock().unwrap_or_else(|e| e.into_inner()).insert(id, (story, depth, by.to_string(), title.clone()));
                if html.is_empty() {
                    return ops;
                }
                let kids = it["kids"].as_array().map(|k| k.len()).unwrap_or(0);
                let row = comment_row(id, by, t_us, html, parent, &parent_author, story, &title, depth, u16::MAX, kids, kids as u32);
                ops.push(json!({"op": "upsert", "table": "hn_comments", "doc": row}));
                if !by.is_empty() {
                    ops.push(json!({"op": "add", "table": "hn_users", "key": by, "field": "comments", "value": 1, "doc": {"id": by, "stories": 0, "comments": 0, "points": 0, "first": created, "last": created}}));
                    ops.push(json!({"op": "set", "table": "hn_users", "key": by, "field": "last", "value": created}));
                    if depth > 1 && !parent_author.is_empty() && parent_author != by {
                        let eid = format!("{by}>{parent_author}");
                        ops.push(json!({"op": "add", "table": "hn_replies", "key": eid, "field": "w", "value": 1.0, "doc": {"id": eid, "src": by, "dst": parent_author, "w": 0.0}}));
                    }
                }
            }
            _ => {}
        }
        ops
    }

    /// Fetches and writes ids `from..=to`, in order, `concurrency` at once.
    pub async fn catch_up(self: &Arc<Self>, from: i64, to: i64, mut progress: impl FnMut(i64, i64)) -> Result<i64, String> {
        let mut at = from;
        while at <= to {
            let end = (at + 499).min(to);
            let ids: Vec<u32> = (at..=end).map(|x| x as u32).collect();
            let items: Vec<(u32, Result<Value, String>)> = stream::iter(ids).map(|id| { let me = self.clone(); async move { (id, me.item(id).await) } }).buffered(self.concurrency).collect().await;
            let mut ops = Vec::new();
            for (id, r) in items {
                match r {
                    Ok(it) => ops.extend(self.ops_for(&it).await),
                    Err(e) => {
                        self.fail(&format!("item {id}: {e}"));
                    }
                }
            }
            let n = ops.len();
            self.db.write(ops).await.map_err(|e| e.to_string())?;
            self.stats.written.fetch_add(n as u64, Ordering::Relaxed);
            self.set_cursor(end).await;
            self.stats.last_ok.store(now(), Ordering::Relaxed);
            progress(end, to);
            at = end + 1;
            let mut c = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            if c.len() > 200_000 {
                c.clear();
            }
        }
        Ok(to)
    }

    /// Fetches stories again for their points and comment counts.
    pub async fn refresh(self: &Arc<Self>, ids: Vec<u32>) -> usize {
        let items: Vec<Result<Value, String>> = stream::iter(ids).map(|id| { let me = self.clone(); async move { me.item(id).await } }).buffered(self.concurrency).collect().await;
        let mut ops = Vec::new();
        for it in items.into_iter().flatten() {
            if !matches!(it["type"].as_str(), Some("story" | "job" | "poll")) || it["dead"].as_bool() == Some(true) {
                continue;
            }
            let id = it["id"].as_u64().unwrap_or(0);
            if it["deleted"].as_bool() == Some(true) {
                ops.push(json!({"op": "delete", "table": "hn_stories", "key": id}));
                continue;
            }
            let points = it["score"].as_i64().unwrap_or(0).max(0);
            ops.push(json!({"op": "set", "table": "hn_stories", "key": id, "field": "points", "value": points}));
            ops.push(json!({"op": "set", "table": "hn_stories", "key": id, "field": "comments", "value": it["descendants"].as_i64().unwrap_or(0).max(0)}));
            ops.push(json!({"op": "set", "table": "hn_stories", "key": id, "field": "pop", "value": (1.0 + points as f64).ln()}));
        }
        let n = ops.len() / 3;
        if self.db.write(ops).await.is_ok() {
            self.stats.refreshed.fetch_add(n as u64, Ordering::Relaxed);
        }
        n
    }

    /// One round: new ids up to maxitem, then the stories that changed.
    /// Tables far behind (an archive months old) start at the last day
    /// instead, so the front page is live at once, and the ids between
    /// are left to [`Feed::backfill`].
    pub async fn round(self: &Arc<Self>) -> Result<(), String> {
        let max = self.get("maxitem.json").await?.as_i64().unwrap_or(0);
        self.stats.max_item.store(max, Ordering::Relaxed);
        let mut cur = self.cursor().await;
        if cur > 0 && max - cur > HEAD_START {
            let from = max - HEAD_START;
            self.put_state("feed:backfill", cur + 1, &format!("{}..{}", cur + 1, from)).await;
            tracing::info!("feed: {} items behind; following from {from}, and filling {}..{from} beside", max - cur, cur + 1);
            cur = from;
            self.set_cursor(cur).await;
        }
        self.stats.cursor.store(cur, Ordering::Relaxed);
        if cur > 0 && max > cur {
            self.catch_up(cur + 1, max, |at, to| if (to - at) % 5000 < 500 { tracing::info!("feed: at {at} of {to}") }).await?;
        }
        // Changed items: refresh the stories among them, and the stories of the front page.
        let upd = self.get("updates.json").await.unwrap_or(Value::Null);
        let mut ids: Vec<u32> = upd["items"].as_array().into_iter().flatten().filter_map(|v| v.as_u64().map(|x| x as u32)).collect();
        if let Ok(top) = self.get("topstories.json").await {
            ids.extend(top.as_array().into_iter().flatten().take(90).filter_map(|v| v.as_u64().map(|x| x as u32)));
        }
        ids.sort_unstable();
        ids.dedup();
        if !ids.is_empty() {
            // Only those that are stories the tables hold.
            let keys: Vec<Value> = ids.iter().map(|&i| json!(i)).collect();
            let got = self.db.post("/v1/kv/get", &json!({"table": "hn_stories", "keys": keys, "columns": ["id"]})).await.unwrap_or(Value::Null);
            let held: Vec<u32> = got["rows"].as_array().into_iter().flatten().filter_map(|r| r["id"].as_u64().map(|x| x as u32)).collect();
            self.refresh(held).await;
        }
        self.stats.last_ok.store(now(), Ordering::Relaxed);
        Ok(())
    }

    /// Rounds every `every`, for as long as the process runs.
    pub async fn run(self: Arc<Self>, every: Duration) {
        self.stats.running.store(true, Ordering::Relaxed);
        loop {
            if let Err(e) = self.round().await {
                self.fail(&e);
                tokio::time::sleep(Duration::from_secs(30)).await;
            }
            tokio::time::sleep(every).await;
        }
    }

    pub fn status(&self) -> Value {
        json!({
            "running": self.stats.running.load(Ordering::Relaxed),
            "cursor": self.stats.cursor.load(Ordering::Relaxed),
            "max_item": self.stats.max_item.load(Ordering::Relaxed),
            "behind": (self.stats.max_item.load(Ordering::Relaxed) - self.stats.cursor.load(Ordering::Relaxed)).max(0),
            "fetched": self.stats.fetched.load(Ordering::Relaxed),
            "written": self.stats.written.load(Ordering::Relaxed),
            "refreshed": self.stats.refreshed.load(Ordering::Relaxed),
            "errors": self.stats.errors.load(Ordering::Relaxed),
            "backfill_left": self.stats.backfill_left.load(Ordering::Relaxed),
            "backfilled": self.stats.backfilled.load(Ordering::Relaxed),
            "last_ok": self.stats.last_ok.load(Ordering::Relaxed),
            "last_error": self.stats.last_error.lock().unwrap_or_else(|e| e.into_inner()).clone(),
        })
    }
}

/// An item as the page shows it when it is not in the tables (a story
/// older than the archive's months loaded): fetched from the API.
pub fn api_url(id: i64) -> String {
    format!("{API}/item/{id}.json")
}
