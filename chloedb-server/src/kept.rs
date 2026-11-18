//! What a coordinator keeps on its shards (OLU-201), as it keeps its keys
//! there ([`crate::keysync`]): its alert rules, its routes, the alerts'
//! states (what fires, acknowledgements, silences, notes, and what was
//! paged), and the latest entries of its audit log.
//!
//! A coordinator holds no data and needs no disk: without `--alerts FILE`
//! and `--audit-log FILE` it held these in memory only, so a restart (a
//! rolling upgrade restarts the coordinators last) lost every rule and
//! route made since it started, and who changed what, without a word.
//! Now every copy of every shard keeps them, in `<db>.coordinator.json`
//! beside its database, and a coordinator that starts takes them back
//! before it serves ([`start`]), as it does its keys:
//!
//! - each part (`rules`, `routes`, `states`) carries the time it was last
//!   changed, and the newest wins, on a copy and on a coordinator, as
//!   between coordinators ([`crate::alerts::Alerts::merge`]); a file a
//!   coordinator is given (`--alerts`) is one more place it was kept, and
//!   the newer of the file and the shards wins;
//! - a coordinator gives its copies what changed within a few seconds (a
//!   rule or a route before it answers the request that changed it), and
//!   everything every [`ALL_EVERY_MS`], so a copy that was down, or
//!   started on an empty disk, has them again;
//! - the audit log's entries go to the shards as they are made, and the
//!   shards keep the newest [`AUDIT_KEPT`] of every coordinator's: a
//!   coordinator that starts again answers them (`GET /v1/admin/audit`,
//!   `source` `shards`) for the time before it started. `--audit-log FILE`
//!   keeps every entry; the shards keep the latest.
//!
//! A coordinator gives its copies nothing until it has read what they
//! keep (a coordinator that started while its shards did not answer would
//! otherwise replace their states with its empty ones), and calls them
//! with the cluster's token, over the call it asks its copies things with
//! (`Profile`, kind `alerts`, op `keep`), which a copy of an older version
//! refuses: that copy keeps nothing, and the coordinator says so
//! (`GET /v1/alerts`'s `kept`).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use serde_json::{json, Value as Json};

use crate::service::{Backend, Service};
use crate::telemetry::now_ms;
use crate::ApiError;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The parts a copy keeps of a coordinator's alerts.
pub const PARTS: [&str; 3] = ["rules", "routes", "states"];
/// How often a coordinator gives each copy everything again.
pub const ALL_EVERY_MS: u64 = 30_000;
/// How often it gives them what changed (and sooner when an alert's state
/// changes, a second after).
const EVERY: Duration = Duration::from_secs(5);
/// The audit entries the shards keep: the newest of every coordinator's.
pub const AUDIT_KEPT: usize = 2_000;
/// The most a copy keeps of them, in bytes of JSON (rules edits carry the
/// rules before and after).
const AUDIT_BYTES: usize = 4 << 20;
/// The most one call gives a copy of them, in bytes of JSON.
const CALL_BYTES: usize = 1 << 20;
/// How long a copy has to answer.
const WAIT: Duration = Duration::from_secs(3);
/// Copies called at once.
const AT_ONCE: usize = 16;

/// The file a copy keeps a coordinator's alerts in: `<db>.coordinator.json`.
pub fn file_of(db: &Path) -> PathBuf {
    let mut p = db.as_os_str().to_owned();
    p.push(".coordinator.json");
    p.into()
}

// ------------------------------------------------------------ on a copy

/// What a copy keeps ([`file_of`]).
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Held {
    /// Each part: `{ms, by?, data}`.
    #[serde(default)]
    parts: serde_json::Map<String, Json>,
    /// The coordinators' audit entries, the oldest first, each naming its
    /// `coordinator`.
    #[serde(default)]
    audit: Vec<Json>,
}

/// What the copies of this process hold, by file (tests run several in one).
static HELD: LazyLock<Mutex<HashMap<PathBuf, Held>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn read_held(p: &Path) -> Held {
    match std::fs::read(p) {
        Ok(b) => serde_json::from_slice(&b).unwrap_or_else(|e| {
            tracing::warn!(file = %p.display(), error = %e, "what this copy kept of its coordinators' alerts does not read: it keeps them again from the next coordinator");
            Held::default()
        }),
        Err(_) => Held::default(),
    }
}

/// What tells one audit entry from another, over every coordinator.
fn audit_key(e: &Json) -> String {
    let s = |k: &str| e[k].as_str().unwrap_or("").to_string();
    format!("{}|{}|{}|{}|{}|{}|{}", s("coordinator"), e["ts_ms"].as_u64().unwrap_or(0), s("method"), s("route"), s("who"), e["status"].as_u64().unwrap_or(0), s("request_id"))
}

/// What tells one audit entry from another wherever it was read (memory,
/// the log store, the shards), the coordinator it names apart.
pub fn entry_key(e: &Json) -> String {
    let s = |k: &str| e[k].as_str().unwrap_or("").to_string();
    format!("{}|{}|{}|{}|{}|{}", e["ts_ms"].as_u64().unwrap_or(0), s("method"), s("route"), s("who"), e["status"].as_u64().unwrap_or(0), s("request_id"))
}

/// The newest of `entries` (the oldest first) that fit [`AUDIT_KEPT`] and
/// [`AUDIT_BYTES`].
fn trim_audit(entries: &mut Vec<Json>) {
    let mut bytes = 0;
    let mut keep = 0;
    for e in entries.iter().rev() {
        bytes += e.to_string().len();
        if keep == AUDIT_KEPT || bytes > AUDIT_BYTES {
            break;
        }
        keep += 1;
    }
    let cut = entries.len() - keep;
    entries.drain(..cut);
}

/// What a copy does when a coordinator gives it what it keeps
/// (`{"op": "keep", "parts": {name: {ms, by?, data}}, "audit": [...],
/// "want": bool}`): it takes each part newer than its own and the audit
/// entries it does not hold, writes them beside its database, and answers
/// `{took, parts: {name: {ms}}, audit_ms: {coordinator: newest}}`, with
/// `data` (every part whole) and `audit` when `want`.
pub fn take(svc: &Service, b: &Json) -> Result<Json, ApiError> {
    let db = match &svc.backend {
        Backend::Local(db) => db.clone(),
        Backend::Cluster(_) => return Err(ApiError { code: None, status: 409, message: "this server is a coordinator: its shards keep what it keeps".into() }),
    };
    let path = file_of(db.storage().path());
    let mut all = lock(&HELD);
    let held = all.entry(path.clone()).or_insert_with(|| read_held(&path));
    let mut took: Vec<String> = Vec::new();
    for (name, part) in b["parts"].as_object().into_iter().flatten() {
        if !PARTS.contains(&name.as_str()) || part.get("data").is_none() {
            continue;
        }
        let ms = part["ms"].as_u64().unwrap_or(0);
        let mine = held.parts.get(name).and_then(|p| p["ms"].as_u64());
        if mine.is_none_or(|m| ms > m) {
            held.parts.insert(name.clone(), part.clone());
            took.push(name.clone());
        }
    }
    let mut added = 0;
    if let Some(entries) = b["audit"].as_array().filter(|a| !a.is_empty()) {
        let seen: HashSet<String> = held.audit.iter().map(audit_key).collect();
        for e in entries {
            if e["coordinator"].is_string() && e["ts_ms"].is_u64() && !seen.contains(&audit_key(e)) {
                held.audit.push(e.clone());
                added += 1;
            }
        }
        if added > 0 {
            held.audit.sort_by_key(|e| e["ts_ms"].as_u64().unwrap_or(0));
            trim_audit(&mut held.audit);
        }
    }
    if !took.is_empty() || added > 0 {
        let tmp = path.with_extension("json.tmp");
        let body = serde_json::to_vec(&*held).map_err(ApiError::internal)?;
        std::fs::write(&tmp, body).and_then(|_| std::fs::rename(&tmp, &path)).map_err(|e| ApiError::internal(format!("write {}: {e}", path.display())))?;
    }
    let versions: serde_json::Map<String, Json> = held.parts.iter().map(|(k, v)| (k.clone(), json!({"ms": v["ms"]}))).collect();
    let mut newest: HashMap<String, u64> = HashMap::new();
    for e in &held.audit {
        let n = newest.entry(e["coordinator"].as_str().unwrap_or("").to_string()).or_default();
        *n = (*n).max(e["ts_ms"].as_u64().unwrap_or(0));
    }
    let mut j = json!({"took": took, "audit_added": added, "parts": versions, "audit_ms": newest});
    if b["want"] == true {
        j["data"] = Json::Object(held.parts.clone());
        j["audit"] = json!(held.audit);
    }
    Ok(j)
}

// ------------------------------------------------------- on a coordinator

/// What one copy said it holds, as this coordinator last heard.
#[derive(Clone, Debug, Default)]
struct CopyHeld {
    rules_ms: u64,
    routes_ms: u64,
    /// The states' stamp it was given last ([`crate::alerts::Alerts::kept_parts`]).
    stamp: Option<u64>,
    /// The newest of this coordinator's audit entries it holds.
    audit_ms: u64,
    /// When it was last given everything.
    all_ms: u64,
    /// When it last answered, or why it did not.
    at_ms: u64,
    error: Option<String>,
}

/// A coordinator's side ([`Service::kept`]).
#[derive(Default)]
pub struct Keeper {
    copies: Mutex<HashMap<String, CopyHeld>>,
    /// Whether this coordinator has read what its shards keep.
    read: AtomicBool,
    /// One push at a time.
    pushing: tokio::sync::Mutex<()>,
    /// The audit entries the shards kept from before this coordinator
    /// started (the oldest first), which `GET /v1/admin/audit` answers.
    restored: Mutex<Vec<Json>>,
    /// What the first read took, for `GET /v1/alerts`'s `kept`.
    took: Mutex<Option<Json>>,
    /// Woken when an audit entry is kept ([`Self::wake`]).
    woken: Arc<tokio::sync::Notify>,
}

impl Keeper {
    /// Something to keep changed (an audit entry): the copies get it a
    /// second from now.
    pub fn wake(&self) {
        self.woken.notify_one();
    }

    /// The audit entries the shards kept from before this coordinator
    /// started, the newest first.
    pub fn restored_audit(&self) -> Vec<Json> {
        lock(&self.restored).iter().rev().cloned().collect()
    }
}

/// The name this coordinator's audit entries go under: `--node-name`, else
/// the address the other coordinators reach it at.
fn me(svc: &Service) -> String {
    svc.place.name.clone().or_else(|| crate::peers::this(svc).map(|p| p.name)).unwrap_or_else(|| "coordinator".into())
}

/// Asks one copy, over `Profile`.
async fn call(svc: &Service, url: &str, body: Json) -> Result<Json, String> {
    let Backend::Cluster(c) = &svc.backend else { return Err("not a coordinator".into()) };
    let a = crate::profiling::Asked { kind: crate::profiling::Kind::Alerts, body, folded: false, in_use: false };
    match tokio::time::timeout(WAIT, c.profile(url, a)).await {
        Err(_) => Err(format!("no answer in {} s", WAIT.as_secs())),
        // A copy of an older version knows no such call.
        Ok(Err(e)) if e.message.contains("no alerts call") || e.message.contains("no profile of kind") => Err("the copy runs an older chloedb-server, which keeps nothing of its coordinator".into()),
        Ok(Err(e)) => Err(e.message),
        Ok(Ok(crate::profiling::Answer::Json(j))) => Ok(j),
        Ok(Ok(_)) => Err("the copy answered folded lines".into()),
    }
}

fn copies(svc: &Service) -> Vec<String> {
    match &svc.backend {
        Backend::Cluster(c) => c.replica_clients().into_iter().map(|(_, url, _)| url).collect(),
        Backend::Local(_) => Vec::new(),
    }
}

/// Reads what the copies keep and takes the newest of each part
/// ([`crate::alerts::Alerts::take_kept`]; `starting`: before this
/// coordinator judged anything, the states whole); the audit entries
/// from before it started, it answers from then on. Whether a copy
/// answered.
pub async fn read(svc: &Arc<Service>, starting: bool) -> bool {
    use futures::StreamExt;
    let urls = copies(svc);
    if urls.is_empty() {
        return false;
    }
    let answers: Vec<(String, Result<Json, String>)> = futures::stream::iter(urls.into_iter().map(|url| async move {
        let r = call(svc, &url, json!({"op": "keep", "want": true, "by": "a coordinator starting"})).await;
        (url, r)
    }))
    .buffer_unordered(AT_ONCE)
    .collect()
    .await;
    let mut best: serde_json::Map<String, Json> = serde_json::Map::new();
    let mut audit: Vec<Json> = Vec::new();
    let mut seen = HashSet::new();
    let mut answered = 0;
    let name = me(svc);
    let now = now_ms();
    {
        let mut held = lock(&svc.kept.copies);
        for (url, a) in &answers {
            let st = held.entry(url.clone()).or_default();
            st.at_ms = now;
            let j = match a {
                Ok(j) => j,
                Err(e) => {
                    st.error = Some(e.clone());
                    continue;
                }
            };
            answered += 1;
            st.error = None;
            st.rules_ms = j["parts"]["rules"]["ms"].as_u64().unwrap_or(0);
            st.routes_ms = j["parts"]["routes"]["ms"].as_u64().unwrap_or(0);
            st.audit_ms = j["audit_ms"][name.as_str()].as_u64().unwrap_or(0);
            for (k, p) in j["data"].as_object().into_iter().flatten() {
                if best.get(k).is_none_or(|b| p["ms"].as_u64().unwrap_or(0) > b["ms"].as_u64().unwrap_or(0)) {
                    best.insert(k.clone(), p.clone());
                }
            }
            for e in j["audit"].as_array().into_iter().flatten() {
                if seen.insert(audit_key(e)) {
                    audit.push(e.clone());
                }
            }
        }
    }
    if answered == 0 {
        let why: Vec<String> = answers.iter().filter_map(|(u, a)| a.as_ref().err().map(|e| format!("{u}: {e}"))).collect();
        tracing::warn!(copies = answers.len(), errors = %why.join("; "), "no copy answered with what the shards keep of this coordinator's alert rules, routes and audit log: they are taken once one does");
        return false;
    }
    let took = match svc.alerts.take_kept(&Json::Object(best.clone()), starting) {
        Ok(t) => t,
        Err(e) => {
            tracing::error!(error = %e.message, "what the shards keep of the alert rules and routes does not read: this coordinator keeps its own");
            Vec::new()
        }
    };
    let started = svc.started.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    audit.retain(|e| e["ts_ms"].as_u64().unwrap_or(0) < started);
    audit.sort_by_key(|e| e["ts_ms"].as_u64().unwrap_or(0));
    let n_audit = audit.len();
    let first = !svc.kept.read.swap(true, Ordering::AcqRel);
    if first {
        *lock(&svc.kept.restored) = audit;
    }
    let custom = svc.alerts.rules().iter().filter(|r| !r.builtin && !crate::alerts::defaults().iter().any(|d| d.id == r.id)).count();
    let routes = svc.alerts.routes().len();
    let summary = json!({"took": took, "copies": answers.len(), "answered": answered, "audit": n_audit, "at_ms": now});
    if first {
        *lock(&svc.kept.took) = Some(summary);
        tracing::info!(took = ?took, copies = answers.len(), answered, rules_of_your_own = custom, routes, audit_entries = n_audit, "read what the shards keep of this coordinator's alert rules, routes, alert states and audit log");
    } else if !took.is_empty() {
        tracing::info!(took = ?took, "took newer alert rules, routes or states from the shards");
    }
    true
}

/// Gives each copy what it does not hold yet: the parts changed since it
/// last took them (all of them every [`ALL_EVERY_MS`]), and this
/// coordinator's audit entries newer than its newest. Answers `{copies,
/// took, failed: [{copy, error}]}` (`Null` on a server that is not a
/// coordinator, or one that has not read its shards yet and cannot).
pub async fn push(svc: &Arc<Service>) -> Json {
    use futures::StreamExt;
    if !matches!(svc.backend, Backend::Cluster(_)) {
        return Json::Null;
    }
    if !svc.kept.read.load(Ordering::Acquire) && !read(svc, false).await {
        status(svc, &[]);
        return json!({"copies": copies(svc).len(), "took": 0, "failed": [], "not_read": true});
    }
    let _one = svc.kept.pushing.lock().await;
    let (parts, stamp) = svc.alerts.kept_parts();
    let name = me(svc);
    // This coordinator's own entries (memory holds only its own), the
    // oldest first.
    let mut own: Vec<Json> = svc.audit.latest(AUDIT_KEPT).iter().rev().map(|e| {
        let mut j = json!(**e);
        j["kind"] = json!(e.kind());
        j["coordinator"] = json!(name);
        j
    }).collect();
    own.retain(|e| e["ts_ms"].is_u64());
    let now = now_ms();
    let urls = copies(svc);
    let plans: Vec<(String, Json, u64, u64)> = {
        let held = lock(&svc.kept.copies);
        urls.iter()
            .filter_map(|url| {
                let st = held.get(url).cloned().unwrap_or_default();
                let all = now.saturating_sub(st.all_ms) >= ALL_EVERY_MS || st.error.is_some();
                let mut give = serde_json::Map::new();
                if all || parts["rules"]["ms"].as_u64().unwrap_or(0) > st.rules_ms {
                    give.insert("rules".into(), parts["rules"].clone());
                }
                if all || parts["routes"]["ms"].as_u64().unwrap_or(0) > st.routes_ms {
                    give.insert("routes".into(), parts["routes"].clone());
                }
                if all || st.stamp != Some(stamp) {
                    give.insert("states".into(), parts["states"].clone());
                }
                let mut entries = Vec::new();
                let mut bytes = 0;
                let mut last = st.audit_ms;
                for e in own.iter().filter(|e| e["ts_ms"].as_u64().unwrap_or(0) > st.audit_ms) {
                    bytes += e.to_string().len();
                    if bytes > CALL_BYTES && !entries.is_empty() {
                        break;
                    }
                    last = e["ts_ms"].as_u64().unwrap_or(last);
                    entries.push(e.clone());
                }
                (!give.is_empty() || !entries.is_empty()).then(|| (url.clone(), json!({"op": "keep", "parts": give, "audit": entries, "by": name}), last, if all { now } else { st.all_ms }))
            })
            .collect()
    };
    let results: Vec<(String, Result<Json, String>, u64, u64)> = futures::stream::iter(plans.into_iter().map(|(url, body, last, all_ms)| async move {
        let r = call(svc, &url, body).await;
        (url, r, last, all_ms)
    }))
    .buffer_unordered(AT_ONCE)
    .collect()
    .await;
    let mut failed = Vec::new();
    let mut took = 0;
    {
        let mut held = lock(&svc.kept.copies);
        for (url, r, last, all_ms) in &results {
            let st = held.entry(url.clone()).or_default();
            st.at_ms = now;
            match r {
                Ok(j) => {
                    took += 1;
                    st.error = None;
                    st.rules_ms = j["parts"]["rules"]["ms"].as_u64().unwrap_or(0);
                    st.routes_ms = j["parts"]["routes"]["ms"].as_u64().unwrap_or(0);
                    st.stamp = Some(stamp);
                    st.audit_ms = j["audit_ms"][name.as_str()].as_u64().unwrap_or(*last).max(*last);
                    st.all_ms = *all_ms;
                }
                Err(e) => {
                    st.error = Some(e.clone());
                    failed.push(json!({"copy": url, "error": e}));
                }
            }
        }
        held.retain(|u, _| urls.contains(u));
    }
    if !failed.is_empty() {
        static LIMIT: crate::obs::RateLimit = crate::obs::RateLimit::new(1);
        if LIMIT.admit().is_some() {
            tracing::warn!(failed = failed.len(), of = results.len(), first = %failed[0]["error"], "copies did not keep this coordinator's alert rules, routes, states or audit log");
        }
    }
    status(svc, &urls);
    json!({"copies": urls.len(), "took": took, "failed": failed})
}

/// Records on the alerts where they are kept ([`crate::alerts::Alerts::set_held`]).
fn status(svc: &Service, urls: &[String]) {
    let held = lock(&svc.kept.copies);
    let (rules_ms, _) = svc.alerts.rules_version();
    let routes_ms = svc.alerts.routes_ms();
    let holding = urls.iter().filter(|u| held.get(*u).is_some_and(|s| s.error.is_none() && s.rules_ms >= rules_ms && s.routes_ms >= routes_ms)).count();
    let failed: Vec<Json> = urls.iter().filter_map(|u| held.get(u).and_then(|s| s.error.as_ref().map(|e| json!({"copy": u, "error": e, "at_ms": s.at_ms})))).collect();
    let at = urls.iter().filter_map(|u| held.get(u).filter(|s| s.error.is_none()).map(|s| s.at_ms)).max();
    svc.alerts.set_held(json!({
        "copies": urls.len(), "holding": holding, "failed": failed, "at_ms": at,
        "read": svc.kept.read.load(Ordering::Relaxed), "started_with": *lock(&svc.kept.took),
    }));
}

/// Gives the copies what changed now, for a request that changed the rules
/// or the routes: answered once they keep it ([`push`]).
pub async fn push_now(svc: &Arc<Service>) -> Json {
    push(svc).await
}

/// A coordinator that stops (SIGTERM) gives its copies what changed since
/// its last push: a few seconds at most.
pub async fn last(svc: &Arc<Service>) {
    if matches!(svc.backend, Backend::Cluster(_)) && svc.kept.read.load(Ordering::Acquire) {
        let _ = tokio::time::timeout(WAIT + Duration::from_secs(1), push(svc)).await;
    }
}

/// On a coordinator: reads what its shards keep before it serves (a few
/// seconds at most), then gives them what changes, every [`EVERY`] and a
/// second after an alert's state changes.
pub async fn start(svc: &Arc<Service>) {
    if !matches!(svc.backend, Backend::Cluster(_)) {
        return;
    }
    // Built-in rules are a coordinator's: they are taken as such.
    svc.alerts.set_coordinator(true);
    if read(svc, true).await {
        push(svc).await;
    }
    let weak = Arc::downgrade(svc);
    let (changed, woken) = (svc.alerts.changed.clone(), svc.kept.woken.clone());
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(EVERY) => {}
                _ = changed.notified() => tokio::time::sleep(Duration::from_secs(1)).await,
                _ = woken.notified() => tokio::time::sleep(Duration::from_secs(1)).await,
            }
            let Some(svc) = weak.upgrade() else { return };
            push(&svc).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(coord: &str, ts: u64, route: &str) -> Json {
        json!({"coordinator": coord, "ts_ms": ts, "method": "PUT", "route": route, "who": "root token", "status": 200})
    }

    /// The newest of each part wins, an entry is kept once, and the newest
    /// entries are kept.
    #[test]
    fn a_copy_keeps_the_newest_part_and_each_entry_once() {
        let mut held = vec![entry("c", 3, "/a"), entry("c", 1, "/b")];
        held.sort_by_key(|e| e["ts_ms"].as_u64().unwrap());
        assert_eq!(audit_key(&held[0]), audit_key(&entry("c", 1, "/b")));
        assert_ne!(audit_key(&entry("c", 1, "/b")), audit_key(&entry("d", 1, "/b")));
        let mut many: Vec<Json> = (0..AUDIT_KEPT as u64 + 10).map(|i| entry("c", i, "/x")).collect();
        trim_audit(&mut many);
        assert_eq!((many.len(), many[0]["ts_ms"].as_u64()), (AUDIT_KEPT, Some(10)));
        assert_eq!(file_of(Path::new("/d/db.chloedb")), PathBuf::from("/d/db.chloedb.coordinator.json"));
    }
}
