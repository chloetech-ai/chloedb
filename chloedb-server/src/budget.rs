//! A budget of model calls for each API key, a day (audit D-B13).
//!
//! Every route that asks a hosted model with the server's own credential
//! (`ANTHROPIC_API_KEY`, or a model's `endpoint.json`) spends the server
//! owner's money, and a key that may call it could otherwise spend without
//! end: `/v1/agent` a call a step, `/v1/judge` a call a batch of rows, the
//! on-call agent a call a step, and a query's `llm()` a call a question it
//! asks (answers from memory are free). Each is counted here, where the
//! request is sent, against the key that asked for it, up to
//! `--model-calls-per-key-day` calls in a day (UTC); past it the route
//! answers 429 until the next day. A caller that brings its own API key
//! (`api_key` in the body) spends its own, and is not counted; neither are
//! the root credential, the cluster's token and `--token`, which are the
//! operator's own. The counts are this process's, in memory: a restart, or
//! a second coordinator, starts its own.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde_json::{json, Value as Json};

use crate::keys::Principal;
use crate::ApiError;

/// The calls a key may make a day by default (`--model-calls-per-key-day`).
pub const DEFAULT_PER_DAY: u64 = 10_000;

const DAY_MS: u64 = 86_400_000;

/// The model calls each key made today, and how many it may.
pub struct ModelBudget {
    /// Calls a key may make a day; 0: no limit.
    per_day: AtomicU64,
    used: Mutex<HashMap<String, (u64, u64)>>,
}

impl Default for ModelBudget {
    fn default() -> ModelBudget {
        ModelBudget::new(DEFAULT_PER_DAY)
    }
}

impl ModelBudget {
    pub fn new(per_day: u64) -> ModelBudget {
        ModelBudget { per_day: AtomicU64::new(per_day), used: Mutex::new(HashMap::new()) }
    }

    pub fn set_per_day(&self, n: u64) {
        self.per_day.store(n, Ordering::Relaxed);
    }

    pub fn per_day(&self) -> u64 {
        self.per_day.load(Ordering::Relaxed)
    }

    /// Which budget `who`'s calls take from: a key's, by its id; `None` for
    /// a credential that is not counted.
    fn account(who: &Principal) -> Option<String> {
        if who.root {
            return None;
        }
        who.key.clone()
    }

    /// Takes `n` calls of `who`'s budget for today, for `what` (a route, or
    /// `llm()`), or refuses (429) when it has fewer left.
    pub fn take(&self, who: Option<&Principal>, n: u64, what: &str) -> Result<(), ApiError> {
        self.take_at(who, n, what, crate::telemetry::now_ms())
    }

    fn take_at(&self, who: Option<&Principal>, n: u64, what: &str, now_ms: u64) -> Result<(), ApiError> {
        let max = self.per_day();
        let Some(who) = who else { return Ok(()) };
        let Some(account) = Self::account(who) else { return Ok(()) };
        if max == 0 || n == 0 {
            return Ok(());
        }
        let day = now_ms / DAY_MS;
        let mut used = self.used.lock().unwrap_or_else(|e| e.into_inner());
        // Yesterday's counts are of no use.
        if used.len() > 1024 {
            used.retain(|_, (d, _)| *d == day);
        }
        let e = used.entry(account).or_insert((day, 0));
        if e.0 != day {
            *e = (day, 0);
        }
        if e.1 + n > max {
            let left = max.saturating_sub(e.1);
            let hours = ((day + 1) * DAY_MS - now_ms).div_ceil(3_600_000);
            return Err(ApiError {
                code: None,
                status: 429,
                message: format!("{what} would ask a model {n} time(s), and the key {:?} has {left} of its {max} model calls a day left (--model-calls-per-key-day); the count starts again in {hours} h (UTC midnight), or pass your own api_key", who.name),
            });
        }
        e.1 += n;
        Ok(())
    }

    /// Today's counts, by key id, for `GET /v1/admin/model-budget`.
    pub fn json(&self) -> Json {
        let day = crate::telemetry::now_ms() / DAY_MS;
        let used = self.used.lock().unwrap_or_else(|e| e.into_inner());
        let keys: serde_json::Map<String, Json> = used.iter().filter(|(_, (d, _))| *d == day).map(|(k, (_, n))| (k.clone(), json!(n))).collect();
        json!({"per_day": self.per_day(), "used": keys})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::Scope;

    fn key(id: &str) -> Principal {
        Principal { name: format!("app-{id}"), key: Some(id.into()), scope: Scope::Read, root: false, tables: None, role: None, agent: false, on_behalf_of: None, allow: None }
    }

    #[test]
    fn a_key_spends_its_day_and_starts_again_the_next() {
        let b = ModelBudget::new(10);
        let (a, other) = (key("a"), key("b"));
        let t = 5 * DAY_MS + 1000;
        b.take_at(Some(&a), 6, "llm()", t).unwrap();
        b.take_at(Some(&a), 4, "llm()", t).unwrap();
        let e = b.take_at(Some(&a), 1, "/v1/agent", t).unwrap_err();
        assert_eq!(e.status, 429);
        assert!(e.message.contains("0 of its 10"), "{}", e.message);
        b.take_at(Some(&other), 10, "llm()", t).expect("each key its own budget");
        b.take_at(Some(&a), 10, "llm()", t + DAY_MS).expect("a new day");
    }

    #[test]
    fn the_operators_credentials_and_no_limit_are_not_counted() {
        let b = ModelBudget::new(1);
        for who in [Principal::open(), Principal::cluster(), Principal { name: "token".into(), key: None, scope: Scope::Admin, root: false, tables: None, role: None, agent: false, on_behalf_of: None, allow: None }] {
            b.take(Some(&who), 100, "llm()").unwrap();
        }
        b.take(None, 100, "llm()").unwrap();
        let off = ModelBudget::new(0);
        off.take(Some(&key("a")), 1_000_000, "llm()").unwrap();
    }
}
