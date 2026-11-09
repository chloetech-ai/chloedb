//! Counters and latency histograms, in the Prometheus text format
//! (`GET /metrics`): requests by route and status, queries by kind,
//! writes, embeddings, and (from [`crate::service::Service::prometheus`])
//! the database, its merges, a coordinator's shards and the process.
//! Durations are in seconds, and every family has `# HELP` and `# TYPE`.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::sync::Mutex;
use std::time::SystemTime;

use chloedb::query::{OrderBy, Projection, Score, Select};

/// Upper bounds of the latency buckets, in seconds.
pub const BUCKETS: [f64; 14] = [0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0];

/// Upper bounds of the stream lag buckets, in seconds: a stream catching
/// up may be hours behind.
const LAG_BUCKETS: [f64; 18] = [0.001, 0.002, 0.005, 0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0, 300.0, 900.0, 3600.0];

struct Histogram {
    bounds: &'static [f64],
    counts: Vec<u64>,
    sum: f64,
    n: u64,
}

impl Default for Histogram {
    fn default() -> Histogram {
        Histogram::with(&BUCKETS)
    }
}

/// The stream lag histogram, in [`LAG_BUCKETS`].
struct Lag(Histogram);

impl Default for Lag {
    fn default() -> Lag {
        Lag(Histogram::with(&LAG_BUCKETS))
    }
}

impl Histogram {
    fn with(bounds: &'static [f64]) -> Histogram {
        Histogram { bounds, counts: vec![0; bounds.len()], sum: 0.0, n: 0 }
    }

    /// The counts per bucket, and last those past every bound.
    fn counts(&self) -> Vec<u64> {
        let mut c = self.counts.clone();
        c.push(self.n - self.counts.iter().sum::<u64>());
        c
    }

    /// Adds its counts per bucket into `into` (counts of the same bounds).
    fn add_to(&self, into: &mut Vec<u64>) {
        let c = self.counts();
        if into.len() < c.len() {
            into.resize(c.len(), 0);
        }
        for (a, b) in into.iter_mut().zip(c) {
            *a += b;
        }
    }

    fn observe(&mut self, secs: f64) {
        if let Some(i) = self.bounds.iter().position(|&b| secs <= b) {
            self.counts[i] += 1;
        }
        self.sum += secs;
        self.n += 1;
    }

    /// The series of the histogram `name`, with `labels` (`k="v",...` or
    /// empty) on each.
    fn render(&self, out: &mut String, name: &str, labels: &str) {
        let sep = if labels.is_empty() { "" } else { "," };
        let mut cum = 0;
        for (b, c) in self.bounds.iter().zip(&self.counts) {
            cum += c;
            let _ = writeln!(out, "{name}_bucket{{{labels}{sep}le=\"{b}\"}} {cum}");
        }
        let _ = writeln!(out, "{name}_bucket{{{labels}{sep}le=\"+Inf\"}} {}", self.n);
        let braces = |l: &str| if l.is_empty() { String::new() } else { format!("{{{l}}}") };
        let _ = writeln!(out, "{name}_sum{} {}", braces(labels), self.sum);
        let _ = writeln!(out, "{name}_count{} {}", braces(labels), self.n);
    }
}

#[derive(Default)]
struct Inner {
    /// (route, status) → requests.
    http: BTreeMap<(&'static str, u16), u64>,
    http_seconds: BTreeMap<&'static str, Histogram>,
    /// The routes' names, each kept once ([`Metrics::http`]).
    routes: std::collections::BTreeSet<&'static str>,
    /// (method, code) → calls.
    grpc: BTreeMap<(&'static str, &'static str), u64>,
    grpc_seconds: BTreeMap<&'static str, Histogram>,
    /// (kind, outcome) → count.
    queries: BTreeMap<(&'static str, &'static str), u64>,
    query_seconds: BTreeMap<&'static str, Histogram>,
    /// The slowest query since the series' last sample, in seconds
    /// ([`Metrics::take_query_max`]): no quantile of a point exceeds it.
    query_max: Option<f64>,
    admission_seconds: Histogram,
    rejected: u64,
    writes: u64,
    written_docs: u64,
    /// HTTP status → failed writes.
    write_errors: BTreeMap<u16, u64>,
    write_seconds: Histogram,
    lag_seconds: Lag,
    replays: u64,
    /// "query" or "write" → requests slower than the slow log's threshold.
    slow: BTreeMap<&'static str, u64>,
    /// "query" or "write" → texts embedded.
    embedded: BTreeMap<&'static str, u64>,
    embed_seconds: BTreeMap<&'static str, Histogram>,
    agent_runs: u64,
}

pub struct Metrics {
    inner: Mutex<Inner>,
    started: SystemTime,
    /// What [`Metrics::routes`] counts, a minute apart (an hour apart past
    /// [`KEEP_MINUTES`]), so that it answers for a window.
    kept: Mutex<std::collections::VecDeque<(u64, std::sync::Arc<Counts>)>>,
}

impl Default for Metrics {
    fn default() -> Metrics {
        Metrics { inner: Mutex::default(), started: SystemTime::now(), kept: Mutex::default() }
    }
}

/// How long the counts of [`Metrics::routes`] are kept a minute apart;
/// an hour apart after that, up to [`KEEP_HOURS`].
const KEEP_MINUTES: u64 = 6 * 60;
const KEEP_HOURS: u64 = 7 * 24;

/// A histogram's counts per bucket (and past every bound), sum and count.
type Hist = (Vec<u64>, f64, u64);

/// What [`Metrics::routes`] counts, at one time.
#[derive(Clone, Default)]
struct Counts {
    http: BTreeMap<(&'static str, u16), u64>,
    http_seconds: BTreeMap<&'static str, Hist>,
    queries: BTreeMap<(&'static str, &'static str), u64>,
    query_seconds: BTreeMap<&'static str, Hist>,
    admission: Hist,
}

impl Counts {
    fn of(m: &Inner) -> Counts {
        let hist = |h: &Histogram| (h.counts(), h.sum, h.n);
        Counts {
            http: m.http.clone(),
            http_seconds: m.http_seconds.iter().map(|(k, h)| (*k, hist(h))).collect(),
            queries: m.queries.clone(),
            query_seconds: m.query_seconds.iter().map(|(k, h)| (*k, hist(h))).collect(),
            admission: hist(&m.admission_seconds),
        }
    }

    /// These counts less `base`'s (an earlier time's): what happened between.
    fn since(&self, base: &Counts) -> Counts {
        fn h(a: &Hist, b: Option<&Hist>) -> Hist {
            let Some(b) = b else { return a.clone() };
            (a.0.iter().enumerate().map(|(i, n)| n.saturating_sub(b.0.get(i).copied().unwrap_or(0))).collect(), (a.1 - b.1).max(0.0), a.2.saturating_sub(b.2))
        }
        Counts {
            http: self.http.iter().map(|(k, n)| (k.clone(), n.saturating_sub(base.http.get(k).copied().unwrap_or(0)))).filter(|(_, n)| *n > 0).collect(),
            http_seconds: self.http_seconds.iter().map(|(k, x)| (*k, h(x, base.http_seconds.get(k)))).filter(|(_, x)| x.2 > 0).collect(),
            queries: self.queries.iter().map(|(k, n)| (*k, n.saturating_sub(base.queries.get(k).copied().unwrap_or(0)))).filter(|(_, n)| *n > 0).collect(),
            query_seconds: self.query_seconds.iter().map(|(k, x)| (*k, h(x, base.query_seconds.get(k)))).filter(|(_, x)| x.2 > 0).collect(),
            admission: h(&self.admission, Some(&base.admission)),
        }
    }

    /// As [`Metrics::routes`] answers.
    fn json(&self) -> serde_json::Value {
        use crate::telemetry::quantile;
        let mut by: BTreeMap<&str, (u64, u64, u64)> = BTreeMap::new();
        for ((route, status), &n) in &self.http {
            let e = by.entry(*route).or_default();
            e.0 += n;
            if (400..500).contains(status) {
                e.1 += n;
            }
            if *status >= 500 {
                e.2 += n;
            }
        }
        let routes: Vec<serde_json::Value> = by
            .into_iter()
            .map(|(route, (n, c4, c5))| {
                let h = self.http_seconds.get(route);
                let p99 = h.and_then(|h| quantile(&BUCKETS, &h.0, 0.99)).map(|s| s * 1e3);
                let mean = h.map(|h| h.1 / h.2.max(1) as f64 * 1e3);
                serde_json::json!({"route": route, "requests": n, "client_errors": c4, "server_errors": c5, "mean_ms": mean, "p99_ms": p99})
            })
            .collect();
        let kinds: Vec<serde_json::Value> = self
            .query_seconds
            .iter()
            .map(|(kind, h)| {
                let by: serde_json::Map<String, serde_json::Value> = self.queries.iter().filter(|((k, _), _)| k == kind).map(|((_, o), n)| (o.to_string(), serde_json::json!(n))).collect();
                serde_json::json!({
                    "kind": kind, "queries": h.2, "outcomes": by, "mean_ms": h.1 / h.2.max(1) as f64 * 1e3,
                    "p50_ms": quantile(&BUCKETS, &h.0, 0.5).map(|s| s * 1e3),
                    "p99_ms": quantile(&BUCKETS, &h.0, 0.99).map(|s| s * 1e3),
                    "hist": h.0,
                })
            })
            .collect();
        serde_json::json!({"http": routes, "kinds": kinds, "buckets_s": BUCKETS, "admission_p99_ms": quantile(&BUCKETS, &self.admission.0, 0.99).map(|s| s * 1e3)})
    }
}

/// A series family the service measures when it is scraped (a gauge, or a
/// counter kept elsewhere): one value per label set (`k="v",...` or "").
pub struct Family {
    pub name: &'static str,
    pub ty: &'static str,
    pub help: &'static str,
    pub values: Vec<(String, f64)>,
}

impl Family {
    pub fn one(name: &'static str, ty: &'static str, help: &'static str, v: f64) -> Family {
        Family { name, ty, help, values: vec![(String::new(), v)] }
    }
}

/// The kind of a query, as its ranking or shape.
pub fn kind(q: &Select) -> &'static str {
    if !q.group_by.is_empty() {
        return if q.projection.iter().all(|(_, p)| matches!(p, Projection::Field(_) | Projection::Count)) { "facet" } else { "aggregate" };
    }
    if q.projection.iter().any(|(_, p)| matches!(p, Projection::Agg { .. })) {
        return "aggregate";
    }
    if q.projection.iter().all(|(_, p)| matches!(p, Projection::Count)) {
        return "count";
    }
    if !q.phases.is_empty() {
        return "reranked";
    }
    match &q.order {
        Some(OrderBy::Score { score, .. }) => match score {
            Score::Bm25 { .. } => "bm25",
            Score::Similarity { .. } | Score::MaxSim { .. } => "vector",
            Score::Sparse { .. } => "sparse",
            Score::Rrf { .. } | Score::Weighted(_) => "hybrid",
            Score::Column { .. } => "column",
            Score::Decay { .. } => "decay",
            Score::Product(_) => "boosted",
            Score::Ppr { .. } | Score::Hops { .. } => "graph",
            _ => "expression",
        },
        _ => "filter",
    }
}

/// A label value escaped for the text format.
pub fn esc(v: &str) -> String {
    v.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

fn head(out: &mut String, name: &str, ty: &str, help: &str) {
    let _ = writeln!(out, "# HELP {name} {help}\n# TYPE {name} {ty}");
}

impl Metrics {
    fn inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// An HTTP request answered: its route pattern and status.
    ///
    /// A route's name is copied once, the first time it is answered: the
    /// routes are the server's own (and "other"), so they are few.
    pub fn http(&self, route: &str, status: u16, secs: f64) {
        let mut m = self.inner();
        let route = match m.routes.get(route) {
            Some(&r) => r,
            None => {
                let r: &'static str = Box::leak(route.into());
                m.routes.insert(r);
                r
            }
        };
        *m.http.entry((route, status)).or_default() += 1;
        m.http_seconds.entry(route).or_default().observe(secs);
    }

    /// A gRPC call answered: its method and status code.
    pub fn grpc(&self, method: &'static str, code: &'static str, secs: f64) {
        let mut m = self.inner();
        *m.grpc.entry((method, code)).or_default() += 1;
        m.grpc_seconds.entry(method).or_default().observe(secs);
    }

    /// `outcome` ([`crate::telemetry::OUTCOMES`]): ok, partial, error,
    /// timeout (the server's deadline), client_error, client_deadline,
    /// refused or cancelled.
    pub fn query(&self, kind: &'static str, outcome: &'static str, secs: f64) {
        let mut m = self.inner();
        *m.queries.entry((kind, outcome)).or_default() += 1;
        m.query_seconds.entry(kind).or_default().observe(secs);
        m.query_max = Some(m.query_max.map_or(secs, |x| x.max(secs)));
    }

    /// The slowest query since the last call, in seconds (`None`: none
    /// ended); the sampler takes it for each point.
    pub fn take_query_max(&self) -> Option<f64> {
        self.inner().query_max.take()
    }

    /// How long a query waited for a slot to run in.
    pub fn admitted(&self, secs: f64) {
        self.inner().admission_seconds.observe(secs);
    }

    pub fn rejected(&self) {
        self.inner().rejected += 1;
    }

    pub fn write(&self, docs: u64, secs: f64) {
        let mut m = self.inner();
        m.writes += 1;
        m.written_docs += docs;
        m.write_seconds.observe(secs);
    }

    /// A row a stream wrote became visible `secs` after its event's time
    /// (`source.ts`).
    pub fn lag(&self, secs: f64) {
        self.inner().lag_seconds.0.observe(secs.max(0.0));
    }

    pub fn write_error(&self, status: u16) {
        *self.inner().write_errors.entry(status).or_default() += 1;
    }

    /// A write answered from its idempotency key, not applied again.
    pub fn replay(&self) {
        self.inner().replays += 1;
    }

    /// A request the slow log wrote down: "query" or "write".
    pub fn slow(&self, what: &'static str) {
        *self.inner().slow.entry(what).or_default() += 1;
    }

    /// `n` texts embedded for `what` ("query" or "write") in `secs`.
    pub fn embedded(&self, what: &'static str, n: u64, secs: f64) {
        let mut m = self.inner();
        *m.embedded.entry(what).or_default() += n;
        m.embed_seconds.entry(what).or_default().observe(secs);
    }

    pub fn agent_run(&self) {
        self.inner().agent_runs += 1;
    }

    /// Queries per kind and outcome, for the UI's status line.
    pub fn summary(&self) -> serde_json::Value {
        let m = self.inner();
        let mut by_kind = serde_json::Map::new();
        for (&(k, o), &n) in &m.queries {
            let e = by_kind.entry(k).or_insert_with(|| serde_json::json!({}));
            e[o] = serde_json::json!(n);
        }
        let write_errors: u64 = m.write_errors.values().sum();
        serde_json::json!({ "queries": by_kind, "writes": m.writes, "written_docs": m.written_docs, "write_errors": write_errors, "rejected": m.rejected, "agent_runs": m.agent_runs })
    }

    /// Keeps what [`Metrics::routes`] counts, once a minute (the series'
    /// sampler calls [`Metrics::totals`] every few seconds).
    fn keep(&self, m: &Inner) {
        let now = crate::telemetry::now_ms();
        let mut kept = self.kept.lock().unwrap_or_else(|e| e.into_inner());
        if kept.back().is_some_and(|(t, _)| now / 60_000 == t / 60_000) {
            return;
        }
        kept.push_back((now, std::sync::Arc::new(Counts::of(m))));
        // Past KEEP_MINUTES, the first of each hour; past KEEP_HOURS, none.
        let mut hour = None;
        kept.retain(|(t, _)| {
            let age = now.saturating_sub(*t) / 60_000;
            if age <= KEEP_MINUTES {
                return true;
            }
            let h = t / 3_600_000;
            let first = hour != Some(h);
            hour = Some(h);
            first && age <= KEEP_HOURS * 60
        });
    }

    /// The counters a sample of the series takes ([`crate::telemetry::Sample`]).
    pub fn totals(&self, s: &mut crate::telemetry::Sample) {
        let m = self.inner();
        self.keep(&m);
        for (&(kind, outcome), &n) in &m.queries {
            s.queries += n;
            *s.by_kind.entry(kind).or_default() += n;
            match outcome {
                "error" => s.query_errors += n,
                "timeout" => {
                    s.query_errors += n;
                    s.timeouts += n;
                }
                "client_error" => s.client_errors += n,
                "client_deadline" => s.client_deadlines += n,
                "refused" | "rejected" => s.rejected += n,
                "partial" => s.partial += n,
                "cancelled" => s.cancelled += n,
                _ => {}
            }
        }
        let mut hist = Vec::new();
        for h in m.query_seconds.values() {
            h.add_to(&mut hist);
        }
        s.query_hist = hist;
        for (&(_, status), &n) in &m.http {
            s.http += n;
            match status {
                400..=499 => s.http_4xx += n,
                500.. => s.http_5xx += n,
                _ => {}
            }
        }
        s.writes = m.writes;
        s.written_docs = m.written_docs;
        s.write_errors = m.write_errors.values().sum();
        s.write_hist = m.write_seconds.counts();
        s.slow = m.slow.values().sum();
    }

    /// [`Metrics::routes`] of the window `[from, to)` (ms): the counts
    /// at its end less those at its start, as kept a minute apart (an hour
    /// apart past 6 hours). `held_from_ms` says where the counts begin when
    /// that is later than `from` (the server started, or kept no older).
    pub fn routes_between(&self, from: u64, to: u64) -> serde_json::Value {
        let now = crate::telemetry::now_ms();
        let started = self.started.duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
        let current = Counts::of(&self.inner());
        let kept: Vec<(u64, std::sync::Arc<Counts>)> = self.kept.lock().unwrap_or_else(|e| e.into_inner()).iter().cloned().collect();
        let zero = Counts::default();
        // The start: the latest kept at or before `from`; the server's start
        // when it began after `from`; else the oldest kept.
        let (held_from, base): (u64, &Counts) = match kept.iter().rev().find(|(t, _)| *t <= from) {
            Some((t, c)) => (*t, c),
            None if started + 60_000 >= from || kept.is_empty() => (started, &zero),
            None => (kept[0].0, &kept[0].1),
        };
        let end: &Counts = if to + 30_000 >= now { &current } else { kept.iter().find(|(t, _)| *t >= to).map_or(&current, |(_, c)| c) };
        let mut j = end.since(base).json();
        j["from_ms"] = serde_json::json!(from);
        j["to_ms"] = serde_json::json!(to.min(now));
        j["held_from_ms"] = serde_json::json!(held_from.max(from));
        j
    }

    /// Requests by route: how many, how many failed (4xx and 5xx), and
    /// their mean and 99th percentile latency, in milliseconds.
    pub fn routes(&self) -> serde_json::Value {
        let m = self.inner();
        let mut by: BTreeMap<&str, (u64, u64, u64)> = BTreeMap::new();
        for ((route, status), &n) in &m.http {
            let e = by.entry(*route).or_default();
            e.0 += n;
            if (400..500).contains(status) {
                e.1 += n;
            }
            if *status >= 500 {
                e.2 += n;
            }
        }
        let routes: Vec<serde_json::Value> = by
            .into_iter()
            .map(|(route, (n, c4, c5))| {
                let h = m.http_seconds.get(route);
                let p99 = h.and_then(|h| crate::telemetry::quantile(&BUCKETS, &h.counts(), 0.99)).map(|s| s * 1e3);
                let mean = h.map(|h| h.sum / h.n.max(1) as f64 * 1e3);
                serde_json::json!({"route": route, "requests": n, "client_errors": c4, "server_errors": c5, "mean_ms": mean, "p99_ms": p99})
            })
            .collect();
        let grpc: Vec<serde_json::Value> = m
            .grpc_seconds
            .iter()
            .map(|(method, h)| {
                let failed: u64 = m.grpc.iter().filter(|((mm, code), _)| mm == method && *code != "Ok").map(|(_, n)| n).sum();
                serde_json::json!({"method": method, "calls": h.n, "failed": failed, "mean_ms": h.sum / h.n.max(1) as f64 * 1e3, "p99_ms": crate::telemetry::quantile(&BUCKETS, &h.counts(), 0.99).map(|s| s * 1e3)})
            })
            .collect();
        let kinds: Vec<serde_json::Value> = m
            .query_seconds
            .iter()
            .map(|(kind, h)| {
                let by: serde_json::Map<String, serde_json::Value> = m.queries.iter().filter(|((k, _), _)| k == kind).map(|((_, o), n)| (o.to_string(), serde_json::json!(n))).collect();
                serde_json::json!({
                    "kind": kind, "queries": h.n, "outcomes": by, "mean_ms": h.sum / h.n.max(1) as f64 * 1e3,
                    "p50_ms": crate::telemetry::quantile(&BUCKETS, &h.counts(), 0.5).map(|s| s * 1e3),
                    "p99_ms": crate::telemetry::quantile(&BUCKETS, &h.counts(), 0.99).map(|s| s * 1e3),
                    "hist": h.counts(),
                })
            })
            .collect();
        serde_json::json!({"http": routes, "grpc": grpc, "kinds": kinds, "buckets_s": BUCKETS, "admission_p99_ms": crate::telemetry::quantile(&BUCKETS, &m.admission_seconds.counts(), 0.99).map(|s| s * 1e3)})
    }

    /// The text format: what this records, then `more`.
    pub fn render(&self, more: &[Family]) -> String {
        let m = self.inner();
        let mut out = String::new();
        let o = &mut out;
        head(o, "chloedb_build_info", "gauge", "The server's version and the SIMD kernels it runs (always 1).");
        let _ = writeln!(o, "chloedb_build_info{{version=\"{}\",simd=\"{}\"}} 1", env!("CARGO_PKG_VERSION"), esc(&chloedb::simd::describe()));
        head(o, "process_start_time_seconds", "gauge", "When the process started, in seconds since the Unix epoch.");
        let _ = writeln!(o, "process_start_time_seconds {}", self.started.duration_since(SystemTime::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64()));
        for p in process() {
            head(o, p.name, p.ty, p.help);
            let _ = writeln!(o, "{} {}", p.name, p.values[0].1);
        }

        head(o, "chloedb_http_requests_total", "counter", "HTTP requests answered, by route and status.");
        for ((route, status), n) in &m.http {
            let _ = writeln!(o, "chloedb_http_requests_total{{route=\"{}\",status=\"{status}\"}} {n}", esc(route));
        }
        head(o, "chloedb_http_request_duration_seconds", "histogram", "Time to answer an HTTP request, by route.");
        for (route, h) in &m.http_seconds {
            h.render(o, "chloedb_http_request_duration_seconds", &format!("route=\"{}\"", esc(route)));
        }
        head(o, "chloedb_grpc_requests_total", "counter", "gRPC calls answered, by method and status code.");
        for ((method, code), n) in &m.grpc {
            let _ = writeln!(o, "chloedb_grpc_requests_total{{method=\"{method}\",code=\"{code}\"}} {n}");
        }
        head(o, "chloedb_grpc_request_duration_seconds", "histogram", "Time to answer a gRPC call, by method.");
        for (method, h) in &m.grpc_seconds {
            h.render(o, "chloedb_grpc_request_duration_seconds", &format!("method=\"{method}\""));
        }

        head(o, "chloedb_queries_total", "counter", "Queries by kind (bm25, vector, sparse, hybrid, boosted, expression, reranked, decay, graph, column, facet, aggregate, count, filter, compound, paths, invalid) and outcome (ok, partial, error, timeout: the server's deadline, client_error: a 4xx such as bad SQL, client_deadline: the client's own deadline, below the server's, refused: 429, cancelled).");
        for (&(k, outcome), n) in &m.queries {
            let _ = writeln!(o, "chloedb_queries_total{{kind=\"{k}\",outcome=\"{outcome}\"}} {n}");
        }
        head(o, "chloedb_query_duration_seconds", "histogram", "Time to answer a query, waiting for a slot included, by kind.");
        for (k, h) in &m.query_seconds {
            h.render(o, "chloedb_query_duration_seconds", &format!("kind=\"{k}\""));
        }
        head(o, "chloedb_query_admission_wait_seconds", "histogram", "Time a query waited for a slot to run in (--max-concurrent).");
        m.admission_seconds.render(o, "chloedb_query_admission_wait_seconds", "");
        head(o, "chloedb_rejected_total", "counter", "Queries refused because no slot came free before their deadline.");
        let _ = writeln!(o, "chloedb_rejected_total {}", m.rejected);

        head(o, "chloedb_writes_total", "counter", "Write requests that succeeded.");
        let _ = writeln!(o, "chloedb_writes_total {}", m.writes);
        head(o, "chloedb_written_docs_total", "counter", "Rows written by write requests.");
        let _ = writeln!(o, "chloedb_written_docs_total {}", m.written_docs);
        head(o, "chloedb_write_errors_total", "counter", "Write requests that failed, by HTTP status.");
        for (status, n) in &m.write_errors {
            let _ = writeln!(o, "chloedb_write_errors_total{{status=\"{status}\"}} {n}");
        }
        head(o, "chloedb_write_duration_seconds", "histogram", "Time from a write request to its commit, embedding and waiting for the writer included.");
        m.write_seconds.render(o, "chloedb_write_duration_seconds", "");
        head(o, "chloedb_stream_lag_seconds", "histogram", "Time from an event (its source.ts) to the moment the rows written for it were visible.");
        m.lag_seconds.0.render(o, "chloedb_stream_lag_seconds", "");
        head(o, "chloedb_idempotent_replays_total", "counter", "Writes answered from their idempotency key instead of being applied again.");
        let _ = writeln!(o, "chloedb_idempotent_replays_total {}", m.replays);
        head(o, "chloedb_slow_requests_total", "counter", "Queries and writes the slow log wrote down (--slow-ms).");
        for (what, n) in &m.slow {
            let _ = writeln!(o, "chloedb_slow_requests_total{{what=\"{what}\"}} {n}");
        }
        head(o, "chloedb_embedded_texts_total", "counter", "Texts embedded by the server's models: rows of writes, query parameters, and rows stored without vectors.");
        for (what, n) in &m.embedded {
            let _ = writeln!(o, "chloedb_embedded_texts_total{{for=\"{what}\"}} {n}");
        }
        head(o, "chloedb_embed_duration_seconds", "histogram", "Time to embed the texts of one write request or one query parameter.");
        for (what, h) in &m.embed_seconds {
            h.render(o, "chloedb_embed_duration_seconds", &format!("for=\"{what}\""));
        }
        head(o, "chloedb_agent_runs_total", "counter", "Questions the agent was asked.");
        let _ = writeln!(o, "chloedb_agent_runs_total {}", m.agent_runs);

        for f in more {
            head(o, f.name, f.ty, f.help);
            for (labels, v) in &f.values {
                let labels = if labels.is_empty() { String::new() } else { format!("{{{labels}}}") };
                let _ = writeln!(o, "{}{labels} {v}", f.name);
            }
        }
        out
    }
}

/// The process's CPU time, and on Linux its resident memory and open
/// file descriptors.
fn process() -> Vec<Family> {
    let mut out = Vec::new();
    // SAFETY: getrusage fills the struct it is given.
    let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut ru) } == 0 {
        let secs = |t: libc::timeval| t.tv_sec as f64 + t.tv_usec as f64 / 1e6;
        out.push(Family::one("process_cpu_seconds_total", "counter", "CPU time the process used, user and system.", secs(ru.ru_utime) + secs(ru.ru_stime)));
    }
    #[cfg(target_os = "linux")]
    {
        // statm: sizes in pages; the second is the resident set.
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(1) as f64;
        if let Some(rss) = std::fs::read_to_string("/proc/self/statm").ok().and_then(|s| s.split_whitespace().nth(1)?.parse::<f64>().ok()) {
            out.push(Family::one("process_resident_memory_bytes", "gauge", "Memory the process holds in RAM, mapped database pages included.", rss * page));
        }
        if let Ok(d) = std::fs::read_dir("/proc/self/fd") {
            out.push(Family::one("process_open_fds", "gauge", "File descriptors the process has open.", d.count() as f64));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_family_has_help_and_type_and_its_series_follow() {
        let m = Metrics::default();
        m.http("/v1/query", 200, 0.003);
        m.grpc("Query", "Ok", 0.002);
        m.query("bm25", "ok", 0.0025);
        m.write(3, 0.01);
        m.embedded("write", 3, 0.2);
        let text = m.render(&[Family { name: "chloedb_x", ty: "gauge", help: "An example.", values: vec![("table=\"a\"".into(), 2.0)] }]);
        let mut family = "";
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("# HELP ") {
                family = rest.split(' ').next().unwrap();
                continue;
            }
            if let Some(rest) = line.strip_prefix("# TYPE ") {
                assert_eq!(rest.split(' ').next().unwrap(), family, "{line}");
                continue;
            }
            assert!(line.starts_with(family), "{line} outside {family}");
        }
        assert!(text.contains("chloedb_http_request_duration_seconds_bucket{route=\"/v1/query\",le=\"0.005\"} 1"), "{text}");
        assert!(text.contains("chloedb_query_duration_seconds_sum{kind=\"bm25\"} 0.0025"), "{text}");
        assert!(text.contains("chloedb_query_admission_wait_seconds_count 0"), "{text}");
        assert!(text.contains("chloedb_x{table=\"a\"} 2"), "{text}");
        assert!(text.contains("process_cpu_seconds_total "), "{text}");
    }

    /// Routes and kinds over a window count only what happened in it.
    #[test]
    fn routes_of_a_window_count_what_happened_in_it() {
        let m = Metrics::default();
        m.http("/v1/query", 200, 0.01);
        m.query("bm25", "ok", 0.01);
        m.totals(&mut crate::telemetry::Sample::default());
        // That minute's counts, as if 10 minutes ago.
        let then = {
            let mut k = m.kept.lock().unwrap();
            k[0].0 -= 600_000;
            k[0].0
        };
        m.http("/v1/query", 500, 2.0);
        m.http("/v1/query", 200, 0.01);
        m.query("bm25", "error", 2.0);
        let now = crate::telemetry::now_ms();
        let w = m.routes_between(then + 1_000, now);
        let q = w["http"].as_array().unwrap().iter().find(|r| r["route"] == "/v1/query").cloned().unwrap();
        assert_eq!((q["requests"].as_u64(), q["server_errors"].as_u64()), (Some(2), Some(1)), "{w}");
        assert_eq!((w["kinds"][0]["queries"].as_u64(), w["kinds"][0]["outcomes"]["error"].as_u64(), w["kinds"][0]["outcomes"].get("ok")), (Some(1), Some(1), None), "{w}");
        assert_eq!(w["held_from_ms"].as_u64(), Some(then + 1_000), "{w}");
        // Before anything was kept: since the server started.
        let all = m.routes_between(0, now);
        assert_eq!(all["http"][0]["requests"].as_u64(), Some(3), "{all}");
        assert_eq!(m.routes()["http"][0]["requests"].as_u64(), Some(3));
    }
}
