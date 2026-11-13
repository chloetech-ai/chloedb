//! `chloedb-server follow`: a cluster that follows another. It reads each
//! shard's feed of the source cluster ([`crate::service::feed`]) and
//! writes its changes through the follower's coordinator, in the order
//! [`crate::cores::follow`] gives, so that the follower holds the source's
//! rows, tables and rank profiles a moment behind it.
//!
//! On its first start it copies: each source shard sends its rows at a
//! version, then its feed from there; the copies are taken only while
//! every shard serves the same slot map and tables. It then follows. One
//! that fell behind a source shard's log (its feed is `gone`) catches up
//! on every shard: a copy of the rows changed since what it applied (but
//! of every row of a table that changed), then a sweep of the keys the
//! source deleted meanwhile ([`Follow::sweep`]), then the feeds. Its
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
//! (`<j>/join/<table>`, `<j>/joined`), the tables it made from the source
//! (`made/<table>`), and for a catch-up what it copies from (`<i>/since`,
//! `<i>/since_term`, `whole/<table>`). A follow process that starts again
//! goes on from there; one that finds no copy done copies again from what
//! that copy began from.
//!
//! Its rows carry no offsets of their own: the follower takes no writes
//! but its follow process's, so a write written again (after a write that
//! failed partway, or a restart) gives the rows it gave, and the marks,
//! written with the rows they cover, say what not to write again. (An
//! offset on each operation, skipped once a shard holds it, would lose
//! writes while the follower's slots move: its coordinator gives every
//! shard every offset of a request, also one that its rows reach only
//! when routed again.)

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use serde_json::{json, Value as Json};
use chloedb::query::ddl::{alter_sql, create_sql};
use chloedb::query::sql::ident;
use chloedb::schema::Schema;
use chloedb::storage::{RankProfile, FUNCTIONS, SOURCES};

use crate::cores::follow::{Order, Step, Txn, Version};
use crate::service::feed::{digests_of, follows_offset, next_offset, Digest, Marks, FOLLOWS, HOLD};
use crate::service::FOLLOW_HEADER;
use crate::ApiError;

/// What `chloedb-server follow` is told.
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
    /// The follower is the cluster the source was promoted from
    /// ([`crate::service::Service::promote_follower`]): it follows the
    /// source from the promotion's numbers, without a copy, once it finds
    /// it holds what the source applied of it and nothing since.
    pub reverse: bool,
    /// For a process started again by itself (the operator's Deployment,
    /// [`crate::operator::FollowSpec`]): when it was asked for, in ms
    /// since the epoch. A copy (`copy`) is then made once: one begun at or
    /// after it is resumed. And once this cluster stopped following the
    /// source, or was promoted from it, at or after it, the process does
    /// not follow again but waits, saying so, until it is stopped.
    pub once: Option<u64>,
}

/// How many transactions of a shard it holds read ahead of what it applied.
const QUEUED: usize = 10_000;

/// How often it tells the follower how it stands.
const STATUS_EVERY: Duration = Duration::from_secs(2);

/// How long a reader waits before it connects again.
const RECONNECT: Duration = Duration::from_secs(1);

/// How many times a copy is tried while the shards' versions differ.
const COPY_TRIES: u32 = 30;

/// How many slots' keys a sweep compares at once.
const SWEEP_SLOTS: usize = 256;

/// `s` as a part of a URL's query.
fn url_part(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// A cluster over HTTP.
#[derive(Clone)]
pub(crate) struct Http {
    url: String,
    client: reqwest::Client,
    auth: Option<String>,
    /// The source the follower's writes follow ([`FOLLOW_HEADER`]).
    follow: Option<String>,
}

impl Http {
    pub(crate) fn new(url: &str, token: Option<&str>, ca: Option<&[u8]>, follow: Option<&str>) -> Result<Http, ApiError> {
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
            return Err(ApiError { code: None, status, message });
        }
        Ok(j)
    }

    pub(crate) async fn get(&self, path: &str) -> Result<Json, ApiError> {
        self.call(reqwest::Method::GET, path, None).await
    }

    pub(crate) async fn post(&self, path: &str, body: &Json) -> Result<Json, ApiError> {
        self.call(reqwest::Method::POST, path, Some(body)).await
    }

    /// The lines of `path`'s answer, an NDJSON stream.
    async fn lines(&self, path: &str) -> Result<Lines, ApiError> {
        let res = self.request(reqwest::Method::GET, path).send().await.map_err(|e| unavailable(&self.url, e))?;
        let status = res.status().as_u16();
        if status >= 300 {
            let j: Json = res.json().await.unwrap_or(Json::Null);
            let message = j.get("error").and_then(Json::as_str).map(String::from).unwrap_or_else(|| format!("{} answered {status}", self.url));
            return Err(ApiError { code: None, status, message });
        }
        Ok(Lines { body: res.bytes_stream().map(|b| b.map(|b| b.to_vec())).boxed(), buf: Vec::new() })
    }
}

fn unavailable(url: &str, e: reqwest::Error) -> ApiError {
    ApiError { code: None, status: 503, message: format!("{url}: {e}") }
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
    /// The feed ended: why, whether reading again may help, and whether
    /// because the source's log no longer holds what it needs (`gone`).
    End { why: String, again: bool, gone: bool },
}

/// A version of a table as the follower compares them: its `CREATE
/// TABLE` statement, which names no field ids, hashed.
fn token(schema: &Schema) -> String {
    format!("{:016x}", twox_hash::XxHash3_64::oneshot(create_sql(schema).as_bytes()))
}

fn schema_of(j: &Json) -> Result<Schema, ApiError> {
    serde_json::from_value(j.clone()).map_err(|e| ApiError::bad(format!("a table's schema in the feed: {e}")))
}

/// Whether a table is the server's own, which the follower leaves: its
/// stream offsets, its stored functions (each cluster defines its own) and
/// the placeholder.
fn own_table(t: &str) -> bool {
    t.eq_ignore_ascii_case(SOURCES) || t.eq_ignore_ascii_case(FUNCTIONS) || t.eq_ignore_ascii_case(chloedb::schema::PLACEHOLDER_TABLE)
}

/// A transaction line of a feed as the follower orders it.
fn txn_of(j: &Json) -> Txn {
    let seq = j["seq"].as_u64().unwrap_or(0);
    let term = j["term"].as_u64().unwrap_or(0);
    let ops: Vec<Json> = j["ops"].as_array().cloned().unwrap_or_default().into_iter().filter(|o| !o["table"].as_str().is_some_and(own_table)).collect();
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
    t.hold = j["hold"].as_bool();
    t
}

/// Reads a source shard's feed from `lines`, or from `path` once
/// connected, and sends what it reads on `tx`.
async fn read(src: Http, path: String, lines: Option<Lines>, tx: tokio::sync::mpsc::Sender<Event>, notify: Arc<tokio::sync::Notify>) {
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
                send(Event::End { why: e.message, again: e.status >= 500 || e.status == 409 || e.status == 429, gone: false }).await;
                return;
            }
        },
    };
    loop {
        let j = match lines.next().await {
            None => {
                send(Event::End { why: "the feed ended".into(), again: true, gone: false }).await;
                return;
            }
            Some(Err(e)) => {
                send(Event::End { why: e, again: true, gone: false }).await;
                return;
            }
            Some(Ok(j)) => j,
        };
        let e = if j.get("seq").is_some() {
            Event::Txn(txn_of(&j))
        } else if let Some(last) = j.get("last").and_then(Json::as_u64) {
            Event::Beat(last)
        } else if j.get("feed").is_some() {
            continue;
        } else if let Some(g) = j.get("gone") {
            let oldest = g["oldest"].as_u64().unwrap_or(0);
            Event::End { why: format!("the source's log no longer holds the transactions the follower needs (it starts at {oldest})"), again: false, gone: true }
        } else if let Some(d) = j.get("diverged") {
            Event::End { why: format!("the source no longer holds the last transaction the follower applied (its last is {} of term {}): a failover lost it; copy again (--copy)", d["seq"], d["term"]), again: false, gone: false }
        } else if let Some(e) = j.get("error") {
            let status = j["status"].as_u64().unwrap_or(500);
            Event::End { why: e.as_str().unwrap_or_default().to_string(), again: status >= 500 || status == 409 || status == 429, gone: false }
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
    /// How far its feed reached, and when it said so: those past what the
    /// follower applied, the first of which says how far behind it is.
    beats: std::collections::VecDeque<(u64, Instant)>,
    /// When the copy it reads on from began: its first beat's time, for
    /// the transactions written while it copied.
    copy_began: Option<Instant>,
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
    /// The copy is of the rows changed since what the follower applied.
    catching: bool,
    /// A shard's feed no longer reaches what the follower applied: it
    /// catches up ([`Self::catch_up`]).
    gone: Option<String>,
    /// While it copies, since when it is behind: when the copy began, or
    /// for a catch-up when it first fell behind.
    behind_at: Option<Instant>,
    /// The keys its last copy deleted, which the source no longer has.
    swept: u64,
    /// It stops because the follower stopped following.
    stopped: bool,
    /// Per source shard, whether it holds its clients' writes as of the
    /// last transaction applied ([`crate::service::feed::HOLD`]).
    holds: BTreeMap<usize, bool>,
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
        catching: false,
        gone: None,
        behind_at: None,
        swept: 0,
        stopped: false,
        holds: BTreeMap::new(),
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
    let kept = f.kept().await?;
    let mine = f.marks_here().await?;
    if let Some(ms) = f.o.once {
        if kept.copy_ms.is_some_and(|c| c >= ms) {
            f.o.copy = false;
        }
        // Stopped (an even mark), and so promoted too, since it was asked.
        let stopped = mine.follows.get(&f.o.name).filter(|&&o| o % 2 == 0).map(|&o| o / 2);
        if let Some(at) = stopped.into_iter().chain(mine.promoted.get(&f.o.name).copied()).filter(|&t| t >= ms as i64).max() {
            return f.idle(format!("this cluster stopped following {} at {}, since this process was asked for (at {}): it does not follow it again", f.o.name, chloedb::timestamp::format(at.saturating_mul(1000)), chloedb::timestamp::format((ms as i64).saturating_mul(1000)))).await;
        }
    }
    // A cluster promoted from the source takes its own writes: only a copy
    // begun since follows the source again (its shards refuse the rest),
    // a new one or the one it kept.
    let since = |at: i64| kept.copy_ms.is_some_and(|c| c as i64 > at);
    if let Some(&at) = mine.promoted.get(&f.o.name).filter(|&&at| !f.o.copy && !f.o.reverse && !since(at)) {
        return Err(ApiError {
            code: None, status: 409,
            message: format!("this cluster was promoted from {} (at {}): it takes its own writes; to follow {} again, copy it afresh (--copy)", f.o.name, chloedb::timestamp::format(at.saturating_mul(1000)), f.o.name),
        });
    }
    let n = shards.unwrap_or(1);
    let reverse = match f.o.reverse {
        true => f.reverse_from(&kept).await?,
        false => None,
    };
    let resume = kept.copied && !f.o.copy && reverse.is_none();
    if f.o.reverse && resume {
        tracing::info!(source = %f.o.source, "this cluster follows the source back already, from its promotion: resuming");
    }
    // A copy's mark goes with the first write, which a cluster promoted
    // from the source takes only from a copy begun since.
    f.copy_ms = match kept.copy_ms {
        Some(c) if resume => c,
        c => crate::telemetry::now_ms().max(c.map_or(0, |c| c + 1)),
    };
    // From here on the follower takes no other client's writes; a hold of
    // them (a switch-over's) ends, as it follows.
    let mut first = Vec::new();
    if mine.holds().is_some() {
        first.push(json!({"op": "source", "source": {"name": HOLD, "offset": next_offset(mine.hold, false)}}));
    }
    f.flush(&mut first, true).await?;
    f.load_tables().await?;
    match reverse {
        Some(seqs) => f.reverse(n, &seqs).await?,
        None if resume => f.resume(n, &kept),
        None => {
            // A copy cut short begins again from what it began with: a
            // catch-up from the transactions the follower had applied.
            let from = match f.o.copy {
                true => CopyFrom { made: kept.made(), ..Default::default() },
                false => CopyFrom { since: kept.since.clone(), whole: kept.whole.clone(), made: kept.made() },
            };
            f.copy(n, from).await?;
        }
    }
    let r = f.follow().await;
    if let Err(e) = &r {
        f.stopped = e.message.contains("stopped following") || e.message.contains("was promoted from");
        if f.stopped && f.o.once.is_some() {
            return f.idle(e.message.clone()).await;
        }
        f.error = Some(e.message.clone());
        f.status(true).await;
    }
    r
}

/// Calls `f` until it answers or fails for good (a 4xx but 408, 409 and
/// 429), waiting longer each time, up to 2 minutes in all.
pub(crate) async fn retry<F, Fut>(what: &str, f: F) -> Result<Json, ApiError>
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
            Err(e) => return Err(ApiError { code: None, status: e.status, message: format!("{what}: {}", e.message) }),
        }
    }
}

/// Whether a failed call may answer when tried again.
pub(crate) fn again(e: &ApiError) -> bool {
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
    /// The copy is a catch-up: per shard, from the transaction (and its
    /// term) the follower had applied.
    since: BTreeMap<usize, (u64, u64)>,
    /// The tables the follower made from the source.
    made: BTreeSet<String>,
    /// The tables a catch-up copies every row of.
    whole: BTreeSet<String>,
    /// Per source shard, the transaction at which it last held its
    /// clients' writes, and the one at which it last ended a hold.
    held: BTreeMap<usize, u64>,
    released: BTreeMap<usize, u64>,
}

/// What a copy is from ([`Follow::copy`]).
#[derive(Debug, Default)]
struct CopyFrom {
    /// Per source shard, the transaction the follower applied (and its
    /// term): a catch-up copies the rows changed after it. None: a copy of
    /// every row.
    since: BTreeMap<usize, (u64, u64)>,
    /// The tables a catch-up copies every row of all the same: those that
    /// changed meanwhile.
    whole: BTreeSet<String>,
    /// The tables the follower made from the source before: it drops one
    /// the source no longer has, and makes one again that changed.
    made: BTreeSet<String>,
}

impl Kept {
    /// The tables the follower holds of the source's: those it made, and
    /// those it wrote or changed.
    fn made(&self) -> BTreeSet<String> {
        let mut m = self.made.clone();
        m.extend(self.tables.iter().map(|(_, t, _)| t.clone()));
        m.extend(self.changes.keys().cloned());
        m
    }
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
                [i, "since"] => {
                    if let Ok(i) = i.parse::<usize>() {
                        k.since.entry(i).or_default().0 = off;
                    }
                }
                [i, "since_term"] => {
                    if let Ok(i) = i.parse::<usize>() {
                        k.since.entry(i).or_default().1 = off;
                    }
                }
                ["made", table] => {
                    k.made.insert(table.to_string());
                }
                [i, "held"] => {
                    if let Ok(i) = i.parse() {
                        k.held.insert(i, off);
                    }
                }
                [i, "released"] => {
                    if let Ok(i) = i.parse() {
                        k.released.insert(i, off);
                    }
                }
                ["whole", table] => {
                    k.whole.insert(table.to_string());
                }
                ["copied"] => k.copied = true,
                _ => {}
            }
        }
        Ok(k)
    }

    /// The follower's own marks ([`Marks`]): of each, the greatest over its
    /// shards.
    async fn marks_here(&self) -> Result<Marks, ApiError> {
        let j = retry("the follower's stream sources", || self.dst.get("/v1/sources?shards=true")).await?;
        let mut all: Vec<(&str, i64)> = j["shards"].as_array().into_iter().flatten().flat_map(|s| s.as_array().into_iter().flatten()).filter_map(|s| Some((s["name"].as_str()?, s["offset"].as_i64()?))).collect();
        all.sort_by_key(|(_, o)| *o);
        Ok(Marks::of(all))
    }

    /// Says why it does not follow (`why`), and waits until it is stopped:
    /// a process [`Options::once`] that its Deployment would start again.
    async fn idle(&mut self, why: String) -> Result<(), ApiError> {
        tracing::warn!(name = %self.o.name, "{why}; waiting to be stopped");
        self.stopped = true;
        self.error = Some(format!("{why}; waiting to be stopped"));
        loop {
            self.status(true).await;
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    }

    /// The source's promotion from this cluster ([`Options::reverse`]):
    /// per source shard, the transaction that ended following, from which
    /// this cluster follows it. Refused (409) unless this cluster holds
    /// what the source applied of it and nothing since: each of its
    /// shards' feeds from there carries no rows. None when this cluster
    /// follows the source back already (`kept`: a copy begun since the
    /// promotion, done), which it resumes.
    async fn reverse_from(&self, kept: &Kept) -> Result<Option<Vec<u64>>, ApiError> {
        let src = &self.o.source;
        let j = retry("the source's promotions", || self.src.get("/v1/feed/promoted")).await?;
        let Some(p) = j["promotions"].as_array().and_then(|a| a.first()).cloned() else {
            return Err(ApiError { code: None, status: 409, message: format!("{src} was never promoted from a cluster it followed: follow it with a copy (without --reverse)") });
        };
        if kept.copied && p["at_ms"].as_u64().is_some_and(|at| kept.copy_ms.is_some_and(|c| c > at)) {
            return Ok(None);
        }
        let seqs: Vec<u64> = p["seqs"].as_array().into_iter().flatten().filter_map(Json::as_u64).collect();
        let applied: Vec<(usize, u64, u64)> = p["applied"].as_array().into_iter().flatten().filter_map(|a| Some((a["shard"].as_u64()? as usize, a["seq"].as_u64()?, a["term"].as_u64().unwrap_or(0)))).collect();
        let health = retry("the follower's health", || self.dst.get("/v1/health")).await?;
        let mine = health.get("shards").and_then(Json::as_array).map(Vec::len);
        let n = mine.unwrap_or(1);
        let again = |why: String| ApiError { code: None, status: 409, message: format!("{why}: this cluster can follow {src} only with a copy (--copy, which writes its rows over these and deletes the keys it lacks)") };
        if seqs.is_empty() || applied.len() != n || applied.iter().enumerate().any(|(i, a)| a.0 != i) {
            return Err(again(format!("{src} was promoted from {} after applying {} shards of it, and this cluster has {n}", p["promoted"], applied.len())));
        }
        for &(i, seq, term) in &applied {
            let mut path = format!("/v1/feed?from={}", seq + 1);
            if mine.is_some() {
                path.push_str(&format!("&shard={i}"));
            }
            if term > 0 {
                path.push_str(&format!("&after_term={term}"));
            }
            let check = async {
                let mut l = self.dst.lines(&path).await?;
                let mut last = None;
                while let Some(line) = l.next().await {
                    let line = line.map_err(|e| ApiError { code: None, status: 503, message: format!("this cluster's shard {i}'s feed: {e}") })?;
                    if let Some(h) = line.get("feed") {
                        last = h["last"].as_u64();
                    } else if line.get("gone").is_some() || line.get("diverged").is_some() {
                        return Err(again(format!("this cluster's shard {i} no longer holds transaction {seq} as {src} applied it, or holds many after it")));
                    } else if let Some(e) = line.get("error") {
                        return Err(ApiError { code: None, status: line["status"].as_u64().unwrap_or(500) as u16, message: format!("this cluster's shard {i}'s feed: {}", e.as_str().unwrap_or_default()) });
                    } else if let Some(t) = line["seq"].as_u64() {
                        let rows = line["ops"].as_array().is_some_and(|o| !o.is_empty()) || line.get("sources").is_some();
                        if rows {
                            return Err(again(format!("this cluster's shard {i} took writes after {src} applied it (transaction {t}, after {seq}): {src} never saw them")));
                        }
                        if last.is_some_and(|l| t >= l) {
                            return Ok(());
                        }
                    }
                    if last.is_some_and(|l| l <= seq) {
                        return Ok(());
                    }
                }
                Err(ApiError { code: None, status: 503, message: format!("this cluster's shard {i}'s feed ended before its last transaction") })
            };
            tokio::time::timeout(Duration::from_secs(30), check).await.map_err(|_| ApiError { code: None, status: 503, message: format!("this cluster's shard {i}'s feed did not reach its last transaction in 30 s") })??;
        }
        tracing::info!(source = %src, from = %p["promoted"], "this cluster holds what the source applied of it: following it from its promotion");
        Ok(Some(seqs))
    }

    /// Follows the source from its promotion's numbers `seqs` (each of its
    /// shards' transaction that ended following this cluster), without a
    /// copy: this cluster's tables are the source's.
    async fn reverse(&mut self, n: usize, seqs: &[u64]) -> Result<(), ApiError> {
        let made: BTreeSet<String> = self.tables.values().filter(|s| !own_table(&s.table)).map(|s| s.table.clone()).collect();
        let k = Kept { copy_ms: Some(self.copy_ms), copied: true, at: seqs.iter().enumerate().map(|(j, s)| (j, (*s, 0))).collect(), made: made.clone(), ..Default::default() };
        self.resume(n, &k);
        let mut m: Vec<Json> = made.iter().map(|t| self.mark(&format!("made/{t}"), 1)).collect();
        // Where each reader begins, so that a restart before its first
        // transaction begins there too.
        m.extend(seqs.iter().enumerate().map(|(j, s)| self.mark(&format!("{j}/at"), *s as i64)));
        m.push(self.mark("copied", 1));
        self.flush(&mut m, true).await?;
        self.status(true).await;
        Ok(())
    }

    /// The path of shard `i`'s feed from transaction `from` (after
    /// `after`'s term).
    fn feed_path(&self, i: usize, from: u64, after: u64) -> String {
        let mut p = format!("/v1/feed?from={from}");
        if self.cluster {
            p.push_str(&format!("&shard={i}"));
        }
        if after > 0 {
            p.push_str(&format!("&after_term={after}"));
        }
        p
    }

    /// The path of shard `i`'s copy: of the rows changed after `since`
    /// (of its term), but every row of the tables `whole`, with the
    /// digests of its keys.
    fn copy_path(&self, i: usize, since: (u64, u64), whole: &BTreeSet<String>) -> String {
        let mut p = "/v1/feed?copy=true&digests=true".to_string();
        if self.cluster {
            p.push_str(&format!("&shard={i}"));
        }
        if since.0 > 0 {
            p.push_str(&format!("&since={}&after_term={}", since.0, since.1));
        }
        if !whole.is_empty() {
            let w: Vec<String> = whole.iter().map(|t| url_part(t)).collect();
            p.push_str(&format!("&whole={}", w.join(",")));
        }
        p
    }

    /// Starts reading shard `i` from transaction `from` (after `after`'s
    /// term), or with `lines` already open.
    fn start(&mut self, i: usize, from: u64, after: u64, lines: Option<Lines>) {
        if self.readers.len() <= i {
            self.readers.resize_with(i + 1, Reader::default);
        }
        let (tx, rx) = tokio::sync::mpsc::channel(256);
        let path = self.feed_path(i, from, after);
        tokio::spawn(read(self.src.clone(), path, lines, tx, self.notify.clone()));
        let r = &mut self.readers[i];
        r.rx = Some(rx);
        r.ended = None;
        r.again_at = None;
    }

    /// Copies every shard of the source: once each serves the same slot
    /// map, tables and rank profiles, makes the tables here as the
    /// source's, writes the rows, deletes the keys the source does not
    /// have ([`Self::sweep`]), and reads on from there. With
    /// [`CopyFrom::since`], a catch-up: of the rows changed since what the
    /// follower applied, but of every row of a table that changed
    /// meanwhile. What it copies from is kept first, so that one cut
    /// short begins again from the same.
    async fn copy(&mut self, n: usize, from: CopyFrom) -> Result<(), ApiError> {
        let CopyFrom { since, mut whole, made } = from;
        let began = Instant::now();
        self.behind_at = Some(self.lag_from().map_or(began, |t| t.min(began)));
        self.copying = true;
        self.catching = since.values().any(|s| s.0 > 0);
        self.readers.clear();
        self.order = Order::new(n).with_batch(self.o.batch);
        // What it copies from, so that a copy cut short begins again so.
        let mut marks: Vec<Json> = Vec::new();
        for (i, (seq, term)) in since.iter().filter(|(_, s)| s.0 > 0) {
            marks.push(self.mark(&format!("{i}/since"), *seq as i64));
            marks.push(self.mark(&format!("{i}/since_term"), *term as i64));
        }
        marks.extend(made.iter().map(|t| self.mark(&format!("made/{t}"), 1)));
        marks.extend(whole.iter().map(|t| self.mark(&format!("whole/{t}"), 1)));
        self.flush(&mut marks, true).await?;
        self.status(true).await;
        let mut tries = 0;
        let (heads, mut lines) = loop {
            tries += 1;
            let mut heads = Vec::with_capacity(n);
            let mut lines = Vec::with_capacity(n);
            for i in 0..n {
                let path = self.copy_path(i, since.get(&i).copied().unwrap_or_default(), &whole);
                let mut l = self.src.lines(&path).await.map_err(|e| ApiError { code: None, status: e.status, message: format!("the copy of shard {i}: {}", e.message) })?;
                let head = match l.next().await {
                    Some(Ok(j)) => j,
                    Some(Err(e)) => return Err(ApiError { code: None, status: 503, message: format!("the copy of shard {i}: {e}") }),
                    None => return Err(ApiError { code: None, status: 503, message: format!("the copy of shard {i} ended at once") }),
                };
                if let Some(e) = head.get("error") {
                    return Err(ApiError { code: None, status: head["status"].as_u64().unwrap_or(500) as u16, message: format!("the copy of shard {i}: {}", e.as_str().unwrap_or_default()) });
                }
                let Some(h) = head.get("copy").filter(|c| c.is_object()).cloned() else {
                    return Err(ApiError::internal(format!("the copy of shard {i} began with {head}, not its version: the source needs shard protocol {}", crate::grpc::FEED_KEYS_PROTOCOL)));
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
                // A catch-up copies every row of a table that is not
                // here as the source has it.
                let tables: Vec<Schema> = heads[0]["tables"].as_array().into_iter().flatten().map(schema_of).collect::<Result<_, _>>()?;
                let changed: BTreeSet<String> = match self.catching {
                    true => tables.iter().filter(|s| !own_table(&s.table) && self.tables.get(&s.table.to_ascii_lowercase()).is_none_or(|h| token(h) != token(s))).map(|s| s.table.clone()).collect(),
                    false => BTreeSet::new(),
                };
                if changed.is_subset(&whole) {
                    break (heads, lines);
                }
                // Kept before it makes them, as a catch-up cut short
                // finds them as the source's.
                tracing::info!(tables = ?changed, "tables changed since the follower fell behind: copying all their rows");
                let mut m: Vec<Json> = changed.iter().map(|t| self.mark(&format!("whole/{t}"), 1)).collect();
                self.flush(&mut m, false).await?;
                whole.extend(changed);
                tries -= 1;
            } else if tries >= COPY_TRIES {
                return Err(ApiError { code: None, status: 503, message: format!("the source's shards served different slot maps or tables in {COPY_TRIES} tries of a copy: a move or a change of tables was under way; start again") });
            } else {
                tracing::info!(tries, "the source's shards differ (a slot move or a change of tables): copying again");
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            drop(lines);
        };
        let tables: Vec<Schema> = heads[0]["tables"].as_array().into_iter().flatten().map(schema_of).collect::<Result<_, _>>()?;
        let profiles = heads[0]["profiles"].as_array().cloned().unwrap_or_default();
        self.make_tables(&tables, &profiles, &made).await?;
        let mut m: Vec<Json> = tables.iter().filter(|s| !own_table(&s.table)).map(|s| self.mark(&format!("made/{}", s.table), 1)).collect();
        self.flush(&mut m, false).await?;
        tracing::info!(shards = n, tables = tables.len(), copy_ms = self.copy_ms, catching_up = self.catching, "copying the source");
        // The rows, a shard after another.
        let mut digests: BTreeMap<usize, Digest> = BTreeMap::new();
        let mut ops: Vec<Json> = Vec::new();
        let mut at = Vec::with_capacity(n);
        for (i, l) in lines.iter_mut().enumerate() {
            loop {
                let j = match l.next().await {
                    Some(Ok(j)) => j,
                    Some(Err(e)) => return Err(ApiError { code: None, status: 503, message: format!("the copy of shard {i}: {e}") }),
                    None => return Err(ApiError { code: None, status: 503, message: format!("the copy of shard {i} ended before its rows did") }),
                };
                if j.get("copy") == Some(&Json::Bool(true)) {
                    let rows = j["ops"].as_array().into_iter().flatten().filter(|o| !o["table"].as_str().is_some_and(own_table));
                    for op in rows {
                        ops.push(op.clone());
                        self.rows_copied += 1;
                    }
                    if ops.len() >= self.o.batch {
                        let marks = self.wrote_at.elapsed() >= STATUS_EVERY;
                        self.flush(&mut ops, marks).await?;
                        self.status(false).await;
                    }
                } else if let Some(c) = j.get("copied") {
                    for (slot, (k, h)) in digests_of(&c["digests"]).map_err(|e| ApiError { code: None, status: 502, message: format!("the copy of shard {i}: {}: the source needs shard protocol {}", e.message, crate::grpc::FEED_KEYS_PROTOCOL) })? {
                        let d = digests.entry(slot).or_default();
                        d.0 += k;
                        d.1 = d.1.wrapping_add(h);
                    }
                    // The source's stream sources, as it holds them.
                    for s in c["sources"].as_array().into_iter().flatten().filter(|s| s["name"].as_str().is_some_and(|n| !n.starts_with(crate::service::MARK))) {
                        ops.push(json!({"op": "source", "source": {"name": s["name"], "offset": s["offset"]}}));
                    }
                    let seq = c["seq"].as_u64().unwrap_or(0);
                    // The shard held its clients' writes at the version.
                    let holds = c["hold"] == true;
                    if holds {
                        ops.push(self.mark(&format!("{i}/held"), seq as i64));
                    }
                    self.holds.insert(i, holds);
                    at.push((seq, c["term"].as_u64().unwrap_or(0)));
                    break;
                } else if let Some(e) = j.get("error") {
                    return Err(ApiError { code: None, status: j["status"].as_u64().unwrap_or(500) as u16, message: format!("the copy of shard {i}: {}", e.as_str().unwrap_or_default()) });
                }
            }
        }
        self.flush(&mut ops, false).await?;
        self.swept = self.sweep(&tables, &digests).await?;
        // It follows from the copy's versions.
        let versions: Vec<(String, Option<String>, u64)> = tables.iter().map(|s| (s.table.clone(), Some(token(s)), 0)).collect();
        self.order = Order::resume(&at, Vec::new(), Vec::new()).with_batch(self.o.batch).with_versions(versions);
        for (i, l) in lines.into_iter().enumerate() {
            self.start(i, at[i].0 + 1, at[i].1, Some(l));
            self.readers[i].copy_began = self.behind_at;
        }
        let mut m = vec![self.mark("copied", 1)];
        self.flush(&mut m, true).await?;
        self.copying = false;
        self.behind_at = None;
        self.status(true).await;
        tracing::info!(rows = self.rows_copied, deleted = self.swept, "copied the source: following it");
        Ok(())
    }

    /// Catches up after a shard's feed no longer reaches what the follower
    /// applied: copies again, of the rows each shard changed since.
    async fn catch_up(&mut self) -> Result<(), ApiError> {
        let since: BTreeMap<usize, (u64, u64)> = (0..self.order.shards()).map(|i| (i, self.order.at(i))).collect();
        let made = self.kept().await?.made();
        let health = retry("the source's health", || self.src.get("/v1/health")).await?;
        let n = health.get("shards").and_then(Json::as_array).map_or(1, Vec::len).max(self.order.shards());
        self.copy_ms = crate::telemetry::now_ms().max(self.copy_ms + 1);
        self.copy(n, CopyFrom { since, whole: BTreeSet::new(), made }).await
    }

    /// Makes the source's `tables` here, with their rank `profiles`: a
    /// table it lacks, and one of `made` (the follower made it from the
    /// source) that is not as the source has it now, which it drops
    /// first; and drops those of `made` the source no longer has. A table
    /// here of another schema that it did not make stops it.
    async fn make_tables(&mut self, tables: &[Schema], profiles: &[Json], made: &BTreeSet<String>) -> Result<(), ApiError> {
        let lower: BTreeSet<String> = tables.iter().map(|s| s.table.to_ascii_lowercase()).collect();
        for t in made {
            if !lower.contains(&t.to_ascii_lowercase()) && self.tables.contains_key(&t.to_ascii_lowercase()) && !own_table(t) {
                tracing::info!(table = %t, "the source dropped a table meanwhile: dropping it");
                self.sql(&format!("DROP TABLE IF EXISTS {}", ident(t))).await?;
            }
        }
        let made: BTreeSet<String> = made.iter().map(|t| t.to_ascii_lowercase()).collect();
        for s in tables {
            if own_table(&s.table) {
                continue;
            }
            match self.tables.get(&s.table.to_ascii_lowercase()) {
                Some(h) if token(h) != token(s) && made.contains(&s.table.to_ascii_lowercase()) => {
                    tracing::info!(table = %s.table, "the source changed a table meanwhile: making it again");
                    self.sql(&format!("DROP TABLE IF EXISTS {}", ident(&s.table))).await?;
                }
                _ => {}
            }
        }
        self.load_tables().await?;
        for s in tables {
            self.ensure_table(s).await?;
        }
        // Rank profiles as the source's: those it lacks dropped.
        let have: Vec<(String, String)> = {
            let j = retry("the follower's tables", || self.dst.get("/v1/tables")).await?;
            j["tables"].as_array().into_iter().flatten().filter(|t| t["table"].as_str().is_some_and(|n| lower.contains(&n.to_ascii_lowercase()))).flat_map(|t| {
                let table = t["table"].as_str().unwrap_or_default().to_string();
                t["profiles"].as_array().into_iter().flatten().filter_map(move |p| Some((table.clone(), p["name"].as_str()?.to_string()))).collect::<Vec<_>>()
            }).collect()
        };
        let theirs: HashSet<(String, String)> = profiles.iter().filter_map(|p| Some((p["table"].as_str()?.to_ascii_lowercase(), p["name"].as_str()?.to_string()))).collect();
        let mut changes: Vec<Json> = have.into_iter().filter(|(t, n)| !theirs.contains(&(t.to_ascii_lowercase(), n.clone()))).map(|(t, n)| json!({"op": "profile", "table": t, "name": n, "def": null})).collect();
        changes.extend(profiles.iter().cloned());
        if !changes.is_empty() {
            self.apply_change(&changes).await?;
        }
        Ok(())
    }

    /// Deletes the keys the follower holds of `tables` that the source does
    /// not: the follower's digests of its keys by slot are compared with
    /// the source's (`source`, of its copy's version), and of the slots
    /// whose differ, the keys the source holds now are read; a key the
    /// source no longer holds here, which the transactions after the copy
    /// do not give again. Returns how many it deleted.
    async fn sweep(&mut self, tables: &[Schema], source: &BTreeMap<usize, Digest>) -> Result<u64, ApiError> {
        let names: Vec<String> = tables.iter().filter(|s| !own_table(&s.table)).map(|s| s.table.clone()).collect();
        let by: HashMap<String, bool> = tables.iter().map(|s| (s.table.to_ascii_lowercase(), s.shard_by.is_some())).collect();
        let body = json!({"tables": names});
        let mine = retry("the follower's keys", || self.dst.post("/v1/feed/keys", &body)).await?;
        let mine = digests_of(&mine["digests"])?;
        let differ: Vec<usize> = (0..crate::slots::SLOTS).filter(|s| source.get(s) != mine.get(s)).collect();
        if differ.is_empty() {
            return Ok(0);
        }
        tracing::info!(slots = differ.len(), "the follower's keys differ from the source's: comparing them");
        let mut deleted = 0;
        for chunk in differ.chunks(SWEEP_SLOTS) {
            let body = json!({"tables": names, "slots": chunk});
            let theirs = retry("the source's keys", || self.src.post("/v1/feed/keys", &body)).await?;
            // A row of a table placed by a column, by its key and that
            // column's value: one the follower holds under another value
            // too is deleted there.
            let id = |k: &Json| (k["table"].as_str().unwrap_or_default().to_ascii_lowercase(), k["key"].to_string(), k["place"].to_string());
            let theirs: HashSet<(String, String, String)> = theirs["keys"].as_array().into_iter().flatten().map(id).collect();
            let ours = retry("the follower's keys", || self.dst.post("/v1/feed/keys", &body)).await?;
            let mut ops: Vec<Json> = Vec::new();
            for k in ours["keys"].as_array().into_iter().flatten() {
                let table = k["table"].as_str().unwrap_or_default();
                if theirs.contains(&id(k)) {
                    continue;
                }
                let mut op = json!({"op": "delete", "table": table, "key": k["key"]});
                if by.get(&table.to_ascii_lowercase()) == Some(&true) {
                    op["routing"] = k["place"].clone();
                }
                ops.push(op);
                if ops.len() >= self.o.batch {
                    deleted += ops.len() as u64;
                    self.flush(&mut ops, false).await?;
                }
            }
            deleted += ops.len() as u64;
            self.flush(&mut ops, false).await?;
        }
        Ok(deleted)
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
        self.holds = (0..known).map(|i| (i, k.held.get(&i).is_some_and(|h| *h > k.released.get(&i).copied().unwrap_or(0)))).collect();
        for i in 0..known {
            let (seq, term) = self.order.at(i);
            self.start(i, seq + 1, term, None);
        }
        tracing::info!(shards = known, "following the source from where the follower was");
    }

    /// Follows until it fails for good.
    async fn follow(&mut self) -> Result<(), ApiError> {
        loop {
            self.take_events()?;
            if let Some(why) = self.gone.take() {
                tracing::warn!("{why}: catching up with the rows changed since");
                self.catch_up().await?;
                continue;
            }
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
                    self.start(j, 1, 0, None);
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
                    Ok(Event::Txn(t)) => self.order.offer(i, t),
                    Ok(Event::Beat(last)) => {
                        let at = self.order.at(i).0;
                        let r = &mut self.readers[i];
                        r.last = last;
                        let when = r.copy_began.take().unwrap_or_else(Instant::now);
                        if last > at && r.beats.back().is_none_or(|b| last > b.0) {
                            r.beats.push_back((last, when));
                        }
                    }
                    Ok(Event::End { why, again, gone }) => {
                        if gone {
                            // Caught up once every shard's reader is in
                            // hand ([`Self::catch_up`]).
                            tracing::warn!(shard = i, "{why}: catching up");
                            self.gone = Some(format!("shard {i}: {why}"));
                            let r = &mut self.readers[i];
                            r.rx = None;
                            r.ended = Some(why);
                            continue;
                        }
                        if !again {
                            return Err(ApiError { code: None, status: 503, message: format!("shard {i}'s feed: {why}") });
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
                self.start(i, seq + 1, term, None);
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
                        ops.push(self.mark(&format!("made/{table}"), 1));
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
            // The source shard holds its clients' writes from here on, or
            // no longer: with what the follower applied, what a promotion
            // looks for ([`crate::service::Service::promote_follower`]).
            match t.hold {
                Some(true) => ops.push(self.mark(&format!("{i}/held"), t.seq as i64)),
                Some(false) => ops.push(self.mark(&format!("{i}/released"), t.seq as i64)),
                None => {}
            }
            if let Some(h) = t.hold {
                self.holds.insert(*i, h);
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
                Err(e) => return Err(ApiError { code: None, status: e.status, message: format!("writing to the follower: {}", e.message) }),
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
                    code: None, status: 409,
                    message: format!("table {} here is not the source's:\n{}\nwhere the source has\n{}\nDrop it here, or follow into a cluster without it", s.table, create_sql(have), create_sql(s)),
                })
            }
            None => {}
        }
        let made = self.sql(&create_sql(s)).await;
        self.load_tables().await?;
        // A CREATE TABLE tried again after its answer was lost (a 5xx, a
        // coordinator that stopped as it answered) finds the table its first
        // try made: made as the source has it, it is done.
        match made {
            Err(e) if !self.tables.get(&s.table.to_ascii_lowercase()).is_some_and(|h| token(h) == token(s)) => Err(e),
            Err(e) => {
                tracing::info!(table = %s.table, "making the table failed ({}), but it is here as the source has it: an earlier try made it", e.message);
                Ok(())
            }
            Ok(_) => Ok(()),
        }
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
                                if let Err(e) = self.sql(&sql).await {
                                    // Tried again after an answer lost, an
                                    // earlier try may have changed it.
                                    self.load_tables().await?;
                                    if !self.tables.get(&table.to_ascii_lowercase()).is_some_and(|h| token(h) == token(&now)) {
                                        return Err(e);
                                    }
                                    tracing::info!(table, "changing the table failed ({}), but it is here as the source has it: an earlier try changed it", e.message);
                                }
                            }
                        }
                        _ => return Err(ApiError { code: None, status: 409, message: format!("table {table} here is neither as the source had it before its change nor after it") }),
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
    /// Since when the follower is behind the source: when the source
    /// first reached a transaction it has not applied (its feed's beats
    /// say how far it reaches, every second), or while it copies, when it
    /// fell behind.
    fn lag_from(&mut self) -> Option<Instant> {
        if self.copying {
            return self.behind_at;
        }
        let mut from: Option<Instant> = None;
        for (i, r) in self.readers.iter_mut().enumerate() {
            let at = self.order.at(i).0;
            while r.beats.front().is_some_and(|b| b.0 <= at) {
                r.beats.pop_front();
            }
            if let Some(&(_, t)) = r.beats.front() {
                from = Some(from.map_or(t, |f| f.min(t)));
            }
        }
        from
    }

    async fn status(&mut self, now: bool) {
        if !now && self.status_at.is_some_and(|t| t.elapsed() < STATUS_EVERY) {
            return;
        }
        let lag_ms = self.lag_from().map_or(0, |t| t.elapsed().as_millis() as u64);
        self.status_at = Some(Instant::now());
        let shards: Vec<Json> = (0..self.order.shards())
            .map(|i| {
                let (at, term) = self.order.at(i);
                let r = self.readers.get(i);
                let last = r.map_or(0, |r| r.last).max(at);
                json!({"shard": i, "applied": at, "term": term, "last": last, "behind": last - at, "queued": self.order.queued(i), "joining": self.order.joining(i), "stopped": r.and_then(|r| r.ended.clone()), "source_holds": self.holds.get(&i) == Some(&true)})
            })
            .collect();
        let state = match () {
            _ if self.stopped => "stopped",
            _ if self.copying && self.catching => "catching up",
            _ if self.copying => "copying",
            _ if self.error.is_some() => "held",
            _ => "following",
        };
        let behind: u64 = shards.iter().filter_map(|s| s["behind"].as_u64()).sum();
        let source_holds = !shards.is_empty() && shards.iter().all(|s| s["source_holds"] == true);
        let body = json!({
            "name": self.o.name, "source": self.o.source, "state": state, "copy_ms": self.copy_ms, "error": self.error, "shards": shards,
            "behind": behind, "lag_ms": lag_ms, "keys_deleted": self.swept, "source_holds": source_holds,
            "rows_copied": self.rows_copied, "written": self.written, "started_ms": self.started_ms, "time_ms": crate::telemetry::now_ms(),
        });
        if let Err(e) = self.dst.post("/v1/admin/follow/status", &body).await {
            tracing::debug!("telling the follower how it stands: {}", e.message);
        }
    }
}


/// What `chloedb-server switch-over` is told.
#[derive(Clone, Debug)]
pub struct SwitchOptions {
    /// The follower's HTTP URL: its coordinator, or a server of its own.
    pub target: String,
    /// The follower's cluster token, which a promotion takes.
    pub target_token: Option<String>,
    pub target_ca: Option<Vec<u8>>,
    /// The name the follower knows the source by.
    pub name: String,
    /// The source's HTTP URL; by default the one its follow process names.
    pub source: Option<String>,
    /// A key of the source that holds its writes (an admin key), or its
    /// token.
    pub source_key: Option<String>,
    pub source_ca: Option<Vec<u8>>,
    /// Promote though the source's writes cannot be held (it is gone),
    /// losing those the follower did not apply.
    pub unplanned: bool,
    /// How long to wait for the follower to apply the source's writes up
    /// to its hold.
    pub wait: Duration,
}

/// Switches over to a follower: holds the source's writes
/// (`POST /v1/admin/hold-writes` there), waits until the follower applied
/// every source shard's up to the hold, and promotes it
/// (`POST /v1/admin/follow/promote`), which then takes its own clients'
/// writes having lost none. A source it cannot hold stops it, saying why,
/// unless [`SwitchOptions::unplanned`]: then it promotes all the same.
/// Answers the promotion.
pub async fn switch_over(o: SwitchOptions) -> Result<Json, ApiError> {
    if !crate::service::follow_name_ok(&o.name) {
        return Err(ApiError::bad("--name is 1 to 64 letters, digits, '.', '_' or '-'"));
    }
    let dst = Http::new(&o.target, o.target_token.as_deref(), o.target_ca.as_deref(), None)?;
    let name = o.name.as_str();
    let cluster = || async {
        let v = dst.get("/v1/admin/follow").await?;
        v["clusters"].as_array().into_iter().flatten().find(|c| c["name"] == name).cloned().ok_or_else(|| ApiError { code: None, status: 404, message: format!("{} never followed {name}", o.target) })
    };
    let c = cluster().await?;
    let mut planned = c["source_holds"] == true || c.get("promoted_ms").is_some();
    if !planned {
        let url = o.source.clone().or_else(|| c["status"]["source"].as_str().map(String::from));
        let held = match &url {
            None => Err(format!("{} does not know where {name} is (its follow process has not said): name it (--source)", o.target)),
            Some(u) => {
                let src = Http::new(u, o.source_key.as_deref(), o.source_ca.as_deref(), None)?;
                src.post("/v1/admin/hold-writes", &json!({"hold": true})).await.map_err(|e| e.message)
            }
        };
        match held {
            Ok(_) => {
                tracing::info!("{name} holds its writes: waiting for {} to apply them", o.target);
                let until = Instant::now() + o.wait;
                loop {
                    let c = cluster().await?;
                    if c["source_holds"] == true {
                        break;
                    }
                    if Instant::now() >= until {
                        return Err(ApiError {
                            code: None, status: 503,
                            message: format!(
                                "{} did not apply the writes of {name} up to its hold in {} s ({} transactions behind, as its follow process last said): {name} still holds its writes; run this again, or end the hold there (POST /v1/admin/hold-writes {{\"hold\": false}})",
                                o.target,
                                o.wait.as_secs(),
                                c["status"]["behind"]
                            ),
                        });
                    }
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
                planned = true;
            }
            Err(why) if !o.unplanned => {
                return Err(ApiError {
                    code: None, status: 409,
                    message: format!(
                        "could not hold the writes of {name}: {why}. Promoting {} now loses the writes {name} took that it has not applied ({} transactions, as its follow process last said); if {name} is gone, switch over with --unplanned",
                        o.target, c["status"]["behind"]
                    ),
                });
            }
            Err(why) => tracing::warn!("could not hold the writes of {name} ({why}): promoting all the same, unplanned"),
        }
    }
    dst.post("/v1/admin/follow/promote", &json!({"name": name, "unplanned": !planned})).await
}
