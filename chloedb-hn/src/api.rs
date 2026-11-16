//! The app's HTTP API, and its page. ChloeDB's command center reaches it
//! through chloedb-server (`--app hn=URL`), which loads the view from here
//! (`/app.json`, `/ui/view.js`). With `--public` the app also serves the
//! page itself at `/`, read-only, for anyone: the searches, threads,
//! people and trends, with a limit of requests per address.
//!
//! Every answer carries `queries`: the SQL it ran, each with its rows and
//! time, which the page shows behind its "SQL" button.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::extract::{ConnectInfo, Path, Query, Request, State};
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::ingest::Feed;
use crate::search::{self, Ctx, Request as SearchReq};
use crate::strata::{RowExt, Strata, Trace};
use crate::{ask, people, query, schema, thread, trends};

pub const VIEW: &str = include_str!("../ui/view.js");
pub const INDEX: &str = include_str!("../ui/index.html");

/// A speech bubble with a lens in it, in the navigation's outline style.
pub const ICON: &str = r#"<path d="M4 5.5A2.5 2.5 0 0 1 6.5 3h11A2.5 2.5 0 0 1 20 5.5v8a2.5 2.5 0 0 1-2.5 2.5H11l-4.5 4v-4A2.5 2.5 0 0 1 4 13.5z"/><circle cx="11.5" cy="9.3" r="2.8"/><path d="M13.6 11.4l2 2"/>"#;

pub struct Service {
    pub db: Strata,
    pub feed: Option<Arc<Feed>>,
    pub token: Option<String>,
    /// Anyone may read, without the token (`--public`).
    pub public: bool,
    /// The model behind "What HN thinks", called through the server's llm().
    pub model: String,
    /// The model that writes the weekly digest's entries.
    pub digest_model: String,
    /// Questions answered a day (new answers; kept ones are free).
    pub ask_per_day: usize,
    pub asks: Mutex<(i64, usize)>,
    /// New digest entries a day, and today's count (the day, the count).
    pub digest_per_day: usize,
    pub digests: Mutex<(i64, usize)>,
    /// Requests a minute from one address, in public.
    pub per_minute: usize,
    pub seen: Mutex<HashMap<String, (i64, usize)>>,
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
        .route("/v1/search", get(search_route))
        .route("/v1/facets", get(facets_route))
        .route("/v1/suggest", get(suggest))
        .route("/v1/item/:id", get(item))
        .route("/v1/item/:id/around", get(around))
        .route("/v1/user/:id", get(user))
        .route("/v1/experts", get(experts))
        .route("/v1/trends", get(trends_route))
        .route("/v1/ask", get(ask_route))
        .route("/v1/ask/comments", get(ask_comments_route))
        .route("/v1/front", get(front))
        .route("/v1/trending", get(trending_route))
        .route("/v1/spikes", get(spikes_route))
        .route("/v1/link", get(link_route))
        .route("/v1/digest", get(digest_route))
        .route("/v1/digest/write", get(digest_write_route))
        .route("/v1/minds", get(minds_route))
        .route("/v1/canon", get(canon_route))
        .route("/v1/canon/work", get(canon_work_route))
        .route("/v1/minds/arc", get(arc_route))
        .layer(axum::middleware::from_fn_with_state(svc.clone(), guard))
        // Up, for a probe: no token, nothing read.
        .route("/healthz", get(|| async { "ok" }))
        .with_state(svc)
}

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The token for everything (the command center sends it), or, in public,
/// reads by anyone within a limit per address. A page on another site may
/// not call the API: a request whose `Origin` is not this host's is refused.
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
        if !(svc.public && req.method() == Method::GET) {
            return ApiError(StatusCode::UNAUTHORIZED, "this app needs its token".into()).into_response();
        }
        // In public, a limit per address (the first forwarded one behind a proxy).
        let who = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()).and_then(|v| v.split(',').next()).map(|s| s.trim().to_string())
            .or_else(|| req.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip().to_string())).unwrap_or_default();
        if path.starts_with("/v1/") {
            let minute = crate::ingest::now() / 60;
            let mut seen = svc.seen.lock().unwrap_or_else(|e| e.into_inner());
            if seen.len() > 50_000 {
                seen.retain(|_, v| v.0 == minute);
            }
            let e = seen.entry(who).or_insert((minute, 0));
            if e.0 != minute {
                *e = (minute, 0);
            }
            e.1 += 1;
            if e.1 > svc.per_minute {
                return ApiError(StatusCode::TOO_MANY_REQUESTS, "too many requests from this address: wait a minute".into()).into_response();
            }
        }
    }
    next.run(req).await
}

async fn index(State(svc): State<Shared>) -> Response {
    if !svc.public && svc.token.is_some() {
        return (StatusCode::NOT_FOUND, "this app is shown in ChloeDB's command center (Apps → Hacker News)").into_response();
    }
    let body = match std::env::var_os("CHLOEDB_HN_UI_DIR") {
        Some(d) => std::fs::read_to_string(std::path::Path::new(&d).join("index.html")).unwrap_or(INDEX.to_string()),
        None => INDEX.to_string(),
    };
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8"), (header::CACHE_CONTROL, "no-cache")], body).into_response()
}

async fn manifest() -> Json<Value> {
    Json(json!({
        "name": "hn",
        "title": "Hacker News",
        "description": "Every Hacker News story and comment, searched by words and by meaning, with threads, trends, experts, the reply graph and cited answers",
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
    let body: std::borrow::Cow<'static, str> = match std::env::var_os("CHLOEDB_HN_UI_DIR") {
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

async fn status(State(svc): State<Shared>) -> Answer {
    let t0 = Instant::now();
    let trace = Trace::new();
    let c = Ctx { db: &svc.db, trace: &trace };
    let (counts, span) = tokio::join!(
        schema::counts(&svc.db),
        c.soft("the archive's span", "SELECT MIN(created) AS first, MAX(created) AS last FROM hn_stories", Value::Null),
    );
    let cspan = c.soft("the comments' span", "SELECT MIN(created) AS first, MAX(created) AS last FROM hn_comments", Value::Null).await;
    done(&trace, json!({
        "tables": counts.unwrap_or(Value::Null),
        "stories": span.first(), "comments": cspan.first(),
        "feed": svc.feed.as_ref().map(|f| f.status()),
        "public": svc.public, "model": svc.model, "uptime_s": svc.started.elapsed().as_secs(),
    }), t0)
}

#[derive(serde::Deserialize, Default)]
pub struct SearchParams {
    #[serde(default)]
    q: String,
    #[serde(default)]
    only: String,
    #[serde(default)]
    sort: String,
    #[serde(default)]
    mode: String,
    #[serde(default)]
    page: usize,
    #[serde(default)]
    facets: Option<u8>,
    /// `comments=0`: the stories only (the page asks for the comments on
    /// their own, so the stories show first).
    #[serde(default)]
    comments: Option<u8>,
    /// Results a page, when not the default.
    #[serde(default)]
    per: Option<usize>,
}

/// The query understood, with a word that may be a person checked against
/// the people table.
async fn understand(c: &Ctx<'_>, q: &str) -> query::Parsed {
    let mut p = query::parse(q, crate::ingest::now());
    if let Some(who) = p.author_maybe.clone() {
        let found = c.soft("is that a person?", "SELECT id FROM hn_users WHERE id = :u", json!({"u": who})).await;
        // HN names are case-sensitive, but people type them in lowercase.
        let found = if found.is_empty() {
            c.soft("is that a person (any case)?", "SELECT id FROM hn_users WHERE id = :u OR id = :l", json!({"u": capital(&who), "l": who.to_lowercase()})).await
        } else {
            found
        };
        if let Some(r) = found.first() {
            p.author_maybe = Some(r.s("id"));
        }
        p.confirm_author(!found.is_empty());
    }
    p
}

/// The facets of a search on their own: the page asks for them once the
/// results are shown.
async fn facets_route(State(svc): State<Shared>, Query(a): Query<SearchParams>) -> Answer {
    let t0 = Instant::now();
    let trace = Trace::new();
    let c = Ctx { db: &svc.db, trace: &trace };
    let p = understand(&c, &a.q).await;
    let only = if !a.only.is_empty() { a.only.clone() } else { p.only.clone().unwrap_or_else(|| "all".into()) };
    let f = search::facets(&c, &p, only == "comment").await;
    done(&trace, json!({"q": a.q, "facets": f}), t0)
}

/// One search: the query understood, the stories, the comments, the
/// facets, and a word the titles hold for each typo.
async fn search_route(State(svc): State<Shared>, Query(a): Query<SearchParams>) -> Answer {
    let t0 = Instant::now();
    let trace = Trace::new();
    let c = Ctx { db: &svc.db, trace: &trace };
    if a.mode == "instant" {
        // A keystroke's search: the stories alone, one statement, no
        // lookups (a word that may be a person stays a word).
        let mut p = query::parse(&a.q, crate::ingest::now());
        p.confirm_author(false);
        let r = SearchReq { q: a.q.clone(), only: "story".into(), sort: "best".into(), mode: "instant".into(), page: a.page, per_page: 20, facets: false };
        let per = a.per.filter(|n| (1..=50).contains(n)).unwrap_or(20);
        let (stories, total) = search::stories(&c, &p, &r, per, a.page * per).await?;
        return done(&trace, json!({
            "q": a.q, "understood": p, "only": "story", "sort": "best", "mode": "instant",
            "stories": stories, "comments": [], "total_stories": total, "total_comments": 0, "page": a.page, "per_page": per,
        }), t0);
    }
    let p = understand(&c, &a.q).await;
    let only = if !a.only.is_empty() { a.only.clone() } else { p.only.clone().unwrap_or_else(|| "all".into()) };
    let sort = if !a.sort.is_empty() { a.sort.clone() } else { p.sort.clone().unwrap_or_else(|| "best".into()) };
    let r = SearchReq { q: a.q.clone(), only: only.clone(), sort, mode: a.mode.clone(), page: a.page, per_page: 20, facets: a.facets != Some(0) };
    let per = a.per.filter(|n| (1..=50).contains(n)).unwrap_or(if only == "all" { 15 } else { 25 });
    let off = a.page * per;
    let want_s = only != "comment";
    let want_c = only != "story" && p.kinds.is_empty() && p.site.is_none() && a.comments != Some(0);
    let words = query::plain_words(&p);
    let (s, cm, f, dym) = tokio::join!(
        async { if want_s { search::stories(&c, &p, &r, per, off).await.map(Some) } else { Ok(None) } },
        async { if want_c { search::comments(&c, &p, &r, if only == "all" { 8 } else { per }, if only == "all" { a.page * 8 } else { off }).await.map(Some) } else { Ok(None) } },
        async { if r.facets && a.page == 0 { Some(search::facets(&c, &p, only == "comment").await) } else { None } },
        async { if a.page == 0 && !words.is_empty() { search::did_you_mean(&c, &words).await } else { None } },
    );
    let (mut stories, total_s) = s?.unwrap_or_default();
    let (mut comments, total_c) = cm?.unwrap_or_default();
    let ans = if a.page == 0 { crate::answer::answer(&c, &p, &stories, &comments, total_s, total_c, f.as_ref()).await } else { None };
    search::mark(&c, &p, &mut stories, &mut comments).await;
    done(&trace, json!({
        "q": a.q, "understood": p, "only": only, "sort": r.sort, "mode": if r.mode.is_empty() { "hybrid" } else { r.mode.as_str() },
        "answer": ans,
        "stories": stories, "comments": comments, "total_stories": total_s, "total_comments": total_c,
        "facets": f, "did_you_mean": dym, "page": a.page, "per_page": per,
    }), t0)
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

#[derive(serde::Deserialize, Default)]
struct Q {
    #[serde(default)]
    q: String,
    #[serde(default)]
    by: String,
    #[serde(default, rename = "in")]
    in_: String,
    #[serde(default)]
    fresh: Option<u8>,
}

/// Completions as a search is typed: whole questions the grammar reads
/// (crate::nl), phrases of titles, people, sites and topics by a prefix of
/// `hn_complete`'s key (crate::complete), and stories whose titles hold
/// the words.
async fn suggest(State(svc): State<Shared>, Query(a): Query<Q>) -> Answer {
    let t0 = Instant::now();
    let trace = Trace::new();
    let c = Ctx { db: &svc.db, trace: &trace };
    let raw = a.q.trim_start().to_string();
    let typed: String = raw.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    if typed.chars().count() < 2 {
        return done(&trace, json!({"questions": [], "phrases": [], "people": [], "sites": [], "topics": [], "stories": []}), t0);
    }
    let partial = !raw.ends_with(' ');
    let words: Vec<&str> = typed.split(' ').collect();
    let last = words.last().copied().unwrap_or("").to_string();
    let head = if words.len() > 1 { words[..words.len() - 1].join(" ") } else { String::new() };
    let frame = crate::nl::read(&typed);
    // A question being typed ("which post ha", "who posted the most about ru"):
    // its words are not a phrase to complete, but its topic is.
    let lead = crate::nl::starts_question(&words, partial);
    // The topic: the words after "about" ("which post about ru"), else the
    // last word where a question wants its topic ("when was bitc"; only the
    // questions that then fit are offered).
    let slot = if lead && partial {
        words.iter().rposition(|w| matches!(*w, "about" | "on" | "mention" | "mentions" | "mentioning" | "regarding")).filter(|k| k + 1 < words.len()).map(|k| (words[..=k].join(" "), words[k + 1..].join(" "), true))
            .or_else(|| (words.len() >= 3 && last.len() >= 3).then(|| (head.clone(), last.clone(), false)))
    } else {
        None
    };
    // The topic words, when the box holds a question: those are completed.
    let p = query::parse(&raw, crate::ingest::now());
    let m = query::match_text(&p);
    let prefix = if m.is_empty() { String::new() } else if partial { format!("{m}*") } else { m.clone() };
    let pre = |k: &str, s: &str| format!("{k}:{s}");
    let full = if partial { typed.clone() } else { format!("{typed} ") };
    let (phr, tail, people, sites, stories) = tokio::join!(
        async {
            match (&slot, lead) {
                (Some((_, t, _)), _) => c.soft("the question's topic as typed", "SELECT text, n FROM hn_complete WHERE PREFIX(id, :p) ORDER BY weight DESC LIMIT 3", json!({"p": pre("p", t)})).await,
                (None, true) => Vec::new(),
                (None, false) => c.soft("phrases as typed", "SELECT text, n FROM hn_complete WHERE PREFIX(id, :p) ORDER BY weight DESC LIMIT 8", json!({"p": pre("p", &full)})).await,
            }
        },
        async {
            // The last word (or two) completed after the words before: a
            // finished word's phrases ("learn" + "to code") before its
            // other endings ("learning").
            if words.len() > 1 && frame.is_none() && !lead {
                let two = words[words.len().saturating_sub(2)..].join(" ");
                let mut r = c.soft("the last words as typed", "SELECT text, n FROM hn_complete WHERE PREFIX(id, :p) ORDER BY weight DESC LIMIT 5", json!({"p": pre("p", &two)})).await;
                if r.len() < 3 && partial {
                    if last.len() >= 3 {
                        r.extend(c.soft("the last word's phrases", "SELECT text, n FROM hn_complete WHERE PREFIX(id, :p) ORDER BY weight DESC LIMIT 4", json!({"p": pre("p", &format!("{last} "))})).await);
                    }
                    r.extend(c.soft("the last word as typed", "SELECT text, n FROM hn_complete WHERE PREFIX(id, :p) ORDER BY weight DESC LIMIT 5", json!({"p": pre("p", &last)})).await);
                }
                r
            } else {
                Vec::new()
            }
        },
        async { if last.len() >= 2 && frame.is_none() && !lead { c.soft("people as typed", "SELECT text, n FROM hn_complete WHERE PREFIX(id, :p) ORDER BY weight DESC LIMIT 3", json!({"p": pre("u", &last)})).await } else { Vec::new() } },
        async { if last.len() >= 3 && frame.is_none() && !lead { c.soft("sites as typed", "SELECT text, n FROM hn_complete WHERE PREFIX(id, :p) ORDER BY weight DESC LIMIT 2", json!({"p": pre("s", &last)})).await } else { Vec::new() } },
        async { if prefix.is_empty() { Vec::new() } else { c.soft("titles as typed", "SELECT id, title, points, comments, created FROM hn_stories WHERE MATCH(title, :p) ORDER BY points DESC LIMIT 4", json!({"p": prefix})).await } },
    );
    // The question finished with each completion of its topic, then as typed.
    if let Some((head, _, about)) = &slot {
        let mut qs: Vec<String> = Vec::new();
        for r in phr.iter().take(2) {
            let whole = format!("{head} {}", r.s("text"));
            // Already a question ("who posted the most about kubernetes"), or one with words to come.
            let mut more = if *about && crate::nl::read(&whole).is_some() { vec![whole.clone()] } else { Vec::new() };
            more.extend(crate::nl::complete(&format!("{whole} "), None, 2));
            for q in more {
                if !qs.contains(&q) {
                    qs.push(q);
                }
            }
        }
        for q in crate::nl::complete(&raw, None, 4) {
            if !qs.contains(&q) {
                qs.push(q);
            }
        }
        qs.truncate(4);
        return done(&trace, json!({"questions": qs, "phrases": [], "people": [], "sites": [], "stories": stories}), t0);
    }
    let mut phrases: Vec<Value> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for r in phr.iter() {
        let t = r.s("text");
        if t != typed.trim() && seen.insert(t.clone()) {
            phrases.push(json!({"text": t, "n": r.i("n")}));
        }
    }
    for r in tail.iter() {
        let t = r.s("text");
        // "rust async ru" + "rust" is not a completion; "rust async" + "runtime" is.
        let two = words[words.len().saturating_sub(2)..].join(" ");
        let base = if t.starts_with(&format!("{two}")) && words.len() > 2 { words[..words.len() - 2].join(" ") } else if t.starts_with(&two) { String::new() } else { head.clone() };
        let whole = if base.is_empty() { t.clone() } else { format!("{base} {t}") };
        // "rust async ru" + "async rust" says rust twice: not a completion.
        let said: std::collections::HashSet<&str> = words[..words.len() - 1].iter().copied().collect();
        let added: Vec<&str> = whole.split(' ').skip(words.len() - 1).collect();
        if added.iter().any(|w| said.contains(w)) {
            continue;
        }
        if whole != typed.trim() && seen.insert(whole.clone()) {
            phrases.push(json!({"text": whole, "n": r.i("n")}));
        }
    }
    phrases.truncate(6);
    // Questions: the grammar's templates, the topic filled with the best completion.
    let topic = if lead { None } else { phrases.first().and_then(|x| x["text"].as_str()).map(str::to_string) };
    let questions = crate::nl::complete(&raw, topic.as_deref(), 4);
    let fuzzy = if phrases.is_empty() && questions.is_empty() && frame.is_none() && typed.len() >= 4 {
        c.soft("phrases near what is typed (typos)", "SELECT text, n FROM hn_complete WHERE PREFIX(id, :p, fuzzy => 'auto') ORDER BY weight DESC LIMIT 4", json!({"p": pre("p", &typed)})).await
    } else {
        Vec::new()
    };
    for r in fuzzy {
        phrases.push(json!({"text": r.s("text"), "n": r.i("n"), "fixed": true}));
    }
    done(&trace, json!({"questions": questions, "phrases": phrases, "people": people, "sites": sites, "stories": stories}), t0)
}

async fn item(State(svc): State<Shared>, Path(id): Path<i64>) -> Answer {
    let t0 = Instant::now();
    let trace = Trace::new();
    let c = Ctx { db: &svc.db, trace: &trace };
    match thread::thread(&c, id).await {
        Some(v) => done(&trace, v, t0),
        None => Err(ApiError(StatusCode::NOT_FOUND, format!("item {id} is not in the tables"))),
    }
}

async fn around(State(svc): State<Shared>, Path(id): Path<i64>) -> Answer {
    let t0 = Instant::now();
    let trace = Trace::new();
    let c = Ctx { db: &svc.db, trace: &trace };
    let v = thread::around(&c, id).await;
    done(&trace, v, t0)
}

async fn user(State(svc): State<Shared>, Path(id): Path<String>) -> Answer {
    let t0 = Instant::now();
    let trace = Trace::new();
    let c = Ctx { db: &svc.db, trace: &trace };
    let v = people::person(&c, &id).await;
    done(&trace, v, t0)
}

async fn experts(State(svc): State<Shared>, Query(a): Query<Q>) -> Answer {
    let t0 = Instant::now();
    let trace = Trace::new();
    let c = Ctx { db: &svc.db, trace: &trace };
    let p = query::parse(&a.q, crate::ingest::now());
    let v = people::experts(&c, &p).await;
    done(&trace, v, t0)
}

async fn trends_route(State(svc): State<Shared>, Query(a): Query<Q>) -> Answer {
    let t0 = Instant::now();
    let trace = Trace::new();
    let c = Ctx { db: &svc.db, trace: &trace };
    let terms: Vec<String> = a.q.split([',', '|']).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    let v = trends::trends(&c, &terms, a.by == "month", a.in_ != "stories").await;
    done(&trace, v, t0)
}

async fn ask_route(State(svc): State<Shared>, Query(a): Query<Q>) -> Answer {
    let t0 = Instant::now();
    let trace = Trace::new();
    let c = Ctx { db: &svc.db, trace: &trace };
    let q = a.q.trim();
    if q.len() < 3 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "ask a question".into()));
    }
    // New answers are counted against a day's allowance; kept ones are free.
    let kept = c.soft("a kept answer?", "SELECT id FROM hn_answers WHERE id = :k", json!({"k": ask::key(q)})).await;
    if kept.is_empty() || a.fresh == Some(1) {
        let day = crate::ingest::now() / 86_400;
        let mut asks = svc.asks.lock().unwrap_or_else(|e| e.into_inner());
        if asks.0 != day {
            *asks = (day, 0);
        }
        if asks.1 >= svc.ask_per_day {
            return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, format!("today's {} new answers are made: try a question asked before, or tomorrow", svc.ask_per_day)));
        }
        asks.1 += 1;
    }
    let v = ask::ask(&c, q, &svc.model, a.fresh == Some(1)).await.map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e))?;
    done(&trace, v, t0)
}

#[derive(serde::Deserialize, Default)]
pub struct TrendingParams {
    #[serde(default)]
    hours: Option<i64>,
    #[serde(default)]
    year: Option<i64>,
}

/// What HN's titles are suddenly about: the last hours against the year
/// before.
async fn trending_route(State(svc): State<Shared>, Query(a): Query<TrendingParams>) -> Answer {
    let t0 = Instant::now();
    let trace = Trace::new();
    let c = Ctx { db: &svc.db, trace: &trace };
    let v = crate::trending::now(&c, a.hours.unwrap_or(48)).await;
    done(&trace, v, t0)
}

/// The spikes of the archive's months: a year's, or every month's
/// strongest.
async fn spikes_route(State(svc): State<Shared>, Query(a): Query<TrendingParams>) -> Answer {
    let t0 = Instant::now();
    let trace = Trace::new();
    let c = Ctx { db: &svc.db, trace: &trace };
    let v = crate::trending::history(&c, a.year).await;
    done(&trace, v, t0)
}

#[derive(serde::Deserialize, Default)]
pub struct DigestParams {
    #[serde(default)]
    week: Option<String>,
    #[serde(default)]
    id: String,
}

/// A week's issue of the digest: its picks, numbers and kept prose.
async fn digest_route(State(svc): State<Shared>, Query(a): Query<DigestParams>) -> Answer {
    let t0 = Instant::now();
    let trace = Trace::new();
    let c = Ctx { db: &svc.db, trace: &trace };
    let v = crate::digest::issue(&c, a.week.as_deref()).await;
    done(&trace, v, t0)
}

/// Writes one entry of an issue (or its opening, `id=intro`) through
/// `llm()`, once: a kept entry comes back as it is, and a new one counts
/// against the day's allowance of new answers.
async fn digest_write_route(State(svc): State<Shared>, Query(a): Query<DigestParams>) -> Answer {
    let t0 = Instant::now();
    let trace = Trace::new();
    let c = Ctx { db: &svc.db, trace: &trace };
    let week = a.week.clone().unwrap_or_default();
    let id = a.id.trim();
    if id != "intro" && id.parse::<u64>().is_err() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "id is a story's id, or intro".into()));
    }
    let from = crate::digest::monday(Some(&week));
    let k = format!("digest:{}:{id}", from.format("%Y-%m-%d"));
    let kept = c.soft("a kept entry?", "SELECT id FROM hn_answers WHERE id = :k", json!({"k": k})).await;
    if kept.is_empty() {
        let day = crate::ingest::now() / 86_400;
        let mut n = svc.digests.lock().unwrap_or_else(|e| e.into_inner());
        if n.0 != day {
            *n = (day, 0);
        }
        if n.1 >= svc.digest_per_day {
            return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, format!("today's {} new digest entries are written: this one is written tomorrow", svc.digest_per_day)));
        }
        n.1 += 1;
    }
    let v = crate::digest::write(&c, &week, id, &svc.digest_model).await.map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e))?;
    done(&trace, v, t0)
}

#[derive(serde::Deserialize, Default)]
pub struct LinkParams {
    #[serde(default)]
    u: String,
}

/// One link across HN: its submissions, the comments elsewhere that cite
/// it, the people around it and the graph of them; a site's most
/// discussed links; with no link, the most discussed links of all.
#[derive(serde::Deserialize, Default)]
pub struct CanonParams {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    from: Option<i64>,
    #[serde(default)]
    to: Option<i64>,
    #[serde(default)]
    q: String,
    #[serde(default)]
    id: String,
}

/// The books or papers HN recommends most, all told or for some years.
async fn canon_route(State(svc): State<Shared>, Query(a): Query<CanonParams>) -> Answer {
    let t0 = Instant::now();
    let trace = Trace::new();
    let c = Ctx { db: &svc.db, trace: &trace };
    let v = crate::canon::list(&c, &a.kind, a.from, a.to, &a.q).await;
    done(&trace, v, t0)
}

/// One work of the canon, with the comments that named it.
async fn canon_work_route(State(svc): State<Shared>, Query(a): Query<CanonParams>) -> Answer {
    let t0 = Instant::now();
    let trace = Trace::new();
    let c = Ctx { db: &svc.db, trace: &trace };
    match crate::canon::work(&c, &a.id).await {
        Some(v) => done(&trace, v, t0),
        None => Err(ApiError(StatusCode::NOT_FOUND, format!("{} is not in the canon", a.id))),
    }
}

#[derive(serde::Deserialize, Default)]
pub struct MindParams {
    #[serde(default)]
    topic: String,
    #[serde(default)]
    who: String,
}

/// The people who wrote about a topic in the most years, and the arcs
/// of those already read.
async fn minds_route(State(svc): State<Shared>, Query(a): Query<MindParams>) -> Answer {
    let t0 = Instant::now();
    let trace = Trace::new();
    let c = Ctx { db: &svc.db, trace: &trace };
    let v = crate::minds::people(&c, &a.topic).await;
    done(&trace, v, t0)
}

/// One person's arc on a topic: kept, or read now by the model (counted
/// against the day's answers, as questions are).
async fn arc_route(State(svc): State<Shared>, Query(a): Query<MindParams>) -> Answer {
    let t0 = Instant::now();
    let trace = Trace::new();
    let c = Ctx { db: &svc.db, trace: &trace };
    if a.topic.trim().is_empty() || a.who.trim().is_empty() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "name a topic and a person".into()));
    }
    let kept = c.soft("a kept arc?", "SELECT id FROM hn_answers WHERE id = :k", json!({"k": crate::minds::key(&a.topic, a.who.trim())})).await;
    if kept.is_empty() {
        let day = crate::ingest::now() / 86_400;
        let mut asks = svc.asks.lock().unwrap_or_else(|e| e.into_inner());
        if asks.0 != day {
            *asks = (day, 0);
        }
        if asks.1 >= svc.ask_per_day {
            return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, format!("today's {} new readings are made: try one read before, or tomorrow", svc.ask_per_day)));
        }
        asks.1 += 1;
    }
    let v = crate::minds::arc(&c, &a.topic, a.who.trim(), &svc.model).await.map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e))?;
    done(&trace, v, t0)
}

async fn link_route(State(svc): State<Shared>, Query(a): Query<LinkParams>) -> Answer {
    let t0 = Instant::now();
    let trace = Trace::new();
    let c = Ctx { db: &svc.db, trace: &trace };
    let u = a.u.trim();
    let v = if u.is_empty() { crate::link::famous(&c).await } else { crate::link::link(&c, u).await };
    done(&trace, v, t0)
}

/// The comments a question's answer is made from, before the model is
/// asked (the page shows them while it writes).
async fn ask_comments_route(State(svc): State<Shared>, Query(a): Query<Q>) -> Answer {
    let t0 = Instant::now();
    let trace = Trace::new();
    let c = Ctx { db: &svc.db, trace: &trace };
    let q = a.q.trim();
    if q.len() < 3 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "ask a question".into()));
    }
    let v = ask::comments_for(&c, q).await;
    done(&trace, v, t0)
}

/// The front page as the tables hold it now: the newest stories, ranked
/// as HN ranks them (points over age, with gravity), with the live feed's
/// state.
async fn front(State(svc): State<Shared>) -> Answer {
    let t0 = Instant::now();
    let trace = Trace::new();
    let c = Ctx { db: &svc.db, trace: &trace };
    let rows = c.soft("the newest stories, ranked as HN's front page ranks them", &format!("SELECT {}, (points - 1) / pow((date_part('epoch', now()) - date_part('epoch', created)) / 3600.0 + 2, 1.8) AS rank FROM hn_stories WHERE created > now() - INTERVAL '2 days' ORDER BY rank DESC LIMIT 30", search::STORY_COLS), Value::Null).await;
    done(&trace, json!({"stories": rows, "feed": svc.feed.as_ref().map(|f| f.status())}), t0)
}
