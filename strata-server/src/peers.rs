//! peers: stub. The coordinators of this cluster that this one knows, to
//! the signatures of fleet's `src/peers.rs` (ROUND2.md, "coordinators that
//! know each other"): itself only, and no calls. The lead drops this
//! commit at merge; fleet's file takes its place.

use serde_json::Value as Json;

use crate::http::App;

pub struct Peer {
    /// Its HTTP base, e.g. http://10.0.0.5:7700.
    pub url: String,
    pub name: String,
    pub this: bool,
    pub last_seen_ms: u64,
}

/// Every coordinator of this cluster this one knows, itself included.
pub fn coordinators(app: &App) -> Vec<Peer> {
    let svc = &app.svc;
    let url = svc.identity.public_url.clone().unwrap_or_default();
    let name = svc.place.name.clone().or_else(|| svc.place.node.clone()).unwrap_or_else(|| "coordinator".into());
    vec![Peer { url, name, this: true, last_seen_ms: crate::telemetry::now_ms() }]
}

/// POST a JSON body to another coordinator's HTTP API.
pub async fn post(_app: &App, _peer: &Peer, path: &str, _body: &Json) -> Result<Json, String> {
    Err(format!("peers: stub, no call to {path}"))
}

/// GET the same way.
pub async fn get(_app: &App, _peer: &Peer, path: &str) -> Result<Json, String> {
    Err(format!("peers: stub, no call to {path}"))
}
