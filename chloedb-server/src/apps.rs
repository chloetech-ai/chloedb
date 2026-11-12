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
//! The server also starts apps itself (`--start-apps`, [`crate::app_procs`]):
//! those are listed here too, each with a token of its own ([`App::token`])
//! in place of `CHLOEDB_APP_TOKEN`, and with their process's state.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value as Json};

use crate::app_procs::{AppKeys, Plan, Proc};

/// The most an app's answer may be.
const MAX_ANSWER: usize = 32 << 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct App {
    pub name: String,
    pub url: String,
    /// The token sent to this app in place of the server's
    /// `CHLOEDB_APP_TOKEN`: an app the server starts has one of its own
    /// ([`crate::app_procs`]).
    pub token: Option<String>,
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
        out.push(App { name: name.to_string(), url: url.to_string(), token: None });
    }
    Ok(out)
}

pub struct Apps {
    pub list: Vec<App>,
    token: Option<String>,
    http: Option<reqwest::Client>,
    /// Each app's manifest, read at most every 15 s.
    manifests: Mutex<HashMap<String, (Instant, Json)>>,
    /// The apps this server runs as child processes ([`crate::app_procs`]).
    pub procs: Vec<Arc<Proc>>,
}

impl Default for Apps {
    fn default() -> Apps {
        Apps::new(Vec::new(), None)
    }
}

impl Apps {
    pub fn new(list: Vec<App>, token: Option<String>) -> Apps {
        let http = (!list.is_empty()).then(|| crate::http_client().connect_timeout(Duration::from_secs(5)).build().expect("an HTTP client"));
        Apps { list, token: token.filter(|t| !t.is_empty()), http, manifests: Mutex::new(HashMap::new()), procs: Vec::new() }
    }

    /// These apps and the ones the server starts (`plans`), which are
    /// listed after the others under their own names and tokens.
    pub fn with_started(list: Vec<App>, token: Option<String>, plans: Vec<Plan>) -> Apps {
        let mut list = list;
        list.extend(plans.iter().map(|p| App { name: p.name.clone(), url: p.url(), token: Some(p.token.clone()) }));
        let mut apps = Apps::new(list, token);
        apps.procs = plans.into_iter().map(Proc::new).collect();
        apps
    }

    /// Starts the apps' processes, against the server at `server`.
    pub fn start(&self, server: &str, keys: Arc<dyn AppKeys>) {
        for p in &self.procs {
            tokio::spawn(p.clone().supervise(server.to_string(), keys.clone()));
        }
    }

    /// Stops the apps' processes.
    pub async fn stop(&self) {
        futures::future::join_all(self.procs.iter().map(|p| p.stop())).await;
    }

    fn token_of<'a>(&'a self, a: &'a App) -> Option<&'a String> {
        a.token.as_ref().or(self.token.as_ref())
    }

    fn proc_of(&self, name: &str) -> Option<&Arc<Proc>> {
        self.procs.iter().find(|p| p.plan.name == name)
    }

    fn find(&self, name: &str) -> Option<&App> {
        self.list.iter().find(|a| a.name == name)
    }

    /// `GET /v1/apps`: each app with its manifest, or why it does not answer.
    pub async fn listing(&self) -> Json {
        let mut out = Vec::new();
        for a in &self.list {
            let m = self.manifest(a).await;
            let process = self.proc_of(&a.name).map(|p| p.json());
            let mut entry = match m {
                Ok(m) => json!({
                    "name": a.name, "url": a.url, "up": true,
                    "title": m["title"].as_str().unwrap_or(&a.name), "description": m["description"], "icon_svg": m["icon_svg"],
                    "view": m["view"].as_str().map(|v| format!("/v1/apps/{}/{}", a.name, v.trim_start_matches('/'))), "version": m["version"],
                }),
                Err(e) => json!({"name": a.name, "url": a.url, "up": false, "title": a.name, "error": e}),
            };
            if let Some(p) = process {
                entry["process"] = p;
            }
            out.push(entry);
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

    /// `/v1/apps/<name>/<rest>`: the request, sent on to the app.
    pub async fn forward(&self, name: &str, method: Method, rest: &str, query: Option<&str>, headers: &HeaderMap, body: Bytes) -> Response {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_apps() {
        let a = parse(&["crawler=http://127.0.0.1:7800/".into(), " x-2 = https://h ".into()]).unwrap();
        assert_eq!(a, vec![App { name: "crawler".into(), url: "http://127.0.0.1:7800".into(), token: None }, App { name: "x-2".into(), url: "https://h".into(), token: None }]);
        assert!(parse(&["crawler".into()]).is_err());
        assert!(parse(&["Crawler=http://h".into()]).is_err());
        assert!(parse(&["c=ftp://h".into()]).is_err());
        assert!(parse(&["c=http://a".into(), "c=http://b".into()]).is_err());
    }
}
