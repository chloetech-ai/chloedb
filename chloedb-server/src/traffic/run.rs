//! A test run of the Simulator (`POST /v1/admin/simulator`): on the
//! cluster this coordinator serves (or the server, alone), over a table
//! of test documents of its own ([`super::data`]), in the background:
//!
//! 1. **Load** a quarter of the documents, wait for the copies to settle,
//!    and send each read kind of the mix alone, one request at a time:
//!    its latency, the shards it asks and their answers' times (from its
//!    trace), and the CPU every copy and the coordinator spent on it.
//! 2. **Load the rest** and measure each kind again, and writes: how a
//!    kind's work grows with the rows a shard holds; and the mix's main
//!    search right after an insert, which waits for the copy it asks to
//!    rebuild the part of its memtable the insert changed.
//! 3. **Traffic**: the mix (reads and writes at the profile's shares) at
//!    a rising rate, an open loop (each request timed from when it was
//!    due, as `engine-compare replay` does), each step judged by the SLO,
//!    until two steps miss it; or a rate held for a time.
//! 4. **Analyse**: the model of [`super::model`] fitted to steps 1 and 2,
//!    calibrated to step 3's capacity, and the report of
//!    [`super::report`]. The test table is dropped unless kept.
//!
//! The requests run in this process, through the same path as an HTTP
//! request's (`Service::query`, `write_request`, `get_rows`), so they
//! cost what an application's cost but its HTTP. One run at a time; the
//! last few reports are kept beside the audit log.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use hdrhistogram::Histogram;
use rand::Rng;
use serde::Deserialize;
use serde_json::{json, Value as Json};

use super::data::{self, Ask, Corpus, Picks, TABLE};
use super::dist::{exp_gap, stream, Rand};
use super::model::{self, KindStats, Quant, Tested, WriteStats};
use crate::keys::Principal;
use crate::service::{After, Backend, QueryReq, Service};
use crate::telemetry::{self, now_ms};
use crate::ApiError;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Reports kept.
const KEEP: usize = 10;
/// Requests in flight at most during traffic: past it, arrivals are
/// refused by the load generator itself (and counted as errors).
const IN_FLIGHT: usize = 4096;

// ===================================================================== what is asked

/// What a run is asked to do. Every field has a default: an empty body
/// runs the search-app profile against 100,000 test documents.
#[derive(Clone, Debug, Deserialize, serde::Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// The profile the mix starts from (`search-app`, `rag`, `today`), or `custom`.
    pub profile: String,
    /// Read kinds and their weights; empty: the profile's.
    pub mix: BTreeMap<String, f64>,
    /// Share of requests that are writes (None: the profile's).
    pub write_share: Option<f64>,
    /// Test documents, and their vectors' dimensions.
    pub docs: u64,
    pub dim: usize,
    /// "ramp": rising rates until two steps miss the SLO; "steady": `rate` for `seconds`.
    pub shape: String,
    pub rate: Option<f64>,
    pub seconds: f64,
    /// The ramp: its first rate (None: a tenth of what the model expects),
    /// each next step's factor, a step's seconds, the most steps.
    pub start_qps: Option<f64>,
    pub factor: f64,
    pub step_s: f64,
    pub max_steps: usize,
    /// Seconds each kind is sent alone at each size.
    pub alone_s: f64,
    /// The reads' p99 a step must hold (None: the profile's), and the share of errors it may have.
    pub slo_p99_ms: Option<f64>,
    pub slo_errors: Option<f64>,
    /// The rate the cluster must hold, for the grade and the advice.
    pub target_qps: Option<f64>,
    /// Rows the advice predicts for (None: the cluster's own, else the test's).
    pub estimate_docs: Option<f64>,
    /// The machine the cluster runs on (a name of [`super::report::reference_machines`] or of a catalogue the page sends).
    pub machine: Option<Json>,
    /// Keep the test table afterwards.
    pub keep: bool,
    pub seed: u64,
    pub label: Option<String>,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            profile: "search-app".into(),
            mix: BTreeMap::new(),
            write_share: None,
            docs: 100_000,
            dim: 64,
            shape: "ramp".into(),
            rate: None,
            seconds: 60.0,
            start_qps: None,
            factor: 1.3,
            step_s: 8.0,
            max_steps: 24,
            alone_s: 2.5,
            slo_p99_ms: None,
            slo_errors: None,
            target_qps: None,
            estimate_docs: None,
            machine: None,
            keep: false,
            seed: 1,
            label: None,
        }
    }
}

/// A run's settings, the profile's filled in and checked.
#[derive(Clone, Debug)]
pub struct Settings {
    pub config: Config,
    /// Read kinds and their shares (summing to 1).
    pub mix: Vec<(String, f64)>,
    pub write_share: f64,
    pub writes: (f64, f64, f64),
    pub slo_ms: f64,
    pub slo_errors: f64,
    pub zipf_s: f64,
    pub tail_share: f64,
    pub docs_zipf: f64,
    pub typo_share: f64,
}

impl Config {
    pub fn settle(self) -> Result<Settings, ApiError> {
        let c = self;
        let p = match c.profile.as_str() {
            "custom" => None,
            name => Some(super::preset(name).ok_or_else(|| ApiError::bad(format!("profile is one of {} or custom, not {name:?}", super::PRESETS.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", "))))?),
        };
        let mix_src = if c.mix.is_empty() { p.as_ref().map(|p| p.mix.clone()).unwrap_or_default() } else { c.mix.clone() };
        let mut mix: Vec<(String, f64)> = Vec::new();
        for (k, w) in &mix_src {
            if super::sql_of(k).is_none() {
                return Err(ApiError::bad(format!("mix names {k:?}, not a kind (the kinds: {})", super::KINDS.iter().map(|(k, _)| *k).collect::<Vec<_>>().join(", "))));
            }
            if !w.is_finite() || *w < 0.0 {
                return Err(ApiError::bad(format!("the weight of {k:?} must be 0 or more")));
            }
            if *w > 0.0 {
                mix.push((k.clone(), *w));
            }
        }
        let total: f64 = mix.iter().map(|(_, w)| w).sum();
        if total <= 0.0 {
            return Err(ApiError::bad("the mix needs a kind with a weight above 0"));
        }
        for m in &mut mix {
            m.1 /= total;
        }
        let write_share = c.write_share.unwrap_or_else(|| p.as_ref().map_or(0.0, |p| p.writes.share));
        if !(0.0..=0.5).contains(&write_share) {
            return Err(ApiError::bad("write_share is between 0 and 0.5"));
        }
        if !(2_000..=5_000_000).contains(&c.docs) {
            return Err(ApiError::bad("docs is between 2,000 and 5,000,000"));
        }
        if !(2..=2048).contains(&c.dim) {
            return Err(ApiError::bad("dim is between 2 and 2,048"));
        }
        match c.shape.as_str() {
            "ramp" => {
                if !(c.factor > 1.0 && c.factor <= 4.0) || !(1.0..=600.0).contains(&c.step_s) || !(2..=60).contains(&c.max_steps) {
                    return Err(ApiError::bad("a ramp needs factor above 1 (at most 4), step_s 1 to 600 and max_steps 2 to 60"));
                }
            }
            "steady" => {
                if !c.rate.is_some_and(|r| r > 0.0 && r <= 1e6) || !(5.0..=3600.0).contains(&c.seconds) {
                    return Err(ApiError::bad("steady needs a rate above 0 (requests a second) and seconds 5 to 3,600"));
                }
            }
            s => return Err(ApiError::bad(format!("shape is ramp or steady, not {s:?}"))),
        }
        if !(0.5..=60.0).contains(&c.alone_s) {
            return Err(ApiError::bad("alone_s is between 0.5 and 60 seconds"));
        }
        let slo_ms = c.slo_p99_ms.unwrap_or_else(|| p.as_ref().map_or(100.0, |p| p.slo.p99_ms));
        let slo_errors = c.slo_errors.unwrap_or_else(|| p.as_ref().map_or(0.001, |p| p.slo.errors));
        if !(slo_ms > 0.0 && slo_ms <= 60_000.0) || !(0.0..1.0).contains(&slo_errors) {
            return Err(ApiError::bad("the SLO needs a p99 above 0 ms and a share of errors below 1"));
        }
        if c.target_qps.is_some_and(|t| !(t > 0.0 && t.is_finite())) {
            return Err(ApiError::bad("target_qps is a rate above 0"));
        }
        let w = p.as_ref().map(|p| (p.writes.insert, p.writes.update, p.writes.delete)).unwrap_or((0.5, 0.45, 0.05));
        let q = p.as_ref().map(|p| p.queries.clone()).unwrap_or_default();
        Ok(Settings {
            mix,
            write_share,
            writes: w,
            slo_ms,
            slo_errors,
            zipf_s: q.zipf_s,
            tail_share: q.tail_share,
            docs_zipf: p.as_ref().map_or(1.0, |p| p.docs.zipf_s),
            typo_share: q.typo_share,
            config: c,
        })
    }
}

// ===================================================================== the state

/// The run under way or the last, and the reports kept.
#[derive(Default)]
pub struct Simulator {
    state: Mutex<State>,
    stop: AtomicBool,
    next: AtomicU64,
    file: OnceLock<Option<PathBuf>>,
}

#[derive(Default)]
struct State {
    run: Option<Progress>,
    /// Finished reports, the newest first.
    reports: VecDeque<Json>,
}

/// A run, as it goes.
#[derive(Clone, Debug, serde::Serialize)]
struct Progress {
    id: String,
    state: String,
    started_ms: u64,
    by: String,
    label: Option<String>,
    phase: String,
    /// What it does now.
    text: String,
    phases: Vec<PhaseState>,
    /// The traffic's steps so far.
    steps: Vec<Json>,
    error: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize)]
struct PhaseState {
    id: &'static str,
    title: &'static str,
    state: &'static str,
    note: Option<String>,
}

const PHASES: &[(&str, &str)] = &[
    ("load", "Load a quarter of the test documents"),
    ("alone_small", "Each kind alone, at a quarter"),
    ("load_rest", "Load the rest"),
    ("alone", "Each kind alone, and writes, at the full size"),
    ("traffic", "The mix at rising rates"),
    ("analyse", "Fit the model, grade, advise"),
];

impl Simulator {
    fn file(&self, svc: &Service) -> Option<PathBuf> {
        self.file
            .get_or_init(|| match chloedb::env::var("CHLOEDB_SIMULATOR_STATE") {
                Ok(p) if !p.is_empty() => Some(PathBuf::from(p)),
                _ => svc.audit.path().map(|p| p.with_extension("simulator.json")),
            })
            .clone()
    }

    /// The reports kept by an earlier run of this server.
    fn load(&self, svc: &Service) {
        let Some(path) = self.file(svc) else { return };
        let Ok(bytes) = std::fs::read(&path) else { return };
        if let Ok(Json::Array(rs)) = serde_json::from_slice::<Json>(&bytes) {
            let mut st = lock(&self.state);
            if st.reports.is_empty() {
                st.reports = rs.into_iter().take(KEEP).collect();
            }
        }
    }

    fn save(&self, svc: &Service) {
        let Some(path) = self.file(svc) else { return };
        let body = serde_json::to_vec(&Json::Array(lock(&self.state).reports.iter().cloned().collect())).unwrap_or_default();
        let tmp = path.with_extension("simulator.tmp");
        if let Err(e) = std::fs::write(&tmp, body).and_then(|_| std::fs::rename(&tmp, &path)) {
            tracing::warn!(error = %e, path = %path.display(), "could not keep the simulator's reports");
        }
    }

    fn update(&self, f: impl FnOnce(&mut Progress)) {
        if let Some(p) = lock(&self.state).run.as_mut() {
            f(p);
        }
    }

    fn phase(&self, id: &'static str, text: impl Into<String>) {
        let text = text.into();
        self.update(|p| {
            for ph in &mut p.phases {
                if ph.id == id {
                    ph.state = "running";
                } else if ph.state == "running" {
                    ph.state = "done";
                }
            }
            p.phase = id.into();
            p.text = text;
        });
    }

    fn note(&self, id: &'static str, note: impl Into<String>) {
        let note = note.into();
        self.update(|p| {
            if let Some(ph) = p.phases.iter_mut().find(|ph| ph.id == id) {
                ph.note = Some(note);
            }
        });
    }

    fn say(&self, text: impl Into<String>) {
        let text = text.into();
        self.update(|p| p.text = text);
    }

    fn stopping(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}

/// A short form of a report, for the list.
fn summary(r: &Json) -> Json {
    json!({
        "id": r["id"], "label": r["label"], "state": r["state"], "started_ms": r["started_ms"], "finished_ms": r["finished_ms"],
        "profile": r["config"]["profile"], "shape": r["config"]["shape"], "grade": r["grade"]["label"], "score": r["grade"]["score"],
        "capacity_qps": r["measured"]["capacity_qps"], "bound": r["measured"]["bound"], "cluster": r["cluster"]["text"], "error": r["error"],
    })
}

/// `GET /v1/admin/simulator`: the run under way (or the last one's
/// progress), the reports kept, the presets, kinds and reference machines.
pub fn view(svc: &Arc<Service>) -> Json {
    let sim = &svc.simulator;
    sim.load(svc);
    let st = lock(&sim.state);
    let presets: Vec<Json> = super::PRESETS
        .iter()
        .filter_map(|(name, _)| super::preset(name).map(|p| (name, p)))
        .map(|(name, p)| json!({"name": name, "description": p.description, "mix": p.mix, "write_share": p.writes.share, "slo_p99_ms": p.slo.p99_ms, "slo_errors": p.slo.errors}))
        .collect();
    json!({
        "run": st.run,
        "reports": st.reports.iter().map(summary).collect::<Vec<_>>(),
        "presets": presets,
        "kinds": super::KINDS.iter().map(|(k, sql)| json!({"kind": k, "sql": sql})).collect::<Vec<_>>(),
        "machines": super::report::reference_machines(),
        "machines_source": super::report::REFERENCE_SOURCE,
        "table": TABLE,
        "defaults": Config::default(),
        "coordinator": matches!(svc.backend, Backend::Cluster(_)),
    })
}

/// `GET /v1/admin/simulator/runs/:id`: a report kept.
pub fn report(svc: &Arc<Service>, id: &str) -> Result<Json, ApiError> {
    let sim = &svc.simulator;
    sim.load(svc);
    lock(&sim.state).reports.iter().find(|r| r["id"] == id).cloned().ok_or_else(|| ApiError { status: 404, message: format!("no report {id} is kept here") })
}

/// `POST /v1/admin/simulator/predict {"run"?, "shards"?: [..], "copies"?:
/// [..], "machine"?, "docs"?, "qps"?, "slo_p99_ms"?, "target_qps"?,
/// "shape"?: {shards, copies}}`: other shapes predicted by the model of a
/// report (`run`, or the last one that has a model).
pub fn predict(svc: &Arc<Service>, body: &Json) -> Result<Json, ApiError> {
    let sim = &svc.simulator;
    sim.load(svc);
    let st = lock(&sim.state);
    let r = match body["run"].as_str() {
        Some(id) => st.reports.iter().find(|r| r["id"] == id).ok_or_else(|| ApiError { status: 404, message: format!("no report {id} is kept here") })?,
        None => st.reports.iter().find(|r| r["fit"].is_object()).ok_or_else(|| ApiError { status: 404, message: "no test run has a model yet: run one first".into() })?,
    };
    super::report::predict(r, body)
}

/// `POST /v1/admin/simulator/stop`: the run stops after the request under way.
pub fn stop(svc: &Arc<Service>) -> Json {
    let running = lock(&svc.simulator.state).run.as_ref().is_some_and(|r| r.state == "running");
    if running {
        svc.simulator.stop.store(true, Ordering::Relaxed);
        svc.simulator.say("Stopping…");
    }
    json!({"stopping": running})
}

/// `POST /v1/admin/simulator`: starts a run.
pub fn start(svc: &Arc<Service>, who: &str, config: Config) -> Result<Json, ApiError> {
    let set = config.settle()?;
    let sim = &svc.simulator;
    sim.load(svc);
    let id = {
        let mut st = lock(&sim.state);
        if st.run.as_ref().is_some_and(|r| r.state == "running") {
            return Err(ApiError { status: 409, message: "a test run is under way: wait for it, or stop it".into() });
        }
        let n = sim.next.fetch_add(1, Ordering::Relaxed);
        let id = format!("s{}-{}", now_ms() / 1000 % 1_000_000, n);
        st.run = Some(Progress {
            id: id.clone(),
            state: "running".into(),
            started_ms: now_ms(),
            by: who.to_string(),
            label: set.config.label.clone(),
            phase: "load".into(),
            text: "Starting".into(),
            phases: PHASES
                .iter()
                .map(|(id, title)| PhaseState { id, title: if *id == "traffic" && set.config.shape == "steady" { "The mix at a steady rate" } else { title }, state: "waiting", note: None })
                .collect(),
            steps: Vec::new(),
            error: None,
        });
        id
    };
    sim.stop.store(false, Ordering::Relaxed);
    let svc2 = svc.clone();
    let who = who.to_string();
    let id2 = id.clone();
    tokio::spawn(async move {
        let t0 = now_ms();
        let out = run(&svc2, &set, &id2).await;
        let sim = &svc2.simulator;
        let stopped = sim.stopping();
        // The test table goes, unless kept.
        if !set.config.keep {
            if let Err(e) = drop_now(&svc2).await {
                tracing::warn!(error = %e.message, "the simulator's test table was not dropped");
            }
        }
        let progress = lock(&sim.state).run.clone();
        let mut report = match out {
            Ok(r) => r,
            Err(e) => json!({"error": e.message, "config": set.config, "steps": progress.as_ref().map(|p| p.steps.clone())}),
        };
        let state = if report.get("error").is_some_and(|e| !e.is_null()) { if stopped { "stopped" } else { "failed" } } else if stopped { "stopped" } else { "done" };
        report["id"] = json!(id2);
        report["state"] = json!(state);
        report["by"] = json!(who);
        report["label"] = json!(set.config.label);
        report["started_ms"] = json!(t0);
        report["finished_ms"] = json!(now_ms());
        report["kept_table"] = json!(set.config.keep);
        {
            let mut st = lock(&sim.state);
            if let Some(p) = st.run.as_mut() {
                p.state = state.into();
                p.error = report["error"].as_str().map(String::from);
                for ph in &mut p.phases {
                    if ph.state == "running" {
                        ph.state = if state == "done" { "done" } else { "stopped" };
                    }
                }
                p.text = match state {
                    "done" => "Done".into(),
                    "stopped" => "Stopped".into(),
                    _ => format!("Failed: {}", report["error"].as_str().unwrap_or("")),
                };
            }
            st.reports.push_front(report);
            st.reports.truncate(KEEP);
        }
        sim.save(&svc2);
        tracing::info!(run = %id2, state, "a simulator run ended");
    });
    Ok(json!({"id": id, "run": lock(&sim.state).run}))
}

/// `DELETE /v1/admin/simulator/table`: drops the test table.
pub async fn drop_table(svc: &Arc<Service>) -> Result<Json, ApiError> {
    if lock(&svc.simulator.state).run.as_ref().is_some_and(|r| r.state == "running") && !svc.simulator.stopping() {
        return Err(ApiError { status: 409, message: "a test run is using the table".into() });
    }
    drop_now(svc).await
}

async fn drop_now(svc: &Arc<Service>) -> Result<Json, ApiError> {
    let had = svc.catalog_now().await.iter().any(|s| s.table == TABLE);
    if had {
        ddl(svc, &format!("DROP TABLE {TABLE}")).await?;
    }
    Ok(json!({"dropped": had}))
}

async fn ddl(svc: &Arc<Service>, sql: &str) -> Result<(), ApiError> {
    svc.sql_in(&Principal::open(), sql, &Default::default(), After::Any, 60_000, None, None).await.map(|_| ())
}

// ===================================================================== the copies' CPU

/// A server of the cluster, for its CPU.
#[derive(Clone)]
struct Server {
    shard: usize,
    name: String,
    host: String,
    client: Option<crate::coord::Client>,
}

/// Each server's CPU seconds and cores, and the coordinator's (this
/// process), at a moment.
#[derive(Clone)]
struct Cpu {
    at: Instant,
    servers: Vec<Option<(f64, f64)>>,
    coord: f64,
}

async fn servers(svc: &Arc<Service>) -> Vec<Server> {
    match &svc.backend {
        Backend::Local(_) => vec![Server { shard: 0, name: "this server".into(), host: "local".into(), client: None }],
        Backend::Cluster(c) => {
            let snap = c.snapshot().await;
            let mut names = HashMap::new();
            for s in snap["shards"].as_array().into_iter().flatten() {
                for r in s["replicas"].as_array().into_iter().flatten() {
                    if let Some(u) = r["url"].as_str() {
                        let name = r["short"].as_str().or(r["name"].as_str()).unwrap_or(u).to_string();
                        let node = r["detail"]["place"]["node"].as_str().map(String::from);
                        names.insert(u.to_string(), (name, node));
                    }
                }
            }
            c.replica_clients()
                .into_iter()
                .map(|(shard, url, client)| {
                    let (name, node) = names.get(&url).cloned().unwrap_or((url.clone(), None));
                    let host = node.unwrap_or_else(|| host_of(&url));
                    Server { shard, name, host, client: Some(client) }
                })
                .collect()
        }
    }
}

fn host_of(url: &str) -> String {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let host = rest.split('/').next().unwrap_or(rest);
    let host = match host.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) => h,
        _ => host,
    };
    match host {
        "localhost" | "127.0.0.1" | "[::1]" | "::1" => "local".into(),
        h => h.into(),
    }
}

async fn cpu(svc: &Arc<Service>, servers: &[Server]) -> Cpu {
    let asks = servers.iter().map(|s| {
        let client = s.client.clone();
        let svc = svc.clone();
        async move {
            match client {
                None => Some((telemetry::process_usage().1, svc.cores() as f64)),
                Some(mut c) => {
                    let a = tokio::time::timeout(Duration::from_secs(3), c.health(crate::pb::Empty {})).await.ok()?.ok()?.into_inner();
                    let d: Json = serde_json::from_str(&a.detail).ok()?;
                    let secs = d["system"]["cpu_seconds"].as_f64()?;
                    Some((secs, d["system"]["cores"].as_f64().unwrap_or(1.0)))
                }
            }
        }
    });
    let servers = futures::future::join_all(asks).await;
    Cpu { at: Instant::now(), servers, coord: telemetry::process_usage().1 }
}

/// CPU seconds between two samples, each server's and the coordinator's
/// (None: a server that did not say), less each one's idle rate.
fn spent(a: &Cpu, b: &Cpu, idle: Option<&Idle>) -> (Vec<Option<f64>>, f64, f64) {
    let secs = b.at.saturating_duration_since(a.at).as_secs_f64().max(1e-6);
    let servers = a
        .servers
        .iter()
        .zip(&b.servers)
        .enumerate()
        .map(|(i, (x, y))| match (x, y) {
            (Some((x, _)), Some((y, _))) => Some(((y - x) - idle.and_then(|d| d.servers.get(i).copied().flatten()).unwrap_or(0.0) * secs).max(0.0)),
            _ => None,
        })
        .collect();
    let coord = ((b.coord - a.coord) - idle.map_or(0.0, |d| d.coord) * secs).max(0.0);
    (servers, coord, secs)
}

/// Each server's CPU rate (cores) while nothing was sent.
#[derive(Clone, Default)]
struct Idle {
    servers: Vec<Option<f64>>,
    coord: f64,
}

async fn idle(svc: &Arc<Service>, servers: &[Server], secs: f64) -> Idle {
    let a = cpu(svc, servers).await;
    tokio::time::sleep(Duration::from_secs_f64(secs)).await;
    let b = cpu(svc, servers).await;
    let (s, c, t) = spent(&a, &b, None);
    Idle { servers: s.into_iter().map(|x| x.map(|v| v / t)).collect(), coord: c / t }
}

/// Waits until every server is quiet (under a fifth of a core) for a
/// second, at most `most` seconds: merges and replication after a load.
async fn settle(svc: &Arc<Service>, servers: &[Server], most: f64) -> f64 {
    let t0 = Instant::now();
    loop {
        let d = idle(svc, servers, 1.0).await;
        let busiest = d.servers.iter().flatten().fold(0.0f64, |a, b| a.max(*b));
        if busiest < 0.2 || t0.elapsed().as_secs_f64() > most || svc.simulator.stopping() {
            return t0.elapsed().as_secs_f64();
        }
    }
}

// ===================================================================== sending

/// What one request did.
struct Done {
    ok: bool,
    error: Option<String>,
    us: u64,
    /// Each shard asked and how long its answers took, µs.
    calls: Vec<(usize, u64)>,
}

fn hist() -> Histogram<u64> {
    Histogram::new_with_bounds(1, 600_000_000, 3).expect("histogram bounds")
}

fn quant(h: &Histogram<u64>) -> Quant {
    if h.is_empty() {
        return Quant::default();
    }
    let ms = |v: u64| v as f64 / 1e3;
    Quant { mean: h.mean() / 1e3, p50: ms(h.value_at_quantile(0.5)), p90: ms(h.value_at_quantile(0.9)), p99: ms(h.value_at_quantile(0.99)) }
}

/// The timeout of a request: generous, so that a slow answer is timed rather than cut.
fn timeout_ms(set: &Settings) -> u64 {
    (set.slo_ms * 20.0).clamp(2_000.0, 60_000.0) as u64
}

/// A read of `kind`, sent now.
async fn read(svc: &Arc<Service>, ask: Ask, timeout: u64) -> Done {
    let trace = telemetry::Trace::new();
    let t0 = Instant::now();
    let r = match ask {
        Ask::Key(k) => telemetry::traced(trace.clone(), svc.get_rows(Some(TABLE), vec![chloedb::Value::Str(k)], None, &After::Any, timeout, false)).await.map(|_| ()),
        Ask::Sql { sql, params } => {
            let fut = async {
                let params = svc.params(Some(&params)).await?;
                svc.query(QueryReq { sql, params, timeout_ms: timeout, quiet: true, ..Default::default() }).await.map(|_| ())
            };
            telemetry::traced(trace.clone(), fut).await
        }
    };
    let us = t0.elapsed().as_micros() as u64;
    let mut by: BTreeMap<usize, u64> = BTreeMap::new();
    for s in trace.spans() {
        if let (Some(i), "ok") = (s.shard, s.outcome) {
            *by.entry(i).or_default() += s.dur_us;
        }
    }
    match r {
        Ok(()) => Done { ok: true, error: None, us, calls: by.into_iter().collect() },
        Err(e) => Done { ok: false, error: Some(short_error(&e.message)), us, calls: Vec::new() },
    }
}

fn short_error(m: &str) -> String {
    let m = m.trim();
    let cut: String = m.chars().take(120).collect();
    if cut.len() < m.len() {
        format!("{cut}…")
    } else {
        cut
    }
}

/// The writes a run makes: new documents after the loaded ones, ratings
/// of popular ones changed, and its own new documents deleted.
struct Writer {
    next: AtomicU64,
    base: u64,
    inserted: Mutex<VecDeque<u64>>,
}

impl Writer {
    fn new(base: u64) -> Writer {
        Writer { next: AtomicU64::new(0), base, inserted: Mutex::new(VecDeque::new()) }
    }

    /// One write's operations, by the profile's weights.
    fn ops(&self, corpus: &Corpus, picks: &Picks, set: &Settings, rng: &mut Rand) -> Vec<Json> {
        let (i, u, d) = set.writes;
        let x = rng.gen::<f64>() * (i + u + d).max(1e-9);
        if x >= i + u {
            if let Some(n) = lock(&self.inserted).pop_front() {
                return vec![json!({"op": "delete", "table": TABLE, "key": data::key(n)})];
            }
        }
        if x >= i && x < i + u {
            let n = picks.doc(rng);
            let rating = (rng.gen_range(0..=500) as f64) / 100.0;
            return vec![json!({"op": "set", "table": TABLE, "key": data::key(n), "field": "rating", "value": rating})];
        }
        self.insert(corpus)
    }

    /// A new document.
    fn insert(&self, corpus: &Corpus) -> Vec<Json> {
        let n = self.base + self.next.fetch_add(1, Ordering::Relaxed);
        lock(&self.inserted).push_back(n);
        vec![json!({"op": "upsert", "table": TABLE, "doc": corpus.doc(n)})]
    }

    /// Deletes what it inserted.
    async fn reset(&self, svc: &Arc<Service>) {
        let all: Vec<u64> = lock(&self.inserted).drain(..).collect();
        for chunk in all.chunks(500) {
            let ops = chunk.iter().map(|n| json!({"op": "delete", "table": TABLE, "key": data::key(*n)})).collect();
            let _ = svc.write_request(ops, None).await;
        }
    }
}

async fn write(svc: &Arc<Service>, ops: Vec<Json>) -> Done {
    let t0 = Instant::now();
    let r = svc.write_request(ops, None).await;
    let us = t0.elapsed().as_micros() as u64;
    match r {
        Ok(_) => Done { ok: true, error: None, us, calls: Vec::new() },
        Err(e) => Done { ok: false, error: Some(short_error(&e.message)), us, calls: Vec::new() },
    }
}

// ===================================================================== loading

/// Loads documents `from..to`, `batch` a request, four requests in flight.
async fn load(svc: &Arc<Service>, corpus: &Arc<Corpus>, from: u64, to: u64, id: &'static str) -> Result<f64, ApiError> {
    let t0 = Instant::now();
    const BATCH: u64 = 500;
    let starts: Vec<u64> = (from..to).step_by(BATCH as usize).collect();
    let done = Arc::new(AtomicU64::new(0));
    let mut it = futures::stream::iter(starts.into_iter().map(|s| {
        let (svc, corpus, done) = (svc.clone(), corpus.clone(), done.clone());
        async move {
            let e = (s + BATCH).min(to);
            let ops: Vec<Json> = (s..e).map(|i| json!({"op": "upsert", "table": TABLE, "doc": corpus.doc(i)})).collect();
            let r = svc.write_request(ops, None).await;
            done.fetch_add(e - s, Ordering::Relaxed);
            r
        }
    }))
    .buffer_unordered(4);
    use futures::StreamExt;
    let mut last = Instant::now();
    while let Some(r) = it.next().await {
        r.map_err(|e| ApiError { status: e.status, message: format!("loading the test documents: {}", e.message) })?;
        if last.elapsed() > Duration::from_millis(500) {
            last = Instant::now();
            let n = done.load(Ordering::Relaxed);
            svc.simulator.say(format!("Loaded {} of {} documents", fmt_n(from + n), fmt_n(to)));
        }
        if svc.simulator.stopping() {
            return Err(ApiError::bad("stopped"));
        }
    }
    let secs = t0.elapsed().as_secs_f64();
    svc.simulator.note(id, format!("{} documents in {:.1} s ({}/s)", fmt_n(to - from), secs, fmt_n(((to - from) as f64 / secs.max(1e-3)) as u64)));
    Ok(secs)
}

fn fmt_n(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

// ===================================================================== alone

/// Sends `kind` alone, one request at a time, for `secs`.
async fn alone(svc: &Arc<Service>, set: &Settings, ctx: &Ctx, kind: &str, secs: f64, idle: &Idle) -> KindStats {
    let mut rng = stream(set.config.seed, &format!("alone-{kind}"));
    let mut picks = ctx.picks();
    let timeout = timeout_ms(set);
    // A warm-up, not timed.
    let warm = Instant::now() + Duration::from_secs_f64((secs * 0.15).min(0.5));
    while Instant::now() < warm {
        if let Some(a) = data::ask(kind, &ctx.corpus, &mut picks, &mut rng) {
            let _ = read(svc, a, timeout).await;
        }
    }
    let a = cpu(svc, &ctx.servers).await;
    let end = Instant::now() + Duration::from_secs_f64(secs);
    let (mut h, mut ch) = (hist(), hist());
    let (mut n, mut errors, mut calls) = (0u64, 0u64, 0u64);
    let mut first_error = None;
    // The time spent making the requests: this process's CPU, not the
    // coordinator's work, so taken off it.
    let mut drive = Duration::ZERO;
    while Instant::now() < end && !svc.simulator.stopping() {
        let made = Instant::now();
        let Some(ask) = data::ask(kind, &ctx.corpus, &mut picks, &mut rng) else { break };
        drive += made.elapsed();
        let d = read(svc, ask, timeout).await;
        n += 1;
        if !d.ok {
            errors += 1;
            first_error = first_error.or(d.error);
            continue;
        }
        let _ = h.record(d.us.max(1));
        calls += d.calls.len() as u64;
        for (_, us) in &d.calls {
            let _ = ch.record((*us).max(1));
        }
    }
    let b = cpu(svc, &ctx.servers).await;
    let (servers, coord, _) = spent(&a, &b, Some(idle));
    let coord = (coord - drive.as_secs_f64()).max(0.0);
    let ok = (n - errors).max(1) as f64;
    let shard_cpu: Option<f64> = if servers.iter().all(Option::is_some) { Some(servers.iter().flatten().sum::<f64>() * 1e3 / ok) } else { None };
    let local = matches!(svc.backend, Backend::Local(_));
    // A single server's CPU is all the shard's: it has no coordinator.
    let (shard_cpu, coord_cpu) = if local { (Some(coord * 1e3 / ok), None) } else { (shard_cpu, Some(coord * 1e3 / ok)) };
    if let Some(e) = first_error {
        tracing::warn!(kind, errors, error = %e, "simulator: requests failed while sent alone");
    }
    let share = set.mix.iter().find(|(k, _)| k == kind).map_or(0.0, |m| m.1);
    KindStats {
        kind: kind.to_string(),
        share,
        requests: n,
        errors,
        lat: quant(&h),
        calls: if n > errors { calls as f64 / (n - errors) as f64 } else { 0.0 },
        call: (!ch.is_empty()).then(|| quant(&ch)),
        cpu_shard_ms: shard_cpu,
        cpu_coord_ms: coord_cpu,
        ..Default::default()
    }
}

/// Writes alone, one at a time, for `secs`.
async fn writes_alone(svc: &Arc<Service>, set: &Settings, ctx: &Ctx, writer: &Writer, secs: f64, idle: &Idle) -> WriteStats {
    let mut rng = stream(set.config.seed, "alone-writes");
    let picks = ctx.picks();
    let a = cpu(svc, &ctx.servers).await;
    let end = Instant::now() + Duration::from_secs_f64(secs);
    let mut h = hist();
    let (mut n, mut errors) = (0u64, 0u64);
    let mut drive = Duration::ZERO;
    while Instant::now() < end && !svc.simulator.stopping() {
        let made = Instant::now();
        let ops = writer.ops(&ctx.corpus, &picks, set, &mut rng);
        drive += made.elapsed();
        let d = write(svc, ops).await;
        n += 1;
        if d.ok {
            let _ = h.record(d.us.max(1));
        } else {
            errors += 1;
        }
    }
    let b = cpu(svc, &ctx.servers).await;
    let (servers, coord, _) = spent(&a, &b, Some(idle));
    let coord = (coord - drive.as_secs_f64()).max(0.0);
    let ok = (n - errors).max(1) as f64;
    let local = matches!(svc.backend, Backend::Local(_));
    let copies = if servers.iter().all(Option::is_some) { Some(servers.iter().flatten().sum::<f64>() * 1e3 / ok) } else { None };
    let (copies, coord) = if local { (Some(coord * 1e3 / ok), None) } else { (copies, Some(coord * 1e3 / ok)) };
    let (i, u, d) = set.writes;
    WriteStats { requests: n, errors, lat: quant(&h), cpu_copies_ms: copies, cpu_coord_ms: coord, insert_share: i / (i + u + d).max(1e-9), ..Default::default() }
}

/// `kind` right after a one-row insert, an insert and a search at a time,
/// for `secs`: the search waits for the copy it asks to rebuild the part
/// of its memtable the insert changed (the next search on each copy does).
async fn after_inserts(svc: &Arc<Service>, set: &Settings, ctx: &Ctx, writer: &Writer, kind: &str, secs: f64) -> Option<Quant> {
    let mut rng = stream(set.config.seed, "after-inserts");
    let mut picks = ctx.picks();
    let timeout = timeout_ms(set);
    let end = Instant::now() + Duration::from_secs_f64(secs);
    let mut h = hist();
    while Instant::now() < end && !svc.simulator.stopping() {
        if !write(svc, writer.insert(&ctx.corpus)).await.ok {
            continue;
        }
        let Some(ask) = data::ask(kind, &ctx.corpus, &mut picks, &mut rng) else { break };
        let d = read(svc, ask, timeout).await;
        if d.ok {
            let _ = h.record(d.us.max(1));
        }
    }
    (!h.is_empty()).then(|| quant(&h))
}

// ===================================================================== traffic

/// What a run shares among its parts.
struct Ctx {
    corpus: Arc<Corpus>,
    servers: Vec<Server>,
    docs: u64,
    head: usize,
    set: Settings,
}

impl Ctx {
    fn picks(&self) -> Picks {
        Picks::new(self.docs, self.head, self.set.zipf_s, self.set.tail_share, self.set.docs_zipf)
    }
}

/// One step's tallies, which its requests add to as they end.
struct Tally {
    reads: Histogram<u64>,
    kinds: BTreeMap<String, Histogram<u64>>,
    writes: Histogram<u64>,
    read_errors: u64,
    write_errors: u64,
    errors: BTreeMap<String, u64>,
    refused: u64,
    closed: bool,
}

impl Tally {
    fn new() -> Tally {
        Tally { reads: hist(), kinds: BTreeMap::new(), writes: hist(), read_errors: 0, write_errors: 0, errors: BTreeMap::new(), refused: 0, closed: false }
    }
}

/// Sends the mix at `rate` for `secs`, an open loop: each request at its
/// time whatever the answers, timed from then. Answers the step's report.
async fn step(svc: &Arc<Service>, ctx: &Arc<Ctx>, writer: &Arc<Writer>, rate: f64, secs: f64, i: usize) -> Json {
    let set = &ctx.set;
    let mut rng = stream(set.config.seed.wrapping_add(i as u64 * 7919), "traffic");
    let mut picks = ctx.picks();
    let tally = Arc::new(Mutex::new(Tally::new()));
    let flying = Arc::new(AtomicUsize::new(0));
    let timeout = timeout_ms(set);
    let cpu_a = cpu(svc, &ctx.servers).await;
    let t0 = tokio::time::Instant::now() + Duration::from_millis(20);
    let end = t0 + Duration::from_secs_f64(secs);
    let mut next = t0;
    let mut sent = 0u64;
    // The time spent making the requests (this process's CPU, taken off
    // the coordinator's).
    let mut drive = Duration::ZERO;
    let cum: Vec<(String, f64)> = set.mix.iter().scan(0.0, |acc, (k, w)| {
        *acc += w;
        Some((k.clone(), *acc))
    }).collect();
    // Requests still in flight at the end: the scheduled times of each.
    let pending: Arc<Mutex<HashMap<u64, tokio::time::Instant>>> = Arc::new(Mutex::new(HashMap::new()));
    while next < end {
        tokio::time::sleep_until(next).await;
        if svc.simulator.stopping() {
            break;
        }
        let now = tokio::time::Instant::now();
        while next <= now && next < end {
            let due = next;
            next += Duration::from_secs_f64(exp_gap(&mut rng, rate));
            sent += 1;
            if flying.load(Ordering::Relaxed) >= IN_FLIGHT {
                let mut t = lock(&tally);
                t.refused += 1;
                t.read_errors += 1;
                *t.errors.entry("the load generator's requests in flight were all waiting".into()).or_default() += 1;
                continue;
            }
            let is_write = set.write_share > 0.0 && rng.gen::<f64>() < set.write_share;
            let (svc, tally, flying, pending) = (svc.clone(), tally.clone(), flying.clone(), pending.clone());
            flying.fetch_add(1, Ordering::Relaxed);
            lock(&pending).insert(sent, due);
            let id = sent;
            if is_write {
                let made = Instant::now();
                let ops = writer.ops(&ctx.corpus, &picks, set, &mut rng);
                drive += made.elapsed();
                tokio::spawn(async move {
                    let d = write(&svc, ops).await;
                    let us = due.elapsed().as_micros() as u64;
                    let mut t = lock(&tally);
                    if !t.closed {
                        if d.ok {
                            let _ = t.writes.record(us.max(1));
                        } else {
                            t.write_errors += 1;
                            *t.errors.entry(format!("write: {}", d.error.unwrap_or_default())).or_default() += 1;
                        }
                    }
                    drop(t);
                    lock(&pending).remove(&id);
                    flying.fetch_sub(1, Ordering::Relaxed);
                });
            } else {
                let x = rng.gen::<f64>();
                let kind = cum.iter().find(|(_, c)| x < *c).map_or_else(|| cum.last().map(|c| c.0.clone()).unwrap_or_default(), |c| c.0.clone());
                let made = Instant::now();
                let ask = data::ask(&kind, &ctx.corpus, &mut picks, &mut rng);
                drive += made.elapsed();
                let Some(ask) = ask else {
                    lock(&pending).remove(&id);
                    flying.fetch_sub(1, Ordering::Relaxed);
                    continue;
                };
                tokio::spawn(async move {
                    let d = read(&svc, ask, timeout).await;
                    let us = due.elapsed().as_micros() as u64;
                    let mut t = lock(&tally);
                    if !t.closed {
                        if d.ok {
                            let _ = t.reads.record(us.max(1));
                            let _ = t.kinds.entry(kind).or_insert_with(hist).record(us.max(1));
                        } else {
                            t.read_errors += 1;
                            *t.errors.entry(d.error.unwrap_or_default()).or_default() += 1;
                        }
                    }
                    drop(t);
                    lock(&pending).remove(&id);
                    flying.fetch_sub(1, Ordering::Relaxed);
                });
            }
        }
    }
    let ran = t0.elapsed().as_secs_f64().min(secs).max(1e-3);
    let cpu_b = cpu(svc, &ctx.servers).await;
    // Answers due by the end get a while longer; those still out are late.
    let drain = Duration::from_secs_f64((set.slo_ms * 10.0 / 1e3).clamp(1.0, 5.0));
    let until = tokio::time::Instant::now() + drain;
    while flying.load(Ordering::Relaxed) > 0 && tokio::time::Instant::now() < until {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let late: Vec<tokio::time::Instant> = lock(&pending).values().copied().collect();
    let mut t = lock(&tally);
    for due in &late {
        let _ = t.reads.record(due.elapsed().as_micros().max(1) as u64);
    }
    t.closed = true;
    let (servers, coord, cpu_secs) = spent(&cpu_a, &cpu_b, None);
    let busy: Vec<Json> = ctx
        .servers
        .iter()
        .zip(&servers)
        .zip(&cpu_b.servers)
        .map(|((s, used), c)| json!({"shard": s.shard, "name": s.name, "host": s.host, "cores_used": used.map(|u| u / cpu_secs), "cores": c.map(|c| c.1)}))
        .collect();
    let q = quant(&t.reads);
    let reads = t.reads.len();
    let read_errors = t.read_errors;
    let error_share = read_errors as f64 / (reads + read_errors).max(1) as f64;
    let holds = reads > 0 && q.p99 <= set.slo_ms && error_share <= set.slo_errors;
    let kinds: serde_json::Map<String, Json> = t.kinds.iter().map(|(k, h)| (k.clone(), json!({"n": h.len(), "p50_ms": quant(h).p50, "p99_ms": quant(h).p99}))).collect();
    let mut errors: Vec<(&String, &u64)> = t.errors.iter().collect();
    errors.sort_by(|a, b| b.1.cmp(a.1));
    json!({
        "i": i, "rate": rate, "seconds": ran, "sent": sent,
        "achieved": (reads + t.writes.len()) as f64 / ran,
        "reads": reads, "p50_ms": q.p50, "p90_ms": q.p90, "p99_ms": q.p99, "mean_ms": q.mean,
        "late": late.len(), "read_errors": read_errors, "error_share": error_share, "refused": t.refused,
        "errors": errors.iter().take(4).map(|(m, n)| json!({"error": m, "n": n})).collect::<Vec<_>>(),
        "writes": {"n": t.writes.len(), "p50_ms": quant(&t.writes).p50, "p99_ms": quant(&t.writes).p99, "errors": t.write_errors},
        "kinds": kinds,
        "busy": {"servers": busy, "coordinator": {"cores_used": (coord - drive.as_secs_f64()).max(0.0) / cpu_secs, "cores": svc.cores()}, "load_generator": {"cores_used": drive.as_secs_f64() / cpu_secs}},
        "holds": holds,
    })
}

/// The rate the steps say the cluster holds: where the reads' p99
/// crosses the SLO between the last step that held and the first that
/// did not (by the logarithms), and whether no step failed (a bound).
pub fn measured_capacity(steps: &[Json], slo_ms: f64) -> (f64, bool, Option<f64>, Option<f64>) {
    let rate = |s: &Json| s["rate"].as_f64().unwrap_or(0.0);
    let Some(last) = steps.iter().filter(|s| s["holds"] == true).max_by(|a, b| rate(a).total_cmp(&rate(b))) else {
        return (0.0, false, None, steps.iter().map(rate).reduce(f64::min));
    };
    let r1 = rate(last);
    let a1 = last["achieved"].as_f64().unwrap_or(r1).min(r1 * 1.05);
    let Some(fail) = steps.iter().filter(|s| s["holds"] != true && rate(s) > r1).min_by(|a, b| rate(a).total_cmp(&rate(b))) else {
        return (a1, true, Some(a1), None);
    };
    let r2 = rate(fail);
    let p1 = last["p99_ms"].as_f64().unwrap_or(0.0).max(1e-3);
    let p2 = fail["p99_ms"].as_f64().unwrap_or(f64::INFINITY);
    // Where the p99 crosses the SLO, by the logarithms; a step that failed
    // for its errors, not its p99, leaves the last that held.
    let cap = if p2.is_finite() && p2 > slo_ms && p1 < slo_ms {
        let x = ((slo_ms.ln() - p1.ln()) / (p2.ln() - p1.ln())).clamp(0.0, 1.0);
        (r1.ln() + x * (r2.ln() - r1.ln())).exp()
    } else {
        r1
    };
    (cap, false, Some(a1), Some(r2))
}

// ===================================================================== the run

async fn run(svc: &Arc<Service>, set: &Settings, id: &str) -> Result<Json, ApiError> {
    let sim = &svc.simulator;
    let c = &set.config;
    let local = matches!(svc.backend, Backend::Local(_));
    let servers = servers(svc).await;
    if servers.is_empty() {
        return Err(ApiError::bad("the cluster has no shard to test"));
    }
    // The cluster before the test: its own rows, its shape.
    let cluster_before = svc.cluster().await;
    let rows_now = rows_of(&cluster_before, local);
    let corpus = Arc::new(Corpus::new(c.seed, c.dim));
    let small = (c.docs / 4).max(500);
    let docs = c.docs;
    let ctx = Arc::new(Ctx { corpus: corpus.clone(), servers: servers.clone(), docs: small, head: 20_000, set: set.clone() });
    let _ = id;

    // 1. The table, a quarter of the rows.
    sim.phase("load", "Making the test table");
    if svc.catalog().iter().any(|s| s.table == TABLE) {
        ddl(svc, &format!("DROP TABLE {TABLE}")).await?;
    }
    ddl(svc, &data::create_sql(c.dim)).await?;
    let mut load_s = load(svc, &corpus, 0, small, "load").await?;
    sim.say("Waiting for merges to settle");
    let settled = settle(svc, &servers, 30.0).await;
    check_stop(svc)?;

    // 2. Each kind alone at a quarter.
    sim.phase("alone_small", "Measuring how quiet the servers are");
    let idle0 = idle(svc, &servers, 1.5).await;
    let mut kinds: Vec<String> = set.mix.iter().map(|(k, _)| k.clone()).collect();
    if !kinds.iter().any(|k| k == "key lookup") {
        kinds.push("key lookup".into());
    }
    let mut smalls = HashMap::new();
    for k in &kinds {
        sim.say(format!("{k}, alone, at {} documents", fmt_n(small)));
        let a = alone(svc, set, &ctx, k, (c.alone_s * 0.6).max(0.5), &idle0).await;
        smalls.insert(k.clone(), a);
        check_stop(svc)?;
    }
    sim.note("alone_small", format!("{} kinds", kinds.len()));

    // 3. The rest.
    sim.phase("load_rest", "Loading the rest");
    load_s += load(svc, &corpus, small, docs, "load_rest").await?;
    sim.say("Waiting for merges to settle");
    let settled2 = settle(svc, &servers, 45.0).await;
    check_stop(svc)?;

    // 4. Each kind alone at the full size, and writes.
    sim.phase("alone", "Measuring how quiet the servers are");
    let ctx = Arc::new(Ctx { corpus: corpus.clone(), servers: servers.clone(), docs, head: 20_000, set: set.clone() });
    let idle1 = idle(svc, &servers, 1.5).await;
    let mut stats = Vec::new();
    for k in &kinds {
        sim.say(format!("{k}, alone, at {} documents", fmt_n(docs)));
        let a = alone(svc, set, &ctx, k, c.alone_s, &idle1).await;
        let mut st = a;
        if let Some(s) = smalls.remove(k) {
            st.lat_small = s.lat;
            st.call_small = s.call;
            st.cpu_shard_ms_small = s.cpu_shard_ms;
        }
        stats.push(st);
        check_stop(svc)?;
    }
    let writer = Arc::new(Writer::new(docs));
    let wstats = if set.write_share > 0.0 {
        sim.say("Writes, alone");
        let mut w = writes_alone(svc, set, &ctx, &writer, c.alone_s, &idle1).await;
        // The mix's main search, each right after an insert.
        if let Some(k) = stats.iter().filter(|s| s.kind != "key lookup" && s.share > 0.0 && s.requests > s.errors).max_by(|a, b| a.share.total_cmp(&b.share)) {
            sim.say(format!("{}, each right after an insert", k.kind));
            w.after_insert = after_inserts(svc, set, &ctx, &writer, &k.kind, c.alone_s).await;
            w.after_kind = w.after_insert.is_some().then(|| k.kind.clone());
        }
        writer.reset(svc).await;
        w
    } else {
        WriteStats::default()
    };
    sim.note("alone", format!("{} kinds{}", stats.len(), if set.write_share > 0.0 { " and writes" } else { "" }));
    check_stop(svc)?;

    // The model as measured so far: where the ramp starts.
    let tested = tested_shape(svc, &servers, &cluster_before, small, docs, local).await;
    let mut fit = model::fit(&tested, &stats, &wstats, set.write_share);
    let shape = fit.tested_shape();
    let expect = fit.capacity(&shape, set.slo_ms).min(fit.saturation(&shape));

    // 5. Traffic.
    sim.phase("traffic", "Sending the mix");
    let mut steps: Vec<Json> = Vec::new();
    if c.shape == "steady" {
        let rate = c.rate.unwrap_or(100.0);
        let windows = (c.seconds / 5.0).ceil().max(1.0) as usize;
        for i in 0..windows {
            let secs = (c.seconds - 5.0 * i as f64).min(5.0);
            sim.say(format!("{rate:.0} requests a second, {:.0} of {:.0} s", 5.0 * i as f64, c.seconds));
            let s = step(svc, &ctx, &writer, rate, secs, i).await;
            sim.update(|p| p.steps.push(s.clone()));
            steps.push(s);
            if sim.stopping() {
                break;
            }
        }
    } else {
        let start = c.start_qps.unwrap_or_else(|| if expect.is_finite() && expect > 0.0 { (expect * 0.12).max(2.0) } else { 20.0 });
        let mut rate = start;
        let mut misses = 0;
        for i in 0..c.max_steps {
            sim.say(format!("Step {}: {} requests a second for {:.0} s", i + 1, fmt_n(rate.round() as u64), c.step_s));
            let s = step(svc, &ctx, &writer, rate, c.step_s, i).await;
            let held = s["holds"] == true;
            sim.update(|p| p.steps.push(s.clone()));
            steps.push(s);
            if sim.stopping() {
                break;
            }
            misses = if held { 0 } else { misses + 1 };
            if misses >= 2 {
                break;
            }
            if expect.is_finite() && rate > expect * 6.0 && held {
                // Far past what the model expects, and still holding: enough.
                break;
            }
            rate *= c.factor;
        }
    }
    writer.reset(svc).await;
    sim.note("traffic", format!("{} steps", steps.len()));

    // 6. The model, calibrated; the grade and the advice.
    sim.phase("analyse", "Fitting the model to the steps");
    let (cap, bound, held, failed) = measured_capacity(&steps, set.slo_ms);
    // Servers that share a machine (a cluster on one host, as in a test
    // on a laptop) took cores from each other as they needed them: the
    // model, which gives each its share, is not scaled to their limit.
    let mut per_host: HashMap<&str, usize> = HashMap::new();
    for s in &servers {
        *per_host.entry(s.host.as_str()).or_default() += 1;
    }
    if !local {
        *per_host.entry("local").or_default() += 1;
    }
    let shared = per_host.values().any(|n| *n > 1);
    let kappa = if c.shape == "ramp" && !shared { model::calibrate(&mut fit, set.slo_ms, cap, bound) } else { 1.0 };
    // How busy one machine's cores were at the busiest step that held.
    let limit = steps.iter().filter(|s| s["holds"] == true).max_by(|a, b| a["rate"].as_f64().unwrap_or(0.0).total_cmp(&b["rate"].as_f64().unwrap_or(0.0)));
    let host_used = limit.map(|s| {
        let b = &s["busy"];
        let servers: f64 = b["servers"].as_array().into_iter().flatten().filter(|x| x["host"] == "local").filter_map(|x| x["cores_used"].as_f64()).sum();
        (s["rate"].as_f64().unwrap_or(0.0), servers + b["coordinator"]["cores_used"].as_f64().unwrap_or(0.0), b["load_generator"]["cores_used"].as_f64().unwrap_or(0.0))
    });
    let report = super::report::build(super::report::Inputs {
        set,
        fit: &fit,
        stats: &stats,
        write: &wstats,
        steps: &steps,
        capacity: (cap, bound, held, failed),
        kappa,
        rows_now,
        servers: &servers.iter().map(|s| json!({"shard": s.shard, "name": s.name, "host": s.host})).collect::<Vec<_>>(),
        data: json!({
            "docs": docs, "docs_small": small, "dim": c.dim, "load_s": load_s, "settle_s": settled + settled2, "rows_s": docs as f64 / load_s.max(1e-3),
            "shared_host": shared, "host_cores": svc.cores(),
            "host_at_limit": host_used.map(|(rate, used, gen)| json!({"rate": rate, "servers_cores": used, "load_generator_cores": gen})),
        }),
    });
    Ok(report)
}

fn check_stop(svc: &Arc<Service>) -> Result<(), ApiError> {
    if svc.simulator.stopping() {
        Err(ApiError::bad("stopped"))
    } else {
        Ok(())
    }
}

/// Rows the cluster holds besides the test table's (a copy's `docs`, the
/// most of each shard's copies, over the shards).
fn rows_of(c: &Json, local: bool) -> u64 {
    if local {
        return c["node"]["health"]["docs"].as_u64().unwrap_or(0);
    }
    c["shards"].as_array().into_iter().flatten().map(|s| s["replicas"].as_array().into_iter().flatten().filter_map(|r| r["docs"].as_u64()).max().unwrap_or(0)).sum()
}

/// The cluster as tested: shards, copies, the cores each had for itself.
async fn tested_shape(svc: &Arc<Service>, servers: &[Server], cluster: &Json, small: u64, docs: u64, local: bool) -> Tested {
    let c = cpu(svc, servers).await;
    let cores: Vec<f64> = c.servers.iter().map(|x| x.map_or(svc.cores() as f64, |x| x.1)).collect();
    let mut per_host: HashMap<&str, usize> = HashMap::new();
    for s in servers {
        *per_host.entry(&s.host).or_default() += 1;
    }
    // The coordinator shares the machine of copies on loopback.
    let coord_host = "local";
    if !local {
        if let Some(n) = per_host.get_mut(coord_host) {
            *n += 1;
        }
    }
    let share = |i: usize| cores[i] / per_host.get(servers[i].host.as_str()).copied().unwrap_or(1).max(1) as f64;
    let copy_cores = (0..servers.len()).map(share).sum::<f64>() / servers.len().max(1) as f64;
    let shards = servers.iter().map(|s| s.shard).max().map_or(1, |m| m + 1);
    let coord_cores = if local { svc.cores() as f64 } else { svc.cores() as f64 / per_host.get(coord_host).copied().unwrap_or(1).max(1) as f64 };
    let _ = cluster;
    let coordinators = if local { 0 } else { crate::peers::json(svc)["coordinators"].as_array().map_or(1, |a| a.len().max(1)) };
    Tested { local, shards, copies: servers.len() as f64 / shards as f64, copy_cores, coord_cores, coordinators: coordinators.max(if local { 0 } else { 1 }), docs_small: small, docs }
}
