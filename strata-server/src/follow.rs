//! `strata-server follow`: a cluster that follows another. It reads each
//! shard's feed of the source cluster ([`crate::service::feed`]) and
//! writes its changes through the follower's coordinator, in the order
//! [`crate::cores::follow`] gives, so that the follower holds the source's
//! rows, tables and rank profiles a moment behind it.
//!
//! On its first start it copies: each source shard sends its rows at a
//! version, then its feed from there; the copies are taken only while
//! every shard serves the same slot map and tables. It then follows. Its
//! writes carry the cluster's token and name the source it follows
//! ([`crate::service::FOLLOW_HEADER`]): the follower takes them, and from
//! then on refuses every other client's writes (409) until it stops
//! following (`POST /v1/admin/follow/stop`).
//!
//! What it applied is kept in the follower's stream sources, written with
//! each write: `follow/<name>/copy`, when the copy it follows from began
//! (its offset, in ms: a copy made again, as after a failover the source
//! lost transactions in, starts afresh), and under `follow/<name>/<copy>/`
//! that the copy is done (`copied`), per source shard its offset (`<i>/at`,
//! `<i>/term`; every transaction up to it is applied), per shard and table
//! the last transaction of the table applied (`<i>/t/<table>`, by which a
//! transaction read again is skipped), the slot marks passed (`left/<i>/<id>`), how many changes of
//! each table it applied (`changes/<table>`), and for a shard the source
//! added since the copy (`<j>/added`), what it joins at
//! (`<j>/join/<table>`, `<j>/joined`). A follow process that starts again
//! goes on from there; one that finds no copy done copies again.
//!
//! Its rows carry no offsets of their own: the follower takes no writes
//! but its follow process's, so a write written again (after a write that
//! failed partway, or a restart) gives the rows it gave, and the marks,
//! written with the rows they cover, say what not to write again. (An
//! offset on each operation, skipped once a shard holds it, would lose
//! writes while the follower's slots move: its coordinator gives every
//! shard every offset of a request, also one that its rows reach only
//! when routed again.)

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use serde_json::{json, Value as Json};
use strata::query::ddl::{alter_sql, create_sql};
use strata::query::sql::ident;
use strata::schema::Schema;
use strata::storage::{RankProfile, SOURCES};

use crate::cores::follow::{Order, Step, Txn, Version};
use crate::service::feed::{follows_offset, FOLLOWS};
use crate::service::FOLLOW_HEADER;
use crate::ApiError;

/// What `strata-server follow` is told.
#[derive(Clone, Debug)]
pub struct Options {
    /// The source cluster's HTTP URL: its coordinator, or a server.
    pub source: String,
    /// A key of the source that reads its feed (`feed` scope or above).
    pub source_key: Option<String>,
    /// A CA certificate (PEM) the source's TLS certificate is checked against.
    pub source_ca: Option<Vec<u8>>,
    /// The follower's HTTP URL: its coordinator, or a server.
    pub target: String,
    /// The follower's cluster token.
    pub target_token: Option<String>,
    pub target_ca: Option<Vec<u8>>,
    /// The name the follower knows the source by.
    pub name: String,
    /// The most operations of one write.
    pub batch: usize,
    /// Copy the source again, whatever the follower kept.
    pub copy: bool,
}

/// How many transactions of a shard it holds read ahead of what it applied.
const QUEUED: usize = 10_000;

/// How often it tells the follower how it stands.
const STATUS_EVERY: Duration = Duration::from_secs(2);

/// How long a reader waits before it connects again.
const RECONNECT: Duration = Duration::from_secs(1);

/// How many times a copy is tried while the shards' versions differ.
const COPY_TRIES: u32 = 30;

/// A cluster over HTTP.
#[derive(Clone)]
struct Http {
    url: String,
    client: reqwest::Client,
    auth: Option<String>,
    /// The source the follower's writes follow ([`FOLLOW_HEADER`]).
    follow: Option<String>,
}

impl Http {
    fn new(url: &str, token: Option<&str>, ca: Option<&[u8]>, follow: Option<&str>) -> Result<Http, ApiError> {
        let mut b = crate::http_client().connect_timeout(Duration::from_secs(5));
        if let Some(ca) = ca {
            let cert = reqwest::Certificate::from_pem(ca).map_err(|e| ApiError::bad(format!("the CA certificate for {url}: {e}")))?;
            b = b.add_root_certificate(cert);
        }
        let client = b.build().map_err(|e| ApiError::bad(format!("an HTTP client for {url}: {e}")))?;
        Ok(Http { url: url.trim_end_matches('/').to_string(), client, auth: token.map(|t| format!("Bearer {t}")), follow: follow.map(String::from) })
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let mut r = self.client.request(method, format!("{}{path}", self.url));
        if let Some(a) = &self.auth {
            r = r.header(reqwest::header::AUTHORIZATION, a);
        }
        if let Some(f) = &self.follow {
            r = r.header(FOLLOW_HEADER, f);
        }
        r
    }

    /// Calls `path`, answering its JSON, or its error with its status.
    async fn call(&self, method: reqwest::Method, path: &str, body: Option<&Json>) -> Result<Json, ApiError> {
        let mut r = self.request(method, path).timeout(Duration::from_secs(120));
        if let Some(b) = body {
            r = r.json(b);
        }
        let res = r.send().await.map_err(|e| unavailable(&self.url, e))?;
        let status = res.status().as_u16();
        let j: Json = res.json().await.unwrap_or(Json::Null);
        if status >= 300 {
            let message = j.get("error").and_then(Json::as_str).map(String::from).unwrap_or_else(|| format!("{} answered {status}", self.url));
            return Err(ApiError { status, message });
        }
        Ok(j)
    }

    async fn get(&self, path: &str) -> Result<Json, ApiError> {
        self.call(reqwest::Method::GET, path, None).await
    }

    async fn post(&self, path: &str, body: &Json) -> Result<Json, ApiError> {
        self.call(reqwest::Method::POST, path, Some(body)).await
    }

    /// The lines of `path`'s answer, an NDJSON stream.
    async fn lines(&self, path: &str) -> Result<Lines, ApiError> {
        let res = self.request(reqwest::Method::GET, path).send().await.map_err(|e| unavailable(&self.url, e))?;
        let status = res.status().as_u16();
        if status >= 300 {
            let j: Json = res.json().await.unwrap_or(Json::Null);
            let message = j.get("error").and_then(Json::as_str).map(String::from).unwrap_or_else(|| format!("{} answered {status}", self.url));
            return Err(ApiError { status, message });
        }
        Ok(Lines { body: res.bytes_stream().map(|b| b.map(|b| b.to_vec())).boxed(), buf: Vec::new() })
    }
}

fn unavailable(url: &str, e: reqwest::Error) -> ApiError {
    ApiError { status: 503, message: format!("{url}: {e}") }
}

/// An NDJSON answer, a line at a time.
struct Lines {
    body: futures::stream::BoxStream<'static, reqwest::Result<Vec<u8>>>,
    buf: Vec<u8>,
}

impl Lines {
    /// The next line as JSON; `None` at the end.
    async fn next(&mut self) -> Option<Result<Json, String>> {
        loop {
            if let Some(n) = self.buf.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = self.buf.drain(..=n).collect();
                if line.len() <= 1 {
                    continue;
                }
                return Some(serde_json::from_slice(&line[..line.len() - 1]).map_err(|e| format!("a line of the feed: {e}")));
            }
            match self.body.next().await {
                Some(Ok(b)) => self.buf.extend_from_slice(&b),
                Some(Err(e)) => return Some(Err(e.to_string())),
                None => return None,
            }
        }
    }
}

/// What a reader of a source shard's feed tells the follower.
enum Event {
    Txn(Txn),
    /// How far the shard's feed reaches (its heartbeat).
    Beat(u64),
    /// The feed ended: why, and whether reading again may help.
    End { why: String, again: bool },
}

/// A version of a table as the follower compares them: its `CREATE
/// TABLE` statement, which names no field ids, hashed.
fn token(schema: &Schema) -> String {
    format!("{:016x}", twox_hash::XxHash3_64::oneshot(create_sql(schema).as_bytes()))
}

fn schema_of(j: &Json) -> Result<Schema, ApiError> {
    serde_json::from_value(j.clone()).map_err(|e| ApiError::bad(format!("a table's schema in the feed: {e}")))
}

/// Whether a table is the server's own, which the follower leaves.
fn own_table(t: &str) -> bool {
    t.eq_ignore_ascii_case(SOURCES) || t.eq_ignore_ascii_case(strata::schema::PLACEHOLDER_TABLE)
}

/// A transaction line of a feed as the follower orders it.
fn txn_of(j: &Json, copy_at: Option<(u64, u64)>) -> Txn {
    let seq = j["seq"].as_u64().unwrap_or(0);
    let term = j["term"].as_u64().unwrap_or(0);
    let ops: Vec<Json> = j["ops"].as_array().cloned().unwrap_or_default().into_iter().filter(|o| !o["table"].as_str().is_some_and(own_table)).collect();
    if j.get("copy") == Some(&Json::Bool(true)) {
        let (seq, term) = copy_at.unwrap_or_default();
        return Txn { seq, term, ops, copy: true, ..Default::default() };
    }
    let change = ops.iter().any(|o| matches!(o["op"].as_str(), Some("alter_table" | "drop_table" | "profile")));
    let versions = ops
        .iter()
        .filter_map(|o| {
            let key = o["table"].as_str()?.to_string();
            let tok = |k: &str| o.get(k).and_then(|s| schema_of(s).ok()).map(|s| token(&s));
            match o["op"].as_str()? {
                "create_table" => Some(Version { key, was: None, now: tok("schema")? }),
                "alter_table" => Some(Version { key, was: tok("was"), now: tok("schema")? }),
                "drop_table" => Some(Version { key, was: tok("was"), now: "dropped".into() }),
                _ => None,
            }
        })
        .collect();
    let mut t = Txn { seq, term, ops, change, versions, ..Default::default() };
    for m in j["slots"].as_array().into_iter().flatten() {
        let id = m["id"].as_str().unwrap_or_default().to_string();
        if let (Some(_), Some(to)) = (m.get("left"), m["to"].as_u64()) {
            t.left.push((to as usize, id));
        } else if let (Some(_), Some(from)) = (m.get("arrived"), m["from"].as_u64()) {
            t.arrived.push((from as usize, id));
        }
    }
    t.sources = j["sources"].as_array().cloned().unwrap_or_default();
    t
}

/// Reads a source shard's feed from `lines`, or from `path` once
/// connected, and sends what it reads on `tx`. A copy's rows take the
/// version of its first line (`copy_at`).
async fn read(src: Http, path: String, lines: Option<Lines>, mut copy_at: Option<(u64, u64)>, tx: tokio::sync::mpsc::Sender<Event>, notify: Arc<tokio::sync::Notify>) {
    let send = |e: Event| {
        let (tx, notify) = (tx.clone(), notify.clone());
        async move {
            let ok = tx.send(e).await.is_ok();
            notify.notify_one();
            ok
        }
    };
    let mut lines = match lines {
        Some(l) => l,
        None => match src.lines(&path).await {
            Ok(l) => l,
            Err(e) => {
                send(Event::End { why: e.message, again: e.status >= 500 || e.status == 409 || e.status == 429 }).await;
                return;
            }
        },
    };
    loop {
        let j = match lines.next().await {
            None => {
                send(Event::End { why: "the feed ended".into(), again: true }).await;
                return;
            }
            Some(Err(e)) => {
                send(Event::End { why: e, again: true }).await;
                return;
            }
            Some(Ok(j)) => j,
        };
        let e = if let Some(h) = j.get("copy").filter(|c| c.is_object()) {
            copy_at = Some((h["seq"].as_u64().unwrap_or(0), h["term"].as_u64().unwrap_or(0)));
            continue;
        } else if let Some(c) = j.get("copied") {
            Event::Txn(Txn { seq: c["seq"].as_u64().unwrap_or(0), term: c["term"].as_u64().unwrap_or(0), sources: c["sources"].as_array().cloned().unwrap_or_default(), copied: true, ..Default::default() })
        } else if j.get("copy").is_some() || j.get("seq").is_some() {
            Event::Txn(txn_of(&j, copy_at))
        } else if let Some(last) = j.get("last").and_then(Json::as_u64) {
            Event::Beat(last)
        } else if j.get("feed").is_some() {
            continue;
        } else if let Some(g) = j.get("gone") {
            let oldest = g["oldest"].as_u64().unwrap_or(0);
            Event::End { why: format!("the source's log no longer holds the transactions the follower needs (it starts at {oldest}): copy again (--copy)"), again: false }
        } else if let Some(d) = j.get("diverged") {
            Event::End { why: format!("the source no longer holds the last transaction the follower applied (its last is {} of term {}): a failover lost it; copy again (--copy)", d["seq"], d["term"]), again: false }
        } else if let Some(e) = j.get("error") {
            let status = j["status"].as_u64().unwrap_or(500);
            Event::End { why: e.as_str().unwrap_or_default().to_string(), again: status >= 500 || status == 409 || status == 429 }
        } else {
            continue;
        };
        let end = matches!(e, Event::End { .. });
        if !send(e).await || end {
            return;
        }
    }
}

/// A source shard's reader as the follower keeps it.
#[derive(Default)]
struct Reader {
    rx: Option<tokio::sync::mpsc::Receiver<Event>>,
    /// How far its feed reaches.
    last: u64,
    /// Why it stopped, and when to read again.
    ended: Option<String>,
    again_at: Option<Instant>,
    /// Its first copy is applied.
    copied: bool,
}

/// The follower, and what it keeps of the source.
struct Follow {
    o: Options,
    src: Http,
    dst: Http,
    /// Whether the source is a coordinator (whose feed is each shard's).
    cluster: bool,
    order: Order,
    readers: Vec<Reader>,
    notify: Arc<tokio::sync::Notify>,
    /// The follower's tables, by name in lower case.
    tables: HashMap<String, Schema>,
    /// The follower's shards' numbers from its last write, which a read
    /// after it waits for.
    seen: Vec<u64>,
    /// The offset of its `_strata/follows` mark.
    follows: i64,
    /// When the copy it follows from began (ms), which names its marks.
    copy_ms: u64,
    copying: bool,
    error: Option<String>,
    rows_copied: u64,
    written: u64,
    started_ms: u64,
    status_at: Option<Instant>,
    /// When it last wrote to the follower.
    wrote_at: Instant,
}

/// Follows `o.source` into `o.target` until it fails for good.
pub async fn run(o: Options) -> Result<(), ApiError> {
    if !crate::service::follow_name_ok(&o.name) {
        return Err(ApiError::bad("--name is 1 to 64 letters, digits, '.', '_' or '-'"));
    }
    let src = Http::new(&o.source, o.source_key.as_deref(), o.source_ca.as_deref(), None)?;
    let dst = Http::new(&o.target, o.target_token.as_deref(), o.target_ca.as_deref(), Some(&o.name))?;
    let health = retry("the source's health", || src.get("/v1/health")).await?;
    let shards = health.get("shards").and_then(Json::as_array).map(Vec::len);
    let mut f = Follow {
        cluster: shards.is_some(),
        order: Order::new(0),
        readers: Vec::new(),
        notify: Arc::new(tokio::sync::Notify::new()),
        tables: HashMap::new(),
        seen: Vec::new(),
        follows: follows_offset(true),
        copy_ms: 0,
        copying: false,
        error: None,
        rows_copied: 0,
        written: 0,
        started_ms: crate::telemetry::now_ms(),
        status_at: None,
        wrote_at: Instant::now(),
        o,
        src,
        dst,
    };
    // From here on the follower takes no other client's writes.
    f.flush(&mut Vec::new(), true).await?;
    f.load_tables().await?;
    let kept = f.kept().await?;
    let n = shards.unwrap_or(1);
    match kept.copy_ms {
        Some(c) if kept.copied && !f.o.copy => {
            f.copy_ms = c;
            f.resume(n, &kept);
        }
        _ => {
            f.copy_ms = crate::telemetry::now_ms().max(kept.copy_ms.map_or(0, |c| c + 1));
            f.copy(n).await?;
        }
    }
    let r = f.follow().await;
    if let Err(e) = &r {
        f.error = Some(e.message.clone());
        f.status(true).await;
    }
    r
}

/// Calls `f` until it answers or fails for good (a 4xx but 408, 409 and
/// 429), waiting longer each time, up to 2 minutes in all.
async fn retry<F, Fut>(what: &str, f: F) -> Result<Json, ApiError>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<Json, ApiError>>,
{
    let until = Instant::now() + Duration::from_secs(120);
    let mut wait = Duration::from_millis(200);
    loop {
        match f().await {
            Ok(j) => return Ok(j),
            Err(e) if again(&e) && Instant::now() + wait < until => {
                tracing::warn!(status = e.status, "{what}: {}; trying again", e.message);
                tokio::time::sleep(wait).await;
                wait = (wait * 2).min(Duration::from_secs(10));
            }
            Err(e) => return Err(ApiError { status: e.status, message: format!("{what}: {}", e.message) }),
        }
    }
}

/// Whether a failed call may answer when tried again.
fn again(e: &ApiError) -> bool {
    e.status >= 500 || matches!(e.status, 408 | 429) || (e.status == 409 && !e.message.contains("follow"))
}

/// What the follower kept (the names of [`self`]), the least over its
/// shards of each.
#[derive(Debug, Default)]
struct Kept {
    /// The last copy begun, and whether it is done.
    copy_ms: Option<u64>,
    copied: bool,
    at: BTreeMap<usize, (u64, u64)>,
    tables: Vec<(usize, String, u64)>,
    left: Vec<(usize, String, u64)>,
    changes: BTreeMap<String, u64>,
    added: Vec<usize>,
    joins: BTreeMap<usize, BTreeMap<String, u64>>,
    joined: Vec<usize>,
}

impl Follow {
    /// The follower's tables, read again.
    async fn load_tables(&mut self) -> Result<(), ApiError> {
        let j = retry("the follower's tables", || self.dst.get("/v1/tables")).await?;
        self.tables = j["tables"].as_array().into_iter().flatten().filter_map(|t| serde_json::from_value::<Schema>(t["schema"].clone()).ok()).map(|s| (s.table.to_ascii_lowercase(), s)).collect();
        Ok(())
    }

    /// What the follower kept, from each of its shards' stream sources.
    async fn kept(&self) -> Result<Kept, ApiError> {
        let j = retry("the follower's stream sources", || self.dst.get("/v1/sources?shards=true")).await?;
        let copy_name = format!("follow/{}/copy", self.o.name);
        let copy_ms = j["shards"].as_array().into_iter().flatten().flat_map(|s| s.as_array().into_iter().flatten()).filter(|s| s["name"] == copy_name.as_str()).filter_map(|s| s["offset"].as_u64()).max();
        let mut k = Kept { copy_ms, ..Default::default() };
        let Some(copy_ms) = copy_ms else { return Ok(k) };
        let prefix = format!("follow/{}/{copy_ms}/", self.o.name);
        // Per name, per shard: the least over the shards that have it.
        let mut least: BTreeMap<String, i64> = BTreeMap::new();
        let mut terms: BTreeMap<String, (i64, i64)> = BTreeMap::new();
        for shard in j["shards"].as_array().into_iter().flatten() {
            let mine: HashMap<&str, i64> = shard.as_array().into_iter().flatten().filter_map(|s| Some((s["name"].as_str()?, s["offset"].as_i64()?))).collect();
            for (name, offset) in &mine {
                let Some(rest) = name.strip_prefix(&prefix) else { continue };
                let e = least.entry(rest.to_string()).or_insert(*offset);
                *e = (*e).min(*offset);
                // A shard's term goes with its least offset.
                if let Some(i) = rest.strip_suffix("/at") {
                    let term = mine.get(format!("{prefix}{i}/term").as_str()).copied().unwrap_or(0);
                    let t = terms.entry(i.to_string()).or_insert((*offset, term));
                    if *offset < t.0 {
                        *t = (*offset, term);
                    }
                }
            }
        }
        for (rest, offset) in least {
            let parts: Vec<&str> = rest.splitn(3, '/').collect();
            let off = offset.max(0) as u64;
            match parts.as_slice() {
                [i, "at"] => {
                    let term = terms.get(*i).map_or(0, |t| t.1.max(0) as u64);
                    if let Ok(i) = i.parse::<usize>() {
                        k.at.insert(i, (off, term));
                    }
                }
                [i, "t", table] => {
                    if let Ok(i) = i.parse() {
                        k.tables.push((i, table.to_string(), off));
                    }
                }
                ["left", i, id] => {
                    if let Ok(i) = i.parse() {
                        k.left.push((i, id.to_string(), off));
                    }
                }
                ["changes", key] => {
                    k.changes.insert(key.to_string(), off);
                }
                [i, "join", key] => {
                    if let Ok(i) = i.parse() {
                        k.joins.entry(i).or_default().insert(key.to_string(), off);
                    }
                }
                [i, "joined"] => {
                    if let Ok(i) = i.parse() {
                        k.joined.push(i);
                    }
                }
                [i, "added"] => {
                    if let Ok(i) = i.parse() {
                        k.added.push(i);
                    }
                }
                ["copied"] => k.copied = true,
                _ => {}
            }
        }
        Ok(k)
    }

    /// The path of shard `i`'s feed.
    fn feed_path(&self, i: usize, from: u64, after: u64, copy: bool) -> String {
        let mut p = format!("/v1/feed?from={from}");
        if self.cluster {
            p.push_str(&format!("&shard={i}"));
        }
        if after > 0 {
            p.push_str(&format!("&after_term={after}"));
        }
        if copy {
            p.push_str("&copy=true");
        }
        p
    }

    /// Starts reading shard `i` from transaction `from` (after `after`'s
    /// term), or with `lines` already open.
    fn start(&mut self, i: usize, from: u64, after: u64, lines: Option<Lines>, copy_at: Option<(u64, u64)>) {
        if self.readers.len() <= i {
            self.readers.resize_with(i + 1, Reader::default);
        }
        let (tx, rx) = tokio::sync::mpsc::channel(256);
        let path = self.feed_path(i, from, after, false);
        tokio::spawn(read(self.src.clone(), path, lines, copy_at, tx, self.notify.clone()));
        let r = &mut self.readers[i];
        r.rx = Some(rx);
        r.ended = None;
        r.again_at = None;
    }

    /// Copies every shard of the source: once each serves the same slot
    /// map, tables and rank profiles, makes the tables here, and reads on.
    async fn copy(&mut self, n: usize) -> Result<(), ApiError> {
        self.copying = true;
        let mut tries = 0;
        let (heads, lines) = loop {
            tries += 1;
            let mut heads = Vec::with_capacity(n);
            let mut lines = Vec::with_capacity(n);
            for i in 0..n {
                let path = self.feed_path(i, 0, 0, true);
                let mut l = self.src.lines(&path).await.map_err(|e| ApiError { status: e.status, message: format!("the copy of shard {i}: {}", e.message) })?;
                let head = match l.next().await {
                    Some(Ok(j)) => j,
                    Some(Err(e)) => return Err(ApiError { status: 503, message: format!("the copy of shard {i}: {e}") }),
                    None => return Err(ApiError { status: 503, message: format!("the copy of shard {i} ended at once") }),
                };
                if let Some(e) = head.get("error") {
                    return Err(ApiError { status: head["status"].as_u64().unwrap_or(500) as u16, message: format!("the copy of shard {i}: {}", e.as_str().unwrap_or_default()) });
                }
                let Some(h) = head.get("copy").filter(|c| c.is_object()).cloned() else {
                    return Err(ApiError::internal(format!("the copy of shard {i} began with {head}, not its version: the source needs shard protocol {}", crate::grpc::FEED_COPY_PROTOCOL)));
                };
                heads.push(h);
                lines.push(l);
            }
            // The same tables, as statements (a shard made later numbers
            // their fields its own way), and rank profiles.
            let shape = |h: &Json| {
                let mut t: Vec<String> = h["tables"].as_array().into_iter().flatten().map(|s| schema_of(s).map_or_else(|e| e.message, |s| create_sql(&s))).collect();
                t.sort();
                let mut p: Vec<String> = h["profiles"].as_array().into_iter().flatten().map(Json::to_string).collect();
                p.sort();
                (h["epoch"].clone(), t, p)
            };
            let same = heads.iter().all(|h| shape(h) == shape(&heads[0]));
            if same {
                break (heads, lines);
            }
            if tries >= COPY_TRIES {
                return Err(ApiError { status: 503, message: format!("the source's shards served different slot maps or tables in {COPY_TRIES} tries of a copy: a move or a change of tables was under way; start again") });
            }
            tracing::info!(tries, "the source's shards differ (a slot move or a change of tables): copying again");
            drop(lines);
            tokio::time::sleep(Duration::from_secs(1)).await;
        };
        let tables: Vec<Schema> = heads[0]["tables"].as_array().into_iter().flatten().map(schema_of).collect::<Result<_, _>>()?;
        for s in &tables {
            self.ensure_table(s).await?;
        }
        let profiles = heads[0]["profiles"].as_array().cloned().unwrap_or_default();
        if !profiles.is_empty() {
            self.apply_change(&profiles).await?;
        }
        self.order = Order::new(n).with_batch(self.o.batch).with_versions(tables.iter().map(|s| (s.table.clone(), Some(token(s)), 0)));
        for (i, (l, h)) in lines.into_iter().zip(&heads).enumerate() {
            let at = (h["seq"].as_u64().unwrap_or(0), h["term"].as_u64().unwrap_or(0));
            self.start(i, at.0 + 1, at.1, Some(l), Some(at));
        }
        tracing::info!(shards = n, tables = tables.len(), copy_ms = self.copy_ms, "copying the source");
        // Marks of this copy from here on.
        self.flush(&mut Vec::new(), true).await
    }

    /// Goes on from what the follower kept: each shard from its offset, a
    /// shard it kept none of from its first transaction, as one added.
    fn resume(&mut self, n: usize, k: &Kept) {
        let known = k.at.keys().next_back().map_or(0, |&i| i + 1).max(n);
        let at: Vec<(u64, u64)> = (0..known).map(|i| k.at.get(&i).copied().unwrap_or((0, 0))).collect();
        let versions: Vec<(String, Option<String>, u64)> = self.tables.values().map(|s| (s.table.clone(), Some(token(s)), k.changes.get(&s.table).copied().unwrap_or(0))).collect();
        let mut order = Order::resume(&at, k.left.clone(), k.tables.clone()).with_batch(self.o.batch).with_versions(versions);
        for i in 0..known {
            // A shard added since the copy, or one never applied from.
            if k.added.contains(&i) || !k.at.contains_key(&i) {
                order.join(i, k.joins.get(&i).into_iter().flatten().map(|(t, n)| (t.clone(), *n)));
                if k.joined.contains(&i) {
                    order.unjoin(i);
                }
            }
        }
        self.order = order;
        for i in 0..known {
            let (seq, term) = self.order.at(i);
            self.start(i, seq + 1, term, None, None);
            self.readers[i].copied = true;
        }
        tracing::info!(shards = known, "following the source from where the follower was");
    }

    /// Follows until it fails for good.
    async fn follow(&mut self) -> Result<(), ApiError> {
        loop {
            self.take_events()?;
            self.restart_readers();
            match self.order.next() {
                Step::Write(b) => self.write(b).await?,
                Step::Change { ops, at } => {
                    self.apply_change(&ops).await?;
                    let mut marks = Vec::new();
                    for (i, seq, _) in at {
                        for t in ops.iter().filter_map(|o| o["table"].as_str()) {
                            marks.push(self.mark(&format!("{i}/t/{t}"), seq as i64));
                        }
                    }
                    self.flush(&mut marks, true).await?;
                }
                Step::Read(j) => {
                    tracing::info!(shard = j, "the source added a shard: reading it");
                    self.start(j, 1, 0, None, None);
                    self.readers[j].copied = true;
                }
                Step::Stuck(why) => {
                    self.error = Some(why);
                    self.wait().await;
                }
                Step::Wait => {
                    let caught_up = self.readers.iter().enumerate().all(|(i, r)| r.ended.is_none() && self.order.read_to(i).0 >= r.last);
                    self.error = match caught_up {
                        true => self.order.blocked(),
                        false => self.readers.iter().enumerate().find_map(|(i, r)| r.ended.as_ref().map(|e| format!("shard {i}: {e}"))),
                    };
                    self.wait().await;
                }
            }
            if self.copying && self.readers.iter().all(|r| r.copied) && (0..self.order.shards()).all(|i| self.order.at(i).0 > 0 || self.order.queued(i) == 0) {
                self.copying = false;
                let mut m = vec![self.mark("copied", 1)];
                self.flush(&mut m, true).await?;
                tracing::info!(rows = self.rows_copied, "copied the source: following it");
            }
            // Its marks now and then, with nothing to write: a shard the
            // follower added holds them too, and so refuses clients.
            if self.wrote_at.elapsed() >= STATUS_EVERY {
                self.flush(&mut Vec::new(), true).await?;
            }
            self.status(false).await;
        }
    }

    /// Waits for a reader to read more, up to a second.
    async fn wait(&self) {
        let _ = tokio::time::timeout(Duration::from_secs(1), self.notify.notified()).await;
    }

    /// Hands the order what the readers read, as far as it holds them.
    fn take_events(&mut self) -> Result<(), ApiError> {
        for i in 0..self.readers.len() {
            while self.order.queued(i) < QUEUED {
                let Some(rx) = self.readers[i].rx.as_mut() else { break };
                match rx.try_recv() {
                    Ok(Event::Txn(t)) => {
                        if t.copy {
                            self.rows_copied += t.ops.len() as u64;
                        }
                        if t.copied {
                            self.readers[i].copied = true;
                        }
                        self.order.offer(i, t);
                    }
                    Ok(Event::Beat(last)) => self.readers[i].last = last,
                    Ok(Event::End { why, again }) => {
                        // A copy cut short is copied again from the start.
                        if !again || !self.readers[i].copied {
                            return Err(ApiError { status: 503, message: format!("shard {i}'s feed: {why}") });
                        }
                        tracing::warn!(shard = i, "the feed stopped: {why}; reading again");
                        let r = &mut self.readers[i];
                        r.rx = None;
                        r.ended = Some(why);
                        r.again_at = Some(Instant::now() + RECONNECT);
                    }
                    Err(_) => break,
                }
            }
        }
        Ok(())
    }

    /// Reads again the shards whose feeds stopped, once their wait passed.
    fn restart_readers(&mut self) {
        for i in 0..self.readers.len() {
            if self.readers[i].again_at.is_some_and(|t| Instant::now() >= t) {
                let (seq, term) = self.order.read_to(i);
                self.start(i, seq + 1, term, None, None);
            }
        }
    }

    /// The name of mark `rest` of the copy it follows from.
    fn name(&self, rest: &str) -> String {
        format!("follow/{}/{}/{rest}", self.o.name, self.copy_ms)
    }

    /// The operation that keeps `offset` under [`Self::name`] `rest`.
    fn mark(&self, rest: &str, offset: i64) -> Json {
        json!({"op": "source", "source": {"name": self.name(rest), "offset": offset}})
    }

    /// Writes transactions `batch`: each operation with the offset of its
    /// shard and table, and what the follower keeps.
    async fn write(&mut self, batch: Vec<(usize, Txn)>) -> Result<(), ApiError> {
        let mut ops: Vec<Json> = Vec::new();
        // Per shard and table, the last transaction written.
        let mut written: BTreeMap<(usize, String), u64> = BTreeMap::new();
        for (i, t) in &batch {
            if !t.copy {
                for table in t.tables() {
                    let e = written.entry((*i, table.to_string())).or_default();
                    *e = (*e).max(t.seq);
                }
            }
            for op in &t.ops {
                let table = op["table"].as_str().unwrap_or_default().to_string();
                match op["op"].as_str() {
                    Some("create_table") => {
                        self.flush(&mut ops, false).await?;
                        self.ensure_table(&schema_of(&op["schema"])?).await?;
                    }
                    Some("delete" | "set") if op.get("routing").is_none() && self.tables.get(&table.to_ascii_lowercase()).is_some_and(|s| s.shard_by.is_some()) => {
                        // Placed by a column the operation does not carry:
                        // to where the follower holds the row.
                        self.flush(&mut ops, false).await?;
                        ops.extend(self.routed(&table, op).await?);
                    }
                    _ => ops.push(op.clone()),
                }
            }
            // The source's stream sources, as it holds them.
            for s in t.sources.iter().filter(|s| s["name"].as_str().is_some_and(|n| !n.starts_with(crate::service::MARK))) {
                ops.push(json!({"op": "source", "source": {"name": s["name"], "offset": s["offset"]}}));
            }
            for (_, id) in &t.left {
                ops.push(self.mark(&format!("left/{i}/{id}"), t.seq as i64));
            }
        }
        for ((i, table), seq) in written {
            ops.push(self.mark(&format!("{i}/t/{table}"), seq as i64));
        }
        self.flush(&mut ops, true).await
    }

    /// The operation `op` (a delete or a `set` by key of a table placed by
    /// a column) once per value of that column the follower holds the key
    /// under.
    async fn routed(&self, table: &str, op: &Json) -> Result<Vec<Json>, ApiError> {
        let s = self.tables.get(&table.to_ascii_lowercase()).ok_or_else(|| ApiError::bad(format!("table {table} is not here")))?;
        let (Some(pk), Some(by)) = (s.primary_key, s.shard_by.as_deref()) else { return Ok(vec![op.clone()]) };
        let sql = format!("SELECT {} FROM {} WHERE {} = :k LIMIT 100", ident(by), ident(table), ident(&s.fields[pk].name));
        let mut body = json!({"sql": sql, "params": {"k": op["key"]}});
        if !self.seen.is_empty() {
            body["min_seq_nos"] = json!(self.seen);
        }
        let j = match retry("a row's place", || self.dst.post("/v1/sql", &body)).await {
            // The follower's shards changed since the last write.
            Err(e) if e.status == 400 && body.get("min_seq_nos").is_some() => {
                body.as_object_mut().map(|b| b.remove("min_seq_nos"));
                retry("a row's place", || self.dst.post("/v1/sql", &body)).await?
            }
            r => r?,
        };
        Ok(j["rows"].as_array().into_iter().flatten().map(|r| {
            let mut op = op.clone();
            op["routing"] = r[0].clone();
            op
        }).collect())
    }

    /// Writes `ops` with the follower's marks (`marks`: its offsets too),
    /// through the follower's coordinator, until it takes them.
    async fn flush(&mut self, ops: &mut Vec<Json>, marks: bool) -> Result<(), ApiError> {
        if marks && self.copy_ms > 0 {
            ops.push(json!({"op": "source", "source": {"name": format!("follow/{}/copy", self.o.name), "offset": self.copy_ms}}));
            for i in 0..self.order.shards() {
                let (seq, term) = self.order.at(i);
                ops.push(self.mark(&format!("{i}/at"), seq as i64));
                ops.push(self.mark(&format!("{i}/term"), term as i64));
                if let Some(j) = self.order.joins(i) {
                    ops.push(self.mark(&format!("{i}/added"), 1));
                    for (t, n) in j {
                        ops.push(self.mark(&format!("{i}/join/{t}"), *n as i64));
                    }
                    if !self.order.joining(i) {
                        ops.push(self.mark(&format!("{i}/joined"), 1));
                    }
                }
            }
            for t in self.tables.values() {
                let n = self.order.changes(&t.table);
                if n > 0 {
                    ops.push(self.mark(&format!("changes/{}", t.table), n as i64));
                }
            }
        }
        if marks {
            ops.push(json!({"op": "source", "source": {"name": format!("{FOLLOWS}{}", self.o.name), "offset": self.follows}}));
        }
        if ops.is_empty() {
            return Ok(());
        }
        let body = json!({"ops": std::mem::take(ops)});
        let j = loop {
            match self.dst.post("/v1/write", &body).await {
                Ok(j) => break j,
                Err(e) if again(&e) => {
                    self.error = Some(format!("writing to the follower: {}", e.message));
                    self.status(false).await;
                    tokio::time::sleep(RECONNECT).await;
                }
                Err(e) => return Err(ApiError { status: e.status, message: format!("writing to the follower: {}", e.message) }),
            }
        };
        self.error = None;
        self.wrote_at = Instant::now();
        self.written += body["ops"].as_array().map_or(0, Vec::len) as u64;
        if let Some(s) = j["seq_nos"].as_array() {
            let s: Vec<u64> = s.iter().map(|v| v.as_u64().unwrap_or(0)).collect();
            if s.len() != self.seen.len() {
                self.seen = s;
            } else {
                for (a, b) in self.seen.iter_mut().zip(s) {
                    *a = (*a).max(b);
                }
            }
        }
        Ok(())
    }

    /// Runs `sql` on the follower.
    async fn sql(&self, sql: &str) -> Result<Json, ApiError> {
        let body = json!({"sql": sql});
        retry("a change of the follower's tables", || self.dst.post("/v1/sql", &body)).await
    }

    /// Makes table `s` here, unless the follower has it as `s` already.
    async fn ensure_table(&mut self, s: &Schema) -> Result<(), ApiError> {
        if own_table(&s.table) {
            return Ok(());
        }
        match self.tables.get(&s.table.to_ascii_lowercase()) {
            Some(have) if token(have) == token(s) => return Ok(()),
            Some(have) => {
                return Err(ApiError {
                    status: 409,
                    message: format!("table {} here is not the source's:\n{}\nwhere the source has\n{}\nDrop it here, or follow into a cluster without it", s.table, create_sql(have), create_sql(s)),
                })
            }
            None => {}
        }
        self.sql(&create_sql(s)).await?;
        self.load_tables().await
    }

    /// Applies a change of tables or rank profiles, which leaves a table
    /// already changed as it is.
    async fn apply_change(&mut self, ops: &[Json]) -> Result<(), ApiError> {
        for op in ops {
            let table = op["table"].as_str().unwrap_or_default();
            let have = self.tables.get(&table.to_ascii_lowercase()).cloned();
            match op["op"].as_str() {
                Some("alter_table") => {
                    let (was, now) = (schema_of(&op["was"])?, schema_of(&op["schema"])?);
                    match have {
                        Some(h) if token(&h) == token(&now) => {}
                        Some(h) if token(&h) == token(&was) => {
                            if let Some(sql) = alter_sql(&was, &now) {
                                self.sql(&sql).await?;
                            }
                        }
                        _ => return Err(ApiError { status: 409, message: format!("table {table} here is neither as the source had it before its change nor after it") }),
                    }
                }
                Some("drop_table") => {
                    if have.is_some() {
                        self.sql(&format!("DROP TABLE IF EXISTS {}", ident(table))).await?;
                    }
                }
                Some("profile") => {
                    let name = op["name"].as_str().unwrap_or_default();
                    match op.get("def").filter(|d| !d.is_null()) {
                        Some(d) => {
                            let p: RankProfile = serde_json::from_value(d.clone()).map_err(|e| ApiError::bad(format!("rank profile {name} of {table} in the feed: {e}")))?;
                            self.sql(&crate::coord::profile_sql(table, &p)).await?;
                        }
                        None => {
                            self.sql(&format!("DROP RANK PROFILE IF EXISTS {} ON {}", ident(name), ident(table))).await?;
                        }
                    }
                }
                Some(other) => return Err(ApiError::internal(format!("a change of tables in the feed holds a {other} operation"))),
                None => {}
            }
        }
        self.load_tables().await
    }

    /// Tells the follower how it stands (`POST /v1/admin/follow/status`),
    /// every few seconds, or now.
    async fn status(&mut self, now: bool) {
        if !now && self.status_at.is_some_and(|t| t.elapsed() < STATUS_EVERY) {
            return;
        }
        self.status_at = Some(Instant::now());
        let shards: Vec<Json> = (0..self.order.shards())
            .map(|i| {
                let (at, term) = self.order.at(i);
                let r = self.readers.get(i);
                let last = r.map_or(0, |r| r.last).max(at);
                json!({"shard": i, "applied": at, "term": term, "last": last, "behind": last - at, "queued": self.order.queued(i), "joining": self.order.joining(i), "stopped": r.and_then(|r| r.ended.clone())})
            })
            .collect();
        let state = match () {
            _ if self.copying => "copying",
            _ if self.error.is_some() => "held",
            _ => "following",
        };
        let body = json!({
            "name": self.o.name, "source": self.o.source, "state": state, "copy_ms": self.copy_ms, "error": self.error, "shards": shards,
            "rows_copied": self.rows_copied, "written": self.written, "started_ms": self.started_ms, "time_ms": crate::telemetry::now_ms(),
        });
        if let Err(e) = self.dst.post("/v1/admin/follow/status", &body).await {
            tracing::debug!("telling the follower how it stands: {}", e.message);
        }
    }
}

