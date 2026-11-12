//! Apps: programs of their own that the command center shows as views
//! (`--app crawler=http://127.0.0.1:7800`). An app answers
//!
//! - `GET /app.json`: its name, title, icon (`icon_svg`, the inside of a
//!   24-unit outline icon), and `view`, the path of its view module;
//! - its view module: JavaScript the command center loads and runs with
//!   its own helpers (the app's code is trusted as the server's is: an
//!   admin names it on the server's command line);
//! - its own API, which the view calls.
//!
//! The browser reaches an app only through this server: `GET /v1/apps`
//! lists them, and `/v1/apps/<name>/<path>` is forwarded to the app's
//! `<path>`, with the server's `CHLOEDB_APP_TOKEN` (`Authorization:
//! Bearer`) in place of the caller's key. So an app needs no address
//! that browsers reach, and no keys of its own: the server checks the
//! caller's (read scope for `GET`, admin for the rest).
//!
//! The path is the app's own: `<path>` is joined to the app's URL segment
//! by segment, each encoded again, so no `..` (written `%2e%2e`, once or
//! twice encoded) climbs above the app's URL and no decoded `?` or `#`
//! starts a query (audit D-B9, G-B17). The listing asks every app at once
//! and keeps a failure as long as a manifest, so an app that does not
//! answer neither slows the others nor is waited for at every page (G-B10).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value as Json};

/// The most an app's answer may be.
const MAX_ANSWER: usize = 32 << 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct App {
    pub name: String,
    pub url: String,
}

/// `name=url` for each `--app`.
pub fn parse(specs: &[String]) -> Result<Vec<App>, String> {
    let mut out: Vec<App> = Vec::new();
    for s in specs.iter().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        let (name, url) = s.split_once('=').ok_or_else(|| format!("--app {s:?}: name=url, as crawler=http://127.0.0.1:7800"))?;
        let (name, url) = (name.trim(), url.trim().trim_end_matches('/'));
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_') {
            return Err(format!("--app {s:?}: a name is lowercase letters, digits, - and _"));
        }
        if !(url.starts_with("http://") || url.starts_with("https://")) || reqwest::Url::parse(url).is_err() {
            return Err(format!("--app {s:?}: {url:?} is not an http(s) URL"));
        }
        if out.iter().any(|a| a.name == name) {
            return Err(format!("--app: two apps named {name}"));
        }
        out.push(App { name: name.to_string(), url: url.to_string() });
    }
    Ok(out)
}

pub struct Apps {
    pub list: Vec<App>,
    token: Option<String>,
    http: Option<reqwest::Client>,
    /// Each app's manifest, or why it could not be read, read at most every
    /// 15 s.
    manifests: Mutex<HashMap<String, (Instant, Result<Json, String>)>>,
}

impl Default for Apps {
    fn default() -> Apps {
        Apps::new(Vec::new(), None)
    }
}

impl Apps {
    pub fn new(list: Vec<App>, token: Option<String>) -> Apps {
        let http = (!list.is_empty()).then(|| crate::http_client().connect_timeout(Duration::from_secs(5)).build().expect("an HTTP client"));
        Apps { list, token: token.filter(|t| !t.is_empty()), http, manifests: Mutex::new(HashMap::new()) }
    }

    fn find(&self, name: &str) -> Option<&App> {
        self.list.iter().find(|a| a.name == name)
    }

    /// `GET /v1/apps`: each app with its manifest, or why it does not answer.
    pub async fn listing(&self) -> Json {
        let manifests = futures::future::join_all(self.list.iter().map(|a| self.manifest(a))).await;
        let mut out = Vec::new();
        for (a, m) in self.list.iter().zip(manifests) {
            out.push(match m {
                Ok(m) => json!({
                    "name": a.name, "url": a.url, "up": true,
                    "title": m["title"].as_str().unwrap_or(&a.name), "description": m["description"], "icon_svg": m["icon_svg"],
                    "view": m["view"].as_str().map(|v| format!("/v1/apps/{}/{}", a.name, v.trim_start_matches('/'))), "version": m["version"],
                }),
                Err(e) => json!({"name": a.name, "url": a.url, "up": false, "title": a.name, "error": e}),
            });
        }
        json!({ "apps": out })
    }

    async fn manifest(&self, a: &App) -> Result<Json, String> {
        if let Some((at, m)) = self.manifests.lock().unwrap_or_else(|e| e.into_inner()).get(&a.name) {
            if at.elapsed() < Duration::from_secs(15) {
                return m.clone();
            }
        }
        let m = self.read_manifest(a).await;
        self.manifests.lock().unwrap_or_else(|e| e.into_inner()).insert(a.name.clone(), (Instant::now(), m.clone()));
        m
    }

    async fn read_manifest(&self, a: &App) -> Result<Json, String> {
        let http = self.http.as_ref().ok_or("no apps")?;
        let mut req = http.get(format!("{}/app.json", a.url)).timeout(Duration::from_secs(5));
        if let Some(t) = &self.token {
            req = req.bearer_auth(t);
        }
        let r = req.send().await.map_err(|e| format!("{} does not answer: {e}", a.url))?;
        if !r.status().is_success() {
            return Err(format!("{}/app.json answers {}", a.url, r.status()));
        }
        r.json().await.map_err(|e| format!("{}/app.json is not JSON: {e}", a.url))
    }

    /// `/v1/apps/<name>/<rest>`: the request, sent on to the app.
    pub async fn forward(&self, name: &str, method: Method, rest: &str, query: Option<&str>, headers: &HeaderMap, body: Bytes) -> Response {
        let err = |status: StatusCode, m: String| (status, axum::Json(json!({ "error": m }))).into_response();
        let Some(a) = self.find(name) else {
            return err(StatusCode::NOT_FOUND, format!("no app {name}: this server's are {}", if self.list.is_empty() { "none (--app)".into() } else { self.list.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ") }));
        };
        let url = match app_url(&a.url, rest, query) {
            Ok(u) => u,
            Err(m) => return err(StatusCode::BAD_REQUEST, m),
        };
        let Some(http) = &self.http else { return err(StatusCode::NOT_FOUND, "no apps".into()) };
        let mut req = http.request(method, url).timeout(Duration::from_secs(120));
        for h in [header::CONTENT_TYPE, header::ACCEPT] {
            if let Some(v) = headers.get(&h) {
                req = req.header(h, v);
            }
        }
        if let Some(t) = &self.token {
            req = req.bearer_auth(t);
        }
        let r = match req.body(body).send().await {
            Ok(r) => r,
            Err(e) => return err(StatusCode::BAD_GATEWAY, format!("the app {name} ({}) does not answer: {e}", a.url)),
        };
        let status = StatusCode::from_u16(r.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        let ctype = r.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("application/octet-stream").to_string();
        if r.content_length().is_some_and(|n| n as usize > MAX_ANSWER) {
            return err(StatusCode::BAD_GATEWAY, format!("the app {name}'s answer is larger than {} MB", MAX_ANSWER >> 20));
        }
        match r.bytes().await {
            Ok(b) if b.len() <= MAX_ANSWER => (status, [(header::CONTENT_TYPE, ctype), (header::CACHE_CONTROL, "no-cache".to_string())], b).into_response(),
            Ok(_) => err(StatusCode::BAD_GATEWAY, format!("the app {name}'s answer is larger than {} MB", MAX_ANSWER >> 20)),
            Err(e) => err(StatusCode::BAD_GATEWAY, format!("the app {name}'s answer was cut off: {e}")),
        }
    }
}

/// Whether a path segment (as the router decoded it) is a dot segment, or
/// one once decoded again (`%2e`, `%2E`, `%252e`...).
pub fn dot_segment(seg: &str) -> bool {
    let mut s = seg.to_string();
    for _ in 0..4 {
        if s == "." || s == ".." {
            return true;
        }
        let next = percent_decode(&s);
        if next == s {
            return false;
        }
        s = next;
    }
    true
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The app's URL for `rest` (the decoded path after `/v1/apps/<name>/`) and
/// the request's query as it came: `rest`'s segments added to the app's
/// path, each encoded, so the URL stays under the app's.
pub fn app_url(base: &str, rest: &str, query: Option<&str>) -> Result<reqwest::Url, String> {
    let segs: Vec<&str> = rest.trim_start_matches('/').split('/').collect();
    if segs.iter().any(|s| dot_segment(s)) {
        return Err("a path of an app has no . or .. in it".into());
    }
    let mut u = reqwest::Url::parse(base).map_err(|e| format!("the app's URL {base}: {e}"))?;
    let prefix = u.path().trim_end_matches('/').to_string();
    u.path_segments_mut().map_err(|_| format!("the app's URL {base} has no path"))?.pop_if_empty().extend(segs);
    u.set_query(query.filter(|q| !q.is_empty()));
    u.set_fragment(None);
    if !u.path().starts_with(&format!("{prefix}/")) {
        return Err(format!("{rest} is not a path of the app"));
    }
    Ok(u)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_apps() {
        let a = parse(&["crawler=http://127.0.0.1:7800/".into(), " x-2 = https://h ".into()]).unwrap();
        assert_eq!(a, vec![App { name: "crawler".into(), url: "http://127.0.0.1:7800".into() }, App { name: "x-2".into(), url: "https://h".into() }]);
        assert!(parse(&["crawler".into()]).is_err());
        assert!(parse(&["Crawler=http://h".into()]).is_err());
        assert!(parse(&["c=ftp://h".into()]).is_err());
        assert!(parse(&["c=http://a".into(), "c=http://b".into()]).is_err());
    }

    #[test]
    fn an_apps_path_stays_under_its_url() {
        let u = |base: &str, rest: &str, q: Option<&str>| app_url(base, rest, q).map(|u| u.to_string());
        assert_eq!(u("http://h:1", "v1/crawls/c1", Some("x=1")).unwrap(), "http://h:1/v1/crawls/c1?x=1");
        assert_eq!(u("http://h:1/tools/x", "ui/view.js", None).unwrap(), "http://h:1/tools/x/ui/view.js");
        assert_eq!(u("http://h:1/tools/x/", "a", None).unwrap(), "http://h:1/tools/x/a");
        // The router decodes once: %252e%252e arrives as %2e%2e.
        for rest in ["..", "a/../b", ".", "%2e%2e/admin", "a/%2E%2e/b", "%252e%252e", "%2e"] {
            assert!(app_url("http://h:1/tools/x", rest, None).is_err(), "{rest}");
        }
        // A decoded ? or # stays in its segment.
        assert_eq!(u("http://h:1/tools/x", "a?admin=1#y", None).unwrap(), "http://h:1/tools/x/a%3Fadmin=1%23y");
        assert_eq!(u("http://h:1", "a\\..\\b", None).unwrap(), "http://h:1/a%5C..%5Cb");
        assert!(!dot_segment("...") && !dot_segment("a.") && !dot_segment("%2"));
    }
}
