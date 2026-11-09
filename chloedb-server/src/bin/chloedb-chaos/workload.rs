//! The clients: readers that search and read rows by key, and writers that
//! add 1 to counters, each through the coordinators (round robin; a
//! coordinator that refuses the connection is skipped, as a load balancer
//! would), with what each second saw kept by kind and outcome.
//!
//! The writers keep a ledger, the checker's truth: per counter, the adds
//! acknowledged (answered 200) and those that may have applied (answered
//! with an error after which the add may still be durable: in doubt, a
//! timeout, a dropped connection). A counter's value must lie between the
//! two: below the acknowledged count, an acknowledged write was lost;
//! above the count that may have applied, a write applied twice. A write
//! answered otherwise is sent again under its idempotency key for up to 8 s.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rand::{Rng, SeedableRng};
use serde_json::{json, Value as Json};

use crate::observe::Events;

pub const KINDS: [&str; 5] = ["search", "get", "write", "flood", "hot"];

const WORDS: [&str; 24] = [
    "tent", "backpack", "espresso", "kettle", "lamp", "chair", "jacket", "headphones", "knife", "boots", "mug", "blender", "desk", "pillow", "speaker", "wool", "steel", "leather", "bamboo", "quiet", "portable", "waterproof", "ceramic", "cotton",
];

#[derive(Default, Clone)]
pub struct Bucket {
    pub ok: u64,
    pub errors: BTreeMap<String, u64>,
    pub lat_ms: Vec<f32>,
}

/// Each second's buckets, by kind; and the latest error text of each class.
#[derive(Default)]
pub struct Stats {
    secs: Mutex<BTreeMap<u64, BTreeMap<&'static str, Bucket>>>,
    pub samples: Mutex<BTreeMap<String, (String, f64)>>,
}

impl Stats {
    fn add(&self, sec: u64, kind: &'static str, outcome: Result<(), &str>, ms: f32) {
        let mut s = self.secs.lock().unwrap();
        let b = s.entry(sec).or_default().entry(kind).or_default();
        match outcome {
            Ok(()) => b.ok += 1,
            Err(c) => *b.errors.entry(c.to_string()).or_default() += 1,
        }
        if b.lat_ms.len() < 20_000 {
            b.lat_ms.push(ms);
        }
    }

    /// What the clients saw from second `from` to `to` (excluded).
    pub fn summary(&self, from: u64, to: u64) -> Json {
        let s = self.secs.lock().unwrap();
        let mut out = serde_json::Map::new();
        for kind in KINDS {
            let (mut ok, mut errs, mut lat) = (0u64, BTreeMap::<String, u64>::new(), Vec::<f32>::new());
            let (mut bad_secs, mut first_bad, mut last_bad) = (0u64, None, None);
            let mut worst = 0.0f64;
            for (&sec, kinds) in s.range(from..to) {
                let Some(b) = kinds.get(kind) else { continue };
                ok += b.ok;
                let e: u64 = b.errors.values().sum();
                for (c, n) in &b.errors {
                    *errs.entry(c.clone()).or_default() += n;
                }
                lat.extend_from_slice(&b.lat_ms);
                let ratio = e as f64 / (e + b.ok).max(1) as f64;
                worst = worst.max(ratio);
                if e > 0 {
                    bad_secs += 1;
                    first_bad.get_or_insert(sec);
                    last_bad = Some(sec);
                }
            }
            let failed: u64 = errs.values().sum();
            if ok + failed == 0 {
                continue;
            }
            lat.sort_by(|a, b| a.total_cmp(b));
            let q = |p: f64| if lat.is_empty() { 0.0 } else { lat[((lat.len() - 1) as f64 * p).round() as usize] as f64 };
            out.insert(kind.into(), json!({
                "calls": ok + failed, "ok": ok, "failed": failed, "error_ratio": round4(failed as f64 / (ok + failed) as f64),
                "errors": errs, "p50_ms": round1(q(0.5)), "p99_ms": round1(q(0.99)), "max_ms": round1(q(1.0)),
                "seconds_with_errors": bad_secs, "worst_second_error_ratio": round4(worst),
                "errors_from_s": first_bad, "errors_to_s": last_bad.map(|x| x + 1),
            }));
        }
        Json::Object(out)
    }

    /// Per second: each kind's calls, failures and p99, for a chart.
    pub fn series(&self, from: u64, to: u64) -> Json {
        let s = self.secs.lock().unwrap();
        let rows: Vec<Json> = s
            .range(from..to)
            .map(|(&sec, kinds)| {
                let mut row = serde_json::Map::new();
                row.insert("s".into(), sec.into());
                for (k, b) in kinds {
                    let mut l = b.lat_ms.clone();
                    l.sort_by(|a, b| a.total_cmp(b));
                    let p99 = if l.is_empty() { 0.0 } else { l[((l.len() - 1) as f64 * 0.99).round() as usize] };
                    row.insert((*k).into(), json!([b.ok, b.errors.values().sum::<u64>(), round1(p99 as f64)]));
                }
                Json::Object(row)
            })
            .collect();
        Json::Array(rows)
    }
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}
fn round4(x: f64) -> f64 {
    (x * 10_000.0).round() / 10_000.0
}

/// The writers' ledger: per counter, (acknowledged, may have applied).
pub struct Ledger {
    pub keys: Vec<String>,
    counts: Mutex<Vec<(u64, u64)>>,
}

pub struct Config {
    pub readers: usize,
    pub writers: usize,
    /// Calls a second, each reader and each writer.
    pub rate: f64,
    pub counters: usize,
    pub rows: usize,
    pub seed: u64,
}

pub struct Workload {
    pub coordinators: Vec<String>,
    http: reqwest::Client,
    pub stats: Stats,
    pub ledger: Ledger,
    pub events: Arc<Events>,
    rr: AtomicUsize,
    /// Writers wait while it is set (the checker reads the counters).
    pub hold_writes: AtomicBool,
    pub inflight_writes: AtomicU64,
    pub stop: AtomicBool,
    pub rate_milli: AtomicU64,
    rows: usize,
    /// The keys clients send (`Authorization: Bearer`), by client.
    pub keys: Mutex<BTreeMap<String, String>>,
    seq: AtomicU64,
}

#[derive(Debug)]
enum Fail {
    /// The class of the failure, its text, and whether the call may
    /// have applied a write.
    Http(String, String, bool),
}

impl Workload {
    pub fn new(coordinators: Vec<String>, cfg: &Config, events: Arc<Events>, keys: BTreeMap<String, String>) -> Arc<Workload> {
        let http = chloedb_server::http_client().pool_max_idle_per_host(64).timeout(Duration::from_secs(10)).connect_timeout(Duration::from_secs(1)).build().unwrap();
        let ledger = Ledger { keys: (0..cfg.counters).map(|i| format!("chaos-c{i:04}")).collect(), counts: Mutex::new(vec![(0, 0); cfg.counters]) };
        Arc::new(Workload {
            coordinators,
            http,
            stats: Stats::default(),
            ledger,
            events,
            rr: AtomicUsize::new(0),
            hold_writes: AtomicBool::new(false),
            inflight_writes: AtomicU64::new(0),
            stop: AtomicBool::new(false),
            rate_milli: AtomicU64::new((cfg.rate * 1000.0) as u64),
            rows: cfg.rows,
            keys: Mutex::new(keys),
            seq: AtomicU64::new(0),
        })
    }

    fn sec(&self) -> u64 {
        self.events.start.elapsed().as_secs()
    }

    fn key(&self, client: &str) -> String {
        self.keys.lock().unwrap().get(client).cloned().unwrap_or_default()
    }

    /// One call through the coordinators: the next one in turn, then, when
    /// it refuses the connection, the one after it.
    async fn call(&self, client: &str, method: reqwest::Method, path: &str, body: Option<&Json>, timeout: Duration) -> Result<Json, Fail> {
        let n = self.coordinators.len();
        let first = self.rr.fetch_add(1, Ordering::Relaxed);
        let key = self.key(client);
        let mut last = None;
        for i in 0..n.min(2) {
            let base = &self.coordinators[(first + i) % n];
            let mut req = self.http.request(method.clone(), format!("{base}{path}")).timeout(timeout).header("x-strata-client", client);
            if !key.is_empty() {
                req = req.header("authorization", format!("Bearer {key}"));
            }
            if let Some(b) = body {
                req = req.json(b);
            }
            match req.send().await {
                Ok(r) => {
                    let st = r.status();
                    let text = r.text().await.unwrap_or_default();
                    let j: Json = serde_json::from_str(&text).unwrap_or(Json::Null);
                    if st.is_success() {
                        return Ok(j);
                    }
                    if st.as_u16() == 404 && j.get("found").is_some() {
                        return Ok(j);
                    }
                    let msg = j["error"].as_str().map(str::to_string).unwrap_or_else(|| text.chars().take(300).collect());
                    // Whether the write may have applied: a 503 and a 429
                    // say nothing was written; another server error may
                    // come after the write applied somewhere.
                    let doubt = (st.as_u16() >= 500 && st.as_u16() != 503) || (st.as_u16() == 409 && msg.contains("before it was acknowledged"));
                    let class = if st.as_u16() == 409 && msg.contains("before it was acknowledged") { "409 in doubt".to_string() } else { st.as_u16().to_string() };
                    return Err(Fail::Http(class, msg, doubt));
                }
                Err(e) if e.is_connect() => {
                    last = Some(Fail::Http("connect".into(), format!("{base}: {e}"), false));
                    continue;
                }
                Err(e) => {
                    let class = if e.is_timeout() { "timeout" } else { "connection" };
                    return Err(Fail::Http(class.into(), format!("{base}: {e}"), true));
                }
            }
        }
        Err(last.unwrap_or(Fail::Http("connect".into(), "no coordinator".into(), false)))
    }

    fn note(&self, kind: &'static str, sec: u64, t: Instant, r: &Result<Json, Fail>) {
        let ms = t.elapsed().as_secs_f32() * 1000.0;
        match r {
            Ok(j) => {
                // An answer that left out shards is a failure for the reader.
                if j["partial"].is_object() {
                    self.stats.add(sec, kind, Err("partial"), ms);
                    self.sample("partial", &j["partial"].to_string());
                } else {
                    self.stats.add(sec, kind, Ok(()), ms);
                }
            }
            Err(Fail::Http(c, m, _)) => {
                self.stats.add(sec, kind, Err(c), ms);
                self.sample(&format!("{kind} {c}"), m);
            }
        }
    }

    fn sample(&self, class: &str, msg: &str) {
        self.stats.samples.lock().unwrap().insert(class.to_string(), (msg.chars().take(400).collect(), self.events.t()));
    }

    async fn pace(&self, started: Instant) {
        let rate = self.rate_milli.load(Ordering::Relaxed) as f64 / 1000.0;
        if rate <= 0.0 {
            tokio::time::sleep(Duration::from_millis(500)).await;
            return;
        }
        let gap = Duration::from_secs_f64(1.0 / rate);
        if let Some(rest) = gap.checked_sub(started.elapsed()) {
            tokio::time::sleep(rest).await;
        }
    }

    pub async fn reader(self: Arc<Self>, seed: u64) {
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(seed);
        while !self.stop.load(Ordering::Relaxed) {
            let t = Instant::now();
            let sec = self.sec();
            if rng.gen_bool(0.7) {
                let mut q = WORDS[rng.gen_range(0..WORDS.len())].to_string();
                if rng.gen_bool(0.5) {
                    q = format!("{q} {}", WORDS[rng.gen_range(0..WORDS.len())]);
                }
                let body = json!({"sql": "SELECT id, title, score() FROM products WHERE MATCH(body, :q) ORDER BY bm25(body, :q) DESC LIMIT 10", "params": {"q": q}, "timeout_ms": 5000});
                let r = self.call("app-search", reqwest::Method::POST, "/v1/query", Some(&body), Duration::from_secs(6)).await;
                self.note("search", sec, t, &r);
            } else {
                let i = rng.gen_range(0..self.rows.max(1));
                let r = self.call("app-search", reqwest::Method::GET, &format!("/v1/kv/products/p{i:07}"), None, Duration::from_secs(6)).await;
                self.note("get", sec, t, &r);
            }
            self.pace(t).await;
        }
    }

    pub async fn writer(self: Arc<Self>, id: usize, seed: u64) {
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(seed ^ 0x5eed_0000 ^ id as u64);
        while !self.stop.load(Ordering::Relaxed) {
            if self.hold_writes.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            }
            let t = Instant::now();
            let k = rng.gen_range(0..self.ledger.keys.len());
            self.inflight_writes.fetch_add(1, Ordering::SeqCst);
            let key = self.ledger.keys[k].clone();
            let n = self.seq.fetch_add(1, Ordering::Relaxed);
            let acked = self.add_one("write", "app-writer", &key, &format!("w{id}-{n}")).await;
            {
                let mut c = self.ledger.counts.lock().unwrap();
                match acked {
                    Some(true) => c[k].0 += 1,
                    Some(false) => c[k].1 += 1,
                    None => {}
                }
            }
            self.inflight_writes.fetch_sub(1, Ordering::SeqCst);
            self.pace(t).await;
        }
    }

    /// Adds 1 to a counter, sending it again under its idempotency key
    /// while that is safe: `Some(true)` acknowledged, `Some(false)` may
    /// have applied, `None` surely not applied.
    async fn add_one(&self, kind: &'static str, client: &str, key: &str, idem: &str) -> Option<bool> {
        let body = json!({"ops": [{"op": "add", "field": "sold", "value": 1, "doc": {"id": key, "title": format!("chaos counter {key}")}}], "idempotency_key": idem, "timeout_ms": 5000});
        let first = Instant::now();
        let mut maybe = false;
        loop {
            let t = Instant::now();
            let sec = self.sec();
            let r = self.call(client, reqwest::Method::POST, "/v1/write", Some(&body), Duration::from_secs(6)).await;
            self.note(kind, sec, t, &r);
            match r {
                Ok(_) => return Some(true),
                Err(Fail::Http(class, _, doubt)) => {
                    maybe |= doubt;
                    // In doubt: sending it again could apply it twice.
                    if class == "409 in doubt" || class == "400" || class == "422" || class == "401" || class == "403" {
                        return if maybe { Some(false) } else { None };
                    }
                    if first.elapsed() > Duration::from_secs(8) || self.stop.load(Ordering::Relaxed) {
                        return if maybe { Some(false) } else { None };
                    }
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
            }
        }
    }

    /// A client that floods the cluster with heavy queries until `stop`.
    pub async fn flooder(self: Arc<Self>, stop: Arc<AtomicBool>, sql: String) {
        while !stop.load(Ordering::Relaxed) && !self.stop.load(Ordering::Relaxed) {
            let t = Instant::now();
            let sec = self.sec();
            let body = json!({"sql": sql, "timeout_ms": 10000});
            let r = self.call("nightly-report", reqwest::Method::POST, "/v1/query", Some(&body), Duration::from_secs(12)).await;
            self.note("flood", sec, t, &r);
            if r.is_err() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
    }

    /// A writer that adds to a few keys, as fast as they are answered.
    pub async fn hot_writer(self: Arc<Self>, stop: Arc<AtomicBool>, keys: Vec<String>, id: usize) {
        let mut i = 0usize;
        while !stop.load(Ordering::Relaxed) && !self.stop.load(Ordering::Relaxed) {
            let key = &keys[i % keys.len()];
            i += 1;
            let n = self.seq.fetch_add(1, Ordering::Relaxed);
            let _ = self.add_one("hot", "app-writer", key, &format!("h{id}-{n}")).await;
        }
    }

    /// Reads every counter and compares it with the ledger, writes held
    /// meanwhile. A counter below what was acknowledged is read again a
    /// few seconds later (a replica may not have caught up) before it is
    /// counted as lost.
    pub async fn check(&self) -> Json {
        self.hold_writes.store(true, Ordering::SeqCst);
        let t = Instant::now();
        while self.inflight_writes.load(Ordering::SeqCst) > 0 && t.elapsed() < Duration::from_secs(20) {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
        let mut result = self.compare().await;
        for _ in 0..3 {
            if result["lost"].as_array().is_none_or(|l| l.is_empty()) && result["error"].is_null() {
                break;
            }
            tokio::time::sleep(Duration::from_secs(3)).await;
            result = self.compare().await;
        }
        self.hold_writes.store(false, Ordering::SeqCst);
        result
    }

    async fn compare(&self) -> Json {
        let keys = &self.ledger.keys;
        let mut values = Vec::with_capacity(keys.len());
        for chunk in keys.chunks(1000) {
            let body = json!({"table": "products", "keys": chunk, "columns": ["sold"], "timeout_ms": 10000});
            match self.call("app-search", reqwest::Method::POST, "/v1/kv/get", Some(&body), Duration::from_secs(12)).await {
                Ok(j) => {
                    for r in j["rows"].as_array().into_iter().flatten() {
                        values.push(r["sold"].as_u64().unwrap_or(0));
                    }
                }
                Err(Fail::Http(c, m, _)) => return json!({"error": format!("{c}: {m}")}),
            }
        }
        if values.len() != keys.len() {
            return json!({"error": format!("read {} counters of {}", values.len(), keys.len())});
        }
        let counts = self.ledger.counts.lock().unwrap().clone();
        let (mut lost, mut double) = (Vec::new(), Vec::new());
        let (mut acked, mut maybe, mut total) = (0u64, 0u64, 0u64);
        for (i, (&(a, m), &v)) in counts.iter().zip(&values).enumerate() {
            acked += a;
            maybe += m;
            total += v;
            if v < a {
                lost.push(json!({"key": keys[i], "acknowledged": a, "may_have_applied": m, "value": v}));
            } else if v > a + m {
                double.push(json!({"key": keys[i], "acknowledged": a, "may_have_applied": m, "value": v}));
            }
        }
        json!({
            "counters": keys.len(), "acknowledged": acked, "may_have_applied": maybe, "sum": total,
            "lost": lost, "applied_twice": double, "ok": lost.is_empty() && double.is_empty(),
        })
    }
}
