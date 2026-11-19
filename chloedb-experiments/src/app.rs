//! `chloedb-experiments serve`: the Experiments app of ChloeDB's command
//! center. chloedb-server shows it (`--app experiments=URL`): it loads the
//! page from here (`/app.json`, `/ui/view.js`) and sends the app's token
//! with each call, which every route but `/healthz` needs.
//!
//! Its sites and experiments are rows of the server it is given
//! (`exp_sites`, `exp_experiments`); an experiment runs in the background,
//! one at a time, and its report is kept with it. The last few runs stay
//! in memory, to cut by two attributes at once.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{Path, Query, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::ab::{self, Progress, RunData, Spec};
use crate::click::Model;
use crate::live::{self, Brain, Live, LiveSpec};
use crate::policy::Policy;
use crate::server::{lit, Server};
use crate::site::{self, Site, Tables};
use crate::judge::Judge;
use crate::personas::{self, PersonaSet};
use crate::source::Source;
use crate::study::{self, Study, StudySpec};
use crate::users::{AGES, ATTRIBUTES, COUNTRIES, DEVICES, GENDERS, TENURES};

pub const VIEW: &str = include_str!("../ui/view.js");

/// Two arms and a split between them.
pub const ICON: &str = r#"<path d="M9 3h6"/><path d="M10 3v6.5L4.6 18.4A1.7 1.7 0 0 0 6 21h12a1.7 1.7 0 0 0 1.4-2.6L14 9.5V3"/><path d="M7.5 15h9"/>"#;

const SITES: &str = "exp_sites";
const EXPERIMENTS: &str = "exp_experiments";
const PERSONAS: &str = "exp_personas";
const STUDIES: &str = "exp_studies";
/// Runs kept in memory for cuts by two attributes.
const KEPT: usize = 3;

pub struct Opts {
    pub token: Option<String>,
    /// BEIR collections to load as sites at the start: (directory, name).
    pub beir: Vec<(PathBuf, String)>,
    /// The key AI agents call Claude with (ANTHROPIC_API_KEY); without
    /// one, live runs offer only scripted agents.
    pub anthropic_key: Option<String>,
    /// The Messages API's address (default https://api.anthropic.com).
    pub anthropic_base: Option<String>,
    /// Real sites to study ([`crate::source`]).
    pub sources: Vec<Source>,
}

pub struct App {
    pub server: Server,
    pub token: Option<String>,
    sites: tokio::sync::Mutex<HashMap<String, Arc<Site>>>,
    loading: Mutex<HashMap<String, String>>,
    progress: Mutex<HashMap<String, Arc<Progress>>>,
    /// Checks under way ([`crate::repeat`]), by experiment.
    checks: Mutex<HashMap<String, Arc<Progress>>>,
    kept: Mutex<VecDeque<(String, Arc<RunData>)>>,
    turn: tokio::sync::Semaphore,
    /// The live runs in memory, newest first: the one running, and a few that ended.
    lives: Mutex<VecDeque<Arc<Live>>>,
    anthropic_key: Option<String>,
    anthropic_base: String,
    sources: Vec<Arc<Source>>,
    /// Persona sets being written, by id: what it is doing, or why it failed.
    writing: Mutex<HashMap<String, Value>>,
    /// Studies in memory, newest first: the one running, and a few that ended.
    studies: Mutex<VecDeque<Arc<Study>>>,
}

type Shared = Arc<App>;

pub struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error": self.1}))).into_response()
    }
}

fn bad(m: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, m.into())
}

fn upstream(m: String) -> ApiError {
    ApiError(StatusCode::BAD_GATEWAY, format!("ChloeDB: {m}"))
}

type Answer = Result<Json<Value>, ApiError>;

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// Rankings offered for a site of BEIR's shape (`title`, `body`).
pub fn beir_rankings() -> Vec<(String, String)> {
    let b = |f: &str| format!("bm25({f}, :t, syntax => 'plain')");
    vec![
        ("Body, title a little".into(), format!("{} + 0.3 * {}", b("body"), b("title"))),
        ("Title first".into(), format!("2 * {} + {}", b("title"), b("body"))),
        ("Body only".into(), b("body")),
        ("Title only".into(), b("title")),
        ("Body and title alike".into(), format!("{} + {}", b("body"), b("title"))),
    ]
}

const ABOUT: &[(&str, &str)] = &[
    ("nfcorpus", "Nutrition and medicine: 3,600 articles of PubMed, asked 323 questions people put to NutritionFacts.org, graded by its links (BEIR's NFCorpus)."),
    ("scifact", "Science claims: 5,200 abstracts, asked 300 claims written by experts, each judged against the abstracts that support or refute it (BEIR's SciFact)."),
    ("arguana", "Arguments: 8,700 counter-arguments from idebate.org, each asked by the argument it answers (BEIR's ArguAna)."),
];

impl App {
    async fn site(&self, name: &str) -> Result<Arc<Site>, ApiError> {
        if let Some(s) = self.sites.lock().await.get(name) {
            return Ok(s.clone());
        }
        let t = self.tables(name).await?;
        let s = Arc::new(site::from_tables(&self.server, &t).await.map_err(bad)?);
        self.sites.lock().await.insert(name.to_string(), s.clone());
        Ok(s)
    }

    async fn tables(&self, name: &str) -> Result<Tables, ApiError> {
        let a = self.server.query(&format!("SELECT doc FROM {SITES} WHERE name = {}", lit(name)), json!({}), None).await.map_err(upstream)?;
        let doc = a["rows"][0][0].as_str().ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("no site {name}")))?;
        serde_json::from_str(doc).map_err(|e| upstream(format!("site {name}: {e}")))
    }

    async fn save_site(&self, t: &Tables) -> Result<(), String> {
        self.server.write(vec![json!({"op": "upsert", "table": SITES, "doc": {"name": t.name, "doc": serde_json::to_string(t).unwrap_or_default(), "created": now_ms()}})]).await.map(|_| ())
    }

    async fn save(&self, id: &str, row: Value) -> Result<(), String> {
        let mut doc = row;
        doc["id"] = json!(id);
        self.server.write(vec![json!({"op": "upsert", "table": EXPERIMENTS, "doc": doc})]).await.map(|_| ())
    }

    async fn row(&self, id: &str) -> Result<Value, ApiError> {
        let a = self.server.query(&format!("SELECT id, name, site, created, status, spec, summary, report, error, seconds FROM {EXPERIMENTS} WHERE id = {}", lit(id)), json!({}), None).await.map_err(upstream)?;
        let r = a["rows"].get(0).cloned().ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("no experiment {id}")))?;
        let parse = |v: &Value| v.as_str().and_then(|s| serde_json::from_str::<Value>(s).ok()).unwrap_or(Value::Null);
        Ok(json!({"id": r[0], "name": r[1], "site": r[2], "created": r[3], "status": r[4], "spec": parse(&r[5]), "summary": parse(&r[6]), "report": parse(&r[7]), "error": r[8], "seconds": r[9]}))
    }

    fn keep_run(&self, id: &str, d: Arc<RunData>) {
        let mut k = self.kept.lock().unwrap_or_else(|e| e.into_inner());
        k.retain(|(x, _)| x != id);
        k.push_front((id.to_string(), d));
        k.truncate(KEPT);
    }

    fn live(&self, id: &str) -> Option<Arc<Live>> {
        self.lives.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|l| l.id == id).cloned()
    }

    fn kept_run(&self, id: &str) -> Option<Arc<RunData>> {
        self.kept.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(x, _)| x == id).map(|(_, d)| d.clone())
    }
}

pub async fn serve(server: Server, listener: tokio::net::TcpListener, o: Opts) -> Result<(), String> {
    let app = Arc::new(App {
        server,
        token: o.token.filter(|t| !t.is_empty()),
        sites: tokio::sync::Mutex::new(HashMap::new()),
        loading: Mutex::new(HashMap::new()),
        progress: Mutex::new(HashMap::new()),
        checks: Mutex::new(HashMap::new()),
        kept: Mutex::new(VecDeque::new()),
        turn: tokio::sync::Semaphore::new(1),
        lives: Mutex::new(VecDeque::new()),
        anthropic_key: o.anthropic_key.filter(|k| !k.trim().is_empty()),
        anthropic_base: o.anthropic_base.filter(|b| !b.trim().is_empty()).unwrap_or_else(|| "https://api.anthropic.com".into()),
        sources: o.sources.into_iter().map(Arc::new).collect(),
        writing: Mutex::new(HashMap::new()),
        studies: Mutex::new(VecDeque::new()),
    });
    {
        let app = app.clone();
        tokio::spawn(async move { start(app, o.beir).await });
    }
    eprintln!("chloedb-experiments: serves at http://{} on {} (token {})", listener.local_addr().map(|a| a.to_string()).unwrap_or_default(), app.server.url, if app.token.is_some() { "needed" } else { "not needed" });
    axum::serve(listener, router(app))
        .with_graceful_shutdown(async {
            let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("a signal handler");
            tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
        })
        .await
        .map_err(|e| e.to_string())
}

/// The tables, the sites to load, and the runs a restart cut short.
async fn start(app: Shared, beir: Vec<(PathBuf, String)>) {
    let s = &app.server;
    for _ in 0..60 {
        if s.health().await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    let tables = [
        format!("CREATE TABLE IF NOT EXISTS {SITES} (name KEYWORD PRIMARY KEY, doc TEXT OPTIONS(indexed = false), created INT)"),
        format!("CREATE TABLE IF NOT EXISTS {EXPERIMENTS} (id KEYWORD PRIMARY KEY, name TEXT, site KEYWORD, created INT, status KEYWORD, spec TEXT OPTIONS(indexed = false), summary TEXT OPTIONS(indexed = false), report TEXT OPTIONS(indexed = false), error TEXT OPTIONS(indexed = false), seconds FLOAT)"),
        format!("CREATE TABLE IF NOT EXISTS {PERSONAS} (id KEYWORD PRIMARY KEY, source KEYWORD, created INT, doc TEXT OPTIONS(indexed = false))"),
        format!("CREATE TABLE IF NOT EXISTS {STUDIES} (id KEYWORD PRIMARY KEY, name TEXT, created INT, status KEYWORD, state TEXT OPTIONS(indexed = false), sessions TEXT OPTIONS(indexed = false))"),
    ];
    for t in &tables {
        if let Err(e) = s.sql(t, json!({})).await {
            eprintln!("chloedb-experiments: {e}");
        }
    }
    if let Err(e) = Judge::create_table(s).await {
        eprintln!("chloedb-experiments: {e}");
    }
    // Studies a restart cut short keep what they did.
    if let Ok(a) = s.query(&format!("SELECT id, state FROM {STUDIES} WHERE status = 'running'"), json!({}), None).await {
        for r in a["rows"].as_array().into_iter().flatten() {
            if let (Some(id), Some(v)) = (r[0].as_str(), r[1].as_str().and_then(|v| serde_json::from_str::<Value>(v).ok())) {
                let mut v = v;
                v["status"] = json!("stopped");
                v["error"] = json!("the app restarted while it ran");
                let _ = s.write(vec![json!({"op": "update", "table": STUDIES, "key": id, "set": {"status": "stopped", "state": v.to_string()}})]).await;
            }
        }
    }
    // Runs left queued or running by a process that ended.
    if let Ok(a) = s.query(&format!("SELECT id FROM {EXPERIMENTS} WHERE status IN ('queued', 'running', 'live')"), json!({}), None).await {
        for r in a["rows"].as_array().into_iter().flatten() {
            if let Some(id) = r[0].as_str() {
                if let Ok(mut row) = app.row(id).await {
                    for k in ["spec", "summary", "report"] {
                        row[k] = json!(row[k].to_string());
                    }
                    row["status"] = json!("failed");
                    row["error"] = json!("the app restarted while it ran: run it again");
                    let _ = app.save(id, row).await;
                }
            }
        }
    }
    for (dir, name) in beir {
        app.loading.lock().unwrap_or_else(|e| e.into_inner()).insert(name.clone(), "loading its rows, queries and grades".into());
        let t0 = std::time::Instant::now();
        let r = async {
            let mut t = site::keep_beir(s, &dir, &name, 7).await?;
            t.about = ABOUT.iter().find(|(n, _)| *n == name).map(|(_, a)| a.to_string()).unwrap_or_else(|| format!("BEIR's {name}"));
            t.rankings = beir_rankings();
            app.save_site(&t).await?;
            Ok::<_, String>(())
        }
        .await;
        match r {
            Ok(()) => {
                eprintln!("chloedb-experiments: site {name} ready ({:.1} s)", t0.elapsed().as_secs_f64());
                app.loading.lock().unwrap_or_else(|e| e.into_inner()).remove(&name);
            }
            Err(e) => {
                eprintln!("chloedb-experiments: site {name}: {e}");
                app.loading.lock().unwrap_or_else(|e| e.into_inner()).insert(name.clone(), format!("failed: {e}"));
            }
        }
    }
}

pub fn router(app: Shared) -> Router {
    Router::new()
        .route("/app.json", get(manifest))
        .route("/ui/view.js", get(view))
        .route("/v1/meta", get(meta))
        .route("/v1/sites", get(sites).post(add_site))
        .route("/v1/sites/:name", get(one_site))
        .route("/v1/preview", post(preview))
        .route("/v1/experiments", get(experiments).post(create))
        .route("/v1/experiments/:id", get(experiment).delete(remove))
        .route("/v1/experiments/:id/cut", post(cut))
        .route("/v1/experiments/:id/check", post(check_run))
        .route("/v1/live", get(lives).post(create_live))
        .route("/v1/live/:id", get(one_live))
        .route("/v1/live/:id/stop", post(stop_live))
        .route("/v1/live/:id/rate", post(rate_live))
        .route("/v1/live/:id/try", post(try_search))
        .route("/v1/live/:id/try/open", post(try_open))
        .route("/v1/live/:id/try/done", post(try_done))
        .route("/v1/live/:id/cohorts", post(live_cohorts))
        .route("/v1/real", get(real_sites))
        .route("/v1/real/:source/search", post(real_search))
        .route("/v1/real/:source/item/*id", get(real_item))
        .route("/v1/personas", get(persona_sets).post(write_personas))
        .route("/v1/personas/:id", get(persona_set).delete(drop_personas))
        .route("/v1/studies", get(studies).post(create_study))
        .route("/v1/studies/:id", get(one_study))
        .route("/v1/studies/:id/sessions", get(study_sessions))
        .route("/v1/studies/:id/stop", post(stop_study))
        .layer(middleware::from_fn_with_state(app.clone(), guard))
        .route("/healthz", get(|| async { "ok" }))
        .with_state(app)
}

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

async fn guard(State(app): State<Shared>, req: Request, next: Next) -> Response {
    let headers = req.headers();
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        let host = headers.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");
        if !origin.split("://").nth(1).is_some_and(|o| o == host) {
            return ApiError(StatusCode::FORBIDDEN, "requests from other sites are refused".into()).into_response();
        }
    }
    let authed = match &app.token {
        None => true,
        Some(t) => headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")).is_some_and(|got| constant_eq(got.as_bytes(), t.as_bytes())),
    };
    if !authed {
        return ApiError(StatusCode::UNAUTHORIZED, "this app needs its token: open it in ChloeDB's command center (Apps → Experiments)".into()).into_response();
    }
    next.run(req).await
}

async fn manifest() -> Json<Value> {
    Json(json!({
        "name": "experiments",
        "title": "Experiments",
        "description": "A/B tests of rankings on simulated users: every metric with its interval, p value and power, cohorts, always-valid peeking, CUPED, and what an exploring log says before you run one",
        "icon_svg": ICON,
        "view": "ui/view.js",
        "version": env!("CARGO_PKG_VERSION"),
        "maturity": "demo",
    }))
}

fn etag(text: &str) -> String {
    let h = text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3));
    format!("\"{h:016x}\"")
}

async fn view(headers: HeaderMap) -> Response {
    let body: String = match std::env::var_os("CHLOEDB_EXPERIMENTS_UI_DIR") {
        Some(d) => std::fs::read_to_string(std::path::Path::new(&d).join("view.js")).unwrap_or_else(|_| VIEW.to_string()),
        None => VIEW.to_string(),
    };
    let tag = etag(&body);
    let fresh = headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()).is_some_and(|v| v.split(',').any(|t| t.trim().trim_start_matches("W/") == tag));
    let head = [(header::CONTENT_TYPE, "text/javascript; charset=utf-8".to_string()), (header::CACHE_CONTROL, "no-cache".to_string()), (header::ETAG, tag)];
    if fresh {
        return (StatusCode::NOT_MODIFIED, head).into_response();
    }
    (head, body).into_response()
}

/// What the builder offers: click models, metrics, cohorts, defaults and
/// limits.
async fn meta(State(app): State<Shared>) -> Json<Value> {
    let d: Spec = serde_json::from_value(json!({"name": "", "site": "", "arms": []})).expect("the defaults");
    let live_defaults: LiveSpec = serde_json::from_value(json!({"name": "", "site": "", "arms": []})).expect("the defaults");
    let models: Vec<Value> = [
        (Model::Pbm, "Position-based: a row is seen by its position alone, clicked if it looks right."),
        (Model::Cascade, "Cascade: people read down, click the first row that looks right and stop."),
        (Model::Dcm, "Dependent clicks: as the cascade, but after a click they may read on."),
        (Model::Dbn, "Dynamic Bayesian network: they read down, click what looks right, and stop once one satisfies (or give up). The default: the closest to real logs."),
        (Model::Ubm, "User browsing: how likely a row is seen depends on how far it is below the last click."),
        (Model::Trust, "Trust bias: rows higher up look better than they are."),
    ]
    .iter()
    .map(|(m, about)| json!({"name": m.name(), "about": about}))
    .collect();
    Json(json!({
        "models": models,
        "metrics": ab::METRICS,
        "attributes": ATTRIBUTES,
        "values": {"age": AGES, "gender": GENDERS, "country": COUNTRIES, "device": DEVICES, "tenure": TENURES},
        "defaults": d,
        "limits": {"users": ab::MAX_USERS, "fast": ab::MAX_FAST, "served": ab::MAX_SERVED, "arms": ab::MAX_ARMS},
        "live": {
            "defaults": live_defaults,
            "limits": {"users": live::MAX_USERS, "rate": live::MAX_RATE, "minutes": live::MAX_MINUTES, "agents": live::MAX_AGENTS, "budget": live::MAX_BUDGET, "steps": live::AGENT_STEPS},
            "models": live::AGENT_MODELS.iter().map(|(id, name, pin, pout, fallbacks)| json!({"id": id, "name": name, "input": pin, "output": pout, "fallbacks": fallbacks})).collect::<Vec<_>>(),
            "claude": app.anthropic_key.is_some(),
        },
        "real": {
            "sources": app.sources.iter().map(|s| s.name.clone()).collect::<Vec<_>>(),
            "limits": {"arms": study::MAX_ARMS, "workers": study::MAX_WORKERS, "sessions": study::MAX_SESSIONS, "budget": study::MAX_BUDGET, "personas": personas::MAX_PERSONAS, "needs": personas::MAX_NEEDS, "steps": live::AGENT_STEPS},
        },
        "policies": [
            {"name": "Softmax over the top 10", "explore": {"policy": "softmax", "tau": 1.0, "top": 10, "by": "rank"}},
            {"name": "Epsilon 0.3 over the top 10", "explore": {"policy": "epsilon", "epsilon": 0.3, "top": 10}},
            {"name": "Shuffle the top 10", "explore": {"policy": "shuffle", "top": 10}},
            {"name": "Swap one pair", "explore": {"policy": "swap", "top": 10}},
        ],
    }))
}

async fn sites(State(app): State<Shared>) -> Answer {
    let a = app.server.query(&format!("SELECT name, doc FROM {SITES} ORDER BY name LIMIT 1000"), json!({}), None).await.map_err(upstream)?;
    let mut out: Vec<Value> = Vec::new();
    for r in a["rows"].as_array().into_iter().flatten() {
        let Some(t) = r[1].as_str().and_then(|d| serde_json::from_str::<Tables>(d).ok()) else { continue };
        let site = app.sites.lock().await.get(&t.name).cloned();
        out.push(json!({"name": t.name, "table": t.table, "queries_table": t.queries, "judgments_table": t.judgments, "about": t.about, "rankings": t.rankings, "queries": site.map(|s| s.queries.len())}));
    }
    let loading = app.loading.lock().unwrap_or_else(|e| e.into_inner()).clone();
    Ok(Json(json!({"sites": out, "loading": loading})))
}

async fn one_site(State(app): State<Shared>, Path(name): Path<String>) -> Answer {
    let t = app.tables(&name).await?;
    let s = app.site(&name).await?;
    let shares = s.shares();
    let mut top: Vec<(usize, f64)> = shares.iter().copied().enumerate().collect();
    top.sort_by(|a, b| b.1.total_cmp(&a.1));
    let judged: usize = s.grades.iter().map(|g| g.len()).sum();
    Ok(Json(json!({
        "name": t.name, "table": t.table, "about": t.about, "rankings": t.rankings,
        "queries": s.queries.len(), "judged": judged, "max_grade": s.max_grade,
        "top": top.iter().take(12).map(|&(q, w)| json!({"id": s.queries[q].id, "text": s.queries[q].text, "share": w, "judged": s.grades[q].len()})).collect::<Vec<_>>(),
    })))
}

async fn add_site(State(app): State<Shared>, Json(t): Json<Tables>) -> Answer {
    t.check().map_err(bad)?;
    let s = site::from_tables(&app.server, &t).await.map_err(bad)?;
    for (title, rank) in &t.rankings {
        if rank.contains(';') {
            return Err(bad(format!("ranking {title} is an expression, not statements")));
        }
    }
    app.save_site(&t).await.map_err(upstream)?;
    let n = s.queries.len();
    app.sites.lock().await.insert(t.name.clone(), Arc::new(s));
    Ok(Json(json!({"name": t.name, "queries": n})))
}

/// What some rankings show for a site's busiest queries, row by row, with
/// each row's grade: a look at the arms before they run.
async fn preview(State(app): State<Shared>, Json(b): Json<Value>) -> Answer {
    let name = b["site"].as_str().ok_or_else(|| bad("preview needs a site"))?;
    let ranks: Vec<String> = b["ranks"].as_array().into_iter().flatten().filter_map(|r| r.as_str().map(str::to_string)).collect();
    if ranks.is_empty() || ranks.len() > ab::MAX_ARMS || ranks.iter().any(|r| r.contains(';') || r.trim().is_empty()) {
        return Err(bad(format!("preview takes 1 to {} rankings, each an ORDER BY expression", ab::MAX_ARMS)));
    }
    let thens: Vec<&str> = (0..ranks.len()).map(|i| b["thens"][i].as_str().unwrap_or("")).collect();
    let limit = b["limit"].as_u64().unwrap_or(5).clamp(1, 10) as usize;
    let n = b["queries"].as_u64().unwrap_or(4).clamp(1, 12) as usize;
    let s = app.site(name).await?;
    let shares = s.shares();
    let mut order: Vec<usize> = (0..s.queries.len()).collect();
    order.sort_by(|&a, &b| shares[b].total_cmp(&shares[a]));
    let picked: Vec<usize> = match b["query"].as_str() {
        Some(id) => s.queries.iter().position(|q| q.id == id).into_iter().collect(),
        None => order.into_iter().take(n).collect(),
    };
    let mut out = Vec::new();
    let mut all_keys = Vec::new();
    for &q in &picked {
        let mut per = Vec::new();
        for (r, then) in ranks.iter().zip(&thens) {
            let sql = format!("SELECT id FROM {} ORDER BY {r} DESC LIMIT {limit}", s.table);
            let a = app.server.query(&sql, json!({"t": s.queries[q].text}), None).await.map_err(|e| bad(format!("ranking {r}: {e}")))?;
            let mut keys: Vec<String> = a["rows"].as_array().into_iter().flatten().map(|x| crate::sim::key_text(&x[0])).collect();
            ab::then_apply(then, &mut keys);
            all_keys.extend(keys.iter().cloned());
            per.push((keys, a["took_ms"].as_f64()));
        }
        out.push((q, per));
    }
    all_keys.sort();
    all_keys.dedup();
    let titles = site::titles(&app.server, &s.table, &all_keys).await.map_err(upstream)?;
    let queries: Vec<Value> = out
        .into_iter()
        .map(|(q, per)| {
            json!({
                "id": s.queries[q].id, "text": s.queries[q].text, "share": shares[q], "judged": s.grades[q].len(),
                "lists": per.into_iter().map(|(keys, took)| json!({"took_ms": took, "rows": keys.iter().map(|k| json!({"key": k, "title": titles.get(k), "grade": s.grades[q].get(k).copied().unwrap_or(0)})).collect::<Vec<_>>()})).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(Json(json!({"site": name, "max_grade": s.max_grade, "queries": queries})))
}

async fn experiments(State(app): State<Shared>) -> Answer {
    let a = app.server.query(&format!("SELECT id, name, site, created, status, summary, error, seconds FROM {EXPERIMENTS} ORDER BY created DESC LIMIT 200"), json!({}), None).await.map_err(upstream)?;
    let progress = app.progress.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let rows: Vec<Value> = a["rows"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| {
            let id = r[0].as_str().unwrap_or_default();
            let live = (r[4] == "live").then(|| app.live(id)).flatten().map(|l| {
                let v = l.view(Some(u64::MAX), u64::MAX);
                json!({"status": v["status"], "stage": v["stage"], "elapsed": v["elapsed"], "limit_s": v["limit_s"], "searches": v["searches"], "agents": l.spec.agents})
            });
            json!({"id": r[0], "name": r[1], "site": r[2], "created": r[3], "status": r[4], "summary": r[5].as_str().and_then(|s| serde_json::from_str::<Value>(s).ok()), "error": r[6], "seconds": r[7], "progress": progress.get(id).map(|p| progress_of(p)), "live": live})
        })
        .collect();
    Ok(Json(json!({"experiments": rows})))
}

fn progress_of(p: &Progress) -> Value {
    let (done, total) = (p.done.load(Ordering::Relaxed), p.total.load(Ordering::Relaxed));
    json!({"done": done, "total": total, "share": if total > 0 { done as f64 / total as f64 } else { 0.0 }, "stage": p.stage.lock().unwrap_or_else(|e| e.into_inner()).clone()})
}

async fn experiment(State(app): State<Shared>, Path(id): Path<String>) -> Answer {
    let mut r = app.row(&id).await?;
    if let Some(p) = app.progress.lock().unwrap_or_else(|e| e.into_inner()).get(&id) {
        r["progress"] = progress_of(p);
    }
    if let Some(p) = app.checks.lock().unwrap_or_else(|e| e.into_inner()).get(&id) {
        r["checking"] = progress_of(p);
    }
    r["cuttable"] = json!(app.kept_run(&id).is_some());
    r["live"] = json!(app.live(&id).is_some());
    Ok(Json(r))
}

async fn remove(State(app): State<Shared>, Path(id): Path<String>) -> Answer {
    let r = app.row(&id).await?;
    if r["status"] == "running" || r["status"] == "queued" || r["status"] == "live" {
        return Err(ApiError(StatusCode::CONFLICT, "this experiment is running: wait for it to end".into()));
    }
    app.server.write(vec![json!({"op": "delete", "table": EXPERIMENTS, "key": id})]).await.map_err(upstream)?;
    app.lives.lock().unwrap_or_else(|e| e.into_inner()).retain(|l| l.id != id);
    if r["spec"]["mode"] == "served" || r["spec"]["mode"] == "live" {
        for t in [format!("exp_log_{id}"), format!("exp_events_{id}")] {
            let _ = app.server.sql(&format!("DROP TABLE IF EXISTS {t}"), json!({})).await;
        }
    }
    app.kept.lock().unwrap_or_else(|e| e.into_inner()).retain(|(x, _)| *x != id);
    Ok(Json(json!({"deleted": id})))
}

async fn cut(State(app): State<Shared>, Path(id): Path<String>, Json(b): Json<Value>) -> Answer {
    let by: Vec<String> = b["by"].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(str::to_string)).collect();
    if by.is_empty() || by.len() > 2 || by.iter().any(|a| !ATTRIBUTES.contains(&a.as_str())) || (by.len() == 2 && by[0] == by[1]) {
        return Err(bad(format!("cut by one or two of {}", ATTRIBUTES.join(", "))));
    }
    let d = app.kept_run(&id).ok_or_else(|| ApiError(StatusCode::CONFLICT, "this run is no longer in memory (the app keeps the last three): run it again to cut by two attributes".into()))?;
    let v = tokio::task::spawn_blocking(move || {
        let by: Vec<&str> = by.iter().map(String::as_str).collect();
        ab::cohort_table(&d, &by)
    })
    .await
    .map_err(|e| upstream(e.to_string()))?;
    Ok(Json(v))
}

/// Runs a finished experiment again `repeats` times with fresh seeds, in
/// the background, and keeps how often its intervals held the truth in
/// its report (`report.check`).
async fn check_run(State(app): State<Shared>, Path(id): Path<String>, body: Option<Json<Value>>) -> Answer {
    let b = body.map(|Json(b)| b).unwrap_or(Value::Null);
    let repeats = b["repeats"].as_u64().unwrap_or(crate::repeat::REPEATS as u64).clamp(5, crate::repeat::MAX_REPEATS as u64) as usize;
    let r = app.row(&id).await?;
    if r["status"] != "done" {
        return Err(ApiError(StatusCode::CONFLICT, "check an experiment once it is done".into()));
    }
    let spec: Spec = serde_json::from_value(r["report"]["spec"].clone()).map_err(|e| bad(format!("this experiment's spec: {e}")))?;
    if spec.mode == "interleave" {
        return Err(bad("an interleaving's report has its A/B beside it already: check A/B tests and live runs"));
    }
    let site = app.site(&spec.site).await?;
    let progress = Arc::new(Progress::default());
    progress.stage("waiting for the run before it");
    {
        let mut c = app.checks.lock().unwrap_or_else(|e| e.into_inner());
        if c.contains_key(&id) {
            return Err(ApiError(StatusCode::CONFLICT, "this experiment is being checked".into()));
        }
        c.insert(id.clone(), progress.clone());
    }
    let searches = crate::repeat::repeat_spec(&spec).impressions;
    {
        let (app, id) = (app.clone(), id.clone());
        tokio::spawn(async move {
            let _turn = app.turn.acquire().await;
            let result = crate::repeat::check(&app.server, &spec, &id, (*site).clone(), repeats, progress).await;
            let check = result.unwrap_or_else(|e| json!({"error": e}));
            // The row may have moved since: read it again, and keep everything but the check.
            match app.row(&id).await {
                Ok(mut r) => {
                    r["report"]["check"] = check;
                    let text = |v: &Value| if v.is_null() { Value::Null } else { json!(v.to_string()) };
                    let row = json!({"name": r["name"], "site": r["site"], "created": r["created"], "status": r["status"], "spec": text(&r["spec"]), "summary": text(&r["summary"]), "report": text(&r["report"]), "error": r["error"], "seconds": r["seconds"]});
                    if let Err(e) = app.save(&id, row).await {
                        eprintln!("chloedb-experiments: saving the check of {id}: {e}");
                    }
                }
                Err(e) => eprintln!("chloedb-experiments: the check of {id}: {}", e.1),
            }
            app.checks.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
        });
    }
    Ok(Json(json!({"id": id, "repeats": repeats, "searches": searches})))
}

async fn create(State(app): State<Shared>, Json(mut spec): Json<Spec>) -> Answer {
    if spec.mode == "interleave" {
        // Every user sees both arms: none explores, and no pre-period is needed.
        spec.explore_share = 0.0;
        spec.pre = 0.0;
    }
    spec.check().map_err(bad)?;
    let site = app.site(&spec.site).await?;
    let id = format!("e{}{:04x}", now_ms(), crate::policy::Rng::of(&format!("{}{}", spec.name, now_ms())).next_u64() & 0xffff);
    app.save(&id, json!({"name": spec.name, "site": spec.site, "created": now_ms(), "status": "queued", "spec": serde_json::to_string(&spec).unwrap_or_default()})).await.map_err(upstream)?;
    let progress = Arc::new(Progress::default());
    progress.stage("waiting for the run before it");
    app.progress.lock().unwrap_or_else(|e| e.into_inner()).insert(id.clone(), progress.clone());
    {
        let (app, id) = (app.clone(), id.clone());
        tokio::spawn(async move {
            let _turn = app.turn.acquire().await;
            let created = now_ms();
            let spec_text = serde_json::to_string(&spec).unwrap_or_default();
            let base = json!({"name": spec.name, "site": spec.site, "created": created, "spec": spec_text});
            let mut row = base.clone();
            row["status"] = json!("running");
            let _ = app.save(&id, row).await;
            let result = run_one(&app, &spec, &id, (*site).clone(), progress).await;
            let mut row = base;
            match result {
                Ok((summary, report, seconds)) => {
                    row["status"] = json!("done");
                    row["summary"] = json!(summary.to_string());
                    row["report"] = json!(report.to_string());
                    row["seconds"] = json!(seconds);
                }
                Err(e) => {
                    row["status"] = json!("failed");
                    row["error"] = json!(e);
                }
            }
            if let Err(e) = app.save(&id, row).await {
                eprintln!("chloedb-experiments: saving {id}: {e}");
            }
            app.progress.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
        });
    }
    Ok(Json(json!({"id": id})))
}

async fn run_one(app: &Shared, spec: &Spec, id: &str, mut site: Site, progress: Arc<Progress>) -> Result<(Value, Value, f64), String> {
    let t0 = std::time::Instant::now();
    if spec.mode == "interleave" {
        let mut report = crate::interleave::run(&app.server, spec, id, site, progress).await?;
        let seconds = t0.elapsed().as_secs_f64();
        report["seconds"] = json!(seconds);
        report["spec"] = json!(spec);
        let summary = json!({
            "verdicts": report["verdicts"], "users": report["users"], "searches": report["searches"], "kind": "interleave",
            "primary": "preference", "arms": spec.arms.iter().map(|a| &a.name).collect::<Vec<_>>(), "mode": "interleave", "model": spec.model,
            "times": report["saving"]["times"],
        });
        return Ok((summary, report, seconds));
    }
    progress.stage("ranking every query under each arm");
    let policy: Policy = spec.policy()?;
    let ranks = ab::rank(&app.server, &mut site, spec, &policy).await?;
    let d = Arc::new(ab::run(&app.server, spec, id, site, ranks, progress.clone()).await?);
    progress.stage("writing the report");
    let d2 = d.clone();
    let mut report = tokio::task::spawn_blocking(move || ab::report(&d2)).await.map_err(|e| e.to_string())?;
    let seconds = t0.elapsed().as_secs_f64();
    report["seconds"] = json!(seconds);
    report["spec"] = json!(spec);
    let summary = json!({
        "verdicts": report["verdicts"], "srm": report["srm"]["ok"], "users": report["users"], "searches": report["searches"],
        "primary": spec.primary, "arms": spec.arms.iter().map(|a| &a.name).collect::<Vec<_>>(), "mode": spec.mode, "model": spec.model,
    });
    app.keep_run(id, d);
    Ok((summary, report, seconds))
}

// ------------------------------------------------------------ live runs

async fn lives(State(app): State<Shared>) -> Answer {
    let ls: Vec<Arc<Live>> = app.lives.lock().unwrap_or_else(|e| e.into_inner()).iter().cloned().collect();
    let out: Vec<Value> = ls
        .iter()
        .map(|l| {
            let v = l.view(Some(u64::MAX), u64::MAX);
            json!({"id": l.id, "name": l.spec.name, "site": l.spec.site, "created": l.created, "status": v["status"], "elapsed": v["elapsed"], "limit_s": v["limit_s"], "searches": v["searches"], "agents": l.spec.agents})
        })
        .collect();
    Ok(Json(json!({"live": out})))
}

async fn create_live(State(app): State<Shared>, Json(spec): Json<LiveSpec>) -> Answer {
    spec.check().map_err(bad)?;
    if let Some(on) = app.lives.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|l| matches!(l.status(), "starting" | "live" | "ending")) {
        return Err(ApiError(StatusCode::CONFLICT, format!("a live run is on ({}): stop it first", on.spec.name)));
    }
    let brain = if spec.agents == 0 || spec.brain == "scripted" {
        Brain::Scripted
    } else {
        match &app.anthropic_key {
            Some(k) => Brain::Claude { key: k.clone(), model: spec.agent_model.clone(), base: app.anthropic_base.clone() },
            None => return Err(bad("Claude agents need an API key: start the app with ANTHROPIC_API_KEY, or choose scripted agents")),
        }
    };
    let site = app.site(&spec.site).await?;
    let t = app.tables(&spec.site).await?;
    let about = if t.about.trim().is_empty() { format!("the site {}, a table of ChloeDB", t.name) } else { t.about.clone() };
    let created = now_ms();
    let id = format!("l{created}{:04x}", crate::policy::Rng::of(&format!("{}{created}", spec.name)).next_u64() & 0xffff);
    let mut saved = serde_json::to_value(&spec).map_err(|e| bad(e.to_string()))?;
    saved["mode"] = json!("live");
    let saved = saved.to_string();
    app.save(&id, json!({"name": spec.name, "site": spec.site, "created": created, "status": "live", "spec": saved})).await.map_err(upstream)?;
    let l = Arc::new(Live::new(app.server.clone(), &id, spec.clone(), about, brain, created));
    {
        let mut ls = app.lives.lock().unwrap_or_else(|e| e.into_inner());
        ls.push_front(l.clone());
        ls.truncate(3);
    }
    {
        let (app, id) = (app.clone(), id.clone());
        tokio::spawn(async move {
            let base = json!({"name": spec.name, "site": spec.site, "created": created, "spec": saved});
            let r = l.clone().run((*site).clone()).await;
            let result: Result<(Value, Value, f64, Arc<RunData>), String> = async {
                r?;
                let d = Arc::new(l.run_data().ok_or("the run never started")?);
                let d2 = d.clone();
                let mut report = tokio::task::spawn_blocking(move || ab::report(&d2)).await.map_err(|e| e.to_string())?;
                let seconds = d.seconds;
                report["seconds"] = json!(seconds);
                report["spec"] = json!(d.spec);
                report["live"] = l.live_part();
                let summary = json!({
                    "verdicts": report["verdicts"], "srm": report["srm"]["ok"], "users": report["users"], "searches": report["searches"],
                    "primary": d.spec.primary, "arms": d.spec.arms.iter().map(|a| &a.name).collect::<Vec<_>>(), "mode": "live", "model": d.spec.model,
                    "agents": l.spec.agents, "cost": report["live"]["cost"],
                });
                Ok((summary, report, seconds, d))
            }
            .await;
            let mut row = base;
            let failed = match result {
                Ok((summary, report, seconds, d)) => {
                    row["status"] = json!("done");
                    row["summary"] = json!(summary.to_string());
                    row["report"] = json!(report.to_string());
                    row["seconds"] = json!(seconds);
                    app.keep_run(&id, d);
                    None
                }
                Err(e) => {
                    row["status"] = json!("failed");
                    row["error"] = json!(e);
                    Some(e)
                }
            };
            if let Err(e) = app.save(&id, row).await {
                eprintln!("chloedb-experiments: saving {id}: {e}");
            }
            match failed {
                None => l.done(),
                Some(e) => l.failed(e),
            }
        });
    }
    Ok(Json(json!({"id": id})))
}

#[derive(serde::Deserialize)]
struct ViewQ {
    after: Option<u64>,
    since: Option<u64>,
}

fn live_or_404(app: &App, id: &str) -> Result<Arc<Live>, ApiError> {
    app.live(id).ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("live run {id} is not in memory (the app keeps the last three): open its report")))
}

async fn one_live(State(app): State<Shared>, Path(id): Path<String>, Query(q): Query<ViewQ>) -> Answer {
    let l = live_or_404(&app, &id)?;
    Ok(Json(l.view(q.after, q.since.unwrap_or(0))))
}

async fn stop_live(State(app): State<Shared>, Path(id): Path<String>) -> Answer {
    let l = live_or_404(&app, &id)?;
    l.stop();
    Ok(Json(json!({"stopping": true})))
}

async fn rate_live(State(app): State<Shared>, Path(id): Path<String>, Json(b): Json<Value>) -> Answer {
    let l = live_or_404(&app, &id)?;
    let r = l.set_rate(b["rate"].as_f64().ok_or_else(|| bad("give a rate: searches a second"))?).map_err(bad)?;
    Ok(Json(json!({"rate": r})))
}

async fn try_search(State(app): State<Shared>, Path(id): Path<String>, Json(b): Json<Value>) -> Answer {
    let l = live_or_404(&app, &id)?;
    let q = b["q"].as_str().unwrap_or("").trim();
    if q.is_empty() || q.chars().count() > 300 {
        return Err(bad("search for something (up to 300 characters)"));
    }
    if l.status() != "live" {
        return Err(ApiError(StatusCode::CONFLICT, "the run is not live".into()));
    }
    Ok(Json(l.try_search(b["who"].as_str().unwrap_or("you"), q).await.map_err(bad)?))
}

async fn try_open(State(app): State<Shared>, Path(id): Path<String>, Json(b): Json<Value>) -> Answer {
    let l = live_or_404(&app, &id)?;
    let request = b["request"].as_str().ok_or_else(|| bad("which page (request)?"))?;
    let n = b["result"].as_u64().ok_or_else(|| bad("which result?"))? as usize;
    Ok(Json(l.try_open(request, n).await.map_err(bad)?))
}

/// Every metric by one or two attributes, over the live run's users so far.
async fn live_cohorts(State(app): State<Shared>, Path(id): Path<String>, Json(b): Json<Value>) -> Answer {
    let by: Vec<String> = b["by"].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(str::to_string)).collect();
    if by.is_empty() || by.len() > 2 || by.iter().any(|a| !ATTRIBUTES.contains(&a.as_str())) || (by.len() == 2 && by[0] == by[1]) {
        return Err(bad(format!("cut by one or two of {}", ATTRIBUTES.join(", "))));
    }
    let l = live_or_404(&app, &id)?;
    let d = l.run_data().ok_or_else(|| ApiError(StatusCode::CONFLICT, "the run is still starting".into()))?;
    let v = tokio::task::spawn_blocking(move || {
        let by: Vec<&str> = by.iter().map(String::as_str).collect();
        ab::cohort_table(&d, &by)
    })
    .await
    .map_err(|e| upstream(e.to_string()))?;
    Ok(Json(v))
}

async fn try_done(State(app): State<Shared>, Path(id): Path<String>, Json(b): Json<Value>) -> Answer {
    let l = live_or_404(&app, &id)?;
    let request = b["request"].as_str().ok_or_else(|| bad("which page (request)?"))?;
    let n = b["result"].as_u64().unwrap_or(0) as usize;
    Ok(Json(l.try_done(request, n, b["found"].as_bool().unwrap_or(false)).map_err(bad)?))
}

// ------------------------------------------------------------ real sites

impl App {
    fn source(&self, name: &str) -> Result<Arc<Source>, ApiError> {
        self.sources.iter().find(|s| s.name == name).cloned().ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("no source {name}: the app is started with --source NAME=KIND:URL")))
    }

    fn brain(&self, kind: &str, model: &str) -> Result<Brain, ApiError> {
        if kind == "scripted" {
            return Ok(Brain::Scripted);
        }
        match &self.anthropic_key {
            Some(k) => Ok(Brain::Claude { key: k.clone(), model: model.to_string(), base: self.anthropic_base.clone() }),
            None => Err(bad("Claude needs an API key: start the app with ANTHROPIC_API_KEY, or choose scripted")),
        }
    }

    async fn persona_set_of(&self, id: &str) -> Result<PersonaSet, ApiError> {
        let a = self.server.query(&format!("SELECT doc FROM {PERSONAS} WHERE id = {}", lit(id)), json!({}), None).await.map_err(upstream)?;
        let doc = a["rows"][0][0].as_str().ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("no personas {id}")))?;
        serde_json::from_str(doc).map_err(|e| upstream(format!("personas {id}: {e}")))
    }

    fn study(&self, id: &str) -> Option<Arc<Study>> {
        self.studies.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|s| s.id == id).cloned()
    }
}

/// The real sites: each source with what it says of itself (its knobs and
/// presets, or why it takes no experiments).
async fn real_sites(State(app): State<Shared>) -> Json<Value> {
    let all = futures_join(app.sources.iter().map(|s| {
        let s = s.clone();
        async move { s.describe().await }
    }))
    .await;
    Json(json!({"sources": all, "kinds": crate::source::KINDS.iter().map(|(k, a)| json!({"kind": k, "about": a})).collect::<Vec<_>>()}))
}

async fn futures_join<F: std::future::Future<Output = Value> + Send + 'static>(fs: impl Iterator<Item = F>) -> Vec<Value> {
    let hs: Vec<_> = fs.map(tokio::spawn).collect();
    let mut out = Vec::new();
    for h in hs {
        out.push(h.await.unwrap_or(Value::Null));
    }
    out
}

#[derive(serde::Deserialize)]
struct RealSearch {
    q: String,
    #[serde(default)]
    knobs: Value,
    #[serde(default)]
    per: Option<usize>,
}

/// A search of a real site with knobs, as an arm would see it.
async fn real_search(State(app): State<Shared>, Path(source): Path<String>, Json(b): Json<RealSearch>) -> Answer {
    let s = app.source(&source)?;
    let page = s.search(b.q.trim(), &b.knobs, b.per.unwrap_or(10).clamp(1, 30)).await.map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(json!(page)))
}

async fn real_item(State(app): State<Shared>, Path((source, id)): Path<(String, String)>) -> Answer {
    let s = app.source(&source)?;
    let it = s.item(&id).await.map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(json!(it)))
}

async fn persona_sets(State(app): State<Shared>) -> Answer {
    let a = app.server.query(&format!("SELECT doc FROM {PERSONAS} ORDER BY created DESC LIMIT 200"), json!({}), None).await.map_err(upstream)?;
    let sets: Vec<Value> = a["rows"].as_array().into_iter().flatten().filter_map(|r| r[0].as_str().and_then(|d| serde_json::from_str::<PersonaSet>(d).ok())).map(|s| s.summary()).collect();
    let writing: Vec<Value> = app.writing.lock().unwrap_or_else(|e| e.into_inner()).values().cloned().collect();
    Ok(Json(json!({"sets": sets, "writing": writing})))
}

async fn persona_set(State(app): State<Shared>, Path(id): Path<String>) -> Answer {
    if let Some(w) = app.writing.lock().unwrap_or_else(|e| e.into_inner()).get(&id) {
        return Ok(Json(w.clone()));
    }
    let s = app.persona_set_of(&id).await?;
    let mut v = json!(s);
    v["status"] = json!("ready");
    Ok(Json(v))
}

async fn drop_personas(State(app): State<Shared>, Path(id): Path<String>) -> Answer {
    app.server.write(vec![json!({"op": "delete", "table": PERSONAS, "key": id})]).await.map_err(upstream)?;
    Ok(Json(json!({"deleted": id})))
}

#[derive(serde::Deserialize)]
struct WritePersonas {
    source: String,
    #[serde(default)]
    count: Option<usize>,
    #[serde(default)]
    needs: Option<usize>,
    #[serde(default)]
    seed: Option<u64>,
    /// `claude` or `scripted`.
    #[serde(default)]
    brain: Option<String>,
    #[serde(default)]
    model: Option<String>,
}

/// Personas written from a real site's rows, in the background: the id
/// answers what it is doing until they are kept.
async fn write_personas(State(app): State<Shared>, Json(b): Json<WritePersonas>) -> Answer {
    let src = app.source(&b.source)?;
    let d = src.describe().await;
    if d["allowed"] != json!(true) {
        return Err(bad(format!("{} takes no experiments: {}", src.name, d["error"].as_str().unwrap_or(""))));
    }
    let model = b.model.unwrap_or_else(|| "claude-opus-5-5".into());
    if !live::AGENT_MODELS.iter().any(|m| m.0 == model) {
        return Err(bad(format!("models are one of {}", live::AGENT_MODELS.iter().map(|m| m.0).collect::<Vec<_>>().join(", "))));
    }
    let brain = app.brain(b.brain.as_deref().unwrap_or("claude"), &model)?;
    let (count, needs, seed) = (b.count.unwrap_or(8).clamp(1, personas::MAX_PERSONAS), b.needs.unwrap_or(3).clamp(1, personas::MAX_NEEDS), b.seed.unwrap_or(7));
    let created = now_ms();
    let id = format!("ps{created}");
    let status = |what: &str| json!({"id": id, "source": src.name, "title": d["title"], "status": "writing", "stage": what, "created": created});
    app.writing.lock().unwrap_or_else(|e| e.into_inner()).insert(id.clone(), status("reading the site's rows"));
    {
        let (app, id) = (app.clone(), id.clone());
        tokio::spawn(async move {
            let r: Result<PersonaSet, String> = async {
                let rows = src.sample(60, seed).await?;
                if rows.is_empty() {
                    return Err(format!("{} gave no rows to write from", src.name));
                }
                app.writing.lock().unwrap_or_else(|e| e.into_inner()).insert(id.clone(), json!({"id": id, "source": src.name, "title": d["title"], "status": "writing", "stage": format!("writing {count} people from {} of its rows", rows.len()), "created": created}));
                let (ps, cost) = personas::write(&brain, &reqwest::Client::new(), &id, &d, &rows, count, needs, seed).await?;
                if ps.is_empty() {
                    return Err("no personas came back".into());
                }
                let set = PersonaSet { id: id.clone(), source: src.name.clone(), title: d["title"].as_str().unwrap_or("").into(), created, model: brain.name().into(), seed, rows: rows.len(), cost, personas: ps };
                app.server.write(vec![json!({"op": "upsert", "table": PERSONAS, "doc": {"id": id, "source": set.source, "created": created, "doc": serde_json::to_string(&set).unwrap_or_default()}})]).await?;
                Ok(set)
            }
            .await;
            let mut w = app.writing.lock().unwrap_or_else(|e| e.into_inner());
            match r {
                Ok(_) => {
                    w.remove(&id);
                }
                Err(e) => {
                    w.insert(id.clone(), json!({"id": id, "source": src.name, "status": "failed", "error": e, "created": created}));
                }
            }
        });
    }
    Ok(Json(json!({"id": id})))
}

async fn studies(State(app): State<Shared>) -> Answer {
    let a = app.server.query(&format!("SELECT id, state FROM {STUDIES} ORDER BY created DESC LIMIT 200"), json!({}), None).await.map_err(upstream)?;
    let mut out = Vec::new();
    for r in a["rows"].as_array().into_iter().flatten() {
        let id = r[0].as_str().unwrap_or("");
        let v = match app.study(id) {
            Some(s) => s.view(),
            None => r[1].as_str().and_then(|v| serde_json::from_str(v).ok()).unwrap_or(Value::Null),
        };
        let cmp = &v["report"]["comparisons"];
        out.push(json!({"id": id, "name": v["name"], "status": v["status"], "created": v["created"], "done": v["done"], "total": v["total"], "spent": v["spent"], "arms": v["spec"]["arms"], "personas": v["personas"], "comparisons": cmp}));
    }
    Ok(Json(json!({"studies": out})))
}

async fn one_study(State(app): State<Shared>, Path(id): Path<String>) -> Answer {
    if let Some(s) = app.study(&id) {
        return Ok(Json(s.view()));
    }
    let a = app.server.query(&format!("SELECT state FROM {STUDIES} WHERE id = {}", lit(&id)), json!({}), None).await.map_err(upstream)?;
    let v = a["rows"][0][0].as_str().and_then(|v| serde_json::from_str::<Value>(v).ok()).ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("no study {id}")))?;
    Ok(Json(v))
}

#[derive(serde::Deserialize)]
struct SessionsQ {
    #[serde(default)]
    need: Option<String>,
}

/// A study's sessions, every step of each: of one need, or all.
async fn study_sessions(State(app): State<Shared>, Path(id): Path<String>, Query(q): Query<SessionsQ>) -> Answer {
    let all: Vec<study::Session> = match app.study(&id) {
        Some(s) => s.kept().1,
        None => {
            let a = app.server.query(&format!("SELECT sessions FROM {STUDIES} WHERE id = {}", lit(&id)), json!({}), None).await.map_err(upstream)?;
            a["rows"][0][0].as_str().and_then(|v| serde_json::from_str(v).ok()).ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("no study {id}")))?
        }
    };
    let list: Vec<&study::Session> = all.iter().filter(|s| q.need.as_ref().is_none_or(|n| &s.need == n)).collect();
    Ok(Json(json!({"sessions": list})))
}

async fn stop_study(State(app): State<Shared>, Path(id): Path<String>) -> Answer {
    let s = app.study(&id).ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("no running study {id}")))?;
    s.stop();
    Ok(Json(json!({"stopping": id})))
}

/// A study, started: every need of the personas on every arm.
async fn create_study(State(app): State<Shared>, Json(spec): Json<StudySpec>) -> Answer {
    let set = app.persona_set_of(&spec.personas).await?;
    spec.check(&set).map_err(bad)?;
    if let Some(on) = app.studies.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|s| s.running()) {
        return Err(ApiError(StatusCode::CONFLICT, format!("a study is running ({}): stop it first", on.spec.name)));
    }
    let mut sources = Vec::new();
    for a in &spec.arms {
        let s = app.source(&a.source)?;
        let d = s.describe().await;
        if d["allowed"] != json!(true) {
            return Err(bad(format!("{}: {} takes no experiments ({})", a.name, s.name, d["error"].as_str().unwrap_or(""))));
        }
        // An arm's knobs are checked by a search before the study starts.
        s.search("test", &a.knobs, 3).await.map_err(|e| bad(format!("{}: {e}", a.name)))?;
        sources.push(s);
    }
    let about = match app.sources.iter().find(|s| s.name == set.source) {
        Some(s) => s.describe().await["about"].as_str().unwrap_or("").to_string(),
        None => sources[0].describe().await["about"].as_str().unwrap_or("").to_string(),
    };
    let brain = app.brain(&spec.brain, &spec.agent_model)?;
    let judge = Arc::new(Judge::new(app.brain(&spec.brain, &spec.judge_model)?, app.server.clone()));
    let created = now_ms();
    let id = format!("st{created}");
    let s = Arc::new(Study::new(&id, spec, set, sources, about, brain, judge, created));
    {
        let mut ls = app.studies.lock().unwrap_or_else(|e| e.into_inner());
        ls.push_front(s.clone());
        ls.truncate(3);
    }
    let save_server = app.server.clone();
    // One write at a time, of the study as it is when the write starts, so
    // the last write kept is the last state.
    let turn = Arc::new(tokio::sync::Mutex::new(()));
    let save = move |st: Arc<Study>| {
        let (server, turn) = (save_server.clone(), turn.clone());
        tokio::spawn(async move {
            let _t = turn.lock().await;
            let (view, sessions) = st.kept();
            let doc = json!({"id": st.id, "name": st.spec.name, "created": st.created, "status": view["status"], "state": view.to_string(), "sessions": serde_json::to_string(&sessions).unwrap_or_default()});
            if let Err(e) = server.write(vec![json!({"op": "upsert", "table": STUDIES, "doc": doc})]).await {
                eprintln!("chloedb-experiments: keeping study {}: {e}", st.id);
            }
        });
    };
    save(s.clone());
    tokio::spawn(s.clone().run(save));
    Ok(Json(json!({"id": id})))
}
