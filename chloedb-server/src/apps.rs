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
//! `<path>`, with the app's token (`Authorization: Bearer`) in place of
//! the caller's key: `CHLOEDB_APP_TOKEN_<NAME>` (the name in capitals, `-`
//! as `_`), else the server's `CHLOEDB_APP_TOKEN`, so that each app can
//! hold a token of its own and none can call another as the server. So an
//! app needs no address that browsers reach: the server checks the
//! caller's key (read scope for `GET`, admin for the rest), and tells the
//! app who it was in headers it sets itself, which the app may trust
//! because they come with its token: `x-chloedb-principal` (the key's
//! name, `token` or `open`), `x-chloedb-key` (its id), `x-chloedb-scope`
//! and, for a key limited to tables, `x-chloedb-tables`
//! (`crawl_*=admin,*=read`). A caller's own headers of those names are
//! never passed on.

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
    /// The token sent to this app in place of the server's
    /// (`CHLOEDB_APP_TOKEN_<NAME>`).
    pub token: Option<String>,
}

/// The variable that holds app `name`'s own token: `CHLOEDB_APP_TOKEN_`
/// and its name in capitals, `-` as `_`.
pub fn token_var(name: &str) -> String {
    format!("CHLOEDB_APP_TOKEN_{}", name.to_ascii_uppercase().replace('-', "_"))
}

/// `name=url` for each `--app`, each with its own token when the
/// environment holds one ([`token_var`]).
pub fn parse(specs: &[String]) -> Result<Vec<App>, String> {
    parse_with(specs, |v| std::env::var(v).ok())
}

/// [`parse`], the variables read by `var`.
pub fn parse_with(specs: &[String], var: impl Fn(&str) -> Option<String>) -> Result<Vec<App>, String> {
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
        let token = var(&token_var(name)).map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
        out.push(App { name: name.to_string(), url: url.to_string(), token });
    }
    Ok(out)
}

pub struct Apps {
    pub list: Vec<App>,
    token: Option<String>,
    /// A folder of each app's own token, in a file of its name
    /// (`--app-token-dir`).
    token_dir: Option<std::path::PathBuf>,
    http: Option<reqwest::Client>,
    /// Each app's manifest, read at most every 15 s.
    manifests: Mutex<HashMap<String, (Instant, Json)>>,
}

impl Default for Apps {
    fn default() -> Apps {
        Apps::new(Vec::new(), None)
    }
}

impl Apps {
    pub fn new(list: Vec<App>, token: Option<String>) -> Apps {
        let http = (!list.is_empty()).then(|| crate::http_client().connect_timeout(Duration::from_secs(5)).build().expect("an HTTP client"));
        Apps { list, token: token.filter(|t| !t.is_empty()), token_dir: None, http, manifests: Mutex::new(HashMap::new()) }
    }

    /// These apps, each one's own token read from `dir` (a file of its
    /// name) when it holds one.
    pub fn with_token_dir(mut self, dir: Option<std::path::PathBuf>) -> Apps {
        self.token_dir = dir;
        self
    }

    fn find(&self, name: &str) -> Option<&App> {
        self.list.iter().find(|a| a.name == name)
    }

    /// The token app `a` is sent: its own (its file in the token folder,
    /// read now, else its variable), else the server's.
    fn token_of(&self, a: &App) -> Option<String> {
        let file = self.token_dir.as_ref().and_then(|d| std::fs::read_to_string(d.join(&a.name)).ok()).map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
        file.or_else(|| a.token.clone()).or_else(|| self.token.clone())
    }

    /// `GET /v1/apps`: each app with its manifest, or why it does not answer.
    pub async fn listing(&self) -> Json {
        let mut out = Vec::new();
        for a in &self.list {
            let m = self.manifest(a).await;
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
                return Ok(m.clone());
            }
        }
        let http = self.http.as_ref().ok_or("no apps")?;
        let mut req = http.get(format!("{}/app.json", a.url)).timeout(Duration::from_secs(5));
        if let Some(t) = self.token_of(a) {
            req = req.bearer_auth(t);
        }
        let r = req.send().await.map_err(|e| format!("{} does not answer: {e}", a.url))?;
        if !r.status().is_success() {
            return Err(format!("{}/app.json answers {}", a.url, r.status()));
        }
        let m: Json = r.json().await.map_err(|e| format!("{}/app.json is not JSON: {e}", a.url))?;
        self.manifests.lock().unwrap_or_else(|e| e.into_inner()).insert(a.name.clone(), (Instant::now(), m.clone()));
        Ok(m)
    }

    /// `/v1/apps/<name>/<rest>`: the request, sent on to the app, with who
    /// made it (`who`) in its headers.
    #[allow(clippy::too_many_arguments)]
    pub async fn forward(&self, name: &str, who: &crate::keys::Principal, method: Method, rest: &str, query: Option<&str>, headers: &HeaderMap, body: Bytes) -> Response {
        let err = |status: StatusCode, m: String| (status, axum::Json(json!({ "error": m }))).into_response();
        let Some(a) = self.find(name) else {
            return err(StatusCode::NOT_FOUND, format!("no app {name}: this server's are {}", if self.list.is_empty() { "none (--app)".into() } else { self.list.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ") }));
        };
        if rest.split('/').any(|s| s == ".." || s == ".") {
            return err(StatusCode::BAD_REQUEST, "a path of an app has no . or .. in it".into());
        }
        let Some(http) = &self.http else { return err(StatusCode::NOT_FOUND, "no apps".into()) };
        let url = match query {
            Some(q) if !q.is_empty() => format!("{}/{}?{q}", a.url, rest.trim_start_matches('/')),
            _ => format!("{}/{}", a.url, rest.trim_start_matches('/')),
        };
        let mut req = http.request(method, &url).timeout(Duration::from_secs(120));
        for h in [header::CONTENT_TYPE, header::ACCEPT] {
            if let Some(v) = headers.get(&h) {
                req = req.header(h, v);
            }
        }
        if let Some(t) = self.token_of(a) {
            req = req.bearer_auth(t);
        }
        for (h, v) in caller_headers(who) {
            req = req.header(h, v);
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

/// The headers that tell an app who called it through the server
/// ([`Apps::forward`]): only these are sent of the caller, so a caller's
/// own `x-chloedb-principal` never reaches the app.
pub fn caller_headers(who: &crate::keys::Principal) -> Vec<(&'static str, String)> {
    // A header holds visible ASCII: a key's name is kept to that.
    let clean = |s: &str| s.chars().map(|c| if c.is_ascii_graphic() || c == ' ' { c } else { '?' }).collect::<String>();
    let mut out = vec![("x-chloedb-principal", clean(&who.name)), ("x-chloedb-scope", who.scope.name().to_string())];
    if let Some(k) = &who.key {
        out.push(("x-chloedb-key", clean(k)));
    }
    if let Some(t) = &who.tables {
        out.push(("x-chloedb-tables", t.iter().map(|g| format!("{}={}", g.table, g.scope.name())).collect::<Vec<_>>().join(",")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_apps() {
        let a = parse_with(&["crawler=http://127.0.0.1:7800/".into(), " x-2 = https://h ".into()], |_| None).unwrap();
        assert_eq!(a, vec![App { name: "crawler".into(), url: "http://127.0.0.1:7800".into(), token: None }, App { name: "x-2".into(), url: "https://h".into(), token: None }]);
        // Each app's own token, by its variable.
        let a = parse_with(&["crawler=http://c".into(), "x-2=http://x".into()], |v| (v == "CHLOEDB_APP_TOKEN_X_2").then(|| "t-x2".to_string())).unwrap();
        assert_eq!((a[0].token.as_deref(), a[1].token.as_deref()), (None, Some("t-x2")));
        let apps = Apps::new(a, Some("shared".into()));
        assert_eq!((apps.token_of(&apps.list[0]).as_deref(), apps.token_of(&apps.list[1]).as_deref()), (Some("shared"), Some("t-x2")));
        // A file of the token folder, read as it is sent: written later,
        // it is taken without a restart.
        let dir = tempfile::tempdir().unwrap();
        let apps = apps.with_token_dir(Some(dir.path().to_path_buf()));
        assert_eq!(apps.token_of(&apps.list[0]).as_deref(), Some("shared"));
        std::fs::write(dir.path().join("crawler"), "t-crawler\n").unwrap();
        assert_eq!(apps.token_of(&apps.list[0]).as_deref(), Some("t-crawler"));
        assert!(parse(&["crawler".into()]).is_err());
        assert!(parse(&["Crawler=http://h".into()]).is_err());
        assert!(parse(&["c=ftp://h".into()]).is_err());
        assert!(parse(&["c=http://a".into(), "c=http://b".into()]).is_err());
    }
}

#[cfg(test)]
mod caller_tests {
    use super::*;
    use crate::keys::{Grant, Principal, Scope};

    #[test]
    fn an_app_is_told_who_called() {
        let p = Principal { name: "ana\nx".into(), key: Some("0a1b".into()), scope: Scope::Write, root: false, tables: Some(vec![Grant::parse("sheets_*=write").unwrap()].into()) };
        let h = caller_headers(&p);
        assert!(h.contains(&("x-chloedb-principal", "ana?x".into())));
        assert!(h.contains(&("x-chloedb-scope", "write".into())) && h.contains(&("x-chloedb-key", "0a1b".into())));
        assert!(h.contains(&("x-chloedb-tables", "sheets_*=write".into())));
        assert_eq!(caller_headers(&Principal::open()).len(), 2);
    }
}
