//! The app's HTTP API, and its page. ChloeDB's command center reaches it
//! through chloedb-server (`--app earth=URL`), which loads the view from
//! here (`/app.json`, `/ui/view.js`) and sends the app's token with each
//! call. Without `--public` every route but `/healthz` needs the token.
//! With it the app also serves its page at `/` to anyone, and the page's
//! reads without the token (and its POSTs: a picture to search by, of at
//! most a few megabytes, and a browser's own fire watches, each under the
//! browser's random key): so many a minute from one address, and so many
//! at once. Nothing runs SQL a reader writes.
//!
//! Every answer carries `queries`: the SQL it ran, each with its rows and
//! time, which the page shows behind its "SQL" button.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, Query, Request, State};
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::pages::{self, Ctx, Filters};
use crate::places::Places;
use crate::strata::{Strata, Trace};

pub const VIEW: &str = include_str!("../ui/view.js");
pub const INDEX: &str = include_str!("../ui/index.html");

/// A globe with a flame.
pub const ICON: &str = r#"<circle cx="12" cy="12" r="9"/><path d="M3.6 9h16.8M3.6 15h7"/><path d="M17 21c-2 0-3.5-1.4-3.5-3.3 0-1.6 1.2-2.6 1.9-3.7.4 1 1.1 1.4 1.6 1.4 0-1.2.4-2.4 1.4-3.4.2 1.8 2.1 2.9 2.1 5.4 0 2.1-1.5 3.6-3.5 3.6z"/>"#;

pub struct Service {
    pub db: Strata,
    pub token: Option<String>,
    pub public: bool,
    pub per_minute: usize,
    pub seen: Mutex<HashMap<String, (u64, usize)>>,
    pub turns: tokio::sync::Semaphore,
    pub started: Instant,
    /// Every place (not its vector), read at the start and every hour.
    pub places: tokio::sync::RwLock<Arc<Places>>,
    /// Each region's Live, worked out every few seconds in the background.
    pub live: HashMap<&'static str, Mutex<Option<(Instant, Value)>>>,
    /// The layers, kept as the JSON they are sent as.
    pub layers: Mutex<HashMap<(String, String), Arc<String>>>,
    /// Where the places' yearly pictures are (tools/years.py fetch).
    pub chips: Option<std::path::PathBuf>,
    /// Where the replays' pictures of the sky are (tools/frames.py).
    pub frames: Option<std::path::PathBuf>,
    /// Fire watch's example, worked out once.
    pub example: Mutex<Option<Value>>,
    /// Each region's skies (ea_sky), read every two minutes.
    pub sky: tokio::sync::RwLock<HashMap<&'static str, Arc<crate::sky::Mem>>>,
    /// Each region's map of its climate in 2071-2100 (ea_climate), read once.
    pub future: tokio::sync::RwLock<HashMap<&'static str, Arc<String>>>,
    /// Each region's solar farms' mean look, for the Land finder.
    pub solar_looks: crate::land::Looks,
}

pub type Shared = Arc<Service>;

#[derive(Debug)]
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

pub struct Opts {
    pub listen: String,
    pub strata: String,
    pub key: Option<String>,
    pub token: Option<String>,
    pub public: bool,
    pub per_minute: usize,
    pub at_once: usize,
    pub rules: bool,
    pub chips: Option<std::path::PathBuf>,
    pub frames: Option<std::path::PathBuf>,
}

const REGIONS: [&str; 2] = ["california", "chile"];

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
        places: tokio::sync::RwLock::new(Arc::new(Places::default())),
        live: REGIONS.into_iter().map(|r| (r, Mutex::new(None))).collect(),
        layers: Mutex::new(HashMap::new()),
        chips: o.chips,
        frames: o.frames,
        example: Mutex::new(None),
        sky: tokio::sync::RwLock::new(HashMap::new()),
        future: tokio::sync::RwLock::new(HashMap::new()),
        solar_looks: tokio::sync::RwLock::new(HashMap::new()),
    });
    {
        let svc = svc.clone();
        let run_rules = o.rules;
        tokio::spawn(async move {
            let mut read_at: Option<Instant> = None;
            let mut rules_at: Option<Instant> = None;
            let mut names_at: Option<Instant> = None;
            let mut watch_at: Option<Instant> = None;
            let mut watched: HashMap<&'static str, i64> = HashMap::new();
            let mut sky_at: Option<Instant> = None;
            loop {
                if read_at.is_none_or(|t| t.elapsed() > Duration::from_secs(3600)) || svc.places.read().await.is_empty() {
                    match Places::read(&svc.db).await {
                        Ok(p) if !p.is_empty() => {
                            tracing::info!(places = p.len(), "places read");
                            *svc.places.write().await = Arc::new(p);
                            svc.layers.lock().unwrap_or_else(|e| e.into_inner()).clear();
                            read_at = Some(Instant::now());
                        }
                        Ok(_) => {}
                        Err(e) => tracing::warn!(error = %e, "places"),
                    }
                }
                if run_rules && rules_at.is_none_or(|t| t.elapsed() >= Duration::from_secs(60)) {
                    rules_at = Some(Instant::now());
                    let t0 = Instant::now();
                    match crate::rules::run_once(&svc.db).await {
                        Ok(v) => tracing::info!(open = %v["open"], secs = t0.elapsed().as_secs_f32(), "rules"),
                        Err(e) => tracing::warn!(error = %e, "rules"),
                    }
                }
                if run_rules && names_at.is_none_or(|t| t.elapsed() >= Duration::from_secs(3600)) {
                    names_at = Some(Instant::now());
                    match crate::names::run(&svc.db).await {
                        Ok(n) => tracing::info!(fires = n, "fires named"),
                        Err(e) => tracing::warn!(error = %e, "names"),
                    }
                }
                // The skies, once the places are there.
                if sky_at.is_none_or(|t| t.elapsed() >= Duration::from_secs(120)) && !svc.places.read().await.is_empty() {
                    sky_at = Some(Instant::now());
                    let places = svc.places.read().await.clone();
                    for r in REGIONS {
                        match crate::sky::read(&svc.db, &places, r).await {
                            Ok(m) => {
                                svc.sky.write().await.insert(r, Arc::new(m));
                            }
                            Err(e) => tracing::warn!(region = r, error = %e, "sky"),
                        }
                    }
                }
                // Fire watch: each region's new scans against the watches.
                if run_rules && watch_at.is_none_or(|t| t.elapsed() >= Duration::from_secs(60)) {
                    watch_at = Some(Instant::now());
                    for r in REGIONS {
                        let since = watched.get(r).copied().unwrap_or(0);
                        match crate::watch::check(&svc.db, r, since).await {
                            Ok((t, n)) => {
                                watched.insert(r, t);
                                if n > 0 {
                                    tracing::info!(region = r, alerts = n, "fire watch");
                                }
                            }
                            Err(e) => tracing::warn!(region = r, error = %e, "fire watch"),
                        }
                    }
                }
                for r in REGIONS {
                    if let Err(e) = refresh_live(&svc, r).await {
                        tracing::warn!(region = r, error = %e.1, "live");
                    }
                }
                tokio::time::sleep(Duration::from_secs(20)).await;
            }
        });
    }
    tracing::info!(listen = %listener.local_addr()?, strata = %o.strata, token = svc.token.is_some(), public = svc.public, "chloedb-earth serves");
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
        .route("/v1/home", get(home))
        .route("/v1/live", get(live))
        .route("/v1/replay/:id", get(replay))
        .route("/v1/fire/:id", get(fire))
        .route("/v1/fires", get(fires))
        .route("/v1/place/:id", get(place))
        .route("/v1/search", get(search))
        .route("/v1/picture", post(picture).layer(DefaultBodyLimit::max(8 << 20)))
        .route("/v1/mix", get(mix))
        .route("/v1/twins", get(twins))
        .route("/v1/changed", get(changed))
        .route("/v1/path/:id", get(path))
        .route("/v1/layer/:name", get(layer))
        .route("/v1/at", get(at))
        .route("/v1/find", get(find))
        .route("/v1/chip/:place/:year", get(chip))
        .route("/v1/frame/:replay/:n", get(frame))
        .route("/v1/sky", get(sky_layer))
        .route("/v1/sky/find", get(sky_find))
        .route("/v1/future", get(future_layer))
        .route("/v1/future/place/:id", get(future_place))
        .route("/v1/land", get(land_find))
        .route("/v1/watches", get(watches).post(watch_new))
        .route("/v1/watches/example", get(watch_example))
        .route("/v1/watches/feed/:key", get(watch_feed))
        .route("/v1/watch/:id", get(watch_one).post(watch_change))
        .route("/v1/rules", get(rules_page))
        .route("/v1/data", get(data))
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
    // The page's writes: a picture to search by, and a browser's own
    // watches (each under its random key, so many and no more).
    let path = req.uri().path();
    let reads = req.method() == Method::GET || (req.method() == Method::POST && (path == "/v1/picture" || path == "/v1/watches" || path.starts_with("/v1/watch/")));
    if !(svc.public && reads) {
        return ApiError(StatusCode::UNAUTHORIZED, "this app needs its token: open it in ChloeDB's command center (Apps → Earth)".into()).into_response();
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
    match std::env::var_os("CHLOEDB_EARTH_UI_DIR") {
        Some(d) => std::fs::read_to_string(std::path::Path::new(&d).join(name)).map(Into::into).unwrap_or(compiled.into()),
        None => compiled.into(),
    }
}

async fn index(State(svc): State<Shared>) -> Response {
    if svc.token.is_some() && !svc.public {
        return (StatusCode::NOT_FOUND, "this app is shown in ChloeDB's command center (Apps → Earth)").into_response();
    }
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8"), (header::CACHE_CONTROL, "no-cache")], ui_file("index.html", INDEX).into_owned()).into_response()
}

async fn manifest() -> Json<Value> {
    Json(json!({
        "name": "earth",
        "title": "Earth",
        "description": "California and Chile from space: fires as GOES sees them every few minutes, places searched by words and pictures, climate twins across the hemispheres and how places changed",
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

fn cached(headers: &HeaderMap, body: String, kind: &str, max_age: u32) -> Response {
    let tag = etag(&body);
    let fresh = headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()).is_some_and(|v| v.split(',').any(|t| t.trim().trim_start_matches("W/") == tag));
    let cache = if max_age == 0 { "no-cache".to_string() } else { format!("max-age={max_age}") };
    let head = [(header::CONTENT_TYPE, kind.to_string()), (header::CACHE_CONTROL, cache), (header::ETAG, tag)];
    if fresh {
        return (StatusCode::NOT_MODIFIED, head).into_response();
    }
    (head, body).into_response()
}

async fn view(headers: HeaderMap) -> Response {
    cached(&headers, ui_file("view.js", VIEW).into_owned(), "text/javascript; charset=utf-8", 0)
}

macro_rules! ctx {
    ($svc:expr, $trace:ident, $c:ident) => {
        let $trace = Trace::new();
        let places = $svc.places.read().await.clone();
        let $c = Ctx { db: &$svc.db, trace: &$trace, places: &places };
    };
}

async fn refresh_live(svc: &Shared, region: &'static str) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let v = pages::live(&c, region).await?;
    let Json(v) = done(&trace, v, t0)?;
    *svc.live[region].lock().unwrap_or_else(|e| e.into_inner()) = Some((Instant::now(), v.clone()));
    Ok(Json(v))
}

#[derive(serde::Deserialize, Default)]
struct RegionParams {
    #[serde(default)]
    region: String,
}

fn region_of(r: &str) -> Result<&'static str, ApiError> {
    REGIONS.into_iter().find(|x| *x == r).ok_or_else(|| ApiError(StatusCode::BAD_REQUEST, "the regions are california and chile".into()))
}

/// Live is the same for everyone: the background loop's answer, younger
/// than a minute and a half; or, when there is none, worked out now.
async fn live(State(svc): State<Shared>, Query(a): Query<RegionParams>) -> Answer {
    let region = region_of(if a.region.is_empty() { "california" } else { &a.region })?;
    if let Some((at, v)) = svc.live[region].lock().unwrap_or_else(|e| e.into_inner()).clone() {
        if at.elapsed() < Duration::from_secs(90) {
            let mut v = v;
            v["kept_s"] = json!(at.elapsed().as_secs());
            return Ok(Json(v));
        }
    }
    refresh_live(&svc, region).await
}

/// The home page: both regions' fires now, the replays, the backtest and
/// a few twins.
async fn home(State(svc): State<Shared>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let mut regions = Vec::new();
    for r in REGIONS {
        let kept = svc.live[r].lock().unwrap_or_else(|e| e.into_inner()).clone();
        let l = match kept {
            Some((_, v)) => v,
            None => pages::live(&c, r).await?,
        };
        let n = c.places.all.iter().filter(|p| p.region == r).count();
        regions.push(json!({"region": r, "places": n, "burning": l["fires"].as_array().map_or(0, |a| a.len()), "alerts": l["alerts"].as_array().map_or(0, |a| a.len()), "now": l["now"], "live": l["live"], "fires": l["fires"].as_array().map(|a| a.iter().take(5).cloned().collect::<Vec<_>>())}));
    }
    let (biggest, twins, backtest, counts) = tokio::join!(
        c.soft("the largest fires GOES saw", format!("SELECT {} FROM ea_fires WHERE state = 'out' ORDER BY area_km2 DESC LIMIT 8", pages::FIRE_CARD), json!({})),
        c.soft("the closest climate twins", format!("SELECT {}, twin, twin_sim, twin_score FROM ea_places WHERE region = 'california' AND twin_score > -1 ORDER BY twin_score DESC LIMIT 60", pages::PLACE_SMALL), json!({})),
        c.soft("the backtest", "SELECT * FROM ea_backtest ORDER BY region, hours LIMIT 10", json!({})),
        c.soft("how much was read", "SELECT COUNT(*) AS scans, SUM(pixels) AS pixels FROM ea_scans", json!({})),
    );
    let ids: Vec<String> = twins.iter().map(|r| crate::strata::RowExt::s(r, "twin")).collect();
    let theirs = if ids.is_empty() { Vec::new() } else { c.soft("their twins", format!("SELECT {} FROM ea_places WHERE id IN (:ids)", pages::PLACE_SMALL), json!({"ids": ids})).await };
    // Only pairs whose land the layers agree on, as Climate twins puts
    // first: the closest pictures are often dark ones, a shadowed forest
    // and deep water.
    let kind = |id: &str| theirs.iter().find(|t| crate::strata::RowExt::s(*t, "id") == id).map(|t| crate::strata::RowExt::s(t, "kind"));
    let twins: Vec<_> = twins.into_iter().filter(|a| kind(&crate::strata::RowExt::s(a, "twin")).is_some_and(|k| k == crate::strata::RowExt::s(a, "kind"))).take(6).collect();
    let v = json!({
        "regions": regions, "biggest": biggest, "twins": twins, "their_twins": theirs, "backtest": backtest, "counts": counts.first(),
        "replays": pages::REPLAYS.iter().map(|r| json!({"id": r.id, "region": r.region, "title": r.title, "about": r.about, "center": [r.center.0, r.center.1], "zoom": r.zoom})).collect::<Vec<_>>(),
    });
    done(&trace, v, t0)
}

async fn replay(State(svc): State<Shared>, Path(id): Path<String>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let mut v = pages::replay(&c, &id).await?;
    // The pictures of the sky under it, when tools/frames.py drew them.
    if let Some(dir) = &svc.frames {
        if let Ok(text) = tokio::fs::read_to_string(dir.join(&id).join("frames.json")).await {
            if let Ok(m) = serde_json::from_str::<Value>(&text) {
                v["frames"] = json!({"box": m["box"], "sat": m["sat"], "frames": m["frames"]});
            }
        }
    }
    done(&trace, v, t0)
}

#[derive(serde::Deserialize, Default)]
struct FireParams {
    at: Option<f64>,
    #[serde(default)]
    anywhere: bool,
}

async fn fire(State(svc): State<Shared>, Path(id): Path<String>, Query(a): Query<FireParams>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let v = pages::fire(&c, &id, a.at.filter(|h| *h > 0.0 && *h < 400.0), a.anywhere).await?;
    done(&trace, v, t0)
}

#[derive(serde::Deserialize, Default)]
struct FiresParams {
    #[serde(default)]
    region: String,
    year: Option<i32>,
    #[serde(default)]
    q: String,
    #[serde(default)]
    sort: String,
}

async fn fires(State(svc): State<Shared>, Query(a): Query<FiresParams>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    if a.q.len() > 100 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "a name of at most 100 letters".into()));
    }
    let region = if a.region.is_empty() { "california" } else { &a.region };
    let v = pages::fires(&c, region, a.year.filter(|y| (2000..2100).contains(y)), &a.q, &a.sort).await?;
    done(&trace, v, t0)
}

async fn place(State(svc): State<Shared>, Path(id): Path<String>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let v = pages::place(&c, &id).await?;
    done(&trace, v, t0)
}

#[derive(serde::Deserialize, Default)]
struct FilterParams {
    #[serde(default)]
    region: String,
    #[serde(default)]
    climate: String,
    #[serde(default)]
    kind: String,
    max_people: Option<f64>,
    min_people: Option<f64>,
    min_trees: Option<f64>,
    burned: Option<bool>,
}

impl FilterParams {
    fn filters(&self) -> Result<Filters, ApiError> {
        let ok = |s: &str| s.len() <= 40 && s.chars().all(|c| c.is_alphanumeric() || " -_".contains(c));
        if !self.region.is_empty() {
            region_of(&self.region)?;
        }
        if !ok(&self.climate) || !ok(&self.kind) {
            return Err(ApiError(StatusCode::BAD_REQUEST, "a climate or a kind is a short word".into()));
        }
        Ok(Filters { region: self.region.clone(), climate: self.climate.clone(), kind: self.kind.clone(), max_people: self.max_people, min_people: self.min_people, min_trees: self.min_trees, burned: self.burned })
    }
}

#[derive(serde::Deserialize, Default)]
struct SearchParams {
    #[serde(default)]
    q: String,
    #[serde(default)]
    mode: String,
    #[serde(flatten)]
    f: FilterParams,
}

async fn search(State(svc): State<Shared>, Query(a): Query<SearchParams>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    if a.q.trim().is_empty() || a.q.len() > 200 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "search for what a place looks like, in at most 200 letters".into()));
    }
    let v = pages::search(&c, &a.q, &a.mode, a.f.filters()?).await?;
    done(&trace, v, t0)
}

#[derive(serde::Deserialize, Default)]
struct PictureBody {
    #[serde(default)]
    image: String,
    #[serde(flatten)]
    f: FilterParams,
}

async fn picture(State(svc): State<Shared>, Json(b): Json<PictureBody>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let image = b.image.split_once(',').filter(|(h, _)| h.starts_with("data:")).map_or(b.image.as_str(), |(_, d)| d);
    if image.is_empty() || image.len() > 7 << 20 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "send a picture (a JPEG or PNG of at most 5 MB) as base64 in \"image\"".into()));
    }
    let v = pages::picture(&c, image, b.f.filters()?).await?;
    done(&trace, v, t0)
}

#[derive(serde::Deserialize, Default)]
struct MixParams {
    #[serde(default)]
    id: String,
    #[serde(default)]
    plus: String,
    #[serde(default)]
    minus: String,
    w: Option<f32>,
    #[serde(flatten)]
    f: FilterParams,
}

async fn mix(State(svc): State<Shared>, Query(a): Query<MixParams>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    if a.id.is_empty() || a.plus.len() > 120 || a.minus.len() > 120 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "mix a place (id) with words to add (plus) or take away (minus)".into()));
    }
    let v = pages::mix(&c, &a.id, &a.plus, &a.minus, a.w.unwrap_or(1.0).clamp(0.0, 4.0), a.f.filters()?).await?;
    done(&trace, v, t0)
}

#[derive(serde::Deserialize, Default)]
struct TwinParams {
    #[serde(default)]
    from: String,
    #[serde(default)]
    climate: String,
    #[serde(default)]
    kind: String,
}

async fn twins(State(svc): State<Shared>, Query(a): Query<TwinParams>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let ok = |s: &str| s.len() <= 40 && s.chars().all(|c| c.is_alphanumeric() || " -_".contains(c));
    if !ok(&a.climate) || !ok(&a.kind) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "a climate or a kind is a short word".into()));
    }
    let v = pages::twins(&c, if a.from.is_empty() { "california" } else { &a.from }, &a.climate, &a.kind).await?;
    done(&trace, v, t0)
}

async fn changed(State(svc): State<Shared>, Query(a): Query<pages::ChangeAsk>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    if a.q.len() > 200 || a.from.len() > 40 || a.to.len() > 40 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "a change in at most 200 letters, such as \u{201c}fields became a solar farm\u{201d}".into()));
    }
    let v = pages::changed(&c, &a).await?;
    done(&trace, v, t0)
}

async fn path(State(svc): State<Shared>, Path(id): Path<String>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let v = pages::same_path(&c, &id).await?;
    done(&trace, v, t0)
}

#[derive(serde::Deserialize)]
struct AtParams {
    lat: f64,
    lon: f64,
}

/// The place under a point of the map, from memory: the nearest centre
/// within 3 km (a place is 3.84 km a side).
async fn at(State(svc): State<Shared>, Query(a): Query<AtParams>) -> Json<Value> {
    let places = svc.places.read().await.clone();
    match places.nearest(a.lat, a.lon, 3.0) {
        Some(p) => Json(json!({"id": p.id, "region": p.region, "lat": p.lat, "lon": p.lon})),
        None => Json(json!({"id": null})),
    }
}

#[derive(serde::Deserialize, Default)]
struct FindParams {
    #[serde(default)]
    q: String,
}

/// Towns and counties by their first letters, or the place under
/// "lat, lon": what the front page's search box asks.
async fn find(State(svc): State<Shared>, Query(a): Query<FindParams>) -> Json<Value> {
    let places = svc.places.read().await.clone();
    let found: Vec<Value> = places
        .find(&a.q, 8)
        .into_iter()
        .map(|(label, p)| json!({"label": label, "id": p.id, "region": p.region, "lat": p.lat, "lon": p.lon, "adm1": p.adm1, "people": p.people.round()}))
        .collect();
    Json(json!({"found": found}))
}

#[derive(serde::Deserialize, Default)]
struct ChipParams {
    /// `?b64=1`: as base64 in JSON.
    b64: Option<String>,
}

/// A place's picture of a year: a JPEG, or as base64 in JSON for the
/// command center (whose requests carry the key in a header an image
/// cannot send).
async fn chip(State(svc): State<Shared>, Path((place, year)): Path<(String, u32)>, Query(a): Query<ChipParams>) -> Response {
    let Some(dir) = &svc.chips else { return ApiError(StatusCode::NOT_FOUND, "this app has no yearly pictures (serve --chips DIR)".into()).into_response() };
    if !place.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return ApiError(StatusCode::BAD_REQUEST, "no such place".into()).into_response();
    }
    jpeg(dir.join(format!("{place}_{year}.jpg")), &a, format!("no picture of {place} in {year}")).await
}

/// A picture of the sky under a replay (tools/frames.py), as a chip is sent.
async fn frame(State(svc): State<Shared>, Path((replay, n)): Path<(String, String)>, Query(a): Query<ChipParams>) -> Response {
    let Some(dir) = &svc.frames else { return ApiError(StatusCode::NOT_FOUND, "this app has no pictures of the replays (serve --frames DIR)".into()).into_response() };
    let n = n.trim_end_matches(".jpg");
    if !pages::REPLAYS.iter().any(|r| r.id == replay) || n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
        return ApiError(StatusCode::BAD_REQUEST, "no such picture".into()).into_response();
    }
    jpeg(dir.join(&replay).join(format!("{n}.jpg")), &a, format!("no picture {n} of {replay}")).await
}

async fn jpeg(path: std::path::PathBuf, a: &ChipParams, missing: String) -> Response {
    match tokio::fs::read(path).await {
        Ok(b) if a.b64.as_deref().is_some_and(|v| !matches!(v, "0" | "false")) => {
            use base64::Engine;
            Json(json!({"mime": "image/jpeg", "data": base64::engine::general_purpose::STANDARD.encode(&b)})).into_response()
        }
        Ok(b) => ([(header::CONTENT_TYPE, "image/jpeg"), (header::CACHE_CONTROL, "max-age=86400")], b).into_response(),
        Err(_) => ApiError(StatusCode::NOT_FOUND, missing).into_response(),
    }
}

/// A layer never changes between reads of the places: kept as the text it
/// is sent as, and cached by the browser for ten minutes.
async fn layer(State(svc): State<Shared>, headers: HeaderMap, Path(name): Path<String>, Query(a): Query<RegionParams>) -> Response {
    let key = (name.clone(), a.region.clone());
    let hit = svc.layers.lock().unwrap_or_else(|e| e.into_inner()).get(&key).cloned();
    let body = match hit {
        Some(b) => b,
        None => {
            let places = svc.places.read().await.clone();
            match pages::layer(&places, &name, &a.region) {
                Ok(v) => {
                    let b = Arc::new(v.to_string());
                    if !places.is_empty() {
                        svc.layers.lock().unwrap_or_else(|e| e.into_inner()).insert(key, b.clone());
                    }
                    b
                }
                Err(e) => return e.into_response(),
            }
        }
    };
    cached(&headers, (*body).clone(), "application/json", 600)
}

async fn sky_mem(svc: &Shared, region: &'static str) -> Result<Arc<crate::sky::Mem>, ApiError> {
    if let Some(m) = svc.sky.read().await.get(region) {
        return Ok(m.clone());
    }
    let places = svc.places.read().await.clone();
    let m = Arc::new(crate::sky::read(&svc.db, &places, region).await?);
    if !places.is_empty() {
        svc.sky.write().await.insert(region, m.clone());
    }
    Ok(m)
}

/// Every place's sky now and its last 30 days, as the map draws it.
async fn sky_layer(State(svc): State<Shared>, headers: HeaderMap, Query(a): Query<RegionParams>) -> Response {
    let region = match region_of(if a.region.is_empty() { "california" } else { &a.region }) {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    match sky_mem(&svc, region).await {
        Ok(m) => cached(&headers, (*m.layer).clone(), "application/json", 60),
        Err(e) => e.into_response(),
    }
}

#[derive(serde::Deserialize, Default)]
struct SkyFindParams {
    #[serde(default)]
    region: String,
    lat: Option<f64>,
    lon: Option<f64>,
    km: Option<f64>,
    #[serde(default)]
    want: String,
    #[serde(default)]
    q: String,
}

async fn sky_find(State(svc): State<Shared>, Query(a): Query<SkyFindParams>) -> Answer {
    let t0 = Instant::now();
    let at = a.lat.zip(a.lon).filter(|(la, lo)| la.is_finite() && lo.is_finite());
    let region = match at.and_then(|(la, lo)| crate::watch::region_of(la, lo)) {
        Some(r) => r,
        None => region_of(if a.region.is_empty() { "california" } else { &a.region })?,
    };
    if a.q.len() > 200 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "what the place looks like, in at most 200 letters".into()));
    }
    let mem = sky_mem(&svc, region).await?;
    ctx!(svc, trace, c);
    let want = match a.want.as_str() {
        "nights" | "days" => a.want.as_str(),
        _ => "now",
    };
    let v = crate::sky::find(&c, &mem, crate::sky::Find { region, at, km: a.km.unwrap_or(50.0).clamp(5.0, 400.0), want, q: &a.q }).await?;
    done(&trace, v, t0)
}

/// A region's map of its climate in 2071-2100.
async fn future_layer(State(svc): State<Shared>, headers: HeaderMap, Query(a): Query<RegionParams>) -> Response {
    let region = match region_of(if a.region.is_empty() { "california" } else { &a.region }) {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };
    if let Some(body) = svc.future.read().await.get(region).cloned() {
        return cached(&headers, (*body).clone(), "application/json", 3600);
    }
    match crate::future::layer(&svc.db, region).await {
        Ok(v) => {
            let body = Arc::new(v.to_string());
            // Kept once the rows are there: they change only when loaded again.
            if v["n"].as_u64().unwrap_or(0) > 0 {
                svc.future.write().await.insert(region, body.clone());
            }
            cached(&headers, (*body).clone(), "application/json", 3600)
        }
        Err(e) => ApiError::from(e).into_response(),
    }
}

#[derive(serde::Deserialize, Default)]
struct ScopeParams {
    #[serde(default)]
    scope: String,
}

async fn future_place(State(svc): State<Shared>, Path(id): Path<String>, Query(a): Query<ScopeParams>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let v = crate::future::place(&c, &id, &a.scope).await?;
    done(&trace, v, t0)
}

/// The Land finder: places within the limits asked for, best first.
async fn land_find(State(svc): State<Shared>, Query(a): Query<crate::land::Ask>) -> Answer {
    let t0 = Instant::now();
    let region = region_of(if a.region.is_empty() { "california" } else { &a.region })?;
    if a.q.len() > 200 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "what the place looks like, in at most 200 letters".into()));
    }
    let cached = svc.solar_looks.read().await.get(region).cloned();
    let look = match cached {
        Some(l) => l,
        None => {
            let l = crate::land::solar_look(&svc.db, region).await?.map(Arc::new);
            svc.solar_looks.write().await.insert(region, l.clone());
            l
        }
    };
    ctx!(svc, trace, c);
    let v = crate::land::find(&c, region, &a, look.as_deref().map(|v| v.as_slice())).await?;
    done(&trace, v, t0)
}

#[derive(serde::Deserialize, Default)]
struct WatcherParams {
    #[serde(default)]
    w: String,
}

async fn watches(State(svc): State<Shared>, Query(a): Query<WatcherParams>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let v = crate::watch::mine(&c, &a.w).await?;
    done(&trace, v, t0)
}

async fn watch_new(State(svc): State<Shared>, Query(a): Query<WatcherParams>, Json(b): Json<crate::watch::NewWatch>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let v = crate::watch::create(&c, &a.w, b).await?;
    done(&trace, v, t0)
}

async fn watch_one(State(svc): State<Shared>, Path(id): Path<String>, Query(a): Query<WatcherParams>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let v = crate::watch::one(&c, &a.w, &id).await?;
    done(&trace, v, t0)
}

async fn watch_change(State(svc): State<Shared>, Path(id): Path<String>, Query(a): Query<WatcherParams>, Json(b): Json<crate::watch::Change>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let v = crate::watch::change(&c, &a.w, &id, b).await?;
    done(&trace, v, t0)
}

/// The Park Fire's first 12 hours as three watches lived them: it never
/// changes, so it is worked out once.
async fn watch_example(State(svc): State<Shared>) -> Answer {
    if let Some(v) = svc.example.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        return Ok(Json(v));
    }
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let v = crate::watch::example(&c).await?;
    let Json(v) = done(&trace, v, t0)?;
    if v["pixels"].as_array().is_some_and(|a| !a.is_empty()) && !c.places.is_empty() {
        *svc.example.lock().unwrap_or_else(|e| e.into_inner()) = Some(v.clone());
    }
    Ok(Json(v))
}

/// A watcher's alerts as an Atom feed: the key in the address is all it
/// needs, so a feed reader can follow it.
async fn watch_feed(State(svc): State<Shared>, headers: HeaderMap, Path(key): Path<String>) -> Response {
    let key = key.trim_end_matches(".xml").trim_end_matches(".atom").to_string();
    if !crate::watch::key_ok(&key) {
        return ApiError(StatusCode::NOT_FOUND, "no such feed".into()).into_response();
    }
    let rows = match svc.db.query("SELECT id, t, title, body, fire FROM ea_watch_alerts WHERE watcher = :w ORDER BY t DESC LIMIT 50", json!({"w": key})).await {
        Ok(r) => r,
        Err(e) => return ApiError::from(e).into_response(),
    };
    let host = headers.get("x-forwarded-host").or_else(|| headers.get(header::HOST)).and_then(|v| v.to_str().ok()).unwrap_or("earth.chloetech.ai");
    let proto = headers.get("x-forwarded-proto").and_then(|v| v.to_str().ok()).unwrap_or(if host.starts_with("127.") || host.starts_with("localhost") { "http" } else { "https" });
    let host: String = host.chars().filter(|c| c.is_ascii_alphanumeric() || ".-:[]".contains(*c)).collect();
    let body = crate::watch::atom(&format!("{proto}://{host}"), &key, &rows);
    ([(header::CONTENT_TYPE, "application/atom+xml; charset=utf-8"), (header::CACHE_CONTROL, "max-age=60")], body).into_response()
}

async fn rules_page(State(svc): State<Shared>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let v = pages::rules(&c).await?;
    done(&trace, v, t0)
}

async fn data(State(svc): State<Shared>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let mut v = pages::data(&c).await?;
    v["uptime_s"] = json!(svc.started.elapsed().as_secs());
    v["public"] = json!(svc.public);
    v["places_in_memory"] = json!(c.places.len());
    done(&trace, v, t0)
}
