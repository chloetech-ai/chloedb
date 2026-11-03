//! The profiles a server answers of itself: a CPU profile
//! ([`crate::profiler`]), an allocation profile ([`crate::heap`]), its
//! memory, its threads, and a table's index statistics
//! ([`crate::inspect::node`], which a coordinator's index inspector asks
//! each replica for). Asked over HTTP (`/v1/profile/<kind>`,
//! [`crate::http`]) or over gRPC (`Profile`, [`crate::grpc`]), which a
//! coordinator calls to profile one of its shards' servers
//! ([`crate::coord::Coordinator::profile`]).

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::{json, Value as Json};

use crate::service::{QueryReq, Service};
use crate::{heap, profiler, telemetry, ApiError};

/// What is profiled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Cpu,
    Heap,
    Memory,
    Threads,
    /// A table's index statistics ([`crate::inspect::node`]).
    Index,
    /// Not a profile: sets this server's log filter for a while
    /// ([`crate::service::Service::set_log_level`]), so that a coordinator
    /// sets a copy's over the same call; without a filter, answers the
    /// one set.
    LogFilter,
    /// Not a profile: a primary compares its replicas' rows with its own
    /// ([`crate::service::Service::verify_replicas`]).
    Verify,
    /// Not a profile: writes a checkpoint into the server's backup
    /// directory ([`crate::service::Service::checkpoint`]).
    Checkpoint,
}

impl Kind {
    pub fn of(s: &str) -> Result<Kind, ApiError> {
        Ok(match s {
            "cpu" => Kind::Cpu,
            "heap" => Kind::Heap,
            "memory" => Kind::Memory,
            "threads" => Kind::Threads,
            "index" => Kind::Index,
            "log_filter" => Kind::LogFilter,
            "verify" => Kind::Verify,
            "checkpoint" => Kind::Checkpoint,
            _ => return Err(ApiError::bad(format!("no profile of kind {s:?}: cpu, heap, memory, threads or index"))),
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::Cpu => "cpu",
            Kind::Heap => "heap",
            Kind::Memory => "memory",
            Kind::Threads => "threads",
            Kind::Index => "index",
            Kind::LogFilter => "log_filter",
            Kind::Verify => "verify",
            Kind::Checkpoint => "checkpoint",
        }
    }

    /// How long a profile of this kind may take with this body, to wait
    /// for one another server takes: its seconds and a minute to name the
    /// frames and run the query once first.
    pub fn wait(self, b: &Json) -> std::time::Duration {
        let s = match self {
            Kind::Cpu | Kind::Heap => b.get("seconds").and_then(|v| v.as_f64()).filter(|s| s.is_finite() && *s > 0.0).unwrap_or(5.0).min(profiler::MAX_SECONDS.max(heap::MAX_SECONDS)),
            Kind::Memory | Kind::Threads | Kind::Index | Kind::LogFilter => 0.0,
            // The replicas' digests, waited for `timeout_ms` (60 s).
            Kind::Verify => b.get("timeout_ms").and_then(|v| v.as_f64()).filter(|t| t.is_finite() && *t > 0.0).map_or(60.0, |t| t / 1e3).min(600.0),
            // A checkpoint copies the database: up to 10 minutes.
            Kind::Checkpoint => 540.0,
        };
        std::time::Duration::from_secs_f64(s + 60.0)
    }
}

/// A profile asked: its kind, the route's body, and how to answer.
#[derive(Clone, Debug)]
pub struct Asked {
    pub kind: Kind,
    pub body: Json,
    /// Folded lines (`thread;root;...;leaf value`) instead of JSON.
    pub folded: bool,
    /// With `folded`, of an allocation profile: the bytes in use at its end.
    pub in_use: bool,
}

/// A profile's answer.
pub enum Answer {
    Json(Json),
    /// Folded lines, and the name of the file they download as.
    Folded(&'static str, String),
}

impl Answer {
    /// The answer's bytes, as the gRPC call sends them.
    pub fn bytes(self) -> Vec<u8> {
        match self {
            Answer::Json(j) => j.to_string().into_bytes(),
            Answer::Folded(_, t) => t.into_bytes(),
        }
    }
}

/// Takes the profile `a` of this server.
pub async fn take(svc: &Arc<Service>, a: Asked) -> Result<Answer, ApiError> {
    let b = &a.body;
    match a.kind {
        Kind::Cpu => {
            let seconds = seconds_of(b, profiler::MAX_SECONDS)?;
            let hz = match b.get("hz") {
                None | Some(Json::Null) => 99,
                Some(v) => v.as_i64().filter(|h| (1..=profiler::MAX_HZ as i64).contains(h)).ok_or_else(|| ApiError::bad(format!("hz is a whole number from 1 to {}", profiler::MAX_HZ)))? as i32,
            };
            let mut load = Load::of(svc, b).await?;
            let (p, _) = profiler::take(hz, seconds, &mut load.during).await.map_err(refused)?;
            tracing::info!(seconds = p.seconds, hz, samples = p.samples, query = load.runs.is_some(), "CPU profile taken");
            if a.folded {
                return Ok(Answer::Folded("strata-cpu.folded", p.folded()));
            }
            let mut j = json!(p);
            j["cores"] = json!(p.cores());
            j["rate"] = json!(p.rate());
            if let Some(r) = load.runs() {
                j["runs"] = r;
            }
            Ok(Answer::Json(j))
        }
        Kind::Heap => {
            let seconds = seconds_of(b, heap::MAX_SECONDS)?;
            let every = match b.get("every") {
                None | Some(Json::Null) => heap::EVERY_DEFAULT,
                Some(v) => v.as_u64().filter(|e| (heap::MIN_EVERY..=heap::MAX_EVERY).contains(e)).ok_or_else(|| ApiError::bad(format!("every is a number of bytes from {} to {}", heap::MIN_EVERY, heap::MAX_EVERY)))?,
            };
            let mut load = Load::of(svc, b).await?;
            let (p, _) = heap::take(every, seconds, &mut load.during).await.map_err(refused)?;
            tracing::info!(seconds = p.seconds, every, samples = p.samples, query = load.runs.is_some(), "allocation profile taken");
            if a.folded {
                return Ok(Answer::Folded("strata-heap.folded", p.folded(a.in_use)));
            }
            let mut j = json!(p);
            if let Some(r) = load.runs() {
                j["runs"] = r;
            }
            Ok(Answer::Json(j))
        }
        Kind::Memory => {
            let (rss, _) = telemetry::process_usage();
            Ok(Answer::Json(json!({
                "heap": heap::stats(),
                "rss_bytes": rss,
                "peak_rss_bytes": telemetry::peak_rss(),
                "memory_limit_bytes": strata::storage::memory_limit(),
                "query_memory_bytes": svc.query_memory.as_ref().map(|b| b.used()),
                "threads": profiler::threads().len(),
                "at_ms": telemetry::now_ms(),
            })))
        }
        Kind::Threads => Ok(Answer::Json(json!({"threads": profiler::threads(), "at_ms": telemetry::now_ms()}))),
        Kind::Index => Ok(Answer::Json(crate::inspect::node(svc, &crate::inspect::Asked::of(b)?).await?)),
        Kind::LogFilter => {
            // Without a filter: the one set, for a coordinator's
            // `GET /v1/logs?node=` (alerts agent).
            let Some(filter) = b["filter"].as_str() else { return Ok(Answer::Json(svc.log_filter_json())) };
            let expires_s = b["expires_s"].as_u64();
            Ok(Answer::Json(svc.set_log_level(filter, expires_s, b["by"].as_str().unwrap_or("a coordinator"))?))
        }
        Kind::Verify => {
            let ms = b.get("timeout_ms").and_then(|v| v.as_u64()).filter(|t| *t > 0).unwrap_or(60_000).min(600_000);
            Ok(Answer::Json(svc.verify_fresh(std::time::Duration::from_millis(ms)).await?))
        }
        Kind::Checkpoint => {
            let layout = match b["layout"].as_str().unwrap_or("file") {
                "file" => strata::storage::Layout::SingleFile,
                "dir" | "directory" => strata::storage::Layout::Directory,
                other => return Err(ApiError::bad(format!("layout is \"file\" or \"dir\", not {other:?}"))),
            };
            let name = b["name"].as_str().filter(|n| !n.is_empty()).ok_or_else(|| ApiError::bad("a checkpoint's body needs \"name\""))?;
            let stats = svc.checkpoint(name, layout, b["link_from"].as_str()).await?;
            Ok(Answer::Json(json!({"checkpoint": stats})))
        }
    }
}

/// What a profile runs while it is taken: nothing (the whole server, as
/// its clients use it), or a query run again and again by `clients`
/// callers at once until the profile ends or a run fails, its run times
/// kept.
struct Load {
    during: Pin<Box<dyn Future<Output = ()> + Send>>,
    runs: Option<(usize, Arc<Mutex<(Vec<f64>, Option<ApiError>)>>)>,
}

impl Load {
    async fn of(svc: &Arc<Service>, b: &Json) -> Result<Load, ApiError> {
        let Some(sql) = b.get("sql").filter(|s| !s.is_null()) else { return Ok(Load { during: Box::pin(std::future::pending()), runs: None }) };
        let sql = sql.as_str().ok_or_else(|| ApiError::bad("the body needs a string \"sql\""))?;
        let clients = match b.get("clients") {
            None | Some(Json::Null) => 1,
            Some(v) => v.as_u64().filter(|c| (1..=64).contains(c)).ok_or_else(|| ApiError::bad("clients is a whole number from 1 to 64"))? as usize,
        };
        let req = QueryReq { sql: sql.to_string(), params: svc.params(b.get("params")).await?, quiet: true, ..Default::default() };
        // Once first: a query that fails answers at once, and the caches it
        // reads are warm when the profile starts.
        svc.query(req.clone()).await?;
        let runs: Arc<Mutex<(Vec<f64>, Option<ApiError>)>> = Default::default();
        let (svc, kept) = (svc.clone(), runs.clone());
        let during = async move {
            futures::future::join_all((0..clients).map(|_| async {
                loop {
                    let t = Instant::now();
                    let r = svc.query(req.clone()).await;
                    let mut k = kept.lock().unwrap_or_else(|e| e.into_inner());
                    if k.1.is_some() {
                        return;
                    }
                    match r {
                        Ok(_) => k.0.push(t.elapsed().as_secs_f64() * 1e3),
                        Err(e) => {
                            k.1 = Some(e);
                            return;
                        }
                    }
                }
            }))
            .await;
        };
        Ok(Load { during: Box::pin(during), runs: Some((clients, runs)) })
    }

    /// The runs' times, for the answer.
    fn runs(&self) -> Option<Json> {
        let (clients, runs) = self.runs.as_ref()?;
        let (mut ms, error) = std::mem::take(&mut *runs.lock().unwrap_or_else(|e| e.into_inner()));
        ms.sort_by(f64::total_cmp);
        let at = |f: f64| ms.get(((ms.len() as f64 - 1.0) * f).round() as usize).copied();
        Some(json!({"count": ms.len(), "clients": clients, "min_ms": at(0.0), "median_ms": at(0.5), "p99_ms": at(0.99), "max_ms": at(1.0), "error": error.map(|e| e.message)}))
    }
}

fn seconds_of(b: &Json, max: f64) -> Result<f64, ApiError> {
    match b.get("seconds") {
        None | Some(Json::Null) => Ok(5.0),
        Some(v) => v.as_f64().filter(|s| *s > 0.0 && *s <= max).ok_or_else(|| ApiError::bad(format!("seconds is a number above 0 and at most {max}"))),
    }
}

fn refused(e: profiler::Refused) -> ApiError {
    match e {
        profiler::Refused::Busy => ApiError { status: 409, message: "a profile of this kind is being taken: one at a time".into() },
        profiler::Refused::Unavailable(m) => ApiError { status: 501, message: format!("no profile: {m}") },
    }
}
