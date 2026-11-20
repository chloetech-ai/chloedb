//! Parley's HTTP API and its web page (the page is examples/parley's).
//!
//! Every answer to a GET carries `queries`, the statements it ran with
//! their time and rows, for the page's SQL drawer, and `took_ms`.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::body::{Body, Bytes};
use axum::extract::{ConnectInfo, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::Router;
use serde_json::{json, Value};

use crate::app::App;
use crate::db::TRACE;
use crate::util::{err, key_hash, now, Error, Result};

pub const INDEX: &str = include_str!("../../examples/parley/ui/index.html");
const MAX_BODY: usize = 8 * 1024 * 1024;

pub struct Server {
    pub app: Arc<App>,
    pub register_token: Option<String>,
    pub admin_key: Option<String>,
    pub per_minute: usize,
    pub behind_proxy: bool,
    pub ui_dir: Option<std::path::PathBuf>,
    hits: Mutex<HashMap<String, Vec<f64>>>,
}

impl Server {
    pub fn new(app: Arc<App>) -> Server {
        Server {
            app,
            register_token: None,
            admin_key: None,
            per_minute: 0,
            behind_proxy: false,
            ui_dir: None,
            hits: Mutex::new(HashMap::new()),
        }
    }

    fn throttle_ok(&self, who: &str) -> bool {
        if self.per_minute == 0 {
            return true;
        }
        let t = now();
        let mut hits = self.hits.lock().unwrap_or_else(|e| e.into_inner());
        let xs = hits.entry(who.to_string()).or_default();
        xs.retain(|x| t - x < 60.0);
        if xs.len() >= self.per_minute {
            return false;
        }
        xs.push(t);
        if hits.len() > 50_000 {
            hits.retain(|_, v| v.last().is_some_and(|l| t - l < 60.0));
        }
        true
    }
}

/// One request, as the routes see it.
pub struct Req {
    pub method: Method,
    pub path: String,
    pub qs: HashMap<String, String>,
    pub headers: HeaderMap,
    pub body: Value,
    pub client: String,
}

impl Req {
    pub fn q(&self, k: &str) -> Option<&str> {
        self.qs.get(k).map(String::as_str).filter(|s| !s.is_empty())
    }
    pub fn qd<'a>(&'a self, k: &str, d: &'a str) -> &'a str {
        self.q(k).unwrap_or(d)
    }
    pub fn lim(&self, k: &str, d: usize, top: usize) -> usize {
        self.q(k)
            .and_then(|v| v.parse::<i64>().ok())
            .map(|n| n.clamp(1, top as i64) as usize)
            .unwrap_or(d)
    }
    pub fn b(&self, k: &str) -> String {
        match &self.body[k] {
            Value::String(s) => s.clone(),
            Value::Null => String::new(),
            v => v.to_string(),
        }
    }
    pub fn bo(&self, k: &str) -> Option<String> {
        Some(self.b(k)).filter(|s| !s.is_empty())
    }
    pub fn header(&self, k: &str) -> &str {
        self.headers
            .get(k)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
    }
}

pub fn router(server: Arc<Server>) -> Router {
    Router::new().fallback(handle).with_state(server)
}

async fn handle(
    State(s): State<Arc<Server>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let path = uri.path().to_string();
    let qs: HashMap<String, String> =
        url::form_urlencoded::parse(uri.query().unwrap_or("").as_bytes())
            .into_owned()
            .collect();
    if method == Method::GET && (path == "/" || path == "/index.html") {
        let page = s
            .ui_dir
            .as_ref()
            .and_then(|d| std::fs::read_to_string(d.join("index.html")).ok())
            .unwrap_or_else(|| INDEX.to_string());
        return bytes(200, page.into_bytes(), "text/html; charset=utf-8", false);
    }
    if method == Method::GET && (path == "/healthz" || path == "/readyz") {
        return json_out(200, &json!({"ok": true}));
    }
    let client = if s.behind_proxy {
        headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').next())
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    } else {
        None
    }
    .unwrap_or_else(|| addr.ip().to_string());
    if !s.throttle_ok(&client) {
        return json_out(429, &json!({"error": "too many requests: slow down"}));
    }
    if body.len() > MAX_BODY {
        return json_out(413, &json!({"error": "request too large"}));
    }
    let body: Value = if body.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice(&body) {
            Ok(v @ Value::Object(_)) => v,
            Ok(_) => return json_out(400, &json!({"error": "the body is a JSON object"})),
            Err(_) => return json_out(400, &json!({"error": "the body is not JSON"})),
        }
    };
    let req = Req {
        method: method.clone(),
        path: path.clone(),
        qs,
        headers,
        body,
        client,
    };
    let trace = Arc::new(Mutex::new(Vec::new()));
    let t0 = Instant::now();
    let out = TRACE
        .scope(trace.clone(), async {
            crate::routes::dispatch(&s, &req).await
        })
        .await;
    match out {
        Ok(Out::Json(status, mut v)) => {
            if v.is_object() && (method == Method::GET || path == "/api/ask") {
                v["queries"] =
                    Value::Array(trace.lock().unwrap_or_else(|e| e.into_inner()).clone());
                if v.get("took_ms").is_none() {
                    v["took_ms"] =
                        json!(crate::util::round(t0.elapsed().as_secs_f64() * 1000.0, 1));
                }
            }
            json_out(status, &v)
        }
        Ok(Out::Raw(r)) => r,
        Err(e) => json_out(e.status, &json!({"error": e.message})),
    }
}

pub enum Out {
    Json(u16, Value),
    Raw(Response),
}

pub fn ok(v: Value) -> Result<Out> {
    Ok(Out::Json(200, v))
}

pub fn created(v: Value) -> Result<Out> {
    Ok(Out::Json(201, v))
}

pub fn json_out(status: u16, v: &Value) -> Response {
    bytes(
        status,
        serde_json::to_vec(v).unwrap_or_default(),
        "application/json; charset=utf-8",
        false,
    )
}

pub fn bytes(status: u16, data: Vec<u8>, ctype: &str, cache: bool) -> Response {
    let mut r = Response::new(Body::from(data));
    *r.status_mut() = StatusCode::from_u16(status).unwrap_or(StatusCode::OK);
    let h = r.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(ctype)
            .unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    h.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    if ctype.starts_with("text/html") {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    }
    if cache {
        h.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        );
        h.insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static("default-src 'none'; sandbox"),
        );
    }
    r
}

impl Server {
    /// The agent whose API key came with the request.
    pub async fn agent(&self, r: &Req) -> Result<String> {
        let auth = r.header("authorization");
        let key = if auth.to_lowercase().starts_with("bearer ") {
            auth[7..].trim()
        } else {
            ""
        };
        self.app.authenticate(key).await
    }

    /// Whose view: an API key, or ?as=<handle>.
    pub async fn viewer(&self, r: &Req, optional: bool) -> Result<Option<String>> {
        if !r.header("authorization").is_empty() {
            return Ok(Some(self.agent(r).await?));
        }
        if let Some(a) = r.q("as") {
            return Ok(Some(a.to_lowercase().trim_start_matches('@').to_string()));
        }
        if optional {
            return Ok(None);
        }
        err(400, "say whose view: an API key, or ?as=<handle>")
    }

    pub fn admin(&self, r: &Req) -> Result<()> {
        let Some(k) = &self.admin_key else {
            return Ok(());
        };
        let given = r.header("x-parley-admin");
        if k.is_empty() || !const_eq(given.as_bytes(), k.as_bytes()) {
            return err(403, "starting and stopping experiments needs the admin key");
        }
        Ok(())
    }
}

pub fn const_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

impl App {
    pub async fn authenticate(&self, key: &str) -> Result<String> {
        if key.is_empty() {
            return err(401, "an API key is needed: Authorization: Bearer pk_...");
        }
        let h = key_hash(key);
        if let Some(a) = self.keys.lock().unwrap_or_else(|e| e.into_inner()).get(&h) {
            return Ok(a.clone());
        }
        let row = self
            .db
            .one(
                "auth",
                "SELECT id, status FROM agents WHERE key_hash = :h LIMIT 1",
                json!({"h": h}),
            )
            .await?;
        let Some(row) = row else {
            return err(401, "unknown API key");
        };
        use crate::db::RowExt;
        if row.s("status") == "banned" {
            return err(403, "this agent is banned");
        }
        let id = row.s("id");
        self.keys
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(h, id.clone());
        Ok(id)
    }
}

impl From<Error> for Response {
    fn from(e: Error) -> Response {
        e.into_response()
    }
}
