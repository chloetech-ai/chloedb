//! The app's HTTP API, and its page. ChloeDB's command center reaches it
//! through chloedb-server (`--app backbone=URL`), which loads the view from
//! here (`/app.json`, `/ui/view.js`) and sends the app's token with each
//! call. Without `--public` every route but `/healthz` needs the token.
//! With it the app also serves its page at `/` to anyone, and the page's
//! reads (every route is a GET) without the token: so many a minute from
//! one address, and so many at once. Nothing runs SQL a reader writes.
//!
//! Every answer carries `queries`: the SQL it ran, each with its rows and
//! time, which the page shows behind its "SQL" button.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use axum::extract::{ConnectInfo, Path, Query, Request, State};
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::pages::{self, Ctx};
use crate::rules::{self, Names};
use crate::strata::{RowExt, Strata, Trace};

pub const VIEW: &str = include_str!("../ui/view.js");
pub const INDEX: &str = include_str!("../ui/index.html");
pub const GEO: &str = include_str!("../ui/geo.json");

/// A globe crossed by routes.
pub const ICON: &str = r#"<circle cx="12" cy="12" r="9"/><path d="M3 12h18"/><path d="M12 3c2.6 2.6 3.8 5.6 3.8 9s-1.2 6.4-3.8 9c-2.6-2.6-3.8-5.6-3.8-9S9.4 5.6 12 3z"/>"#;

pub struct Service {
    pub db: Strata,
    pub token: Option<String>,
    pub public: bool,
    pub per_minute: usize,
    pub seen: Mutex<HashMap<String, (u64, usize)>>,
    pub turns: tokio::sync::Semaphore,
    pub started: Instant,
    /// Every network's short name and country, read at the start and every
    /// hour.
    pub names: tokio::sync::RwLock<Arc<Names>>,
    /// The last run of the rules: when, and what it found.
    pub last_rules: Mutex<Option<(Instant, Value)>>,
    /// The pages everyone sees the same (Now, the world map, the strip):
    /// worked out in the background every few seconds, so a reader is
    /// answered from memory and never waits for their SQL.
    pub shared: HashMap<&'static str, Kept>,
}

/// One shared page: its last answer, and a lock so that one reading of it
/// runs at a time.
#[derive(Default)]
pub struct Kept {
    answer: Mutex<Option<(Instant, Value)>>,
    working: tokio::sync::Mutex<()>,
}

impl Kept {
    /// The last answer, if it was worked out after `after`, or is younger
    /// than `young`.
    fn get(&self, after: Option<Instant>, young: Duration) -> Option<Value> {
        let held = self.answer.lock().unwrap_or_else(|e| e.into_inner());
        let (at, v) = held.as_ref()?;
        if !(after.is_some_and(|a| *at >= a) || at.elapsed() < young) {
            return None;
        }
        let mut v = v.clone();
        v["kept_s"] = json!(at.elapsed().as_secs());
        Some(v)
    }
}

/// How often the background loop works the shared pages out again.
const SHARED_EVERY: Duration = Duration::from_secs(15);
/// The oldest shared answer a reader is given; older (the loop could not
/// reach ChloeDB), the reader's request works it out itself.
const SHARED_KEEP: Duration = Duration::from_secs(300);

pub type Shared = Arc<Service>;

pub struct ApiError(pub StatusCode, pub String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error": self.1}))).into_response()
    }
}

impl From<crate::strata::Error> for ApiError {
    fn from(e: crate::strata::Error) -> ApiError {
        let status = match e.status {
            400 | 404 | 409 => StatusCode::from_u16(e.status).unwrap_or(StatusCode::BAD_REQUEST),
            _ => StatusCode::BAD_GATEWAY,
        };
        ApiError(status, if e.status == 0 || e.status >= 500 { format!("ChloeDB: {}", e.message) } else { e.message })
    }
}

type Answer = Result<Json<Value>, ApiError>;

fn done(trace: &Trace, mut v: Value, t0: Instant) -> Answer {
    v["queries"] = json!(trace.take());
    v["took_ms"] = json!((t0.elapsed().as_secs_f64() * 1000.0 * 10.0).round() / 10.0);
    Ok(Json(v))
}

/// Every network's short name and country (about 120,000 rows), read in
/// ranges of numbers (10,000 at a time, a query's most).
pub async fn names(db: &Strata) -> Names {
    let mut n = Names::default().with_countries(GEO);
    let mut from = 0i64;
    loop {
        let rows = match db.query("SELECT asn, short, country FROM bb_as WHERE asn >= :a ORDER BY asn LIMIT 10000", json!({"a": from})).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, "names");
                break;
            }
        };
        let Some(last) = rows.last().map(|r| r.i("asn")) else { break };
        for r in &rows {
            n.asn.insert(r.i("asn"), (r.s("short"), r.s("country")));
        }
        if rows.len() < 10000 {
            break;
        }
        from = last + 1;
    }
    n
}

pub struct Opts {
    pub listen: String,
    pub strata: String,
    pub key: Option<String>,
    pub token: Option<String>,
    pub public: bool,
    pub per_minute: usize,
    pub at_once: usize,
    pub rules: bool,
}

pub async fn serve(o: Opts) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(&o.listen).await.map_err(|e| anyhow::anyhow!("{}: {e}", o.listen))?;
    let db = Strata::new(&o.strata, o.key.filter(|k| !k.is_empty()));
    let svc = Arc::new(Service {
        db: db.clone(),
        token: o.token.filter(|t| !t.is_empty()),
        public: o.public,
        per_minute: o.per_minute.max(1),
        seen: Mutex::new(HashMap::new()),
        turns: tokio::sync::Semaphore::new(o.at_once.max(1)),
        started: Instant::now(),
        names: tokio::sync::RwLock::new(Arc::new(Names::default())),
        last_rules: Mutex::new(None),
        shared: SHARED.into_iter().map(|k| (k, Kept::default())).collect(),
    });
    {
        let svc = svc.clone();
        let run_rules = o.rules;
        tokio::spawn(async move {
            let mut read_at: Option<Instant> = None;
            let mut rules_at: Option<Instant> = None;
            // One loop, so the rules and the shared pages never read at
            // the same moment: the rules each minute, the pages after.
            loop {
                if read_at.is_none_or(|t| t.elapsed() > Duration::from_secs(3600)) {
                    let n = names(&svc.db).await;
                    if !n.asn.is_empty() {
                        tracing::info!(networks = n.asn.len(), "names read");
                        *svc.names.write().await = Arc::new(n);
                        read_at = Some(Instant::now());
                    }
                }
                if run_rules && rules_at.is_none_or(|t| t.elapsed() >= Duration::from_secs(60)) {
                    rules_at = Some(Instant::now());
                    let n = svc.names.read().await.clone();
                    let t0 = Instant::now();
                    match rules::run_once(&svc.db, &n).await {
                        Ok(v) => {
                            tracing::info!(open = %v["open"], incidents = %v["incidents"], secs = t0.elapsed().as_secs_f32(), "rules");
                            *svc.last_rules.lock().unwrap_or_else(|e| e.into_inner()) = Some((Instant::now(), v));
                        }
                        Err(e) => tracing::warn!(error = %e, "rules"),
                    }
                }
                for page in SHARED {
                    if let Err(e) = refresh(&svc, page).await {
                        tracing::warn!(page, error = %e.1, "shared page");
                    }
                }
                tokio::time::sleep(SHARED_EVERY).await;
            }
        });
    }
    tracing::info!(listen = %listener.local_addr()?, strata = %o.strata, token = svc.token.is_some(), public = svc.public, "chloedb-backbone serves");
    axum::serve(listener, router(svc).into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(async {
            let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("a signal handler");
            tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
        })
        .await?;
    Ok(())
}

pub fn router(svc: Shared) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/app.json", get(manifest))
        .route("/ui/view.js", get(view))
        .route("/ui/geo.json", get(geo))
        .route("/v1/now", get(now))
        .route("/v1/map", get(world))
        .route("/v1/timeline", get(timeline))
        .route("/v1/watch", get(watch))
        .route("/v1/country/:cc", get(country))
        .route("/v1/as/:asn", get(network))
        .route("/v1/prefix/*p", get(prefix))
        .route("/v1/incident/:id", get(incident))
        .route("/v1/incidents", get(incidents))
        .route("/v1/search", get(search))
        .route("/v1/diagnose", get(diagnose))
        .route("/v1/rules", get(rules_page))
        .route("/v1/scale", get(scale))
        .layer(axum::middleware::from_fn_with_state(svc.clone(), guard))
        .route("/healthz", get(|| async { "ok" }))
        .with_state(svc)
}

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

async fn guard(State(svc): State<Shared>, req: Request, next: Next) -> Response {
    let headers = req.headers();
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        let host = headers.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");
        if !origin.split("://").nth(1).is_some_and(|o| o == host) {
            return ApiError(StatusCode::FORBIDDEN, "requests from other sites are refused".into()).into_response();
        }
    }
    let authed = match &svc.token {
        None => true,
        Some(t) => headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .is_some_and(|got| constant_eq(got.as_bytes(), t.as_bytes())),
    };
    if authed {
        return next.run(req).await;
    }
    if !(svc.public && req.method() == Method::GET) {
        return ApiError(StatusCode::UNAUTHORIZED, "this app needs its token: open it in ChloeDB's command center (Apps → Backbone)".into()).into_response();
    }
    if !req.uri().path().starts_with("/v1/") {
        return next.run(req).await;
    }
    let who = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|s| s.trim().to_string())
        .or_else(|| req.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip().to_string()))
        .unwrap_or_default();
    if !svc.admit(&who, now_s() / 60) {
        return ApiError(StatusCode::TOO_MANY_REQUESTS, "too many requests from this address: wait a minute".into()).into_response();
    }
    let Ok(Ok(_turn)) = tokio::time::timeout(Duration::from_secs(30), svc.turns.acquire()).await else {
        return ApiError(StatusCode::SERVICE_UNAVAILABLE, "many people are reading at once: try again in a moment".into()).into_response();
    };
    next.run(req).await
}

fn now_s() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl Service {
    pub fn admit(&self, who: &str, minute: u64) -> bool {
        let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
        if seen.len() > 50_000 {
            seen.retain(|_, v| v.0 == minute);
        }
        let e = seen.entry(who.to_string()).or_insert((minute, 0));
        if e.0 != minute {
            *e = (minute, 0);
        }
        e.1 += 1;
        e.1 <= self.per_minute
    }
}

fn ui_file(name: &str, compiled: &'static str) -> std::borrow::Cow<'static, str> {
    match std::env::var_os("CHLOEDB_BACKBONE_UI_DIR") {
        Some(d) => std::fs::read_to_string(std::path::Path::new(&d).join(name)).map(Into::into).unwrap_or(compiled.into()),
        None => compiled.into(),
    }
}

async fn index(State(svc): State<Shared>) -> Response {
    if svc.token.is_some() && !svc.public {
        return (StatusCode::NOT_FOUND, "this app is shown in ChloeDB's command center (Apps → Backbone)").into_response();
    }
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8"), (header::CACHE_CONTROL, "no-cache")], ui_file("index.html", INDEX).into_owned()).into_response()
}

async fn manifest() -> Json<Value> {
    Json(json!({
        "name": "backbone",
        "title": "Backbone",
        "description": "The internet's backbone, live: every BGP update RIPE RIS hears, checked against RPKI, with exchanges, relationships, Atlas measurements and outages",
        "icon_svg": ICON,
        "view": "ui/view.js",
        "version": env!("CARGO_PKG_VERSION"),
        "maturity": "demo",
    }))
}

pub fn etag(text: &str) -> String {
    let h = text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3));
    format!("\"{h:016x}\"")
}

fn cached(headers: &HeaderMap, body: std::borrow::Cow<'static, str>, kind: &str) -> Response {
    let tag = etag(&body);
    let fresh = headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()).is_some_and(|v| v.split(',').any(|t| t.trim().trim_start_matches("W/") == tag));
    let head = [(header::CONTENT_TYPE, kind.to_string()), (header::CACHE_CONTROL, "no-cache".to_string()), (header::ETAG, tag)];
    if fresh {
        return (StatusCode::NOT_MODIFIED, head).into_response();
    }
    (head, body.into_owned()).into_response()
}

async fn view(headers: HeaderMap) -> Response {
    cached(&headers, ui_file("view.js", VIEW), "text/javascript; charset=utf-8")
}

async fn geo(headers: HeaderMap) -> Response {
    cached(&headers, ui_file("geo.json", GEO), "application/json")
}

macro_rules! ctx {
    ($svc:expr, $trace:ident, $c:ident) => {
        let $trace = Trace::new();
        let names = $svc.names.read().await.clone();
        let $c = Ctx { db: &$svc.db, trace: &$trace, names: &names };
    };
}

/// The pages kept in memory for everyone.
const SHARED: [&str; 3] = ["now", "world", "timeline"];

/// One answer for everyone: the background loop's, so a reader never waits
/// for its SQL (which takes seconds while the rules or the feed are busy).
/// Only when there is none yet (the app just started), or the loop could
/// not reach ChloeDB for minutes, does the reader's request read it.
async fn kept(svc: &Shared, page: &'static str) -> Answer {
    if let Some(v) = svc.shared[page].get(None, SHARED_KEEP) {
        return Ok(Json(v));
    }
    refresh(svc, page).await
}

/// Works a shared page out again. The lock makes a crowd that arrives
/// together wait for one reading rather than start one each.
async fn refresh(svc: &Shared, page: &'static str) -> Answer {
    let k = &svc.shared[page];
    let asked = Instant::now();
    let _one = k.working.lock().await;
    if let Some(v) = k.get(Some(asked), Duration::ZERO) {
        return Ok(Json(v));
    }
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let v = match page {
        "now" => pages::now(&c).await?,
        "timeline" => pages::timeline(&c).await?,
        _ => pages::world(&c).await?,
    };
    let Json(v) = done(&trace, v, t0)?;
    *k.answer.lock().unwrap_or_else(|e| e.into_inner()) = Some((Instant::now(), v.clone()));
    Ok(Json(v))
}

async fn now(State(svc): State<Shared>) -> Answer {
    kept(&svc, "now").await
}

async fn world(State(svc): State<Shared>) -> Answer {
    kept(&svc, "world").await
}

async fn timeline(State(svc): State<Shared>) -> Answer {
    kept(&svc, "timeline").await
}

#[derive(serde::Deserialize, Default)]
struct WatchParams {
    #[serde(default)]
    asns: String,
}

/// `?asns=13335,3320`: at most 12 networks.
async fn watch(State(svc): State<Shared>, Query(a): Query<WatchParams>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let mut asns: Vec<i64> = a.asns.split(',').filter_map(|x| x.trim().trim_start_matches(['A', 'S', 'a', 's']).parse::<i64>().ok()).filter(|x| *x > 0).collect();
    asns.dedup();
    if asns.is_empty() || asns.len() > 12 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "watch one to twelve networks: ?asns=13335,3320".into()));
    }
    let v = pages::watch(&c, &asns).await?;
    done(&trace, v, t0)
}

async fn country(State(svc): State<Shared>, Path(cc): Path<String>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let cc = cc.to_uppercase();
    if cc.len() != 2 || !cc.chars().all(|c| c.is_ascii_uppercase()) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "a country is two letters, such as TH".into()));
    }
    let v = pages::country(&c, &cc).await?;
    done(&trace, v, t0)
}

async fn network(State(svc): State<Shared>, Path(asn): Path<String>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let Ok(asn) = asn.trim_start_matches(['A', 'S', 'a', 's']).parse::<i64>() else {
        return Err(ApiError(StatusCode::BAD_REQUEST, "a network is a number, such as 13335 or AS13335".into()));
    };
    let v = pages::network(&c, asn).await?;
    done(&trace, v, t0)
}

async fn prefix(State(svc): State<Shared>, Path(p): Path<String>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let Some(p) = crate::mrt::Prefix::parse(&p) else {
        return Err(ApiError(StatusCode::BAD_REQUEST, "a prefix is an address and a length, such as 84.255.184.0/24".into()));
    };
    let v = pages::prefix(&c, &p).await?;
    done(&trace, v, t0)
}

async fn incident(State(svc): State<Shared>, Path(id): Path<String>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let v = pages::incident(&c, &id).await?;
    done(&trace, v, t0)
}

async fn incidents(State(svc): State<Shared>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let v = pages::incidents(&c).await?;
    done(&trace, v, t0)
}

#[derive(serde::Deserialize, Default)]
struct SearchParams {
    #[serde(default)]
    q: String,
}

async fn search(State(svc): State<Shared>, Query(a): Query<SearchParams>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let q = a.q.trim();
    if q.is_empty() || q.len() > 200 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "search for a network, a prefix, an address, an exchange or a country".into()));
    }
    let v = pages::search(&c, q).await?;
    done(&trace, v, t0)
}

#[derive(serde::Deserialize, Default)]
struct DiagnoseParams {
    #[serde(default)]
    cc: String,
}

async fn diagnose(State(svc): State<Shared>, Query(a): Query<DiagnoseParams>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let v = pages::diagnose(&c, a.cc.trim().to_uppercase().as_str()).await?;
    done(&trace, v, t0)
}

async fn rules_page(State(svc): State<Shared>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let v = pages::rules(&c).await?;
    done(&trace, v, t0)
}

async fn scale(State(svc): State<Shared>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let mut v = pages::scale(&c).await?;
    v["uptime_s"] = json!(svc.started.elapsed().as_secs());
    v["public"] = json!(svc.public);
    done(&trace, v, t0)
}
