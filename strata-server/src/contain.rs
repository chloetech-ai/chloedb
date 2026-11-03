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
//! Rules are this server's, in memory: a coordinator contains what its
//! clients send it.

use std::sync::atomic::{AtomicU64, Ordering};
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
}

/// What a query admitted holds while it runs: its turns of `limit` rules.
pub struct Admitted {
    _turns: Vec<OwnedSemaphorePermit>,
}

impl Contain {
    /// Adds a rule from its request body; `by` names who.
    pub fn add(&self, b: &Json, by: &str) -> Result<Arc<Rule>, ApiError> {
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
        let expires_s = b.get("expires_s").and_then(Json::as_u64).unwrap_or(900);
        if expires_s == 0 || expires_s > MAX_EXPIRES_S {
            return Err(ApiError::bad(format!("expires_s is 1 to {MAX_EXPIRES_S} (a day)")));
        }
        let reason = b.get("reason").and_then(Json::as_str).map(str::trim).unwrap_or_default();
        let reason = crate::keys::caller().and_then(|c| c.reason).filter(|_| reason.is_empty()).unwrap_or_else(|| reason.to_string());
        if reason.is_empty() {
            return Err(ApiError::bad("say why: the body needs a \"reason\", which the refused clients read"));
        }
        let t = now_ms();
        let mut rules = lock(&self.rules);
        rules.retain(|r| r.expires_ms > t);
        if rules.len() >= MAX_RULES {
            return Err(ApiError { status: 409, message: format!("{MAX_RULES} contain rules hold already: remove one first") });
        }
        let r = Arc::new(Rule {
            id: format!("c{}", self.next.fetch_add(1, Ordering::Relaxed) + 1),
            fingerprint,
            key,
            action,
            concurrency,
            created_ms: t,
            expires_ms: t + expires_s * 1000,
            by: by.to_string(),
            reason: crate::obs::truncate(&reason, 500).into_owned(),
            hits: AtomicU64::new(0),
            refused: AtomicU64::new(0),
            slots: Arc::new(Semaphore::new(concurrency as usize)),
        });
        rules.push(r.clone());
        Ok(r)
    }

    /// Removes rule `id`.
    pub fn remove(&self, id: &str) -> Option<Arc<Rule>> {
        let mut rules = lock(&self.rules);
        let i = rules.iter().position(|r| r.id == id)?;
        Some(rules.remove(i))
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
}
