//! A test Kafka broker: enough of Kafka's wire protocol for the `kafka`
//! connector ([`super::kafka`]) to connect to, in the server's own process
//! or on its own (`chloedb-server kafka-test-broker`).
//!
//! One broker (node 0) keeps its topics in memory: each partition is a
//! list of messages numbered from its log start, the oldest dropped past
//! [`Options::retain`] messages, so a consumer that falls that far behind
//! meets Kafka's `OFFSET_OUT_OF_RANGE` as it would on a real broker. It
//! answers the requests a client such as rskafka makes, at one version
//! each (the versions it lists in `ApiVersions`):
//!
//! - `ApiVersions` (v0 to v3), `Metadata` (v1, making a topic asked for
//!   by name that it lacks, as Kafka's auto.create.topics.enable does),
//!   `ListOffsets` (v1);
//! - `Fetch` (v4), which waits up to the request's `max_wait_ms` for a
//!   message when there is none yet, and answers one uncompressed record
//!   batch (magic 2, CRC-32C) per partition;
//! - `Produce` (v3) of uncompressed record batches, making a topic it
//!   lacks; `CreateTopics` and `DeleteTopics` (v0);
//! - `SaslHandshake` (v1) and `SaslAuthenticate` (v0) with `PLAIN`, when
//!   it is started with a user and password: until a client signs in, it
//!   answers nothing else, and closes the connection.
//!
//! No consumer groups (FindCoordinator, JoinGroup, OffsetCommit), no
//! transactions, no compression, no replication: a client that needs
//! them (librdkafka's group consumer) is told the versions it has and
//! fails; rskafka and franz-go (rpk's) consume from it and produce to it.
//! A generator writes made-up click events into a topic at a rate
//! ([`TestBroker::set_generator`]), and [`TestBroker::produce`] adds
//! messages directly, which `POST /v1/admin/kafka/test-broker/produce`
//! does from the command center.

use std::collections::{BTreeMap, VecDeque};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value as Json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{watch, Notify};

use crate::telemetry::now_ms;

const PRODUCE: i16 = 0;
const FETCH: i16 = 1;
const LIST_OFFSETS: i16 = 2;
const METADATA: i16 = 3;
const SASL_HANDSHAKE: i16 = 17;
const API_VERSIONS: i16 = 18;
const CREATE_TOPICS: i16 = 19;
const DELETE_TOPICS: i16 = 20;
const SASL_AUTHENTICATE: i16 = 36;

/// The versions answered, by API key: (key, least, most).
const VERSIONS: &[(i16, i16, i16)] = &[
    (PRODUCE, 3, 3),
    (FETCH, 4, 4),
    (LIST_OFFSETS, 1, 1),
    (METADATA, 1, 1),
    (SASL_HANDSHAKE, 1, 1),
    (API_VERSIONS, 0, 3),
    (CREATE_TOPICS, 0, 0),
    (DELETE_TOPICS, 0, 0),
    (SASL_AUTHENTICATE, 0, 0),
];

// Kafka's error codes.
const OFFSET_OUT_OF_RANGE: i16 = 1;
const CORRUPT_MESSAGE: i16 = 2;
const UNKNOWN_TOPIC_OR_PARTITION: i16 = 3;
const INVALID_TOPIC: i16 = 17;
const UNSUPPORTED_SASL_MECHANISM: i16 = 33;
const ILLEGAL_SASL_STATE: i16 = 34;
const UNSUPPORTED_VERSION: i16 = 35;
const TOPIC_ALREADY_EXISTS: i16 = 36;
const INVALID_PARTITIONS: i16 = 37;
const SASL_AUTHENTICATION_FAILED: i16 = 58;
const UNSUPPORTED_COMPRESSION_TYPE: i16 = 76;

/// The largest request a client may send (Kafka's default
/// `socket.request.max.bytes`).
const MAX_REQUEST: usize = 100 << 20;
/// The most partitions a topic has here.
pub const MAX_PARTITIONS: i32 = 64;
/// The longest a fetch waits for a message.
const MAX_WAIT: Duration = Duration::from_secs(10);

/// How a test broker starts.
#[derive(Clone, Debug)]
pub struct Options {
    /// Where it listens (port 0: any free one).
    pub listen: SocketAddr,
    /// The topic made at the start, which the generator writes.
    pub topic: String,
    pub partitions: i32,
    /// Made-up events a second written to `topic` (0: none until set).
    pub rate: f64,
    /// A user and password clients sign in with (SASL PLAIN); none: no
    /// sign-in.
    pub sasl: Option<(String, String)>,
    /// The most messages a partition keeps; older ones are dropped, as a
    /// broker's retention does.
    pub retain: usize,
    /// The host it tells clients to connect to (Metadata's broker): by
    /// default the address it listens on, or 127.0.0.1 when that is
    /// 0.0.0.0. Clients on other machines need one they can reach.
    pub advertise: Option<String>,
}

impl Default for Options {
    fn default() -> Options {
        Options { listen: SocketAddr::from(([127, 0, 0, 1], 19092)), topic: "demo.clicks".into(), partitions: 3, rate: 5.0, sasl: None, retain: 200_000, advertise: None }
    }
}

/// One message of a partition.
#[derive(Clone, Debug, Default)]
pub struct Message {
    /// Milliseconds since the epoch.
    pub ts: i64,
    pub key: Option<Vec<u8>>,
    pub value: Option<Vec<u8>>,
    pub headers: Vec<(String, Option<Vec<u8>>)>,
}

impl Message {
    fn size(&self) -> usize {
        24 + self.key.as_ref().map_or(0, Vec::len) + self.value.as_ref().map_or(0, Vec::len) + self.headers.iter().map(|(k, v)| k.len() + v.as_ref().map_or(0, Vec::len) + 4).sum::<usize>()
    }
}

#[derive(Default)]
struct Partition {
    /// The offset of `msgs[0]` (Kafka's log start offset).
    start: i64,
    msgs: VecDeque<Message>,
    bytes: usize,
}

impl Partition {
    /// The offset the next message takes (Kafka's high watermark).
    fn end(&self) -> i64 {
        self.start + self.msgs.len() as i64
    }
}

#[derive(Default)]
struct Topic {
    parts: Vec<Partition>,
    created_ms: u64,
}

/// The generator's settings and what it wrote.
#[derive(Clone, Debug, Default)]
struct Generator {
    topic: String,
    rate: f64,
    /// Events owed to the rate, not yet written.
    owed: f64,
    written: u64,
    seq: u64,
    seed: u64,
}

struct Inner {
    topics: Mutex<BTreeMap<String, Topic>>,
    generator: Mutex<Generator>,
    /// Woken when a message is added, for fetches that wait.
    added: Notify,
    sasl: Option<(String, String)>,
    retain: usize,
    host: String,
    port: u16,
    connections: AtomicU64,
    open: AtomicU64,
    requests: AtomicU64,
    fetches: AtomicU64,
    produced: AtomicU64,
}

/// A running test broker; dropped (or [`Self::stop`]ped), it closes its
/// port and connections.
pub struct TestBroker {
    inner: Arc<Inner>,
    stop: watch::Sender<bool>,
    addr: SocketAddr,
    started_ms: u64,
}

impl Drop for TestBroker {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
    }
}

/// Whether `name` may name a topic: Kafka's rule (1 to 249 letters,
/// digits, `.`, `_` and `-`, not `.` or `..`).
pub fn topic_ok(name: &str) -> bool {
    !name.is_empty() && name.len() <= 249 && name != "." && name != ".." && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

impl TestBroker {
    /// Listens on `o.listen`, makes `o.topic`, and starts the generator.
    pub async fn start(o: Options) -> Result<TestBroker, String> {
        if !topic_ok(&o.topic) {
            return Err(format!("{:?} is not a topic's name: 1 to 249 letters, digits, '.', '_' and '-'", o.topic));
        }
        if !(1..=MAX_PARTITIONS).contains(&o.partitions) {
            return Err(format!("a topic has 1 to {MAX_PARTITIONS} partitions"));
        }
        if !(0.0..=100_000.0).contains(&o.rate) {
            return Err("the rate is 0 to 100,000 events a second".into());
        }
        let l = TcpListener::bind(o.listen).await.map_err(|e| format!("listening on {}: {e}", o.listen))?;
        let addr = l.local_addr().map_err(|e| e.to_string())?;
        let host = match &o.advertise {
            Some(h) if !h.trim().is_empty() => h.trim().trim_matches(['[', ']']).to_string(),
            _ if addr.ip().is_unspecified() => "127.0.0.1".to_string(),
            _ => addr.ip().to_string(),
        };
        let inner = Arc::new(Inner {
            topics: Mutex::new(BTreeMap::new()),
            generator: Mutex::new(Generator { topic: o.topic.clone(), rate: o.rate, seed: now_ms() | 1, ..Default::default() }),
            added: Notify::new(),
            sasl: o.sasl.clone(),
            retain: o.retain.max(1),
            host,
            port: addr.port(),
            connections: AtomicU64::new(0),
            open: AtomicU64::new(0),
            requests: AtomicU64::new(0),
            fetches: AtomicU64::new(0),
            produced: AtomicU64::new(0),
        });
        inner.create(&o.topic, o.partitions).map_err(|e| format!("topic {}: error {e}", o.topic))?;
        let (stop, rx) = watch::channel(false);
        let i = inner.clone();
        let mut stopped = rx.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    a = l.accept() => match a {
                        Ok((s, _)) => {
                            let _ = s.set_nodelay(true);
                            i.connections.fetch_add(1, Ordering::Relaxed);
                            let (i, rx) = (i.clone(), rx.clone());
                            tokio::spawn(async move {
                                i.open.fetch_add(1, Ordering::Relaxed);
                                let _ = serve(&i, s, rx).await;
                                i.open.fetch_sub(1, Ordering::Relaxed);
                            });
                        }
                        Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
                    },
                    _ = stopped.changed() => return,
                }
            }
        });
        // The generator: a tick every 100 ms writes what the rate owes.
        let i = inner.clone();
        let mut stopped = stop.subscribe();
        tokio::spawn(async move {
            let mut t = tokio::time::interval(Duration::from_millis(100));
            t.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = t.tick() => i.generate(0.1),
                    _ = stopped.changed() => return,
                }
            }
        });
        Ok(TestBroker { inner, stop, addr, started_ms: now_ms() })
    }

    /// Where clients connect: `host:port`.
    pub fn address(&self) -> String {
        format!("{}:{}", self.inner.host, self.addr.port())
    }

    pub fn stop(&self) {
        let _ = self.stop.send(true);
    }

    /// Makes topic `name` with `partitions` partitions.
    pub fn create_topic(&self, name: &str, partitions: i32) -> Result<(), String> {
        if !topic_ok(name) {
            return Err(format!("{name:?} is not a topic's name: 1 to 249 letters, digits, '.', '_' and '-'"));
        }
        self.inner.create(name, partitions).map_err(|e| match e {
            TOPIC_ALREADY_EXISTS => format!("topic {name} exists"),
            _ => format!("a topic has 1 to {MAX_PARTITIONS} partitions"),
        })
    }

    /// Writes made-up click events to `topic` (made with three partitions
    /// if it does not exist) at `rate` a second (0: none).
    pub fn set_generator(&self, topic: &str, rate: f64) -> Result<(), String> {
        if !topic_ok(topic) {
            return Err(format!("{topic:?} is not a topic's name"));
        }
        if !(0.0..=100_000.0).contains(&rate) {
            return Err("the rate is 0 to 100,000 events a second".into());
        }
        if !self.inner.has(topic) {
            let _ = self.inner.create(topic, 3);
        }
        let mut g = self.inner.generator.lock().unwrap_or_else(|e| e.into_inner());
        g.topic = topic.to_string();
        g.rate = rate;
        g.owed = 0.0;
        Ok(())
    }

    /// Adds `msgs` to `topic` (made with three partitions if missing): to
    /// `partition`, or each to the partition its key hashes to (no key:
    /// in turn). The offsets they took, as (partition, offset).
    pub fn produce(&self, topic: &str, partition: Option<i32>, msgs: Vec<Message>) -> Result<Vec<(i32, i64)>, String> {
        if !topic_ok(topic) {
            return Err(format!("{topic:?} is not a topic's name"));
        }
        if !self.inner.has(topic) {
            let _ = self.inner.create(topic, 3);
        }
        let mut out = Vec::with_capacity(msgs.len());
        for m in msgs {
            let n = self.inner.partitions(topic);
            let p = match partition {
                Some(p) if (0..n).contains(&p) => p,
                Some(p) => return Err(format!("topic {topic} has partitions 0 to {}, not {p}", n - 1)),
                None => match &m.key {
                    Some(k) => (fnv(k) % n.max(1) as u64) as i32,
                    None => (self.inner.produced.load(Ordering::Relaxed) % n.max(1) as u64) as i32,
                },
            };
            let offset = self.inner.append(topic, p, vec![m]).map_err(|e| format!("error {e}"))?;
            out.push((p, offset));
        }
        Ok(out)
    }

    /// What the broker holds and has done: its address, topics with each
    /// partition's offsets, the generator, its connections.
    pub fn status(&self) -> Json {
        let topics: Vec<Json> = {
            let t = self.inner.topics.lock().unwrap_or_else(|e| e.into_inner());
            t.iter()
                .map(|(name, t)| {
                    let parts: Vec<Json> = t.parts.iter().enumerate().map(|(i, p)| json!({"partition": i, "start": p.start, "end": p.end(), "bytes": p.bytes})).collect();
                    json!({"name": name, "partitions": parts, "messages": t.parts.iter().map(|p| p.end()).sum::<i64>(), "created_ms": t.created_ms})
                })
                .collect()
        };
        let g = self.inner.generator.lock().unwrap_or_else(|e| e.into_inner()).clone();
        json!({
            "running": true, "address": self.address(), "started_ms": self.started_ms,
            "sasl": self.inner.sasl.as_ref().map(|(u, _)| json!({"mechanism": "PLAIN", "username": u})),
            "retain": self.inner.retain, "topics": topics,
            "generator": {"topic": g.topic, "rate": g.rate, "written": g.written},
            "connections": self.inner.connections.load(Ordering::Relaxed), "open": self.inner.open.load(Ordering::Relaxed),
            "requests": self.inner.requests.load(Ordering::Relaxed), "fetches": self.inner.fetches.load(Ordering::Relaxed),
            "produced": self.inner.produced.load(Ordering::Relaxed),
        })
    }
}

/// FNV-1a of `b`: which partition a key goes to.
fn fnv(b: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for x in b {
        h ^= *x as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

impl Inner {
    fn topics(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Topic>> {
        self.topics.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn has(&self, name: &str) -> bool {
        self.topics().contains_key(name)
    }

    fn partitions(&self, name: &str) -> i32 {
        self.topics().get(name).map_or(0, |t| t.parts.len() as i32)
    }

    /// Makes a topic; Kafka's error code when it cannot.
    fn create(&self, name: &str, partitions: i32) -> Result<(), i16> {
        if !topic_ok(name) {
            return Err(INVALID_TOPIC);
        }
        if !(1..=MAX_PARTITIONS).contains(&partitions) {
            return Err(INVALID_PARTITIONS);
        }
        let mut t = self.topics();
        if t.contains_key(name) {
            return Err(TOPIC_ALREADY_EXISTS);
        }
        t.insert(name.to_string(), Topic { parts: (0..partitions).map(|_| Partition::default()).collect(), created_ms: now_ms() });
        Ok(())
    }

    /// Appends `msgs` to a partition: the offset of the first.
    fn append(&self, topic: &str, p: i32, msgs: Vec<Message>) -> Result<i64, i16> {
        let base = {
            let mut t = self.topics();
            let part = t.get_mut(topic).and_then(|t| t.parts.get_mut(p as usize)).ok_or(UNKNOWN_TOPIC_OR_PARTITION)?;
            let base = part.end();
            let n = msgs.len() as u64;
            for m in msgs {
                part.bytes += m.size();
                part.msgs.push_back(m);
            }
            while part.msgs.len() > self.retain {
                if let Some(old) = part.msgs.pop_front() {
                    part.bytes = part.bytes.saturating_sub(old.size());
                }
                part.start += 1;
            }
            self.produced.fetch_add(n, Ordering::Relaxed);
            base
        };
        self.added.notify_waiters();
        Ok(base)
    }

    /// Writes the events the generator owes after `secs` more seconds.
    fn generate(&self, secs: f64) {
        let (topic, events) = {
            let mut g = self.generator.lock().unwrap_or_else(|e| e.into_inner());
            if g.rate <= 0.0 {
                return;
            }
            g.owed += g.rate * secs;
            let n = (g.owed.floor() as u64).min(100_000);
            g.owed -= n as f64;
            let mut events = Vec::with_capacity(n as usize);
            for _ in 0..n {
                g.seq += 1;
                g.seed ^= g.seed << 13;
                g.seed ^= g.seed >> 7;
                g.seed ^= g.seed << 17;
                events.push(click(g.seq, g.seed));
            }
            g.written += n;
            (g.topic.clone(), events)
        };
        if events.is_empty() {
            return;
        }
        let n = self.partitions(&topic);
        if n == 0 {
            return;
        }
        let mut by: BTreeMap<i32, Vec<Message>> = BTreeMap::new();
        for m in events {
            let p = (fnv(m.key.as_deref().unwrap_or_default()) % n as u64) as i32;
            by.entry(p).or_default().push(m);
        }
        for (p, msgs) in by {
            let _ = self.append(&topic, p, msgs);
        }
    }
}

/// A made-up click event, the `seq`th, drawn from `r`.
fn click(seq: u64, r: u64) -> Message {
    const ACTIONS: [&str; 8] = ["view", "view", "view", "view", "click", "click", "cart", "buy"];
    const PAGES: [&str; 5] = ["home", "search", "item", "cart", "checkout"];
    let user = format!("u{:03}", r % 200);
    let action = ACTIONS[((r >> 8) % ACTIONS.len() as u64) as usize];
    let item = format!("i{:04}", (r >> 16) % 1000);
    let at = now_ms() as i64;
    let mut v = json!({
        "event_id": format!("e-{seq:010}"), "user": user, "item": item, "action": action,
        "page": PAGES[((r >> 24) % PAGES.len() as u64) as usize], "at": chloedb::timestamp::format(at * 1000),
    });
    if action == "buy" || action == "cart" {
        v["price"] = json!(((r >> 32) % 20_000) as f64 / 100.0 + 1.0);
    }
    Message { ts: at, key: Some(user.into_bytes()), value: Some(v.to_string().into_bytes()), headers: vec![("source".into(), Some(b"chloedb-test-broker".to_vec()))] }
}

// ----- the wire

type R<T> = Result<T, String>;

/// Reads Kafka's encodings from a request.
struct Rd<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Rd<'a> {
    fn new(b: &'a [u8]) -> Rd<'a> {
        Rd { b, i: 0 }
    }

    fn take(&mut self, n: usize) -> R<&'a [u8]> {
        if self.i + n > self.b.len() {
            return Err(format!("the request ends early ({} bytes, wanted {} at {})", self.b.len(), n, self.i));
        }
        let s = &self.b[self.i..self.i + n];
        self.i += n;
        Ok(s)
    }

    fn i8(&mut self) -> R<i8> {
        Ok(self.take(1)?[0] as i8)
    }

    fn i16(&mut self) -> R<i16> {
        Ok(i16::from_be_bytes(self.take(2)?.try_into().unwrap_or_default()))
    }

    fn i32(&mut self) -> R<i32> {
        Ok(i32::from_be_bytes(self.take(4)?.try_into().unwrap_or_default()))
    }

    fn i64(&mut self) -> R<i64> {
        Ok(i64::from_be_bytes(self.take(8)?.try_into().unwrap_or_default()))
    }

    fn uvarint(&mut self) -> R<u64> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let b = self.take(1)?[0];
            v |= ((b & 0x7f) as u64) << shift;
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err("a varint is too long".into())
    }

    /// A zigzag varint (Kafka's records).
    fn varint(&mut self) -> R<i64> {
        let u = self.uvarint()?;
        Ok(((u >> 1) as i64) ^ -((u & 1) as i64))
    }

    fn string(&mut self) -> R<String> {
        self.nullable_string()?.ok_or_else(|| "a string is null".into())
    }

    fn nullable_string(&mut self) -> R<Option<String>> {
        let n = self.i16()?;
        if n < 0 {
            return Ok(None);
        }
        Ok(Some(String::from_utf8_lossy(self.take(n as usize)?).into_owned()))
    }

    fn nullable_bytes(&mut self) -> R<Option<&'a [u8]>> {
        let n = self.i32()?;
        if n < 0 {
            return Ok(None);
        }
        Ok(Some(self.take(n as usize)?))
    }

    /// An array's length: none for null.
    fn array(&mut self) -> R<Option<usize>> {
        let n = self.i32()?;
        if n < 0 {
            return Ok(None);
        }
        if n as usize > self.b.len() {
            return Err(format!("an array of {n} in a request of {} bytes", self.b.len()));
        }
        Ok(Some(n as usize))
    }

    /// Skips a flexible version's tagged fields.
    fn tagged(&mut self) -> R<()> {
        let n = self.uvarint()?;
        for _ in 0..n {
            self.uvarint()?;
            let size = self.uvarint()? as usize;
            self.take(size)?;
        }
        Ok(())
    }

    /// A record's bytes: a zigzag varint length, -1 for null.
    fn varbytes(&mut self) -> R<Option<&'a [u8]>> {
        let n = self.varint()?;
        if n < 0 {
            return Ok(None);
        }
        Ok(Some(self.take(n as usize)?))
    }
}

/// Writes Kafka's encodings into an answer.
#[derive(Default)]
struct Wr(Vec<u8>);

impl Wr {
    fn i8(&mut self, v: i8) -> &mut Self {
        self.0.push(v as u8);
        self
    }
    fn i16(&mut self, v: i16) -> &mut Self {
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }
    fn i32(&mut self, v: i32) -> &mut Self {
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }
    fn i64(&mut self, v: i64) -> &mut Self {
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }
    fn uvarint(&mut self, mut v: u64) -> &mut Self {
        while v >= 0x80 {
            self.0.push((v as u8) | 0x80);
            v >>= 7;
        }
        self.0.push(v as u8);
        self
    }
    fn varint(&mut self, v: i64) -> &mut Self {
        self.uvarint(((v << 1) ^ (v >> 63)) as u64)
    }
    fn string(&mut self, s: &str) -> &mut Self {
        self.i16(s.len() as i16);
        self.0.extend_from_slice(s.as_bytes());
        self
    }
    fn nullable_string(&mut self, s: Option<&str>) -> &mut Self {
        match s {
            Some(s) => self.string(s),
            None => self.i16(-1),
        }
    }
    fn bytes(&mut self, b: &[u8]) -> &mut Self {
        self.i32(b.len() as i32);
        self.0.extend_from_slice(b);
        self
    }
    fn varbytes(&mut self, b: Option<&[u8]>) -> &mut Self {
        match b {
            Some(b) => {
                self.varint(b.len() as i64);
                self.0.extend_from_slice(b);
            }
            None => {
                self.varint(-1);
            }
        }
        self
    }
    fn array(&mut self, n: usize) -> &mut Self {
        self.i32(n as i32)
    }
}

/// One record batch (magic 2, uncompressed) of `msgs`, the first at
/// offset `base`.
fn encode_batch(base: i64, msgs: &[&Message]) -> Vec<u8> {
    let base_ts = msgs.first().map_or(0, |m| m.ts);
    let max_ts = msgs.iter().map(|m| m.ts).max().unwrap_or(base_ts);
    let mut recs = Wr::default();
    for (i, m) in msgs.iter().enumerate() {
        let mut r = Wr::default();
        r.i8(0).varint(m.ts - base_ts).varint(i as i64).varbytes(m.key.as_deref()).varbytes(m.value.as_deref()).varint(m.headers.len() as i64);
        for (k, v) in &m.headers {
            // A null value as an empty one: clients such as rskafka read no null.
            r.varbytes(Some(k.as_bytes())).varbytes(Some(v.as_deref().unwrap_or_default()));
        }
        recs.varint(r.0.len() as i64);
        recs.0.extend_from_slice(&r.0);
    }
    let mut after = Wr::default();
    // attributes: no compression, create time; no producer.
    after.i16(0).i32(msgs.len().saturating_sub(1) as i32).i64(base_ts).i64(max_ts).i64(-1).i16(-1).i32(-1).i32(msgs.len() as i32);
    after.0.extend_from_slice(&recs.0);
    let crc = crc32c::crc32c(&after.0);
    let mut out = Wr::default();
    out.i64(base).i32((4 + 1 + 4 + after.0.len()) as i32).i32(0).i8(2);
    out.0.extend_from_slice(&crc.to_be_bytes());
    out.0.extend_from_slice(&after.0);
    out.0
}

/// The messages of the record batches in `b` (a Produce's records), or
/// Kafka's error code for them.
fn decode_batches(b: &[u8]) -> Result<Vec<Message>, i16> {
    let mut rd = Rd::new(b);
    let mut out = Vec::new();
    while rd.i < b.len() {
        let _base = rd.i64().map_err(|_| CORRUPT_MESSAGE)?;
        let len = rd.i32().map_err(|_| CORRUPT_MESSAGE)?;
        if len < 9 {
            return Err(CORRUPT_MESSAGE);
        }
        let batch = rd.take(len as usize).map_err(|_| CORRUPT_MESSAGE)?;
        let mut r = Rd::new(batch);
        let _epoch = r.i32().map_err(|_| CORRUPT_MESSAGE)?;
        if r.i8().map_err(|_| CORRUPT_MESSAGE)? != 2 {
            return Err(CORRUPT_MESSAGE);
        }
        let crc = r.i32().map_err(|_| CORRUPT_MESSAGE)? as u32;
        if crc32c::crc32c(&batch[9..]) != crc {
            return Err(CORRUPT_MESSAGE);
        }
        let attributes = r.i16().map_err(|_| CORRUPT_MESSAGE)?;
        if attributes & 0x07 != 0 {
            return Err(UNSUPPORTED_COMPRESSION_TYPE);
        }
        let parse = |r: &mut Rd<'_>| -> R<Vec<Message>> {
            let _last_delta = r.i32()?;
            let base_ts = r.i64()?;
            let _max_ts = r.i64()?;
            let _pid = r.i64()?;
            let _pepoch = r.i16()?;
            let _seq = r.i32()?;
            let n = r.i32()?;
            let mut msgs = Vec::with_capacity(n.clamp(0, 100_000) as usize);
            for _ in 0..n.max(0) {
                let size = r.varint()?;
                let body = r.take(size.max(0) as usize)?;
                let mut x = Rd::new(body);
                let _attr = x.i8()?;
                let ts_delta = x.varint()?;
                let _offset_delta = x.varint()?;
                let key = x.varbytes()?.map(<[u8]>::to_vec);
                let value = x.varbytes()?.map(<[u8]>::to_vec);
                let nh = x.varint()?;
                let mut headers = Vec::new();
                for _ in 0..nh.max(0) {
                    let k = x.varbytes()?.map(|k| String::from_utf8_lossy(k).into_owned()).unwrap_or_default();
                    let v = x.varbytes()?.map(<[u8]>::to_vec);
                    headers.push((k, v));
                }
                let ts = if base_ts < 0 { now_ms() as i64 } else { base_ts + ts_delta };
                msgs.push(Message { ts, key, value, headers });
            }
            Ok(msgs)
        };
        out.extend(parse(&mut r).map_err(|_| CORRUPT_MESSAGE)?);
    }
    Ok(out)
}

/// Serves one client until it closes, sends what cannot be read, or the
/// broker stops.
async fn serve(i: &Arc<Inner>, mut s: TcpStream, mut stop: watch::Receiver<bool>) -> R<()> {
    let mut signed_in = i.sasl.is_none();
    let mut handshake = false;
    loop {
        let mut len = [0u8; 4];
        tokio::select! {
            r = s.read_exact(&mut len) => { r.map_err(|e| e.to_string())?; }
            _ = stop.changed() => return Ok(()),
        }
        let n = i32::from_be_bytes(len);
        if n < 8 || n as usize > MAX_REQUEST {
            return Err(format!("a request of {n} bytes"));
        }
        let mut buf = vec![0u8; n as usize];
        s.read_exact(&mut buf).await.map_err(|e| e.to_string())?;
        i.requests.fetch_add(1, Ordering::Relaxed);
        let mut rd = Rd::new(&buf);
        let api = rd.i16()?;
        let version = rd.i16()?;
        let correlation = rd.i32()?;
        let _client = rd.nullable_string()?;
        if api == API_VERSIONS && version >= 3 {
            rd.tagged()?;
        }
        if !signed_in && !matches!(api, API_VERSIONS | SASL_HANDSHAKE | SASL_AUTHENTICATE) {
            return Err("a request before signing in".into());
        }
        let known = VERSIONS.iter().any(|(k, lo, hi)| *k == api && (*lo..=*hi).contains(&version));
        let body = if api == API_VERSIONS {
            Some(api_versions(version))
        } else if !known {
            // A version this broker does not speak: it cannot answer in it.
            return Err(format!("API {api} at version {version}"));
        } else {
            match api {
                METADATA => Some(metadata(i, &mut rd)?),
                LIST_OFFSETS => Some(list_offsets(i, &mut rd)?),
                FETCH => match fetch(i, &mut rd, &mut stop).await? {
                    Some(b) => Some(b),
                    None => return Ok(()),
                },
                PRODUCE => produce(i, &mut rd)?,
                CREATE_TOPICS => Some(create_topics(i, &mut rd)?),
                DELETE_TOPICS => Some(delete_topics(i, &mut rd)?),
                SASL_HANDSHAKE => {
                    let mech = rd.string()?;
                    let mut w = Wr::default();
                    let ok = mech == "PLAIN" && i.sasl.is_some();
                    handshake = ok;
                    w.i16(if ok { 0 } else { UNSUPPORTED_SASL_MECHANISM });
                    match &i.sasl {
                        Some(_) => w.array(1).string("PLAIN"),
                        None => w.array(0),
                    };
                    Some(w.0)
                }
                SASL_AUTHENTICATE => {
                    let token = rd.nullable_bytes()?.unwrap_or_default();
                    let mut w = Wr::default();
                    if !handshake {
                        w.i16(ILLEGAL_SASL_STATE).nullable_string(Some("SaslHandshake first")).bytes(&[]);
                    } else {
                        // PLAIN: [authzid] NUL user NUL password.
                        let parts: Vec<&[u8]> = token.split(|b| *b == 0).collect();
                        let ok = match (&i.sasl, parts.as_slice()) {
                            (Some((u, p)), [_, user, pass]) => user == &u.as_bytes() && pass == &p.as_bytes(),
                            _ => false,
                        };
                        if ok {
                            signed_in = true;
                            w.i16(0).nullable_string(None).bytes(&[]);
                        } else {
                            w.i16(SASL_AUTHENTICATION_FAILED).nullable_string(Some("Authentication failed: Invalid username or password")).bytes(&[]);
                        }
                    }
                    Some(w.0)
                }
                _ => return Err(format!("API {api}")),
            }
        };
        let Some(body) = body else { continue };
        let mut frame = Vec::with_capacity(body.len() + 8);
        frame.extend_from_slice(&((body.len() + 4) as i32).to_be_bytes());
        frame.extend_from_slice(&correlation.to_be_bytes());
        frame.extend_from_slice(&body);
        s.write_all(&frame).await.map_err(|e| e.to_string())?;
    }
}

fn api_versions(version: i16) -> Vec<u8> {
    let mut w = Wr::default();
    if !(0..=3).contains(&version) {
        // Kafka answers a version it does not know in version 0.
        w.i16(UNSUPPORTED_VERSION).array(VERSIONS.len());
        for (k, lo, hi) in VERSIONS {
            w.i16(*k).i16(*lo).i16(*hi);
        }
        return w.0;
    }
    w.i16(0);
    if version >= 3 {
        w.uvarint(VERSIONS.len() as u64 + 1);
        for (k, lo, hi) in VERSIONS {
            w.i16(*k).i16(*lo).i16(*hi).uvarint(0);
        }
    } else {
        w.array(VERSIONS.len());
        for (k, lo, hi) in VERSIONS {
            w.i16(*k).i16(*lo).i16(*hi);
        }
    }
    if version >= 1 {
        w.i32(0);
    }
    if version >= 3 {
        w.uvarint(0);
    }
    w.0
}

fn metadata(i: &Inner, rd: &mut Rd<'_>) -> R<Vec<u8>> {
    let asked: Option<Vec<String>> = match rd.array()? {
        None => None,
        Some(n) => Some((0..n).map(|_| rd.string()).collect::<R<_>>()?),
    };
    // A topic asked for by name is made, as a Kafka broker with
    // auto.create.topics.enable (its default) does for Metadata before
    // version 4: so a producer's first write to a new topic finds it.
    for name in asked.iter().flatten() {
        if topic_ok(name) && !i.has(name) {
            let _ = i.create(name, 3);
        }
    }
    let t = i.topics();
    let mut w = Wr::default();
    w.array(1).i32(0).string(&i.host).i32(i.port as i32).nullable_string(None);
    w.i32(0);
    let names: Vec<String> = asked.unwrap_or_else(|| t.keys().cloned().collect());
    w.array(names.len());
    for name in names {
        match t.get(&name) {
            Some(topic) => {
                w.i16(0).string(&name).i8(0).array(topic.parts.len());
                for p in 0..topic.parts.len() {
                    w.i16(0).i32(p as i32).i32(0).array(1).i32(0).array(1).i32(0);
                }
            }
            None => {
                w.i16(if topic_ok(&name) { UNKNOWN_TOPIC_OR_PARTITION } else { INVALID_TOPIC }).string(&name).i8(0).array(0);
            }
        }
    }
    Ok(w.0)
}

fn list_offsets(i: &Inner, rd: &mut Rd<'_>) -> R<Vec<u8>> {
    let _replica = rd.i32()?;
    let nt = rd.array()?.unwrap_or(0);
    let t = i.topics();
    let mut w = Wr::default();
    w.array(nt);
    for _ in 0..nt {
        let name = rd.string()?;
        let np = rd.array()?.unwrap_or(0);
        w.string(&name).array(np);
        for _ in 0..np {
            let p = rd.i32()?;
            let ts = rd.i64()?;
            match t.get(&name).and_then(|t| t.parts.get(p as usize)) {
                None => {
                    w.i32(p).i16(UNKNOWN_TOPIC_OR_PARTITION).i64(-1).i64(-1);
                }
                Some(part) => {
                    let (at, offset) = match ts {
                        -1 => (-1, part.end()),
                        -2 => (-1, part.start),
                        ts => match part.msgs.iter().position(|m| m.ts >= ts) {
                            Some(k) => (part.msgs[k].ts, part.start + k as i64),
                            None => (-1, part.end()),
                        },
                    };
                    w.i32(p).i16(0).i64(at).i64(offset);
                }
            }
        }
    }
    Ok(w.0)
}

/// One partition a fetch asks for.
struct Want {
    partition: i32,
    offset: i64,
    max_bytes: i32,
}

/// A Fetch's answer, once it has a message or its wait is over (None: the
/// broker stopped).
async fn fetch(i: &Arc<Inner>, rd: &mut Rd<'_>, stop: &mut watch::Receiver<bool>) -> R<Option<Vec<u8>>> {
    i.fetches.fetch_add(1, Ordering::Relaxed);
    let _replica = rd.i32()?;
    let max_wait = rd.i32()?;
    let min_bytes = rd.i32()?;
    let max_bytes = rd.i32()?;
    let _isolation = rd.i8()?;
    let nt = rd.array()?.unwrap_or(0);
    let mut asked: Vec<(String, Vec<Want>)> = Vec::with_capacity(nt);
    for _ in 0..nt {
        let name = rd.string()?;
        let np = rd.array()?.unwrap_or(0);
        let mut ps = Vec::with_capacity(np);
        for _ in 0..np {
            ps.push(Want { partition: rd.i32()?, offset: rd.i64()?, max_bytes: rd.i32()? });
        }
        asked.push((name, ps));
    }
    let deadline = tokio::time::Instant::now() + Duration::from_millis(max_wait.max(0) as u64).min(MAX_WAIT);
    loop {
        let added = i.added.notified();
        tokio::pin!(added);
        added.as_mut().enable();
        let (body, bytes, errors) = fetch_answer(i, &asked, max_bytes);
        if errors || bytes >= min_bytes.max(1) as usize || tokio::time::Instant::now() >= deadline {
            return Ok(Some(body));
        }
        tokio::select! {
            _ = &mut added => {}
            _ = tokio::time::sleep_until(deadline) => {}
            _ = stop.changed() => return Ok(None),
        }
    }
}

/// A Fetch's answer as things stand: it, the bytes of records in it, and
/// whether a partition answered an error.
fn fetch_answer(i: &Inner, asked: &[(String, Vec<Want>)], max_bytes: i32) -> (Vec<u8>, usize, bool) {
    let t = i.topics();
    let mut w = Wr::default();
    let mut total = 0usize;
    let mut errors = false;
    let mut left = if max_bytes > 0 { max_bytes as usize } else { usize::MAX };
    w.i32(0).array(asked.len());
    for (name, ps) in asked {
        w.string(name).array(ps.len());
        for want in ps {
            let part = t.get(name).and_then(|t| t.parts.get(want.partition as usize));
            let Some(part) = part else {
                errors = true;
                w.i32(want.partition).i16(UNKNOWN_TOPIC_OR_PARTITION).i64(-1).i64(-1).i32(-1).bytes(&[]);
                continue;
            };
            let end = part.end();
            if want.offset < part.start || want.offset > end {
                errors = true;
                w.i32(want.partition).i16(OFFSET_OUT_OF_RANGE).i64(end).i64(end).i32(-1).bytes(&[]);
                continue;
            }
            let cap = (want.max_bytes.max(1) as usize).min(left);
            let mut msgs = Vec::new();
            let mut size = 0usize;
            for m in part.msgs.iter().skip((want.offset - part.start) as usize) {
                let s = m.size();
                // At least one message, however large, as Kafka does.
                if !msgs.is_empty() && size + s > cap {
                    break;
                }
                size += s;
                msgs.push(m);
                if msgs.len() >= 10_000 {
                    break;
                }
            }
            let records = if msgs.is_empty() { Vec::new() } else { encode_batch(want.offset, &msgs) };
            total += records.len();
            left = left.saturating_sub(records.len());
            // No aborted transactions (v4's list), the records.
            w.i32(want.partition).i16(0).i64(end).i64(end).i32(-1).bytes(&records);
        }
    }
    (w.0, total, errors)
}

/// A Produce's answer (None: the client asked for none, acks 0).
fn produce(i: &Inner, rd: &mut Rd<'_>) -> R<Option<Vec<u8>>> {
    let _txn = rd.nullable_string()?;
    let acks = rd.i16()?;
    let _timeout = rd.i32()?;
    let nt = rd.array()?.unwrap_or(0);
    let mut w = Wr::default();
    w.array(nt);
    for _ in 0..nt {
        let name = rd.string()?;
        let np = rd.array()?.unwrap_or(0);
        w.string(&name).array(np);
        if !i.has(&name) && topic_ok(&name) {
            // As a broker with auto.create.topics.enable.
            let _ = i.create(&name, 3);
        }
        for _ in 0..np {
            let p = rd.i32()?;
            let records = rd.nullable_bytes()?.unwrap_or_default();
            let r = decode_batches(records).and_then(|msgs| i.append(&name, p, msgs));
            match r {
                Ok(base) => w.i32(p).i16(0).i64(base).i64(-1),
                Err(code) => w.i32(p).i16(code).i64(-1).i64(-1),
            };
        }
    }
    w.i32(0);
    Ok((acks != 0).then_some(w.0))
}

fn create_topics(i: &Inner, rd: &mut Rd<'_>) -> R<Vec<u8>> {
    let nt = rd.array()?.unwrap_or(0);
    let mut out = Vec::with_capacity(nt);
    for _ in 0..nt {
        let name = rd.string()?;
        let partitions = rd.i32()?;
        let _replication = rd.i16()?;
        let na = rd.array()?.unwrap_or(0);
        for _ in 0..na {
            rd.i32()?;
            let nb = rd.array()?.unwrap_or(0);
            for _ in 0..nb {
                rd.i32()?;
            }
        }
        let nc = rd.array()?.unwrap_or(0);
        for _ in 0..nc {
            rd.string()?;
            rd.nullable_string()?;
        }
        let code = match i.create(&name, if partitions < 0 { 1 } else { partitions }) {
            Ok(()) => 0,
            Err(c) => c,
        };
        out.push((name, code));
    }
    let _timeout = rd.i32()?;
    let mut w = Wr::default();
    w.array(out.len());
    for (name, code) in out {
        w.string(&name).i16(code);
    }
    Ok(w.0)
}

fn delete_topics(i: &Inner, rd: &mut Rd<'_>) -> R<Vec<u8>> {
    let n = rd.array()?.unwrap_or(0);
    let names: Vec<String> = (0..n).map(|_| rd.string()).collect::<R<_>>()?;
    let _timeout = rd.i32()?;
    let mut w = Wr::default();
    w.array(names.len());
    let mut t = i.topics();
    for name in names {
        let code = if t.remove(&name).is_some() { 0 } else { UNKNOWN_TOPIC_OR_PARTITION };
        w.string(&name).i16(code);
    }
    Ok(w.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_batch_reads_back_as_written() {
        let msgs = [
            Message { ts: 1_000, key: Some(b"k".to_vec()), value: Some(b"{\"a\":1}".to_vec()), headers: vec![("h".into(), Some(b"v".to_vec()))] },
            Message { ts: 1_005, key: None, value: None, headers: vec![] },
        ];
        let b = encode_batch(41, &msgs.iter().collect::<Vec<_>>());
        let back = decode_batches(&b).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!((back[0].ts, back[0].key.as_deref(), back[0].value.as_deref()), (1_000, Some(&b"k"[..]), Some(&b"{\"a\":1}"[..])));
        assert_eq!(back[0].headers, vec![("h".to_string(), Some(b"v".to_vec()))]);
        assert_eq!((back[1].ts, back[1].key.is_none(), back[1].value.is_none()), (1_005, true, true));
        // A flipped bit fails the checksum.
        let mut bad = b.clone();
        let last = bad.len() - 1;
        bad[last] ^= 1;
        assert_eq!(decode_batches(&bad).unwrap_err(), CORRUPT_MESSAGE);
    }

    #[test]
    fn varints_are_zigzag() {
        for v in [0i64, 1, -1, 63, -64, 64, 300, -300, i32::MAX as i64, i64::MIN / 2] {
            let mut w = Wr::default();
            w.varint(v);
            assert_eq!(Rd::new(&w.0).varint().unwrap(), v);
        }
        let mut w = Wr::default();
        w.varint(-1);
        assert_eq!(w.0, vec![1]);
    }

    #[test]
    fn retention_moves_the_log_start() {
        let i = Inner {
            topics: Mutex::new(BTreeMap::new()),
            generator: Mutex::new(Generator::default()),
            added: Notify::new(),
            sasl: None,
            retain: 3,
            host: "127.0.0.1".into(),
            port: 0,
            connections: AtomicU64::new(0),
            open: AtomicU64::new(0),
            requests: AtomicU64::new(0),
            fetches: AtomicU64::new(0),
            produced: AtomicU64::new(0),
        };
        i.create("t", 1).unwrap();
        assert_eq!(i.create("t", 1), Err(TOPIC_ALREADY_EXISTS));
        assert_eq!(i.create("bad name", 1), Err(INVALID_TOPIC));
        for n in 0..5 {
            assert_eq!(i.append("t", 0, vec![Message { ts: n, ..Default::default() }]), Ok(n));
        }
        let t = i.topics();
        let p = &t["t"].parts[0];
        assert_eq!((p.start, p.end(), p.msgs.len()), (2, 5, 3));
    }
}
