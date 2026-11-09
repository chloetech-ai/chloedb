//! Contain rules: an operator limits or refuses, for a while, a statement
//! ([`crate::telemetry::fingerprint`]), a key, or a statement from one key
//! (`POST /v1/admin/contain`).
//!
//! A rule applies when a query is admitted, before it is planned: `refuse`
//! answers 429 at once, and `limit` lets `concurrency` matching queries
//! run at once, the others waiting for a turn within their deadline and
//! then answering 429. A 429 names the rule and its reason. Rules end by
//! themselves (`expires_s`, 15 minutes by default, a day at most); every
//! rule added, removed or ended is audited and an event (`contain`).
//!
//! A cluster's coordinators hold the same rules (fleet): a rule added or
//! removed on one is sent to every other ([`crate::peers`]), with its id,
//! times and author, and the answer says which took it; a coordinator that
//! starts asks the others for theirs ([`sync`]); and with a file
//! (`--contain`, else `contain.json` beside `--alerts`) a coordinator keeps
//! its rules, and those removed by hand, across a restart.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value as Json};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::telemetry::now_ms;
use crate::ApiError;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The longest a rule holds: a day.
pub const MAX_EXPIRES_S: u64 = 86_400;
/// Rules held at once at most.
pub const MAX_RULES: usize = 100;

/// One rule.
pub struct Rule {
    pub id: String,
    /// The statement it matches (a fingerprint), if it names one.
    pub fingerprint: Option<String>,
    /// The key it matches (its name or id), if it names one.
    pub key: Option<String>,
    /// `limit` or `refuse`.
    pub action: &'static str,
    /// With `limit`: the matching queries that run at once.
    pub concurrency: u32,
    pub created_ms: u64,
    pub expires_ms: u64,
    pub by: String,
    pub reason: String,
    hits: AtomicU64,
    refused: AtomicU64,
    slots: Arc<Semaphore>,
}

impl Rule {
    fn matches(&self, fingerprint: &str, key: Option<&str>, key_id: Option<&str>) -> bool {
        self.fingerprint.as_deref().is_none_or(|f| f == fingerprint) && self.key.as_deref().is_none_or(|k| Some(k) == key || Some(k) == key_id)
    }

    pub fn json(&self) -> Json {
        let mut m = serde_json::Map::new();
        if let Some(f) = &self.fingerprint {
            m.insert("fingerprint".into(), json!(f));
        }
        if let Some(k) = &self.key {
            m.insert("key".into(), json!(k));
        }
        let mut j = json!({
            "id": self.id, "match": m, "action": self.action, "created_ms": self.created_ms, "expires_ms": self.expires_ms,
            "by": self.by, "reason": self.reason, "hits": self.hits.load(Ordering::Relaxed), "refused": self.refused.load(Ordering::Relaxed),
        });
        if self.action == "limit" {
            j["concurrency"] = json!(self.concurrency);
            j["running"] = json!(self.concurrency as usize - self.slots.available_permits().min(self.concurrency as usize));
        }
        j
    }

    /// The sentence an event and the audit log say of it.
    pub fn what(&self) -> String {
        let whom = match (&self.fingerprint, &self.key) {
            (Some(f), Some(k)) => format!("statement {} from key {k}", crate::obs::truncate(f, 120)),
            (Some(f), None) => format!("statement {}", crate::obs::truncate(f, 120)),
            (None, Some(k)) => format!("everything from key {k}"),
            (None, None) => "every query".into(),
        };
        match self.action {
            "refuse" => format!("refuses {whom}"),
            _ => format!("limits {whom} to {} at once", self.concurrency),
        }
    }

    fn refusal(&self) -> ApiError {
        let until = self.expires_ms.saturating_sub(now_ms()) / 1000;
        let why = if self.reason.is_empty() { String::new() } else { format!(": {}", self.reason) };
        let message = match self.action {
            "refuse" => format!("refused by contain rule {} ({}, for {} s more){why}", self.id, self.what(), until),
            _ => format!("refused by contain rule {}: {} are running ({}, for {} s more){why}", self.id, self.concurrency, self.what(), until),
        };
        ApiError { status: 429, message }
    }
}

/// The rules of a server.
#[derive(Default)]
pub struct Contain {
    rules: Mutex<Vec<Arc<Rule>>>,
    next: AtomicU64,
    /// Rules removed by hand, by id, until they would have ended: a sync
    /// from another coordinator does not bring them back.
    removed: Mutex<HashMap<String, u64>>,
    /// Where the rules are kept (`None`: in memory).
    file: Option<PathBuf>,
    /// The rules of the other coordinators were taken up ([`sync`]).
    synced: AtomicBool,
}

/// A rule's id: unique among a cluster's coordinators, which share rules.
fn new_id(n: u64) -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(n);
    h.write_u64(now_ms());
    format!("c{:06x}", h.finish() & 0xff_ffff)
}

/// A rule from what [`Rule::json`] says of it, or a request's body.
#[allow(clippy::too_many_arguments)]
fn build(b: &Json, by: &str, id: String, created_ms: u64, expires_ms: u64, reason: String) -> Result<Rule, ApiError> {
    let m = b.get("match").cloned().unwrap_or(Json::Null);
    let text = |k: &str| m.get(k).and_then(Json::as_str).map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    let (fingerprint, key) = (text("fingerprint"), text("key"));
    if fingerprint.is_none() && key.is_none() {
        return Err(ApiError::bad("a contain rule matches a statement, a key or both: the body needs \"match\": {\"fingerprint\"?, \"key\"?}"));
    }
    let action = match b.get("action").and_then(Json::as_str) {
        Some("limit") => "limit",
        Some("refuse") => "refuse",
        other => return Err(ApiError::bad(format!("action is \"limit\" or \"refuse\", not {other:?}"))),
    };
    let concurrency = match action {
        "limit" => b.get("concurrency").and_then(Json::as_u64).filter(|&c| c <= 10_000).ok_or_else(|| ApiError::bad("a limit needs \"concurrency\": the queries that run at once, 0 to 10000"))? as u32,
        _ => 0,
    };
    if reason.is_empty() {
        return Err(ApiError::bad("say why: the body needs a \"reason\", which the refused clients read"));
    }
    Ok(Rule {
        id,
        fingerprint,
        key,
        action,
        concurrency,
        created_ms,
        expires_ms,
        by: by.to_string(),
        reason: crate::obs::truncate(&reason, 500).into_owned(),
        hits: AtomicU64::new(0),
        refused: AtomicU64::new(0),
        slots: Arc::new(Semaphore::new(concurrency as usize)),
    })
}

/// What a query admitted holds while it runs: its turns of `limit` rules.
pub struct Admitted {
    _turns: Vec<OwnedSemaphorePermit>,
}

impl Contain {
    /// The rules kept in `file`, which is written as they change (it
    /// need not exist).
    pub fn open(file: PathBuf) -> Result<Contain, String> {
        let c = Contain { file: Some(file.clone()), ..Contain::default() };
        let b = match std::fs::read(&file) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(c),
            Err(e) => return Err(format!("--contain {}: {e}", file.display())),
        };
        let j: Json = serde_json::from_slice(&b).map_err(|e| format!("--contain {}: {e}", file.display()))?;
        let t = now_ms();
        let mut rules = Vec::new();
        for r in j["rules"].as_array().map_or(&[][..], |v| v.as_slice()) {
            match Self::of_json(r, t) {
                Ok(Some(r)) => rules.push(Arc::new(r)),
                Ok(None) => {}
                Err(e) => tracing::warn!(file = %file.display(), error = %e.message, "a contain rule kept is not one: left out"),
            }
        }
        if !rules.is_empty() {
            tracing::info!(rules = rules.len(), file = %file.display(), "contain rules kept from before the restart");
        }
        *lock(&c.rules) = rules;
        *lock(&c.removed) = j["removed"].as_object().map(|o| o.iter().filter_map(|(k, v)| v.as_u64().filter(|&u| u > t).map(|u| (k.clone(), u))).collect()).unwrap_or_default();
        Ok(c)
    }

    /// A rule from [`Rule::json`], `None` when it ended by `t`.
    fn of_json(j: &Json, t: u64) -> Result<Option<Rule>, ApiError> {
        let id = j["id"].as_str().filter(|s| !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')).ok_or_else(|| ApiError::bad("a rule from another coordinator has an id"))?;
        let expires_ms = j["expires_ms"].as_u64().ok_or_else(|| ApiError::bad("a rule from another coordinator has expires_ms"))?;
        if expires_ms <= t {
            return Ok(None);
        }
        let expires_ms = expires_ms.min(t + MAX_EXPIRES_S * 1000);
        let created_ms = j["created_ms"].as_u64().unwrap_or(t).min(t);
        let reason = j["reason"].as_str().map(str::trim).unwrap_or_default().to_string();
        build(j, j["by"].as_str().unwrap_or("another coordinator"), id.to_string(), created_ms, expires_ms, reason).map(Some)
    }

    /// Writes the rules and those removed to the file, if there is one.
    fn save(&self) {
        let Some(file) = &self.file else { return };
        let t = now_ms();
        let rules: Vec<Json> = lock(&self.rules).iter().filter(|r| r.expires_ms > t).map(|r| r.json()).collect();
        let removed: serde_json::Map<String, Json> = lock(&self.removed).iter().filter(|(_, &u)| u > t).map(|(k, &u)| (k.clone(), json!(u))).collect();
        let body = json!({"rules": rules, "removed": removed});
        let tmp = file.with_extension("json.tmp");
        let r = std::fs::write(&tmp, serde_json::to_vec_pretty(&body).unwrap_or_default()).and_then(|_| std::fs::rename(&tmp, file));
        if let Err(e) = r {
            tracing::warn!(file = %file.display(), error = %e, "cannot keep the contain rules: a restart loses them");
        }
    }

    /// Adds a rule from its request body; `by` names who.
    pub fn add(&self, b: &Json, by: &str) -> Result<Arc<Rule>, ApiError> {
        let expires_s = b.get("expires_s").and_then(Json::as_u64).unwrap_or(900);
        if expires_s == 0 || expires_s > MAX_EXPIRES_S {
            return Err(ApiError::bad(format!("expires_s is 1 to {MAX_EXPIRES_S} (a day)")));
        }
        let reason = b.get("reason").and_then(Json::as_str).map(str::trim).unwrap_or_default();
        let reason = crate::keys::caller().and_then(|c| c.reason).filter(|_| reason.is_empty()).unwrap_or_else(|| reason.to_string());
        let t = now_ms();
        let r = Arc::new(build(b, by, new_id(self.next.fetch_add(1, Ordering::Relaxed)), t, t + expires_s * 1000, reason)?);
        self.insert(r.clone())?;
        Ok(r)
    }

    fn insert(&self, r: Arc<Rule>) -> Result<(), ApiError> {
        self.push(r)?;
        self.save();
        Ok(())
    }

    /// Adds `r` to the rules in force, without saving.
    fn push(&self, r: Arc<Rule>) -> Result<(), ApiError> {
        let t = now_ms();
        let mut rules = lock(&self.rules);
        rules.retain(|r| r.expires_ms > t);
        if rules.len() >= MAX_RULES {
            return Err(ApiError { status: 409, message: format!("{MAX_RULES} contain rules hold already: remove one first") });
        }
        rules.push(r);
        Ok(())
    }

    /// Takes up a rule another coordinator holds ([`Rule::json`]), with
    /// its id, times and author: the rule, and whether it is new here.
    /// `None` when it ended, or was removed here.
    pub fn adopt(&self, j: &Json) -> Result<Option<(Arc<Rule>, bool)>, ApiError> {
        let t = now_ms();
        let Some(r) = Self::of_json(j, t)? else { return Ok(None) };
        // The removals stay locked until the rule is in, so a removal
        // that comes meanwhile (another coordinator's) is not undone.
        let removed = lock(&self.removed);
        if removed.contains_key(&r.id) {
            return Ok(None);
        }
        if let Some(had) = lock(&self.rules).iter().find(|h| h.id == r.id) {
            return Ok(Some((had.clone(), false)));
        }
        let r = Arc::new(r);
        self.push(r.clone())?;
        drop(removed);
        self.save();
        Ok(Some((r, true)))
    }

    /// Removes rule `id`.
    pub fn remove(&self, id: &str) -> Option<Arc<Rule>> {
        // Removals, then rules: the order adopt takes them in.
        let mut removed = lock(&self.removed);
        let mut rules = lock(&self.rules);
        let gone = rules.iter().position(|r| r.id == id).map(|i| rules.remove(i));
        drop(rules);
        // Kept as removed even when it is not here: a sync from another
        // coordinator that still holds it does not bring it back.
        if id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') && (gone.is_some() || removed.len() < 1000) {
            removed.insert(id.to_string(), gone.as_ref().map_or(now_ms() + MAX_EXPIRES_S * 1000, |r| r.expires_ms));
        }
        drop(removed);
        self.save();
        gone
    }

    /// This server's rules and those removed, for another coordinator's
    /// sync (`GET /v1/admin/contain?local=1`).
    pub fn local_json(&self) -> Json {
        let t = now_ms();
        let removed: serde_json::Map<String, Json> = lock(&self.removed).iter().filter(|(_, &u)| u > t).map(|(k, &u)| (k.clone(), json!(u))).collect();
        json!({"rules": self.list().iter().map(|r| r.json()).collect::<Vec<_>>(), "removed": removed})
    }

    /// The rules in force, the newest first.
    pub fn list(&self) -> Vec<Arc<Rule>> {
        let t = now_ms();
        let mut v: Vec<Arc<Rule>> = lock(&self.rules).iter().filter(|r| r.expires_ms > t).cloned().collect();
        v.reverse();
        v
    }

    /// Takes out the rules that ended, for their events.
    pub fn expired(&self) -> Vec<Arc<Rule>> {
        let t = now_ms();
        let mut rules = lock(&self.rules);
        let (gone, kept): (Vec<_>, Vec<_>) = rules.drain(..).partition(|r| r.expires_ms <= t);
        *rules = kept;
        drop(rules);
        lock(&self.removed).retain(|_, &mut u| u > t);
        if !gone.is_empty() {
            self.save();
        }
        gone
    }

    /// Whether any rule holds (a query without a rule to match pays
    /// nothing more).
    pub fn is_empty(&self) -> bool {
        lock(&self.rules).is_empty()
    }

    /// Admits a query of `fingerprint` from `key` (its name and id), or
    /// refuses it (429 naming the rule); a limited one waits for its turn
    /// until `wait` passes.
    pub async fn admit(&self, fingerprint: &str, key: Option<&str>, key_id: Option<&str>, wait: Duration) -> Result<Admitted, ApiError> {
        let t = now_ms();
        let matching: Vec<Arc<Rule>> = lock(&self.rules).iter().filter(|r| r.expires_ms > t && r.matches(fingerprint, key, key_id)).cloned().collect();
        let mut turns = Vec::new();
        for r in matching {
            r.hits.fetch_add(1, Ordering::Relaxed);
            if r.action == "refuse" || r.concurrency == 0 {
                r.refused.fetch_add(1, Ordering::Relaxed);
                return Err(r.refusal());
            }
            let wait = wait.min(Duration::from_millis(r.expires_ms.saturating_sub(now_ms())));
            match tokio::time::timeout(wait, r.slots.clone().acquire_owned()).await {
                Ok(Ok(p)) => turns.push(p),
                _ => {
                    r.refused.fetch_add(1, Ordering::Relaxed);
                    return Err(r.refusal());
                }
            }
        }
        Ok(Admitted { _turns: turns })
    }
}

/// The event of a rule added, removed or ended.
pub fn event(r: &Rule, what: &str, who: Option<String>) -> crate::events::Event {
    let title = match what {
        "added" => format!("Contain rule {} {} for {} min: {}", r.id, r.what(), r.expires_ms.saturating_sub(r.created_ms) / 60_000, r.reason),
        "removed" => format!("Contain rule {} removed: it {}", r.id, r.what()),
        _ => format!("Contain rule {} ended: it {}", r.id, r.what()),
    };
    crate::events::Event { kind: "contain".into(), severity: if r.action == "refuse" { "warning".into() } else { "info".into() }, title, detail: json!({"rule": r.json(), "change": what}), where_: Json::Null, who }
}

// ----- fleet: the rules of every coordinator ([`crate::peers`])

/// Sends rule `r`, just added here, to every other coordinator: which of
/// them took it (`[{url, name, this, ok, error?}]`).
pub async fn spread(svc: &crate::service::Service, r: &Rule) -> Json {
    let answers = crate::peers::post_all(svc, "/v1/admin/contain", &r.json()).await;
    crate::peers::reach(svc, &answers)
}

/// Removes rule `id` on every other coordinator: which answered, and
/// whether any of them held it.
pub async fn withdraw(svc: &crate::service::Service, id: &str, by: &str) -> (Json, bool) {
    let answers = crate::peers::delete_all(svc, &format!("/v1/admin/contain/{id}?by={}", crate::peers::encode(by))).await;
    let held = answers.iter().any(|(_, a)| a.is_ok());
    // A coordinator that did not hold it answered 404: it took the removal.
    let answers: Vec<_> = answers.into_iter().map(|(p, a)| (p, a.or_else(|e| if e.contains(": 404 ") { Ok(Json::Null) } else { Err(e) }))).collect();
    (crate::peers::reach(svc, &answers), held)
}

/// The rules of every coordinator (`GET /v1/admin/contain`), the newest
/// first: each with the coordinators that hold it (`on`), those that
/// answered without it (`missing`) and those that did not answer
/// (`unreachable`), and its hits and refusals summed over them.
pub async fn gathered(svc: &crate::service::Service) -> Json {
    let me = crate::peers::coordinators(svc).into_iter().find(|p| p.this).map_or_else(|| "this coordinator".to_string(), |p| p.name);
    let mut answers: Vec<(String, Result<Vec<Json>, String>)> = vec![(me, Ok(svc.contain.list().iter().map(|r| r.json()).collect()))];
    if matches!(svc.backend, crate::service::Backend::Cluster(_)) {
        for (p, a) in crate::peers::get_all(svc, "/v1/admin/contain?local=1").await {
            answers.push((p.name, a.map(|j| j["rules"].as_array().cloned().unwrap_or_default())));
        }
    }
    let mut order: Vec<String> = Vec::new();
    let mut by: HashMap<String, (Json, Vec<String>)> = HashMap::new();
    for (name, a) in &answers {
        for r in a.as_ref().map_or(&[][..], |v| v.as_slice()) {
            let Some(id) = r["id"].as_str() else { continue };
            match by.get_mut(id) {
                Some((j, on)) => {
                    for k in ["hits", "refused", "running"] {
                        if let (Some(a), Some(b)) = (j[k].as_u64(), r[k].as_u64()) {
                            j[k] = json!(a + b);
                        }
                    }
                    on.push(name.clone());
                }
                None => {
                    order.push(id.to_string());
                    by.insert(id.to_string(), (r.clone(), vec![name.clone()]));
                }
            }
        }
    }
    let unreachable: Vec<&String> = answers.iter().filter(|(_, a)| a.is_err()).map(|(n, _)| n).collect();
    let mut out: Vec<Json> = order
        .into_iter()
        .filter_map(|id| by.remove(&id))
        .map(|(mut j, on)| {
            let missing: Vec<&String> = answers.iter().filter(|(n, a)| a.is_ok() && !on.contains(n)).map(|(n, _)| n).collect();
            j["on"] = json!(on);
            j["missing"] = json!(missing);
            j["unreachable"] = json!(unreachable);
            j
        })
        .collect();
    out.sort_by(|a, b| b["created_ms"].as_u64().cmp(&a["created_ms"].as_u64()));
    Json::Array(out)
}

/// Takes up rule `j` sent by another coordinator (`from`): an event here
/// when it is new, so that this coordinator's Timeline says it too.
pub fn adopted(svc: &crate::service::Service, j: &Json, from: &str) -> Result<Option<(Arc<Rule>, bool)>, ApiError> {
    let Some((r, new)) = svc.contain.adopt(j)? else { return Ok(None) };
    // A coordinator by its name when it is known, else its URL.
    let from = crate::peers::coordinators(svc).into_iter().find(|p| p.url == from.trim_end_matches('/')).map_or_else(|| from.to_string(), |p| p.name);
    let from = from.as_str();
    if new {
        tracing::info!(rule = %r.id, by = %r.by, from, "contain rule taken up from another coordinator: it {}", r.what());
        let mut e = event(&r, "added", Some(r.by.clone()));
        e.title = format!("{} (set on {from})", e.title);
        e.detail["from"] = json!(from);
        svc.events.add(e);
    }
    Ok(Some((r, new)))
}

/// A coordinator that started takes up the rules the other coordinators
/// hold, and drops those they removed, once one of them answers; alone
/// for its first minute, it has nothing to take up.
pub async fn sync(svc: &crate::service::Service) {
    if svc.contain.synced.load(Ordering::Relaxed) {
        return;
    }
    let others = crate::peers::others(svc);
    if others.is_empty() {
        if svc.started.elapsed().is_ok_and(|d| d > Duration::from_secs(60)) {
            svc.contain.synced.store(true, Ordering::Relaxed);
        }
        return;
    }
    let mut taken = 0;
    let mut any = false;
    for (p, a) in crate::peers::get_all(svc, "/v1/admin/contain?local=1").await {
        let Ok(j) = a else { continue };
        any = true;
        for (id, until) in j["removed"].as_object().into_iter().flatten() {
            if let Some(r) = svc.contain.remove(id).filter(|_| until.as_u64().is_some()) {
                svc.events.add(event(&r, "removed", Some(format!("{} (removed while this coordinator was down)", p.name))));
            }
        }
        for r in j["rules"].as_array().map_or(&[][..], |v| v.as_slice()) {
            if let Ok(Some((_, true))) = adopted(svc, r, &p.name) {
                taken += 1;
            }
        }
    }
    if any {
        svc.contain.synced.store(true, Ordering::Relaxed);
        tracing::info!(rules = taken, "contain rules of the other coordinators taken up");
    }
}

/// Ends the rules whose time passed, each an event and an audit entry:
/// by `system`, with the rule before and the reason `expired` (the event
/// says it on the Timeline, so the entry is not a change event too).
pub fn sweep(svc: &crate::service::Service) {
    for r in svc.contain.expired() {
        tracing::info!(rule = %r.id, "contain rule ended: it {}", r.what());
        svc.events.add(event(&r, "ended", None));
        let entry = crate::keys::Entry {
            ts_ms: now_ms(),
            who: "system".into(),
            method: "EXPIRE".into(),
            route: "/v1/admin/contain/:id".into(),
            status: 200,
            detail: Some(format!("contain rule {} ended: it {} (set by {}, its time over)", r.id, r.what(), r.by)),
            kind: crate::keys::CHANGE.into(),
            reason: Some("expired".into()),
            object: Some(format!("contain rule {}", r.id)),
            before: Some(r.json()),
            actor: "server".into(),
            ..Default::default()
        };
        svc.audit_keep(entry, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rules_limit_refuse_and_match() {
        let c = Contain::default();
        assert!(c.add(&json!({"match": {}, "action": "refuse", "reason": "x"}), "ops").is_err());
        assert!(c.add(&json!({"match": {"key": "k"}, "action": "refuse"}), "ops").is_err());
        let r = c.add(&json!({"match": {"fingerprint": "SELECT ?", "key": "batch"}, "action": "limit", "concurrency": 1, "reason": "slow"}), "ops").unwrap();
        let w = Duration::from_millis(50);
        let one = c.admit("SELECT ?", Some("batch"), None, w).await.unwrap();
        let e = c.admit("SELECT ?", Some("batch"), None, w).await.err().unwrap();
        assert_eq!(e.status, 429);
        assert!(e.message.contains(&r.id) && e.message.contains("slow"), "{}", e.message);
        // Other keys, other statements: not matched.
        assert!(c.admit("SELECT ?", Some("app"), None, w).await.is_ok());
        assert!(c.admit("SELECT ? FROM t", Some("batch"), None, w).await.is_ok());
        drop(one);
        assert!(c.admit("SELECT ?", Some("batch"), None, w).await.is_ok());
        assert_eq!(r.json()["hits"], 3);
        assert_eq!(r.json()["refused"], 1);
        assert!(c.remove(&r.id).is_some());
        assert!(c.is_empty());
    }

    #[test]
    fn ids_differ_between_coordinators() {
        let (a, b) = (Contain::default(), Contain::default());
        let body = json!({"match": {"key": "batch"}, "action": "refuse", "reason": "x"});
        let ids: std::collections::HashSet<String> = (0..20).flat_map(|_| [a.add(&body, "ops").unwrap().id.clone(), b.add(&body, "ops").unwrap().id.clone()]).collect();
        assert_eq!(ids.len(), 40, "{ids:?}");
        assert!(ids.iter().all(|i| i.len() == 7 && i.starts_with('c')));
    }

    #[test]
    fn a_rule_taken_up_keeps_its_id_times_and_author_and_one_removed_stays_removed() {
        let (a, b) = (Contain::default(), Contain::default());
        let r = a.add(&json!({"match": {"fingerprint": "SELECT ?"}, "action": "limit", "concurrency": 2, "reason": "slow", "expires_s": 60}), "ops").unwrap();
        let (got, new) = b.adopt(&r.json()).unwrap().unwrap();
        assert!(new);
        assert_eq!((got.id.as_str(), got.created_ms, got.expires_ms, got.by.as_str(), got.concurrency), (r.id.as_str(), r.created_ms, r.expires_ms, "ops", 2));
        // Sent twice: held once.
        assert!(!b.adopt(&r.json()).unwrap().unwrap().1);
        assert_eq!(b.list().len(), 1);
        // Removed on b, then sent again by a coordinator that still holds it: not back.
        assert!(b.remove(&r.id).is_some());
        assert!(b.adopt(&r.json()).unwrap().is_none());
        assert!(b.local_json()["removed"][r.id.as_str()].as_u64().is_some());
        // A rule that ended is not taken up; an id that is not one is refused.
        let mut ended = r.json();
        ended["id"] = json!("c000001");
        ended["expires_ms"] = json!(now_ms() - 1);
        assert!(b.adopt(&ended).unwrap().is_none());
        let mut bad = r.json();
        bad["id"] = json!("../x");
        assert!(b.adopt(&bad).is_err());
    }

    #[test]
    fn rules_and_removals_survive_a_restart_in_the_file() {
        let dir = std::env::temp_dir().join(format!("strata-contain-{}-{}", std::process::id(), now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("contain.json");
        let c = Contain::open(f.clone()).unwrap();
        assert!(c.is_empty(), "no file yet: no rules");
        let kept = c.add(&json!({"match": {"key": "batch"}, "action": "refuse", "reason": "runaway", "expires_s": 600}), "ops").unwrap();
        let gone = c.add(&json!({"match": {"key": "app"}, "action": "refuse", "reason": "x"}), "ops").unwrap();
        c.remove(&gone.id);
        drop(c);
        let c = Contain::open(f.clone()).unwrap();
        let rules = c.list();
        assert_eq!(rules.len(), 1);
        assert_eq!((rules[0].id.as_str(), rules[0].expires_ms, rules[0].reason.as_str()), (kept.id.as_str(), kept.expires_ms, "runaway"));
        assert!(c.adopt(&gone.json()).unwrap().is_none(), "removed before the restart: still removed");
        std::fs::write(&f, b"not json").unwrap();
        assert!(Contain::open(f.clone()).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
