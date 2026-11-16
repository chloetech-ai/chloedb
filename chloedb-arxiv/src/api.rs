//! The app's HTTP API, and its page. ChloeDB's command center reaches it
//! through chloedb-server (`--app arxiv=URL`), which loads the view from
//! here (`/app.json`, `/ui/view.js`). With `--public` the app also serves
//! the page itself at `/`, for anyone: searches, papers, people, topics,
//! trends, the atlas, and a reader's own feeds and recommendations (kept
//! under an id their browser makes), with a limit of requests per address
//! and of new model calls a day.
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
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::digest::{self, Config, Pages};
use crate::live::Live;
use crate::search::{self, Ctx, Request as SearchReq};
use crate::strata::{RowExt, Strata, Trace};
use crate::{answer, ask, born, code, collab, gaps, labs, spread, pages, schema, understand, watch};

pub const VIEW: &str = include_str!("../ui/view.js");
pub const INDEX: &str = include_str!("../ui/index.html");

/// An open book with a spark: papers, read and found.
pub const ICON: &str = r#"<path d="M3 5.5c2.6-1 5.4-1 8 .6v13c-2.6-1.6-5.4-1.6-8-.6z"/><path d="M21 5.5c-2.6-1-5.4-1-8 .6v13c2.6-1.6 5.4-1.6 8-.6z"/><path d="M17.5 2v3M16 3.5h3"/>"#;

pub struct Service {
    pub db: Strata,
    /// The graph server (events, recommender, citations, coauthors, what
    /// readers keep); `db` again when one server holds everything.
    pub graph: Strata,
    pub token: Option<String>,
    pub public: bool,
    pub model: String,
    /// The model that explains an issue's papers from their full text.
    pub issue_model: String,
    pub llm_per_day: usize,
    pub llm_used: Mutex<(i64, usize)>,
    pub per_minute: usize,
    pub seen: Mutex<HashMap<String, (i64, usize)>>,
    pub started: Instant,
    pub live: Option<Arc<Live>>,
    pub pages: Pages,
    pub jobs: Mutex<HashMap<String, Value>>,
    /// The gap finder's list and when it was made (made again after 12 hours).
    pub gaps: tokio::sync::Mutex<Option<(Instant, Value)>>,
    /// Topics born lately, made once and kept 12 hours.
    pub born: tokio::sync::Mutex<Option<(Instant, Value)>>,
    /// Techniques' spread over categories, made once and kept 12 hours.
    pub spread: tokio::sync::Mutex<Option<(Instant, Value)>>,
    /// Answers kept for a while by their key (rising people, people to work with).
    pub memo: Mutex<HashMap<String, (Instant, Value)>>,
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
        .route("/v1/suggest", get(suggest))
        .route("/v1/paper/*id", get(paper))
        .route("/v1/tldr/*id", get(tldr))
        .route("/v1/new/*id", get(novelty))
        .route("/v1/author/:id", get(author))
        .route("/v1/collab/:id", get(collaborators))
        .route("/v1/reviewers", post(reviewers_for_text))
        .route("/v1/reviewers/*id", get(reviewers_for_paper))
        .route("/v1/topic/:id", get(topic))
        .route("/v1/concept/:slug", get(concept))
        .route("/v1/category/:code", get(category))
        .route("/v1/venue/:name", get(venue))
        .route("/v1/atlas", get(atlas))
        .route("/v1/trends", get(trends))
        .route("/v1/hot", get(hot))
        .route("/v1/hot/people", get(hot_people))
        .route("/v1/code", get(code_page))
        .route("/v1/labs", get(labs_index))
        .route("/v1/labs/find", get(labs_find))
        .route("/v1/lab/:id", get(lab_page))
        .route("/v1/labs/of/*id", get(paper_labs))
        .route("/v1/gaps", get(gaps_list))
        .route("/v1/gap", get(gap_one))
        .route("/v1/born", get(born_list))
        .route("/v1/spread", get(spread_list))
        .route("/v1/spread/:id", get(spread_one))
        .route("/v1/cite", post(cite))
        .route("/v1/summary", get(summary))
        .route("/v1/foryou", get(for_you))
        .route("/v1/event", post(event))
        .route("/v1/library", get(library))
        .route("/v1/feeds", get(feeds).post(save_feed))
        .route("/v1/feeds/example", get(example_feed))
        .route("/v1/feeds/preview", post(preview))
        .route("/v1/feeds/:id", get(feed))
        .route("/v1/feeds/:id/issue", post(make_issue))
        .route("/v1/jobs/:id", get(job))
        .route("/v1/issue/:id", get(issue))
        .route("/v1/figure/*id", get(figure))
        .route("/feeds/:id/rss.xml", get(rss))
        .route("/v1/watches", get(watches).post(add_watch))
        .route("/v1/watches/:id", get(watch_one))
        .route("/v1/watches/:id/delete", post(drop_watch))
        .route("/watches/:id/rss.xml", get(watch_rss))
        .layer(axum::middleware::from_fn_with_state(svc.clone(), guard))
        .route("/healthz", get(|| async { "ok" }))
        .with_state(svc)
}

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// What a reader without the token may do in public: read (What to cite
/// is a read sent as a POST, a draft being too long for a URL), and keep
/// their own events, library, feeds and standing questions.
fn public_ok(method: &Method, path: &str) -> bool {
    method == Method::GET
        || (method == Method::POST
            && (path == "/v1/event" || path == "/v1/cite" || path == "/v1/feeds" || path == "/v1/feeds/preview" || (path.starts_with("/v1/feeds/") && path.ends_with("/issue")) || path == "/v1/watches" || path == "/v1/reviewers" || (path.starts_with("/v1/watches/") && path.ends_with("/delete"))))
}

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
        if !(svc.public && public_ok(req.method(), &path)) {
            return ApiError(StatusCode::UNAUTHORIZED, "this app needs its token".into()).into_response();
        }
        let who = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()).and_then(|v| v.split(',').next()).map(|s| s.trim().to_string())
            .or_else(|| req.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip().to_string())).unwrap_or_default();
        if path.starts_with("/v1/") {
            let minute = crate::live::now() / 60;
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

impl Service {
    /// One new model call against the day's allowance.
    fn spend(&self, n: usize) -> Result<(), ApiError> {
        let day = crate::live::now() / 86_400;
        let mut u = self.llm_used.lock().unwrap_or_else(|e| e.into_inner());
        if u.0 != day {
            *u = (day, 0);
        }
        if u.1 + n > self.llm_per_day {
            return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, format!("today's {} new summaries are written: try one asked before, or tomorrow", self.llm_per_day)));
        }
        u.1 += n;
        Ok(())
    }
}

fn ui_file(name: &str, compiled: &'static str) -> std::borrow::Cow<'static, str> {
    match std::env::var_os("CHLOEDB_ARXIV_UI_DIR") {
        Some(d) => std::fs::read_to_string(std::path::Path::new(&d).join(name)).map(Into::into).unwrap_or(compiled.into()),
        None => compiled.into(),
    }
}

async fn index(State(svc): State<Shared>) -> Response {
    if !svc.public && svc.token.is_some() {
        return (StatusCode::NOT_FOUND, "this app is shown in ChloeDB's command center (Apps → arXiv)").into_response();
    }
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8"), (header::CACHE_CONTROL, "no-cache")], ui_file("index.html", INDEX).into_owned()).into_response()
}

async fn manifest() -> Json<Value> {
    Json(json!({
        "name": "arxiv",
        "title": "arXiv",
        "description": "Every arXiv paper, searched by words and meaning, with questions answered in SQL, topics, trends, people, citations, what to read next and your own weekly digest",
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

macro_rules! ctx {
    ($svc:expr, $trace:ident, $c:ident) => {
        let $trace = Trace::new();
        let $c = Ctx { db: &$svc.db, g: &$svc.graph, trace: &$trace };
    };
}

async fn status(State(svc): State<Shared>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let (counts, span) = tokio::join!(schema::counts(&svc.db, &svc.graph), c.soft("the papers' span", "SELECT MIN(created) AS first, MAX(created) AS last FROM ax_papers", Value::Null));
    done(&trace, json!({
        "tables": counts.unwrap_or(Value::Null), "papers": span.first(),
        "live": svc.live.as_ref().map(|l| l.status()), "public": svc.public, "model": svc.model, "issue_model": svc.issue_model, "uptime_s": svc.started.elapsed().as_secs(),
    }), t0)
}

/// The front page: the newest day's papers by area, what grows fastest,
/// the most cited of the last months, and the counts.
async fn home(State(svc): State<Shared>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let cols = search::COLS;
    let (last, rising, areas, topics, techs, n) = tokio::join!(
        c.soft("the newest papers", &format!("SELECT {cols} FROM ax_papers ORDER BY created DESC LIMIT 24"), Value::Null),
        c.soft("the most cited of the last six months", &format!("SELECT {cols} FROM ax_papers WHERE created >= now() - INTERVAL '182 days' ORDER BY cites DESC LIMIT 12"), Value::Null),
        c.soft("the last month's papers by area", "SELECT area, COUNT(*) AS n FROM ax_papers WHERE created >= now() - INTERVAL '30 days' GROUP BY area ORDER BY n DESC", Value::Null),
        c.soft("the topics growing fastest", "SELECT id, label, about, field_label, papers, recent, growth FROM ax_topics WHERE recent >= 150 ORDER BY growth DESC LIMIT 12", Value::Null),
        c.soft("the techniques growing fastest", "SELECT id, kind, name, papers, recent, growth FROM ax_concepts WHERE recent >= 200 ORDER BY growth DESC LIMIT 12", Value::Null),
        c.soft("how many papers", "SELECT COUNT(*) AS n, MAX(created) AS last FROM ax_papers", Value::Null),
    );
    let groups: HashMap<String, String> = crate::concepts::groups().iter().map(|(a, b)| (a.clone(), b.clone())).collect();
    done(&trace, json!({
        "newest": last, "rising": rising,
        "areas": areas.iter().map(|r| json!({"id": r.s("area"), "name": groups.get(&r.s("area")).cloned().unwrap_or(r.s("area")), "n": r.i("n")})).collect::<Vec<_>>(),
        "topics": topics, "concepts": techs,
        "papers": n.first().map(|r| r.i("n")).unwrap_or(0), "last": n.first().map(|r| r.s("last")),
        "live": svc.live.as_ref().map(|l| l.status()),
    }), t0)
}

#[derive(serde::Deserialize, Default)]
pub struct SearchParams {
    #[serde(default)]
    q: String,
    #[serde(default)]
    sort: String,
    #[serde(default)]
    mode: String,
    #[serde(default)]
    page: usize,
    #[serde(default)]
    facets: Option<u8>,
    #[serde(default)]
    per: Option<usize>,
}

/// One search: the query understood, its question answered, the papers,
/// the facets, and a word the titles hold for each typo.
async fn search_route(State(svc): State<Shared>, Query(a): Query<SearchParams>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let mut p = understand::parse(&a.q, crate::live::now());
    if let Some(who) = p.author_maybe.clone() {
        let found = search::person(&c, &who).await;
        p.confirm_author(found);
    }
    // An id alone: the paper itself.
    if let (Some(id), true) = (p.paper.clone(), p.text.split_whitespace().count() <= 1) {
        return done(&trace, json!({"q": a.q, "understood": p, "open": {"paper": id}}), t0);
    }
    let sort = if !a.sort.is_empty() { a.sort.clone() } else { p.sort.clone().unwrap_or_else(|| "best".into()) };
    let r = SearchReq { sort: sort.clone(), mode: a.mode.clone() };
    let per = a.per.unwrap_or(match p.limit { Some(n) if n <= 50 => n, _ => 20 }).clamp(1, 50);
    let off = a.page * per;
    let words = p.words();
    let facets = a.facets != Some(0) && a.page == 0;
    let (found, f, dym) = tokio::join!(
        search::papers(&c, &p, &r, per, off),
        async { if facets { Some(search::facets(&c, &p).await) } else { None } },
        async { if a.page == 0 && !words.is_empty() { search::did_you_mean(&c, &words).await } else { None } },
    );
    let (mut papers, total) = found?;
    let ans = if a.page == 0 { answer::answer(&c, &p, &papers, total).await } else { None };
    search::mark(&c, &p, &mut papers).await;
    search::trim_abstract(&mut papers, 420);
    done(&trace, json!({
        "q": a.q, "understood": p, "sort": sort, "mode": if r.mode.is_empty() { "hybrid" } else { r.mode.as_str() },
        "answer": ans, "papers": papers, "total": total, "facets": f, "did_you_mean": dym, "page": a.page, "per_page": per,
    }), t0)
}

#[derive(serde::Deserialize, Default)]
struct Q {
    #[serde(default)]
    q: String,
    #[serde(default)]
    cat: String,
    #[serde(default)]
    area: String,
    #[serde(default)]
    user: String,
    #[serde(default)]
    fresh: Option<u8>,
    /// What is rising: the days of each span compared.
    #[serde(default)]
    days: Option<i64>,
    /// The gap finder: a technique and a domain.
    #[serde(default)]
    t: String,
    #[serde(default)]
    d: String,
}

/// Completions as a search is typed: questions the grammar reads, phrases
/// of titles, techniques, categories and people by a prefix of
/// `ax_complete`'s key, and papers whose titles hold the words.
async fn suggest(State(svc): State<Shared>, Query(a): Query<Q>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let raw = a.q.trim_start().to_string();
    let typed: String = raw.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    if typed.chars().count() < 2 {
        return done(&trace, json!({"questions": [], "phrases": [], "people": [], "concepts": [], "categories": [], "papers": []}), t0);
    }
    let partial = !raw.ends_with(' ');
    let words: Vec<&str> = typed.split(' ').collect();
    let last = words.last().copied().unwrap_or("").to_string();
    let p = understand::parse(&raw, crate::live::now());
    let topic_words = p.match_text().to_lowercase();
    let full = if partial { typed.clone() } else { format!("{typed} ") };
    let tw = if partial { topic_words.clone() } else { format!("{topic_words} ") };
    let prefix = if topic_words.is_empty() { String::new() } else if partial { format!("{topic_words}*") } else { topic_words.clone() };
    let in_frame = p.ask.is_some();
    let (phr, tail, people, concepts, cats, papers) = tokio::join!(
        c.soft("phrases as typed", "SELECT text, n FROM ax_complete WHERE PREFIX(id, :p) ORDER BY weight DESC LIMIT 8", json!({"p": format!("p:{}", if in_frame { &tw } else { &full })})),
        async {
            if words.len() > 1 && !in_frame && partial {
                c.soft("the last word as typed", "SELECT text, n FROM ax_complete WHERE PREFIX(id, :p) ORDER BY weight DESC LIMIT 5", json!({"p": format!("p:{last}")})).await
            } else {
                Vec::new()
            }
        },
        async { if last.len() >= 2 { c.soft("people as typed", "SELECT text, n, ref FROM ax_complete WHERE PREFIX(id, :p) ORDER BY weight DESC LIMIT 4", json!({"p": format!("a:{}", if words.len() >= 2 && !in_frame { typed.clone() } else { last.clone() })})).await } else { Vec::new() } },
        async { if last.len() >= 2 { c.soft("techniques and domains as typed", "SELECT text, n, ref, kind FROM ax_complete WHERE PREFIX(id, :p) ORDER BY weight DESC LIMIT 4", json!({"p": format!("t:{}", if in_frame { topic_words.clone() } else { typed.clone() })})).await } else { Vec::new() } },
        async { if last.len() >= 2 { c.soft("categories as typed", "SELECT text, n, ref FROM ax_complete WHERE PREFIX(id, :p) ORDER BY weight DESC LIMIT 3", json!({"p": format!("c:{last}")})).await } else { Vec::new() } },
        async { if prefix.len() >= 3 { c.soft("titles as typed", "SELECT id, title, year, cites FROM ax_papers WHERE MATCH(title, :p) ORDER BY cites DESC LIMIT 4", json!({"p": prefix})).await } else { Vec::new() } },
    );
    let mut phrases: Vec<Value> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let head = if in_frame { typed.trim_end().strip_suffix(&topic_words).unwrap_or("").to_string() } else { String::new() };
    for r in phr.iter() {
        let t = r.s("text");
        let whole = format!("{head}{t}");
        if whole != typed.trim() && seen.insert(whole.clone()) {
            phrases.push(json!({"text": whole, "n": r.i("n")}));
        }
    }
    let before_last = if words.len() > 1 { words[..words.len() - 1].join(" ") } else { String::new() };
    for r in tail.iter() {
        let whole = format!("{before_last} {}", r.s("text"));
        if whole != typed.trim() && seen.insert(whole.clone()) {
            phrases.push(json!({"text": whole, "n": r.i("n")}));
        }
    }
    phrases.truncate(6);
    let topic = phrases.first().and_then(|x| x["text"].as_str()).map(str::to_string);
    let questions = understand::complete(&raw, topic.as_deref(), 4);
    let fuzzy = if phrases.is_empty() && questions.is_empty() && typed.len() >= 4 {
        c.soft("phrases near what is typed (typos)", "SELECT text, n FROM ax_complete WHERE PREFIX(id, :p, fuzzy => 'auto') ORDER BY weight DESC LIMIT 4", json!({"p": format!("p:{typed}")})).await
    } else {
        Vec::new()
    };
    for r in fuzzy {
        phrases.push(json!({"text": r.s("text"), "n": r.i("n"), "fixed": true}));
    }
    let mut pseen = std::collections::HashSet::new();
    let people: Vec<Value> = people.iter().filter(|r| pseen.insert(r.s("ref"))).map(|r| json!({"name": r.s("text"), "id": r.s("ref"), "n": r.i("n")})).collect();
    done(&trace, json!({
        "questions": questions, "phrases": phrases, "people": people,
        "concepts": concepts.iter().map(|r| json!({"name": r.s("text"), "id": r.s("ref"), "n": r.i("n")})).collect::<Vec<_>>(),
        "categories": cats.iter().map(|r| json!({"name": r.s("text"), "id": r.s("ref"), "n": r.i("n")})).collect::<Vec<_>>(),
        "papers": papers,
    }), t0)
}

fn not_found(what: &str) -> ApiError {
    ApiError(StatusCode::NOT_FOUND, format!("{what} is not in the tables"))
}

async fn paper(State(svc): State<Shared>, Path(id): Path<String>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let v = pages::paper(&c, &id).await.ok_or_else(|| not_found(&format!("paper {id}")))?;
    done(&trace, v, t0)
}

async fn tldr(State(svc): State<Shared>, Path(id): Path<String>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let kept = c.gsoft("a kept TL;DR?", "SELECT id FROM ax_notes WHERE id = :k", json!({"k": format!("tldr:{}", understand::clean_id(&id))})).await;
    if kept.is_empty() {
        svc.spend(1)?;
    }
    let v = ask::tldr(&c, &id, &svc.model).await.map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e))?;
    done(&trace, v, t0)
}

/// What a paper adds to the earlier papers closest to it.
async fn novelty(State(svc): State<Shared>, Path(id): Path<String>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let kept = c.gsoft("a kept comparison?", "SELECT id FROM ax_notes WHERE id = :k", json!({"k": format!("new:{}", understand::clean_id(&id))})).await;
    if kept.is_empty() {
        svc.spend(1)?;
    }
    let v = ask::novelty(&c, &id, &svc.model).await.map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e))?;
    done(&trace, v, t0)
}

async fn author(State(svc): State<Shared>, Path(id): Path<String>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let v = pages::author(&c, &id).await.ok_or_else(|| not_found(&format!("person {id}")))?;
    done(&trace, v, t0)
}

async fn collaborators(State(svc): State<Shared>, Path(id): Path<String>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let key = format!("collab:{id}");
    let v = match memo_get(&svc, &key, 12 * 3600) {
        Some(v) => v,
        None => {
            let v = collab::find(&c, &id).await.ok_or_else(|| not_found(&format!("person {id}")))?;
            memo_put(&svc, &key, &v);
            v
        }
    };
    done(&trace, v, t0)
}

async fn reviewers_for_paper(State(svc): State<Shared>, Path(id): Path<String>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let id = understand::clean_id(&id);
    let r = c.soft("the paper", "SELECT id, title, abstract, people, pcat FROM ax_papers WHERE id = :id", json!({"id": id})).await.into_iter().next().ok_or_else(|| not_found(&format!("paper {id}")))?;
    let authors: Vec<String> = r.get("people").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
    let mut v = collab::reviewers(&c, &r.s("title"), &r.s("abstract"), &crate::concepts::group_of(&r.s("pcat")), &authors, Some(&id)).await;
    v["paper"] = json!({"id": id, "title": r.s("title")});
    done(&trace, v, t0)
}

#[derive(serde::Deserialize, Default)]
struct ReviewBody {
    #[serde(default)]
    title: String,
    #[serde(default, rename = "abstract")]
    abstract_: String,
    #[serde(default)]
    authors: String,
}

async fn reviewers_for_text(State(svc): State<Shared>, Json(b): Json<ReviewBody>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let n = b.title.chars().count() + b.abstract_.chars().count();
    if n < 40 || n > 6000 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "give a title and abstract, 40 to 6,000 characters".into()));
    }
    let authors: Vec<String> = b.authors.split([',', ';', '\n']).flat_map(|a| a.split(" and ")).map(|a| crate::concepts::person_key("", a.trim())).filter(|a| !a.is_empty()).take(50).collect();
    let mut v = collab::reviewers(&c, b.title.trim(), b.abstract_.trim(), "cs", &authors, None).await;
    v["authors"] = json!(authors);
    done(&trace, v, t0)
}

async fn topic(State(svc): State<Shared>, Path(id): Path<i64>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let v = pages::topic(&c, id).await.ok_or_else(|| not_found(&format!("topic {id}")))?;
    done(&trace, v, t0)
}

async fn concept(State(svc): State<Shared>, Path(slug): Path<String>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let v = pages::concept(&c, &slug).await.ok_or_else(|| not_found(&format!("concept {slug}")))?;
    done(&trace, v, t0)
}

async fn category(State(svc): State<Shared>, Path(code): Path<String>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let v = pages::category(&c, &code).await.ok_or_else(|| not_found(&format!("category {code}")))?;
    done(&trace, v, t0)
}

async fn venue(State(svc): State<Shared>, Path(name): Path<String>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let v = pages::venue(&c, &name).await.ok_or_else(|| not_found(&format!("venue {name}")))?;
    done(&trace, v, t0)
}

async fn atlas(State(svc): State<Shared>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let v = pages::atlas(&c).await;
    done(&trace, v, t0)
}

/// Topics side by side, year by year (`q=lora,qlora,dora`).
async fn trends(State(svc): State<Shared>, Query(a): Query<Q>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let terms: Vec<String> = a.q.split([',', '|']).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).take(6).collect();
    if terms.is_empty() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "name a topic or more: q=a,b".into()));
    }
    let mut p = understand::Parsed::default();
    if !a.cat.is_empty() {
        p.cats.push(a.cat.clone());
    }
    p.ask = Some(understand::Ask::Compare { terms });
    let v = answer::answer(&c, &p, &[], 0).await.unwrap_or(Value::Null);
    done(&trace, v, t0)
}

/// What grows fastest, in a category or an area or all of arXiv.
async fn hot(State(svc): State<Shared>, Query(a): Query<Q>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let mut p = understand::Parsed::default();
    if !a.cat.is_empty() {
        p.cats.push(a.cat.clone());
    }
    if !a.area.is_empty() {
        p.area = Some(a.area.clone());
    }
    p.ask = Some(understand::Ask::Hot);
    p.window = a.days;
    let v = answer::answer(&c, &p, &[], 0).await.unwrap_or(Value::Null);
    done(&trace, v, t0)
}

/// An answer kept for `secs` under `key`; at most 2,000 are kept.
fn memo_get(svc: &Service, key: &str, secs: u64) -> Option<Value> {
    let m = svc.memo.lock().unwrap();
    m.get(key).filter(|(at, _)| at.elapsed() < Duration::from_secs(secs)).map(|(_, v)| v.clone())
}

fn memo_put(svc: &Service, key: &str, v: &Value) {
    let mut m = svc.memo.lock().unwrap();
    if m.len() >= 2000 {
        m.retain(|_, (at, _)| at.elapsed() < Duration::from_secs(3600));
        if m.len() >= 2000 {
            m.clear();
        }
    }
    m.insert(key.to_string(), (Instant::now(), v.clone()));
}

async fn labs_index(State(svc): State<Shared>, Query(a): Query<Q>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let key = format!("labs:{}:{}", a.area, a.t);
    let v = match memo_get(&svc, &key, 12 * 3600) {
        Some(v) => v,
        None => {
            let v = labs::index(&c, &a.area, &a.t).await;
            memo_put(&svc, &key, &v);
            v
        }
    };
    done(&trace, v, t0)
}

async fn labs_find(State(svc): State<Shared>, Query(a): Query<Q>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let q: String = a.q.chars().take(100).collect();
    if q.trim().is_empty() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "name a lab: q=".into()));
    }
    let v = labs::find(&c, q.trim()).await;
    done(&trace, v, t0)
}

async fn lab_page(State(svc): State<Shared>, Path(id): Path<String>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let id = id.to_lowercase();
    let key = format!("lab:{id}");
    let v = match memo_get(&svc, &key, 12 * 3600) {
        Some(v) => v,
        None => {
            let v = labs::lab(&c, &id).await.ok_or_else(|| not_found(&format!("lab {id}")))?;
            memo_put(&svc, &key, &v);
            v
        }
    };
    done(&trace, v, t0)
}

async fn paper_labs(State(svc): State<Shared>, Path(id): Path<String>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let v = labs::of_paper(&c, &understand::clean_id(&id)).await;
    done(&trace, v, t0)
}

async fn code_page(State(svc): State<Shared>, Query(a): Query<Q>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let key = format!("code:{}", a.area);
    let v = match memo_get(&svc, &key, 12 * 3600) {
        Some(v) => v,
        None => {
            let v = code::page(&c, &a.area).await;
            memo_put(&svc, &key, &v);
            v
        }
    };
    done(&trace, v, t0)
}

async fn hot_people(State(svc): State<Shared>, Query(a): Query<Q>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let days = a.days.unwrap_or(730).clamp(365, 1460);
    let key = format!("rising:{}:{days}", a.area);
    let v = match memo_get(&svc, &key, 12 * 3600) {
        Some(v) => v,
        None => {
            let v = collab::rising(&c, &a.area, days).await;
            memo_put(&svc, &key, &v);
            v
        }
    };
    done(&trace, v, t0)
}

/// The gap finder's list, made once and kept 12 hours.
pub async fn gaps_cached(svc: &Service) -> (Value, f64, bool) {
    let mut g = svc.gaps.lock().await;
    if let Some((at, v)) = g.as_ref() {
        if at.elapsed() < Duration::from_secs(12 * 3600) {
            return (v.clone(), at.elapsed().as_secs_f64(), true);
        }
    }
    let trace = Trace::new();
    let c = Ctx { db: &svc.db, g: &svc.graph, trace: &trace };
    let t0 = Instant::now();
    let mut v = gaps::find(&c).await;
    v["made_ms"] = json!((t0.elapsed().as_secs_f64() * 1000.0).round());
    v["made_queries"] = json!(trace.take().len());
    if v["gaps"].as_array().is_some_and(|a| !a.is_empty()) {
        *g = Some((Instant::now(), v.clone()));
    }
    (v, 0.0, false)
}

/// Topics born lately, made once and kept 12 hours.
pub async fn born_cached(svc: &Service) -> (Value, f64, bool) {
    let mut g = svc.born.lock().await;
    if let Some((at, v)) = g.as_ref() {
        if at.elapsed() < Duration::from_secs(12 * 3600) {
            return (v.clone(), at.elapsed().as_secs_f64(), true);
        }
    }
    let trace = Trace::new();
    let c = Ctx { db: &svc.db, g: &svc.graph, trace: &trace };
    let t0 = Instant::now();
    let mut v = born::find(&c).await;
    v["made_ms"] = json!((t0.elapsed().as_secs_f64() * 1000.0).round());
    v["made_queries"] = json!(trace.take().len());
    if v["topics"].as_array().is_some_and(|a| !a.is_empty()) {
        *g = Some((Instant::now(), v.clone()));
    }
    (v, 0.0, false)
}

/// Techniques' spread over categories, made once and kept 12 hours.
pub async fn spread_cached(svc: &Service) -> (Value, f64, bool) {
    let mut g = svc.spread.lock().await;
    if let Some((at, v)) = g.as_ref() {
        if at.elapsed() < Duration::from_secs(12 * 3600) {
            return (v.clone(), at.elapsed().as_secs_f64(), true);
        }
    }
    let trace = Trace::new();
    let c = Ctx { db: &svc.db, g: &svc.graph, trace: &trace };
    let t0 = Instant::now();
    let mut v = spread::find(&c).await;
    v["made_ms"] = json!((t0.elapsed().as_secs_f64() * 1000.0).round());
    v["made_queries"] = json!(trace.take().len());
    if v["techniques"].as_array().is_some_and(|a| !a.is_empty()) {
        *g = Some((Instant::now(), v.clone()));
    }
    (v, 0.0, false)
}

async fn spread_list(State(svc): State<Shared>) -> Answer {
    let t0 = Instant::now();
    let (mut v, age, kept) = spread_cached(&svc).await;
    v["age_s"] = json!(age.round());
    v["kept"] = json!(kept);
    v["took_ms"] = json!((t0.elapsed().as_secs_f64() * 10000.0).round() / 10.0);
    Ok(Json(v))
}

/// One technique's first papers in each category it reached.
async fn spread_one(State(svc): State<Shared>, Path(id): Path<String>) -> Answer {
    let t0 = Instant::now();
    let (list, _, _) = spread_cached(&svc).await;
    let Some(t) = list["techniques"].as_array().and_then(|a| a.iter().find(|t| t["id"] == id.as_str()).cloned()) else {
        return Err(ApiError(StatusCode::NOT_FOUND, format!("no technique {id:?} with a spread")));
    };
    let mut stops: Vec<(String, i64)> = vec![(t["home"]["cat"].as_str().unwrap_or_default().to_string(), t["home"]["year"].as_i64().unwrap_or(0))];
    stops.extend(t["hops"].as_array().into_iter().flatten().map(|h| (h["cat"].as_str().unwrap_or_default().to_string(), h["year"].as_i64().unwrap_or(0))));
    ctx!(svc, trace, c);
    let mut v = spread::firsts(&c, &id, &stops).await;
    v["technique"] = t;
    done(&trace, v, t0)
}

async fn born_list(State(svc): State<Shared>) -> Answer {
    let t0 = Instant::now();
    let (mut v, age, kept) = born_cached(&svc).await;
    v["age_s"] = json!(age.round());
    v["kept"] = json!(kept);
    v["took_ms"] = json!((t0.elapsed().as_secs_f64() * 10000.0).round() / 10.0);
    Ok(Json(v))
}

async fn gaps_list(State(svc): State<Shared>, Query(a): Query<Q>) -> Answer {
    let t0 = Instant::now();
    let (mut v, age, kept) = gaps_cached(&svc).await;
    let (t, d) = (a.t.trim(), a.d.trim());
    if !t.is_empty() || !d.is_empty() {
        let keep: Vec<Value> = v["gaps"].as_array().cloned().unwrap_or_default().into_iter().filter(|g| (t.is_empty() || g["t"] == t) && (d.is_empty() || g["d"] == d)).collect();
        v["gaps"] = json!(keep);
    }
    v["age_s"] = json!(age.round());
    v["kept"] = json!(kept);
    v["took_ms"] = json!((t0.elapsed().as_secs_f64() * 10000.0).round() / 10.0);
    Ok(Json(v))
}

async fn gap_one(State(svc): State<Shared>, Query(a): Query<Q>) -> Answer {
    let t0 = Instant::now();
    let (t, d) = (a.t.trim().to_string(), a.d.trim().to_string());
    if t.is_empty() || d.is_empty() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "name a technique (t) and a domain (d)".into()));
    }
    let (list, _, _) = gaps_cached(&svc).await;
    let gap = list["gaps"].as_array().and_then(|a| a.iter().find(|g| g["t"] == t.as_str() && g["d"] == d.as_str()).cloned()).unwrap_or(Value::Null);
    let likes: Vec<String> = gap["like"].as_array().map(|a| a.iter().filter_map(|x| x["id"].as_str().map(String::from)).collect()).unwrap_or_default();
    ctx!(svc, trace, c);
    let mut v = gaps::one(&c, &t, &d, &likes).await;
    v["gap"] = gap;
    done(&trace, v, t0)
}

#[derive(serde::Deserialize)]
struct CiteBody {
    #[serde(default)]
    text: String,
    /// Only papers posted before this time (a draft from the past, or a test).
    #[serde(default)]
    before: Option<String>,
    /// Which lists to fuse: "all" (the default), or one of "meaning" (the
    /// abstract), "cited_with", "near_refs", to compare them.
    #[serde(default)]
    only: Option<String>,
}

/// Missing citations: an abstract and references in, what to cite out.
async fn cite(State(svc): State<Shared>, Json(b): Json<CiteBody>) -> Answer {
    let t0 = Instant::now();
    if b.text.trim().is_empty() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "paste an abstract or references".into()));
    }
    if b.text.len() > 60_000 {
        return Err(ApiError(StatusCode::PAYLOAD_TOO_LARGE, "paste at most 60,000 characters".into()));
    }
    ctx!(svc, trace, c);
    let v = crate::cite::suggest(&c, &b.text, b.before.as_deref(), b.only.as_deref(), 30).await;
    done(&trace, v, t0)
}

async fn summary(State(svc): State<Shared>, Query(a): Query<Q>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let q = a.q.trim();
    if q.len() < 2 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "name a topic".into()));
    }
    let kept = c.gsoft("a kept summary?", "SELECT id FROM ax_answers WHERE id = :k AND answer != ''", json!({"k": ask::key(q)})).await;
    if kept.is_empty() || a.fresh == Some(1) {
        svc.spend(1)?;
    }
    let v = ask::summary(&c, q, &svc.model, a.fresh == Some(1)).await.map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e))?;
    done(&trace, v, t0)
}

fn user_id(u: &str) -> Result<String, ApiError> {
    let u = u.trim();
    if u.len() < 6 || u.len() > 64 || !u.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(ApiError(StatusCode::BAD_REQUEST, "a reader's id is 6 to 64 letters, digits, - or _".into()));
    }
    Ok(u.to_string())
}

async fn for_you(State(svc): State<Shared>, Query(a): Query<Q>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let u = user_id(&a.user)?;
    let v = pages::for_you(&c, &u, if a.area.is_empty() { None } else { Some(a.area.as_str()) }).await;
    done(&trace, v, t0)
}

#[derive(serde::Deserialize)]
struct EventBody {
    user: String,
    paper: String,
    #[serde(default)]
    kind: String,
}

async fn event(State(svc): State<Shared>, Json(b): Json<EventBody>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let u = user_id(&b.user)?;
    let kind = match b.kind.as_str() {
        "save" | "open" | "pdf" => b.kind.clone(),
        _ => "open".into(),
    };
    let v = pages::event(&c, &u, &b.paper, &kind).await?;
    done(&trace, v, t0)
}

async fn library(State(svc): State<Shared>, Query(a): Query<Q>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let u = user_id(&a.user)?;
    let v = pages::library(&c, &u).await;
    done(&trace, v, t0)
}

async fn feeds(State(svc): State<Shared>, Query(a): Query<Q>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let rows = if a.user.is_empty() {
        c.gsoft("the public feeds", "SELECT id, owner, name, updated FROM ax_feeds WHERE public ORDER BY updated DESC LIMIT 50", Value::Null).await
    } else {
        c.gsoft("your feeds", "SELECT id, owner, name, updated, public FROM ax_feeds WHERE owner = :u ORDER BY updated DESC LIMIT 50", json!({"u": user_id(&a.user)?})).await
    };
    done(&trace, json!({"feeds": rows}), t0)
}

async fn example_feed() -> Json<Value> {
    Json(json!({"config": digest::example()}))
}

#[derive(serde::Deserialize)]
struct FeedBody {
    #[serde(default)]
    id: String,
    user: String,
    config: Config,
    #[serde(default)]
    public: bool,
}

async fn save_feed(State(svc): State<Shared>, Json(b): Json<FeedBody>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let u = user_id(&b.user)?;
    let id = if b.id.is_empty() { format!("f{}", crate::live::now().to_string() + &u[..4]) } else { b.id.clone() };
    // A feed is changed by its owner only.
    if !b.id.is_empty() {
        let was = c.gsoft("whose feed it is", "SELECT owner FROM ax_feeds WHERE id = :f", json!({"f": id})).await;
        if was.first().is_some_and(|r| r.s("owner") != u) {
            return Err(ApiError(StatusCode::FORBIDDEN, "that feed is someone else's".into()));
        }
    }
    let now = chrono::Utc::now().to_rfc3339();
    svc.graph.upsert("ax_feeds", vec![json!({"id": id, "owner": u, "name": b.config.name, "config": serde_json::to_string(&b.config).unwrap_or_default(), "created": now, "updated": now, "public": b.public})]).await?;
    done(&trace, json!({"id": id}), t0)
}

async fn load_feed(c: &Ctx<'_>, id: &str) -> Result<(Config, serde_json::Map<String, Value>), ApiError> {
    let row = c.gsoft("the feed", "SELECT * FROM ax_feeds WHERE id = :f", json!({"f": id})).await.into_iter().next().ok_or_else(|| not_found(&format!("feed {id}")))?;
    let cfg: Config = serde_json::from_str(&row.s("config")).unwrap_or_default();
    Ok((cfg, row))
}

async fn feed(State(svc): State<Shared>, Path(id): Path<String>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let (cfg, row) = load_feed(&c, &id).await?;
    let issues = c.gsoft("its issues", "SELECT id, vol, title, since, until, made, papers FROM ax_issues WHERE feed = :f ORDER BY vol DESC LIMIT 100", json!({"f": id})).await;
    done(&trace, json!({"id": id, "owner": row.s("owner"), "public": row.get("public"), "config": cfg, "issues": issues}), t0)
}

#[derive(serde::Deserialize)]
struct PreviewBody {
    config: Config,
    #[serde(default)]
    until: String,
}

/// The papers an issue would hold now, with why: no model is called, so it
/// answers as the knobs move.
async fn preview(State(svc): State<Shared>, Json(b): Json<PreviewBody>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let (since, until) = digest::span(&c, b.config.days, Some(b.until.as_str())).await;
    let ranked = digest::choose(&c, &b.config, &since, &until).await;
    let (main, tools, benches, surveys) = digest::pick(&b.config, &ranked);
    let mut main = main;
    search::trim_abstract(&mut main, 360);
    done(&trace, json!({"since": since, "until": until, "span": digest::span_text(&since, &until), "papers": main, "tools": tools, "benchmarks": benches, "surveys": surveys, "candidates": ranked.len()}), t0)
}

#[derive(serde::Deserialize, Default)]
struct IssueBody {
    #[serde(default)]
    until: String,
    #[serde(default)]
    user: String,
}

/// A figure's image an issue shows, as JSON (its type and its bytes in
/// base64): the view makes it an image itself, so a copied SVG is only ever
/// shown as an image and never opened as a page.
async fn figure(State(svc): State<Shared>, Path(id): Path<String>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let id = id.trim_start_matches('/').to_string();
    let rows = c.gsoft("the figure", "SELECT id, mime, data, w, h, src FROM ax_figures WHERE id = :id", json!({"id": id})).await;
    let Some(r) = rows.first() else {
        return Err(ApiError(StatusCode::NOT_FOUND, format!("no figure {id}")));
    };
    done(&trace, json!({"id": r.s("id"), "mime": r.s("mime"), "data": r.s("data"), "w": r.i("w"), "h": r.i("h"), "src": r.s("src")}), t0)
}

/// Writes an issue in the background; `GET /v1/jobs/:id` says how it goes.
async fn make_issue(State(svc): State<Shared>, Path(id): Path<String>, body: Option<Json<IssueBody>>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let b = body.map(|b| b.0).unwrap_or_default();
    let (cfg, row) = load_feed(&c, &id).await?;
    if svc.token.is_some() && row.s("owner") != b.user && svc.public {
        // In public, only the owner writes a feed's issues.
        let u = user_id(&b.user)?;
        if row.s("owner") != u {
            return Err(ApiError(StatusCode::FORBIDDEN, "only the feed's owner writes its issues".into()));
        }
    }
    svc.spend(cfg.papers.clamp(1, 30))?;
    let job = format!("j{}", crate::live::now() * 1000 + (t0.elapsed().as_nanos() % 1000) as i64);
    svc.jobs.lock().unwrap_or_else(|e| e.into_inner()).insert(job.clone(), json!({"state": "running", "step": "starting", "feed": id}));
    let svc2 = svc.clone();
    let job2 = job.clone();
    tokio::spawn(async move {
        let trace = Trace::new();
        let c = Ctx { db: &svc2.db, g: &svc2.graph, trace: &trace };
        let progress = |s: String| {
            if let Some(j) = svc2.jobs.lock().unwrap_or_else(|e| e.into_inner()).get_mut(&job2) {
                j["step"] = json!(s);
            }
        };
        let until = if b.until.is_empty() { None } else { Some(b.until.as_str()) };
        let got = digest::write_issue(&c, &svc2.pages, &id, &cfg, &svc2.issue_model, until, &progress).await;
        let mut jobs = svc2.jobs.lock().unwrap_or_else(|e| e.into_inner());
        if jobs.len() > 200 {
            jobs.retain(|_, v| v["state"] == "running");
        }
        let v = match got {
            Ok(issue) => json!({"state": "done", "issue": issue["id"], "queries": trace.take()}),
            Err(e) => json!({"state": "failed", "error": e, "queries": trace.take()}),
        };
        jobs.insert(job2, v);
    });
    done(&trace, json!({"job": job}), t0)
}

async fn job(State(svc): State<Shared>, Path(id): Path<String>) -> Answer {
    let v = svc.jobs.lock().unwrap_or_else(|e| e.into_inner()).get(&id).cloned().ok_or_else(|| not_found(&format!("job {id}")))?;
    Ok(Json(v))
}

async fn issue(State(svc): State<Shared>, Path(id): Path<String>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let row = c.gsoft("the issue", "SELECT * FROM ax_issues WHERE id = :i", json!({"i": id})).await.into_iter().next().ok_or_else(|| not_found(&format!("issue {id}")))?;
    let body: Value = serde_json::from_str(&row.s("body")).unwrap_or(Value::Null);
    done(&trace, json!({"id": id, "title": row.s("title"), "made": row.s("made"), "issue": body}), t0)
}

async fn rss(State(svc): State<Shared>, Path(id): Path<String>, headers: HeaderMap) -> Result<Response, ApiError> {
    let trace = Trace::new();
    let c = Ctx { db: &svc.db, g: &svc.graph, trace: &trace };
    let id = id.trim_end_matches(".xml").to_string();
    let (cfg, _) = load_feed(&c, &id).await?;
    let issues = c.gsoft("its issues", "SELECT id, title, made, body FROM ax_issues WHERE feed = :f ORDER BY vol DESC LIMIT 20", json!({"f": id})).await;
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("localhost");
    let proto = headers.get("x-forwarded-proto").and_then(|v| v.to_str().ok()).unwrap_or("http");
    let body = digest::rss(&id, &cfg, &issues, &format!("{proto}://{host}"));
    Ok(([(header::CONTENT_TYPE, "application/rss+xml; charset=utf-8")], body).into_response())
}

// ------------------------------------------------------- standing questions

#[derive(serde::Deserialize, Default)]
struct WatchBody {
    #[serde(default)]
    user: String,
    #[serde(default)]
    question: String,
}

/// Papers' rows for the matches of a question, newest first.
async fn with_papers(c: &Ctx<'_>, matches: Vec<crate::strata::Row>) -> Vec<Value> {
    let ids: Vec<String> = matches.iter().map(|r| r.s("paper")).collect();
    if ids.is_empty() {
        return Vec::new();
    }
    let rows = c.soft("the papers", &format!("SELECT {} FROM ax_papers WHERE id IN (:ids)", search::COLS), json!({"ids": ids})).await;
    let by: HashMap<String, crate::strata::Row> = rows.into_iter().map(|r| (r.s("id"), r)).collect();
    matches.into_iter().filter_map(|m| by.get(&m.s("paper")).map(|p| json!({"paper": p, "fit": m.get("fit"), "why": m.s("why"), "at": m.s("at")}))).collect()
}

async fn watches(State(svc): State<Shared>, Query(a): Query<Q>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let rows = if a.user.is_empty() { Vec::new() } else { c.gsoft("your questions", "SELECT id, question, created, checked, read, fit FROM ax_watches WHERE owner = :u ORDER BY created DESC LIMIT 50", json!({"u": user_id(&a.user)?})).await };
    done(&trace, json!({"watches": rows, "per_reader": watch::PER_READER}), t0)
}

async fn add_watch(State(svc): State<Shared>, Json(b): Json<WatchBody>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let u = user_id(&b.user)?;
    let q = b.question.split_whitespace().collect::<Vec<_>>().join(" ");
    if q.chars().count() < 8 || q.chars().count() > 300 {
        return Err(ApiError(StatusCode::BAD_REQUEST, "a question is 8 to 300 characters".into()));
    }
    let (mine, all) = tokio::join!(
        c.gsoft("your questions", "SELECT COUNT(*) AS n FROM ax_watches WHERE owner = :u", json!({"u": u})),
        c.gsoft("all questions", "SELECT COUNT(*) AS n FROM ax_watches", Value::Null),
    );
    if mine.first().map(|r| r.i("n")).unwrap_or(0) >= watch::PER_READER as i64 {
        return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, format!("a reader keeps {} questions at most: drop one first", watch::PER_READER)));
    }
    if all.first().map(|r| r.i("n")).unwrap_or(0) >= watch::IN_ALL as i64 {
        return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, "this explorer keeps as many questions as it can check: try again later".into()));
    }
    let id = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (&u, &q, crate::live::now(), t0.elapsed().as_nanos()).hash(&mut h);
        format!("w{:012x}", h.finish() & 0xffff_ffff_ffff)
    };
    let now = chrono::Utc::now();
    // The first check reads the closest papers of the last month, so the
    // question has its answers at once.
    let since = (now - chrono::Duration::days(30)).to_rfc3339();
    let got = watch::check(&c, &svc.model, &id, &q, &since, 20, &|| svc.spend(1).map_err(|e| e.1)).await;
    let (read, fit) = match &got {
        Ok(k) => (k.read as i64, k.fit as i64),
        Err(_) => (0, 0),
    };
    svc.graph.upsert("ax_watches", vec![json!({"id": id, "owner": u, "question": q, "created": now.to_rfc3339(), "checked": now.to_rfc3339(), "read": read, "fit": fit})]).await?;
    done(&trace, json!({"id": id, "read": read, "fit": fit, "error": got.err()}), t0)
}

async fn watch_one(State(svc): State<Shared>, Path(id): Path<String>, Query(a): Query<Q>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let row = c.gsoft("the question", "SELECT id, owner, question, created, checked, read, fit FROM ax_watches WHERE id = :w", json!({"w": id})).await.into_iter().next().ok_or_else(|| not_found(&format!("question {id}")))?;
    let (fits, misses) = tokio::join!(
        c.gsoft("the papers that answer it", "SELECT paper, fit, why, at, created FROM ax_matches WHERE watch = :w AND fit ORDER BY created DESC LIMIT 100", json!({"w": id})),
        c.gsoft("the papers read and set aside", "SELECT paper, fit, why, at, created FROM ax_matches WHERE watch = :w AND NOT fit ORDER BY created DESC LIMIT 40", json!({"w": id})),
    );
    let (fits, misses) = tokio::join!(with_papers(&c, fits), with_papers(&c, misses));
    let mine = !a.user.is_empty() && row.s("owner") == a.user;
    let mut w = row.clone();
    w.remove("owner");
    done(&trace, json!({"watch": w, "mine": mine, "fits": fits, "misses": misses}), t0)
}

async fn drop_watch(State(svc): State<Shared>, Path(id): Path<String>, Json(b): Json<WatchBody>) -> Answer {
    let t0 = Instant::now();
    ctx!(svc, trace, c);
    let u = user_id(&b.user)?;
    let row = c.gsoft("whose question it is", "SELECT owner FROM ax_watches WHERE id = :w", json!({"w": id})).await.into_iter().next().ok_or_else(|| not_found(&format!("question {id}")))?;
    if row.s("owner") != u {
        return Err(ApiError(StatusCode::FORBIDDEN, "that question is someone else's".into()));
    }
    svc.graph.sql_params("DELETE FROM ax_watches WHERE id = :w", json!({"w": id})).await?;
    let _ = svc.graph.sql_params("DELETE FROM ax_matches WHERE watch = :w", json!({"w": id})).await;
    done(&trace, json!({"dropped": id}), t0)
}

async fn watch_rss(State(svc): State<Shared>, Path(id): Path<String>, headers: HeaderMap) -> Result<Response, ApiError> {
    let trace = Trace::new();
    let c = Ctx { db: &svc.db, g: &svc.graph, trace: &trace };
    let id = id.trim_end_matches(".xml").to_string();
    let row = c.gsoft("the question", "SELECT question FROM ax_watches WHERE id = :w", json!({"w": id})).await.into_iter().next().ok_or_else(|| not_found(&format!("question {id}")))?;
    let fits = c.gsoft("the papers that answer it", "SELECT paper, why, created FROM ax_matches WHERE watch = :w AND fit ORDER BY created DESC LIMIT 50", json!({"w": id})).await;
    let ids: Vec<String> = fits.iter().map(|r| r.s("paper")).collect();
    let papers: HashMap<String, crate::strata::Row> = if ids.is_empty() { HashMap::new() } else { c.soft("the papers", "SELECT id, title, authors FROM ax_papers WHERE id IN (:ids)", json!({"ids": ids})).await.into_iter().map(|r| (r.s("id"), r)).collect() };
    let items: Vec<crate::strata::Row> = fits.into_iter().map(|mut m| {
        if let Some(p) = papers.get(&m.s("paper")) {
            m.insert("title".into(), json!(p.s("title")));
            m.insert("authors".into(), json!(p.s("authors")));
        }
        m
    }).collect();
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("localhost");
    let proto = headers.get("x-forwarded-proto").and_then(|v| v.to_str().ok()).unwrap_or("http");
    let body = watch::rss(&id, &row.s("question"), &items, &format!("{proto}://{host}"));
    Ok(([(header::CONTENT_TYPE, "application/rss+xml; charset=utf-8")], body).into_response())
}

/// Checks every standing question each hour against the papers of the
/// last week (the live feed brings them in rounds, and a paper can arrive
/// days after the date it carries), within the day's allowance.
pub async fn watch_rounds(svc: Shared) {
    tokio::time::sleep(Duration::from_secs(120)).await;
    loop {
        let trace = Trace::new();
        let c = Ctx { db: &svc.db, g: &svc.graph, trace: &trace };
        let all = c.gsoft("the questions", &format!("SELECT id, owner, question, created FROM ax_watches ORDER BY checked ASC LIMIT {}", watch::IN_ALL), Value::Null).await;
        let since = (chrono::Utc::now() - chrono::Duration::days(7)).to_rfc3339();
        let (mut read, mut fit) = (0, 0);
        for w in &all {
            let id = w.s("id");
            match watch::check(&c, &svc.model, &id, &w.s("question"), &since, watch::CLOSEST, &|| svc.spend(1).map_err(|e| e.1)).await {
                Ok(k) => {
                    read += k.read;
                    fit += k.fit;
                    let (r, f) = tokio::join!(
                        c.gsoft("papers read for it", "SELECT COUNT(*) AS n FROM ax_matches WHERE watch = :w", json!({"w": id})),
                        c.gsoft("papers that answer it", "SELECT COUNT(*) AS n FROM ax_matches WHERE watch = :w AND fit", json!({"w": id})),
                    );
                    // An upsert writes the whole row.
                    let _ = svc.graph.upsert("ax_watches", vec![json!({"id": id, "owner": w.s("owner"), "question": w.s("question"), "created": w.s("created"), "checked": chrono::Utc::now().to_rfc3339(), "read": r.first().map(|x| x.i("n")).unwrap_or(0), "fit": f.first().map(|x| x.i("n")).unwrap_or(0)})]).await;
                }
                Err(e) => {
                    tracing::warn!("standing question {id}: {e}");
                    if e.contains("today's") {
                        break;
                    }
                }
            }
        }
        if !all.is_empty() {
            eprintln!("chloedb-arxiv: {} standing questions checked, {read} papers read, {fit} answer one", all.len());
        }
        tokio::time::sleep(Duration::from_secs(3600)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_a_public_reader_may_do() {
        assert!(public_ok(&Method::GET, "/v1/gaps"));
        assert!(public_ok(&Method::POST, "/v1/cite"));
        assert!(public_ok(&Method::POST, "/v1/feeds/f1/issue"));
        assert!(public_ok(&Method::POST, "/v1/watches"));
        assert!(public_ok(&Method::POST, "/v1/watches/w1/delete"));
        assert!(!public_ok(&Method::POST, "/v1/watches/w1"));
        assert!(!public_ok(&Method::POST, "/v1/sql"));
        assert!(!public_ok(&Method::DELETE, "/v1/feeds/f1"));
    }
}
