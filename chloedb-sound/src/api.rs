//! The app's HTTP API, and its page. ChloeDB's command center reaches it
//! through chloedb-server (`--app sound=URL`), which loads the view from
//! here (`/app.json`, `/ui/view.js`). With `--public` the app also serves
//! the page itself at `/`, read-only, for anyone, within a limit of
//! requests per address: the one thing a visitor sends is a clip of at
//! most ten seconds to listen for (`POST /v1/listen`), which is heard,
//! compared and dropped.
//!
//! Every answer carries `queries`: the SQL it ran, each with its rows and
//! time, which the page shows behind its "SQL" button.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, Query, Request, State};
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine as _;
use serde_json::{json, Value};

use crate::sound::{self, Ctx, Search};
use crate::strata::{RowExt, Strata, Trace};
use crate::vecs::Embedder;

pub const VIEW: &str = include_str!("../ui/view.js");
pub const INDEX: &str = include_str!("../ui/index.html");

/// A waveform in the navigation's outline style.
pub const ICON: &str = r#"<path d="M3 12h1.5M7 9v6M10.5 5v14M14 8v8M17.5 10.5v3M21 12h-.5"/>"#;

/// The longest clip `POST /v1/listen` takes, as a WAV of 48 kHz, 16 bits,
/// one channel, in base64: ten seconds and a little room.
pub const MAX_CLIP_BYTES: usize = 2 << 20;

pub struct Service {
    pub db: Strata,
    pub emb: Arc<Embedder>,
    /// Where the sounds' previews are (`--audio`).
    pub audio: PathBuf,
    pub token: Option<String>,
    /// Anyone may read, without the token (`--public`).
    pub public: bool,
    /// Requests a minute from one address, in public, and clips heard a minute.
    pub per_minute: usize,
    pub listens_per_minute: usize,
    pub seen: Mutex<HashMap<String, (u64, usize, usize)>>,
    /// The preview file of each sound, once looked up.
    pub files: Mutex<HashMap<String, String>>,
    pub started: Instant,
}

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

pub fn router(svc: Shared) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/app.json", get(manifest))
        .route("/ui/view.js", get(view))
        .route("/v1/status", get(status))
        .route("/v1/home", get(home))
        .route("/v1/search", get(search_route))
        .route("/v1/sound/:id", get(sound_route))
        .route("/v1/moments", get(moments_route))
        .route("/v1/listen", post(listen_route))
        .route("/v1/scene", get(scene_route))
        .route("/v1/drift", get(drift_route))
        .route("/v1/map", get(map_route))
        .route("/v1/labels", get(labels_route))
        .route("/v1/audio/:id", get(audio_route))
        .layer(DefaultBodyLimit::max(MAX_CLIP_BYTES * 4 / 3 + 4096))
        .layer(axum::middleware::from_fn_with_state(svc.clone(), guard))
        // Up, for a probe: no token, nothing read.
        .route("/healthz", get(|| async { "ok" }))
        .with_state(svc)
}

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// The token for everything (the command center sends it), or, in public,
/// reads by anyone and the clip to listen for, within limits per address.
/// A page on another site may not call the API: a request whose `Origin`
/// is not this host's is refused.
async fn guard(State(svc): State<Shared>, req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    let headers = req.headers();
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        let host = headers.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");
        let same = origin.split("://").nth(1).is_some_and(|o| o == host);
        if !same {
            return ApiError(StatusCode::FORBIDDEN, "requests from other sites are refused".into()).into_response();
        }
    }
    let authed = match &svc.token {
        None => true,
        Some(t) => headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")).is_some_and(|got| constant_eq(got.as_bytes(), t.as_bytes())),
    };
    if !authed {
        let listen = req.method() == Method::POST && path == "/v1/listen";
        if !(svc.public && (req.method() == Method::GET || listen)) {
            return ApiError(StatusCode::UNAUTHORIZED, "this app needs its token".into()).into_response();
        }
        // In public, a limit per address (the first forwarded one behind a proxy).
        let who = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()).and_then(|v| v.split(',').next()).map(|s| s.trim().to_string())
            .or_else(|| req.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip().to_string())).unwrap_or_default();
        if path.starts_with("/v1/") && !path.starts_with("/v1/audio/") {
            let minute = now() / 60;
            let mut seen = svc.seen.lock().unwrap_or_else(|e| e.into_inner());
            if seen.len() > 50_000 {
                seen.retain(|_, v| v.0 == minute);
            }
            let e = seen.entry(who).or_insert((minute, 0, 0));
            if e.0 != minute {
                *e = (minute, 0, 0);
            }
            e.1 += 1;
            if listen {
                e.2 += 1;
            }
            if e.1 > svc.per_minute || e.2 > svc.listens_per_minute {
                return ApiError(StatusCode::TOO_MANY_REQUESTS, "too many requests from this address: wait a minute".into()).into_response();
            }
        }
    }
    next.run(req).await
}

async fn index(State(svc): State<Shared>) -> Response {
    if !svc.public && svc.token.is_some() {
        return (StatusCode::NOT_FOUND, "this app is shown in ChloeDB's command center (Apps → Sound Explorer)").into_response();
    }
    let body = match std::env::var_os("CHLOEDB_SOUND_UI_DIR") {
        Some(d) => std::fs::read_to_string(std::path::Path::new(&d).join("index.html")).unwrap_or(INDEX.to_string()),
        None => INDEX.to_string(),
    };
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8"), (header::CACHE_CONTROL, "no-cache")], body).into_response()
}

async fn manifest() -> Json<Value> {
    Json(json!({
        "name": "sound",
        "title": "Sound Explorer",
        "description": "Free sounds and music searched by words and by how they sound, inside long recordings too, with the clip you hum, scenes, a map and drifts, each credited and linked to its source",
        "icon_svg": ICON,
        "view": "ui/view.js",
        "version": env!("CARGO_PKG_VERSION"),
        // On the demo train: tested by the Demos workflow, not by CI's
        // required check, and marked as a demo in the command center.
        "maturity": "demo",
    }))
}

pub fn etag(text: &str) -> String {
    let h = text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3));
    format!("\"{h:016x}\"")
}

async fn view(headers: HeaderMap) -> Response {
    let body: std::borrow::Cow<'static, str> = match std::env::var_os("CHLOEDB_SOUND_UI_DIR") {
        Some(d) => std::fs::read_to_string(std::path::Path::new(&d).join("view.js")).map(Into::into).unwrap_or(VIEW.into()),
        None => VIEW.into(),
    };
    let tag = etag(&body);
    let fresh = headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()).is_some_and(|v| v.split(',').any(|t| t.trim().trim_start_matches("W/") == tag));
    let head = [(header::CONTENT_TYPE, "text/javascript; charset=utf-8".to_string()), (header::CACHE_CONTROL, "no-cache".to_string()), (header::ETAG, tag)];
    if fresh {
        return (StatusCode::NOT_MODIFIED, head).into_response();
    }
    (head, body.into_owned()).into_response()
}

fn ctx(svc: &Service) -> Ctx<'_> {
    Ctx { db: &svc.db, emb: &svc.emb, trace: Trace::new() }
}

async fn status(State(svc): State<Shared>) -> Answer {
    let t0 = Instant::now();
    let c = ctx(&svc);
    let s = sound::stats(&c).await?;
    let tables = svc.db.get("/v1/tables").await.unwrap_or(Value::Null);
    let sizes: Vec<Value> = tables["tables"].as_array().into_iter().flatten().filter(|t| t["table"].as_str().is_some_and(|n| n.starts_with("au_"))).cloned().collect();
    let trace = c.trace.clone();
    done(&trace, json!({"stats": s, "tables": sizes, "public": svc.public, "uptime_s": svc.started.elapsed().as_secs()}), t0)
}

async fn home(State(svc): State<Shared>) -> Answer {
    let t0 = Instant::now();
    let c = ctx(&svc);
    let h = sound::home(&c).await?;
    let trace = c.trace.clone();
    done(&trace, h, t0)
}

fn list(s: &str) -> Vec<String> {
    s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).take(20).collect()
}

#[derive(serde::Deserialize, Default)]
pub struct SearchParams {
    #[serde(default)]
    q: String,
    #[serde(default)]
    not: String,
    #[serde(default)]
    like: String,
    #[serde(default)]
    unlike: String,
    #[serde(default)]
    steer: String,
    #[serde(default)]
    source: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    lic: String,
    #[serde(default)]
    min: Option<f64>,
    #[serde(default)]
    max: Option<f64>,
    #[serde(default)]
    by: String,
    #[serde(default)]
    page: usize,
    #[serde(default)]
    per: Option<usize>,
}

async fn search_route(State(svc): State<Shared>, Query(a): Query<SearchParams>) -> Answer {
    let t0 = Instant::now();
    let c = ctx(&svc);
    let s = Search {
        q: a.q.chars().take(300).collect(),
        not: a.not.chars().take(200).collect(),
        like: list(&a.like),
        unlike: list(&a.unlike),
        steer: list(&a.steer),
        source: list(&a.source),
        kind: list(&a.kind),
        lic: list(&a.lic),
        min_s: a.min,
        max_s: a.max,
        by: a.by.clone(),
        page: a.page.min(50),
        per: a.per.unwrap_or(24),
    };
    let mut r = sound::search(&c, &s).await?;
    r["q"] = json!(s.q);
    let trace = c.trace.clone();
    done(&trace, r, t0)
}

async fn sound_route(State(svc): State<Shared>, Path(id): Path<String>) -> Answer {
    let t0 = Instant::now();
    let c = ctx(&svc);
    match sound::sound(&c, &id).await? {
        Some(r) => {
            let trace = c.trace.clone();
            done(&trace, r, t0)
        }
        None => Err(ApiError(StatusCode::NOT_FOUND, format!("no sound {id}"))),
    }
}

#[derive(serde::Deserialize, Default)]
pub struct TextParams {
    #[serde(default)]
    q: String,
    #[serde(default)]
    min: Option<f64>,
}

async fn moments_route(State(svc): State<Shared>, Query(a): Query<TextParams>) -> Answer {
    let t0 = Instant::now();
    let c = ctx(&svc);
    let q: String = a.q.chars().take(300).collect();
    if q.trim().is_empty() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "say what to listen for: ?q=".into()));
    }
    let v = svc.emb.text(&q).await?;
    let found = sound::moments_like(&c, &v, a.min.unwrap_or(30.0), 20).await?;
    let trace = c.trace.clone();
    done(&trace, json!({"q": q, "results": found}), t0)
}

#[derive(serde::Deserialize)]
pub struct ListenBody {
    /// The clip: a WAV (or FLAC, MP3, Ogg) file, in base64.
    audio: String,
}

async fn listen_route(State(svc): State<Shared>, Json(b): Json<ListenBody>) -> Answer {
    let t0 = Instant::now();
    let raw = base64::engine::general_purpose::STANDARD.decode(b.audio.trim()).map_err(|_| ApiError(StatusCode::BAD_REQUEST, "the clip is a sound file in base64".into()))?;
    if raw.len() > MAX_CLIP_BYTES {
        return Err(ApiError(StatusCode::PAYLOAD_TOO_LARGE, "a clip is ten seconds at most".into()));
    }
    let c = ctx(&svc);
    // The clip is never kept: it goes to the server's model and is dropped.
    let r = sound::listen(&c, b.audio.trim()).await?;
    let trace = c.trace.clone();
    done(&trace, r, t0)
}

async fn scene_route(State(svc): State<Shared>, Query(a): Query<TextParams>) -> Answer {
    let t0 = Instant::now();
    let c = ctx(&svc);
    let q: String = a.q.chars().take(300).collect();
    if q.trim().is_empty() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "describe a place: ?q=".into()));
    }
    let r = sound::scene(&c, &q).await?;
    let trace = c.trace.clone();
    done(&trace, r, t0)
}

#[derive(serde::Deserialize, Default)]
pub struct DriftParams {
    from: String,
    to: String,
    #[serde(default)]
    steps: Option<usize>,
    #[serde(default)]
    kind: Option<String>,
}

async fn drift_route(State(svc): State<Shared>, Query(a): Query<DriftParams>) -> Answer {
    let t0 = Instant::now();
    let c = ctx(&svc);
    let r = sound::drift(&c, &a.from, &a.to, a.steps.unwrap_or(8), a.kind.as_deref().filter(|k| !k.is_empty())).await?;
    let trace = c.trace.clone();
    done(&trace, r, t0)
}

async fn map_route(State(svc): State<Shared>) -> Answer {
    let t0 = Instant::now();
    let c = ctx(&svc);
    let r = sound::map(&c, 20_000).await?;
    let trace = c.trace.clone();
    done(&trace, r, t0)
}

async fn labels_route(State(svc): State<Shared>) -> Answer {
    let t0 = Instant::now();
    let l = sound::labels(&svc.emb).await?;
    done(&Trace::new(), json!({"labels": l}), t0)
}

/// A name of a preview file: letters, digits, `.`, `_` and `-` only.
fn safe(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

#[derive(serde::Deserialize, Default)]
pub struct AudioParams {
    /// `b64=1`: the file as JSON, `{mime, data}`, for the command center,
    /// whose requests carry the token in a header an audio element cannot.
    #[serde(default)]
    b64: Option<u8>,
}

/// A sound's preview, from `--audio`, with ranges for seeking.
async fn audio_route(State(svc): State<Shared>, Path(id): Path<String>, Query(a): Query<AudioParams>, headers: HeaderMap) -> Result<Response, ApiError> {
    let cached = svc.files.lock().unwrap_or_else(|e| e.into_inner()).get(&id).cloned();
    let file = match cached {
        Some(f) => f,
        None => {
            let rows = svc.db.query("SELECT audio FROM au_sounds WHERE id = :id", json!({"id": id})).await?;
            let f = rows.first().map(|r| r.s("audio")).unwrap_or_default();
            if !safe(&f) {
                return Err(ApiError(StatusCode::NOT_FOUND, format!("no sound {id}")));
            }
            let mut m = svc.files.lock().unwrap_or_else(|e| e.into_inner());
            if m.len() > 200_000 {
                m.clear();
            }
            m.insert(id.clone(), f.clone());
            f
        }
    };
    let bytes = tokio::fs::read(svc.audio.join(&file)).await.map_err(|_| ApiError(StatusCode::NOT_FOUND, format!("no file for {id}")))?;
    let mime = if file.ends_with(".mp3") { "audio/mpeg" } else if file.ends_with(".ogg") { "audio/ogg" } else { "application/octet-stream" };
    if a.b64 == Some(1) {
        return Ok(Json(json!({"mime": mime, "data": base64::engine::general_purpose::STANDARD.encode(&bytes)})).into_response());
    }
    let n = bytes.len();
    let range = headers.get(header::RANGE).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("bytes=")).and_then(|v| {
        let (a, b) = v.split_once('-')?;
        let start: usize = if a.is_empty() { n.saturating_sub(b.parse().ok()?) } else { a.parse().ok()? };
        let end: usize = if a.is_empty() || b.is_empty() { n.saturating_sub(1) } else { b.parse::<usize>().ok()?.min(n.saturating_sub(1)) };
        (start <= end && start < n).then_some((start, end))
    });
    let common = [(header::CONTENT_TYPE, mime.to_string()), (header::CACHE_CONTROL, "public, max-age=86400".to_string()), (header::ACCEPT_RANGES, "bytes".to_string())];
    Ok(match range {
        Some((s, e)) => (StatusCode::PARTIAL_CONTENT, common, [(header::CONTENT_RANGE, format!("bytes {s}-{e}/{n}"))], bytes[s..=e].to_vec()).into_response(),
        None => (common, bytes).into_response(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_cannot_leave_the_folder() {
        assert!(safe("fsd_12345.mp3"));
        assert!(!safe("../etc/passwd"));
        assert!(!safe(".hidden"));
        assert!(!safe("a/b.mp3"));
        assert!(!safe(""));
    }
}
