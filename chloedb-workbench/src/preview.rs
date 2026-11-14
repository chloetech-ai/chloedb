//! Preview: the app an agent runs (a dev server on loopback), shown
//! through Workbench at `/preview/<agent>/`, with an overlay that lets the
//! user pick an element and send it to the agent whose diff made it.
//!
//! Security: an agent's preview URL is `http://127.0.0.1:<port>` or
//! `http://localhost:<port>` only (never Workbench's own port), so the
//! proxy reaches nothing but loopback. The server's guard checks the host
//! and, for a POST, the origin, as on every route. The token comes once as
//! `?token=`; the answer is a redirect without it that sets the cookie
//! `wb_preview` (HttpOnly, SameSite=Strict, Path=/preview/), so it is sent
//! to the proxy only, and the `/api` routes never take it. The previewed
//! page runs in Workbench's origin (the overlay must reach its parent),
//! so preview only apps you would trust with the session: the code the
//! agents wrote.

use std::path::PathBuf;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json as AxJson, Router};
use serde_json::{json, Value as Json};

use crate::git;
use crate::map;
use crate::server::{blocking, parse_body, same_bytes, ApiResult, App};
use crate::supervisor::{ApiError, Supervisor};
use crate::trail;

const OVERLAY: &str = include_str!("../ui/preview-overlay.js");
const COOKIE: &str = "wb_preview";
/// The most of an answer the proxy reads.
const BODY_MAX: u64 = 64 << 20;
const INJECT: &str = "<script src=\"/preview-overlay.js\"></script>";

pub fn routes() -> Router<App> {
    Router::new()
        .route("/api/agents/:id/preview", put(set_preview))
        .route("/api/preview/route", post(route_pick))
        .route("/preview-overlay.js", get(overlay))
        .route("/preview/:agent", get(proxy_root).post(proxy_root))
        .route("/preview/:agent/", get(proxy_root).post(proxy_root))
        .route("/preview/:agent/*rest", get(proxy_rest).post(proxy_rest))
}

// The preview's URL.

/// A preview URL, checked: `http://127.0.0.1:<port>` or
/// `http://localhost:<port>`, optionally with a path (for a dev server
/// served under a base path), never `own_port`. Answers it without a
/// trailing `/`.
pub fn check_url(url: &str, own_port: Option<u16>) -> Result<String, String> {
    let bad = || format!("a preview URL is http://127.0.0.1:<port> or http://localhost:<port>, not {url:?}");
    let rest = url.trim().strip_prefix("http://").ok_or_else(bad)?;
    let end = rest.find('/').unwrap_or(rest.len());
    let (auth, path) = rest.split_at(end);
    if path.contains(['?', '#']) || path.chars().any(|c| c.is_whitespace() || c.is_control() || c == '\\') {
        return Err(format!("a preview URL has no query, fragment or spaces: {url:?}"));
    }
    let (host, port) = auth.rsplit_once(':').ok_or_else(bad)?;
    if !matches!(host, "127.0.0.1" | "localhost") {
        return Err(bad());
    }
    let port: u16 = port.parse().ok().filter(|p| *p > 0).ok_or_else(bad)?;
    if Some(port) == own_port {
        return Err(format!("port {port} is Workbench's own"));
    }
    let path = path.trim_end_matches('/');
    if path.split('/').any(|s| s == ".." || s == ".") {
        return Err(format!("a preview URL's path has no . or .. segments: {url:?}"));
    }
    Ok(format!("http://{host}:{port}{path}"))
}

fn own_port(sup: &Supervisor) -> Option<u16> {
    sup.cfg.url.rsplit(':').next().and_then(|p| p.trim_end_matches('/').parse().ok())
}

#[derive(serde::Deserialize)]
struct PreviewB {
    url: Option<String>,
}

/// `PUT /api/agents/:id/preview {url}`: sets where the agent's app runs;
/// an empty or null `url` clears it.
async fn set_preview(State(app): State<App>, Path(id): Path<String>, body: Bytes) -> ApiResult {
    let b: PreviewB = parse_body(&body)?;
    let url = match b.url.as_deref().map(str::trim).filter(|u| !u.is_empty()) {
        Some(u) => Some(check_url(u, own_port(&app.sup)).map_err(ApiError::bad)?),
        None => None,
    };
    let agent = app.sup.set_preview_url(&id, url)?;
    Ok(AxJson(json!({ "agent": agent })).into_response())
}

// The overlay.

async fn overlay(State(app): State<App>) -> Response {
    let body = match &app.ui_dir {
        Some(dir) => match std::fs::read_to_string(dir.join("preview-overlay.js")) {
            Ok(s) => s,
            Err(e) => return ApiError::not_found(format!("cannot read {}: {e}", dir.join("preview-overlay.js").display())).into_response(),
        },
        None => OVERLAY.to_string(),
    };
    ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8"), (header::CACHE_CONTROL, "no-store"), (header::X_CONTENT_TYPE_OPTIONS, "nosniff")], body).into_response()
}

// The proxy.

fn page(status: StatusCode, msg: &str) -> Response {
    let esc = msg.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let body = format!("<!doctype html><meta charset=utf-8><title>Preview</title><body style=\"font:14px system-ui;padding:24px;color:#555\"><p>{esc}</p></body>");
    (status, [(header::CONTENT_TYPE, "text/html; charset=utf-8"), (header::CACHE_CONTROL, "no-store")], body).into_response()
}

fn cookie_token(headers: &HeaderMap) -> Option<String> {
    headers.get_all(header::COOKIE).iter().filter_map(|h| h.to_str().ok()).flat_map(|h| h.split(';')).find_map(|kv| kv.trim().strip_prefix("wb_preview=").map(str::to_string))
}

/// The query without its `token`, and the token.
fn split_query(q: Option<&str>) -> (String, Option<String>) {
    let mut token = None;
    let rest: Vec<&str> = q
        .unwrap_or("")
        .split('&')
        .filter(|kv| !kv.is_empty())
        .filter(|kv| match kv.strip_prefix("token=") {
            Some(t) => {
                token = Some(t.to_string());
                false
            }
            None => true,
        })
        .collect();
    (rest.join("&"), token)
}

async fn proxy_root(State(app): State<App>, Path(agent): Path<String>, method: Method, uri: Uri, headers: HeaderMap, body: Bytes) -> Response {
    proxy(app, agent, String::new(), method, uri, headers, body).await
}

async fn proxy_rest(State(app): State<App>, Path((agent, rest)): Path<(String, String)>, method: Method, uri: Uri, headers: HeaderMap, body: Bytes) -> Response {
    proxy(app, agent, rest, method, uri, headers, body).await
}

async fn proxy(app: App, agent: String, rest: String, method: Method, uri: Uri, headers: HeaderMap, body: Bytes) -> Response {
    if !matches!(method, Method::GET | Method::HEAD | Method::POST) {
        return page(StatusCode::METHOD_NOT_ALLOWED, "The preview takes GET, HEAD and POST.");
    }
    let token = &app.sup.cfg.token;
    let (query, given) = split_query(uri.query());
    let prefix = format!("/preview/{agent}");
    let set_cookie = match given {
        Some(t) if same_bytes(&t, token) => {
            let cookie = format!("{COOKIE}={token}; Path=/preview/; HttpOnly; SameSite=Strict");
            if method == Method::GET {
                // The address without the token, which the page never sees.
                let mut to = if uri.path() == prefix { format!("{prefix}/") } else { uri.path().to_string() };
                if !query.is_empty() {
                    to = format!("{to}?{query}");
                }
                return (StatusCode::SEE_OTHER, [(header::LOCATION, to), (header::SET_COOKIE, cookie), (header::CACHE_CONTROL, "no-store".into())]).into_response();
            }
            Some(cookie)
        }
        Some(_) => return page(StatusCode::UNAUTHORIZED, "This preview link's token is not Workbench's."),
        None => {
            let bearer = headers.get(header::AUTHORIZATION).and_then(|h| h.to_str().ok()).and_then(|h| h.strip_prefix("Bearer ")).map(|b| b.trim().to_string());
            if !cookie_token(&headers).or(bearer).is_some_and(|t| same_bytes(&t, token)) {
                return page(StatusCode::UNAUTHORIZED, "The preview needs Workbench's token: open it from Workbench.");
            }
            None
        }
    };
    if uri.path() == prefix {
        let to = if query.is_empty() { format!("{prefix}/") } else { format!("{prefix}/?{query}") };
        return (StatusCode::SEE_OTHER, [(header::LOCATION, to)]).into_response();
    }
    let Some(a) = app.sup.agent_row(&agent) else { return page(StatusCode::NOT_FOUND, &format!("There is no agent {agent}.")) };
    let Some(base) = a.preview_url.clone() else {
        return page(StatusCode::NOT_FOUND, &format!("{} has no preview yet: it sets one with the set_preview tool, or PUT /api/agents/{agent}/preview.", a.name));
    };
    let req = Upstream { base, prefix: format!("{prefix}/"), rest, query, method, headers, body };
    let name = a.name.clone();
    let mut res = match tokio::task::spawn_blocking(move || req.send()).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => page(StatusCode::BAD_GATEWAY, &format!("{name}'s preview does not answer: {e}")),
        Err(e) => page(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    if let Some(c) = set_cookie.and_then(|c| HeaderValue::from_str(&c).ok()) {
        res.headers_mut().append(header::SET_COOKIE, c);
    }
    res
}

/// A request to forward to an agent's app.
struct Upstream {
    /// The app's URL, `http://host:port[/base]`.
    base: String,
    /// `/preview/<agent>/`.
    prefix: String,
    rest: String,
    query: String,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
}

/// The request headers passed on to the app.
const PASS: [&str; 10] = ["accept", "accept-language", "content-type", "user-agent", "if-none-match", "if-modified-since", "range", "cache-control", "x-requested-with", "authorization"];
/// The answer's headers not passed back.
const DROP: [&str; 12] = ["connection", "keep-alive", "proxy-authenticate", "proxy-authorization", "te", "trailer", "trailers", "transfer-encoding", "upgrade", "content-length", "x-frame-options", "set-cookie"];

fn with_headers<B>(mut rb: ureq::RequestBuilder<B>, hs: &[(String, String)]) -> ureq::RequestBuilder<B> {
    for (k, v) in hs {
        rb = rb.header(k.as_str(), v.as_str());
    }
    rb
}

impl Upstream {
    /// `(origin, base path)` of the app's URL.
    fn origin(&self) -> (&str, &str) {
        let after = self.base.find("://").map_or(0, |i| i + 3);
        match self.base[after..].find('/') {
            Some(i) => self.base.split_at(after + i),
            None => (self.base.as_str(), ""),
        }
    }

    fn send(self) -> Result<Response, String> {
        let (origin, base_path) = self.origin();
        let mut url = format!("{origin}{base_path}/{}", self.rest);
        if !self.query.is_empty() {
            url = format!("{url}?{}", self.query);
        }
        let mut hs: Vec<(String, String)> = Vec::new();
        for k in PASS {
            // A Bearer token for Workbench is not the app's.
            if k == "authorization" && self.headers.get(k).and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("Bearer ")) {
                continue;
            }
            if let Some(v) = self.headers.get(k).and_then(|v| v.to_str().ok()) {
                hs.push((k.into(), v.into()));
            }
        }
        let cookies: Vec<String> = self.headers.get_all(header::COOKIE).iter().filter_map(|h| h.to_str().ok()).flat_map(|h| h.split(';')).map(str::trim).filter(|c| !c.is_empty() && !c.starts_with("wb_preview=")).map(str::to_string).collect();
        if !cookies.is_empty() {
            hs.push(("cookie".into(), cookies.join("; ")));
        }
        // The app sees its own origin, as a browser at it would send.
        if self.headers.contains_key(header::ORIGIN) {
            hs.push(("origin".into(), origin.to_string()));
        }
        if let Some(r) = self.headers.get(header::REFERER).and_then(|v| v.to_str().ok()) {
            if let Some(i) = r.find(&self.prefix) {
                hs.push(("referer".into(), format!("{origin}{base_path}/{}", &r[i + self.prefix.len()..])));
            }
        }
        // Plain bodies, so that HTML can take the overlay.
        hs.push(("accept-encoding".into(), "identity".into()));
        let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(60))).http_status_as_error(false).max_redirects(0).proxy(None).build().into();
        let res = match self.method {
            Method::HEAD => with_headers(agent.head(&url), &hs).call(),
            Method::POST => with_headers(agent.post(&url), &hs).send(&self.body[..]),
            _ => with_headers(agent.get(&url), &hs).call(),
        };
        let res = res.map_err(|e| format!("{e} ({url})"))?;
        let (parts, mut body) = res.into_parts();
        let bytes = if self.method == Method::HEAD { Vec::new() } else { body.with_config().limit(BODY_MAX).read_to_vec().map_err(|e| e.to_string())? };
        let html = parts.headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(|t| t.to_ascii_lowercase().starts_with("text/html"));
        let encoded = parts.headers.get(header::CONTENT_ENCODING).and_then(|v| v.to_str().ok()).is_some_and(|e| !e.eq_ignore_ascii_case("identity"));
        let bytes = if html && !encoded && self.method != Method::HEAD { inject(&bytes, &self.prefix, base_path) } else { bytes };

        let mut out = Response::builder().status(parts.status.as_u16());
        let hdrs = out.headers_mut().unwrap();
        for (k, v) in &parts.headers {
            let name = k.as_str();
            if DROP.contains(&name) {
                continue;
            }
            if name == "location" {
                if let Some(to) = v.to_str().ok().map(|l| self.location(l)).and_then(|l| HeaderValue::from_str(&l).ok()) {
                    hdrs.append(header::LOCATION, to);
                }
                continue;
            }
            hdrs.append(k.clone(), v.clone());
        }
        for c in parts.headers.get_all(header::SET_COOKIE).iter().filter_map(|v| v.to_str().ok()) {
            if let Ok(v) = HeaderValue::from_str(&cookie_path(c, &self.prefix, base_path)) {
                hdrs.append(header::SET_COOKIE, v);
            }
        }
        hdrs.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("SAMEORIGIN"));
        out.body(Body::from(bytes)).map_err(|e| e.to_string())
    }

    /// A redirect's target, through the proxy when it is the app's.
    fn location(&self, l: &str) -> String {
        let (origin, base_path) = self.origin();
        let port = origin.rsplit(':').next().unwrap_or("");
        let mut path = l;
        for host in ["127.0.0.1", "localhost"] {
            if let Some(p) = l.strip_prefix(&format!("http://{host}:{port}")) {
                path = if p.is_empty() { "/" } else { p };
            }
        }
        if path.starts_with('/') && !path.starts_with("//") {
            proxied(path, &self.prefix, base_path)
        } else {
            l.to_string()
        }
    }
}

/// An absolute path of the app as Workbench serves it: under the prefix,
/// without the app's base path.
fn proxied(path: &str, prefix: &str, base_path: &str) -> String {
    if path.starts_with(prefix) {
        return path.to_string();
    }
    let p = if !base_path.is_empty() && (path == base_path || path.starts_with(&format!("{base_path}/"))) { &path[base_path.len()..] } else { path };
    format!("{}{}", prefix.trim_end_matches('/'), if p.is_empty() { "/" } else { p })
}

/// An app's cookie, kept to its preview: no `Domain`, its `Path` under
/// the prefix.
fn cookie_path(c: &str, prefix: &str, base_path: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut has_path = false;
    for (i, p) in c.split(';').enumerate() {
        let t = p.trim();
        let lower = t.to_ascii_lowercase();
        if i > 0 && lower.starts_with("domain=") {
            continue;
        }
        if i > 0 && lower.starts_with("path=") {
            has_path = true;
            parts.push(format!("Path={}", proxied(&t[5..], prefix, base_path)));
            continue;
        }
        parts.push(t.to_string());
    }
    if !has_path {
        parts.push(format!("Path={prefix}"));
    }
    parts.join("; ")
}

/// An HTML answer with the overlay's script before `</body>` (or at the
/// end), and its absolute-path links (`src`, `href`, `action`,
/// `formaction`, `poster`) through the proxy.
pub fn inject(body: &[u8], prefix: &str, base_path: &str) -> Vec<u8> {
    let Ok(html) = std::str::from_utf8(body) else {
        let mut out = body.to_vec();
        out.extend_from_slice(INJECT.as_bytes());
        return out;
    };
    let mut html = rewrite_links(html, prefix, base_path);
    let lower = html.to_ascii_lowercase();
    match lower.rfind("</body>").or_else(|| lower.rfind("</html>")) {
        Some(i) => html.insert_str(i, INJECT),
        None => html.push_str(INJECT),
    }
    html.into_bytes()
}

fn rewrite_links(html: &str, prefix: &str, base_path: &str) -> String {
    const ATTRS: [&str; 5] = ["src", "href", "action", "formaction", "poster"];
    let b = html.as_bytes();
    let lower = html.to_ascii_lowercase();
    let lb = lower.as_bytes();
    let mut out = String::with_capacity(html.len() + 256);
    let mut last = 0;
    let mut i = 0;
    while i < b.len() {
        let at_word = i > 0 && b[i - 1].is_ascii_whitespace();
        let attr = if at_word { ATTRS.iter().find(|a| lb[i..].starts_with(a.as_bytes())) } else { None };
        let Some(attr) = attr else {
            i += 1;
            continue;
        };
        let mut j = i + attr.len();
        while j < b.len() && b[j].is_ascii_whitespace() {
            j += 1;
        }
        if j >= b.len() || b[j] != b'=' {
            i += 1;
            continue;
        }
        j += 1;
        while j < b.len() && b[j].is_ascii_whitespace() {
            j += 1;
        }
        let quote = b.get(j).copied().filter(|q| *q == b'"' || *q == b'\'');
        let start = if quote.is_some() { j + 1 } else { j };
        let end = match quote {
            Some(q) => html[start..].find(q as char).map_or(b.len(), |k| start + k),
            None => html[start..].find(|c: char| c.is_ascii_whitespace() || c == '>').map_or(b.len(), |k| start + k),
        };
        let value = &html[start..end];
        if value.starts_with('/') && !value.starts_with("//") && !value.starts_with(prefix) {
            out.push_str(&html[last..start]);
            out.push_str(&proxied(value, prefix, base_path));
            last = end;
        }
        i = end.max(i + 1);
    }
    out.push_str(&html[last..]);
    out
}

// Routing a pick to the agent that made it.

#[derive(serde::Deserialize)]
struct RouteB {
    agent: Option<String>,
    pick: Option<Json>,
}

async fn route_pick(State(app): State<App>, body: Bytes) -> ApiResult {
    let b: RouteB = parse_body(&body)?;
    let agent = b.agent.filter(|a| !a.is_empty()).ok_or_else(|| ApiError::bad("agent is required: the preview's agent"))?;
    let pick = b.pick.filter(Json::is_object).ok_or_else(|| ApiError::bad("pick is required: what the overlay sent (selector, text, html)"))?;
    let sup = app.sup.clone();
    Ok(AxJson(blocking(move || route(&sup, &agent, &pick)).await?).into_response())
}

/// The words of a pick to look for in diffs: its text's lines, and its
/// class names and id.
fn needles(pick: &Json) -> (Vec<String>, Vec<String>) {
    let s = |k: &str| pick.get(k).and_then(Json::as_str).unwrap_or("");
    let texts: Vec<String> = s("text").lines().map(str::trim).filter(|l| l.chars().count() >= 3).take(8).map(|l| l.chars().take(120).collect()).collect();
    let mut names: Vec<String> = Vec::new();
    let html = s("html");
    let first_tag = html.split('>').next().unwrap_or("");
    for attr in ["class=", "id="] {
        if let Some(i) = first_tag.find(attr) {
            let v = &first_tag[i + attr.len()..];
            let q = v.chars().next().filter(|c| *c == '"' || *c == '\'');
            let v = match q {
                Some(q) => v[1..].split(q).next().unwrap_or(""),
                None => v.split(|c: char| c.is_whitespace() || c == '>').next().unwrap_or(""),
            };
            names.extend(v.split_whitespace().map(str::to_string));
        }
    }
    for part in s("selector").split(|c: char| c.is_whitespace() || c == '>') {
        for (i, seg) in part.split(['.', '#']).enumerate() {
            if i > 0 {
                names.push(seg.split([':', '[']).next().unwrap_or("").to_string());
            }
        }
    }
    names.retain(|n| n.len() >= 3);
    names.sort();
    names.dedup();
    (texts, names)
}

fn has_word(line: &str, w: &str) -> bool {
    let word = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
    line.match_indices(w).any(|(i, _)| !line[..i].chars().next_back().is_some_and(word) && !line[i + w.len()..].chars().next().is_some_and(word))
}

/// `POST /api/preview/route`: the agent whose diff holds the picked
/// element's text or class names (else the preview's own agent), the
/// files where, ChloeCtx's records about those files, and the message to
/// send the owner.
pub fn route(sup: &Supervisor, preview_agent: &str, pick: &Json) -> Result<Json, ApiError> {
    let own = sup.agent_row(preview_agent).ok_or_else(|| ApiError::not_found(format!("no agent {preview_agent}")))?;
    let main = map::main_of(&own.repo).unwrap_or_else(|_| PathBuf::from(&own.repo));
    let (texts, names) = needles(pick);
    let mut best: Option<(i64, String, Vec<(String, i64)>, Vec<Json>)> = None;
    for a in map::repo_agents(sup, &main) {
        let cwd = PathBuf::from(&a.cwd);
        let Some(base) = a.base.clone().or_else(|| git::rev_parse(&cwd, "HEAD")) else { continue };
        let Ok(files) = git::diff(&cwd, &base) else { continue };
        let mut total = 0;
        let mut per_file: Vec<(String, i64)> = Vec::new();
        let mut matched: Vec<Json> = Vec::new();
        for f in &files {
            let path = f["path"].as_str().unwrap_or("").to_string();
            let mut score = 0;
            for l in f["hunks"].as_array().into_iter().flatten().flat_map(|h| h["lines"].as_array().into_iter().flatten()).filter(|l| l["kind"] == "+") {
                let text = l["text"].as_str().unwrap_or("");
                let s = texts.iter().filter(|t| text.contains(t.as_str())).count() as i64 * 3 + names.iter().filter(|n| has_word(text, n)).count() as i64;
                if s > 0 {
                    score += s;
                    if matched.len() < 20 {
                        matched.push(json!({ "path": path, "line": l["new_line"], "text": text }));
                    }
                }
            }
            if score > 0 {
                per_file.push((path, score));
            }
            total += score;
        }
        // Ties go to the preview's own agent.
        let better = match &best {
            None => total > 0,
            Some((s, id, ..)) => total > *s || (total == *s && total > 0 && a.id == own.id && *id != own.id),
        };
        if better {
            per_file.sort_by(|x, y| y.1.cmp(&x.1).then(x.0.cmp(&y.0)));
            best = Some((total, a.id.clone(), per_file, matched));
        }
    }
    let (score, owner, files, matched) = best.unwrap_or((0, own.id.clone(), Vec::new(), Vec::new()));
    let files: Vec<String> = files.into_iter().map(|f| f.0).collect();
    let mut missing = Vec::new();
    let related: Vec<Json> = if files.is_empty() {
        Vec::new()
    } else {
        match map::file_records(sup, &main) {
            Ok(recs) => recs.iter().filter(|r| r.files.iter().any(|f| files.contains(&map::split_line(f).0))).take(20).map(|r| r.to_json()).collect(),
            Err(e) => {
                missing.push(e);
                Vec::new()
            }
        }
    };
    let s = |k: &str| pick.get(k).and_then(Json::as_str).unwrap_or("").to_string();
    let html: String = s("html").chars().take(2048).collect();
    let message = format!(
        "In the preview of {}, the user picked the element `{}` (text: {:?}). Its HTML:\n```html\n{}\n```",
        own.name,
        s("selector"),
        s("text").chars().take(300).collect::<String>(),
        html
    );
    Ok(json!({
        "owner": trail::agent_json(sup, &sup.agent_row(&owner).unwrap_or(own)),
        "files": files,
        "related": related,
        "matched": matched,
        "score": score,
        "message": message,
        "missing": missing,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_preview_urls() {
        assert_eq!(check_url("http://127.0.0.1:5173/", Some(7740)).unwrap(), "http://127.0.0.1:5173");
        assert_eq!(check_url("http://localhost:3000/app/", None).unwrap(), "http://localhost:3000/app");
        for bad in ["https://127.0.0.1:5173", "http://10.0.0.1:80", "http://127.0.0.1", "http://evil@127.0.0.1:80", "http://127.0.0.1:7740", "http://127.0.0.1:0", "http://127.0.0.1:80/a?b", "http://127.0.0.1:80/../x", "http://127.0.0.1.evil:80"] {
            assert!(check_url(bad, Some(7740)).is_err(), "{bad}");
        }
    }

    #[test]
    fn injects_the_overlay_and_rewrites_links() {
        let html = "<html><head><link href=\"/a.css\" rel=stylesheet><script src='/x.js'></script></head><body><a href=\"//cdn/x\">c</a><a href=/b>b</a><img src=\"rel.png\"><form action=\"/p/form\"></form></BODY></html>";
        let out = String::from_utf8(inject(html.as_bytes(), "/preview/a1/", "")).unwrap();
        assert!(out.contains("href=\"/preview/a1/a.css\""), "{out}");
        assert!(out.contains("src='/preview/a1/x.js'"), "{out}");
        assert!(out.contains("href=\"//cdn/x\""), "{out}");
        assert!(out.contains("href=/preview/a1/b>"), "{out}");
        assert!(out.contains("src=\"rel.png\""), "{out}");
        assert!(out.contains("action=\"/preview/a1/p/form\""), "{out}");
        assert!(out.ends_with("<script src=\"/preview-overlay.js\"></script></BODY></html>"), "{out}");
        // Under a base path, and links already through the proxy.
        assert_eq!(proxied("/app/x", "/preview/a1/", "/app"), "/preview/a1/x");
        assert_eq!(proxied("/preview/a1/x", "/preview/a1/", ""), "/preview/a1/x");
        assert_eq!(cookie_path("sid=1; Path=/; Domain=localhost; HttpOnly", "/preview/a1/", ""), "sid=1; Path=/preview/a1/; HttpOnly");
        assert_eq!(cookie_path("sid=1", "/preview/a1/", ""), "sid=1; Path=/preview/a1/");
    }

    #[test]
    fn needles_of_a_pick() {
        let (t, n) = needles(&json!({ "text": "Refund\n  ok", "html": "<button class=\"btn refund-btn\" id=\"go\">Refund</button>", "selector": "div.row > button.refund-btn:nth-of-type(2)" }));
        assert_eq!(t, ["Refund"]);
        assert_eq!(n, ["btn", "refund-btn", "row"]);
        assert!(has_word("<b class=\"refund-btn\">", "refund-btn"));
        assert!(!has_word("refund-btn-x", "refund-btn"));
    }
}
