//! The app's HTTP API, and its view. ChloeDB's command center reaches it
//! through chloedb-server (`--app gh=URL`), which loads the view from here
//! (`/app.json`, `/ui/view.js`) and sends the app's token with each call.
//! Without `--public` every route but `/healthz` needs the token. With it
//! the app also serves its page at `/` to anyone, and the page's reads
//! (every route is a GET) without the token: so many a minute from one
//! address, and so many at once, the rest waiting for a turn. Nothing runs
//! SQL a reader writes: each page asks its own statements.
//!
//! Every answer carries `queries`: the SQL it ran, each with its rows and
//! time, which the page shows behind its "SQL" button.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, Path, Query, Request, State};
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::pages::{self, Ctx, DAY};
use crate::strata::{RowExt, Strata, Trace};

pub const VIEW: &str = include_str!("../ui/view.js");
pub const INDEX: &str = include_str!("../ui/index.html");

/// A branch with a star: repos, and what they grow into.
pub const ICON: &str = r#"<circle cx="6" cy="5" r="2"/><circle cx="6" cy="19" r="2"/><path d="M6 7v10"/><path d="M6 13c0-4 4-4 7-6"/><path d="M17.5 2.5l1.1 2.3 2.4.3-1.8 1.7.5 2.4-2.2-1.2-2.2 1.2.5-2.4-1.8-1.7 2.4-.3z"/>"#;

/// Hacker News, through its app on ChloeDB: when and where a repo was
/// discussed, to explain a jump in its stars.
pub struct Hn {
    base: String,
    http: reqwest::Client,
    cache: Mutex<HashMap<String, (Instant, Value)>>,
}

impl Hn {
    pub fn new(base: &str) -> Hn {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(6))
            .user_agent("chloedb-gh")
            .build()
            .expect("an HTTP client");
        Hn {
            base: base.trim_end_matches('/').to_string(),
            http,
            cache: Mutex::new(HashMap::new()),
        }
    }

    async fn link(&self, name: &str) -> Value {
        let key = name.to_lowercase();
        if let Some((t, v)) = self
            .cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&key)
        {
            if t.elapsed() < Duration::from_secs(3600) {
                return v.clone();
            }
        }
        let url = format!("{}/v1/link", self.base);
        let v = match self
            .http
            .get(&url)
            .query(&[("u", format!("github.com/{name}"))])
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => r.json::<Value>().await.unwrap_or(Value::Null),
            _ => Value::Null,
        };
        let mut c = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if c.len() > 20_000 {
            c.clear();
        }
        c.insert(key, (Instant::now(), v.clone()));
        v
    }

    /// For each repo name, its most upvoted submission to Hacker News
    /// between `from` and `to` (seconds), if there was one.
    pub async fn mentions(&self, names: &[String], from: i64, to: i64) -> HashMap<String, Value> {
        let found = futures_util::future::join_all(
            names
                .iter()
                .map(|n| async move { (n.clone(), self.link(n).await) }),
        )
        .await;
        let mut out = HashMap::new();
        for (name, v) in found {
            let best = v["submissions"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|s| {
                    let t = s["created"]
                        .as_str()
                        .and_then(|c| chrono::DateTime::parse_from_rfc3339(c).ok())
                        .map_or(0, |d| d.timestamp());
                    t >= from && t <= to
                })
                .max_by_key(|s| s["points"].as_i64().unwrap_or(0));
            if let Some(s) = best {
                if s["points"].as_i64().unwrap_or(0) >= 10 {
                    out.insert(name, json!({"title": s["title"], "points": s["points"], "comments": s["comments"], "created": s["created"], "url": format!("https://news.ycombinator.com/item?id={}", s["id"])}));
                }
            }
        }
        out
    }
}

pub struct Service {
    pub db: Strata,
    pub token: Option<String>,
    /// Whether readers without the token may read (`--public`).
    pub public: bool,
    /// Requests a minute one address may make without the token.
    pub per_minute: usize,
    /// Each address's minute and its requests in it.
    pub seen: Mutex<HashMap<String, (u64, usize)>>,
    /// Turns for the requests without the token: so many are answered at
    /// once, so that a crowd waits rather than outruns the shards' memory.
    pub turns: tokio::sync::Semaphore,
    pub hn: Option<Hn>,
    pub started: Instant,
    /// The newest day in the archive, and the most starred repos, kept a
    /// few minutes.
    pub latest: Mutex<Option<(Instant, i64, Vec<i64>)>>,
    /// The topics with the most stars in the last 12 months, for Rising's
    /// picker, kept 12 hours.
    pub topics: tokio::sync::Mutex<Option<(Instant, Vec<Value>)>>,
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
        ApiError(
            status,
            if e.status == 0 || e.status >= 500 {
                format!("ChloeDB: {}", e.message)
            } else {
                e.message
            },
        )
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
        .route("/v1/search", get(search))
        .route("/v1/repo/*name", get(repo))
        .route("/v1/person/:login", get(person))
        .route("/v1/rising", get(rising))
        .route("/v1/trends", get(trends))
        .route("/v1/ask", get(ask))
        .route("/v1/foryou", get(for_you))
        .route("/v1/weekly", get(weekly))
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
        let host = headers
            .get(header::HOST)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if !origin.split("://").nth(1).is_some_and(|o| o == host) {
            return ApiError(
                StatusCode::FORBIDDEN,
                "requests from other sites are refused".into(),
            )
            .into_response();
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
        return ApiError(
            StatusCode::UNAUTHORIZED,
            "this app needs its token: open it in ChloeDB's command center (Apps → GitHub)".into(),
        )
        .into_response();
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
        return ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "too many requests from this address: wait a minute".into(),
        )
        .into_response();
    }
    let Ok(Ok(_turn)) = tokio::time::timeout(Duration::from_secs(30), svc.turns.acquire()).await else {
        return ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "many people are reading at once: try again in a moment".into(),
        )
        .into_response();
    };
    next.run(req).await
}

fn now_s() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn ui_file(name: &str, compiled: &'static str) -> std::borrow::Cow<'static, str> {
    match std::env::var_os("CHLOEDB_GH_UI_DIR") {
        Some(d) => std::fs::read_to_string(std::path::Path::new(&d).join(name))
            .map(Into::into)
            .unwrap_or(compiled.into()),
        None => compiled.into(),
    }
}

/// The page on its own: in public, or on one's own machine without a token.
async fn index(State(svc): State<Shared>) -> Response {
    if svc.token.is_some() && !svc.public {
        return (
            StatusCode::NOT_FOUND,
            "this app is shown in ChloeDB's command center (Apps → GitHub)",
        )
            .into_response();
    }
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        ui_file("index.html", INDEX).into_owned(),
    )
        .into_response()
}

async fn manifest() -> Json<Value> {
    Json(json!({
        "name": "gh",
        "title": "GitHub",
        "description": "Every public GitHub event since 2011: repos found by what they do, why they trend, what people use instead, who keeps them going, and what to star next",
        "icon_svg": ICON,
        "view": "ui/view.js",
        "version": env!("CARGO_PKG_VERSION"),
        "maturity": "demo",
    }))
}

pub fn etag(text: &str) -> String {
    let h = text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3)
    });
    format!("\"{h:016x}\"")
}

async fn view(headers: HeaderMap) -> Response {
    let body = ui_file("view.js", VIEW);
    let tag = etag(&body);
    let fresh = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(',')
                .any(|t| t.trim().trim_start_matches("W/") == tag)
        });
    let head = [
        (
            header::CONTENT_TYPE,
            "text/javascript; charset=utf-8".to_string(),
        ),
        (header::CACHE_CONTROL, "no-cache".to_string()),
        (header::ETAG, tag),
    ];
    if fresh {
        return (StatusCode::NOT_MODIFIED, head).into_response();
    }
    (head, body.into_owned()).into_response()
}

macro_rules! ctx {
    ($svc:expr, $trace:ident, $c:ident) => {
        let $trace = Trace::new();
        let $c = Ctx {
            db: &$svc.db,
            trace: &$trace,
        };
    };
}

impl Service {
    /// The 80 topics with the most stars in the last 12 months (worked
    /// out once and kept 12 hours: the sum reads every topic's months).
    pub async fn topics(&self, c: &crate::pages::Ctx<'_>) -> Vec<Value> {
        let mut kept = self.topics.lock().await;
        if let Some((at, list)) = kept.as_ref() {
            if at.elapsed() < Duration::from_secs(12 * 3600) {
                return list.clone();
            }
        }
        let since = crate::pages::months_back(now_s() as i64, 12);
        let rows = c.soft_within("the topics with the most stars, last 12 months", "SELECT topic, SUM(stars) AS s FROM gh_topic_months WHERE month >= :m GROUP BY topic ORDER BY s DESC LIMIT 80", json!({"m": since}), 60_000).await;
        let list: Vec<Value> = rows.iter().map(|r| json!({"topic": r.s("topic"), "stars": r.i("s")})).collect();
        if !list.is_empty() {
            *kept = Some((Instant::now(), list.clone()));
        }
        list
    }

    /// Counts a request from `who` in minute `minute`: false past
    /// `per_minute` of them.
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

    /// The newest day with stars, and the 300 most starred repos.
    async fn latest(&self) -> (i64, Vec<i64>) {
        if let Some((t, d, big)) = self
            .latest
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            if t.elapsed() < Duration::from_secs(600) {
                return (*d, big.clone());
            }
        }
        let (d, big) = tokio::join!(
            self.db
                .query("SELECT MAX(day) AS d FROM gh_stars_daily", Value::Null),
            self.db.query(
                "SELECT repo FROM gh_repos ORDER BY stars DESC LIMIT 300",
                Value::Null
            )
        );
        let d = d
            .ok()
            .and_then(|r| r.first().map(|r| r.i("d")))
            .filter(|d| *d > 0)
            .unwrap_or_else(|| {
                let now = chrono::Utc::now().timestamp();
                now - now % DAY - DAY
            });
        let big = big.map(|r| pages::ids(&r, "repo")).unwrap_or_default();
        *self.latest.lock().unwrap_or_else(|e| e.into_inner()) =
            Some((Instant::now(), d, big.clone()));
        (d, big)
    }
}

/// A day as YYYY-MM-DD, or the newest one.
fn day_of(s: &Option<String>, latest: i64) -> i64 {
    s.as_deref()
        .and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map_or(latest, |d| d.and_utc().timestamp())
        .min(latest)
}

async fn status(State(svc): State<Shared>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let (latest, _) = svc.latest().await;
    let (events, repos, people) = tokio::join!(
        c.soft(
            "how many events",
            "SELECT COUNT(*) AS n FROM gh_events",
            Value::Null
        ),
        c.soft(
            "how many repos",
            "SELECT COUNT(*) AS n FROM gh_repos",
            Value::Null
        ),
        c.soft(
            "how many people",
            "SELECT COUNT(*) AS n FROM gh_actors",
            Value::Null
        ),
    );
    done(
        &trace,
        json!({
            "latest": latest, "events": events.first().map(|r| r.i("n")), "repos": repos.first().map(|r| r.i("n")), "people": people.first().map(|r| r.i("n")),
            "hn": svc.hn.is_some(), "public": svc.public, "examples": crate::ask::EXAMPLES, "uptime_s": svc.started.elapsed().as_secs(),
        }),
        t0,
    )
}

async fn home(State(svc): State<Shared>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let (latest, _) = svc.latest().await;
    let v = pages::home(&c, latest, svc.hn.as_ref()).await?;
    done(&trace, v, t0)
}

#[derive(serde::Deserialize, Default)]
struct SearchParams {
    #[serde(default)]
    q: String,
    #[serde(default)]
    lang: String,
    #[serde(default)]
    sort: String,
    #[serde(default)]
    page: usize,
}

async fn search(State(svc): State<Shared>, Query(a): Query<SearchParams>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    if a.q.trim().is_empty() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "say what the repo should do".into(),
        ));
    }
    let (latest, _) = svc.latest().await;
    let s = pages::Search {
        q: a.q.clone(),
        language: a.lang.clone(),
        sort: a.sort.clone(),
        page: a.page.min(30),
    };
    let v = pages::search(&c, &s, latest).await?;
    done(&trace, v, t0)
}

async fn repo(State(svc): State<Shared>, Path(name): Path<String>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let (latest, _) = svc.latest().await;
    let Some(id) = pages::find_repo(&c, &name).await else {
        return done(&trace, json!({"found": false, "name": name}), t0);
    };
    let mut v = pages::repo(&c, id, latest).await?;
    // The stories on Hacker News over its life, for its spikes.
    if let (Some(hn), Some(n)) = (&svc.hn, v["repo"]["name"].as_str().map(str::to_string)) {
        let link = hn.link(&n).await;
        let subs: Vec<Value> = link["submissions"].as_array().into_iter().flatten().filter(|s| s["points"].as_i64().unwrap_or(0) >= 5).map(|s| json!({"title": s["title"], "points": s["points"], "comments": s["comments"], "created": s["created"], "url": format!("https://news.ycombinator.com/item?id={}", s["id"])})).collect();
        v["hn"] = json!({"submissions": subs, "totals": link["totals"]});
    }
    done(&trace, v, t0)
}

async fn person(State(svc): State<Shared>, Path(login): Path<String>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let v = pages::person(&c, &login).await?;
    done(&trace, v, t0)
}

#[derive(serde::Deserialize, Default)]
struct RisingParams {
    at: Option<String>,
    days: Option<i64>,
    #[serde(default)]
    topic: String,
}

async fn rising(State(svc): State<Shared>, Query(a): Query<RisingParams>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let (latest, _) = svc.latest().await;
    let at = day_of(&a.at, latest);
    let topic = a.topic.trim().to_lowercase();
    let topic = (!topic.is_empty() && topic.len() <= 60).then_some(topic);
    let mut v = pages::rising_about(&c, at, a.days.unwrap_or(7).clamp(1, 90), svc.hn.as_ref(), topic.as_deref()).await?;
    v["topic"] = json!(topic);
    v["topics"] = json!(svc.topics(&c).await);
    done(&trace, v, t0)
}

#[derive(serde::Deserialize, Default)]
struct TrendParams {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    names: String,
}

async fn trends(State(svc): State<Shared>, Query(a): Query<TrendParams>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let (latest, _) = svc.latest().await;
    let names: Vec<String> = a
        .names
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .take(10)
        .collect();
    let v = pages::trends(
        &c,
        if a.kind == "topic" {
            "topic"
        } else {
            "language"
        },
        &names,
        latest,
    )
    .await?;
    done(&trace, v, t0)
}

#[derive(serde::Deserialize, Default)]
struct AskParams {
    #[serde(default)]
    q: String,
}

async fn ask(State(svc): State<Shared>, Query(a): Query<AskParams>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let (latest, _) = svc.latest().await;
    let v = crate::ask::ask(&c, a.q.trim(), latest).await?;
    done(&trace, v, t0)
}

#[derive(serde::Deserialize, Default)]
struct ForYouParams {
    #[serde(default)]
    login: String,
}

async fn for_you(State(svc): State<Shared>, Query(a): Query<ForYouParams>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    if a.login.trim().is_empty() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "give a GitHub login".into(),
        ));
    }
    let v = pages::for_you(&c, &a.login).await?;
    done(&trace, v, t0)
}

async fn weekly(State(svc): State<Shared>, Query(a): Query<RisingParams>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let (latest, big) = svc.latest().await;
    let v = pages::weekly(&c, day_of(&a.at, latest), svc.hn.as_ref(), &big).await?;
    done(&trace, v, t0)
}

/// Each shard of a coordinator's `/v1/cluster`: its number, state, slots
/// and its first copy's rows and segments.
fn shards_of(cl: &Value) -> Vec<Value> {
    cl["shards"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|s| {
            let r = &s["replicas"][0];
            json!({"shard": s["shard"], "state": s["state"], "slots": s["slots_held"], "rows": r["docs"], "segments": r["segments"]})
        })
        .collect()
}

async fn scale(State(svc): State<Shared>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let mut v = pages::scale(&c).await;
    // What each shard holds, from the cluster: its rows, segments and
    // state, not its address or settings.
    if let Ok(cl) = svc.db.get("/v1/cluster").await {
        v["shards"] = json!(shards_of(&cl));
    }
    done(&trace, v, t0)
}

#[cfg(test)]
mod public_tests {
    use super::*;

    fn svc(per_minute: usize) -> Service {
        Service {
            db: Strata::new("http://127.0.0.1:1", None),
            token: Some("t".into()),
            public: true,
            per_minute,
            seen: Mutex::new(HashMap::new()),
            turns: tokio::sync::Semaphore::new(1),
            hn: None,
            started: Instant::now(),
            latest: Mutex::new(None),
            topics: tokio::sync::Mutex::new(None),
        }
    }

    #[test]
    fn an_address_gets_so_many_a_minute() {
        let s = svc(3);
        assert!((0..3).all(|_| s.admit("1.2.3.4", 10)));
        assert!(!s.admit("1.2.3.4", 10));
        assert!(s.admit("5.6.7.8", 10), "another address has its own count");
        assert!(s.admit("1.2.3.4", 11), "a new minute starts again");
    }

    #[test]
    fn the_cluster_summary_leaves_out_addresses() {
        let cl = json!({"shards": [{"shard": 0, "state": "ok", "slots_held": 1024, "url": "http://gh-s0:7701",
            "replicas": [{"docs": 5, "segments": 2, "url": "http://gh-s0:7701", "backup_dir": "/data/backup"}]}]});
        let s = shards_of(&cl);
        assert_eq!(s, vec![json!({"shard": 0, "state": "ok", "slots": 1024, "rows": 5, "segments": 2})]);
        assert!(!serde_json::to_string(&s).unwrap().contains("gh-s0"));
    }
}
