//! Coordinators that know each other (`GET /v1/coordinators`).
//!
//! A cluster runs several coordinators over the same shards, and none is
//! told of the others. Each coordinator names itself to the shards it
//! surveys: its `Health` calls carry its URL, name and version in their
//! gRPC metadata (`x-strata-coordinator`, `-name`, `-version`; no change
//! to the shard protocol). A shard keeps who named itself with the
//! cluster's token (a key's holder may not), for ten minutes, and shows
//! them in its detail (`coordinators`), which every coordinator reads in
//! its next survey. So a coordinator learns the others from the union of
//! what its shards heard, within two surveys (5 s apart). One not heard
//! for a minute is gone: [`coordinators`] leaves it out, and
//! `/v1/coordinators` lists it under `gone` until the shards forget it.
//!
//! The URL a coordinator names itself by is `--peer-url`, else its pod's
//! address (`STRATA_POD_IP`) and port, else the address it listens on
//! ([`own_url`]). It is not `--public-url`, which on Kubernetes is the
//! address every coordinator shares behind its Service.
//!
//! With them known, what the command center does on one coordinator it
//! does on all: [`post`], [`get`] and [`delete`] call another
//! coordinator's HTTP API with the shards' token, and say they come from a
//! peer ([`PEER_HEADER`]) so that the receiver does not pass them on again.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::Duration;

use serde_json::{json, Value as Json};

use crate::service::{Backend, Service};
use crate::telemetry::now_ms;

/// What the peer calls take: the service (`&Arc<Service>` coerces).
pub type App = Service;

/// A coordinator of this cluster.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Peer {
    /// Its HTTP base, e.g. `http://10.0.0.5:7700`.
    pub url: String,
    pub name: String,
    /// It is this coordinator.
    pub this: bool,
    /// When a shard last heard from it (now, for this one).
    pub last_seen_ms: u64,
}

/// A coordinator not heard from for this long is gone.
pub const LIVE_MS: u64 = 60_000;
/// How long a shard keeps a coordinator it no longer hears from.
pub const KEEP_MS: u64 = 600_000;
/// How often a coordinator reads what its shards heard.
pub const REFRESH: Duration = Duration::from_secs(5);
/// How long a call to another coordinator may take.
pub const TIMEOUT: Duration = Duration::from_secs(5);
/// The header a coordinator's call to another carries: its own URL.
pub const PEER_HEADER: &str = "x-strata-peer";
/// The gRPC metadata a coordinator names itself by to its shards.
pub const URL_MD: &str = "x-strata-coordinator";
pub const NAME_MD: &str = "x-strata-coordinator-name";
pub const VERSION_MD: &str = "x-strata-coordinator-version";
/// The most coordinators a shard keeps (a cluster has a handful).
const MAX_HEARD: usize = 64;

/// A coordinator as heard of: by a shard from its calls, by a
/// coordinator from its shards.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Heard {
    pub url: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub last_seen_ms: u64,
}

/// This coordinator, as it names itself.
#[derive(Clone, Debug)]
struct Me {
    url: String,
    name: String,
    version: String,
}

/// The coordinators a server knows.
#[derive(Default)]
pub struct Peers {
    me: OnceLock<Me>,
    /// A shard's: who named itself, by URL.
    heard: Mutex<HashMap<String, Heard>>,
    /// A coordinator's: what its shards heard, at their last survey.
    known: RwLock<Vec<Heard>>,
    /// When a coordinator first heard of another, by URL.
    first: Mutex<HashMap<String, u64>>,
    /// What each other coordinator last said of its searches, by URL: when,
    /// and its answer or why none ([`windows`]).
    windows: RwLock<HashMap<String, (u64, Result<Json, String>)>>,
    http: OnceLock<reqwest::Client>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Peers {
    /// A shard heard coordinator `url` now.
    pub fn heard(&self, url: &str, name: Option<&str>, version: Option<&str>, now: u64) {
        let url = url.trim().trim_end_matches('/');
        if !(url.starts_with("http://") || url.starts_with("https://")) || url.len() > 512 {
            return;
        }
        let mut h = lock(&self.heard);
        h.retain(|_, v| v.last_seen_ms + KEEP_MS > now);
        if h.len() >= MAX_HEARD && !h.contains_key(url) {
            return;
        }
        let name = name.map(str::trim).filter(|n| !n.is_empty()).map_or_else(|| host_of(url), |n| crate::obs::truncate(n, 120).into_owned());
        h.insert(url.to_string(), Heard { url: url.to_string(), name, version: version.map(|v| crate::obs::truncate(v, 200).into_owned()), last_seen_ms: now });
    }

    /// What a shard heard in the last ten minutes, the most recent first:
    /// its detail's `coordinators`.
    pub fn heard_json(&self) -> Json {
        let now = now_ms();
        let mut v: Vec<Heard> = lock(&self.heard).values().filter(|h| h.last_seen_ms + KEEP_MS > now).cloned().collect();
        v.sort_by(|a, b| b.last_seen_ms.cmp(&a.last_seen_ms).then_with(|| a.url.cmp(&b.url)));
        json!(v)
    }

    /// This coordinator's URL, once [`start`] named it.
    pub fn me(&self) -> Option<&str> {
        self.me.get().map(|m| m.url.as_str())
    }
}

/// `s` as a header value: bytes beyond printable ASCII, and `%`, as
/// `%XX` (which the receiver's `header_text` decodes).
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if (0x20..0x7f).contains(&b) && b != b'%' {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `s` as a part of a URL's query: all but letters, digits and `-._~` as
/// `%XX`.
pub fn encode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// `host:port` of a URL.
fn host_of(url: &str) -> String {
    url.split("://").nth(1).unwrap_or(url).split('/').next().unwrap_or(url).to_string()
}

/// The URL a coordinator names itself by ([`crate::peers`]): `given`
/// (`--peer-url`), else `STRATA_POD_IP` and the port it listens on, else
/// the address it listens on, with the machine's name for an address of
/// every interface.
pub fn own_url(given: Option<&str>, listen: std::net::SocketAddr, tls: bool) -> String {
    if let Some(u) = given.map(str::trim).filter(|u| !u.is_empty()) {
        return u.trim_end_matches('/').to_string();
    }
    let scheme = if tls { "https" } else { "http" };
    let port = listen.port();
    if let Some(ip) = std::env::var("STRATA_POD_IP").ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty()) {
        let host = if ip.contains(':') { format!("[{ip}]") } else { ip };
        return format!("{scheme}://{host}:{port}");
    }
    let host = match listen.ip() {
        ip if ip.is_unspecified() => crate::logstore::hostname(),
        std::net::IpAddr::V6(ip) => format!("[{ip}]"),
        ip => ip.to_string(),
    };
    format!("{scheme}://{host}:{port}")
}

/// Names this server, a coordinator, as `url` to its shards and starts
/// reading who else they heard, every [`REFRESH`], until the service is
/// dropped. Nothing on a shard server.
pub fn start(svc: &Arc<Service>, url: String) {
    let Backend::Cluster(c) = &svc.backend else { return };
    let name = svc.place.name.clone().or_else(|| svc.place.node.clone()).unwrap_or_else(|| host_of(&url));
    let version = svc.place.image.clone().map_or_else(|| env!("CARGO_PKG_VERSION").to_string(), |i| format!("{} ({i})", env!("CARGO_PKG_VERSION")));
    let mut md = Vec::new();
    for (k, v) in [(URL_MD, &url), (NAME_MD, &name), (VERSION_MD, &version)] {
        if let Ok(v) = v.parse::<tonic::metadata::AsciiMetadataValue>() {
            md.push((k, v));
        }
    }
    let _ = c.announce.set(md);
    let _ = svc.peers.me.set(Me { url, name, version });
    let weak = Arc::downgrade(svc);
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(REFRESH);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let Some(svc) = weak.upgrade() else { return };
            refresh(&svc).await;
            fetch_windows(&svc).await;
            crate::contain::sync(&svc).await;
        }
    });
}

/// A shard's `Health` call from a coordinator that names itself: kept
/// when the call holds the cluster's token (or the server is open).
pub fn heard_from(svc: &Service, md: &tonic::metadata::MetadataMap, who: Option<&crate::keys::Principal>) {
    let Some(url) = md.get(URL_MD).and_then(|v| v.to_str().ok()) else { return };
    if !who.is_none_or(|p| p.root) {
        return;
    }
    let text = |k: &str| md.get(k).and_then(|v| v.to_str().ok());
    svc.peers.heard(url, text(NAME_MD), text(VERSION_MD), now_ms());
}

/// Reads who the shards heard, from the cluster snapshot (one at most
/// 10 s old: the coordinator surveys every 5 s anyway).
pub async fn refresh(svc: &Service) {
    let Backend::Cluster(c) = &svc.backend else { return };
    let snap = c.snapshot_within(Duration::from_secs(10)).await;
    let mut by: HashMap<String, Heard> = HashMap::new();
    for s in snap["shards"].as_array().map_or(&[][..], |v| v.as_slice()) {
        for r in s["replicas"].as_array().map_or(&[][..], |v| v.as_slice()) {
            for h in r["detail"]["coordinators"].as_array().map_or(&[][..], |v| v.as_slice()) {
                let Ok(h) = serde_json::from_value::<Heard>(h.clone()) else { continue };
                match by.get_mut(&h.url) {
                    Some(k) if k.last_seen_ms >= h.last_seen_ms => {}
                    _ => {
                        by.insert(h.url.clone(), h);
                    }
                }
            }
        }
    }
    let mut v: Vec<Heard> = by.into_values().collect();
    v.sort_by(|a, b| a.url.cmp(&b.url));
    let now = now_ms();
    let mut first = lock(&svc.peers.first);
    for h in &v {
        first.entry(h.url.clone()).or_insert(now);
    }
    drop(first);
    *svc.peers.known.write().unwrap_or_else(|e| e.into_inner()) = v;
}

/// This server's searches over the fleet health windows, for the other
/// coordinators (`GET /v1/coordinators?window=1`).
pub fn window_json(app: &App) -> Json {
    let now = now_ms();
    let points = app.series.since(now.saturating_sub(crate::fleet::WINDOW_S * 1000));
    let w = crate::fleet::Window::of(&points);
    let r = crate::fleet::Window::since(&points, now.saturating_sub(crate::fleet::RECENT_S * 1000));
    json!({"window": w.json(), "recent": r.json(), "window_s": crate::fleet::WINDOW_S, "recent_s": crate::fleet::RECENT_S, "at_ms": now})
}

/// Asks every other coordinator for its searches ([`window_json`]).
async fn fetch_windows(svc: &Service) {
    let answers = get_all(svc, "/v1/coordinators?window=1").await;
    let now = now_ms();
    let mut w = svc.peers.windows.write().unwrap_or_else(|e| e.into_inner());
    w.retain(|_, (t, _)| *t + LIVE_MS > now);
    for (p, a) in answers {
        w.insert(p.url, (now, a.map(|j| j["window"].clone())));
    }
}

/// What each other live coordinator last said of its searches (within
/// 20 s), for fleet health: the SLO checks judge every coordinator's.
pub fn windows(app: &App) -> Vec<crate::fleet::PeerWindow> {
    let now = now_ms();
    let held = app.peers.windows.read().unwrap_or_else(|e| e.into_inner());
    others(app)
        .into_iter()
        .map(|p| {
            let mut pw = crate::fleet::PeerWindow { url: p.url.clone(), name: p.name.clone(), ..Default::default() };
            match held.get(&p.url) {
                Some((t, Ok(j))) if t + 20_000 > now && j.is_object() => {
                    pw.window = crate::fleet::Window::from_json(&j["window"]);
                    pw.recent = crate::fleet::Window::from_json(&j["recent"]);
                }
                Some((t, Err(e))) if t + 20_000 > now => pw.error = Some(e.clone()),
                _ => pw.error = Some(format!("{}: not asked yet", p.name)),
            }
            pw
        })
        .collect()
}

/// When this coordinator first heard of coordinator `url`.
pub fn first_heard(app: &App, url: &str) -> Option<u64> {
    lock(&app.peers.first).get(url).copied()
}

/// Every coordinator this server knows, with each one's version, this one
/// first if it is one; gone ones too (not heard for [`LIVE_MS`]).
fn all(app: &App) -> Vec<(Peer, Option<String>)> {
    let now = now_ms();
    let mut out: Vec<(Peer, Option<String>)> = Vec::new();
    match &app.backend {
        Backend::Cluster(_) => {
            let me = app.peers.me.get();
            let (url, name, version) = match me {
                Some(m) => (m.url.clone(), m.name.clone(), m.version.clone()),
                None => (app.identity.public_url.clone().unwrap_or_default(), app.place.name.clone().unwrap_or_else(|| "this coordinator".into()), env!("CARGO_PKG_VERSION").to_string()),
            };
            out.push((Peer { url: url.clone(), name, this: true, last_seen_ms: now }, Some(version)));
            for h in app.peers.known.read().unwrap_or_else(|e| e.into_inner()).iter() {
                if h.url != url {
                    out.push((Peer { url: h.url.clone(), name: h.name.clone(), this: false, last_seen_ms: h.last_seen_ms }, h.version.clone()));
                }
            }
        }
        Backend::Local(_) => {
            for h in lock(&app.peers.heard).values().filter(|h| h.last_seen_ms + KEEP_MS > now) {
                out.push((Peer { url: h.url.clone(), name: h.name.clone(), this: false, last_seen_ms: h.last_seen_ms }, h.version.clone()));
            }
        }
    }
    out
}

fn live(p: &Peer, now: u64) -> bool {
    p.this || p.last_seen_ms + LIVE_MS > now
}

/// Every coordinator of this cluster this one knows, itself included:
/// those heard from in the last minute, by URL (the lowest first). On a
/// shard server, the coordinators it heard from.
pub fn coordinators(app: &App) -> Vec<Peer> {
    let now = now_ms();
    let mut v: Vec<Peer> = all(app).into_iter().map(|(p, _)| p).filter(|p| live(p, now)).collect();
    v.sort_by(|a, b| a.url.cmp(&b.url));
    v
}

/// The other coordinators this one knows (live ones).
pub fn others(app: &App) -> Vec<Peer> {
    coordinators(app).into_iter().filter(|p| !p.this).collect()
}

/// `GET /v1/coordinators`: `{"coordinators": [{url, name, this,
/// last_seen_ms, version}], "gone": [...], "server", "at_ms"}`.
pub fn json(app: &App) -> Json {
    let now = now_ms();
    let (mut up, mut gone) = (Vec::new(), Vec::new());
    for (p, version) in all(app) {
        let j = json!({"url": p.url, "name": p.name, "this": p.this, "last_seen_ms": p.last_seen_ms, "version": version});
        if live(&p, now) {
            up.push(j);
        } else {
            gone.push(j);
        }
    }
    up.sort_by(|a, b| a["url"].as_str().cmp(&b["url"].as_str()));
    gone.sort_by(|a, b| b["last_seen_ms"].as_u64().cmp(&a["last_seen_ms"].as_u64()));
    let server = if matches!(app.backend, Backend::Cluster(_)) { "coordinator" } else { "shard" };
    json!({"coordinators": up, "gone": gone, "server": server, "at_ms": now, "live_ms": LIVE_MS})
}

/// The URL of the coordinator a request came from ([`PEER_HEADER`]), if
/// another coordinator sent it.
pub fn from_peer(headers: &axum::http::HeaderMap) -> Option<String> {
    headers.get(PEER_HEADER).and_then(|v| v.to_str().ok()).map(str::trim).filter(|v| !v.is_empty()).map(String::from)
}

/// POSTs `body` to `path` on coordinator `peer`, as this coordinator.
pub async fn post(app: &App, peer: &Peer, path: &str, body: &Json) -> Result<Json, String> {
    send(app, peer, reqwest::Method::POST, path, Some(body)).await
}

/// GETs `path` from coordinator `peer`, as this coordinator.
pub async fn get(app: &App, peer: &Peer, path: &str) -> Result<Json, String> {
    send(app, peer, reqwest::Method::GET, path, None).await
}

/// DELETEs `path` on coordinator `peer`, as this coordinator.
pub async fn delete(app: &App, peer: &Peer, path: &str) -> Result<Json, String> {
    send(app, peer, reqwest::Method::DELETE, path, None).await
}

/// One call to another coordinator: with the shards' token, the request's
/// client and reason, and [`PEER_HEADER`]; its JSON answer, or why not
/// (naming the coordinator).
async fn send(app: &App, peer: &Peer, method: reqwest::Method, path: &str, body: Option<&Json>) -> Result<Json, String> {
    if peer.this {
        return Err(format!("{} is this coordinator", peer.name));
    }
    let client = app.peers.http.get_or_init(|| crate::http_client().timeout(TIMEOUT).connect_timeout(Duration::from_secs(2)).build().unwrap_or_default());
    let url = format!("{}/{}", peer.url.trim_end_matches('/'), path.trim_start_matches('/'));
    let me = app.peers.me().map(String::from).or_else(|| app.identity.public_url.clone()).unwrap_or_else(|| "coordinator".into());
    let mut rq = client.request(method, &url).header(PEER_HEADER, me);
    if let Backend::Cluster(c) = &app.backend {
        if let Some(t) = &c.opts.token {
            rq = rq.bearer_auth(&*t.0);
        }
    }
    let caller = crate::keys::caller().unwrap_or_default();
    if let Some(c) = caller.client.as_deref().filter(|c| c.is_ascii()) {
        rq = rq.header("x-strata-client", c);
    }
    if let Some(r) = caller.reason.as_deref() {
        rq = rq.header("x-strata-reason", escape(r));
    }
    if let Some(id) = crate::obs::current() {
        rq = rq.header(crate::obs::HEADER, id.to_string());
    }
    if let Some(b) = body {
        rq = rq.json(b);
    }
    let r = rq.send().await.map_err(|e| format!("{}: {}", peer.name, if e.is_timeout() { "no answer within 5 s".to_string() } else if e.is_connect() { "cannot connect".to_string() } else { e.to_string() }))?;
    let status = r.status();
    let text = r.text().await.map_err(|e| format!("{}: {e}", peer.name))?;
    let j: Json = serde_json::from_str(&text).unwrap_or(Json::Null);
    if !status.is_success() {
        let why = j["error"].as_str().map(String::from).unwrap_or_else(|| crate::obs::truncate(text.trim(), 200).into_owned());
        return Err(format!("{}: {} {why}", peer.name, status.as_u16()));
    }
    Ok(j)
}

/// GETs `path` from every other live coordinator at once: each one and
/// its answer.
pub async fn get_all(app: &App, path: &str) -> Vec<(Peer, Result<Json, String>)> {
    let peers = others(app);
    let answers = futures::future::join_all(peers.iter().map(|p| get(app, p, path))).await;
    peers.into_iter().zip(answers).collect()
}

/// POSTs `body` to `path` on every other live coordinator at once.
pub async fn post_all(app: &App, path: &str, body: &Json) -> Vec<(Peer, Result<Json, String>)> {
    let peers = others(app);
    let answers = futures::future::join_all(peers.iter().map(|p| post(app, p, path, body))).await;
    peers.into_iter().zip(answers).collect()
}

/// DELETEs `path` on every other live coordinator at once.
pub async fn delete_all(app: &App, path: &str) -> Vec<(Peer, Result<Json, String>)> {
    let peers = others(app);
    let answers = futures::future::join_all(peers.iter().map(|p| delete(app, p, path))).await;
    peers.into_iter().zip(answers).collect()
}

/// What the answers of [`get_all`] and the like say for the caller: `[{url, name, this,
/// ok, error?}]`, this coordinator first.
pub fn reach(app: &App, answers: &[(Peer, Result<Json, String>)]) -> Json {
    let me = coordinators(app).into_iter().find(|p| p.this);
    let mut v: Vec<Json> = me.into_iter().map(|p| json!({"url": p.url, "name": p.name, "this": true, "ok": true})).collect();
    for (p, a) in answers {
        let mut j = json!({"url": p.url, "name": p.name, "this": false, "ok": a.is_ok()});
        if let Err(e) = a {
            j["error"] = json!(e);
        }
        v.push(j);
    }
    Json::Array(v)
}

// ----- Running now on every coordinator

/// This coordinator's [`Peer`], if it is one.
pub fn this(app: &App) -> Option<Peer> {
    match &app.backend {
        Backend::Cluster(_) => coordinators(app).into_iter().find(|p| p.this),
        Backend::Local(_) => None,
    }
}

/// Marks the queries and groups of an answer of `GET /v1/queries` as
/// coordinator `p`'s: `node` (its name) and `coordinator_url`. Another
/// coordinator's query ids become `<id>@<its name>`, which
/// `DELETE /v1/queries/:id` on this coordinator sends there: an id alone
/// would name a query of this one.
fn tag(j: &mut Json, p: &Peer) {
    let id = |v: &Json| if p.this { v.clone() } else { json!(format!("{}@{}", v.as_u64().map_or_else(|| v.as_str().unwrap_or("").to_string(), |n| n.to_string()), p.name)) };
    for q in j["queries"].as_array_mut().into_iter().flatten() {
        q["node"] = json!(p.name);
        q["coordinator_url"] = json!(p.url);
        if !p.this {
            q["local_id"] = q["id"].clone();
            q["id"] = id(&q["id"]);
        }
    }
    for g in j["groups"].as_array_mut().into_iter().flatten() {
        if !p.this {
            g["ids"] = Json::Array(g["ids"].as_array().map_or(&[][..], |v| v.as_slice()).iter().map(id).collect());
        }
        g["coordinators"] = json!([{"name": p.name, "url": p.url, "count": g["count"], "ids": g["ids"]}]);
    }
}

/// The other coordinator a query id of [`gather_live`] names
/// (`<id>@<name>`), and its own id there; `None` for an id of this one.
pub fn query_at(app: &App, id: &str) -> Option<Result<(Peer, u64), String>> {
    let (n, name) = id.split_once('@')?;
    let Ok(n) = n.parse::<u64>() else { return Some(Err(format!("{id} is not a query id"))) };
    let mut v: Vec<Peer> = others(app).into_iter().filter(|p| p.name == name).collect();
    Some(match v.len() {
        1 => Ok((v.remove(0), n)),
        0 => Err(format!("no coordinator named {name} is known here")),
        _ => Err(format!("{} coordinators are named {name}: cancel it on that one", v.len())),
    })
}

/// `GET /v1/queries` of every coordinator: this one's answer `local` with
/// the others' queries added and their groups merged (by statement and
/// key: counts and times summed, `coordinators` saying how many run on
/// which), each query naming its coordinator (`node`, `coordinator_url`);
/// `running` summed, and `nodes` saying which coordinators answered.
pub async fn gather_live(app: &App, local: Json, groups: bool) -> Json {
    let Some(me) = this(app) else { return local };
    let answers = get_all(app, if groups { "/v1/queries?group=shape" } else { "/v1/queries" }).await;
    let mut out = if answers.is_empty() { local } else { merge_live(local, &me, &answers) };
    out["nodes"] = reach(app, &answers);
    out
}

/// This coordinator's (`me`) answer of `GET /v1/queries` with the others'.
fn merge_live(mut local: Json, me: &Peer, answers: &[(Peer, Result<Json, String>)]) -> Json {
    tag(&mut local, me);
    for (p, a) in answers {
        let Ok(j) = a else { continue };
        let mut j = j.clone();
        tag(&mut j, p);
        if let (Some(mine), Some(theirs)) = (local["queries"].as_array_mut(), j["queries"].as_array()) {
            mine.extend(theirs.iter().cloned());
        }
        local["running"] = json!(local["running"].as_u64().unwrap_or(0) + j["running"].as_u64().unwrap_or(0));
        let Some(gs) = local["groups"].as_array_mut() else { continue };
        for g in j["groups"].as_array().into_iter().flatten() {
            match gs.iter_mut().find(|x| x["fingerprint"] == g["fingerprint"] && x["key"] == g["key"]) {
                Some(x) => {
                    x["count"] = json!(x["count"].as_u64().unwrap_or(0) + g["count"].as_u64().unwrap_or(0));
                    x["total_ms"] = json!(x["total_ms"].as_f64().unwrap_or(0.0) + g["total_ms"].as_f64().unwrap_or(0.0));
                    x["oldest_ms"] = json!(x["oldest_ms"].as_f64().unwrap_or(0.0).max(g["oldest_ms"].as_f64().unwrap_or(0.0)));
                    if let Some(t) = g["oldest_started_ms"].as_u64().filter(|&t| t > 0 && x["oldest_started_ms"].as_u64().is_none_or(|u| t < u)) {
                        x["oldest_started_ms"] = json!(t);
                    }
                    if let (Some(c), Some(more)) = (x["coordinators"].as_array_mut(), g["coordinators"].as_array()) {
                        c.extend(more.iter().cloned());
                    }
                }
                None => gs.push(g.clone()),
            }
        }
    }
    if let Some(gs) = local["groups"].as_array_mut() {
        gs.sort_by(|a, b| b["total_ms"].as_f64().unwrap_or(0.0).total_cmp(&a["total_ms"].as_f64().unwrap_or(0.0)));
    }
    if let Some(qs) = local["queries"].as_array_mut() {
        qs.sort_by_key(|q| q["started_ms"].as_u64().unwrap_or(0));
    }
    local
}

/// Cancels a statement's running queries (of `key` if given) on every
/// other coordinator: how many each cancelled.
pub async fn cancel_elsewhere(app: &App, fingerprint: &str, key: Option<&str>) -> (u64, Json) {
    let path = format!("/v1/queries?fingerprint={}{}", encode(fingerprint), key.map_or(String::new(), |k| format!("&key={}", encode(k))));
    let answers = delete_all(app, &path).await;
    let n = answers.iter().filter_map(|(_, a)| a.as_ref().ok()?["cancelled"].as_u64()).sum();
    let mut reached = reach(app, &answers);
    for (r, (_, a)) in reached.as_array_mut().into_iter().flatten().skip(1).zip(&answers) {
        if let Ok(j) = a {
            r["cancelled"] = j["cancelled"].clone();
        }
    }
    (n, reached)
}

/// The live coordinator at `url`, other than this one.
pub fn other_at(app: &App, url: &str) -> Option<Peer> {
    let url = url.trim().trim_end_matches('/');
    others(app).into_iter().find(|p| p.url == url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shard_keeps_who_named_itself_for_a_while() {
        let p = Peers::default();
        p.heard("http://10.0.0.5:7700/", Some("coord-a"), Some("0.1.0"), 1_000);
        p.heard("ftp://nope", Some("x"), None, 1_000);
        p.heard("http://10.0.0.6:7700", None, None, 2_000);
        let h = lock(&p.heard);
        assert_eq!(h.len(), 2);
        assert_eq!(h["http://10.0.0.5:7700"].name, "coord-a");
        assert_eq!(h["http://10.0.0.6:7700"].name, "10.0.0.6:7700", "no name: its host and port");
        drop(h);
        // Ten minutes later, the first is forgotten when another is heard.
        p.heard("http://10.0.0.6:7700", None, None, 1_000 + KEEP_MS);
        assert_eq!(lock(&p.heard).len(), 1);
    }

    #[test]
    fn a_coordinator_names_itself_by_its_given_url_else_its_address() {
        let a: std::net::SocketAddr = "127.0.0.1:8100".parse().unwrap();
        assert_eq!(own_url(Some("http://c1:7700/"), a, false), "http://c1:7700");
        if std::env::var("STRATA_POD_IP").is_err() {
            assert_eq!(own_url(None, a, false), "http://127.0.0.1:8100");
            assert_eq!(own_url(None, "[::1]:8100".parse().unwrap(), true), "https://[::1]:8100");
            let any = own_url(None, "0.0.0.0:7700".parse().unwrap(), false);
            assert!(any.starts_with("http://") && any.ends_with(":7700") && !any.contains("0.0.0.0"), "{any}");
        }
    }

    #[test]
    fn a_reason_travels_as_a_header() {
        assert_eq!(escape("runaway: 100% CPU"), "runaway: 100%25 CPU");
        assert_eq!(escape("café\n"), "caf%C3%A9%0A");
    }

    fn peer(url: &str, name: &str, this: bool) -> Peer {
        Peer { url: url.into(), name: name.into(), this, last_seen_ms: 0 }
    }

    #[test]
    fn running_now_merges_every_coordinators_queries_and_groups() {
        let a = peer("http://a:1", "coord-a", true);
        let b = peer("http://b:1", "coord-b", false);
        let c = peer("http://c:1", "coord-c", false);
        let local = json!({"running": 2, "queries": [{"id": 7, "started_ms": 200}, {"id": 8, "started_ms": 50}],
            "groups": [{"fingerprint": "SELECT ?", "key": "batch", "count": 2, "total_ms": 300.0, "oldest_ms": 200.0, "oldest_started_ms": 50, "ids": [7, 8]}]});
        let theirs = json!({"running": 3, "queries": [{"id": 7, "started_ms": 100}],
            "groups": [{"fingerprint": "SELECT ?", "key": "batch", "count": 1, "total_ms": 900.0, "oldest_ms": 900.0, "oldest_started_ms": 10, "ids": [7]},
                       {"fingerprint": "SELECT ? FROM t", "key": null, "count": 2, "total_ms": 40.0, "oldest_ms": 30.0, "ids": [9, 10]}]});
        let j = merge_live(local, &a, &[(b.clone(), Ok(theirs)), (c, Err("coord-c: cannot connect".into()))]);
        assert_eq!(j["running"], 5);
        let qs = j["queries"].as_array().unwrap();
        assert_eq!(qs.iter().map(|q| (q["id"].to_string(), q["node"].as_str().unwrap())).collect::<Vec<_>>(), [("8".into(), "coord-a"), ("\"7@coord-b\"".into(), "coord-b"), ("7".into(), "coord-a")], "by start; the same id on two coordinators kept apart");
        assert_eq!((qs[1]["coordinator_url"].as_str(), qs[1]["local_id"].as_u64()), (Some("http://b:1"), Some(7)));
        let gs = j["groups"].as_array().unwrap();
        assert_eq!(gs.len(), 2);
        assert_eq!((gs[0]["count"].as_u64(), gs[0]["total_ms"].as_f64(), gs[0]["oldest_ms"].as_f64(), gs[0]["oldest_started_ms"].as_u64()), (Some(3), Some(1200.0), Some(900.0), Some(10)));
        let on: Vec<(&str, u64)> = gs[0]["coordinators"].as_array().unwrap().iter().map(|c| (c["name"].as_str().unwrap(), c["count"].as_u64().unwrap())).collect();
        assert_eq!(on, [("coord-a", 2), ("coord-b", 1)]);
        assert_eq!(gs[0]["coordinators"][1]["ids"], json!(["7@coord-b"]));
        assert_eq!(gs[0]["coordinators"][0]["ids"], json!([7, 8]));
        assert_eq!(gs[1]["coordinators"][0]["url"], "http://b:1");
    }

    #[test]
    fn a_url_component_is_escaped() {
        assert_eq!(encode("SELECT ? FROM t WHERE a = ?&b"), "SELECT%20%3F%20FROM%20t%20WHERE%20a%20%3D%20%3F%26b");
        assert_eq!(encode("ops-1_x.y~"), "ops-1_x.y~");
    }

    #[test]
    fn host_of_a_url() {
        assert_eq!(host_of("http://a:1/x"), "a:1");
        assert_eq!(host_of("https://b"), "b");
    }
}
