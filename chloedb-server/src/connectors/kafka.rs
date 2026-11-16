//! The `kafka` connector: Kafka topics read into a table by the server
//! itself, each event written once.
//!
//! A pull connector runs now and then and a push connector waits for a
//! request; a Kafka connector streams. While it is on (`enabled`, started
//! and stopped from the command center or `POST
//! /v1/admin/connectors/NAME/start` and `/stop`), the server that runs the
//! connectors' pulls ([`super`]: a server of its own, or the coordinator
//! that sends the alerts) keeps a consumer of its topics open: a task per
//! partition fetches (rskafka, a client in Rust alone), and one writer
//! turns the events into rows and writes them in batches through the
//! server's stream path ([`Service::stream_write`], as `/v1/stream` does).
//!
//! - **Each event once.** Every partition is a stream source named
//!   `<prefix><topic>/<partition>` (prefix `kafka/<connector>/` unless the
//!   config gives one), whose last offset the server keeps in `_sources`
//!   in the transaction of the rows its events made. A consumer starts
//!   each partition after the offset ChloeDB has, never after what a
//!   consumer group committed to Kafka, so a restart of the consumer, of
//!   the server or of a broker, a failover between coordinators, or two
//!   coordinators reading for a moment at once, writes no event twice:
//!   the second write of an offset is a replay, and skipped. A partition
//!   ChloeDB has no offset of starts at its first event (`"start":
//!   "earliest"`, the default) or at its end (`"latest"`).
//! - **No consumer group.** The server reads every partition of its topics
//!   itself and commits nothing to Kafka: Kafka's lag tools do not see
//!   it, and the command center shows each partition's lag instead.
//! - **Rows.** An event's value is JSON (`"format": "json"`) or text
//!   (`"text"`). Without a map, a JSON object's fields are the row's
//!   columns; a map reads the whole event: `$.value` (and `$.value.x`),
//!   `$.key`, `$.topic`, `$.partition`, `$.offset`, `$.ts` (milliseconds)
//!   and `$.headers`. A keyed table whose row has no key gets
//!   `<topic>/<partition>/<offset>`; a keyed table's rows are upserted,
//!   another's inserted. An event no row comes of (a value that is not
//!   JSON, a map that fails) still moves its partition's offset, and is
//!   counted. A tombstone (no value) writes nothing, or deletes the row
//!   of its key with `"tombstones": "delete"`.
//! - **Brokers.** Plain TCP, or TLS (`"tls": true`, checked against the
//!   public roots or the config's `ca_pem`), with SASL PLAIN or SCRAM
//!   (`"sasl": {"mechanism", "username"}`, the password a secret). The
//!   bootstrap brokers are checked as a pull's URL is ([`super::reach`]):
//!   a private address only with `"allow_internal": true`, never a
//!   link-local one; the brokers a bootstrap broker names are reached as
//!   it names them.
//! - **When it fails.** A failure of the brokers or of a write ends the
//!   consumer's session; the next starts after a wait that doubles up to
//!   a minute, again from ChloeDB's offsets. A partition whose events
//!   retention deleted before they were read goes on from its first event
//!   kept, and the events lost are counted and named in a warning. A
//!   partition whose offsets went back (a topic emptied or made again)
//!   stops the consumer with an error: its new events would be taken for
//!   ones written already, so the config needs another prefix to read it
//!   again.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rskafka::client::error::{Error as KafkaError, ProtocolError};
use rskafka::client::partition::{OffsetAt, PartitionClient, UnknownTopicHandling};
use rskafka::client::{Client, ClientBuilder, Credentials, SaslConfig};
use rskafka::record::RecordAndOffset;
use serde_json::{json, Map, Value as Json};
use tokio::sync::{mpsc, watch};

use super::kinds::{self, Fetched};
use super::{lock, map, Connector, State, RUNS, STATE};
use crate::service::{Service, Stream};
use crate::telemetry::now_ms;
use chloedb::schema::{FieldType, Schema};

/// The kind's name.
pub const KIND: &str = "kafka";

/// Where a partition ChloeDB has no offset of starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Start {
    Earliest,
    Latest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Json,
    Text,
}

#[derive(Clone, Debug)]
pub struct Sasl {
    pub mechanism: String,
    pub username: String,
}

/// A Kafka connector's config, checked.
#[derive(Clone, Debug)]
pub struct Settings {
    pub brokers: Vec<String>,
    pub topics: Vec<String>,
    pub start: Start,
    pub format: Format,
    pub prefix: String,
    pub tls: bool,
    pub ca_pem: Option<String>,
    pub sasl: Option<Sasl>,
    /// The most events of one write.
    pub batch: usize,
    pub max_wait_ms: i32,
    /// The most bytes one fetch of a partition reads.
    pub fetch_bytes: i32,
    pub delete_tombstones: bool,
}

fn text_list(v: &Json, what: &str) -> Result<Vec<String>, String> {
    let list: Vec<String> = match v {
        Json::String(s) => s.split(',').map(str::trim).filter(|x| !x.is_empty()).map(str::to_string).collect(),
        Json::Array(a) => a.iter().map(|x| x.as_str().map(|s| s.trim().to_string()).ok_or_else(|| format!("{what} are text"))).collect::<Result<_, _>>()?,
        Json::Null => Vec::new(),
        _ => return Err(format!("{what} are text joined by commas, or a list")),
    };
    Ok(list)
}

impl Settings {
    /// `c`'s settings, or what is wrong with them.
    pub fn of(c: &Connector) -> Result<Settings, String> {
        let k = &c.config;
        let brokers = text_list(&k["brokers"], "brokers")?;
        if brokers.is_empty() || brokers.len() > 16 {
            return Err("brokers names 1 to 16 bootstrap brokers, host:port each (\"kafka-1:9092,kafka-2:9092\")".into());
        }
        for b in &brokers {
            let (host, port) = b.rsplit_once(':').ok_or_else(|| format!("broker {b:?} is host:port"))?;
            if host.trim_matches(['[', ']']).is_empty() || port.parse::<u16>().map_or(true, |p| p == 0) || b.contains('/') || b.contains('@') {
                return Err(format!("broker {b:?} is host:port"));
            }
        }
        let topics = text_list(&k["topics"], "topics")?;
        if topics.is_empty() || topics.len() > 32 {
            return Err("topics names 1 to 32 topics".into());
        }
        if let Some(t) = topics.iter().find(|t| !super::kafka_broker::topic_ok(t)) {
            return Err(format!("{t:?} is not a topic's name: 1 to 249 letters, digits, '.', '_' and '-'"));
        }
        let start = match k["start"].as_str() {
            None | Some("earliest") => Start::Earliest,
            Some("latest") => Start::Latest,
            Some(o) => return Err(format!("start is earliest or latest, not {o:?}")),
        };
        if !k["start"].is_null() && !k["start"].is_string() {
            return Err("start is earliest or latest".into());
        }
        let format = match k["format"].as_str() {
            None | Some("json") => Format::Json,
            Some("text") => Format::Text,
            Some(o) => return Err(format!("format is json or text, not {o:?}")),
        };
        let prefix = match &k["prefix"] {
            Json::Null => format!("kafka/{}/", c.name),
            Json::String(p) => p.clone(),
            _ => return Err("prefix is text".into()),
        };
        if prefix.len() > 128 || prefix.starts_with(crate::service::MARK) {
            return Err(format!("prefix is 128 bytes at most, and does not start with {} (the server's own sources)", crate::service::MARK));
        }
        let flag = |name: &str| -> Result<bool, String> {
            match &k[name] {
                Json::Null => Ok(false),
                Json::Bool(b) => Ok(*b),
                _ => Err(format!("{name} is true or false")),
            }
        };
        let tls = flag("tls")?;
        let ca_pem = match &k["ca_pem"] {
            Json::Null => None,
            Json::String(s) if s.contains("-----BEGIN CERTIFICATE-----") => Some(s.clone()),
            _ => return Err("ca_pem is a PEM certificate (-----BEGIN CERTIFICATE----- ...)".into()),
        };
        if ca_pem.is_some() && !tls {
            return Err("ca_pem checks a TLS broker: set \"tls\": true".into());
        }
        let sasl = match &k["sasl"] {
            Json::Null => None,
            Json::Object(o) => {
                let mechanism = o.get("mechanism").and_then(Json::as_str).unwrap_or("PLAIN").to_ascii_uppercase();
                if !matches!(mechanism.as_str(), "PLAIN" | "SCRAM-SHA-256" | "SCRAM-SHA-512") {
                    return Err(format!("sasl.mechanism is PLAIN, SCRAM-SHA-256 or SCRAM-SHA-512, not {mechanism:?}"));
                }
                let username = o.get("username").and_then(Json::as_str).filter(|u| !u.is_empty()).ok_or("sasl.username is the user it signs in as (its password is the secret \"password\")")?;
                Some(Sasl { mechanism, username: username.to_string() })
            }
            _ => return Err("sasl is {\"mechanism\": \"PLAIN\", \"username\": \"...\"}, its password the secret \"password\"".into()),
        };
        let num = |name: &str, lo: u64, hi: u64, default: u64| -> Result<u64, String> {
            match &k[name] {
                Json::Null => Ok(default),
                v => v.as_u64().filter(|n| (lo..=hi).contains(n)).ok_or_else(|| format!("{name} is a whole number from {lo} to {hi}")),
            }
        };
        let batch = num("batch", 1, 4096, 1000)? as usize;
        let max_wait_ms = num("max_wait_ms", 10, 10_000, 500)? as i32;
        let fetch_bytes = (num("fetch_kb", 1, 65_536, 1024)? * 1024) as i32;
        let delete_tombstones = match k["tombstones"].as_str() {
            None | Some("skip") => false,
            Some("delete") => true,
            Some(o) => return Err(format!("tombstones is skip or delete, not {o:?}")),
        };
        Ok(Settings { brokers, topics, start, format, prefix, tls, ca_pem, sasl, batch, max_wait_ms, fetch_bytes, delete_tombstones })
    }

    /// The stream source of a partition.
    pub fn source(&self, topic: &str, partition: i32) -> String {
        format!("{}{topic}/{partition}", self.prefix)
    }
}

// ----- what the command center sees

/// One partition, as the consumer sees it.
#[derive(Clone, Debug, Default)]
struct Part {
    topic: String,
    partition: i32,
    /// The offset of the next event to write (ChloeDB has those before).
    next: i64,
    /// Kafka's high watermark: the offset the next event will take.
    high: i64,
    /// When the last event written happened (its Kafka timestamp, ms).
    last_ts: Option<i64>,
    lost: u64,
}

#[derive(Clone, Debug, Default)]
struct LiveState {
    /// starting, connecting, streaming, retrying, stopped.
    state: &'static str,
    since_ms: u64,
    session_ms: u64,
    sessions: u64,
    events: u64,
    rows: u64,
    replays: u64,
    unmapped: u64,
    deleted: u64,
    lost: u64,
    last_write_ms: u64,
    last_error: Option<String>,
    last_error_ms: u64,
    failures: u32,
    warning: Option<String>,
    /// The first event of the session no row came of, and why.
    first_unmapped: Option<String>,
    parts: BTreeMap<(String, i32), Part>,
    /// Events written, by second (the last minute's).
    window: VecDeque<(u64, u64)>,
    /// What was saved to the state table already.
    saved_events: u64,
    saved_rows: u64,
    /// Since the last run row.
    run_from_ms: u64,
    run_events: u64,
    run_rows: u64,
    run_replays: u64,
}

/// A consumer's state, shown live; kept after it stops, until the server
/// does.
#[derive(Default)]
pub struct Live {
    s: Mutex<LiveState>,
}

impl Live {
    fn with<T>(&self, f: impl FnOnce(&mut LiveState) -> T) -> T {
        f(&mut lock(&self.s))
    }

    fn set(&self, state: &'static str) {
        self.with(|s| {
            if s.state != state {
                s.state = state;
                s.since_ms = now_ms();
            }
        });
    }

    fn state(&self) -> &'static str {
        self.with(|s| s.state)
    }

    fn rate(s: &LiveState, secs: u64) -> f64 {
        let now = now_ms() / 1000;
        let n: u64 = s.window.iter().filter(|(t, _)| now.saturating_sub(*t) < secs).map(|(_, n)| n).sum();
        n as f64 / secs as f64
    }

    fn json(&self) -> Json {
        self.with(|s| {
            let parts: Vec<Json> = s
                .parts
                .values()
                .map(|p| json!({"topic": p.topic, "partition": p.partition, "next": p.next, "high": p.high, "lag": (p.high - p.next).max(0), "last_event_ms": p.last_ts, "lost": p.lost}))
                .collect();
            let lag: i64 = s.parts.values().map(|p| (p.high - p.next).max(0)).sum();
            json!({
                "state": s.state, "since_ms": s.since_ms, "session_ms": s.session_ms, "sessions": s.sessions,
                "events": s.events, "rows": s.rows, "replays": s.replays, "unmapped": s.unmapped, "deleted": s.deleted, "lost": s.lost,
                "rate_10s": Self::rate(s, 10), "rate_60s": Self::rate(s, 60), "lag": lag, "partitions": parts,
                "last_write_ms": (s.last_write_ms > 0).then_some(s.last_write_ms),
                "last_error": s.last_error, "last_error_ms": (s.last_error_ms > 0).then_some(s.last_error_ms), "failures": s.failures,
                "warning": s.warning, "first_unmapped": s.first_unmapped,
            })
        })
    }
}

/// A consumer the server runs.
pub struct Running {
    /// The config it runs (its `updated_ms`): another restarts it.
    version: u64,
    stop: watch::Sender<bool>,
    handle: tokio::task::JoinHandle<()>,
}

impl Running {
    fn stop(&self) {
        let _ = self.stop.send(true);
    }
}

/// The live state of connector `name`, made if it has none.
fn live(svc: &Service, name: &str) -> Arc<Live> {
    lock(&svc.connectors.lives).entry(name.to_string()).or_default().clone()
}

/// What the connectors' API answers of a Kafka connector beyond the rest:
/// its consumer, live (null before it first ran here).
pub fn json_of(svc: &Service, c: &Connector) -> Json {
    let l = lock(&svc.connectors.lives).get(&c.name).cloned();
    let mut j = l.map_or(Json::Null, |l| l.json());
    if let Ok(s) = Settings::of(c) {
        if j.is_null() {
            j = json!({"state": if c.enabled { "pending" } else { "stopped" }});
        }
        j["sources"] = json!(s.topics.iter().map(|t| format!("{}{t}/*", s.prefix)).collect::<Vec<_>>());
    }
    j
}

/// A Kafka connector's health ([`super::Connectors::health`]).
pub fn health(svc: &Service, c: &Connector) -> &'static str {
    if c.invalid.is_some() {
        return "invalid";
    }
    if !c.enabled {
        return "disabled";
    }
    let Some(l) = lock(&svc.connectors.lives).get(&c.name).cloned() else { return "pending" };
    l.with(|s| match s.state {
        "streaming" if s.warning.is_some() => "warning",
        "streaming" => "streaming",
        "retrying" => "failing",
        "starting" | "connecting" => "connecting",
        _ => "pending",
    })
}

/// Starts the consumers of the Kafka connectors in `list` that are on, and
/// stops those that are off, gone or changed (`here`: this server runs the
/// connectors; when it does not, every consumer stops).
pub fn supervise(svc: &Arc<Service>, list: &[Connector], here: bool) {
    let want: HashMap<&str, &Connector> = if here { list.iter().filter(|c| c.kind == KIND && c.enabled && c.invalid.is_none()).map(|c| (c.name.as_str(), c)).collect() } else { HashMap::new() };
    let mut running = lock(&svc.connectors.streams);
    running.retain(|name, r| {
        let keep = want.get(name.as_str()).is_some_and(|c| c.updated_ms == r.version) && !r.handle.is_finished();
        if !keep {
            r.stop();
        }
        keep
    });
    for (name, c) in want {
        if running.contains_key(name) {
            continue;
        }
        let (tx, rx) = watch::channel(false);
        let l = live(svc, name);
        l.set("starting");
        let (svc2, c2) = (svc.clone(), c.clone());
        let handle = tokio::spawn(async move { run(&svc2, c2, l, rx).await });
        running.insert(name.to_string(), Running { version: c.updated_ms, stop: tx, handle });
    }
}

/// Stops connector `name`'s consumer now, if it runs.
pub fn stop(svc: &Service, name: &str) {
    if let Some(r) = lock(&svc.connectors.streams).remove(name) {
        r.stop();
    }
}

/// A consumer: sessions one after the other until it is stopped.
async fn run(svc: &Arc<Service>, c: Connector, l: Arc<Live>, mut stop: watch::Receiver<bool>) {
    let mut failures = 0u32;
    l.with(|s| {
        s.run_from_ms = now_ms();
        s.failures = 0;
    });
    loop {
        if *stop.borrow() {
            break;
        }
        let before = l.with(|s| s.events);
        let r = session(svc, &c, &l, &mut stop).await;
        let secrets = svc.connectors.secrets_of(&c.name);
        let r = r.map_err(|e| kinds::redact(&e, &secrets));
        if l.with(|s| s.events) > before {
            failures = 0;
        }
        match &r {
            Ok(()) => {}
            Err(e) => {
                failures += 1;
                tracing::warn!(connector = %c.name, error = %e, "kafka connector's session ended");
                l.with(|s| {
                    s.last_error = Some(e.clone());
                    s.last_error_ms = now_ms();
                    s.failures = failures;
                });
                l.set("retrying");
            }
        }
        save(svc, &c, &l, Some(r.as_ref().err().cloned())).await;
        if r.is_ok() {
            break;
        }
        let wait = Duration::from_secs((1u64 << failures.min(6)).min(60));
        tokio::select! {
            _ = tokio::time::sleep(wait) => {}
            _ = stop.changed() => break,
        }
    }
    l.set("stopped");
    save(svc, &c, &l, None).await;
}

/// The bootstrap brokers may be reached by `c` ([`super::reach`]).
async fn reach_brokers(c: &Connector, s: &Settings) -> Result<(), String> {
    // A broker is reached directly, never through the HTTP proxies a
    // pull's reach lets through.
    let reach = super::reach::Reach::direct(kinds::internal(c));
    for b in &s.brokers {
        let (host, _) = b.rsplit_once(':').unwrap_or((b, ""));
        let host = host.trim_matches(['[', ']']);
        let addrs: Vec<SocketAddr> = tokio::time::timeout(Duration::from_secs(10), tokio::net::lookup_host(b.as_str())).await.map_err(|_| format!("{b}: its name did not resolve in 10 s"))?.map_err(|e| format!("{b}: {e}"))?.collect();
        if addrs.is_empty() {
            return Err(format!("{b}: its name resolves to no address"));
        }
        reach.addresses(host, addrs)?;
    }
    Ok(())
}

fn tls_config(ca_pem: Option<&str>) -> Result<rustls::ClientConfig, String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut roots = rustls::RootCertStore::empty();
    match ca_pem {
        Some(pem) => {
            for cert in rustls_pemfile::certs(&mut pem.as_bytes()) {
                let cert = cert.map_err(|e| format!("ca_pem: {e}"))?;
                roots.add(cert).map_err(|e| format!("ca_pem: {e}"))?;
            }
        }
        None => roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned()),
    }
    Ok(rustls::ClientConfig::builder_with_provider(provider).with_safe_default_protocol_versions().map_err(|e| format!("TLS: {e}"))?.with_root_certificates(roots).with_no_client_auth())
}

/// A client of `c`'s brokers, signed in.
async fn connect(c: &Connector, s: &Settings, secrets: &HashMap<String, String>) -> Result<Client, String> {
    reach_brokers(c, s).await?;
    let backoff = rskafka::BackoffConfig { init_backoff: Duration::from_millis(100), max_backoff: Duration::from_secs(5), base: 3.0, deadline: Some(Duration::from_secs(10)) };
    let mut b = ClientBuilder::new(s.brokers.clone()).client_id(format!("chloedb-{}", c.name)).backoff_config(backoff);
    if s.tls {
        b = b.tls_config(Arc::new(tls_config(s.ca_pem.as_deref())?));
    }
    if let Some(sasl) = &s.sasl {
        let password = secrets.get("password").cloned().ok_or("sasl signs in with the secret \"password\", which this connector has not been given")?;
        let cred = Credentials::new(sasl.username.clone(), password);
        b = b.sasl_config(match sasl.mechanism.as_str() {
            "SCRAM-SHA-256" => SaslConfig::ScramSha256(cred),
            "SCRAM-SHA-512" => SaslConfig::ScramSha512(cred),
            _ => SaslConfig::Plain(cred),
        });
    }
    match tokio::time::timeout(Duration::from_secs(30), b.build()).await {
        Err(_) => Err(format!("connecting to {} took more than 30 s", s.brokers.join(","))),
        Ok(Err(e)) => Err(format!("connecting to {}: {e}", s.brokers.join(","))),
        Ok(Ok(client)) => Ok(client),
    }
}

/// The partitions of `s`'s topics.
async fn partitions(client: &Client, s: &Settings) -> Result<Vec<(String, i32)>, String> {
    let topics = tokio::time::timeout(Duration::from_secs(30), client.list_topics()).await.map_err(|_| "listing the topics took more than 30 s".to_string())?.map_err(|e| format!("listing the topics: {e}"))?;
    let mut out = Vec::new();
    for t in &s.topics {
        let found = topics.iter().find(|x| &x.name == t).ok_or_else(|| format!("there is no topic {t} on {}", s.brokers.join(",")))?;
        out.extend(found.partitions.iter().map(|p| (t.clone(), *p)));
    }
    Ok(out)
}

/// The offset of the next event a partition is read from: after the one
/// ChloeDB has, else where `s` starts.
async fn first_offset(pc: &PartitionClient, have: Option<i64>, start: Start) -> Result<i64, String> {
    if let Some(o) = have {
        return Ok(o + 1);
    }
    let at = match start {
        Start::Earliest => OffsetAt::Earliest,
        Start::Latest => OffsetAt::Latest,
    };
    pc.get_offset(at).await.map_err(|e| format!("{}/{}: its first offset: {e}", pc.topic(), pc.partition()))
}

/// Events of one partition, in order.
struct Chunk {
    topic: String,
    partition: i32,
    records: Vec<RecordAndOffset>,
}

/// Reads one partition from `next` on, into `tx`, until the session ends
/// (Ok) or the partition cannot be read.
async fn fetcher(pc: Arc<PartitionClient>, mut next: i64, s: Arc<Settings>, tx: mpsc::Sender<Chunk>, l: Arc<Live>) -> Result<(), String> {
    let (topic, p) = (pc.topic().to_string(), pc.partition());
    // rskafka retries a broker it lost for a while, reconnecting within
    // each retry: a fetch that has not answered 15 s after its wait ends
    // the session, which starts again from ChloeDB's offsets.
    let limit = Duration::from_millis(s.max_wait_ms as u64) + Duration::from_secs(15);
    loop {
        let fetched = tokio::time::timeout(limit, pc.fetch_records(next, 1..s.fetch_bytes, s.max_wait_ms))
            .await
            .map_err(|_| format!("fetching {topic}/{p} at offset {next}: no answer from the brokers in {} s", limit.as_secs()))?;
        match fetched {
            Ok((recs, high)) => {
                l.with(|st| {
                    if let Some(part) = st.parts.get_mut(&(topic.clone(), p)) {
                        part.high = high;
                    }
                });
                let mut recs: Vec<RecordAndOffset> = recs.into_iter().filter(|r| r.offset >= next).collect();
                let Some(last) = recs.last().map(|r| r.offset) else { continue };
                next = last + 1;
                while !recs.is_empty() {
                    let rest = if recs.len() > s.batch { recs.split_off(s.batch) } else { Vec::new() };
                    if tx.send(Chunk { topic: topic.clone(), partition: p, records: std::mem::replace(&mut recs, rest) }).await.is_err() {
                        return Ok(());
                    }
                }
            }
            Err(KafkaError::ServerError { protocol_error: ProtocolError::OffsetOutOfRange, .. }) => {
                let earliest = pc.get_offset(OffsetAt::Earliest).await.map_err(|e| format!("{topic}/{p}: its first offset: {e}"))?;
                let latest = pc.get_offset(OffsetAt::Latest).await.map_err(|e| format!("{topic}/{p}: its last offset: {e}"))?;
                if next < earliest {
                    let gone = (earliest - next) as u64;
                    tracing::warn!(topic = %topic, partition = p, from = next, to = earliest, "kafka connector: events deleted by the topic's retention before they were read");
                    l.with(|st| {
                        st.lost += gone;
                        if let Some(part) = st.parts.get_mut(&(topic.clone(), p)) {
                            part.lost += gone;
                        }
                        st.warning = Some(format!("{topic}/{p}: the events at offsets {next} to {} were deleted by the topic's retention before they were read ({gone} lost); reading on from {earliest}", earliest - 1));
                    });
                    next = earliest;
                } else {
                    return Err(format!(
                        "{topic}/{p}: ChloeDB has this partition's events up to offset {}, and the partition ends at {latest}: the topic was emptied or made again, and its new events would be taken for ones written already. Give the connector another \"prefix\" to read the topic from its start",
                        next - 1
                    ));
                }
            }
            Err(e) => return Err(format!("fetching {topic}/{p} at offset {next}: {e}")),
        }
    }
}

/// An event as a map reads it.
pub fn event(topic: &str, partition: i32, r: &RecordAndOffset, format: Format) -> Result<Json, (Json, String)> {
    let ts = r.record.timestamp.timestamp_millis();
    let key = r.record.key.as_ref().map(|k| String::from_utf8_lossy(k).into_owned());
    let headers: Map<String, Json> = r.record.headers.iter().map(|(k, v)| (k.clone(), Json::String(String::from_utf8_lossy(v).into_owned()))).collect();
    let mut e = json!({"topic": topic, "partition": partition, "offset": r.offset, "ts": (ts > 0).then_some(ts), "key": key, "value": null, "headers": headers});
    match (&r.record.value, format) {
        (None, _) => {}
        (Some(v), Format::Text) => e["value"] = Json::String(String::from_utf8_lossy(v).into_owned()),
        (Some(v), Format::Json) => match serde_json::from_slice::<Json>(v) {
            Ok(j) => e["value"] = j,
            Err(err) => {
                e["value"] = Json::String(String::from_utf8_lossy(&v[..v.len().min(200)]).into_owned());
                return Err((e, format!("{topic}/{partition} offset {}: the value is not JSON ({err})", r.offset)));
            }
        },
    }
    Ok(e)
}

/// The primary key column of `schema`, when it is text (a key the
/// event's place can stand in for).
fn text_key(schema: &Schema) -> Option<&str> {
    let f = schema.primary_key.and_then(|i| schema.fields.get(i))?;
    matches!(f.ty, FieldType::Keyword | FieldType::Text { .. }).then_some(f.name.as_str())
}

/// The row an event makes: none for a tombstone that writes nothing.
pub fn row_of(m: Option<&Map<String, Json>>, e: &Json, schema: Option<&Schema>, fitted: &mut map::Fitted, now_us: i64) -> Result<Option<Map<String, Json>>, String> {
    if e["value"].is_null() && m.is_none() {
        return Ok(None);
    }
    let mut row = match m {
        Some(m) => map::row(e, Some(m), now_us)?,
        None => match &e["value"] {
            Json::Object(o) => o.clone(),
            v => {
                let mut r = Map::new();
                r.insert("value".into(), v.clone());
                r
            }
        },
    };
    let Some(schema) = schema else { return Ok(Some(row)) };
    if let Some(pk) = text_key(schema) {
        let has = row.iter().any(|(k, v)| k.eq_ignore_ascii_case(pk) && !v.is_null() && v.as_str() != Some(""));
        if !has {
            row.retain(|k, _| !k.eq_ignore_ascii_case(pk));
            row.insert(pk.to_string(), json!(format!("{}/{}/{}", e["topic"].as_str().unwrap_or_default(), e["partition"], e["offset"])));
        }
    }
    map::fit(row, schema, fitted).map(Some)
}

/// What a write did, to count.
struct Wrote {
    events: u64,
    rows: u64,
    replays: u64,
    unmapped: u64,
    deleted: u64,
}

/// Writes `chunks`' events in one batch of the session's stream.
async fn write(svc: &Arc<Service>, c: &Connector, s: &Settings, schema: &Schema, stream: &mut Stream, chunks: Vec<Chunk>, l: &Live) -> Result<Wrote, String> {
    let now_us = now_ms() as i64 * 1000;
    let m = kinds::map_of(c);
    let keyed = schema.primary_key.is_some();
    let op = if keyed { "upsert" } else { "insert" };
    let mut fitted = map::Fitted::default();
    let mut ops = Vec::new();
    let mut got = Wrote { events: 0, rows: 0, replays: 0, unmapped: 0, deleted: 0 };
    let mut first_bad: Option<String> = None;
    let mut ends: Vec<(String, i32, i64, Option<i64>)> = Vec::new();
    for ch in &chunks {
        let name = s.source(&ch.topic, ch.partition);
        for r in &ch.records {
            got.events += 1;
            let ts = r.record.timestamp.timestamp_millis();
            let mut src = json!({"name": name, "offset": r.offset});
            if ts > 0 {
                src["ts"] = json!(ts);
            }
            let mark = || json!({"op": "source", "source": src.clone()});
            let this: Json = match event(&ch.topic, ch.partition, r, s.format) {
                Err((_, why)) => {
                    got.unmapped += 1;
                    first_bad.get_or_insert(why);
                    mark()
                }
                Ok(e) if e["value"].is_null() && s.delete_tombstones && keyed => match e["key"].as_str() {
                    Some(k) if !k.is_empty() => {
                        got.deleted += 1;
                        let typed = schema.primary_key.and_then(|i| schema.fields.get(i)).map(|f| matches!(f.ty, FieldType::Int)).unwrap_or(false);
                        let key = if typed { k.parse::<i64>().map(Json::from).unwrap_or_else(|_| json!(k)) } else { json!(k) };
                        json!({"op": "delete", "table": c.target, "key": key, "source": src.clone()})
                    }
                    _ => mark(),
                },
                Ok(e) => match row_of(m.as_ref(), &e, Some(schema), &mut fitted, now_us) {
                    Ok(Some(row)) => json!({"op": op, "table": c.target, "doc": row, "source": src.clone()}),
                    Ok(None) => mark(),
                    Err(why) => {
                        got.unmapped += 1;
                        first_bad.get_or_insert(format!("{}/{} offset {}: {why}", ch.topic, ch.partition, r.offset));
                        mark()
                    }
                },
            };
            let one = [this];
            stream.admit(&one).map_err(|e| e.message)?;
            let [this] = one;
            ops.push(this);
        }
        if let Some(last) = ch.records.last() {
            let ts = last.record.timestamp.timestamp_millis();
            ends.push((ch.topic.clone(), ch.partition, last.offset, (ts > 0).then_some(ts)));
        }
    }
    let w = svc.stream_write(stream, ops).await.map_err(|e| format!("writing {}: {}", c.target, e.message))?;
    got.rows = w.rows;
    got.replays = w.skipped;
    let now = now_ms();
    l.with(|st| {
        st.events += got.events;
        st.rows += got.rows;
        st.replays += got.replays;
        st.unmapped += got.unmapped;
        st.deleted += got.deleted;
        st.run_events += got.events;
        st.run_rows += got.rows;
        st.run_replays += got.replays;
        st.last_write_ms = now;
        if st.first_unmapped.is_none() {
            st.first_unmapped = first_bad.clone();
        }
        if !fitted.dropped.is_empty() && st.warning.is_none() {
            st.warning = Some(format!("not columns of {}, left out: {}", c.target, fitted.dropped.join(", ")));
        }
        for (t, p, off, ts) in ends {
            if let Some(part) = st.parts.get_mut(&(t, p)) {
                part.next = part.next.max(off + 1);
                if ts.is_some() {
                    part.last_ts = ts;
                }
            }
        }
        let sec = now / 1000;
        match st.window.back_mut() {
            Some((t, n)) if *t == sec => *n += got.events,
            _ => st.window.push_back((sec, got.events)),
        }
        while st.window.front().is_some_and(|(t, _)| sec.saturating_sub(*t) >= 60) {
            st.window.pop_front();
        }
    });
    Ok(got)
}

/// One session of a consumer: connected, it reads until it is stopped
/// (Ok) or something fails.
async fn session(svc: &Arc<Service>, c: &Connector, l: &Arc<Live>, stop: &mut watch::Receiver<bool>) -> Result<(), String> {
    l.set("connecting");
    l.with(|st| {
        st.sessions += 1;
        st.session_ms = now_ms();
        st.first_unmapped = None;
    });
    let s = Arc::new(Settings::of(c)?);
    let secrets = svc.connectors.secrets_of(&c.name);
    let schema = svc.table(&c.target).await.map_err(|e| format!("the target table {}: {}", c.target, e.message))?;
    let client = tokio::select! {
        r = connect(c, &s, &secrets) => r?,
        _ = stop.changed() => return Ok(()),
    };
    let have: HashMap<String, i64> = svc.sources().await.map_err(|e| format!("ChloeDB's offsets: {}", e.message))?.into_iter().map(|x| (x.name, x.offset)).collect();
    let (tx, mut rx) = mpsc::channel::<Chunk>(32);
    let mut fetchers = tokio::task::JoinSet::new();
    let mut started: HashSet<(String, i32)> = HashSet::new();
    // The partitions not read yet, each with where it starts.
    let begin = |parts: Vec<(String, i32)>, start: Start, started: &mut HashSet<(String, i32)>| -> Vec<(String, i32, Start)> {
        parts.into_iter().filter(|(t, p)| started.insert((t.clone(), *p))).map(|(t, p)| (t, p, start)).collect()
    };
    let todo = begin(partitions(&client, &s).await?, s.start, &mut started);
    for (t, p, start) in todo {
        let pc = Arc::new(client.partition_client(t.clone(), p, UnknownTopicHandling::Error).await.map_err(|e| format!("{t}/{p}: {e}"))?);
        let next = first_offset(&pc, have.get(&s.source(&t, p)).copied(), start).await?;
        l.with(|st| {
            let e = st.parts.entry((t.clone(), p)).or_default();
            e.topic = t.clone();
            e.partition = p;
            e.next = next;
            e.high = e.high.max(next);
        });
        fetchers.spawn(fetcher(pc, next, s.clone(), tx.clone(), l.clone()));
    }
    l.with(|st| {
        st.last_error = None;
        st.failures = 0;
        let keep: HashSet<(String, i32)> = started.clone();
        st.parts.retain(|k, _| keep.contains(k));
    });
    l.set("streaming");
    // Its health in the state table turns from failing at once.
    save(svc, c, l, None).await;
    let mut stream = Stream::default();
    let mut refresh = tokio::time::interval(Duration::from_secs(30));
    refresh.tick().await;
    let mut saving = tokio::time::interval(Duration::from_secs(10));
    saving.tick().await;
    loop {
        let first = tokio::select! {
            ch = rx.recv() => match ch {
                Some(ch) => ch,
                None => return Err("the partitions' readers stopped".into()),
            },
            Some(r) = fetchers.join_next() => {
                return Err(match r {
                    Ok(Err(e)) => e,
                    Ok(Ok(())) => "a partition's reader stopped".into(),
                    Err(e) => format!("a partition's reader failed: {e}"),
                });
            }
            _ = refresh.tick() => {
                // Partitions added to the topics since: read from their start.
                let parts = partitions(&client, &s).await?;
                let todo = begin(parts, Start::Earliest, &mut started);
                for (t, p, start) in todo {
                    let pc = Arc::new(client.partition_client(t.clone(), p, UnknownTopicHandling::Error).await.map_err(|e| format!("{t}/{p}: {e}"))?);
                    let next = first_offset(&pc, have.get(&s.source(&t, p)).copied(), start).await?;
                    l.with(|st| {
                        let e = st.parts.entry((t.clone(), p)).or_default();
                        e.topic = t.clone();
                        e.partition = p;
                        e.next = next;
                    });
                    fetchers.spawn(fetcher(pc, next, s.clone(), tx.clone(), l.clone()));
                }
                continue;
            }
            _ = saving.tick() => {
                save(svc, c, l, None).await;
                continue;
            }
            _ = stop.changed() => return Ok(()),
        };
        let mut chunks = vec![first];
        let mut n = chunks[0].records.len();
        while n < s.batch {
            match rx.try_recv() {
                Ok(ch) => {
                    n += ch.records.len();
                    chunks.push(ch);
                }
                Err(_) => break,
            }
        }
        write(svc, c, &s, &schema, &mut stream, chunks, l).await?;
    }
}

/// Saves the consumer's counts to the state table, and a run row every 10
/// minutes and when a session ends (`ended`: with its error, if any).
async fn save(svc: &Arc<Service>, c: &Connector, l: &Live, ended: Option<Option<String>>) {
    let me = &svc.connectors;
    if me.make_tables(svc).await.is_err() {
        return;
    }
    let now = now_ms();
    let (d_events, d_rows, cursor, error, warning, run) = l.with(|s| {
        let d = (s.events - s.saved_events, s.rows - s.saved_rows);
        s.saved_events = s.events;
        s.saved_rows = s.rows;
        let cursor: Map<String, Json> = s.parts.values().map(|p| (format!("{}/{}", p.topic, p.partition), json!(p.next))).collect();
        let due = ended.is_some() || now.saturating_sub(s.run_from_ms) >= 600_000;
        let run = (due && (s.run_events > 0 || ended.is_some())).then(|| {
            let r = (s.run_from_ms, s.run_events, s.run_rows, s.run_replays);
            s.run_from_ms = now;
            s.run_events = 0;
            s.run_rows = 0;
            s.run_replays = 0;
            r
        });
        (d.0, d.1, Json::Object(cursor).to_string(), s.last_error.clone(), s.warning.clone(), run)
    });
    let mut st: State = me.state_of(&c.name);
    st.records_total += d_events;
    st.rows_total += d_rows;
    st.last_rows = d_rows;
    st.cursor = Some(cursor.clone());
    st.last_run_ms = l.with(|s| s.session_ms);
    match &ended {
        Some(Some(e)) => {
            st.failures += 1;
            st.last_error = Some(e.clone());
        }
        _ if error.is_none() => {
            st.failures = 0;
            st.last_error = None;
            st.last_ok_ms = now;
        }
        _ => {}
    }
    if ended.is_none() && l.state() == "streaming" {
        st.runs = st.runs.max(1);
    }
    st.last_warning = warning.clone();
    lock(&me.state).insert(c.name.clone(), st.clone());
    let mut ops = vec![json!({"op": "upsert", "table": STATE, "doc": st.row(&c.name)})];
    if let Some((from, events, rows, replays)) = run {
        st.runs += 1;
        lock(&me.state).insert(c.name.clone(), st.clone());
        ops[0] = json!({"op": "upsert", "table": STATE, "doc": st.row(&c.name)});
        let err = match &ended {
            Some(Some(e)) => e.clone(),
            _ => String::new(),
        };
        let id = format!("{}/{from}/{}", c.name, crate::keys::random_hex(3).unwrap_or_default());
        ops.push(json!({
            "op": "insert", "table": RUNS,
            "doc": {
                "id": id, "connector": c.name, "kind": c.kind, "target": c.target, "trigger": "stream",
                "started": chloedb::timestamp::format(from as i64 * 1000), "ended": chloedb::timestamp::format(now as i64 * 1000),
                "took_ms": now.saturating_sub(from), "ok": err.is_empty(), "records": events, "rows_": rows, "skipped": replays,
                "dropped": Vec::<String>::new(), "error": err, "cursor": cursor, "more": false, "warning": warning.unwrap_or_default(),
            }
        }));
    }
    if let Err(e) = me.own_write(svc, ops).await {
        tracing::warn!(connector = %c.name, error = %e, "cannot record a kafka connector's state");
    }
}

/// A dry run: up to `limit` events of each partition, from where the
/// consumer would read next, else the last ones the topics hold, as the
/// map reads them; nothing written.
pub async fn sample(c: &Connector, secrets: &HashMap<String, String>, have: &HashMap<String, i64>, limit: usize) -> Result<Fetched, String> {
    let s = Settings::of(c)?;
    let client = connect(c, &s, secrets).await?;
    let parts = partitions(&client, &s).await?;
    let mut records = Vec::new();
    let mut notes = Vec::new();
    let mut waiting = 0i64;
    for (t, p) in parts.iter().take(16) {
        let pc = client.partition_client(t.clone(), *p, UnknownTopicHandling::Error).await.map_err(|e| format!("{t}/{p}: {e}"))?;
        let next = first_offset(&pc, have.get(&s.source(t, *p)).copied(), s.start).await?;
        let latest = pc.get_offset(OffsetAt::Latest).await.map_err(|e| format!("{t}/{p}: {e}"))?;
        let earliest = pc.get_offset(OffsetAt::Earliest).await.map_err(|e| format!("{t}/{p}: {e}"))?;
        waiting += (latest - next).max(0);
        // Nothing to read next: the last events, to try the map on.
        let from = if next < latest { next.max(earliest) } else { (latest - limit as i64).max(earliest) };
        if from >= latest {
            continue;
        }
        let (recs, _) = pc.fetch_records(from, 1..s.fetch_bytes.min(4 << 20), 1000).await.map_err(|e| format!("fetching {t}/{p}: {e}"))?;
        for r in recs.iter().filter(|r| r.offset >= from).take(limit.saturating_sub(records.len() / parts.len().max(1)).max(1)) {
            records.push(match event(t, *p, r, s.format) {
                Ok(e) => e,
                Err((e, why)) => {
                    notes.push(why);
                    e
                }
            });
            if records.len() >= limit * 4 {
                break;
            }
        }
    }
    let warning = if records.is_empty() {
        Some(format!("the topics hold no events yet ({} partitions)", parts.len()))
    } else {
        notes.first().cloned()
    };
    Ok(Fetched { records, cursor: Some(json!({"partitions": parts.len(), "to_read": waiting}).to_string()), more: waiting > 0, warning })
}
