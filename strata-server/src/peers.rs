//! The coordinators of one cluster, which know each other.
//!
//! STUB (alerts2, round 2): the shared contract's signatures, so that the
//! alerts can be built and tested before the fleet branch lands. The lead
//! drops this commit ("peers: stub") at merge and keeps fleet's
//! `peers.rs`. Here a coordinator knows only itself, and, for tests on one
//! machine, the coordinators listed in the environment variable
//! `STRATA_PEERS` (comma-separated HTTP bases); it calls them with its own
//! root token.

use serde_json::Value as Json;

use crate::http::App;
use crate::telemetry::now_ms;

#[derive(Clone, Debug, serde::Serialize)]
pub struct Peer {
    /// Its HTTP base, e.g. `http://10.0.0.5:7700`.
    pub url: String,
    pub name: String,
    /// This coordinator.
    pub this: bool,
    pub last_seen_ms: u64,
}

/// Every coordinator of this cluster this one knows, itself included.
pub(crate) fn coordinators(app: &App) -> Vec<Peer> {
    let me = app.svc.identity.public_url.clone().unwrap_or_else(|| "http://this".into());
    let mut v = vec![Peer { name: me.clone(), url: me.trim_end_matches('/').to_string(), this: true, last_seen_ms: now_ms() }];
    for u in std::env::var("STRATA_PEERS").unwrap_or_default().split(',').map(|u| u.trim().trim_end_matches('/')).filter(|u| !u.is_empty()) {
        if !v.iter().any(|p| p.url == u) {
            v.push(Peer { url: u.to_string(), name: u.to_string(), this: false, last_seen_ms: now_ms() });
        }
    }
    v
}

fn client() -> Result<reqwest::Client, String> {
    crate::http_client().timeout(std::time::Duration::from_secs(5)).build().map_err(|e| e.to_string())
}

async fn send(app: &App, r: reqwest::RequestBuilder) -> Result<Json, String> {
    let me = coordinators(app).into_iter().find(|p| p.this).map(|p| p.url).unwrap_or_default();
    let mut r = r.header("x-strata-peer", me);
    if let Some(t) = &app.token {
        r = r.bearer_auth(&*t.0);
    }
    let res = r.send().await.map_err(|e| e.without_url().to_string())?;
    let status = res.status();
    let j: Json = res.json().await.unwrap_or(Json::Null);
    if !status.is_success() {
        return Err(j["error"].as_str().map_or_else(|| format!("{status}"), |e| format!("{status}: {e}")));
    }
    Ok(j)
}

/// POSTs a JSON body to another coordinator's HTTP API, with the header
/// `x-strata-peer: <this coordinator's url>`.
pub(crate) async fn post(app: &App, peer: &Peer, path: &str, body: &Json) -> Result<Json, String> {
    send(app, client()?.post(format!("{}{path}", peer.url)).json(body)).await
}

/// GETs the same way.
pub(crate) async fn get(app: &App, peer: &Peer, path: &str) -> Result<Json, String> {
    send(app, client()?.get(format!("{}{path}", peer.url))).await
}
