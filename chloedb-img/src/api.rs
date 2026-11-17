//! The app's HTTP API, its page and its thumbnails. ChloeDB's command
//! center reaches it through chloedb-server (`--app img=URL`), which loads
//! the view from here (`/app.json`, `/ui/view.js`). With `--public` the app
//! also serves the page itself at `/`, for anyone, with a limit of requests
//! a minute an address, of photos embedded and of thumbnails made.
//!
//! Every search is one statement, `ORDER BY similarity(clip, :v)` over
//! `img_pictures`, with a different vector: the words', a picture's, a
//! photo's, a detail's, a picture's plus words', a point between two
//! pictures. Every answer carries `queries`, the statements it ran with
//! their rows and time (vectors shown by their length).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, Query, Request, State};
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::clip::Clip;
use crate::colors;
use crate::pd12m::{dot, unit};
use crate::strata::{Row, RowExt, Strata, Trace};
use crate::thumbs::{self, Size, Source, Thumbs};

pub const VIEW: &str = include_str!("../ui/view.js");
pub const INDEX: &str = include_str!("../ui/index.html");

/// A picture frame with a sun and a hill.
pub const ICON: &str = r#"<rect x="3" y="4" width="18" height="16" rx="2.5"/><circle cx="9" cy="10" r="2"/><path d="M21 16l-5.5-5.5L6 20"/>"#;

/// The columns of a picture a grid shows.
pub const COLS: &str = "id, n, title, source, licence, width, height, kind, theme, url, mp";

/// The kinds a search by a photo shows apart: what it is, and how artists saw it.
const PHOTO_KINDS: &[&str] = &["Nature", "Photo"];
const ART_KINDS: &[&str] = &["Painting", "Print", "Drawing", "Illustration", "Old photo"];

pub struct Service {
    pub db: Strata,
    pub clip: Arc<Clip>,
    pub thumbs: Arc<Thumbs>,
    pub token: Option<String>,
    pub public: bool,
    pub per_minute: usize,
    /// Photos and details embedded a minute an address, and thumbnails made.
    pub photos_per_minute: usize,
    pub thumbs_per_minute: usize,
    pub seen: Mutex<HashMap<String, (i64, usize, usize, usize)>>,
    pub started: Instant,
    /// Where each picture shown lately is, so that its thumbnail is made
    /// without a lookup.
    pub sources: Mutex<HashMap<String, Source>>,
    /// The times of the last statements of each kind of search.
    pub timings: Mutex<HashMap<&'static str, Vec<f64>>>,
    /// The pictures in the table (counted at start, and again now and then).
    pub total: std::sync::atomic::AtomicU64,
    /// What the front page opens with, made once (`front`).
    pub examples: tokio::sync::OnceCell<Value>,
}

impl Service {
    pub fn new(db: Strata, clip: Arc<Clip>, thumbs: Arc<Thumbs>, token: Option<String>, public: bool, per_minute: usize, photos_per_minute: usize, thumbs_per_minute: usize) -> Service {
        Service {
            db, clip, thumbs, token, public, per_minute, photos_per_minute, thumbs_per_minute,
            seen: Mutex::new(HashMap::new()), started: Instant::now(), sources: Mutex::new(HashMap::new()),
            timings: Mutex::new(HashMap::new()), total: 0.into(), examples: tokio::sync::OnceCell::new(),
        }
    }
}

/// The front page's example searches.
pub const EXAMPLES: &[&str] = &["a lighthouse in a storm", "a red fox in the snow", "botanical drawing of a fern", "a map of an old city", "a cat asleep on a chair", "a steam locomotive"];

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

impl From<String> for ApiError {
    fn from(e: String) -> ApiError {
        ApiError(StatusCode::BAD_REQUEST, e)
    }
}

type Answer = Result<Json<Value>, ApiError>;

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn done(trace: &Trace, mut v: Value, t0: Instant) -> Answer {
    v["queries"] = json!(trace.take());
    v["took_ms"] = json!((t0.elapsed().as_secs_f64() * 1000.0 * 10.0).round() / 10.0);
    Ok(Json(v))
}

pub fn router(svc: Shared) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/app.json", get(manifest))
        .route("/ui/view.js", get(view))
        .route("/t/:id", get(small))
        .route("/l/:id", get(large))
        .route("/v1/thumb/:id", get(thumb_json))
        .route("/v1/status", get(status))
        .route("/v1/home", get(home))
        .route("/v1/search", get(search))
        .route("/v1/picture/:id", get(picture))
        .route("/v1/known/:id", get(known))
        .route("/v1/more/:id", get(more))
        .route("/v1/photo", post(photo))
        .route("/v1/detail", post(detail))
        .route("/v1/mix", get(mix))
        .route("/v1/bridge", get(bridge))
        .route("/v1/themes", get(themes))
        .route("/v1/theme/:id", get(theme))
        .route("/v1/colors", get(colors_route))
        .route("/v1/built", get(built))
        .layer(DefaultBodyLimit::max(16 << 20))
        .layer(axum::middleware::from_fn_with_state(svc.clone(), guard))
        .route("/healthz", get(|| async { "ok" }))
        .with_state(svc)
}

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// What a visitor without the token may do in public: read, and search
/// with a photo or a detail (a read sent as a POST, a photo being too big
/// for a URL).
fn public_ok(method: &Method, path: &str) -> bool {
    method == Method::GET || method == Method::HEAD || (method == Method::POST && (path == "/v1/photo" || path == "/v1/detail"))
}

async fn guard(State(svc): State<Shared>, req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    let headers = req.headers();
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        let host = headers.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");
        let same = origin.split("://").nth(1).is_some_and(|o| o == host);
        if !same && req.method() != Method::GET {
            return ApiError(StatusCode::FORBIDDEN, "requests from other sites are refused".into()).into_response();
        }
    }
    let authed = match &svc.token {
        None => true,
        Some(t) => headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")).is_some_and(|got| constant_eq(got.as_bytes(), t.as_bytes())),
    };
    if !authed {
        // Thumbnails are pictures anyone may see, even when the page is not public.
        let picture = path.starts_with("/t/") || path.starts_with("/l/");
        if !(picture || (svc.public && public_ok(req.method(), &path))) {
            return ApiError(StatusCode::UNAUTHORIZED, "this app needs its token".into()).into_response();
        }
        let who = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()).and_then(|v| v.split(',').next()).map(|s| s.trim().to_string())
            .or_else(|| req.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip().to_string())).unwrap_or_default();
        let minute = now() / 60;
        let mut seen = svc.seen.lock().unwrap_or_else(|e| e.into_inner());
        if seen.len() > 50_000 {
            seen.retain(|_, v| v.0 == minute);
        }
        let e = seen.entry(who).or_insert((minute, 0, 0, 0));
        if e.0 != minute {
            *e = (minute, 0, 0, 0);
        }
        if path.starts_with("/v1/") {
            e.1 += 1;
            if e.1 > svc.per_minute {
                return ApiError(StatusCode::TOO_MANY_REQUESTS, "too many searches from this address: wait a minute".into()).into_response();
            }
        }
        if path == "/v1/photo" || path == "/v1/detail" {
            e.2 += 1;
            if e.2 > svc.photos_per_minute {
                return ApiError(StatusCode::TOO_MANY_REQUESTS, "too many photos from this address: wait a minute".into()).into_response();
            }
        }
        if picture {
            let id = path[3..].trim_end_matches(".jpg");
            let size = if path.starts_with("/t/") { Size::Small } else { Size::Large };
            if thumbs::valid_id(id) && !svc.thumbs.have(id, size) {
                e.3 += 1;
                if e.3 > svc.thumbs_per_minute {
                    return ApiError(StatusCode::TOO_MANY_REQUESTS, "too many new pictures from this address: wait a minute".into()).into_response();
                }
            }
        }
    }
    next.run(req).await
}

fn ui_file(name: &str, compiled: &'static str) -> std::borrow::Cow<'static, str> {
    match std::env::var_os("CHLOEDB_IMG_UI_DIR") {
        Some(d) => std::fs::read_to_string(std::path::Path::new(&d).join(name)).map(Into::into).unwrap_or(compiled.into()),
        None => compiled.into(),
    }
}

async fn index(State(svc): State<Shared>) -> Response {
    if !svc.public && svc.token.is_some() {
        return (StatusCode::NOT_FOUND, "this app is shown in ChloeDB's command center (Apps → Image Explorer)").into_response();
    }
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8"), (header::CACHE_CONTROL, "no-cache")], ui_file("index.html", INDEX).into_owned()).into_response()
}

async fn manifest() -> Json<Value> {
    Json(json!({
        "name": "img",
        "title": "Image Explorer",
        "description": "12.4 million public-domain and CC0 pictures searched by what is in them: words, your photo, a detail, a picture plus words, colors and a map of themes",
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

async fn view(headers: HeaderMap) -> Response {
    let body = ui_file("view.js", VIEW);
    let tag = etag(&body);
    let fresh = headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()).is_some_and(|v| v.split(',').any(|t| t.trim().trim_start_matches("W/") == tag));
    let head = [(header::CONTENT_TYPE, "text/javascript; charset=utf-8".to_string()), (header::CACHE_CONTROL, "no-cache".to_string()), (header::ETAG, tag)];
    if fresh {
        return (StatusCode::NOT_MODIFIED, head).into_response();
    }
    (head, body.into_owned()).into_response()
}

// ---------------------------------------------------------------- helpers

/// The statements one request runs, timed, with vectors shown by length.
struct Ctx<'a> {
    svc: &'a Service,
    trace: Trace,
}

fn shown(params: &Value) -> Value {
    match params {
        Value::Object(m) => Value::Object(m.iter().map(|(k, v)| (k.clone(), match v {
            Value::Array(a) if a.len() > 16 && a.iter().all(|x| x.is_number()) => json!(format!("[{} numbers]", a.len())),
            Value::Array(a) if a.len() > 24 => json!(format!("[{} values]", a.len())),
            v => v.clone(),
        })).collect()),
        v => v.clone(),
    }
}

impl<'a> Ctx<'a> {
    fn new(svc: &'a Service) -> Ctx<'a> {
        Ctx { svc, trace: Trace::new() }
    }

    /// A statement's rows, timed under `kind` for How it's built.
    async fn q(&self, kind: &'static str, what: &str, sql: &str, params: Value) -> Result<Vec<Row>, ApiError> {
        let t0 = Instant::now();
        let r = self.svc.db.query(sql, params.clone()).await;
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        match &r {
            Ok(rows) => {
                self.trace.push(what, sql, &shown(&params), rows.len(), ms, None);
                if !kind.is_empty() {
                    let mut t = self.svc.timings.lock().unwrap_or_else(|e| e.into_inner());
                    let v = t.entry(kind).or_default();
                    if v.len() >= 1000 {
                        v.remove(0);
                    }
                    v.push(ms);
                }
            }
            Err(e) => self.trace.push(what, sql, &shown(&params), 0, ms, Some(&format!("refused: {}", e.message))),
        }
        Ok(r?)
    }

    /// Pictures as the grids show them, kept as where to make thumbnails.
    fn pics(&self, rows: &[Row]) -> Vec<Value> {
        let mut src = self.svc.sources.lock().unwrap_or_else(|e| e.into_inner());
        if src.len() > 200_000 {
            src.clear();
        }
        rows.iter().map(|r| {
            src.insert(r.s("id"), Source { url: r.s("url"), kind: r.s("kind"), source: r.s("source"), mp: r.f("mp") });
            pic(r)
        }).collect()
    }
}

/// A picture as the page shows it.
pub fn pic(r: &Row) -> Value {
    let mut v = json!({
        "id": r.s("id"), "title": r.s("title"), "source": r.s("source"), "licence": r.s("licence"),
        "w": r.i("width"), "h": r.i("height"), "kind": r.s("kind"), "theme": r.i("theme"),
    });
    if let Some(s) = r.get("s").and_then(|s| s.as_f64()) {
        v["s"] = json!((s * 1000.0).round() / 1000.0);
    }
    v
}

fn vec_of(r: &Row, k: &str) -> Vec<f32> {
    r.get(k).and_then(|v| v.as_array()).map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0) as f32).collect()).unwrap_or_default()
}

fn vjson(v: &[f32]) -> Value {
    json!(v)
}

/// `a + w·b`, unit length.
fn plus(a: &[f32], b: &[f32], w: f32) -> Vec<f32> {
    let mut v: Vec<f32> = a.iter().zip(b).map(|(x, y)| x + w * y).collect();
    unit(&mut v);
    v
}

fn mean(vs: &[Vec<f32>]) -> Option<Vec<f32>> {
    let first = vs.first()?;
    let mut m = vec![0f32; first.len()];
    for v in vs {
        m.iter_mut().zip(v).for_each(|(a, b)| *a += b);
    }
    unit(&mut m);
    Some(m)
}

fn ids_of(s: &str) -> Vec<String> {
    s.split(',').map(str::trim).filter(|x| thumbs::valid_id(x)).take(40).map(str::to_string).collect()
}

/// The filters a grid takes, as SQL over `img_pictures` and its parameters.
#[derive(serde::Deserialize, Default, Clone)]
pub struct Filters {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    source: String,
    #[serde(default)]
    licence: String,
    #[serde(default)]
    shape: String,
    #[serde(default)]
    size: String,
    #[serde(default)]
    theme: Option<i64>,
}

impl Filters {
    fn sql(&self, p: &mut serde_json::Map<String, Value>) -> String {
        let mut w: Vec<String> = Vec::new();
        let list = |s: &str| -> Vec<String> { s.split(',').map(str::trim).filter(|x| !x.is_empty()).take(12).map(str::to_string).collect() };
        for (col, val, key) in [("kind", &self.kind, "f_kind"), ("source", &self.source, "f_source"), ("licence", &self.licence, "f_licence"), ("shape", &self.shape, "f_shape"), ("size", &self.size, "f_size")] {
            let l = list(val);
            if !l.is_empty() {
                w.push(format!("{col} IN (:{key})"));
                p.insert(key.into(), json!(l));
            }
        }
        if let Some(t) = self.theme {
            w.push("theme = :f_theme".into());
            p.insert("f_theme".into(), json!(t));
        }
        match w.is_empty() {
            true => String::new(),
            false => format!("WHERE {}", w.join(" AND ")),
        }
    }
}

/// The pictures closest to `v` that pass `f`: one statement.
async fn closest(c: &Ctx<'_>, kind: &'static str, what: &str, v: &[f32], f: &Filters, k: usize, offset: usize) -> Result<Vec<Row>, ApiError> {
    let mut p = serde_json::Map::new();
    let w = f.sql(&mut p);
    p.insert("v".into(), vjson(v));
    let sql = format!("SELECT {COLS}, score() AS s FROM img_pictures {w} ORDER BY similarity(clip, :v) DESC LIMIT {k} OFFSET {offset}");
    c.q(kind, what, &sql, Value::Object(p)).await
}

/// A picture's row and vector.
async fn one(c: &Ctx<'_>, id: &str) -> Result<Row, ApiError> {
    if !thumbs::valid_id(id) {
        return Err(ApiError(StatusCode::NOT_FOUND, format!("no picture {id}")));
    }
    let rows = c.q("", "the picture", &format!("SELECT {COLS}, caption, hash, mime, clip FROM img_pictures WHERE id = :id"), json!({"id": id})).await?;
    let r = rows.into_iter().next().ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("no picture {id}")))?;
    c.pics(std::slice::from_ref(&r));
    Ok(r)
}

/// Several pictures' vectors.
async fn vectors(c: &Ctx<'_>, ids: &[String]) -> Result<Vec<Vec<f32>>, ApiError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = c.q("", "the vectors of the pictures liked and hidden", "SELECT id, clip FROM img_pictures WHERE id IN (:ids)", json!({"ids": ids})).await?;
    Ok(rows.iter().map(|r| vec_of(r, "clip")).filter(|v| !v.is_empty()).collect())
}

// ---------------------------------------------------------------- thumbnails

async fn source_of(svc: &Service, id: &str) -> Option<Source> {
    if let Some(s) = svc.sources.lock().unwrap_or_else(|e| e.into_inner()).get(id) {
        return Some(s.clone());
    }
    let rows = svc.db.query("SELECT url, kind, source, mp FROM img_pictures WHERE id = :id", json!({"id": id})).await.ok()?;
    let r = rows.first()?;
    Some(Source { url: r.s("url"), kind: r.s("kind"), source: r.s("source"), mp: r.f("mp") })
}

async fn thumb(svc: Shared, id: String, size: Size) -> Response {
    let id = id.trim_end_matches(".jpg").to_string();
    if !thumbs::valid_id(&id) {
        return (StatusCode::NOT_FOUND, "no such picture").into_response();
    }
    let src = match svc.thumbs.have(&id, size) {
        true => Source::default(),
        false => match source_of(&svc, &id).await {
            Some(s) => s,
            None => return (StatusCode::NOT_FOUND, "no such picture").into_response(),
        },
    };
    match svc.thumbs.get(&id, size, &src).await {
        Some(b) => ([(header::CONTENT_TYPE, "image/jpeg"), (header::CACHE_CONTROL, "public, max-age=31536000, immutable")], b).into_response(),
        None => ([(header::CACHE_CONTROL, "public, max-age=3600")], (StatusCode::NOT_FOUND, "this picture's file could not be read")).into_response(),
    }
}

async fn small(State(svc): State<Shared>, Path(id): Path<String>) -> Response {
    thumb(svc, id, Size::Small).await
}

async fn large(State(svc): State<Shared>, Path(id): Path<String>) -> Response {
    thumb(svc, id, Size::Large).await
}

#[derive(serde::Deserialize, Default)]
struct ThumbQ {
    #[serde(default)]
    size: String,
}

/// A thumbnail as base64 in JSON, for the command center, whose requests
/// carry a key that an image's own request cannot.
async fn thumb_json(State(svc): State<Shared>, Path(id): Path<String>, Query(q): Query<ThumbQ>) -> Answer {
    use base64::Engine;
    let size = if q.size == "large" { Size::Large } else { Size::Small };
    let src = match svc.thumbs.have(&id, size) {
        true => Source::default(),
        false => source_of(&svc, &id).await.ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("no picture {id}")))?,
    };
    let b = svc.thumbs.get(&id, size, &src).await.ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "this picture's file could not be read".into()))?;
    Ok(Json(json!({"mime": "image/jpeg", "data": base64::engine::general_purpose::STANDARD.encode(b)})))
}

// ---------------------------------------------------------------- pages

async fn status(State(svc): State<Shared>) -> Answer {
    let t0 = Instant::now();
    let c = Ctx::new(&svc);
    let n = c.q("", "how many pictures", "SELECT COUNT(*) AS n FROM img_pictures", Value::Null).await?;
    done(&c.trace, json!({"pictures": n.first().map(|r| r.i("n")).unwrap_or(0), "public": svc.public, "model": svc.clip.name(), "uptime_s": svc.started.elapsed().as_secs()}), t0)
}

/// The front page: what the collection holds, and the pictures it opens
/// with (made once: see `front`).
async fn home(State(svc): State<Shared>) -> Answer {
    let t0 = Instant::now();
    let c = Ctx::new(&svc);
    let by_source = c.q("", "the pictures by collection", "SELECT source, COUNT(*) AS n FROM img_pictures GROUP BY source ORDER BY n DESC LIMIT 1000", Value::Null).await?;
    let total: i64 = by_source.iter().map(|r| r.i("n")).sum();
    let count = |name: &str| by_source.iter().find(|r| r.s("source") == name).map(|r| r.i("n")).unwrap_or(0);
    svc.total.store(total as u64, std::sync::atomic::Ordering::Relaxed);
    let mut out = svc.examples.get_or_try_init(|| front(&c)).await.cloned().unwrap_or_else(|_| json!({}));
    for (k, v) in [("total", json!(total)), ("collections", json!(by_source.len())), ("commons", json!(count("Wikimedia Commons"))), ("inaturalist", json!(count("iNaturalist"))), ("museums", json!(total - count("Wikimedia Commons") - count("iNaturalist")))] {
        out[k] = v;
    }
    done(&c.trace, out, t0)
}

/// The first picture `q` finds.
async fn first_for(c: &Ctx<'_>, q: &str) -> Result<Option<Value>, ApiError> {
    let v = c.svc.clip.text(q).await?;
    let rows = closest(c, "", &format!("the first picture for “{q}”"), &v, &Filters::default(), 1, 0).await?;
    Ok(c.pics(&rows).into_iter().next())
}

/// What the front page and the empty pages open with: the example
/// searches, a few striking pictures of each theme (of the nine nearest
/// its middle, the two CLIP finds most like "a beautiful picture"), and a
/// picture to draw on, one to add words to and two to bridge.
async fn front(c: &Ctx<'_>) -> Result<Value, ApiError> {
    let mut examples = Vec::new();
    for q in EXAMPLES {
        examples.push(json!({"q": q, "pic": first_for(c, q).await?}));
    }
    let themes = c.q("", "the themes", "SELECT id, grp, reps FROM img_themes ORDER BY n DESC", Value::Null).await?;
    let ns: Vec<i64> = themes.iter().flat_map(|t| t.get("reps").and_then(|r| r.as_array()).into_iter().flatten().take(9).filter_map(|x| x.as_i64())).collect();
    let rows = if ns.is_empty() { Vec::new() } else { c.q("", "the pictures nearest each theme's middle", &format!("SELECT {COLS}, clip FROM img_pictures WHERE n IN (:ns)"), json!({"ns": ns})).await? };
    let fine = c.svc.clip.text("a beautiful, striking, high quality picture").await?;
    let by_n: HashMap<i64, &Row> = rows.iter().map(|r| (r.i("n"), r)).collect();
    // Each theme's best two, the best first, art, photos and nature in turn.
    let mut firsts: Vec<(f32, String, Row)> = Vec::new();
    let mut seconds: Vec<(f32, String, Row)> = Vec::new();
    for t in &themes {
        let mut mine: Vec<(f32, &Row)> = t.get("reps").and_then(|r| r.as_array()).into_iter().flatten().take(9).filter_map(|x| x.as_i64()).filter_map(|n| by_n.get(&n).copied()).map(|r| (dot(&vec_of(r, "clip"), &fine), r)).collect();
        mine.sort_by(|a, b| b.0.total_cmp(&a.0));
        let mut it = mine.into_iter();
        if let Some((s, r)) = it.next() {
            firsts.push((s, t.s("grp"), r.clone()));
        }
        if let Some((s, r)) = it.next() {
            seconds.push((s, t.s("grp"), r.clone()));
        }
    }
    let turns = |mut l: Vec<(f32, String, Row)>| -> Vec<Row> {
        l.sort_by(|a, b| b.0.total_cmp(&a.0));
        let mut groups: Vec<(String, std::collections::VecDeque<Row>)> = Vec::new();
        for (_, g, r) in l {
            match groups.iter_mut().find(|x| x.0 == g) {
                Some(x) => x.1.push_back(r),
                None => groups.push((g, std::collections::VecDeque::from([r]))),
            }
        }
        let mut out = Vec::new();
        while groups.iter().any(|g| !g.1.is_empty()) {
            for g in groups.iter_mut() {
                if let Some(r) = g.1.pop_front() {
                    out.push(r);
                }
            }
        }
        out
    };
    let few: Vec<Row> = turns(firsts).into_iter().chain(turns(seconds)).collect();
    let detail = first_for(c, "a painting of a busy harbour with sailing ships, boats and people on the quay").await?;
    let mix = first_for(c, "a painting of a Dutch river landscape with boats in summer").await?;
    let (a, b) = (first_for(c, "a blue beetle on a green leaf").await?, first_for(c, "a gold pendant with pearls and rubies").await?);
    Ok(json!({
        "examples": examples, "few": c.pics(&few),
        "detail": detail, "mix": {"pic": mix, "words": "in winter, snow and ice"}, "bridge": {"a": a, "b": b},
    }))
}

#[derive(serde::Deserialize, Default)]
struct SearchQ {
    #[serde(default)]
    q: String,
    #[serde(default)]
    like: String,
    #[serde(default)]
    hide: String,
    #[serde(default)]
    steer: String,
    #[serde(default)]
    order: String,
    #[serde(default)]
    page: usize,
    #[serde(flatten)]
    f: Filters,
}

const PER_PAGE: usize = 48;

/// Facet counts of the closest 2,000.
fn facets(rows: &[Row]) -> Value {
    let mut out = serde_json::Map::new();
    for col in ["kind", "source", "licence", "shape", "size"] {
        let mut m: HashMap<String, usize> = HashMap::new();
        for r in rows {
            let v = r.s(col);
            if !v.is_empty() {
                *m.entry(v).or_default() += 1;
            }
        }
        let mut l: Vec<(String, usize)> = m.into_iter().collect();
        l.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        l.truncate(if col == "source" { 12 } else { 10 });
        out.insert(col.into(), json!(l.into_iter().map(|(k, n)| json!({"v": k, "n": n})).collect::<Vec<_>>()));
    }
    Value::Object(out)
}

/// Maximal marginal relevance: from the closest, the next one that is
/// close to the query and least like those already taken.
fn varied(rows: Vec<Row>, k: usize) -> Vec<Row> {
    let vs: Vec<Vec<f32>> = rows.iter().map(|r| vec_of(r, "clip")).collect();
    let rel: Vec<f32> = rows.iter().map(|r| r.f("s") as f32).collect();
    let mut taken: Vec<usize> = Vec::new();
    let mut left: Vec<usize> = (0..rows.len()).collect();
    while taken.len() < k && !left.is_empty() {
        let (bi, _) = left.iter().enumerate().map(|(i, &j)| {
            let sim = taken.iter().map(|&t| dot(&vs[j], &vs[t])).fold(f32::MIN, f32::max);
            (i, 0.65 * rel[j] - 0.35 * if taken.is_empty() { 0.0 } else { sim })
        }).max_by(|a, b| a.1.total_cmp(&b.1)).expect("one left");
        taken.push(left.remove(bi));
    }
    let mut rows: Vec<Option<Row>> = rows.into_iter().map(Some).collect();
    taken.into_iter().filter_map(|i| rows[i].take()).collect()
}

/// A search by words, steered by the pictures liked and hidden, with its
/// facets over the closest 2,000.
async fn search(State(svc): State<Shared>, Query(a): Query<SearchQ>) -> Answer {
    let t0 = Instant::now();
    let c = Ctx::new(&svc);
    let q = a.q.trim();
    if q.is_empty() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "search with some words".into()));
    }
    if q.chars().count() > 300 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "at most 300 characters".into()));
    }
    let text = svc.clip.text(q).await?;
    let (like, hide) = (ids_of(&a.like), ids_of(&a.hide));
    let (lv, hv) = tokio::join!(vectors(&c, &like), vectors(&c, &hide));
    let (lv, hv) = (lv?, hv?);
    let strong = a.steer == "strong";
    let mut v: Vec<f32> = text.to_vec();
    if let Some(m) = mean(&lv) {
        v = plus(&v, &m, if strong { 0.9 } else { 0.45 });
    }
    if let Some(m) = mean(&hv) {
        v = plus(&v, &m, if strong { -0.7 } else { -0.35 });
    }
    let page = a.page.min(40);
    let mut fp = serde_json::Map::new();
    fp.insert("v".into(), vjson(&v));
    let facet_sql = "SELECT kind, source, licence, shape, size FROM img_pictures ORDER BY similarity(clip, :v) DESC LIMIT 2000";
    let variety = a.order == "variety";
    let (facet_rows, rows) = tokio::join!(
        c.q("facets", "the facets of the 2,000 closest", facet_sql, Value::Object(fp)),
        async {
            match variety {
                true => {
                    let mut p = serde_json::Map::new();
                    let w = a.f.sql(&mut p);
                    p.insert("v".into(), vjson(&v));
                    let sql = format!("SELECT {COLS}, clip, score() AS s FROM img_pictures {w} ORDER BY similarity(clip, :v) DESC LIMIT {} OFFSET {}", PER_PAGE * 4, page * PER_PAGE * 4);
                    c.q("words", "the closest pictures, to pick a varied page from", &sql, Value::Object(p)).await.map(|r| varied(r, PER_PAGE))
                }
                false => closest(&c, "words", "the closest pictures", &v, &a.f, PER_PAGE + hide.len(), page * PER_PAGE).await,
            }
        }
    );
    let rows: Vec<Row> = rows?.into_iter().filter(|r| !hide.contains(&r.s("id"))).take(PER_PAGE).collect();
    let total = svc.total.load(std::sync::atomic::Ordering::Relaxed);
    done(&c.trace, json!({
        "q": q, "pictures": c.pics(&rows), "facets": facets(&facet_rows.unwrap_or_default()),
        "liked": like, "hidden": hide, "page": page, "more": rows.len() == PER_PAGE, "compared": total,
    }), t0)
}

/// The words of a picture's subject, from its title ("A painting of a
/// ship in rough seas" → "a ship in rough seas").
fn subject(title: &str) -> String {
    let t = title.trim_end_matches('…');
    match t.find(" of ") {
        Some(i) if i < 40 => t[i + 4..].to_string(),
        _ => t.to_string(),
    }
}

/// The media another one is shown in, and the words for each.
const MEDIA: &[(&str, &str, &[&str])] = &[
    ("As a pencil drawing", "a pencil drawing of", &["Drawing"]),
    ("As an oil painting", "an oil painting of", &["Painting"]),
    ("As an old photograph", "an old black and white photograph of", &["Old photo"]),
    ("As an engraving", "an engraving of", &["Print"]),
    ("As a photograph today", "a color photograph of", &["Photo", "Nature"]),
];

/// A picture: its record, its colors, its page on Commons, the same picture
/// in other collections, more like it, and its subject in other media.
#[derive(serde::Deserialize, Default)]
struct PictureQ {
    /// Only the record, for a page that shows the picture for another use.
    #[serde(default)]
    brief: Option<String>,
}

async fn picture(State(svc): State<Shared>, Path(id): Path<String>, Query(pq): Query<PictureQ>) -> Answer {
    let t0 = Instant::now();
    let c = Ctx::new(&svc);
    let r = one(&c, &id).await?;
    if pq.brief.is_some() {
        let mut p = pic(&r);
        p["mp"] = json!(r.f("mp"));
        return done(&c.trace, json!({"picture": p}), t0);
    }
    let v = vec_of(&r, "clip");
    let src = r.s("source");
    let kind = r.s("kind");
    let subj = subject(&r.s("title"));
    let media: Vec<(&str, &str, &[&str])> = MEDIA.iter().filter(|m| !m.2.contains(&kind.as_str())).take(4).map(|m| (m.0, m.1, m.2)).collect();
    let none = Filters::default();
    let (more, same, colors, link, theme) = tokio::join!(
        closest(&c, "picture", "more like this", &v, &none, 25, 0),
        async {
            let mut p = serde_json::Map::new();
            p.insert("v".into(), vjson(&v));
            p.insert("src".into(), json!(src));
            c.q("", "the same picture in other collections", &format!("SELECT {COLS}, score() AS s FROM img_pictures WHERE source <> :src ORDER BY similarity(clip, :v) DESC LIMIT 6"), Value::Object(p)).await
        },
        c.q("", "its colors", "SELECT palette FROM img_colors WHERE id = :id", json!({"id": id})),
        c.q("", "its page at the collection", "SELECT page, title, found FROM img_links WHERE id = :id", json!({"id": id})),
        c.q("", "its theme", "SELECT id, name, n FROM img_themes WHERE id = :t", json!({"t": r.i("theme")})),
    );
    let mut other = Vec::new();
    // Its subject in another medium: the picture's vector plus the words',
    // among the pictures of that kind.
    let texts: Vec<_> = media.iter().map(|(_, words, _)| {
        let (clip, t) = (svc.clip.clone(), format!("{words} {subj}"));
        tokio::spawn(async move { clip.text(&t).await })
    }).collect();
    for ((label, words, kinds), tv) in media.iter().zip(texts) {
        let tv = tv.await.map_err(|e| e.to_string())??;
        let mv = plus(&v, &tv, 1.0);
        let f = Filters { kind: kinds.join(","), ..Filters::default() };
        let rows = closest(&c, "", &format!("its subject {}", label.to_lowercase()), &mv, &f, 9, 0).await?;
        let rows: Vec<Row> = rows.into_iter().filter(|x| x.s("id") != id).take(8).collect();
        other.push(json!({"label": label, "words": format!("{words} {subj}"), "pictures": c.pics(&rows)}));
    }
    let more: Vec<Row> = more?.into_iter().filter(|x| x.s("id") != id).take(24).collect();
    let same: Vec<Row> = same?.into_iter().filter(|x| x.f("s") >= 0.93).collect();
    let palette: Value = colors.ok().and_then(|c| c.first().map(|r| r.s("palette"))).and_then(|p| serde_json::from_str(&p).ok()).unwrap_or(Value::Null);
    let link = link.ok().and_then(|l| l.into_iter().next()).filter(|l| l.get("found").and_then(|f| f.as_bool()).unwrap_or(false)).map(|l| json!({"page": l.s("page"), "title": l.s("title")}));
    let mut p = pic(&r);
    p["caption"] = json!(r.s("caption"));
    p["mp"] = json!(r.f("mp"));
    p["file"] = json!(r.s("url"));
    p["mime"] = json!(r.s("mime"));
    p["colors"] = palette;
    p["link"] = json!(link);
    done(&c.trace, json!({
        "picture": p, "theme": theme.ok().and_then(|t| t.into_iter().next()).map(|t| json!({"id": t.i("id"), "name": t.s("name"), "n": t.i("n")})),
        "more": c.pics(&more), "same": c.pics(&same), "other": other,
    }), t0)
}

/// What is learnt about a picture when its copy is made: its colors and
/// its page on Commons.
async fn known(State(svc): State<Shared>, Path(id): Path<String>) -> Answer {
    let t0 = Instant::now();
    let c = Ctx::new(&svc);
    if !thumbs::valid_id(&id) {
        return Err(ApiError(StatusCode::NOT_FOUND, format!("no picture {id}")));
    }
    let (colors, link) = tokio::join!(
        c.q("", "its colors", "SELECT palette FROM img_colors WHERE id = :id", json!({"id": id})),
        c.q("", "its page at the collection", "SELECT page, title, found FROM img_links WHERE id = :id", json!({"id": id})),
    );
    let palette: Value = colors.ok().and_then(|c| c.first().map(|r| r.s("palette"))).and_then(|p| serde_json::from_str(&p).ok()).unwrap_or(Value::Null);
    let link = link.ok().and_then(|l| l.into_iter().next()).filter(|l| l.get("found").and_then(|f| f.as_bool()).unwrap_or(false)).map(|l| json!({"page": l.s("page"), "title": l.s("title")}));
    done(&c.trace, json!({"colors": palette, "link": link}), t0)
}

#[derive(serde::Deserialize, Default)]
struct PageQ {
    #[serde(default)]
    page: usize,
    #[serde(flatten)]
    f: Filters,
}

/// More like a picture, a page at a time.
async fn more(State(svc): State<Shared>, Path(id): Path<String>, Query(a): Query<PageQ>) -> Answer {
    let t0 = Instant::now();
    let c = Ctx::new(&svc);
    let r = one(&c, &id).await?;
    let v = vec_of(&r, "clip");
    let page = a.page.min(40);
    let rows = closest(&c, "picture", "more like this", &v, &a.f, PER_PAGE + 1, page * PER_PAGE).await?;
    let rows: Vec<Row> = rows.into_iter().filter(|x| x.s("id") != id).take(PER_PAGE).collect();
    done(&c.trace, json!({"of": pic(&r), "pictures": c.pics(&rows), "page": page, "more": rows.len() == PER_PAGE}), t0)
}

#[derive(serde::Deserialize)]
struct PhotoBody {
    /// The photo, base64.
    data: String,
    #[serde(default)]
    words: String,
}

fn decode_photo(b64: &str) -> Result<image::RgbImage, ApiError> {
    use base64::Engine;
    let b64 = b64.split_once("base64,").map(|x| x.1).unwrap_or(b64);
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim()).map_err(|_| ApiError(StatusCode::BAD_REQUEST, "the photo is not base64".into()))?;
    if bytes.len() > 12 << 20 {
        return Err(ApiError(StatusCode::PAYLOAD_TOO_LARGE, "a photo of 12 MB at most".into()));
    }
    let img = image::load_from_memory(&bytes).map_err(|e| ApiError(StatusCode::BAD_REQUEST, format!("not a picture this site reads (JPEG, PNG, WebP, GIF): {e}")))?;
    // A huge photo is made smaller before CLIP, which sees 224 pixels anyway.
    let img = if img.width().max(img.height()) > 1600 { img.thumbnail(1600, 1600) } else { img };
    Ok(img.to_rgb8())
}

/// Pictures closest to a vector, in the groups a photo is shown in.
async fn groups(c: &Ctx<'_>, kind: &'static str, v: &[f32], skip: &str) -> Result<Value, ApiError> {
    let f = |kinds: &[&str]| Filters { kind: kinds.join(","), ..Filters::default() };
    let (none, photos, arts) = (Filters::default(), f(PHOTO_KINDS), f(ART_KINDS));
    let (all, nature, art) = tokio::join!(
        closest(c, kind, "the closest pictures", v, &none, 25, 0),
        closest(c, "", "the closest photos", v, &photos, 13, 0),
        closest(c, "", "the closest paintings, prints, drawings and old photos", v, &arts, 13, 0),
    );
    let keep = |rows: Vec<Row>, k: usize| -> Vec<Row> { rows.into_iter().filter(|r| r.s("id") != skip).take(k).collect() };
    Ok(json!({
        "pictures": c.pics(&keep(all?, 24)),
        "photos": c.pics(&keep(nature?, 12)),
        "art": c.pics(&keep(art?, 12)),
    }))
}

/// A search with a photo of one's own: it is embedded and thrown away.
async fn photo(State(svc): State<Shared>, Json(b): Json<PhotoBody>) -> Answer {
    let t0 = Instant::now();
    let c = Ctx::new(&svc);
    let img = decode_photo(&b.data)?;
    let (w, h) = img.dimensions();
    let te = Instant::now();
    let mut v = svc.clip.image(img).await?;
    let embed_ms = te.elapsed().as_secs_f64() * 1000.0;
    if !b.words.trim().is_empty() {
        let t = svc.clip.text(b.words.trim()).await?;
        v = plus(&v, &t, 1.0);
    }
    let mut out = groups(&c, "photo", &v, "").await?;
    out["photo"] = json!({"w": w, "h": h, "embed_ms": (embed_ms * 10.0).round() / 10.0});
    done(&c.trace, out, t0)
}

#[derive(serde::Deserialize)]
struct DetailBody {
    id: String,
    /// The box, as fractions of the picture: left, top, right, bottom.
    #[serde(rename = "box")]
    rect: [f64; 4],
    #[serde(default)]
    kind: String,
}

/// A search for what is inside a box drawn on a picture.
async fn detail(State(svc): State<Shared>, Json(b): Json<DetailBody>) -> Answer {
    let t0 = Instant::now();
    let c = Ctx::new(&svc);
    let r = one(&c, &b.id).await?;
    let src = Source { url: r.s("url"), kind: r.s("kind"), source: r.s("source"), mp: r.f("mp") };
    let bytes = svc.thumbs.get(&b.id, Size::Large, &src).await.ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "this picture's file could not be read".into()))?;
    let img = image::load_from_memory(&bytes).map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?.to_rgb8();
    let (w, h) = (img.width() as f64, img.height() as f64);
    let [l, t, rr, bb] = b.rect.map(|x| x.clamp(0.0, 1.0));
    let (x0, y0) = ((l.min(rr) * w) as u32, (t.min(bb) * h) as u32);
    let (cw, ch) = ((((rr - l).abs() * w) as u32).max(16), (((bb - t).abs() * h) as u32).max(16));
    let cw = cw.min(img.width() - x0.min(img.width() - 1));
    let ch = ch.min(img.height() - y0.min(img.height() - 1));
    let crop = image::imageops::crop_imm(&img, x0.min(img.width() - 1), y0.min(img.height() - 1), cw, ch).to_image();
    let te = Instant::now();
    let v = svc.clip.image(crop).await?;
    let embed_ms = te.elapsed().as_secs_f64() * 1000.0;
    let mut out = match b.kind.as_str() {
        "" => groups(&c, "detail", &v, &b.id).await?,
        k => {
            let f = Filters { kind: k.to_string(), ..Filters::default() };
            let rows = closest(&c, "detail", "the closest pictures", &v, &f, 25, 0).await?;
            let rows: Vec<Row> = rows.into_iter().filter(|x| x.s("id") != b.id).take(24).collect();
            json!({"pictures": c.pics(&rows)})
        }
    };
    out["of"] = pic(&r);
    out["embed_ms"] = json!((embed_ms * 10.0).round() / 10.0);
    done(&c.trace, out, t0)
}

#[derive(serde::Deserialize, Default)]
struct MixQ {
    id: String,
    #[serde(default)]
    q: String,
    /// How much of the words: 0 to 1.
    #[serde(default)]
    w: Option<f32>,
}

/// A picture plus words: the picture's vector moved toward the words'.
async fn mix(State(svc): State<Shared>, Query(a): Query<MixQ>) -> Answer {
    let t0 = Instant::now();
    let c = Ctx::new(&svc);
    let r = one(&c, &a.id).await?;
    let v = vec_of(&r, "clip");
    let q = a.q.trim();
    if q.is_empty() || q.chars().count() > 200 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "add some words (200 characters at most)".into()));
    }
    let t = svc.clip.text(q).await?;
    let x = a.w.unwrap_or(0.5).clamp(0.0, 1.0);
    let weight = 0.3 + 1.7 * x;
    let mv = plus(&v, &t, weight);
    let rows = closest(&c, "mix", "the closest to the picture plus the words", &mv, &Filters::default(), 25, 0).await?;
    let rows: Vec<Row> = rows.into_iter().filter(|x| x.s("id") != a.id).take(24).collect();
    done(&c.trace, json!({"of": pic(&r), "q": q, "w": x, "weight": weight, "pictures": c.pics(&rows)}), t0)
}

#[derive(serde::Deserialize, Default)]
struct BridgeQ {
    a: String,
    b: String,
    #[serde(default)]
    steps: Option<usize>,
}

/// The pictures on the way between two: the closest to each point on the
/// great circle between their vectors, each used once.
async fn bridge(State(svc): State<Shared>, Query(q): Query<BridgeQ>) -> Answer {
    let t0 = Instant::now();
    let c = Ctx::new(&svc);
    let (ra, rb) = tokio::join!(one(&c, &q.a), one(&c, &q.b));
    let (ra, rb) = (ra?, rb?);
    let (va, vb) = (vec_of(&ra, "clip"), vec_of(&rb, "clip"));
    let steps = q.steps.unwrap_or(7).clamp(3, 11);
    let om = dot(&va, &vb).clamp(-1.0, 1.0).acos();
    let mut used = vec![q.a.clone(), q.b.clone()];
    let mut path = Vec::new();
    for i in 1..=steps {
        let t = i as f32 / (steps + 1) as f32;
        let mut v: Vec<f32> = match om.sin().abs() < 1e-4 {
            true => va.clone(),
            false => va.iter().zip(&vb).map(|(a, b)| (((1.0 - t) * om).sin() * a + (t * om).sin() * b) / om.sin()).collect(),
        };
        unit(&mut v);
        let rows = closest(&c, "bridge", &format!("step {i} of {steps}"), &v, &Filters::default(), 6 + used.len(), 0).await?;
        if let Some(r) = rows.into_iter().find(|r| !used.contains(&r.s("id"))) {
            used.push(r.s("id"));
            path.push(r);
        }
    }
    done(&c.trace, json!({"a": c.pics(&[ra])[0], "b": c.pics(&[rb])[0], "steps": steps, "path": c.pics(&path)}), t0)
}

/// Every theme, for the map, with the pictures nearest each one's middle.
async fn themes(State(svc): State<Shared>) -> Answer {
    let t0 = Instant::now();
    let c = Ctx::new(&svc);
    let ts = c.q("", "the themes", "SELECT id, name, n, grp, row, col, reps, near FROM img_themes ORDER BY id", Value::Null).await?;
    let ns: Vec<i64> = ts.iter().flat_map(|t| t.get("reps").and_then(|r| r.as_array()).into_iter().flatten().take(9).filter_map(|x| x.as_i64())).collect();
    let rows = if ns.is_empty() { Vec::new() } else { c.q("", "the pictures nearest each theme's middle", &format!("SELECT {COLS} FROM img_pictures WHERE n IN (:ns)"), json!({"ns": ns})).await? };
    let pics: HashMap<i64, Value> = rows.iter().map(|r| (r.i("n"), r)).zip(c.pics(&rows)).map(|((n, _), p)| (n, p)).collect();
    let out: Vec<Value> = ts.iter().map(|t| {
        let reps: Vec<Value> = t.get("reps").and_then(|r| r.as_array()).into_iter().flatten().take(9).filter_map(|x| x.as_i64()).filter_map(|n| pics.get(&n).cloned()).collect();
        json!({"id": t.i("id"), "name": t.s("name"), "n": t.i("n"), "group": t.s("grp"), "row": t.i("row"), "col": t.i("col"), "near": t.get("near"), "reps": reps})
    }).collect();
    let total = svc.total.load(std::sync::atomic::Ordering::Relaxed);
    done(&c.trace, json!({"themes": out, "total": total}), t0)
}

#[derive(serde::Deserialize, Default)]
struct ThemeQ {
    #[serde(default)]
    q: String,
    #[serde(default)]
    page: usize,
    #[serde(default)]
    kind: String,
}

/// One theme: its pictures nearest its middle, or nearest some words.
async fn theme(State(svc): State<Shared>, Path(id): Path<i64>, Query(a): Query<ThemeQ>) -> Answer {
    let t0 = Instant::now();
    let c = Ctx::new(&svc);
    let ts = c.q("", "the theme", "SELECT id, name, n, grp, near, kinds, centre FROM img_themes WHERE id = :id", json!({"id": id})).await?;
    let t = ts.into_iter().next().ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("no theme {id}")))?;
    let near_ids: Vec<i64> = t.get("near").and_then(|n| n.as_array()).into_iter().flatten().filter_map(|x| x.as_i64()).collect();
    let near = if near_ids.is_empty() { Vec::new() } else { c.q("", "the themes next to it", "SELECT id, name, n FROM img_themes WHERE id IN (:ids)", json!({"ids": near_ids})).await? };
    let v: Vec<f32> = match a.q.trim() {
        "" => vec_of(&t, "centre"),
        q => svc.clip.text(q).await?.to_vec(),
    };
    let f = Filters { theme: Some(id), kind: a.kind.clone(), ..Filters::default() };
    let page = a.page.min(40);
    let rows = closest(&c, "theme", if a.q.trim().is_empty() { "its pictures nearest its middle" } else { "its pictures nearest the words" }, &v, &f, PER_PAGE, page * PER_PAGE).await?;
    let kinds: Value = serde_json::from_str(&t.s("kinds")).unwrap_or(Value::Null);
    done(&c.trace, json!({
        "theme": {"id": t.i("id"), "name": t.s("name"), "n": t.i("n"), "group": t.s("grp"), "kinds": kinds},
        "near": near.iter().map(|n| json!({"id": n.i("id"), "name": n.s("name"), "n": n.i("n")})).collect::<Vec<_>>(),
        "q": a.q.trim(), "pictures": c.pics(&rows), "page": page, "more": rows.len() == PER_PAGE,
    }), t0)
}

#[derive(serde::Deserialize, Default)]
struct ColorsQ {
    /// Up to three colors, `1f3a6e,e8792b` (a weight after a colon: `1f3a6e:2`).
    c: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    page: usize,
}

/// Pictures ranked by how much of the picked colors they hold, among those
/// whose colors are known (each picture's, once its thumbnail is made).
async fn colors_route(State(svc): State<Shared>, Query(a): Query<ColorsQ>) -> Answer {
    let t0 = Instant::now();
    let c = Ctx::new(&svc);
    let picks: Vec<([u8; 3], f32)> = a.c.split(',').filter_map(|p| {
        let (h, w) = p.split_once(':').unwrap_or((p, "1"));
        Some((colors::parse_hex(h)?, w.parse::<f32>().ok().unwrap_or(1.0).clamp(0.1, 5.0)))
    }).take(3).collect();
    if picks.is_empty() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "pick a color: c=1f3a6e,e8792b".into()));
    }
    let v = colors::query(&picks);
    let mut p = serde_json::Map::new();
    p.insert("c".into(), vjson(&v));
    let kinds: Vec<String> = a.kind.split(',').map(str::trim).filter(|x| !x.is_empty()).take(8).map(str::to_string).collect();
    let w = match kinds.is_empty() {
        true => String::new(),
        false => {
            p.insert("kinds".into(), json!(kinds));
            "WHERE kind IN (:kinds)".to_string()
        }
    };
    let page = a.page.min(40);
    let sql = format!("SELECT id, palette, score() AS s FROM img_colors {w} ORDER BY similarity(colors, :c) DESC LIMIT {PER_PAGE} OFFSET {}", page * PER_PAGE);
    let (found, known) = tokio::join!(
        c.q("colors", "the pictures whose colors are closest", &sql, Value::Object(p)),
        c.q("", "how many pictures' colors are known", "SELECT COUNT(*) AS n FROM img_colors", Value::Null),
    );
    let found = found?;
    let ids: Vec<String> = found.iter().map(|r| r.s("id")).collect();
    let rows = if ids.is_empty() { Vec::new() } else { c.q("", "their records", &format!("SELECT {COLS} FROM img_pictures WHERE id IN (:ids)"), json!({"ids": ids})).await? };
    let by_id: HashMap<String, &Row> = rows.iter().map(|r| (r.s("id"), r)).collect();
    let mut out = Vec::new();
    for f in &found {
        if let Some(r) = by_id.get(&f.s("id")) {
            let mut p = c.pics(std::slice::from_ref(*r)).pop().unwrap_or(Value::Null);
            p["colors"] = serde_json::from_str(&f.s("palette")).unwrap_or(Value::Null);
            p["s"] = json!((f.f("s") * 1000.0).round() / 1000.0);
            out.push(p);
        }
    }
    let total = svc.total.load(std::sync::atomic::Ordering::Relaxed);
    done(&c.trace, json!({
        "picks": picks.iter().map(|(c, w)| json!({"hex": colors::hex(*c), "w": w})).collect::<Vec<_>>(),
        "pictures": out, "known": known.ok().and_then(|k| k.first().map(|r| r.i("n"))).unwrap_or(0), "total": total,
        "page": page, "more": found.len() == PER_PAGE,
    }), t0)
}

/// Makes the thumbnails a first visitor sees before anyone asks: the
/// pictures nearest each theme's middle (the map and the front page), then
/// the first `first` pictures, `at_once` at a time.
pub async fn warm(svc: Shared, per_theme: usize, first: usize, at_once: usize) {
    let mut ns: Vec<i64> = Vec::new();
    if let Ok(ts) = svc.db.query("SELECT reps FROM img_themes ORDER BY n DESC", Value::Null).await {
        for round in 0..per_theme {
            for t in &ts {
                if let Some(n) = t.get("reps").and_then(|r| r.as_array()).and_then(|a| a.get(round)).and_then(|x| x.as_i64()) {
                    ns.push(n);
                }
            }
        }
    }
    ns.extend(0..first as i64);
    let mut seen = std::collections::HashSet::new();
    ns.retain(|n| seen.insert(*n));
    let t0 = Instant::now();
    let slots = Arc::new(tokio::sync::Semaphore::new(at_once.max(1)));
    let mut made = 0usize;
    for chunk in ns.chunks(500) {
        let rows = match svc.db.query("SELECT id, url, kind, source, mp FROM img_pictures WHERE n IN (:ns)", json!({"ns": chunk})).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("warming thumbnails: {}", e.message);
                return;
            }
        };
        let mut jobs = Vec::new();
        for r in rows {
            let id = r.s("id");
            if svc.thumbs.have(&id, Size::Small) {
                continue;
            }
            let src = Source { url: r.s("url"), kind: r.s("kind"), source: r.s("source"), mp: r.f("mp") };
            let (svc, slots) = (svc.clone(), slots.clone());
            jobs.push(tokio::spawn(async move {
                let _s = slots.acquire_owned().await;
                svc.thumbs.get(&id, Size::Small, &src).await.is_some()
            }));
        }
        for j in jobs {
            if j.await.unwrap_or(false) {
                made += 1;
            }
        }
    }
    tracing::info!("warmed {made} thumbnails of {} pictures in {:.0} s", ns.len(), t0.elapsed().as_secs_f64());
}

fn stats(v: &[f64]) -> Value {
    if v.is_empty() {
        return Value::Null;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    let at = |q: f64| s[((s.len() as f64 - 1.0) * q).round() as usize];
    json!({"n": s.len(), "median_ms": (at(0.5) * 10.0).round() / 10.0, "p99_ms": (at(0.99) * 10.0).round() / 10.0})
}

/// How it's built: what is stored, and how long each kind of search took
/// lately, measured here.
async fn built(State(svc): State<Shared>) -> Answer {
    let t0 = Instant::now();
    let c = Ctx::new(&svc);
    let (n, colors, links, themes) = tokio::join!(
        c.q("", "how many pictures", "SELECT COUNT(*) AS n FROM img_pictures", Value::Null),
        c.q("", "how many pictures' colors are known", "SELECT COUNT(*) AS n FROM img_colors", Value::Null),
        c.q("", "how many pictures were found on Commons", "SELECT COUNT(*) AS n FROM img_links WHERE found = true", Value::Null),
        c.q("", "how many themes", "SELECT COUNT(*) AS n FROM img_themes", Value::Null),
    );
    let first = |r: Result<Vec<Row>, ApiError>| r.ok().and_then(|x| x.first().map(|r| r.i("n"))).unwrap_or(0);
    let timings: serde_json::Map<String, Value> = svc.timings.lock().unwrap_or_else(|e| e.into_inner()).iter().map(|(k, v)| (k.to_string(), stats(v))).collect();
    let (text, image) = {
        let t = svc.clip.times.lock().unwrap_or_else(|e| e.into_inner());
        (stats(&t.text), stats(&t.image))
    };
    done(&c.trace, json!({
        "pictures": first(n), "colors": first(colors), "commons_pages": first(links), "themes": first(themes),
        "timings": timings, "embed": {"text": text, "image": image}, "model": svc.clip.name(),
        "thumbs_made": svc.thumbs.made.load(std::sync::atomic::Ordering::Relaxed),
        "thumbs_failed": svc.thumbs.failed.load(std::sync::atomic::Ordering::Relaxed),
        "uptime_s": svc.started.elapsed().as_secs(),
    }), t0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subjects_come_from_titles() {
        assert_eq!(subject("A painting of a ship in rough seas"), "a ship in rough seas");
        assert_eq!(subject("Red fox in the snow"), "Red fox in the snow");
    }

    #[test]
    fn filters_become_parameters() {
        let f = Filters { kind: "Painting,Print".into(), licence: "CC0".into(), theme: Some(3), ..Filters::default() };
        let mut p = serde_json::Map::new();
        assert_eq!(f.sql(&mut p), "WHERE kind IN (:f_kind) AND licence IN (:f_licence) AND theme = :f_theme");
        assert_eq!(p["f_kind"], json!(["Painting", "Print"]));
        assert_eq!(Filters::default().sql(&mut p), "");
    }

    #[test]
    fn only_reads_and_photos_are_public() {
        assert!(public_ok(&Method::GET, "/v1/search"));
        assert!(public_ok(&Method::POST, "/v1/photo"));
        assert!(!public_ok(&Method::POST, "/v1/anything"));
    }
}
