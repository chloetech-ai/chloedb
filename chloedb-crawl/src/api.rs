//! The crawler's HTTP API: start, follow, pause and stop crawls, preview
//! what a site's table would be, and the cluster's growth. The command
//! center reaches it through chloedb-server (`--app crawler=URL`), which
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
    /// Who started each crawl (its key's name), by the crawl's id.
    owners: Mutex<std::collections::HashMap<String, String>>,
}

/// Who called, as chloedb-server says in the headers it sets on what it
/// forwards to an app that checks its callers (`"checks_callers": true`):
/// `x-chloedb-principal`, `x-chloedb-scope` and, for a key limited to
/// tables, `x-chloedb-tables`. A crawl runs with this app's token narrowed
/// to the caller's tables (`x-chloedb-as-tables`), so it makes and fills
/// only a table the caller's key may; a key limited to tables sees and
/// steers only its own crawls; starting one takes the write scope. A
/// request without the headers (a program with the app's token) may do
/// anything.
#[derive(Clone, Debug, Default)]
pub struct Caller {
    pub name: Option<String>,
    /// 0 feed, 1 read, 2 write, 3 admin.
    pub scope: u8,
    pub tables: Option<String>,
}

const SCOPES: [&str; 4] = ["feed", "read", "write", "admin"];

impl Caller {
    pub fn of(h: &HeaderMap) -> Caller {
        let get = |k: &str| h.get(k).and_then(|v| v.to_str().ok()).map(str::to_string);
        let scope = get("x-chloedb-scope");
        let level = |s: &str| SCOPES.iter().position(|x| x.eq_ignore_ascii_case(s.trim())).unwrap_or(0) as u8;
        Caller { name: get("x-chloedb-principal").or_else(|| scope.as_ref().map(|_| "someone".into())), scope: scope.as_deref().map_or(3, level), tables: get("x-chloedb-tables").filter(|t| !t.trim().is_empty()) }
    }

    fn direct(&self) -> bool {
        self.name.is_none()
    }

    /// The grants its crawls' statements are narrowed to.
    pub fn as_tables(&self) -> Option<String> {
        if self.direct() {
            return None;
        }
        Some(match &self.tables {
            Some(t) => t.clone(),
            None => format!("*={}", SCOPES[self.scope as usize]),
        })
    }

    /// Whether it sees a crawl `owner` started: every crawl for a key of
    /// every table (read), its own for one limited to tables.
    fn sees(&self, owner: Option<&str>) -> bool {
        self.direct() || (self.tables.is_none() && self.scope >= 1) || (owner.is_some() && owner == self.name.as_deref())
    }

    /// Refuses (403) what takes the write scope.
    fn may_write(&self, what: &str) -> Result<(), ApiError> {
        if self.direct() || self.scope >= 2 {
            return Ok(());
        }
        Err(ApiError(StatusCode::FORBIDDEN, format!("{what} takes a key that may write; the key {:?} has the {} scope", self.name.as_deref().unwrap_or("?"), SCOPES[self.scope as usize])))
    }

    /// Refuses (403) steering a crawl it did not start, unless an admin of
    /// every table.
    fn may_steer(&self, owner: Option<&str>) -> Result<(), ApiError> {
        self.may_write("steering a crawl")?;
        if self.direct() || (self.tables.is_none() && self.scope >= 3) || (owner.is_some() && owner == self.name.as_deref()) {
            return Ok(());
        }
        Err(ApiError(StatusCode::FORBIDDEN, format!("the crawl is {}'s: the key {:?} steers only its own", owner.unwrap_or("an admin"), self.name.as_deref().unwrap_or("?"))))
    }
}

impl Service {
    pub fn new(strata: Strata, grower: Arc<Grower>, token: Option<String>, local: bool) -> Arc<Service> {
        Service::with_contact(strata, grower, token, local, None)
    }

    pub fn with_contact(strata: Strata, grower: Arc<Grower>, token: Option<String>, local: bool, contact: Option<String>) -> Arc<Service> {
        Arc::new(Service { strata, grower, jobs: Mutex::new(Vec::new()), token: token.filter(|t| !t.is_empty()), next: AtomicU64::new(1), local, contact: contact.filter(|c| !c.trim().is_empty()), owners: Mutex::default() })
    }

    fn owner(&self, id: &str) -> Option<String> {
        self.owners.lock().unwrap_or_else(|e| e.into_inner()).get(id).cloned()
    }

    fn jobs(&self) -> std::sync::MutexGuard<'_, Vec<Arc<Job>>> {
        self.jobs.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn job(&self, id: &str) -> Option<Arc<Job>> {
        self.jobs().iter().find(|j| j.id == id).cloned()
    }

    /// Starts a crawl. Two crawls may not fill one table at once.
    pub fn start(&self, spec: Spec) -> Result<Arc<Job>, String> {
        self.start_for(spec, &Caller::default(), self.strata.clone())
    }

    /// Starts a crawl for `who`, writing with `strata` (narrowed to the
    /// caller's tables).
    fn start_for(&self, mut spec: Spec, who: &Caller, strata: Strata) -> Result<Arc<Job>, String> {
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
        if let Some(n) = &who.name {
            self.owners.lock().unwrap_or_else(|e| e.into_inner()).insert(job.id.clone(), n.clone());
        }
        self.grower.watch(&job.table);
        tokio::spawn(job.clone().run(strata));
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

/// Requests come from chloedb-server (for the command center) or from a
/// program, never from a page in a browser: a request with an `Origin`
/// is refused (another site's page could otherwise start crawls), and
/// with `--token`, a request without it.
async fn guard(State(svc): State<Shared>, req: Request, next: Next) -> Response {
    if req.headers().contains_key(header::ORIGIN) {
        return ApiError(StatusCode::FORBIDDEN, "the crawler takes no requests from browsers: open it in ChloeDB's command center (chloedb-server --app crawler=URL)".into()).into_response();
    }
    if let Some(t) = &svc.token {
        let given = req.headers().get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")).unwrap_or("");
        if !constant_eq(given.as_bytes(), t.as_bytes()) {
            return ApiError(StatusCode::UNAUTHORIZED, "this crawler needs its token (Authorization: Bearer ...)".into()).into_response();
        }
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caller(h: &[(&'static str, &str)]) -> Caller {
        let mut m = HeaderMap::new();
        for (k, v) in h {
            m.insert(*k, v.parse().unwrap());
        }
        Caller::of(&m)
    }

    #[test]
    fn crawls_by_the_callers_keys() {
        let own = caller(&[("x-chloedb-principal", "crawl-bot"), ("x-chloedb-scope", "admin"), ("x-chloedb-tables", "crawl_*=admin")]);
        let reader = caller(&[("x-chloedb-principal", "r"), ("x-chloedb-scope", "read")]);
        let admin = caller(&[("x-chloedb-principal", "token"), ("x-chloedb-scope", "admin")]);
        let program = caller(&[]);
        assert_eq!(own.as_tables().as_deref(), Some("crawl_*=admin"));
        assert_eq!(reader.as_tables().as_deref(), Some("*=read"));
        assert_eq!(program.as_tables(), None);
        assert!(own.sees(Some("crawl-bot")) && !own.sees(Some("other")) && !own.sees(None));
        assert!(reader.sees(Some("other")), "a key of every table sees every crawl");
        assert!(reader.may_write("x").is_err());
        assert!(own.may_steer(Some("crawl-bot")).is_ok() && own.may_steer(Some("other")).is_err());
        assert!(admin.may_steer(Some("other")).is_ok() && program.may_steer(None).is_ok());
    }
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
        // It refuses what x-chloedb-scope and x-chloedb-tables do not allow
        // ([`Caller`]): the server forwards it keys limited to tables.
        "checks_callers": true,
    }))
}

async fn view() -> Response {
    let body = match std::env::var_os("CHLOEDB_CRAWL_UI_DIR") {
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

async fn list(State(svc): State<Shared>, headers: HeaderMap) -> Json<Value> {
    let who = Caller::of(&headers);
    let jobs: Vec<Value> = svc
        .jobs()
        .iter()
        .rev()
        .filter_map(|j| {
            let owner = svc.owner(&j.id);
            who.sees(owner.as_deref()).then(|| {
                let mut s = j.summary();
                s["owner"] = json!(owner);
                s
            })
        })
        .collect();
    Json(json!({"crawls": jobs}))
}

async fn create(State(svc): State<Shared>, headers: HeaderMap, Json(spec): Json<Spec>) -> Result<Json<Value>, ApiError> {
    let who = Caller::of(&headers);
    who.may_write("starting a crawl")?;
    let job = svc.start_for(spec, &who, svc.strata.as_caller(who.as_tables())).map_err(bad)?;
    Ok(Json(json!({"id": job.id, "table": job.table})))
}

async fn one(State(svc): State<Shared>, headers: HeaderMap, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
    if !Caller::of(&headers).sees(svc.owner(&id).as_deref()) {
        return Err(not_found(&id));
    }
    let job = svc.job(&id).ok_or_else(|| not_found(&id))?;
    let mut s = job.status();
    s["grow"] = svc.grower.status();
    Ok(Json(s))
}

async fn forget(State(svc): State<Shared>, headers: HeaderMap, Path(id): Path<String>) -> Result<Json<Value>, ApiError> {
    let who = Caller::of(&headers);
    if !who.sees(svc.owner(&id).as_deref()) {
        return Err(not_found(&id));
    }
    who.may_steer(svc.owner(&id).as_deref())?;
    let job = svc.job(&id).ok_or_else(|| not_found(&id))?;
    if job.active() {
        return Err(ApiError(StatusCode::CONFLICT, format!("crawl {id} runs: stop it first")));
    }
    svc.jobs().retain(|j| j.id != id);
    Ok(Json(json!({"forgot": id})))
}

async fn act(State(svc): State<Shared>, headers: HeaderMap, Path((id, action)): Path<(String, String)>) -> Result<Json<Value>, ApiError> {
    let who = Caller::of(&headers);
    if !who.sees(svc.owner(&id).as_deref()) {
        return Err(not_found(&id));
    }
    who.may_steer(svc.owner(&id).as_deref())?;
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
    Caller::of(&headers).may_write("a crawl's preview (it fetches pages)")?;
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
