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

use axum::extract::{Path, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::ab::{self, Progress, RunData, Spec};
use crate::click::Model;
use crate::policy::Policy;
use crate::server::{lit, Server};
use crate::site::{self, Site, Tables};
use crate::users::{AGES, ATTRIBUTES, COUNTRIES, DEVICES, GENDERS, TENURES};

pub const VIEW: &str = include_str!("../ui/view.js");

/// Two arms and a split between them.
pub const ICON: &str = r#"<path d="M9 3h6"/><path d="M10 3v6.5L4.6 18.4A1.7 1.7 0 0 0 6 21h12a1.7 1.7 0 0 0 1.4-2.6L14 9.5V3"/><path d="M7.5 15h9"/>"#;

const SITES: &str = "exp_sites";
const EXPERIMENTS: &str = "exp_experiments";
/// Runs kept in memory for cuts by two attributes.
const KEPT: usize = 3;

pub struct Opts {
    pub token: Option<String>,
    /// BEIR collections to load as sites at the start: (directory, name).
    pub beir: Vec<(PathBuf, String)>,
}

pub struct App {
    pub server: Server,
    pub token: Option<String>,
    sites: tokio::sync::Mutex<HashMap<String, Arc<Site>>>,
    loading: Mutex<HashMap<String, String>>,
    progress: Mutex<HashMap<String, Arc<Progress>>>,
    kept: Mutex<VecDeque<(String, Arc<RunData>)>>,
    turn: tokio::sync::Semaphore,
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

fn now_ms() -> i64 {
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
        kept: Mutex::new(VecDeque::new()),
        turn: tokio::sync::Semaphore::new(1),
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
    ];
    for t in &tables {
        if let Err(e) = s.sql(t, json!({})).await {
            eprintln!("chloedb-experiments: {e}");
        }
    }
    // Runs left queued or running by a process that ended.
    if let Ok(a) = s.query(&format!("SELECT id FROM {EXPERIMENTS} WHERE status IN ('queued', 'running')"), json!({}), None).await {
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
async fn meta() -> Json<Value> {
    let d: Spec = serde_json::from_value(json!({"name": "", "site": "", "arms": []})).expect("the defaults");
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
        for r in &ranks {
            let sql = format!("SELECT id FROM {} ORDER BY {r} DESC LIMIT {limit}", s.table);
            let a = app.server.query(&sql, json!({"t": s.queries[q].text}), None).await.map_err(|e| bad(format!("ranking {r}: {e}")))?;
            let keys: Vec<String> = a["rows"].as_array().into_iter().flatten().map(|x| crate::sim::key_text(&x[0])).collect();
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
            json!({"id": r[0], "name": r[1], "site": r[2], "created": r[3], "status": r[4], "summary": r[5].as_str().and_then(|s| serde_json::from_str::<Value>(s).ok()), "error": r[6], "seconds": r[7], "progress": progress.get(id).map(|p| progress_of(p))})
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
    r["cuttable"] = json!(app.kept_run(&id).is_some());
    Ok(Json(r))
}

async fn remove(State(app): State<Shared>, Path(id): Path<String>) -> Answer {
    let r = app.row(&id).await?;
    if r["status"] == "running" || r["status"] == "queued" {
        return Err(ApiError(StatusCode::CONFLICT, "this experiment is running: wait for it to end".into()));
    }
    app.server.write(vec![json!({"op": "delete", "table": EXPERIMENTS, "key": id})]).await.map_err(upstream)?;
    if r["spec"]["mode"] == "served" {
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

async fn create(State(app): State<Shared>, Json(spec): Json<Spec>) -> Answer {
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
