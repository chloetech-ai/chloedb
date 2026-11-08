//! The crawler's HTTP API: start, follow, pause and stop crawls, preview
//! what a site's table would be, and the cluster's growth. The command
//! center reaches it through strata-server (`--app crawler=URL`), which
//! also loads the crawler's view from here (`/app.json`, `/ui/view.js`).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use axum::extract::{Path, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::grow::Grower;
use crate::job::{Job, Spec};
use crate::strata::Strata;

pub const VIEW: &str = include_str!("../ui/view.js");

/// A globe crossed by links, for the navigation.
const ICON: &str = r#"<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c2.8 3 2.8 15 0 18M12 3c-2.8 3-2.8 15 0 18"/>"#;

pub struct Service {
    pub strata: Strata,
    pub grower: Arc<Grower>,
    pub jobs: Mutex<Vec<Arc<Job>>>,
    pub token: Option<String>,
    next: AtomicU64,
    /// Whether the crawler started the cluster it writes to (`--local`).
    pub local: bool,
    /// The contact of crawls that give none (`--contact`): whom a site's
    /// owner reaches, in the user agent.
    pub contact: Option<String>,
}

impl Service {
    pub fn new(strata: Strata, grower: Arc<Grower>, token: Option<String>, local: bool) -> Arc<Service> {
        Service::with_contact(strata, grower, token, local, None)
    }

    pub fn with_contact(strata: Strata, grower: Arc<Grower>, token: Option<String>, local: bool, contact: Option<String>) -> Arc<Service> {
        Arc::new(Service { strata, grower, jobs: Mutex::new(Vec::new()), token: token.filter(|t| !t.is_empty()), next: AtomicU64::new(1), local, contact: contact.filter(|c| !c.trim().is_empty()) })
    }

    fn jobs(&self) -> std::sync::MutexGuard<'_, Vec<Arc<Job>>> {
        self.jobs.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn job(&self, id: &str) -> Option<Arc<Job>> {
        self.jobs().iter().find(|j| j.id == id).cloned()
    }

    /// Starts a crawl. Two crawls may not fill one table at once.
    pub fn start(&self, mut spec: Spec) -> Result<Arc<Job>, String> {
        if spec.contact.is_none() {
            spec.contact = self.contact.clone();
        }
        let id = format!("c{}", self.next.fetch_add(1, Ordering::Relaxed));
        let job = Job::new(id, spec)?;
        {
            let mut jobs = self.jobs();
            if let Some(j) = jobs.iter().find(|j| j.table == job.table && j.active()) {
                return Err(format!("crawl {} fills table {} now: stop it first, or name another table", j.id, j.table));
            }
            jobs.push(job.clone());
        }
        self.grower.watch(&job.table);
        tokio::spawn(job.clone().run(self.strata.clone()));
        Ok(job)
    }

    /// Stops every crawl, and waits up to `secs` for their rows.
    pub async fn stop_all(&self, secs: u64) {
        let jobs: Vec<Arc<Job>> = self.jobs().clone();
        for j in &jobs {
            j.stop();
        }
        let t0 = std::time::Instant::now();
        while jobs.iter().any(|j| j.active()) && t0.elapsed().as_secs() < secs {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    }
}

type Shared = Arc<Service>;

struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error": self.1}))).into_response()
    }
}

fn bad(m: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, m.into())
}

fn not_found(id: &str) -> ApiError {
    ApiError(StatusCode::NOT_FOUND, format!("no crawl {id}"))
}

pub fn router(svc: Shared) -> Router {
    Router::new()
        .route("/app.json", get(manifest))
        .route("/ui/view.js", get(view))
        .route("/v1/info", get(info))
        .route("/v1/crawls", get(list).post(create))
        .route("/v1/crawls/:id", get(one).delete(forget))
        .route("/v1/crawls/:id/:action", post(act))
        .route("/v1/preview", post(preview))
        .route("/v1/grow", get(grow))
        .layer(axum::middleware::from_fn_with_state(svc.clone(), guard))
        .with_state(svc)
}

/// Requests come from strata-server (for the command center) or from a
/// program, never from a page in a browser: a request with an `Origin`
/// is refused (another site's page could otherwise start crawls), and
/// with `--token`, a request without it.
async fn guard(State(svc): State<Shared>, req: Request, next: Next) -> Response {
    if req.headers().contains_key(header::ORIGIN) {
        return ApiError(StatusCode::FORBIDDEN, "the crawler takes no requests from browsers: open it in Strata's command center (strata-server --app crawler=URL)".into()).into_response();
    }
    if let Some(t) = &svc.token {
        let given = req.headers().get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")).unwrap_or("");
        if !constant_eq(given.as_bytes(), t.as_bytes()) {
            return ApiError(StatusCode::UNAUTHORIZED, "this crawler needs its token (Authorization: Bearer ...)".into()).into_response();
        }
    }
    next.run(req).await
}

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

async fn manifest() -> Json<Value> {
    Json(json!({
        "name": "crawler",
        "title": "Crawler",
        "description": "Crawl a site into a table: fields extracted, its schema inferred, the cluster grown as it fills",
        "icon_svg": ICON,
        "view": "ui/view.js",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn view() -> Response {
    let body = match std::env::var_os("STRATA_CRAWL_UI_DIR") {
        Some(d) => std::fs::read_to_string(std::path::Path::new(&d).join("view.js")).unwrap_or_else(|_| VIEW.into()),
        None => VIEW.into(),
    };
    ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8"), (header::CACHE_CONTROL, "no-cache")], body).into_response()
}

async fn info(State(svc): State<Shared>) -> Json<Value> {
    let health = svc.strata.health().await;
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "strata": {"url": svc.strata.url, "role": health.as_ref().ok().and_then(|h| h["role"].as_str().map(str::to_string)), "error": health.err().map(|e| e.to_string())},
        "local": svc.local,
        "grow": svc.grower.status(),
        "defaults": serde_json::to_value(Spec { contact: svc.contact.clone(), ..default_spec() }).unwrap_or(Value::Null),
    }))
}

pub fn default_spec() -> Spec {
    serde_json::from_value(json!({"seeds": []})).expect("defaults")
}

async fn list(State(svc): State<Shared>) -> Json<Value> {
    let jobs: Vec<Value> = svc.jobs().iter().rev().map(|j| j.summary()).collect();
    Json(json!({"crawls": jobs}))
}

async fn create(State(svc): State<Shared>, Json(spec): Json<Spec>) -> Result<Json<Value>, ApiError> {
    let job = svc.start(spec).map_err(bad)?;
    Ok(Json(json!({"id": job.id, "table": job.table})))
}

async fn one(State(svc): State<Shared>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
    let job = svc.job(&id).ok_or_else(|| not_found(&id))?;
    let mut s = job.status();
    s["grow"] = svc.grower.status();
    Ok(Json(s))
}

async fn forget(State(svc): State<Shared>, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
    let job = svc.job(&id).ok_or_else(|| not_found(&id))?;
    if job.active() {
        return Err(ApiError(StatusCode::CONFLICT, format!("crawl {id} runs: stop it first")));
    }
    svc.jobs().retain(|j| j.id != id);
    Ok(Json(json!({"forgot": id})))
}

async fn act(State(svc): State<Shared>, Path((id, action)): Path<(String, String)>) -> Result<Json<Value>, ApiError> {
    let job = svc.job(&id).ok_or_else(|| not_found(&id))?;
    match action.as_str() {
        "pause" => job.pause(true),
        "resume" => job.pause(false),
        "stop" => job.stop(),
        _ => return Err(ApiError(StatusCode::NOT_FOUND, format!("no action {action}: pause, resume or stop"))),
    }
    Ok(Json(json!({"id": id, "phase": job.phase()})))
}

async fn preview(State(svc): State<Shared>, headers: HeaderMap, Json(mut b): Json<Value>) -> Result<Json<Value>, ApiError> {
    let _ = headers;
    let n = b.get("pages").and_then(Value::as_u64).unwrap_or(6).clamp(1, 20) as usize;
    if let Some(o) = b.as_object_mut() {
        o.remove("pages");
        if o.get("contact").is_none_or(Value::is_null) {
            if let Some(c) = &svc.contact {
                o.insert("contact".into(), Value::String(c.clone()));
            }
        }
    }
    let spec: Spec = serde_json::from_value(b).map_err(|e| bad(e.to_string()))?;
    let j = crate::job::preview(&spec, n).await.map_err(bad)?;
    Ok(Json(j))
}

async fn grow(State(svc): State<Shared>) -> Json<Value> {
    Json(svc.grower.status())
}
