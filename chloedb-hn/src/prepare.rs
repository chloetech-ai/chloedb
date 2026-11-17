//! `chloedb-hn prepare`: the Hacker News archive (monthly Parquet files of
//! every item, as the HN API gives them) turned into the rows of the app's
//! tables, as JSON lines for `chloedb-server import` or `/v1/import`.
//!
//! Two passes over the files. The first reads the few columns that place an
//! item in its thread (id, type, author, parent, kids, title) for every item
//! there is, so that each comment gets its story, its depth, its place among
//! its siblings and the size of the discussion under it, and each author
//! the people they replied to. The second writes the rows.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow_array::{Array, ArrayRef, Int32Array, Int8Array, ListArray, RecordBatch, StringArray, LargeStringArray, StringViewArray, TimestampMicrosecondArray, TimestampMillisecondArray, TimestampNanosecondArray, TimestampSecondArray, UInt32Array, UInt8Array};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ProjectionMask;
use serde_json::{json, Value};

use crate::text;

/// Where the archive lives on Hugging Face: `data/YYYY/YYYY-MM.parquet`.
pub const ARCHIVE: &str = "https://huggingface.co/datasets/open-index/hacker-news/resolve/main";

pub struct Opts {
    pub parquet: PathBuf,
    pub out: PathBuf,
    /// Comments from this time on (RFC 3339 or `YYYY-MM-DD`); every story
    /// is written whatever its time.
    pub comments_since: Option<String>,
    /// Comments before this time are not written either.
    pub comments_until: Option<String>,
    /// Pairs of people who replied to each other fewer times than this are
    /// left out of the reply graph.
    pub min_replies: u32,
}

/// The months of the archive from `from` to `to`, as `YYYY-MM`.
pub fn months(from: &str, to: &str) -> Vec<String> {
    let parse = |s: &str| -> (i32, u32) {
        let mut p = s.split('-');
        (p.next().and_then(|y| y.parse().ok()).unwrap_or(2006), p.next().and_then(|m| m.parse().ok()).unwrap_or(1))
    };
    let (mut y, mut m) = parse(from);
    let (ty, tm) = parse(to);
    let mut out = Vec::new();
    while (y, m) <= (ty, tm) {
        out.push(format!("{y:04}-{m:02}"));
        m += 1;
        if m > 12 {
            m = 1;
            y += 1;
        }
    }
    out
}

/// Downloads the months not yet in `dir` (a part file renamed when whole).
pub async fn download(dir: &Path, months: &[String]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let http = reqwest::Client::builder().timeout(std::time::Duration::from_secs(1800)).build().map_err(|e| e.to_string())?;
    for m in months {
        let path = dir.join(format!("{m}.parquet"));
        if path.exists() && std::fs::metadata(&path).map(|x| x.len() > 0).unwrap_or(false) {
            continue;
        }
        let url = format!("{ARCHIVE}/data/{}/{m}.parquet", &m[..4]);
        let mut tries = 0;
        loop {
            match fetch(&http, &url, &path).await {
                Ok(n) => {
                    eprintln!("chloedb-hn: {m}: {:.1} MB", n as f64 / 1e6);
                    break;
                }
                Err(e) if tries < 5 => {
                    tries += 1;
                    eprintln!("chloedb-hn: {m}: {e}; again");
                    tokio::time::sleep(std::time::Duration::from_secs(2 * tries)).await;
                }
                Err(e) => return Err(format!("{m}: {e}")),
            }
        }
    }
    Ok(())
}

async fn fetch(http: &reqwest::Client, url: &str, path: &Path) -> Result<u64, String> {
    use futures_util::StreamExt;
    let r = http.get(url).send().await.map_err(|e| e.to_string())?;
    if r.status().as_u16() == 404 {
        // A month the archive has no file for (2006-11): an empty one.
        std::fs::write(path, b"").map_err(|e| e.to_string())?;
        return Ok(0);
    }
    if !r.status().is_success() {
        return Err(format!("HTTP {}", r.status()));
    }
    let part = path.with_extension("parquet.part");
    let mut f = tokio::fs::File::create(&part).await.map_err(|e| e.to_string())?;
    let mut n = 0u64;
    let mut s = r.bytes_stream();
    while let Some(chunk) = s.next().await {
        let c = chunk.map_err(|e| e.to_string())?;
        n += c.len() as u64;
        tokio::io::AsyncWriteExt::write_all(&mut f, &c).await.map_err(|e| e.to_string())?;
    }
    tokio::io::AsyncWriteExt::flush(&mut f).await.map_err(|e| e.to_string())?;
    drop(f);
    std::fs::rename(&part, path).map_err(|e| e.to_string())?;
    Ok(n)
}

/// A string column of any of Arrow's three layouts.
enum Strs<'a> {
    S(&'a StringArray),
    L(&'a LargeStringArray),
    V(&'a StringViewArray),
    None,
}

impl<'a> Strs<'a> {
    fn of(a: Option<&'a ArrayRef>) -> Strs<'a> {
        let Some(a) = a else { return Strs::None };
        if let Some(s) = a.as_any().downcast_ref::<StringArray>() {
            Strs::S(s)
        } else if let Some(s) = a.as_any().downcast_ref::<LargeStringArray>() {
            Strs::L(s)
        } else if let Some(s) = a.as_any().downcast_ref::<StringViewArray>() {
            Strs::V(s)
        } else {
            Strs::None
        }
    }
    fn get(&self, i: usize) -> &'a str {
        match self {
            Strs::S(s) if s.is_valid(i) => s.value(i),
            Strs::L(s) if s.is_valid(i) => s.value(i),
            Strs::V(s) if s.is_valid(i) => s.value(i),
            _ => "",
        }
    }
}

fn col<'a>(b: &'a RecordBatch, name: &str) -> Option<&'a ArrayRef> {
    b.schema().index_of(name).ok().map(|i| b.column(i))
}

fn u32s<'a>(b: &'a RecordBatch, name: &str) -> Option<&'a UInt32Array> {
    col(b, name).and_then(|a| a.as_any().downcast_ref::<UInt32Array>())
}

fn u8s<'a>(b: &'a RecordBatch, name: &str) -> Option<&'a UInt8Array> {
    col(b, name).and_then(|a| a.as_any().downcast_ref::<UInt8Array>())
}

fn read(path: &Path, cols: &[&str], mut each: impl FnMut(&RecordBatch)) -> Result<(), String> {
    let f = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if f.metadata().map(|m| m.len()).unwrap_or(0) == 0 {
        return Ok(());
    }
    let b = ParquetRecordBatchReaderBuilder::try_new(f).map_err(|e| format!("{}: {e}", path.display()))?;
    let schema = b.parquet_schema();
    let idx: Vec<usize> = (0..schema.root_schema().get_fields().len()).filter(|&i| cols.contains(&schema.root_schema().get_fields()[i].name())).collect();
    let mask = ProjectionMask::roots(schema, idx);
    let r = b.with_projection(mask).with_batch_size(32_768).build().map_err(|e| format!("{}: {e}", path.display()))?;
    for batch in r {
        let batch = batch.map_err(|e| format!("{}: {e}", path.display()))?;
        each(&batch);
    }
    Ok(())
}

/// What the first pass keeps of every item, by id.
struct Threads {
    /// Item type (1 story, 2 comment, 3 poll, 4 pollopt, 5 job), and 8 when dead or deleted.
    typ: Vec<u8>,
    parent: Vec<u32>,
    /// The author, as an index into `users`.
    by: Vec<u32>,
    /// The place among its parent's kids (HN's order: 0 is shown first).
    pos: Vec<u16>,
    users: Vec<String>,
    user_ix: HashMap<String, u32>,
    titles: HashMap<u32, String>,
}

const DEAD: u8 = 8;

impl Threads {
    fn new() -> Threads {
        Threads { typ: Vec::new(), parent: Vec::new(), by: Vec::new(), pos: Vec::new(), users: vec![String::new()], user_ix: HashMap::new(), titles: HashMap::new() }
    }
    fn grow(&mut self, id: usize) {
        if id >= self.typ.len() {
            let n = (id + 1).max(self.typ.len() * 5 / 4);
            self.typ.resize(n, 0);
            self.parent.resize(n, 0);
            self.by.resize(n, 0);
            self.pos.resize(n, u16::MAX);
        }
    }
    fn user(&mut self, name: &str) -> u32 {
        if name.is_empty() {
            return 0;
        }
        if let Some(&i) = self.user_ix.get(name) {
            return i;
        }
        let i = self.users.len() as u32;
        self.users.push(name.to_string());
        self.user_ix.insert(name.to_string(), i);
        i
    }
    fn live(&self, id: u32) -> bool {
        let t = self.typ.get(id as usize).copied().unwrap_or(0);
        t != 0 && t & DEAD == 0
    }
    /// The story at the top of an item's thread, and how deep the item is.
    fn root(&self, mut id: u32) -> (u32, u32) {
        let mut depth = 0;
        for _ in 0..10_000 {
            let t = self.typ.get(id as usize).copied().unwrap_or(0) & !DEAD;
            if t != 2 && t != 4 {
                return (id, depth);
            }
            let p = self.parent[id as usize];
            if p == 0 || p == id {
                return (id, depth);
            }
            id = p;
            depth += 1;
        }
        (id, depth)
    }
}

fn iso(us: i64) -> String {
    chrono::DateTime::from_timestamp_micros(us).map(|t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string()).unwrap_or_default()
}

fn since_us(s: &Option<String>) -> Option<i64> {
    let s = s.as_ref()?;
    let t = if s.len() == 10 { format!("{s}T00:00:00Z") } else { s.clone() };
    chrono::DateTime::parse_from_rfc3339(&t).ok().map(|d| d.timestamp_micros())
}

struct Out {
    w: BufWriter<File>,
    n: u64,
}

impl Out {
    fn new(dir: &Path, name: &str) -> Result<Out, String> {
        let f = File::create(dir.join(name)).map_err(|e| format!("{name}: {e}"))?;
        Ok(Out { w: BufWriter::with_capacity(1 << 20, f), n: 0 })
    }
    fn row(&mut self, v: &Value) -> Result<(), String> {
        serde_json::to_writer(&mut self.w, v).map_err(|e| e.to_string())?;
        self.w.write_all(b"\n").map_err(|e| e.to_string())?;
        self.n += 1;
        Ok(())
    }
    fn done(mut self) -> Result<u64, String> {
        self.w.flush().map_err(|e| e.to_string())?;
        Ok(self.n)
    }
}

/// A story's row (stories, Ask/Show/Launch/Tell HN, jobs and polls).
#[allow(clippy::too_many_arguments)]
/// A column of times in microseconds, whatever unit the file keeps them in
/// (the archive's months keep milliseconds).
pub fn micros(b: &RecordBatch, name: &str) -> Vec<i64> {
    let Some(a) = col(b, name) else { return vec![0; b.num_rows()] };
    let any = a.as_any();
    if let Some(t) = any.downcast_ref::<TimestampMicrosecondArray>() {
        return t.values().to_vec();
    }
    if let Some(t) = any.downcast_ref::<TimestampMillisecondArray>() {
        return t.values().iter().map(|v| v * 1_000).collect();
    }
    if let Some(t) = any.downcast_ref::<TimestampSecondArray>() {
        return t.values().iter().map(|v| v * 1_000_000).collect();
    }
    if let Some(t) = any.downcast_ref::<TimestampNanosecondArray>() {
        return t.values().iter().map(|v| v / 1_000).collect();
    }
    if let Some(t) = any.downcast_ref::<arrow_array::Int64Array>() {
        // Plain numbers: seconds, as the HN API gives them.
        return t.values().iter().map(|v| v * 1_000_000).collect();
    }
    vec![0; b.num_rows()]
}

pub fn story_row(id: u32, item_type: i8, by: &str, time_us: i64, title: &str, html: &str, url: &str, score: i64, descendants: i64) -> Value {
    let body = text::html_to_text(html);
    let kind = text::kind(item_type, title);
    let mut about = title.to_string();
    if !body.is_empty() {
        about.push(' ');
        about.push_str(&text::clip(&body, 600));
    }
    let topics = text::topics(&about);
    let year = chrono::DateTime::from_timestamp_micros(time_us).map(|t| chrono::Datelike::year(&t)).unwrap_or(0);
    json!({
        "id": id, "title": title, "body": body, "url": url, "curl": text::canonical_url(url),
        "domain": text::domain(url), "site": text::site(url), "urlwords": text::url_words(url),
        "author": by, "created": iso(time_us), "year": year, "points": score.max(0), "comments": descendants.max(0),
        "kind": kind, "topics": topics, "pop": (1.0 + score.max(0) as f64).ln(),
    })
}

/// A comment's row. `thread` is how many comments are under it, `pos` its
/// place among its siblings (0 is HN's first), `story_title` the title at
/// the top of its thread, so a search finds a comment by its story's words.
#[allow(clippy::too_many_arguments)]
pub fn comment_row(id: u32, by: &str, time_us: i64, html: &str, parent: u32, parent_by: &str, story: u32, story_title: &str, depth: u32, pos: u16, replies: usize, thread: u32) -> Value {
    let body = text::html_to_text(html);
    let year = chrono::DateTime::from_timestamp_micros(time_us).map(|t| chrono::Datelike::year(&t)).unwrap_or(0);
    // A quality prior: HN's own order among siblings (its ranking uses votes,
    // which the API does not give) and the discussion a comment started.
    let pos = if pos == u16::MAX { 50 } else { pos };
    let pop = (1.0 + thread as f64).ln() + 1.5 / (1.0 + pos as f64);
    json!({
        "id": id, "body": body, "author": by, "created": iso(time_us), "year": year,
        "parent": parent, "parent_author": parent_by, "story": story, "story_title": story_title,
        "depth": depth, "pos": pos, "replies": replies, "thread": thread, "pop": (pop * 1000.0).round() / 1000.0,
        "chars": body.chars().count(),
    })
}

pub fn run(o: &Opts) -> Result<(), String> {
    std::fs::create_dir_all(&o.out).map_err(|e| format!("{}: {e}", o.out.display()))?;
    let mut files: Vec<PathBuf> = std::fs::read_dir(&o.parquet).map_err(|e| format!("{}: {e}", o.parquet.display()))?
        .filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "parquet")).collect();
    files.sort();
    if files.is_empty() {
        return Err(format!("no .parquet files in {}", o.parquet.display()));
    }
    let since = since_us(&o.comments_since).unwrap_or(i64::MIN);
    let until = since_us(&o.comments_until).unwrap_or(i64::MAX);
    let t0 = std::time::Instant::now();

    // Pass 1: every item's place.
    let mut th = Threads::new();
    let mut kids_of: Vec<(u32, u32, u16)> = Vec::new();
    for f in &files {
        read(f, &["id", "type", "by", "parent", "kids", "title", "deleted", "dead"], |b| {
            let (Some(id), Some(parent)) = (u32s(b, "id"), u32s(b, "parent")) else { return };
            let typ = col(b, "type").and_then(|a| a.as_any().downcast_ref::<Int8Array>());
            let by = Strs::of(col(b, "by"));
            let title = Strs::of(col(b, "title"));
            let dead = u8s(b, "dead");
            let deleted = u8s(b, "deleted");
            let kids = col(b, "kids").and_then(|a| a.as_any().downcast_ref::<ListArray>());
            for i in 0..b.num_rows() {
                let x = id.value(i);
                th.grow(x as usize);
                let t = typ.map(|t| t.value(i) as u8).unwrap_or(0);
                let gone = dead.is_some_and(|d| d.value(i) != 0) || deleted.is_some_and(|d| d.value(i) != 0);
                th.typ[x as usize] = t | if gone { DEAD } else { 0 };
                th.parent[x as usize] = parent.value(i);
                let u = th.user(by.get(i));
                th.by[x as usize] = u;
                if matches!(t, 1 | 3 | 5) && !gone {
                    let tt = title.get(i);
                    if !tt.is_empty() {
                        th.titles.insert(x, tt.to_string());
                    }
                }
                if let Some(k) = kids {
                    if k.is_valid(i) {
                        let v = k.value(i);
                        if let Some(v) = v.as_any().downcast_ref::<UInt32Array>() {
                            for (j, kid) in v.values().iter().enumerate() {
                                kids_of.push((*kid, x, j.min(u16::MAX as usize - 1) as u16));
                            }
                        }
                    }
                }
            }
        })?;
        eprintln!("chloedb-hn: pass 1 {} ({} items so far, {:.0} s)", f.file_name().unwrap_or_default().to_string_lossy(), th.typ.iter().filter(|&&t| t != 0).count(), t0.elapsed().as_secs_f64());
    }
    // Each kid's place among its parent's live kids, as HN orders them.
    let mut live_kids: HashMap<u32, u16> = HashMap::new();
    kids_of.sort_unstable_by_key(|&(_, p, j)| (p, j));
    let mut replies: Vec<u16> = vec![0; th.typ.len()];
    for &(kid, p, _) in &kids_of {
        if (kid as usize) < th.typ.len() && th.live(kid) && th.parent[kid as usize] == p {
            let n = live_kids.entry(p).or_insert(0);
            th.pos[kid as usize] = *n;
            *n = n.saturating_add(1);
            replies[p as usize] = replies[p as usize].saturating_add(1);
        }
    }
    drop(kids_of);
    drop(live_kids);
    // The comments under each item, from the last id down (a reply comes after what it answers).
    let mut under: Vec<u32> = vec![0; th.typ.len()];
    for id in (1..th.typ.len()).rev() {
        if th.typ[id] == 2 {
            let p = th.parent[id] as usize;
            if p < id && p < under.len() {
                under[p] = under[p].saturating_add(under[id] + 1);
            }
        }
    }
    eprintln!("chloedb-hn: pass 1 done: {} users, {} stories, {:.0} s", th.users.len() - 1, th.titles.len(), t0.elapsed().as_secs_f64());

    // Pass 2: the rows.
    let mut stories = Out::new(&o.out, "hn_stories.jsonl")?;
    let mut comments = Out::new(&o.out, "hn_comments.jsonl")?;
    let mut links = Out::new(&o.out, "hn_links.jsonl")?;
    #[derive(Default, Clone)]
    struct U {
        stories: u32,
        comments: u32,
        points: i64,
        first: i64,
        last: i64,
        replies_got: u32,
    }
    let mut us: Vec<U> = vec![U::default(); th.users.len()];
    let mut pairs: HashMap<(u32, u32), u32> = HashMap::new();
    let mut link_pairs: HashMap<(u32, u32), u32> = HashMap::new();
    let mut err = None;
    for f in &files {
        read(f, &["id", "type", "by", "time", "text", "parent", "url", "score", "title", "descendants", "deleted", "dead"], |b| {
            if err.is_some() {
                return;
            }
            let (Some(id), Some(parent)) = (u32s(b, "id"), u32s(b, "parent")) else { return };
            let typ = col(b, "type").and_then(|a| a.as_any().downcast_ref::<Int8Array>());
            let time = micros(b, "time");
            let by = Strs::of(col(b, "by"));
            let html = Strs::of(col(b, "text"));
            let url = Strs::of(col(b, "url"));
            let title = Strs::of(col(b, "title"));
            let score = col(b, "score").and_then(|a| a.as_any().downcast_ref::<Int32Array>());
            let desc = col(b, "descendants").and_then(|a| a.as_any().downcast_ref::<Int32Array>());
            for i in 0..b.num_rows() {
                let x = id.value(i);
                if !th.live(x) {
                    continue;
                }
                let t = typ.map(|t| t.value(i)).unwrap_or(0);
                let tus = time.get(i).copied().unwrap_or(0);
                let author = by.get(i);
                let ui = th.by[x as usize] as usize;
                if ui != 0 {
                    let u = &mut us[ui];
                    if u.first == 0 || tus < u.first {
                        u.first = tus;
                    }
                    u.last = u.last.max(tus);
                }
                match t {
                    1 | 3 | 5 => {
                        let tt = title.get(i);
                        if tt.is_empty() {
                            continue;
                        }
                        let s = score.map(|s| s.value(i) as i64).unwrap_or(0);
                        let d = desc.map(|d| d.value(i) as i64).unwrap_or(0);
                        if ui != 0 {
                            us[ui].stories += 1;
                            us[ui].points += s.max(0);
                        }
                        let row = story_row(x, t, author, tus, tt, html.get(i), url.get(i), s, d);
                        for l in text::item_links(html.get(i)) {
                            let (r, _) = th.root(l);
                            if r != x && th.titles.contains_key(&r) {
                                *link_pairs.entry((x, r)).or_insert(0) += 1;
                            }
                        }
                        if let Err(e) = stories.row(&row) {
                            err = Some(e);
                            return;
                        }
                    }
                    2 => {
                        let p = parent.value(i);
                        let (story, depth) = th.root(x);
                        let pu = th.by.get(p as usize).copied().unwrap_or(0);
                        if ui != 0 {
                            us[ui].comments += 1;
                        }
                        if pu != 0 {
                            us[pu as usize].replies_got += 1;
                            // Only replies to comments: a top-level comment answers the story.
                            if th.typ.get(p as usize).is_some_and(|&t| t & !DEAD == 2) && pu as usize != ui && ui != 0 {
                                *pairs.entry((ui as u32, pu)).or_insert(0) += 1;
                            }
                        }
                        let h = html.get(i);
                        if th.titles.contains_key(&story) {
                            for l in text::item_links(h) {
                                let (r, _) = th.root(l);
                                if r != story && th.titles.contains_key(&r) {
                                    *link_pairs.entry((story, r)).or_insert(0) += 1;
                                }
                            }
                        }
                        if tus < since || tus >= until || h.is_empty() {
                            continue;
                        }
                        let st = th.titles.get(&story).map(String::as_str).unwrap_or("");
                        let row = comment_row(x, author, tus, h, p, &th.users[pu as usize], story, st, depth, th.pos[x as usize], replies[x as usize] as usize, under[x as usize]);
                        if let Err(e) = comments.row(&row) {
                            err = Some(e);
                            return;
                        }
                    }
                    _ => {}
                }
            }
        })?;
        if let Some(e) = err.take() {
            return Err(e);
        }
        eprintln!("chloedb-hn: pass 2 {} ({} stories, {} comments, {:.0} s)", f.file_name().unwrap_or_default().to_string_lossy(), stories.n, comments.n, t0.elapsed().as_secs_f64());
    }
    let ns = stories.done()?;
    let nc = comments.done()?;
    for ((a, b), w) in &link_pairs {
        links.row(&json!({"id": format!("{a}>{b}"), "src": a, "dst": b, "w": w}))?;
    }
    let nl = links.done()?;

    let mut users = Out::new(&o.out, "hn_users.jsonl")?;
    let mut replies_out = Out::new(&o.out, "hn_replies.jsonl")?;
    let mut talked: Vec<u32> = vec![0; th.users.len()];
    for ((a, b), w) in &pairs {
        if *w >= o.min_replies {
            talked[*a as usize] += 1;
            replies_out.row(&json!({"id": format!("{}>{}", th.users[*a as usize], th.users[*b as usize]), "src": th.users[*a as usize], "dst": th.users[*b as usize], "w": w}))?;
        }
    }
    for (i, u) in us.iter().enumerate().skip(1) {
        if u.stories == 0 && u.comments == 0 {
            continue;
        }
        users.row(&json!({"id": th.users[i], "stories": u.stories, "comments": u.comments, "points": u.points, "first": iso(u.first), "last": iso(u.last), "replies_got": u.replies_got, "talked_to": talked[i]}))?;
    }
    let nu = users.done()?;
    let nr = replies_out.done()?;
    let _ = Arc::new(());
    eprintln!("chloedb-hn: wrote {ns} stories, {nc} comments, {nu} people, {nr} reply pairs, {nl} links between discussions into {} in {:.0} s", o.out.display(), t0.elapsed().as_secs_f64());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn months_span_years() {
        assert_eq!(months("2006-10", "2007-02"), vec!["2006-10", "2006-11", "2006-12", "2007-01", "2007-02"]);
    }

    #[test]
    fn times_in_any_unit_read_as_microseconds() {
        use std::sync::Arc;
        let ms: ArrayRef = Arc::new(TimestampMillisecondArray::from(vec![1_160_418_111_000i64]).with_timezone("UTC"));
        let b = RecordBatch::try_from_iter(vec![("time", ms)]).unwrap();
        assert_eq!(micros(&b, "time"), vec![1_160_418_111_000_000]);
        let s: ArrayRef = Arc::new(TimestampSecondArray::from(vec![1_160_418_111i64]));
        let b = RecordBatch::try_from_iter(vec![("time", s)]).unwrap();
        assert_eq!(micros(&b, "time"), vec![1_160_418_111_000_000]);
    }

    #[test]
    fn rows_carry_what_search_needs() {
        let s = story_row(1, 1, "pg", 1_160_418_111_000_000, "Show HN: Y Combinator", "", "http://www.ycombinator.com", 57, 3);
        assert_eq!(s["kind"], "show");
        assert_eq!(s["domain"], "ycombinator.com");
        assert_eq!(s["year"], 2006);
        assert_eq!(s["created"], "2006-10-09T18:21:51Z");
        let c = comment_row(15, "sama", 1_160_423_461_000_000, "a<p>b", 1, "pg", 1, "Y Combinator", 1, 0, 2, 5);
        assert_eq!(c["body"], "a\n\nb");
        assert_eq!(c["story_title"], "Y Combinator");
    }
}
